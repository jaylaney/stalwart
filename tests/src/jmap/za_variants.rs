/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account variants of the JMAP sub-tests (mail spec 11), run with
//! `ZA_KEY_ACCOUNTS=1`: every user is a key account, so the steps that query,
//! sort or snippet sealed content (spec 8), copy across accounts or share a
//! key account's mailbox (6.3), read a sealed source raw (5.7, rulings R31
//! and R32), expect threads (5.4) or push content (9) assert the refusal or
//! the designed outcome, and every other step runs as upstream runs it. The
//! admin and the group `sales@example.com` stay plain. JMAP calendars are not
//! offered to key accounts (release 1, spec 9), so the calendar modules are
//! one refusal variant.

use crate::{
    jmap::{
        files,
        mail::{
            acl::assert_forbidden,
            changes::{LogAction, ParseState},
            query::create as create_query_corpus,
            query_changes::QueryChanges,
            thread_merge::{build_messages, test_1, test_2, test_3},
        },
    },
    utils::{
        account::Account,
        jmap::{ChangeType, JmapResponse, JmapUtils},
        push::spawn_mock_push_server,
        server::{DestroyAllMailboxes, TestServer},
        smtp::SmtpConnection,
    },
};
use ::email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, Mailbox, TRASH_ID},
    message::{
        ingest::{EmailIngest, IngestEmail, IngestSource},
        metadata::MessageData,
    },
    push::{EmailPush, Urgency},
};
use ahash::{AHashMap, AHashSet};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{
    auth::AccessToken,
    storage::index::ObjectIndexBuilder,
    za::{ZA_MAIL_KEYLESS, ZA_MAIL_SEARCH_REFUSED},
};
use hyper::StatusCode;
use jmap_client::{
    Error,
    core::{
        error::{MethodError, MethodErrorType},
        set::SetErrorType,
    },
    email::{self, Property},
    mailbox::{self, Role},
    principal::ACL,
    push_subscription::Keys,
};
use jmap_proto::{
    method::query::Filter as ProtoFilter,
    object::{
        email::EmailFilter, file_node::FileNodeProperty, principal::PrincipalProperty,
        push_subscription::EmailPushProperty,
    },
    request::method::MethodObject,
    types::state::State,
};
use mail_parser::{DateTime, MessageParser, mailbox::mbox::MessageIterator};
use registry::schema::prelude::ObjectType;
use serde_json::{Value, json};
use services::state_manager::email_push::build_email_push_object;
use std::{
    collections::BTreeSet, fs, io::Cursor, path::PathBuf, str::FromStr, sync::atomic::Ordering,
    time::Duration,
};
use store::{
    ValueKey,
    rand::{self, RngExt},
    write::{AlignedBytes, Archive, BatchBuilder},
};
use tokio::sync::mpsc;
use types::{
    collection::{Collection, SyncCollection},
    id::Id,
    keyword::Keyword,
    special_use::SpecialUse,
};

/// Mail spec 8: method response `n` is the error `kind` (`unsupportedFilter`
/// or `unsupportedSort`) with the fixed text, naming `property` when given.
fn assert_search_refused(
    reply: &JmapResponse,
    n: usize,
    kind: &str,
    property: Option<&str>,
    context: &str,
) {
    let response = &reply.0["methodResponses"][n];
    assert_eq!(response[0], "error", "{context}: {response}");
    assert_eq!(response[1]["type"], kind, "{context}: {response}");
    let description = response[1]["description"].as_str().unwrap_or_default();
    match property {
        Some(property) => assert_eq!(
            description,
            format!("{property}: {ZA_MAIL_SEARCH_REFUSED}"),
            "{context}: {response}"
        ),
        None => assert!(
            description.ends_with(ZA_MAIL_SEARCH_REFUSED),
            "{context}: {response}"
        ),
    }
}

/// Mail spec 4, 6.3: the method call answered the keyless refusal (R22).
fn assert_keyless(response: &Value, context: &str) {
    assert_eq!(response[0], "error", "{context}: {response}");
    assert_eq!(
        response[1],
        json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS }),
        "{context}: {response}"
    );
}

/// A jmap-client result that is the keyless refusal, as a method or a set
/// error.
fn assert_keyless_result<T: std::fmt::Debug>(result: Result<T, Error>, context: &str) {
    match result {
        Err(Error::Set(err))
            if err.type_ == SetErrorType::Forbidden
                && err.description() == Some(ZA_MAIL_KEYLESS) => {}
        Err(Error::Method(MethodError {
            p_type: MethodErrorType::Forbidden,
        })) => {}
        other => panic!("{context}: expected the keyless refusal, got {other:?}"),
    }
}

/// The account owning document id `account_id`.
fn account_by_id(test: &TestServer, account_id: u32) -> &Account {
    test.accounts
        .values()
        .find(|account| account.id().document_id() == account_id)
        .unwrap_or_else(|| panic!("no test account with id {account_id}"))
}

fn date(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp).to_rfc3339()
}

/// One message of the query corpus, as Email/get reads it with the owner's
/// keys.
struct CorpusMessage {
    received_at: i64,
    size: u64,
    keywords: AHashSet<String>,
    mailboxes: AHashSet<String>,
    thread_id: String,
}

/// Every message of the account, by JMAP id.
async fn load_corpus(account: &Account) -> AHashMap<String, CorpusMessage> {
    let ids = account
        .jmap_method_call(
            "Email/query",
            json!({ "accountId": account.id_string(), "sort": [{ "property": "receivedAt" }] }),
        )
        .await
        .ids()
        .map(String::from)
        .collect::<Vec<_>>();
    let mut corpus = AHashMap::default();
    for chunk in ids.chunks(100) {
        let reply = account
            .jmap_method_call(
                "Email/get",
                json!({
                    "accountId": account.id_string(),
                    "ids": chunk,
                    "properties": ["id", "receivedAt", "size", "keywords", "mailboxIds", "threadId"],
                }),
            )
            .await;
        for item in reply.list() {
            let keys = |field: &str| {
                item[field]
                    .as_object()
                    .map(|map| map.keys().cloned().collect::<AHashSet<_>>())
                    .unwrap_or_default()
            };
            corpus.insert(
                item.text_field("id").to_string(),
                CorpusMessage {
                    received_at: DateTime::parse_rfc3339(item.text_field("receivedAt"))
                        .unwrap()
                        .to_timestamp(),
                    size: item["size"].as_u64().unwrap(),
                    keywords: keys("keywords"),
                    mailboxes: keys("mailboxIds"),
                    thread_id: item.text_field("threadId").to_string(),
                },
            );
        }
    }
    assert_eq!(corpus.len(), ids.len());
    corpus
}

/// `Email/query` of `filter` (or none) sorted by `sort`, with
/// `collapseThreads` and any extra arguments.
async fn email_query(
    account: &Account,
    filter: Option<Value>,
    sort: Value,
    extra: Value,
) -> JmapResponse {
    let mut args = json!({ "accountId": account.id_string(), "sort": sort });
    if let Some(filter) = filter {
        args["filter"] = filter;
    }
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            args[key] = value;
        }
    }
    account.jmap_method_call("Email/query", args).await
}

fn ids(reply: &JmapResponse, context: &str) -> Vec<String> {
    assert_eq!(reply.name_at(0), "Email/query", "{context}: {reply:?}");
    reply.ids().map(String::from).collect()
}

/// `mail::query::test` in key mode, against upstream's imported corpus of
/// 978 messages (sealed). Every content filter and sort answers
/// `unsupportedFilter` or `unsupportedSort` (spec 8), upstream's cases among
/// them. Each served filter is checked against the corpus as Email/get
/// reports it, in `receivedAt` order; a thread-keyword filter or sort answers
/// what the message's own keyword answers, threads being singletons (5.4);
/// and upstream's paging options run on a `receivedAt` sort.
pub async fn query(test: &TestServer) {
    println!("Running key-account Email Query tests...");
    let server = test.server.clone();
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;
    let account_id = account.id().document_id();

    if test.is_reset() {
        // Add some "virtual" mailbox ids so create doesn't fail
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Mailbox);
        for mailbox_id in 1545..3010 {
            batch
                .with_document(mailbox_id)
                .custom(ObjectIndexBuilder::<(), _>::new().with_changes(Mailbox {
                    name: format!("Mailbox {mailbox_id}"),
                    role: SpecialUse::None,
                    parent_id: 0,
                    sort_order: None,
                    uid_validity: 0,
                    subscribers: vec![],
                    acls: vec![],
                }))
                .unwrap();
        }
        server
            .core
            .storage
            .data
            .write(batch.build_all())
            .await
            .unwrap();

        println!("Inserting JMAP Mail query test messages...");
        create_query_corpus(test, account).await;

        // Spec 5.4: one thread per message, where upstream finds 100.
        let cached = test.server.get_cached_messages(account_id).await.unwrap();
        assert_eq!(
            cached
                .emails
                .items
                .iter()
                .map(|m| m.thread_id)
                .collect::<AHashSet<_>>()
                .len(),
            cached.emails.items.len()
        );
        test.wait_for_tasks().await;
    }
    let corpus = load_corpus(account).await;
    assert_eq!(
        corpus
            .values()
            .map(|m| m.thread_id.as_str())
            .collect::<AHashSet<_>>()
            .len(),
        corpus.len(),
        "singleton threads"
    );

    query_refusals(account).await;
    query_served(account, &corpus).await;
    query_options(account, &corpus).await;

    println!("Deleting all messages...");
    let mut request = client.build();
    let result_ref = request.query_email().result_reference();
    request.set_email().destroy_ref(result_ref);
    let response = request.send().await.unwrap();
    response
        .unwrap_method_responses()
        .pop()
        .unwrap()
        .unwrap_set_email()
        .unwrap();

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// Every content filter and sort, alone and in upstream's combinations.
async fn query_refusals(account: &Account) {
    let and = |conditions: Vec<Value>| json!({ "operator": "AND", "conditions": conditions });
    let received = json!([{ "property": "receivedAt" }]);
    let mut filters = vec![
        ("from", json!({ "from": "george" })),
        ("to", json!({ "to": "jdoe" })),
        ("cc", json!({ "cc": "canvas" })),
        ("bcc", json!({ "bcc": "x" })),
        ("subject", json!({ "subject": "Year" })),
        ("body", json!({ "body": "bequeathed" })),
        ("text", json!({ "text": "study" })),
        ("header", json!({ "header": ["Comments", "attributed"] })),
        ("hasAttachment", json!({ "hasAttachment": true })),
        ("sentBefore", json!({ "sentBefore": date(2_000_000_000) })),
        ("sentAfter", json!({ "sentAfter": date(0) })),
    ];
    // Upstream's combinations: a served condition beside a refused one.
    filters.extend([
        (
            "from",
            and(vec![
                json!({ "after": date(1850) }),
                json!({ "from": "george" }),
            ]),
        ),
        (
            "cc",
            and(vec![
                json!({ "inMailbox": Id::new(1768u64).to_string() }),
                json!({ "cc": "canvas" }),
            ]),
        ),
        (
            "from",
            and(vec![
                json!({ "hasKeyword": "N0" }),
                json!({ "operator": "NOT", "conditions": [{ "from": "collins" }] }),
                json!({ "body": "bequeathed" }),
            ]),
        ),
        (
            "text",
            and(vec![
                json!({ "after": date(1970) }),
                json!({ "before": date(1972) }),
                json!({ "text": "colour" }),
            ]),
        ),
        ("text", and(vec![json!({ "text": "'cats and dogs'" })])),
    ]);
    for (property, filter) in filters {
        let context = format!("filter {filter}");
        let reply = email_query(account, Some(filter), received.clone(), json!({})).await;
        assert_search_refused(&reply, 0, "unsupportedFilter", Some(property), &context);
    }
    // A refused filter is named before a refused sort.
    let reply = email_query(
        account,
        Some(json!({ "from": "george" })),
        json!([{ "property": "subject" }]),
        json!({}),
    )
    .await;
    assert_search_refused(
        &reply,
        0,
        "unsupportedFilter",
        Some("from"),
        "filter and sort",
    );

    // Sorts, alone and in upstream's combinations with a served filter.
    for property in ["from", "to", "cc", "subject", "sentAt"] {
        let reply = email_query(
            account,
            None,
            json!([{ "property": property, "isAscending": true }]),
            json!({}),
        )
        .await;
        assert_search_refused(&reply, 0, "unsupportedSort", Some(property), property);
    }
    for (property, filter, sort) in [
        (
            "subject",
            json!({ "notKeyword": "artist" }),
            json!([{ "property": "subject" }, { "property": "sentAt" }]),
        ),
        (
            "from",
            json!({ "before": date(1800) }),
            json!([
                { "property": "allInThreadHaveKeyword", "keyword": "N" },
                { "property": "from" },
                { "property": "sentAt" },
            ]),
        ),
        (
            "from",
            and(vec![
                json!({ "allInThreadHaveKeyword": "N" }),
                json!({ "before": date(1800) }),
            ]),
            json!([{ "property": "from" }, { "property": "sentAt" }]),
        ),
    ] {
        let context = format!("sort {sort}");
        let reply = email_query(account, Some(filter), sort, json!({})).await;
        assert_search_refused(&reply, 0, "unsupportedSort", Some(property), &context);
    }
}

/// The served filters, each against the corpus, in `receivedAt` order, and
/// the served sorts.
async fn query_served(account: &Account, corpus: &AHashMap<String, CorpusMessage>) {
    type Matcher = Box<dyn Fn(&CorpusMessage) -> bool>;
    let and = |conditions: Vec<Value>| json!({ "operator": "AND", "conditions": conditions });
    let has = |keyword: &'static str| move |m: &CorpusMessage| m.keywords.contains(keyword);
    let mailbox_1768 = Id::new(1768u64).to_string();
    let others = [1991u64, 1870, 2011, 1951, 1902, 1808, 1963]
        .into_iter()
        .map(|id| Id::new(id).to_string())
        .collect::<Vec<_>>();
    let mut sizes = corpus.values().map(|m| m.size).collect::<Vec<_>>();
    sizes.sort_unstable();
    let median = sizes[sizes.len() / 2];
    let some_thread = corpus.values().next().unwrap().thread_id.clone();

    let cases: Vec<(&str, Value, Matcher)> = vec![
        (
            "after",
            json!({ "after": date(1850) }),
            Box::new(|m| m.received_at >= 1850),
        ),
        (
            "before",
            json!({ "before": date(1800) }),
            Box::new(|m| m.received_at < 1800),
        ),
        (
            "after and before",
            and(vec![
                json!({ "after": date(1970) }),
                json!({ "before": date(1972) }),
            ]),
            Box::new(|m| (1970..1972).contains(&m.received_at)),
        ),
        (
            "hasKeyword",
            json!({ "hasKeyword": "N0" }),
            Box::new(has("N0")),
        ),
        (
            "notKeyword",
            json!({ "notKeyword": "artist" }),
            Box::new(|m| !m.keywords.contains("artist")),
        ),
        (
            "NOT hasKeyword",
            json!({ "operator": "NOT", "conditions": [{ "hasKeyword": "N0" }] }),
            Box::new(|m| !m.keywords.contains("N0")),
        ),
        (
            "OR hasKeyword",
            json!({ "operator": "OR", "conditions": [
                { "hasKeyword": "Bronze" }, { "hasKeyword": "artist" }
            ] }),
            Box::new(|m| m.keywords.contains("Bronze") || m.keywords.contains("artist")),
        ),
        (
            "inMailbox",
            json!({ "inMailbox": &mailbox_1768 }),
            Box::new({
                let mailbox = mailbox_1768.clone();
                move |m| m.mailboxes.contains(&mailbox)
            }),
        ),
        (
            "inMailboxOtherThan",
            json!({ "inMailboxOtherThan": &others }),
            Box::new({
                let others = others.clone();
                move |m| m.mailboxes.iter().any(|id| !others.contains(id))
            }),
        ),
        (
            "allInThreadHaveKeyword",
            and(vec![
                json!({ "allInThreadHaveKeyword": "N" }),
                json!({ "before": date(1800) }),
            ]),
            Box::new(|m| m.keywords.contains("N") && m.received_at < 1800),
        ),
        (
            "noneInThreadHaveKeyword",
            and(vec![
                json!({ "noneInThreadHaveKeyword": "N" }),
                json!({ "after": date(1995) }),
            ]),
            Box::new(|m| !m.keywords.contains("N") && m.received_at >= 1995),
        ),
        (
            "someInThreadHaveKeyword",
            and(vec![
                json!({ "someInThreadHaveKeyword": "Bronze" }),
                json!({ "before": date(1878) }),
            ]),
            Box::new(|m| m.keywords.contains("Bronze") && m.received_at < 1878),
        ),
        (
            "hasKeyword in a range",
            and(vec![
                json!({ "after": date(1786) }),
                json!({ "before": date(1840) }),
                json!({ "hasKeyword": "T" }),
            ]),
            Box::new(|m| m.keywords.contains("T") && (1786..1840).contains(&m.received_at)),
        ),
        (
            "minSize",
            json!({ "minSize": median }),
            Box::new(move |m| m.size >= median),
        ),
        (
            "maxSize",
            json!({ "maxSize": median }),
            Box::new(move |m| m.size < median),
        ),
        (
            "inThread",
            json!({ "inThread": &some_thread }),
            Box::new({
                let thread = some_thread.clone();
                move |m| m.thread_id == thread
            }),
        ),
    ];
    for (name, filter, matches) in cases {
        let mut expected = corpus
            .iter()
            .filter(|(_, m)| matches(m))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        expected.sort_unstable();
        assert!(!expected.is_empty(), "{name}: the corpus has no match");
        for ascending in [true, false] {
            for collapse in [false, true] {
                let reply = email_query(
                    account,
                    Some(filter.clone()),
                    json!([{ "property": "receivedAt", "isAscending": ascending }]),
                    json!({ "collapseThreads": collapse }),
                )
                .await;
                let found = ids(&reply, name);
                let order = found
                    .iter()
                    .map(|id| corpus[id].received_at)
                    .collect::<Vec<_>>();
                assert!(
                    order.windows(2).all(|w| if ascending {
                        w[0] <= w[1]
                    } else {
                        w[0] >= w[1]
                    }),
                    "{name}: not in receivedAt order"
                );
                let mut found = found;
                found.sort_unstable();
                assert_eq!(found, expected, "{name} (collapseThreads {collapse})");
            }
        }
    }

    // Sorts: by size; by a thread keyword as by the message's own keyword.
    let reply = email_query(account, None, json!([{ "property": "size" }]), json!({})).await;
    let found = ids(&reply, "size");
    assert_eq!(found.len(), corpus.len());
    assert!(
        found
            .windows(2)
            .all(|w| corpus[&w[0]].size <= corpus[&w[1]].size),
        "not in size order"
    );
    for (thread_property, keyword, filter) in [
        (
            "allInThreadHaveKeyword",
            "N",
            json!({ "before": date(1800) }),
        ),
        (
            "someInThreadHaveKeyword",
            "Bronze",
            and(vec![
                json!({ "after": date(1875) }),
                json!({ "before": date(1878) }),
            ]),
        ),
    ] {
        for ascending in [true, false] {
            let sort = |property: &str| {
                json!([
                    { "property": property, "keyword": keyword, "isAscending": ascending },
                    { "property": "receivedAt" },
                ])
            };
            let by_thread = email_query(
                account,
                Some(filter.clone()),
                sort(thread_property),
                json!({}),
            )
            .await;
            let by_message =
                email_query(account, Some(filter.clone()), sort("hasKeyword"), json!({})).await;
            assert_eq!(
                ids(&by_thread, thread_property),
                ids(&by_message, "hasKeyword"),
                "{thread_property} {keyword} (ascending {ascending})"
            );
            let found = ids(&by_message, "hasKeyword");
            let groups = found
                .iter()
                .map(|id| corpus[id].keywords.contains(keyword))
                .collect::<Vec<_>>();
            assert!(
                groups.windows(2).filter(|w| w[0] != w[1]).count() <= 1,
                "{thread_property} {keyword}: the keyword does not partition the result"
            );
        }
    }
}

/// Upstream's paging options (position, anchor, anchor offset, limit, with
/// and without collapsed threads), on the `receivedAt` order.
async fn query_options(account: &Account, corpus: &AHashMap<String, CorpusMessage>) {
    let sort = json!([{ "property": "receivedAt" }]);
    let all = ids(
        &email_query(account, None, sort.clone(), json!({})).await,
        "all",
    );
    assert_eq!(all.len(), corpus.len());
    let len = all.len();
    let slice = |from: usize, to: usize| all[from..to.min(len)].to_vec();
    let cases = [
        (json!({ "position": 0, "limit": 10 }), slice(0, 10)),
        (json!({ "position": 10, "limit": 10 }), slice(10, 20)),
        (json!({ "position": -10 }), slice(len - 10, len)),
        (
            json!({ "position": -20, "limit": 10 }),
            slice(len - 20, len - 10),
        ),
        (json!({ "position": -100000, "limit": 1 }), slice(0, 1)),
        (
            json!({ "position": -1, "limit": 100000 }),
            slice(len - 1, len),
        ),
        (
            json!({ "anchor": &all[100], "anchorOffset": 0, "limit": 10 }),
            slice(100, 110),
        ),
        (
            json!({ "anchor": &all[100], "anchorOffset": 10, "limit": 10 }),
            slice(110, 120),
        ),
        (
            json!({ "anchor": &all[100], "anchorOffset": -10, "limit": 10 }),
            slice(90, 100),
        ),
        (
            json!({ "anchor": &all[0], "anchorOffset": -10, "limit": 10 }),
            slice(0, 10),
        ),
        (
            json!({ "anchor": &all[len - 1], "anchorOffset": 10, "limit": 10 }),
            vec![],
        ),
        (
            json!({ "anchor": &all[len - 3], "anchorOffset": 0 }),
            slice(len - 3, len),
        ),
    ];
    for (options, expected) in cases {
        for collapse in [false, true] {
            let mut extra = options.clone();
            extra["collapseThreads"] = json!(collapse);
            let reply = email_query(account, None, sort.clone(), extra).await;
            assert_eq!(
                ids(&reply, &options.to_string()),
                expected,
                "{options} (collapseThreads {collapse})"
            );
        }
    }
}

/// `mail::search_snippet::test` in key mode: the same messages are imported;
/// every text, subject and body filter is refused by Email/query and by
/// SearchSnippet/get (spec 8), and a snippet request without one answers
/// bare snippets.
pub async fn search_snippet(test: &TestServer) {
    println!("Running key-account SearchSnippet tests...");
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;
    let mailbox_id = Id::from(INBOX_ID).to_string();

    let mut email_ids = AHashMap::default();
    let mut test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    test_dir.push("resources");
    test_dir.push("jmap");
    test_dir.push("email_snippet");
    for email_name in [
        "html",
        "subpart",
        "mixed",
        "text_plain",
        "text_plain_chinese",
    ] {
        let mut file_name = test_dir.clone();
        file_name.push(format!("{}.eml", email_name));
        let email_id = client
            .email_import(
                fs::read(&file_name).unwrap(),
                [&mailbox_id],
                None::<Vec<&str>>,
                None,
            )
            .await
            .unwrap()
            .take_id();
        email_ids.insert(email_name, email_id);
    }
    test.wait_for_tasks().await;

    let or = |conditions: Vec<Value>| json!({ "operator": "OR", "conditions": conditions });
    for (property, filter, email_name) in [
        (
            "subject",
            or(vec![
                or(vec![
                    json!({ "subject": "friend" }),
                    json!({ "subject": "help" }),
                ]),
                or(vec![json!({ "body": "secret" }), json!({ "body": "call" })]),
            ]),
            "text_plain",
        ),
        ("text", json!({ "text": "côte" }), "text_plain"),
        ("text", json!({ "text": "\"your country\"" }), "text_plain"),
        ("text", json!({ "text": "overseas" }), "text_plain"),
        ("text", json!({ "text": "孫子兵法" }), "text_plain_chinese"),
        ("text", json!({ "text": "cia" }), "subpart"),
        ("text", json!({ "text": "frösche" }), "html"),
        (
            "text",
            json!({ "text": "es:galería vasto biblioteca" }),
            "mixed",
        ),
    ] {
        let context = format!("{filter}");
        let reply = account
            .jmap_method_calls(json!([
                ["Email/query", { "accountId": account.id_string(), "filter": &filter }, "q"],
                ["SearchSnippet/get", {
                    "accountId": account.id_string(),
                    "filter": &filter,
                    "emailIds": [&email_ids[email_name]],
                }, "s"],
            ]))
            .await;
        assert_search_refused(&reply, 0, "unsupportedFilter", Some(property), &context);
        assert_search_refused(&reply, 1, "unsupportedFilter", Some(property), &context);
    }

    // Without a text term the snippets are bare.
    let all = email_ids.values().cloned().collect::<Vec<_>>();
    let reply = account
        .jmap_method_call(
            "SearchSnippet/get",
            json!({
                "accountId": account.id_string(),
                "filter": { "inMailbox": &mailbox_id },
                "emailIds": &all,
            }),
        )
        .await;
    assert_eq!(reply.list().len(), all.len(), "{reply:?}");
    for snippet in reply.list() {
        assert!(snippet["subject"].is_null(), "{snippet}");
        assert!(snippet["preview"].is_null(), "{snippet}");
    }

    // Destroy test data
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// `mail::query_changes::test` in key mode: upstream's log of inserts,
/// updates, moves and deletes, and its five queryChanges per state; the one
/// on `from` is refused (spec 8), the rest answer as upstream. Threads are
/// singletons, which the collapsed-thread check accepts as upstream's
/// imports do.
pub async fn query_changes(test: &TestServer) {
    println!("Running key-account Email QueryChanges tests...");
    let server = test.server.clone();
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;

    let mailbox1_id = client
        .mailbox_create("JMAP Changes 1", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();
    let mailbox2_id = client
        .mailbox_create("JMAP Changes 2", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    let mut states = vec![State::Initial];
    let mut id_map = AHashMap::default();

    let mut updated_ids = AHashSet::default();
    let mut removed_ids = AHashSet::default();
    let mut type1_ids = AHashSet::default();
    let mut thread_id_map: AHashMap<u32, Id> = AHashMap::default();

    let mut thread_id = 100;
    let mut refused = 0;

    for (change_num, change) in [
        LogAction::Insert(0),
        LogAction::Insert(1),
        LogAction::Insert(2),
        LogAction::Move(0, 3),
        LogAction::Insert(4),
        LogAction::Insert(5),
        LogAction::Update(1),
        LogAction::Update(2),
        LogAction::Delete(1),
        LogAction::Insert(6),
        LogAction::Insert(7),
        LogAction::Update(2),
        LogAction::Update(4),
        LogAction::Update(5),
        LogAction::Update(6),
        LogAction::Update(7),
        LogAction::Delete(4),
        LogAction::Delete(5),
        LogAction::Delete(6),
        LogAction::Insert(8),
        LogAction::Insert(9),
        LogAction::Insert(10),
        LogAction::Update(3),
        LogAction::Update(2),
        LogAction::Update(8),
        LogAction::Move(9, 11),
        LogAction::Move(10, 12),
        LogAction::Delete(8),
    ]
    .iter()
    .enumerate()
    {
        match &change {
            LogAction::Insert(id) => {
                let jmap_id = Id::from_str(
                    client
                        .email_import(
                            format!(
                                "From: test_{}\nSubject: test_{}\n\ntest",
                                if change_num % 2 == 0 { 1 } else { 2 },
                                *id
                            )
                            .into_bytes(),
                            [if change_num % 2 == 0 {
                                &mailbox1_id
                            } else {
                                &mailbox2_id
                            }],
                            [if change_num % 2 == 0 { "1" } else { "2" }].into(),
                            Some(*id as i64),
                        )
                        .await
                        .unwrap()
                        .id()
                        .unwrap(),
                )
                .unwrap();

                id_map.insert(*id, jmap_id);
                if change_num % 2 == 0 {
                    type1_ids.insert(jmap_id);
                }
                thread_id_map.entry(jmap_id.prefix_id()).or_insert(jmap_id);
            }
            LogAction::Update(id) => {
                let id = *id_map.get(id).unwrap();
                let mut batch = BatchBuilder::new();
                batch
                    .with_document(id.document_id())
                    .log_item_update(SyncCollection::Email, id.prefix_id().into());
                server.store().write(batch.build_all()).await.unwrap();
                updated_ids.insert(id);
            }
            LogAction::Delete(id) => {
                let id = *id_map.get(id).unwrap();
                client.email_destroy(&id.to_string()).await.unwrap();
                removed_ids.insert(id);
            }
            LogAction::Move(from, to) => {
                let id = *id_map.get(from).unwrap();
                let new_id = Id::from_parts(thread_id, id.document_id());

                let old_message_ = server
                    .store()
                    .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                        account.id().document_id(),
                        Collection::Email,
                        id.document_id(),
                    ))
                    .await
                    .unwrap()
                    .unwrap();
                let old_message = old_message_.to_unarchived::<MessageData>().unwrap();
                let mut new_message = old_message.deserialize::<MessageData>().unwrap();
                new_message.thread_id = thread_id;

                server
                    .core
                    .storage
                    .data
                    .write(
                        BatchBuilder::new()
                            .with_account_id(account.id().document_id())
                            .with_collection(Collection::Email)
                            .with_document(id.document_id())
                            .custom(
                                ObjectIndexBuilder::new()
                                    .with_current(old_message)
                                    .with_changes(new_message),
                            )
                            .unwrap()
                            .build_all(),
                    )
                    .await
                    .unwrap();

                id_map.insert(*to, new_id);
                if type1_ids.contains(&id) {
                    type1_ids.insert(new_id);
                }
                removed_ids.insert(id);
                thread_id_map.insert(new_id.prefix_id(), new_id);
                thread_id += 1;
            }
            LogAction::UpdateChild(_) => unreachable!(),
        }

        let mut new_state = State::Initial;
        for state in &states {
            for (test_num, query) in vec![
                QueryChanges {
                    filter: None,
                    sort: vec![email::query::Comparator::received_at()],
                    since_query_state: state.clone(),
                    max_changes: 0,
                    up_to_id: None,
                    collapse_threads: false,
                },
                QueryChanges {
                    filter: Some(email::query::Filter::from("test_1").into()),
                    sort: vec![email::query::Comparator::received_at()],
                    since_query_state: state.clone(),
                    max_changes: 0,
                    up_to_id: None,
                    collapse_threads: false,
                },
                QueryChanges {
                    filter: Some(email::query::Filter::in_mailbox(&mailbox1_id).into()),
                    sort: vec![email::query::Comparator::received_at()],
                    since_query_state: state.clone(),
                    max_changes: 0,
                    up_to_id: None,
                    collapse_threads: false,
                },
                QueryChanges {
                    filter: None,
                    sort: vec![email::query::Comparator::received_at()],
                    since_query_state: state.clone(),
                    max_changes: 0,
                    up_to_id: id_map
                        .get(&7)
                        .map(|id| id.to_string().into())
                        .unwrap_or(None),
                    collapse_threads: false,
                },
                QueryChanges {
                    filter: None,
                    sort: vec![email::query::Comparator::received_at()],
                    since_query_state: state.clone(),
                    max_changes: 0,
                    up_to_id: None,
                    collapse_threads: true,
                },
            ]
            .into_iter()
            .enumerate()
            {
                if (test_num == 3 || test_num == 4) && query.up_to_id.is_none() {
                    continue;
                }
                if test_num == 4 && !query.collapse_threads {
                    continue;
                }
                let mut request = client.build();
                let query_request = request
                    .query_email_changes(query.since_query_state.to_string())
                    .sort(query.sort);

                if let Some(filter) = query.filter {
                    query_request.filter(filter);
                }

                if let Some(up_to_id) = query.up_to_id {
                    query_request.up_to_id(up_to_id);
                }

                if query.collapse_threads {
                    query_request.arguments().collapse_threads(true);
                }

                let result = request.send_query_email_changes().await;
                if test_num == 1 {
                    // Spec 8: `from` needs the sealed content.
                    assert!(
                        matches!(
                            result,
                            Err(Error::Method(MethodError {
                                p_type: MethodErrorType::UnsupportedFilter
                            }))
                        ),
                        "{result:?}"
                    );
                    refused += 1;
                    continue;
                }
                let changes = result.unwrap();

                if test_num == 0 {
                    // Immutable filters should not return modified ids, only deletions.
                    for id in changes.removed() {
                        let id = Id::from_str(id).unwrap();
                        assert!(
                            removed_ids.contains(&id),
                            "{:?} (id: {:?})",
                            changes,
                            id_map.iter().find(|(_, v)| **v == id).map(|(k, _)| k)
                        );
                    }
                }
                if test_num == 2 {
                    // Only type 1 results should be added to the list.
                    for item in changes.added() {
                        let id = Id::from_str(item.id()).unwrap();
                        assert!(
                            type1_ids.contains(&id),
                            "{:?} (id: {:?})",
                            changes,
                            id_map.iter().find(|(_, v)| **v == id).map(|(k, _)| k)
                        );
                    }
                }
                if test_num == 3 {
                    // Only ids up to 7 should be added to the list.
                    for item in changes.added() {
                        let item_id = Id::from_str(item.id()).unwrap();
                        let id = id_map.iter().find(|(_, v)| **v == item_id).unwrap().0;
                        assert!(id <= &7, "{:?} (id: {})", changes, id);
                    }
                }
                if test_num == 4 {
                    // With collapse_threads, only first email per thread should be added.
                    let mut seen_threads = AHashSet::new();
                    for item in changes.added() {
                        let item_id = Id::from_str(item.id()).unwrap();
                        let thread_id = item_id.prefix_id();
                        assert!(
                            seen_threads.insert(thread_id),
                            "Thread {} appears multiple times with collapse_threads: {:?}",
                            thread_id,
                            changes
                        );
                        assert_eq!(
                            thread_id_map.get(&thread_id),
                            Some(&item_id),
                            "Expected first email in thread {}, got {:?}",
                            thread_id,
                            item_id
                        );
                    }
                }

                if let State::Initial = state {
                    new_state = State::parse_str(changes.new_query_state()).unwrap();
                }
            }
        }
        states.push(new_state);
    }
    assert!(refused > 0);

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// `mail::copy::test` in key mode: upstream copies a message from account 1
/// (the admin, plain) to account 2 (a key account). Both directions of a
/// copy with a key side are refused (spec 6.3): the admin's session cannot
/// reach the key account at all (R34), and the key account's own session,
/// though it reads the admin's shared mailbox, cannot copy from it. The
/// original stays, and nothing reaches the key account.
pub async fn copy(test: &TestServer) {
    println!("Running key-account Email Copy tests...");
    let admin = test.account("admin@example.com");
    let source = account_by_id(test, 1);
    let target = account_by_id(test, 2);
    assert_eq!(source.name(), admin.name(), "account 1 is the plain admin");
    let mut admin_client = admin.jmap_client().await;
    let target_client = target.jmap_client().await;

    let ac1_mailbox_id = admin_client
        .set_default_account_id(Id::new(1).to_string())
        .mailbox_create("Copy Test Ac# 1", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();
    let ac1_email_id = admin_client
        .email_import(
            concat!(
                "From: bill@example.com\r\n",
                "To: jdoe@example.com\r\n",
                "Subject: TPS Report\r\n",
                "\r\n",
                "I'm going to need those TPS reports ASAP. ",
                "So, if you could do that, that'd be great."
            )
            .as_bytes()
            .to_vec(),
            [&ac1_mailbox_id],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap()
        .take_id();
    // The admin's session cannot create a mailbox in the key account (R34):
    // the key account creates its own.
    assert_keyless_result(
        admin_client
            .set_default_account_id(Id::new(2).to_string())
            .mailbox_create("Copy Test Ac# 2", None::<String>, Role::None)
            .await,
        "the admin's Mailbox/set on the key account",
    );
    let ac2_mailbox_id = target_client
        .mailbox_create("Copy Test Ac# 2", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    // The admin's copy into the key account, as upstream sends it.
    let copy = |from: &str, to: &str| {
        json!({
            "fromAccountId": from,
            "accountId": to,
            "onSuccessDestroyOriginal": true,
            "create": { &ac1_email_id: {
                "id": &ac1_email_id,
                "mailboxIds": { &ac2_mailbox_id: true },
                "keywords": { "$draft": true },
                "receivedAt": date(311923920),
            } }
        })
    };
    let source_id = Id::new(1).to_string();
    let target_id = Id::new(2).to_string();
    let reply = admin
        .jmap_method_call("Email/copy", copy(&source_id, &target_id))
        .await;
    assert_keyless(&reply.0["methodResponses"][0], "the admin's Email/copy");

    // The key account reads the admin's shared mailbox, but cannot copy from
    // it into its own.
    admin_client
        .set_default_account_id(Id::new(1).to_string())
        .mailbox_update_acl(&ac1_mailbox_id, target.id_string(), [ACL::ReadItems])
        .await
        .unwrap();
    let mut shared = target.jmap_client().await;
    assert_eq!(
        shared
            .set_default_account_id(source_id.as_str())
            .email_get(&ac1_email_id, [Property::Subject].into())
            .await
            .unwrap()
            .unwrap()
            .subject()
            .unwrap(),
        "TPS Report"
    );
    let reply = target
        .jmap_method_call("Email/copy", copy(&source_id, &target_id))
        .await;
    assert_keyless(
        &reply.0["methodResponses"][0],
        "the key account's Email/copy",
    );

    // The original is still there, and the key account holds nothing.
    assert!(
        admin_client
            .set_default_account_id(Id::new(1).to_string())
            .email_get(&ac1_email_id, None::<Vec<_>>)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        test.server
            .get_cached_messages(2)
            .await
            .unwrap()
            .emails
            .items
            .len(),
        0
    );

    // Empty store
    admin.destroy_all_mailboxes_for_account(1).await;
    test.destroy_all_mailboxes(target).await;
    test.assert_is_empty().await;
}

/// `mail::thread_get::test` in key mode: the five replies to one message are
/// five singleton threads (spec 5.4).
pub async fn thread_get(test: &TestServer) {
    println!("Running key-account Email Thread tests...");
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;

    let mailbox_id = client
        .mailbox_create("JMAP Get", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    let mut threads = AHashSet::new();
    for num in [5, 3, 1, 2, 4] {
        let mut email = client
            .email_import(
                format!("Subject: test\nReferences: <1234>\n\n{}", num).into_bytes(),
                [&mailbox_id],
                None::<Vec<String>>,
                Some(10000i64 + num as i64),
            )
            .await
            .unwrap();
        let thread_id = email.thread_id().unwrap().to_string();
        let email_id = email.take_id();
        threads.insert(thread_id.clone());
        test.wait_for_tasks().await;
        assert_eq!(
            client
                .thread_get(&thread_id)
                .await
                .unwrap()
                .unwrap()
                .email_ids(),
            [email_id]
        );
    }
    assert_eq!(threads.len(), 5);

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// The order in which upstream imports the messages into account `index`.
fn import_order(messages: &[String], index: usize) -> Vec<&String> {
    match index {
        0 => messages.iter().collect(),
        1 => messages.iter().rev().collect(),
        2 => messages.chunks(5).flatten().collect(),
        3 => messages.chunks(5).flat_map(|c| c.iter().rev()).collect(),
        4 => messages.chunks(5).rev().flatten().collect(),
        _ => messages
            .chunks(5)
            .rev()
            .flat_map(|c| c.iter().rev())
            .collect(),
    }
}

/// `mail::thread_merge::test` in key mode. The single-threaded part imports
/// upstream's three thread trees into its six accounts in the same orders:
/// the admin and the Sales group, plain, merge them into upstream's threads;
/// the four key accounts, each through its own session (the admin's cannot
/// reach them, R34), keep every message in a thread of its own (spec 5.4).
/// The multi-threaded part ingests upstream's mailbox at server level, as
/// SMTP with a keyless token, so every message is sealed under the account's
/// public key: the messages are singletons. `wait_for_tasks` is the drain
/// (it returns when the task range is empty), so the test asserts no queue
/// state of its own (ruling R43); a scheduled `MergeThreads` that merged
/// anything would break the singleton assertion.
pub async fn thread_merge(test: &TestServer) {
    println!("Running key-account Email Merge Threads tests...");
    let admin = test.account("admin@example.com");
    let mut admin_client = admin.jmap_client().await;
    let names = [
        "admin@example.com",
        "jdoe@example.com",
        "jane.smith@example.com",
        "bill@example.com",
        "robert@example.com",
        "sales@example.com",
    ];
    // The plain accounts are reached by the admin's session, the key
    // accounts by their own.
    let mut clients = Vec::new();
    for name in names {
        let account = test.account(name);
        let is_key = test
            .server
            .account(account.id().document_id())
            .await
            .unwrap()
            .is_key_account();
        let mut client = if is_key {
            account.jmap_client().await
        } else {
            admin.jmap_client().await
        };
        client.set_default_account_id(account.id_string());
        clients.push((is_key, client));
    }
    assert_eq!(
        clients.iter().filter(|(is_key, _)| *is_key).count(),
        4,
        "four key accounts"
    );
    assert_keyless_result(
        admin_client
            .set_default_account_id(test.account("jdoe@example.com").id_string())
            .mailbox_create("Thread nightmare", None::<String>, Role::None)
            .await,
        "the admin's Mailbox/set on a key account",
    );

    for (test_group_num, tree) in [test_1(), test_2(), test_3()].iter().enumerate() {
        let mut messages = Vec::new();
        let mut total_messages = 0;
        let mut messages_per_thread =
            build_messages(tree, &mut messages, &mut total_messages, None, 0);
        messages_per_thread.sort_unstable();

        let mut mailbox_ids = Vec::with_capacity(6);
        for (_, client) in &clients {
            mailbox_ids.push(
                client
                    .mailbox_create("Thread nightmare", None::<String>, Role::None)
                    .await
                    .unwrap()
                    .take_id(),
            );
        }
        for (index, (_, client)) in clients.iter().enumerate() {
            for message in import_order(&messages, index) {
                client
                    .email_import(
                        message.to_string().into_bytes(),
                        [mailbox_ids[index].clone()],
                        None::<Vec<String>>,
                        None,
                    )
                    .await
                    .unwrap();
            }
        }
        test.wait_for_tasks().await;

        for (test_num, (is_key, client)) in clients.iter().enumerate() {
            let result = client
                .email_query(
                    email::query::Filter::in_mailbox(mailbox_ids[test_num].clone()).into(),
                    None::<Vec<_>>,
                )
                .await
                .unwrap();
            assert_eq!(
                result.ids().len(),
                total_messages,
                "test# {}/{}",
                test_group_num,
                test_num
            );
            let thread_ids: AHashSet<u32> = result
                .ids()
                .iter()
                .map(|id| Id::from_str(id).unwrap().prefix_id())
                .collect();
            let mut messages_per_thread_db = Vec::new();
            for thread_id in thread_ids {
                messages_per_thread_db.push(
                    client
                        .thread_get(&Id::new(thread_id as u64).to_string())
                        .await
                        .unwrap()
                        .unwrap()
                        .email_ids()
                        .len(),
                );
            }
            messages_per_thread_db.sort_unstable();
            if *is_key {
                assert_eq!(
                    messages_per_thread_db,
                    vec![1; total_messages],
                    "test# {test_group_num}/{test_num}: singleton threads"
                );
            } else {
                assert_eq!(messages_per_thread_db, messages_per_thread);
            }
            println!("passed test# {}/{}", test_group_num, test_num);
        }

        for (_, client) in &clients {
            client.destroy_all_mailboxes().await;
        }
        test.wait_for_tasks().await;
        test.assert_is_empty().await;
    }

    // The multi-threaded part
    println!("Running key-account Email Merge Threads tests (multi-threaded)...");
    let mut handles = vec![];
    let account = test.account("jdoe@example.com");
    let account_id = account.id().document_id();
    for message in MessageIterator::new(Cursor::new(crate::store::deflate_test_resource(
        "mailbox.gz",
    )))
    .collect::<Vec<_>>()
    .into_iter()
    {
        let message = message.unwrap();
        let server = test.server.clone();
        handles.push(tokio::task::spawn(async move {
            let mut retry_count = 0;
            loop {
                match server
                    .email_ingest(IngestEmail {
                        raw_message: message.contents(),
                        message: MessageParser::new().parse(message.contents()),
                        blob_hash: None,
                        access_token: &AccessToken::from_id_maybe_invalid(account_id),
                        mailbox_ids: vec![INBOX_ID],
                        keywords: vec![],
                        received_at: None,
                        source: IngestSource::Smtp {
                            deliver_to: "test@domain.org",
                            is_sender_authenticated: true,
                            is_spam: false,
                        },
                        session_id: 0,
                    })
                    .await
                {
                    Ok(_) => break,
                    Err(err) => {
                        if err.is_assertion_failure() && retry_count < 10 {
                            let backoff = rand::rng().random_range(50..=300);
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            retry_count += 1;
                            continue;
                        }
                        panic!("Failed to ingest message: {:?}", err);
                    }
                }
            }
        }));
    }
    let messages = handles.len();
    println!("Waiting for {} tasks to complete...", messages);
    for handle in handles {
        handle.await.expect("Task panicked");
    }
    test.wait_for_tasks().await;
    let cached = test.server.get_cached_messages(account_id).await.unwrap();
    assert_eq!(messages, cached.emails.items.len());
    assert!(
        cached
            .emails
            .items
            .iter()
            .all(|m| m.thread_id == m.document_id),
        "a sealed message is its own thread"
    );
    println!("Deleting all messages...");
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// `mail::acl::test` in key mode. John, Jane and Bill are key accounts; the
/// admin and the Sales group are plain. Every grant on a key account's
/// mailbox is refused (spec 6.3), so John never reaches Jane's mail, and a
/// blob copy, an import or an Email/copy across a key account is refused.
/// Upstream's grant-and-try sequence runs on the admin's INBOX shared with
/// John (rights are enforced for a key session in a plain share as
/// upstream enforces them), its cross-account import and copy steps
/// refused; the Sales group's membership steps run as upstream.
pub async fn acl(test: &TestServer) {
    println!("Running key-account ACL tests...");
    let inbox_id = Id::new(INBOX_ID as u64).to_string();
    let trash_id = Id::new(TRASH_ID as u64).to_string();

    let admin = test.account("admin@example.com");
    let john = test.account("jdoe@example.com");
    let jane = test.account("jane.smith@example.com");
    let bill = test.account("bill@example.com");
    let sales = test.account("sales@example.com");

    let mut john_client = john.jmap_client().await;
    let mut jane_client = jane.jmap_client().await;
    let mut bill_client = bill.jmap_client().await;
    let mut admin_client = admin.jmap_client().await;

    // The admin account's Email state before this module's messages
    let admin_state = admin
        .jmap_method_call(
            "Email/get",
            json!({ "accountId": admin.id_string(), "ids": [] }),
        )
        .await
        .pointer("/methodResponses/0/1/state")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    // Insert two emails in each account
    let mut email_ids = AHashMap::default();
    for (client, account_id, name) in [
        (&mut john_client, john.id(), "john"),
        (&mut jane_client, jane.id(), "jane"),
        (&mut bill_client, bill.id(), "bill"),
        (&mut admin.jmap_client().await, sales.id(), "sales"),
        (&mut admin.jmap_client().await, admin.id(), "admin"),
    ] {
        let user_name = client.session().username().to_string();
        let mut ids = Vec::with_capacity(2);
        for (mailbox_id, mailbox_name) in [(&inbox_id, "inbox"), (&trash_id, "trash")] {
            ids.push(
                client
                    .set_default_account_id(account_id.to_string())
                    .email_import(
                        format!(
                            concat!(
                                "From: acl_test@example.com\r\n",
                                "To: {}\r\n",
                                "Subject: Owned by {} in {}\r\n",
                                "\r\n",
                                "This message is owned by {}.",
                            ),
                            user_name, name, mailbox_name, name
                        )
                        .into_bytes(),
                        [mailbox_id],
                        None::<Vec<&str>>,
                        None,
                    )
                    .await
                    .unwrap()
                    .take_id(),
            );
        }
        email_ids.insert(name, ids);
    }
    let first = |name: &str| email_ids[name][0].clone();
    let last = |name: &str| email_ids[name][1].clone();

    // John should have access to his emails only
    assert_eq!(
        john_client
            .email_get(&first("john"), [Property::Subject].into())
            .await
            .unwrap()
            .unwrap()
            .subject()
            .unwrap(),
        "Owned by john in inbox"
    );
    for (account, email) in [(jane, first("jane")), (sales, first("sales"))] {
        assert_forbidden(
            john_client
                .set_default_account_id(account.id_string())
                .email_get(&email, [Property::Subject].into())
                .await,
        );
        assert_forbidden(
            john_client
                .set_default_account_id(account.id_string())
                .mailbox_get(&inbox_id, None::<Vec<_>>)
                .await,
        );
    }

    // Spec 6.3: Jane's grants on her INBOX are refused, so John stays out.
    for rights in [
        vec![ACL::ReadItems],
        vec![ACL::ReadItems, ACL::AddItems],
        vec![ACL::ReadItems, ACL::Administer],
    ] {
        assert_keyless_result(
            jane_client
                .set_default_account_id(jane.id_string())
                .mailbox_update_acl(&inbox_id, john.id_string(), rights)
                .await,
            "Jane's grant on her INBOX",
        );
    }
    assert_forbidden(
        john_client
            .set_default_account_id(jane.id_string())
            .email_get(&first("jane"), [Property::Subject].into())
            .await,
    );
    assert_forbidden(
        john_client
            .set_default_account_id(jane.id_string())
            .email_changes("n", None)
            .await,
    );
    john_client.refresh_session().await.unwrap();
    assert!(john_client.session().account(jane.id_string()).is_none());

    // Blobs, imports and copies across a key account are refused.
    let blob_id = jane_client
        .set_default_account_id(jane.id_string())
        .email_get(&first("jane"), [Property::BlobId].into())
        .await
        .unwrap()
        .unwrap()
        .take_blob_id();
    assert_forbidden(
        john_client
            .set_default_account_id(john.id_string())
            .blob_copy(jane.id_string(), &blob_id)
            .await,
    );
    let upload = john_client
        .set_default_account_id(john.id_string())
        .upload(
            Some(john.id_string()),
            concat!(
                "From: acl_test@example.com\r\n",
                "To: jane.smith@example.com\r\n",
                "Subject: Created by john in jane's inbox\r\n",
                "\r\n",
                "This message is owned by jane.",
            )
            .as_bytes()
            .to_vec(),
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let reply = john
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": jane.id_string(),
                "emails": { "i": { "blobId": &upload, "mailboxIds": { &inbox_id: true } } }
            }),
        )
        .await;
    assert_eq!(reply.0["methodResponses"][0][0], "error", "{reply:?}");
    assert_eq!(
        reply.0["methodResponses"][0][1]["type"], "forbidden",
        "{reply:?}"
    );
    assert_forbidden(
        john_client
            .set_default_account_id(jane.id_string())
            .email_copy(
                john.id_string(),
                &last("john"),
                [&inbox_id],
                None::<Vec<&str>>,
                None,
            )
            .await,
    );

    // The admin, a plain account, shares its INBOX with John, and upstream's
    // grant-and-try sequence runs on that share with John's key session as
    // upstream runs it on Jane's INBOX. The steps that move mail across John's
    // key account stay refused (spec 6.3, ruling R31): a Blob/copy into his
    // account, an Email/copy from the share into it, and the import of his
    // sealed upload and the Email/copy of his message into the share, with
    // AddItems granted too; the owner imports the two messages those steps
    // add upstream.
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&inbox_id, john.id_string(), [ACL::ReadItems])
        .await
        .unwrap();

    // John should have ReadItems access to Inbox
    assert_eq!(
        john_client
            .set_default_account_id(admin.id_string())
            .email_get(&first("admin"), [Property::Subject].into())
            .await
            .unwrap()
            .unwrap()
            .subject()
            .unwrap(),
        "Owned by admin in inbox"
    );
    assert_eq!(
        john_client
            .set_default_account_id(admin.id_string())
            .email_query(None::<email::query::Filter>, None::<Vec<_>>)
            .await
            .unwrap()
            .ids(),
        [first("admin").as_str()]
    );

    // John's session resource should contain the admin's account details
    john_client.refresh_session().await.unwrap();
    assert_eq!(
        john_client
            .session()
            .account(admin.id_string())
            .unwrap()
            .name(),
        "admin@example.com"
    );

    // John should not have access to emails in the admin's Trash folder
    assert!(
        john_client
            .set_default_account_id(admin.id_string())
            .email_get(&last("admin"), [Property::Subject].into())
            .await
            .unwrap()
            .is_none(),
        "the unshared Trash"
    );

    // Email/changes must not leak ids of emails in folders John cannot read
    // (since the state before the admin's two messages: the admin account's
    // log holds the earlier modules' changes too).
    let changed_ids = john_client
        .set_default_account_id(admin.id_string())
        .email_changes(&admin_state, None)
        .await
        .unwrap()
        .created()
        .to_vec();
    assert!(
        changed_ids.contains(&first("admin")),
        "Email/changes should report the shared Inbox email"
    );
    assert!(
        !changed_ids.contains(&last("admin")),
        "Email/changes leaked the id of a non-shared Trash email"
    );

    // Blobs are not copied into John's account, the shared message's
    // included (R31), and the Trash one is not readable; nor is a message of
    // the share copied into his account.
    for email in [first("admin"), last("admin")] {
        let blob_id = admin_client
            .set_default_account_id(admin.id_string())
            .email_get(&email, [Property::BlobId].into())
            .await
            .unwrap()
            .unwrap()
            .take_blob_id();
        assert_forbidden(
            john_client
                .set_default_account_id(john.id_string())
                .blob_copy(admin.id_string(), &blob_id)
                .await,
        );
    }
    let reply = john
        .jmap_method_call(
            "Email/copy",
            json!({
                "fromAccountId": admin.id_string(),
                "accountId": john.id_string(),
                "create": { "c": { "id": first("admin"), "mailboxIds": { &inbox_id: true } } }
            }),
        )
        .await;
    assert_keyless(&reply.0["methodResponses"][0], "John's copy from the share");

    // John only has ReadItems access to Inbox
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&inbox_id, john.id_string(), [ACL::ReadItems])
        .await
        .unwrap();
    assert_eq!(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_get(&inbox_id, [mailbox::Property::MyRights].into())
            .await
            .unwrap()
            .unwrap()
            .my_rights()
            .unwrap()
            .acl_list(),
        vec![ACL::ReadItems]
    );

    // Try to add items using import and copy. Both are refused whether or
    // not John holds AddItems: the keyless checks come before the ACL check
    // (R31 refuses a sealed source of John's into another account in
    // Email/import, and spec 6.3 refuses the cross-account copy), so the
    // answer below says nothing about AddItems. AddItems is enforced and
    // shown by the move step further down and by IMAP APPEND in the IMAP
    // variant.
    let import = json!({
        "accountId": admin.id_string(),
        "emails": { "i": { "blobId": &upload, "mailboxIds": { &inbox_id: true } } }
    });
    let copy = json!({
        "fromAccountId": john.id_string(),
        "accountId": admin.id_string(),
        "create": { "i": { "id": last("john"), "mailboxIds": { &inbox_id: true } } }
    });
    for (method, args) in [("Email/import", &import), ("Email/copy", &copy)] {
        let reply = john.jmap_method_call(method, args.clone()).await;
        let response = &reply.0["methodResponses"][0];
        assert!(
            (response[0] == "error" && response[1]["type"] == "forbidden")
                || response[1]["notCreated"]["i"]["type"] == "forbidden",
            "John's {method} into the share without AddItems: {response}"
        );
    }
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&inbox_id, john.id_string(), [ACL::ReadItems, ACL::AddItems])
        .await
        .unwrap();
    for (method, args) in [("Email/import", &import), ("Email/copy", &copy)] {
        let reply = john.jmap_method_call(method, args.clone()).await;
        assert_keyless(
            &reply.0["methodResponses"][0],
            &format!("John's {method} into the share with AddItems"),
        );
    }
    let mut owner_imports = Vec::with_capacity(2);
    for subject in [
        "Created for john in admin's inbox",
        "Owned by admin in inbox, second",
    ] {
        owner_imports.push(
            admin_client
                .set_default_account_id(admin.id_string())
                .email_import(
                    format!(
                        concat!(
                            "From: acl_test@example.com\r\n",
                            "To: admin@example.com\r\n",
                            "Subject: {}\r\n",
                            "\r\n",
                            "This message is owned by admin.",
                        ),
                        subject
                    )
                    .into_bytes(),
                    [&inbox_id],
                    None::<Vec<&str>>,
                    None,
                )
                .await
                .unwrap()
                .take_id(),
        );
    }
    let (email_id, email_id_2) = (owner_imports[0].clone(), owner_imports[1].clone());

    // Try removing items
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .email_destroy(&email_id)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &inbox_id,
            john.id_string(),
            [ACL::ReadItems, ACL::AddItems, ACL::RemoveItems],
        )
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .email_destroy(&email_id)
        .await
        .unwrap();

    // Try to set keywords
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .email_set_keyword(&email_id_2, "$seen", true)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &inbox_id,
            john.id_string(),
            [
                ACL::ReadItems,
                ACL::AddItems,
                ACL::RemoveItems,
                ACL::SetKeywords,
            ],
        )
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .email_set_keyword(&email_id_2, "$seen", true)
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .email_set_keyword(&email_id_2, "my-keyword", true)
        .await
        .unwrap();

    // Try to create a child
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_create("John's mailbox", None::<&str>, Role::None)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &inbox_id,
            john.id_string(),
            [
                ACL::ReadItems,
                ACL::AddItems,
                ACL::RemoveItems,
                ACL::SetKeywords,
                ACL::CreateChild,
            ],
        )
        .await
        .unwrap();
    let mailbox_id = john_client
        .set_default_account_id(admin.id_string())
        .mailbox_create("John's mailbox", Some(&inbox_id), Role::None)
        .await
        .unwrap()
        .take_id();

    // Try renaming a mailbox
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_rename(&mailbox_id, "John's private mailbox")
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&mailbox_id, john.id_string(), [ACL::ReadItems, ACL::Rename])
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .mailbox_rename(&mailbox_id, "John's private mailbox")
        .await
        .unwrap();

    // Try moving a message
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .email_set_mailbox(&email_id_2, &mailbox_id, true)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &mailbox_id,
            john.id_string(),
            [ACL::ReadItems, ACL::Rename, ACL::AddItems],
        )
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .email_set_mailbox(&email_id_2, &mailbox_id, true)
        .await
        .unwrap();

    // Try deleting a mailbox
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_destroy(&mailbox_id, true)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &mailbox_id,
            john.id_string(),
            [ACL::ReadItems, ACL::Rename, ACL::AddItems, ACL::Delete],
        )
        .await
        .unwrap();
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_destroy(&mailbox_id, true)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &mailbox_id,
            john.id_string(),
            [
                ACL::ReadItems,
                ACL::Rename,
                ACL::AddItems,
                ACL::Delete,
                ACL::RemoveItems,
            ],
        )
        .await
        .unwrap();
    john_client
        .set_default_account_id(admin.id_string())
        .mailbox_destroy(&mailbox_id, true)
        .await
        .unwrap();

    // Try changing ACL
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_update_acl(&inbox_id, bill.id_string(), [ACL::ReadItems])
            .await,
    );
    assert_forbidden(
        bill_client
            .set_default_account_id(admin.id_string())
            .email_query(None::<email::query::Filter>, None::<Vec<_>>)
            .await,
    );
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(
            &inbox_id,
            john.id_string(),
            [
                ACL::ReadItems,
                ACL::AddItems,
                ACL::RemoveItems,
                ACL::SetKeywords,
                ACL::CreateChild,
                ACL::Rename,
                ACL::Administer,
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        john_client
            .set_default_account_id(admin.id_string())
            .mailbox_get(&inbox_id, [mailbox::Property::MyRights].into())
            .await
            .unwrap()
            .unwrap()
            .my_rights()
            .unwrap()
            .acl_list(),
        vec![
            ACL::ReadItems,
            ACL::AddItems,
            ACL::RemoveItems,
            ACL::SetSeen,
            ACL::SetKeywords,
            ACL::CreateChild,
            ACL::Rename
        ]
    );
    john_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&inbox_id, bill.id_string(), [ACL::ReadItems])
        .await
        .unwrap();
    assert_eq!(
        bill_client
            .set_default_account_id(admin.id_string())
            .email_query(
                None::<email::query::Filter>,
                vec![email::query::Comparator::subject()].into()
            )
            .await
            .unwrap()
            .ids(),
        [first("admin").as_str(), &email_id_2]
    );

    // Revoke all access to John
    admin_client
        .set_default_account_id(admin.id_string())
        .mailbox_update_acl(&inbox_id, john.id_string(), [])
        .await
        .unwrap();
    assert_forbidden(
        john_client
            .set_default_account_id(admin.id_string())
            .email_get(&first("admin"), [Property::Subject].into())
            .await,
    );
    john_client.refresh_session().await.unwrap();
    assert!(john_client.session().account(admin.id_string()).is_none());
    assert_eq!(
        bill_client
            .set_default_account_id(admin.id_string())
            .email_get(&first("admin"), [Property::Subject].into())
            .await
            .unwrap()
            .unwrap()
            .subject()
            .unwrap(),
        "Owned by admin in inbox"
    );

    // Add John and Jane to the Sales group
    let sales_id = sales.id();
    for name in ["jdoe@example.com", "jane.smith@example.com"] {
        admin
            .registry_update_object(
                ObjectType::Account,
                test.account(name).id(),
                json!({
                    "memberGroupIds": { sales_id: true },
                }),
            )
            .await;
    }
    john_client.refresh_session().await.unwrap();
    jane_client.refresh_session().await.unwrap();
    bill_client.refresh_session().await.unwrap();
    assert_eq!(
        john_client
            .session()
            .account(sales.id_string())
            .unwrap()
            .name(),
        "sales@example.com"
    );
    assert!(
        !john_client
            .session()
            .account(sales.id_string())
            .unwrap()
            .is_personal()
    );
    assert_eq!(
        jane_client
            .session()
            .account(sales.id_string())
            .unwrap()
            .name(),
        "sales@example.com"
    );
    assert!(bill_client.session().account(sales.id_string()).is_none());

    // Insert a message in Sales's inbox
    let blob_id = john_client
        .set_default_account_id(sales.id_string())
        .upload(
            Some(sales.id_string()),
            concat!(
                "From: acl_test@example.com\r\n",
                "To: sales@example.com\r\n",
                "Subject: Created by john in sales\r\n",
                "\r\n",
                "This message is owned by sales.",
            )
            .as_bytes()
            .to_vec(),
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let reply = john
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": sales.id_string(),
                "emails": { "i": { "blobId": &blob_id, "mailboxIds": { &inbox_id: true } } }
            }),
        )
        .await;
    let email_id = reply
        .pointer("/methodResponses/0/1/created/i/id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("Email/import into Sales: {reply:?}"))
        .to_string();

    // Both Jane and John should be able to see this message, but not Bill
    for client in [&mut john_client, &mut jane_client] {
        assert_eq!(
            client
                .set_default_account_id(sales.id_string())
                .email_get(&email_id, [Property::Subject].into())
                .await
                .unwrap()
                .unwrap()
                .subject()
                .unwrap(),
            "Created by john in sales"
        );
    }
    assert_forbidden(
        bill_client
            .set_default_account_id(sales.id_string())
            .email_get(&email_id, [Property::Subject].into())
            .await,
    );

    // Remove John from the sales group
    admin
        .registry_update_object(
            ObjectType::Account,
            john.id(),
            json!({
                "memberGroupIds": { sales_id: false },
            }),
        )
        .await;
    assert_forbidden(
        john_client
            .set_default_account_id(sales.id_string())
            .email_get(&email_id, [Property::Subject].into())
            .await,
    );

    // Destroy test account data: the plain accounts through the admin, the
    // key accounts through their own sessions.
    for account in [sales, admin] {
        admin
            .destroy_all_mailboxes_for_account(account.id().document_id())
            .await;
    }
    for account in [john, bill, jane] {
        test.destroy_all_mailboxes(account).await;
    }
    test.blob_expire_all().await;
    test.assert_is_empty().await;
}

/// One push the mock server received within `wait`.
async fn next_push(rx: &mut mpsc::Receiver<Value>, wait: Duration) -> Value {
    match tokio::time::timeout(wait, rx.recv()).await {
        Ok(Some(push)) => push,
        result => panic!("Timeout waiting for push: {result:?}"),
    }
}

async fn no_push(rx: &mut mpsc::Receiver<Value>) {
    if let Ok(message) = tokio::time::timeout(Duration::from_millis(1000), rx.recv()).await {
        panic!("Received a message when expecting nothing: {message:?}");
    }
}

/// A push verification: its subscription id and code.
fn verification(push: &Value) -> (String, String) {
    assert_eq!(push["@type"], "PushVerification", "{push}");
    (
        push.text_field("pushSubscriptionId").to_string(),
        push.text_field("verificationCode").to_string(),
    )
}

/// The data types a state change names for `account`.
fn state_change(push: &Value, account: &Account) -> BTreeSet<String> {
    assert_eq!(push["@type"], "StateChange", "{push}");
    push["changed"][account.id_string()]
        .as_object()
        .unwrap_or_else(|| panic!("{push}"))
        .keys()
        .cloned()
        .collect()
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// `core::push_subscription::test` in key mode: subscriptions, verification,
/// type filters, retries and grouping run as upstream; an `emailPush`
/// subscription asking for a message's sender and subject receives the
/// state change alone for every delivery (spec 9), and the push object
/// builder produces nothing for a key account's message.
pub async fn push_subscription(test: &TestServer) {
    println!("Running key-account Push Subscription tests...");
    let account = test.account("robert@example.com");
    let client = account.jmap_client().await;
    let wait = Duration::from_millis(1500);

    // Start mock push server
    let (mut event_rx, push) = spawn_mock_push_server(test).await;
    let push_server = push.server.clone();
    let pubkey = push.public_key.clone();
    let auth_secret = push.auth_secret.clone();
    let keys = Keys::new(&pubkey, &auth_secret);

    // Register push notification (no encryption) and verify it
    let push_id = client
        .push_subscription_create("123", "https://127.0.0.1:19000/push", None)
        .await
        .unwrap()
        .take_id();
    let (id, code) = verification(&next_push(&mut event_rx, wait).await);
    assert_eq!(id, push_id);
    client
        .push_subscription_verify(&push_id, code)
        .await
        .unwrap();

    // Create a mailbox and expect a state change
    let mailbox_id = client
        .mailbox_create("PushSubscription Test", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();
    assert_eq!(
        state_change(&next_push(&mut event_rx, wait).await, account),
        names(&["Mailbox"])
    );

    // Receive states just for the requested types
    client
        .push_subscription_update_types(&push_id, [jmap_client::DataType::Email].into())
        .await
        .unwrap();
    client
        .mailbox_update_sort_order(&mailbox_id, 123)
        .await
        .unwrap();
    no_push(&mut event_rx).await;
    client.push_subscription_destroy(&push_id).await.unwrap();

    // Only one verification per minute is allowed
    let first_id = client
        .push_subscription_create(
            "first",
            "https://127.0.0.1:19000/push?skip_checks=true",
            None,
        )
        .await
        .unwrap()
        .take_id();
    assert_eq!(
        verification(&next_push(&mut event_rx, wait).await).0,
        first_id
    );
    let deferred_id = client
        .push_subscription_create("deferred", "https://127.0.0.1:19000/push", None)
        .await
        .unwrap()
        .take_id();
    no_push(&mut event_rx).await;
    assert_eq!(
        verification(&next_push(&mut event_rx, Duration::from_secs(8)).await).0,
        deferred_id
    );
    account
        .jmap_request(
            &["urn:ietf:params:jmap:core"],
            json!([[
                "PushSubscription/set",
                { "destroy": [first_id, deferred_id] },
                "0"
            ]]),
        )
        .await;

    // Register push notification (with encryption)
    let push_id = client
        .push_subscription_create(
            "123",
            "https://127.0.0.1:19000/push?skip_checks=true",
            keys.into(),
        )
        .await
        .unwrap()
        .take_id();
    let (id, code) = verification(&next_push(&mut event_rx, wait).await);
    assert_eq!(id, push_id);
    client
        .push_subscription_verify(&push_id, code)
        .await
        .unwrap();

    // Failed deliveries should be re-attempted
    push_server.fail_requests.store(true, Ordering::Relaxed);
    client
        .mailbox_update_sort_order(&mailbox_id, 101)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    push_server.fail_requests.store(false, Ordering::Relaxed);
    assert_eq!(
        state_change(&next_push(&mut event_rx, wait).await, account),
        names(&["Mailbox"])
    );

    // Make a mailbox change and expect state change
    client
        .mailbox_rename(&mailbox_id, "My Mailbox")
        .await
        .unwrap();
    assert_eq!(
        state_change(&next_push(&mut event_rx, wait).await, account),
        names(&["Mailbox"])
    );

    // Multiple change updates should be grouped and pushed in intervals
    for num in 0..5 {
        client
            .mailbox_update_sort_order(&mailbox_id, num)
            .await
            .unwrap();
    }
    assert_eq!(
        state_change(&next_push(&mut event_rx, wait).await, account),
        names(&["Mailbox"])
    );
    no_push(&mut event_rx).await;

    // Destroy mailbox
    client.push_subscription_destroy(&push_id).await.unwrap();
    client.mailbox_destroy(&mailbox_id, true).await.unwrap();
    no_push(&mut event_rx).await;

    // Spec 9: an emailPush subscription asking for content
    let account_id_str = account.id_string().to_string();
    let p256dh = URL_SAFE_NO_PAD.encode(&pubkey);
    let auth = URL_SAFE_NO_PAD.encode(auth_secret);
    let create = account
        .jmap_request(
            &[
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:emailpush",
            ],
            json!([[
                "PushSubscription/set",
                {
                    "create": {
                        "i0": {
                            "deviceClientId": "emailpush",
                            "url": "https://127.0.0.1:19000/push?skip_checks=true",
                            "keys": { "p256dh": p256dh, "auth": auth },
                            "types": [],
                            "emailPush": {
                                (account_id_str.clone()): {
                                    "filter": { "subject": "urgent" },
                                    "properties": ["from", "subject", "id"],
                                    "urgency": "high"
                                }
                            }
                        }
                    }
                },
                "0"
            ]]),
        )
        .await;
    let ep_id = create.created_id(0);
    let (id, code) = verification(&next_push(&mut event_rx, wait).await);
    assert_eq!(id, ep_id.to_string());
    account
        .jmap_request(
            &["urn:ietf:params:jmap:core"],
            json!([[
                "PushSubscription/set",
                { "update": { (ep_id.to_string()): { "verificationCode": code } } },
                "0"
            ]]),
        )
        .await;

    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest(
        "sender@example.com",
        &["robert@example.com"],
        concat!(
            "From: Sender <sender@example.com>\r\n",
            "To: robert@example.com\r\n",
            "Subject: Urgent: action required\r\n",
            "\r\n",
            "Please respond as soon as possible."
        ),
    )
    .await;
    lmtp.quit().await;
    let push = next_push(&mut event_rx, wait).await;
    let changed = state_change(&push, account);
    assert!(changed.contains("Email"), "{push}");
    let text = push.to_string();
    for content in ["Urgent", "action required", "sender@example.com"] {
        assert!(
            !text.contains(content),
            "the push carries {content}: {text}"
        );
    }

    // A message the filter would not match: whatever is pushed is a state
    // change, without content.
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest(
        "sender@example.com",
        &["robert@example.com"],
        concat!(
            "From: Sender <sender@example.com>\r\n",
            "To: robert@example.com\r\n",
            "Subject: weekly newsletter\r\n",
            "\r\n",
            "Nothing important here."
        ),
    )
    .await;
    lmtp.quit().await;
    while let Ok(Some(push)) = tokio::time::timeout(wait, event_rx.recv()).await {
        state_change(&push, account);
        assert!(!push.to_string().contains("newsletter"), "{push}");
    }

    account
        .jmap_request(
            &["urn:ietf:params:jmap:core"],
            json!([[
                "PushSubscription/set",
                { "destroy": [ep_id.to_string()] },
                "0"
            ]]),
        )
        .await;

    // The EmailPush object builder produces nothing for a key account's
    // message (defence in depth behind the state-only conversion).
    let mailbox_id = client
        .mailbox_create("EmailPush Object Test", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();
    let mailbox_doc_id = Id::from_str(&mailbox_id).unwrap().document_id();
    let email_id = client
        .email_import(
            b"From: Alice <alice@example.com>\r\nTo: robert@example.com\r\nSubject: Urgent meeting tonight\r\n\r\nPlease join the urgent meeting tonight.".to_vec(),
            [&mailbox_id],
            Some(["$notify"]),
            None,
        )
        .await
        .unwrap()
        .take_id();
    let document_id = Id::from_str(&email_id).unwrap().document_id();
    test.wait_for_tasks().await;
    let account_id = account.id().document_id();
    for filter in [
        vec![],
        vec![ProtoFilter::Property(EmailFilter::Subject("URGENT".into()))],
        vec![ProtoFilter::Property(EmailFilter::InMailbox(Id::from(
            mailbox_doc_id,
        )))],
        vec![ProtoFilter::Property(EmailFilter::HasKeyword(
            Keyword::parse("$notify"),
        ))],
    ] {
        let config = EmailPush {
            account_id,
            properties: vec![
                EmailPushProperty::Id,
                EmailPushProperty::From,
                EmailPushProperty::Subject,
            ],
            filter: filter.clone(),
            urgency: Urgency::Normal,
        };
        assert!(
            build_email_push_object(&test.server, account_id, document_id, &config, 4096)
                .await
                .unwrap()
                .is_none(),
            "an EmailPush object for a key account's message: {filter:?}"
        );
    }

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// The value at `pointer` of `response`, as text.
fn text_at(response: &JmapResponse, pointer: &str) -> String {
    response
        .pointer(pointer)
        .and_then(|v| match v {
            Value::String(s) => Some(s.to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

/// `core::blob::test` in key mode: the account's uploads are sealed (spec
/// 5.7) and read back with the session's keys, so Blob/upload, Blob/get,
/// the import of an upload and Blob/lookup answer as upstream; a
/// Blob/upload whose data source is the account's own upload is a raw read
/// of a sealed source, refused (rulings R31, R32).
pub async fn blob(test: &TestServer) {
    println!("Running key-account blob tests...");
    let account = test.account("jdoe@example.com");
    test.blob_expire_all().await;

    // Blob/set simple test
    let response = account.jmap_method_call("Blob/upload", json!({
             "accountId": account.id_string(),
             "create": {
              "abc": {
               "data" : [
               {
                "data:asBase64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQMAAAAl21bKAAAAA1BMVEX/AAAZ4gk3AAAAAXRSTlN/gFy0ywAAAApJREFUeJxjYgAAAAYAAzY3fKgAAAAASUVORK5CYII="
               }
              ],
              "type": "image/png"
              }
             }
            })).await;
    assert_eq!(
        text_at(&response, "/methodResponses/0/1/created/abc/type"),
        "image/png",
        "Response: {response:?}"
    );
    assert_eq!(
        text_at(&response, "/methodResponses/0/1/created/abc/size"),
        "95",
        "Response: {response:?}"
    );

    // Blob/get simple test
    let blob_id = account
        .jmap_method_call(
            "Blob/upload",
            json!({
             "accountId": account.id_string(),
             "create": {
              "abc": {
               "data" : [
               {
                "data:asText": "The quick brown fox jumped over the lazy dog."
               }
              ]
              }
             }
            }),
        )
        .await
        .pointer("/methodResponses/0/1/created/abc/id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let response = account
        .jmap_method_calls(json!([[
            "Blob/get",
            {
              "accountId": account.id_string(),
              "ids" : [blob_id],
              "properties" : ["data:asText", "digest:sha", "size"]
            },
            "R1"
          ],
          [
            "Blob/get",
            {
              "accountId": account.id_string(),
              "ids" : [blob_id],
              "properties" : ["data:asText", "digest:sha", "digest:sha-256", "size"],
              "offset" : 4,
              "length" : 9
            },
            "R2"
          ]
        ]))
        .await;
    for (pointer, expected) in [
        (
            "/methodResponses/0/1/list/0/data:asText",
            "The quick brown fox jumped over the lazy dog.",
        ),
        (
            "/methodResponses/0/1/list/0/digest:sha",
            "wIVPufsDxBzOOALLDSIFKebu+U4=",
        ),
        ("/methodResponses/0/1/list/0/size", "45"),
        ("/methodResponses/1/1/list/0/data:asText", "quick bro"),
        (
            "/methodResponses/1/1/list/0/digest:sha",
            "QiRAPtfyX8K6tm1iOAtZ87Xj3Ww=",
        ),
        (
            "/methodResponses/1/1/list/0/digest:sha-256",
            "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=",
        ),
    ] {
        assert_eq!(
            text_at(&response, pointer),
            expected,
            "Pointer {pointer:?} Response: {response:?}",
        );
    }
    test.blob_expire_all().await;

    // Blob/upload Complex Example: the second upload reads the first one
    // raw, which is refused; the Blob/get of it then has no reference.
    let response = account
        .jmap_method_calls(json!([
         [
          "Blob/upload",
          {
           "accountId": account.id_string(),
           "create": {
            "b4": {
             "data": [{ "data:asText": "The quick brown fox jumped over the lazy dog." }]
            }
           }
          },
          "S4"
         ],
         [
          "Blob/upload",
          {
           "accountId": account.id_string(),
           "create": {
             "cat": {
               "data": [
                 { "data:asText": "How" },
                 { "blobId": "#b4", "length": 7, "offset": 3 },
                 { "data:asText": "was t" },
                 { "blobId": "#b4", "length": 1, "offset": 1 },
                 { "data:asBase64": "YXQ/" }
               ]
             }
           }
          },
          "CAT"
         ],
         [
          "Blob/get",
          {
           "accountId": account.id_string(),
           "properties": ["data:asText", "size"],
           "ids": ["#cat"]
          },
          "G4"
         ]
        ]))
        .await;
    assert_eq!(
        text_at(&response, "/methodResponses/0/1/created/b4/size"),
        "45",
        "{response:?}"
    );
    assert_keyless(
        &response.0["methodResponses"][1],
        "a data source from the account's own upload",
    );
    assert_eq!(
        response.error_type_at(2),
        Some("invalidResultReference"),
        "{response:?}"
    );
    test.blob_expire_all().await;

    // Blob/get Example with Range and Encoding Errors
    let response = account.jmap_method_calls(json!([
            [
              "Blob/upload",
              {
                "accountId": account.id_string(),
                "create": {
                  "b1": {
                    "data": [
                      {
                        "data:asBase64": "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wZWQgb3ZlciB0aGUggYEgZG9nLg=="
                      }
                    ]
                  },
                  "b2": {
                    "data": [{ "data:asText": "hello world" }],
                    "type" : "text/plain"
                  }
                }
              },
              "S1"
            ],
            [
              "Blob/get",
              { "accountId": account.id_string(), "ids": ["#b1", "#b2"] },
              "G1"
            ],
            [
              "Blob/get",
              {
                "accountId": account.id_string(),
                "ids": ["#b1", "#b2"],
                "properties": ["data:asText", "size"]
              },
              "G2"
            ],
            [
              "Blob/get",
              {
                "accountId": account.id_string(),
                "ids": ["#b1", "#b2"],
                "properties": ["data:asBase64", "size"]
              },
              "G3"
            ],
            [
              "Blob/get",
              {
                "accountId": account.id_string(),
                "offset": 0,
                "length": 5,
                "ids": ["#b1", "#b2"]
              },
              "G4"
            ],
            [
              "Blob/get",
              {
                "accountId": account.id_string(),
                "offset": 20,
                "length": 100,
                "ids": ["#b1", "#b2"]
              },
              "G5"
            ]
          ])).await;
    for (pointer, expected) in [
        (
            "/methodResponses/1/1/list/0/data:asBase64",
            "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wZWQgb3ZlciB0aGUggYEgZG9nLg==",
        ),
        ("/methodResponses/1/1/list/1/data:asText", "hello world"),
        ("/methodResponses/2/1/list/0/isEncodingProblem", "true"),
        ("/methodResponses/2/1/list/1/data:asText", "hello world"),
        (
            "/methodResponses/3/1/list/0/data:asBase64",
            "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wZWQgb3ZlciB0aGUggYEgZG9nLg==",
        ),
        (
            "/methodResponses/3/1/list/1/data:asBase64",
            "aGVsbG8gd29ybGQ=",
        ),
        ("/methodResponses/4/1/list/0/data:asText", "The q"),
        ("/methodResponses/4/1/list/1/data:asText", "hello"),
        ("/methodResponses/5/1/list/0/isEncodingProblem", "true"),
        ("/methodResponses/5/1/list/0/isTruncated", "true"),
        ("/methodResponses/5/1/list/1/isTruncated", "true"),
    ] {
        assert_eq!(
            text_at(&response, pointer),
            expected,
            "Pointer {pointer:?} Response: {response:?}",
        );
    }
    test.blob_expire_all().await;

    // Email/import of an upload by creation id: the account's own, sealed
    let inbox_id = Id::from(INBOX_ID).to_string();
    let response = account
        .jmap_method_calls(json!([
          [
            "Blob/upload",
            {
              "accountId": account.id_string(),
              "create": {
                "m0": {
                  "data": [
                    {
                      "data:asText": concat!(
                          "From: bill@example.com\r\n",
                          "To: jdoe@example.com\r\n",
                          "Subject: Blob reference import\r\n",
                          "\r\n",
                          "Imported through a Blob/upload creation id."
                      )
                    }
                  ],
                  "type": "message/rfc822"
                }
              }
            },
            "U0"
          ],
          [
            "Email/import",
            {
              "accountId": account.id_string(),
              "emails": {
                "i0": { "blobId": "#m0", "mailboxIds": { (inbox_id.as_str()): true } }
              }
            },
            "I0"
          ],
          [
            "Email/import",
            {
              "accountId": account.id_string(),
              "emails": {
                "i1": { "blobId": "#missing", "mailboxIds": { (inbox_id.as_str()): true } }
              }
            },
            "I1"
          ]
        ]))
        .await;
    assert_eq!(response.name_at(1), "Email/import", "{response:?}");
    assert!(
        response
            .pointer("/methodResponses/1/1/created/i0/id")
            .and_then(|v| v.as_str())
            .is_some(),
        "{response:?}"
    );
    assert!(
        response
            .pointer("/methodResponses/1/1/notCreated")
            .is_none(),
        "{response:?}"
    );
    assert_eq!(
        response.error_type_at(2),
        Some("invalidResultReference"),
        "{response:?}"
    );
    test.blob_expire_all().await;

    // Blob/lookup
    let client = account.jmap_client().await;
    let blob_id = client
        .email_import(
            concat!(
                "From: bill@example.com\r\n",
                "To: jdoe@example.com\r\n",
                "Subject: TPS Report\r\n",
                "\r\n",
                "I'm going to need those TPS reports ASAP. ",
                "So, if you could do that, that'd be great."
            )
            .as_bytes()
            .to_vec(),
            [&Id::from(INBOX_ID).to_string()],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let response = account
        .jmap_method_call(
            "Blob/lookup",
            json!({
              "accountId": account.id_string(),
              "typeNames": ["Mailbox", "Thread", "Email"],
              "ids": [blob_id, "not-a-blob"]
            }),
        )
        .await;
    for pointer in [
        "/methodResponses/0/1/list/0/matchedIds/Email",
        "/methodResponses/0/1/list/0/matchedIds/Mailbox",
        "/methodResponses/0/1/list/0/matchedIds/Thread",
    ] {
        assert_eq!(
            response
                .pointer(pointer)
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or_default(),
            1,
            "Pointer {pointer:?} Response: {response:#?}",
        );
    }

    // Remove test data
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

/// `files::node::test` in key mode: folders behave as upstream; a file whose
/// content is the account's own upload is a raw read of a sealed source,
/// refused (5.7, R32), so a key account creates no file with content over
/// JMAP. The file is written over WebDAV instead (files are plaintext) and
/// upstream's JMAP steps on it run, followed by upstream's cases that
/// upload nothing (`files::node::test_options`).
pub async fn file_node(test: &TestServer) {
    println!("Running key-account File Storage tests...");
    let account = test.account("jdoe@example.com");

    // Obtain change id
    let change_id = account
        .jmap_get(
            MethodObject::FileNode,
            [FileNodeProperty::Id],
            Vec::<&str>::new(),
        )
        .await
        .state()
        .to_string();
    let response = account
        .jmap_changes(MethodObject::FileNode, &change_id)
        .await;
    assert_eq!(response.changes().next(), None);
    assert_eq!(response.new_state(), change_id);

    // Create test folders
    let response = account
        .jmap_create(
            MethodObject::FileNode,
            [
                json!({ "name": "Root Folder", "parentId": null }),
                json!({ "name": "Sub Folder", "parentId": "#i0" }),
                json!({ "name": "Sub-sub Folder", "parentId": "#i1" }),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let root_folder_id = response.created(0).id().to_string();
    let sub_folder_id = response.created(1).id().to_string();
    let sub_sub_folder_id = response.created(2).id().to_string();
    assert_eq!(
        account
            .jmap_changes(MethodObject::FileNode, change_id)
            .await
            .changes()
            .collect::<AHashSet<_>>(),
        [
            ChangeType::Created(&root_folder_id),
            ChangeType::Created(&sub_folder_id),
            ChangeType::Created(&sub_sub_folder_id)
        ]
        .into_iter()
        .collect::<AHashSet<_>>()
    );

    // A file from the account's own upload is refused
    let response = account
        .jmap_method_calls(json!([
         [
          "Blob/upload",
          {
           "accountId": account.id_string(),
           "create": { "hello": { "data": [{ "data:asText": "hello world" }] } }
          },
          "S4"
         ],
         [
          "FileNode/set",
          {
            "accountId": account.id_string(),
            "create": {
              "i0": {
                "name": "hello.txt",
                "parentId": &root_folder_id,
                "blobId": "#hello",
                "type": "text/plain",
              }
            }
          },
          "G4"
         ]
        ]))
        .await;
    assert_keyless(
        &response.0["methodResponses"][1],
        "a file from the account's own upload",
    );
    test.blob_expire_all().await;

    // Files are plaintext (spec 1.2): the same file written over WebDAV, and
    // upstream's steps on it over JMAP.
    account
        .webdav_client()
        .request_with_headers(
            "PUT",
            "/dav/file/jdoe%40example.com/Root%20Folder/hello.txt",
            [("content-type", "text/plain")],
            "hello world",
        )
        .await
        .with_status(StatusCode::CREATED);
    let file_id = account
        .jmap_get(
            MethodObject::FileNode,
            [FileNodeProperty::Id, FileNodeProperty::Name],
            Vec::<&str>::new(),
        )
        .await
        .list()
        .iter()
        .find(|node| node.text_field("name") == "hello.txt")
        .expect("the file written over WebDAV")
        .id()
        .to_string();
    let response = account
        .jmap_get(
            MethodObject::FileNode,
            [
                FileNodeProperty::Id,
                FileNodeProperty::BlobId,
                FileNodeProperty::Name,
                FileNodeProperty::ParentId,
                FileNodeProperty::Type,
                FileNodeProperty::Size,
            ],
            [&file_id],
        )
        .await;
    let blob_id = response.list()[0].blob_id().to_string();
    response.list()[0].assert_is_equal(json!({
        "id": &file_id,
        "name": "hello.txt",
        "parentId": &root_folder_id,
        "type": "text/plain",
        "size": 11,
        "blobId": &blob_id,
    }));
    assert_eq!(
        account
            .jmap_get(MethodObject::Blob, ["data:asText"], [&blob_id])
            .await
            .list()[0]
            .text_field("data:asText"),
        "hello world"
    );

    // Creating folders with invalid names or parent ids should fail
    let response = account
        .jmap_create(
            MethodObject::FileNode,
            [
                json!({ "name": "Sub Folder", "parentId": &root_folder_id }),
                json!({ "name": "Folder under file", "parentId": &file_id }),
                json!({ "name": "My/Sub/Folder" }),
                json!({ "name": "." }),
                json!({ "name": ".." }),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let err = response.not_created(0);
    assert_eq!(err.typ(), "alreadyExists");
    assert_eq!(err.text_field("existingId"), sub_folder_id.as_str());
    assert_eq!(
        response.not_created(1).description(),
        "Parent ID does not exist or is not a folder."
    );
    assert_eq!(
        response.not_created(2).description(),
        "Name contains a forbidden character."
    );
    for n in [3, 4] {
        assert_eq!(
            response.not_created(n).description(),
            "Name is reserved and cannot be used."
        );
    }

    // Circular folder references should fail
    let response = account
        .jmap_update(
            MethodObject::FileNode,
            [(&root_folder_id, json!({ "parentId": &sub_sub_folder_id }))],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    assert_eq!(
        response.not_updated(&root_folder_id).description(),
        "Circular reference in parent ids."
    );

    // Rename folder and file
    let response = account
        .jmap_update(
            MethodObject::FileNode,
            [
                (&sub_folder_id, json!({ "name": "Renamed Sub Folder" })),
                (&file_id, json!({ "name": "renamed-hello.txt" })),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    response.updated(&sub_folder_id);
    response.updated(&file_id);
    let response = account
        .jmap_get(
            MethodObject::FileNode,
            [
                FileNodeProperty::Id,
                FileNodeProperty::Name,
                FileNodeProperty::ParentId,
            ],
            [&sub_folder_id, &file_id],
        )
        .await;
    let list = response.list();
    assert_eq!(list.len(), 2);
    list[0].assert_is_equal(json!({
        "id": &sub_folder_id,
        "name": "Renamed Sub Folder",
        "parentId": &root_folder_id,
    }));
    list[1].assert_is_equal(json!({
        "id": &file_id,
        "name": "renamed-hello.txt",
        "parentId": &root_folder_id,
    }));

    // Destroying a folder with children should fail
    assert_eq!(
        account
            .jmap_destroy(
                MethodObject::FileNode,
                [&root_folder_id],
                Vec::<(&str, &str)>::new(),
            )
            .await
            .not_destroyed(&root_folder_id)
            .description(),
        "Cannot delete non-empty folder."
    );

    // Delete file and sub folders
    assert_eq!(
        account
            .jmap_destroy(
                MethodObject::FileNode,
                [&file_id],
                [("onDestroyRemoveChildren", true)],
            )
            .await
            .destroyed()
            .collect::<AHashSet<_>>(),
        [file_id.as_str()].into_iter().collect::<AHashSet<_>>()
    );
    assert_eq!(
        account
            .jmap_destroy(
                MethodObject::FileNode,
                [&root_folder_id],
                [("onDestroyRemoveChildren", true)],
            )
            .await
            .destroyed()
            .collect::<AHashSet<_>>(),
        [
            sub_sub_folder_id.as_str(),
            sub_folder_id.as_str(),
            root_folder_id.as_str()
        ]
        .into_iter()
        .collect::<AHashSet<_>>()
    );

    files::node::test_options(test).await;
}

/// The `calendar::*` modules and `principal::availability` in key mode:
/// JMAP calendars are not offered to key accounts in this release (release
/// 1 spec 9, plan 3), so every calendar method on a key account answers
/// `accountNotSupportedByMethod`, its availability is empty and nothing is
/// stored.
pub async fn calendars(test: &TestServer) {
    println!("Running key-account Calendar tests...");
    let account = test.account("jdoe@example.com");
    let id = account.id_string();
    for (method, args) in [
        ("Calendar/get", json!({ "accountId": id, "ids": null })),
        (
            "Calendar/set",
            json!({ "accountId": id, "create": { "c": { "name": "Test Calendar" } } }),
        ),
        ("CalendarEvent/get", json!({ "accountId": id, "ids": null })),
        ("CalendarEvent/query", json!({ "accountId": id })),
        (
            "CalendarEvent/set",
            json!({ "accountId": id, "create": { "e": {
                "@type": "Event",
                "title": "Test",
                "start": "2025-01-01T09:00:00",
                "duration": "PT1H",
                "calendarIds": { "a": true }
            } } }),
        ),
        (
            "CalendarEventNotification/get",
            json!({ "accountId": id, "ids": null }),
        ),
        (
            "ParticipantIdentity/get",
            json!({ "accountId": id, "ids": null }),
        ),
    ] {
        let reply = account.jmap_method_call(method, args).await;
        assert_eq!(
            reply.error_type_at(0),
            Some("accountNotSupportedByMethod"),
            "{method}: {reply:?}"
        );
    }
    // A key account holds no JMAP-visible calendar, so its availability is
    // empty.
    let reply = account
        .jmap_method_call(
            "Principal/getAvailability",
            json!({
                "accountId": id,
                "id": id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            }),
        )
        .await;
    reply.list_array().assert_is_equal(json!([]));
    test.assert_is_empty().await;
}

/// `principal::get::test` in key mode: the session object and the principal
/// objects of key accounts do not advertise JMAP calendars (release 1 spec
/// 9); everything else, and the plain Sales group's principal, are
/// upstream's.
pub async fn principal_get(test: &TestServer) {
    println!("Running key-account Principal get/query tests...");
    let john = test.account("jdoe@example.com");
    let jane = test.account("jane.smith@example.com");
    let bill = test.account("bill@example.com");
    let sales = test.account("sales@example.com");
    let john_id = john.id_string();
    let jane_id = jane.id_string();
    let bill_id = bill.id_string();
    let sales_id = sales.id_string();

    // Validate session object capabilities: upstream's object, minus the
    // account's calendar capabilities (release 1 spec 9).
    let response = john.jmap_session_object().await.into_inner();
    let application_server_key =
        response["capabilities"]["urn:ietf:params:jmap:webpush-vapid"]["applicationServerKey"]
            .clone();
    response.assert_is_equal(json!({
      "capabilities": {
        "urn:ietf:params:jmap:core": {
          "maxSizeUpload": 50000000,
          "maxConcurrentUpload": 4,
          "maxSizeRequest": 10000000,
          "maxConcurrentRequests": 4,
          "maxCallsInRequest": 16,
          "maxObjectsInGet": 100000,
          "maxObjectsInSet": 100000,
          "collationAlgorithms": [
            "i;ascii-numeric",
            "i;ascii-casemap",
            "i;unicode-casemap"
          ]
        },
        "urn:ietf:params:jmap:mail": {},
        "urn:ietf:params:jmap:calendars": {},
        "urn:ietf:params:jmap:calendars:parse": {},
        "urn:ietf:params:jmap:contacts": {},
        "urn:ietf:params:jmap:contacts:parse": {},
        "urn:ietf:params:jmap:emailpush": {},
        "urn:ietf:params:jmap:filenode": {},
        "urn:ietf:params:jmap:principals": {},
        "urn:ietf:params:jmap:principals:availability": {},
        "urn:ietf:params:jmap:submission": {},
        "urn:ietf:params:jmap:vacationresponse": {},
        "urn:ietf:params:jmap:sieve": {
          "implementation": "Stalwart v1.0.0"
        },
        "urn:ietf:params:jmap:blob": {},
        "urn:ietf:params:jmap:quota": {},
        "urn:ietf:params:jmap:webpush-vapid": {
          "applicationServerKey": application_server_key
        },
        "urn:ietf:params:jmap:websocket": {
          "url": "wss://127.0.0.1:8899/jmap/ws",
          "supportsPush": true
        }
      },
      "accounts": {
        john_id: {
          "name": "jdoe@example.com",
          "isPersonal": true,
          "isReadOnly": false,
          "accountCapabilities": {
            "urn:ietf:params:jmap:mail": {
              "maxMailboxesPerEmail": null,
              "maxMailboxDepth": 10,
              "maxSizeMailboxName": 255,
              "maxSizeAttachmentsPerEmail": 50000000,
              // Recorded deferred finding: the from/to/subject/sentAt sorts
              // are advertised although spec 8 refuses them for a key
              // account (no per-account trimming); pinned as served today.
              "emailQuerySortOptions": [
                "receivedAt",
                "size",
                "from",
                "to",
                "subject",
                "sentAt",
                "hasKeyword",
                "allInThreadHaveKeyword",
                "someInThreadHaveKeyword"
              ],
              "mayCreateTopLevelMailbox": true
            },
            "urn:ietf:params:jmap:submission": {
              "maxDelayedSend": 2592000,
              "submissionExtensions": {
                "FUTURERELEASE": [],
                "SIZE": [],
                "DSN": [],
                "DELIVERYBY": [],
                "MT-PRIORITY": [
                  "MIXER"
                ],
                "REQUIRETLS": []
              }
            },
            "urn:ietf:params:jmap:vacationresponse": {},
            "urn:ietf:params:jmap:contacts": {
              "maxAddressBooksPerCard": null,
              "mayCreateAddressBook": true
            },
            "urn:ietf:params:jmap:contacts:parse": {},
            "urn:ietf:params:jmap:emailpush": {},
            "urn:ietf:params:jmap:websocket": {},
            "urn:ietf:params:jmap:sieve": {
              "maxSizeScriptName": 512,
              "maxSizeScript": 102400,
              "maxNumberScripts": 100,
              "maxNumberRedirects": 1,
              "sieveExtensions": [
                "body",
                "comparator-elbonia",
                "comparator-i;ascii-casemap",
                "comparator-i;ascii-numeric",
                "comparator-i;octet",
                "convert",
                "copy",
                "date",
                "duplicate",
                "editheader",
                "enclose",
                "encoded-character",
                "enotify",
                "envelope",
                "envelope-deliverby",
                "envelope-dsn",
                "environment",
                "ereject",
                "extlists",
                "extracttext",
                "fcc",
                "fileinto",
                "foreverypart",
                "ihave",
                "imap4flags",
                "imapsieve",
                "include",
                "index",
                "mailbox",
                "mailboxid",
                "mboxmetadata",
                "mime",
                "redirect-deliverby",
                "redirect-dsn",
                "regex",
                "reject",
                "relational",
                "replace",
                "servermetadata",
                "spamtest",
                "spamtestplus",
                "special-use",
                "subaddress",
                "vacation",
                "vacation-seconds",
                "variables",
                "virustest"
              ],
              "notificationMethods": [
                "mailto"
              ],
              "externalLists": null
            },
            "urn:ietf:params:jmap:blob": {
              "maxSizeBlobSet": 7499488,
              "maxDataSources": 16,
              "supportedTypeNames": [
                "Email",
                "Thread",
                "SieveScript"
              ],
              "supportedDigestAlgorithms": [
                "sha",
                "sha-256",
                "sha-512"
              ]
            },
            "urn:ietf:params:jmap:quota": {},
            "urn:ietf:params:jmap:principals": {
              "currentUserPrincipalId": john_id
            },
            "urn:ietf:params:jmap:principals:availability": {
              "maxAvailabilityDuration": "P52W1D",
            },
            "urn:ietf:params:jmap:filenode": {
              "maxFileNodeDepth": null,
              "maxSizeFileNodeName": 255,
              "forbiddenNameChars": "/<>:\"\\|?*",
              "forbiddenNodeNames": [
                ".",
                "..",
                "CON",
                "PRN",
                "AUX",
                "NUL",
                "COM0",
                "COM1",
                "COM2",
                "COM3",
                "COM4",
                "COM5",
                "COM6",
                "COM7",
                "COM8",
                "COM9",
                "LPT0",
                "LPT1",
                "LPT2",
                "LPT3",
                "LPT4",
                "LPT5",
                "LPT6",
                "LPT7",
                "LPT8",
                "LPT9"
              ],
              "fileNodeQuerySortOptions": [
                "name",
                "size",
                "nodeType"
              ],
              "mayCreateTopLevelFileNode": true,
              "caseInsensitiveNames": false,
              "webTrashUrl": null,
              "webUrlTemplate": null,
              "webWriteUrlTemplate": null
            },
            "urn:ietf:params:jmap:mail:share": {},
            "urn:stalwart:jmap": {}
          }
        }
      },
      "primaryAccounts": {
        "urn:ietf:params:jmap:mail": john_id,
        "urn:ietf:params:jmap:submission": john_id,
        "urn:ietf:params:jmap:vacationresponse": john_id,
        "urn:ietf:params:jmap:contacts": john_id,
        "urn:ietf:params:jmap:contacts:parse": john_id,
        "urn:ietf:params:jmap:emailpush": john_id,
        "urn:ietf:params:jmap:websocket": john_id,
        "urn:ietf:params:jmap:sieve": john_id,
        "urn:ietf:params:jmap:blob": john_id,
        "urn:ietf:params:jmap:quota": john_id,
        "urn:ietf:params:jmap:principals": john_id,
        "urn:ietf:params:jmap:principals:availability": john_id,
        "urn:ietf:params:jmap:filenode": john_id,
        "urn:ietf:params:jmap:mail:share": john_id,
        "urn:stalwart:jmap": john_id
      },
      "username": "jdoe@example.com",
      "apiUrl": "https://127.0.0.1:8899/jmap/",
      "downloadUrl":
      "https://127.0.0.1:8899/jmap/download/{accountId}/{blobId}/{name}?accept={type}",
      "uploadUrl":
      "https://127.0.0.1:8899/jmap/upload/{accountId}/",
      "eventSourceUrl":
      "https://127.0.0.1:8899/jmap/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
      "state": response.text_field("state")
    }));

    // Obtain principal ids for Jane, Bill and the sales group
    for (filter, expected) in [
        (("email", "john.doe@example.com"), john_id),
        (("name", "bill@example.com"), bill_id),
        (("text", "sales group"), sales_id),
    ] {
        let response = john
            .jmap_query(
                MethodObject::Principal,
                [filter],
                ["name"],
                Vec::<(&str, &str)>::new(),
            )
            .await;
        assert_eq!(response.ids().collect::<Vec<_>>(), [expected]);
    }
    let response = john
        .jmap_query(
            MethodObject::Principal,
            [("accountIds", [jane_id])],
            ["name"],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    assert_eq!(response.ids().collect::<Vec<_>>(), [jane_id]);

    // Validate principal contents
    let response = john
        .jmap_get(
            MethodObject::Principal,
            [
                PrincipalProperty::Id,
                PrincipalProperty::Type,
                PrincipalProperty::Email,
                PrincipalProperty::Description,
                PrincipalProperty::Name,
                PrincipalProperty::Timezone,
                PrincipalProperty::Capabilities,
                PrincipalProperty::Accounts,
            ],
            [john_id, jane_id, bill_id, sales_id],
        )
        .await;
    let list = response.list();
    assert_eq!(list.len(), 4);
    for (item, (id, email, description)) in list.iter().zip([
        (john_id, "jdoe@example.com", "John Doe"),
        (jane_id, "jane.smith@example.com", "Jane Smith"),
        (bill_id, "bill@example.com", "Bill Foobar"),
    ]) {
        item.assert_is_equal(json!({
          "id": id,
          "type": "individual",
          "email": email,
          "description": description,
          "name": email,
          "timezone": null,
          "capabilities": {
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:contacts": {},
            "urn:ietf:params:jmap:filenode": {},
            "urn:ietf:params:jmap:principals": {}
          },
          "accounts": {
            id: {
              "urn:ietf:params:jmap:mail": {},
              "urn:ietf:params:jmap:contacts": {},
              "urn:ietf:params:jmap:filenode": {},
              "urn:ietf:params:jmap:principals": {},
              "urn:ietf:params:jmap:principals:owner": {
                "accountIdForPrincipal": id,
                "principalId": id
              }
            }
          }
        }));
    }
    list[3].assert_is_equal(json!({
      "id": sales_id,
      "type": "group",
      "email": "sales@example.com",
      "description": "Sales Group",
      "name": "sales@example.com",
      "timezone": null,
      "capabilities": {
        "urn:ietf:params:jmap:mail": {},
        "urn:ietf:params:jmap:contacts": {},
        "urn:ietf:params:jmap:calendars": {},
        "urn:ietf:params:jmap:filenode": {},
        "urn:ietf:params:jmap:principals": {}
      },
      "accounts": {
        sales_id: {
          "urn:ietf:params:jmap:mail": {},
          "urn:ietf:params:jmap:contacts": {},
          "urn:ietf:params:jmap:calendars": {
            "accountId": sales_id,
            "mayGetAvailability": true,
            "mayShareWith": true,
            "calendarAddress": "mailto:sales@example.com"
          },
          "urn:ietf:params:jmap:filenode": {},
          "urn:ietf:params:jmap:principals": {},
          "urn:ietf:params:jmap:principals:owner": {
            "accountIdForPrincipal": sales_id,
            "principalId": sales_id
          }
        }
      }
    }));
}
