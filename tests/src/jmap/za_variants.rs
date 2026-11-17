/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account JMAP variants (mail spec 11), run with `ZA_KEY_ACCOUNTS=1`.
//! Keyed search and threading use upstream's results (release 7 spec 9.3).
//! The remaining differences cover keyless mail calls, cross-account copy,
//! sharing, raw sealed sources and push content. The admin and Sales group
//! stay plain. The calendar block (release 6 spec 9.4) runs upstream's
//! `calendar::calendars`, `instance` and `alarm` bodies for key accounts and
//! mirrors `event`, `identity`, `acl` and `principal::availability` where a
//! ruling changes an assertion; event notifications are refused (6.11).

use crate::{
    jmap::{
        calendar::{
            self,
            event::{
                assert_eq_ignoring_updated, test_jscalendar_1, test_jscalendar_2,
                test_jscalendar_3, test_jscalendar_4,
            },
        },
        files,
        mail::{
            acl::{assert_account_not_found, assert_forbidden, assert_from_account_not_found},
            changes::{LogAction, ParseState},
            query as upstream_query,
            query_changes::QueryChanges,
            thread_merge::{build_messages, test_1, test_2, test_3},
        },
    },
    utils::{
        account::Account,
        jmap::{ChangeType, IntoJmapSet, JmapResponse, JmapUtils},
        push::spawn_mock_push_server,
        server::{DestroyAllMailboxes, TestServer},
        smtp::SmtpConnection,
        za::{
            Keyless, finish_bulk_delivery_indexing, za_grant_token, za_index_current, za_post,
            za_revoke_grant,
        },
    },
    za::{STRONG, user_permissions},
};
use ::email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, Mailbox, TRASH_ID},
    message::{
        delivery::SenderAuthentication,
        ingest::{EmailIngest, IngestEmail, IngestSource},
    },
    push::{EmailPush, Urgency},
};
use ahash::{AHashMap, AHashSet};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use calcard::jscalendar::JSCalendarProperty;
use common::{
    auth::{AccessToken, oauth::scope::OAuthScopes},
    storage::index::ObjectIndexBuilder,
    za::ZA_MAIL_KEYLESS,
};
use dav_proto::Depth;
use groupware::{DavResourceName, cache::GroupwareCache};
use hyper::StatusCode;
use jmap_client::{
    Error,
    core::{
        error::{MethodError, MethodErrorType},
        query,
        set::SetErrorType,
    },
    email::{self, Property, query::Filter},
    mailbox::{self, Role},
    principal::ACL,
    push_subscription::Keys,
};
use jmap_proto::{
    method::query::Filter as ProtoFilter,
    object::{
        calendar::CalendarProperty, email::EmailFilter, file_node::FileNodeProperty,
        participant_identity::ParticipantIdentityProperty, principal::PrincipalProperty,
        push_subscription::EmailPushProperty, share_notification::ShareNotificationProperty,
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
    rand::{self, RngExt},
    write::BatchBuilder,
};
use tokio::sync::mpsc;
use types::{
    collection::{Collection, SyncCollection},
    field::PrincipalField,
    id::Id,
    keyword::Keyword,
};

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

impl CorpusMessage {
    /// 1.0 stores a keyword in lower case (`Keyword::other`), so the corpus
    /// is matched case-insensitively.
    fn has_keyword(&self, keyword: &str) -> bool {
        self.keywords.contains(&keyword.to_ascii_lowercase())
    }

    fn thread_has_keyword(
        &self,
        corpus: &AHashMap<String, CorpusMessage>,
        keyword: &str,
        all: bool,
    ) -> bool {
        let mut members = corpus.values().filter(|m| m.thread_id == self.thread_id);
        if all {
            members.all(|m| m.has_keyword(keyword))
        } else {
            members.any(|m| m.has_keyword(keyword))
        }
    }
}

/// Every imported message, read independently of the metadata queries below.
/// The query inventory must account for the complete cached document inventory;
/// each Email/get chunk must return precisely its requested IDs.
async fn load_corpus(test: &TestServer, account: &Account) -> AHashMap<String, CorpusMessage> {
    let cached = test
        .server
        .get_cached_messages(account.id().document_id())
        .await
        .unwrap();
    let documents = cached
        .emails
        .iter()
        .map(|message| message.document_id())
        .collect::<AHashSet<_>>();
    // mail/query.rs::create selects 978 artwork records in 100 year groups.
    assert_eq!(documents.len(), 978, "complete upstream query corpus");
    let reply = email_query(
        account,
        None,
        json!([{ "property": "receivedAt" }]),
        json!({ "calculateTotal": true }),
    )
    .await;
    let all = ids(&reply, "corpus inventory");
    assert_eq!(all.len(), documents.len(), "{reply:?}");
    assert_eq!(reply.0["methodResponses"][0][1]["total"], all.len());
    assert_eq!(
        all.iter()
            .map(|id| Id::from_str(id).unwrap().document_id())
            .collect::<AHashSet<_>>(),
        documents,
        "the query inventory must cover every imported document"
    );

    let mut corpus = AHashMap::default();
    let mut message_ids = AHashSet::new();
    let mut year_threads = AHashMap::default();
    let mut thread_years = AHashMap::default();
    for chunk in all.chunks(100) {
        let reply = account
            .jmap_method_call(
                "Email/get",
                json!({
                    "accountId": account.id_string(),
                    "ids": chunk,
                    "properties": [
                        "id", "receivedAt", "size", "keywords", "mailboxIds", "threadId",
                        "messageId", "subject", "references",
                    ],
                }),
            )
            .await;
        assert_eq!(reply.name_at(0), "Email/get", "{reply:?}");
        assert_eq!(reply.not_found().count(), 0, "{reply:?}");
        assert_eq!(reply.list().len(), chunk.len(), "{reply:?}");
        assert_eq!(
            reply
                .list()
                .iter()
                .map(|email| email.text_field("id"))
                .collect::<AHashSet<_>>(),
            chunk.iter().map(String::as_str).collect::<AHashSet<_>>(),
            "{reply:?}"
        );
        for item in reply.list() {
            let keys = |field: &str| {
                item[field]
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect::<AHashSet<_>>()
            };
            let received_at = DateTime::parse_rfc3339(item.text_field("receivedAt"))
                .unwrap()
                .to_timestamp();
            let thread_id = item.text_field("threadId").to_string();
            // The fixture uses the artwork year for receivedAt, Subject,
            // References and one mailbox. Independently validate the mapped
            // partition: all of a year, and no other year, belongs to a thread.
            assert_eq!(item["subject"], format!("Year {received_at}"));
            assert_eq!(item["references"], json!([received_at.to_string()]));
            assert_eq!(item["messageId"].as_array().unwrap().len(), 1);
            assert!(message_ids.insert(item["messageId"][0].as_str().unwrap().to_string()));
            if let Some(previous) = year_threads.insert(received_at, thread_id.clone()) {
                assert_eq!(previous, thread_id, "split fixture year {received_at}");
            }
            if let Some(previous) = thread_years.insert(thread_id.clone(), received_at) {
                assert_eq!(previous, received_at, "merged distinct fixture years");
            }
            let mailboxes = keys("mailboxIds");
            assert!(mailboxes.contains(&Id::new(received_at as u64).to_string()));
            assert_eq!(mailboxes.len(), 2);
            assert!(
                corpus
                    .insert(
                        item.text_field("id").to_string(),
                        CorpusMessage {
                            received_at,
                            size: item["size"].as_u64().unwrap(),
                            keywords: keys("keywords"),
                            mailboxes,
                            thread_id,
                        },
                    )
                    .is_none(),
                "duplicate corpus id: {item}"
            );
        }
    }
    assert_eq!(corpus.len(), all.len());
    assert_eq!(message_ids.len(), all.len());
    assert_eq!(year_threads.len(), 100);
    assert_eq!(thread_years.len(), 100);
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

/// The receivedAt-only order is (arrival, document id); descending reverses
/// that order. Collapse retains its first matching member of each mapped thread.
fn received_order(
    corpus: &AHashMap<String, CorpusMessage>,
    mut matching: Vec<String>,
    ascending: bool,
    collapse: bool,
) -> Vec<String> {
    matching.sort_unstable_by_key(|id| {
        (
            corpus[id].received_at,
            Id::from_str(id).unwrap().document_id(),
        )
    });
    if !ascending {
        matching.reverse();
    }
    if collapse {
        let mut seen = AHashSet::new();
        matching.retain(|id| seen.insert(corpus[id].thread_id.as_str()));
    }
    matching
}

/// `mail::query::test` in key mode: upstream's corpus, assertions and
/// options, with the index made current by a keyed content request in place
/// of the queue wait, plus the independent served-metadata corpus oracle.
/// A keyless token is refused on `Email/query` and `SearchSnippet/get`
/// (release 7 spec 6.2).
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
                // 1.0's `Mailbox` keeps its role in `flags` (0: none).
                .custom(ObjectIndexBuilder::<(), _>::new().with_changes(Mailbox {
                    uid_validity: 0,
                    ..Mailbox::new(format!("Mailbox {mailbox_id}"))
                }))
                .unwrap();
        }
        server
            .core
            .storage
            .data
            .write_batch(&mut batch)
            .await
            .unwrap();

        println!("Inserting JMAP Mail query test messages...");
        upstream_query::create(test, account).await;

        // The ids-only index documents still use the task queue.
        test.wait_for_tasks().await;
    }
    za_index_current(account).await;
    let corpus = load_corpus(test, account).await;
    query_refusals(account).await;
    query_served(account, &corpus).await;
    query_options(account, &corpus).await;

    upstream_query::query(&client, true).await;
    upstream_query::query_options(&client).await;
    keyless_query_refused(test, account, corpus.keys().next().unwrap()).await;

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

/// `cc` is still an unsupported sort for both account modes. The content
/// filters and other content sorts are served and checked by upstream's cases.
async fn query_refusals(account: &Account) {
    let reply = email_query(account, None, json!([{ "property": "cc" }]), json!({})).await;
    assert_eq!(reply.name_at(0), "error", "{reply:?}");
    assert_eq!(reply.error_type_at(0), Some("unsupportedSort"), "{reply:?}");
    assert!(
        reply.0["methodResponses"][0][1]["description"]
            .as_str()
            .is_some_and(|description| description.contains("cc")),
        "{reply:?}"
    );
}

/// The served filters, each against the corpus, in `receivedAt` order, and
/// the served sorts.
async fn query_served(account: &Account, corpus: &AHashMap<String, CorpusMessage>) {
    type Matcher<'a> = Box<dyn Fn(&CorpusMessage) -> bool + 'a>;
    let and = |conditions: Vec<Value>| json!({ "operator": "AND", "conditions": conditions });
    let has = |keyword: &'static str| move |m: &CorpusMessage| m.has_keyword(keyword);
    let mailbox_1768 = Id::new(1768u64).to_string();
    let others = [1991u64, 1870, 2011, 1951, 1902, 1808, 1963]
        .into_iter()
        .map(|id| Id::new(id).to_string())
        .collect::<Vec<_>>();
    let mut sizes = corpus.values().map(|m| m.size).collect::<Vec<_>>();
    sizes.sort_unstable();
    let median = sizes[sizes.len() / 2];
    let some_thread = corpus
        .values()
        .find(|m| m.received_at == 1768)
        .unwrap()
        .thread_id
        .clone();
    assert!(
        corpus
            .values()
            .filter(|m| m.thread_id == some_thread)
            .count()
            > 1
    );

    let cases: Vec<(&str, Value, Matcher<'_>)> = vec![
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
            Box::new(|m| !m.has_keyword("artist")),
        ),
        (
            "NOT hasKeyword",
            json!({ "operator": "NOT", "conditions": [{ "hasKeyword": "N0" }] }),
            Box::new(|m| !m.has_keyword("N0")),
        ),
        (
            "OR hasKeyword",
            json!({ "operator": "OR", "conditions": [
                { "hasKeyword": "Bronze" }, { "hasKeyword": "artist" }
            ] }),
            Box::new(|m| m.has_keyword("Bronze") || m.has_keyword("artist")),
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
            Box::new(|m| m.thread_has_keyword(corpus, "N", true) && m.received_at < 1800),
        ),
        (
            "noneInThreadHaveKeyword",
            and(vec![
                json!({ "noneInThreadHaveKeyword": "N" }),
                json!({ "after": date(1995) }),
            ]),
            Box::new(|m| !m.thread_has_keyword(corpus, "N", false) && m.received_at >= 1995),
        ),
        (
            "someInThreadHaveKeyword",
            and(vec![
                json!({ "someInThreadHaveKeyword": "Bronze" }),
                json!({ "before": date(1878) }),
            ]),
            Box::new(|m| m.thread_has_keyword(corpus, "Bronze", false) && m.received_at < 1878),
        ),
        (
            "hasKeyword in a range",
            and(vec![
                json!({ "after": date(1786) }),
                json!({ "before": date(1840) }),
                json!({ "hasKeyword": "T" }),
            ]),
            Box::new(|m| m.has_keyword("T") && (1786..1840).contains(&m.received_at)),
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
                assert_eq!(
                    ids(&reply, name),
                    received_order(corpus, expected.clone(), ascending, collapse),
                    "{name} (ascending {ascending}, collapseThreads {collapse})"
                );
            }
        }
    }

    // Size comes from Email/get, independently of the query's sort result.
    let all = corpus.keys().cloned().collect::<AHashSet<_>>();
    for ascending in [true, false] {
        let reply = email_query(
            account,
            None,
            json!([{ "property": "size", "isAscending": ascending }]),
            json!({}),
        )
        .await;
        let found = ids(&reply, "size");
        assert_eq!(found.len(), corpus.len());
        assert_eq!(found.iter().cloned().collect::<AHashSet<_>>(), all);
        assert!(
            found.windows(2).all(|w| if ascending {
                corpus[&w[0]].size <= corpus[&w[1]].size
            } else {
                corpus[&w[0]].size >= corpus[&w[1]].size
            }),
            "not in size order (ascending {ascending})"
        );
    }

    // Thread-keyword ordering uses all mapped members, while hasKeyword
    // uses the message itself. Both are checked against readback metadata.
    for (thread_property, keyword, filter, from, to, match_all) in [
        (
            "allInThreadHaveKeyword",
            "N",
            json!({ "before": date(1800) }),
            0,
            1800,
            true,
        ),
        (
            "someInThreadHaveKeyword",
            "Bronze",
            and(vec![
                json!({ "after": date(1875) }),
                json!({ "before": date(1878) }),
            ]),
            1875,
            1878,
            false,
        ),
    ] {
        let expected = corpus
            .iter()
            .filter(|(_, m)| (from..to).contains(&m.received_at))
            .map(|(id, _)| id.clone())
            .collect::<AHashSet<_>>();
        for ascending in [true, false] {
            for property in [thread_property, "hasKeyword"] {
                let reply = email_query(
                    account,
                    Some(filter.clone()),
                    json!([
                        { "property": property, "keyword": keyword, "isAscending": ascending },
                        { "property": "receivedAt" },
                    ]),
                    json!({}),
                )
                .await;
                let found = ids(&reply, property);
                assert_eq!(found.len(), expected.len());
                assert_eq!(
                    found.iter().cloned().collect::<AHashSet<_>>(),
                    expected,
                    "{property} {keyword} membership"
                );
                let order = found
                    .iter()
                    .map(|id| {
                        let m = &corpus[id];
                        let has = if property == "hasKeyword" {
                            m.has_keyword(keyword)
                        } else {
                            m.thread_has_keyword(corpus, keyword, match_all)
                        };
                        // Ascending sorts false before true; receivedAt is
                        // ascending in both directions of the first comparator.
                        (if ascending { has } else { !has }, m.received_at)
                    })
                    .collect::<Vec<_>>();
                assert!(order.iter().any(|(has, _)| *has));
                assert!(order.iter().any(|(has, _)| !*has));
                assert!(
                    order.windows(2).all(|w| w[0] <= w[1]),
                    "{property} {keyword} (ascending {ascending}): not in keyword/receivedAt order"
                );
            }
        }
    }
}

/// Upstream's paging options (position, anchor, anchor offset, limit, with
/// and without collapsed threads), on the `receivedAt` order.
async fn query_options(account: &Account, corpus: &AHashMap<String, CorpusMessage>) {
    let sort = json!([{ "property": "receivedAt" }]);
    for collapse in [false, true] {
        let all = received_order(corpus, corpus.keys().cloned().collect(), true, collapse);
        let complete = email_query(
            account,
            None,
            sort.clone(),
            json!({ "collapseThreads": collapse }),
        )
        .await;
        assert_eq!(ids(&complete, "all"), all, "collapseThreads {collapse}");
        let len = all.len();
        // The mapped corpus has 100 threads; keep an interior anchor in
        // collapsed mode and the old position-100 anchor without collapse.
        let anchor = if collapse { len / 2 } else { 100 };
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
                json!({ "anchor": &all[anchor], "anchorOffset": 0, "limit": 10 }),
                slice(anchor, anchor + 10),
            ),
            (
                json!({ "anchor": &all[anchor], "anchorOffset": 10, "limit": 10 }),
                slice(anchor + 10, anchor + 20),
            ),
            (
                json!({ "anchor": &all[anchor], "anchorOffset": -10, "limit": 10 }),
                slice(anchor - 10, anchor),
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

async fn keyless_query_refused(test: &TestServer, account: &Account, email_id: &str) {
    let (grant_id, token) = za_grant_token(&test.server, account.name(), OAuthScopes::ALL).await;
    let mut bearer = Keyless::Bearer(token);
    let account_id = account.id_string();
    let filter = json!({ "text": "study" });
    for (method, args, field) in [
        ("Email/query", json!({ "accountId": account_id }), "ids"),
        (
            "Email/query",
            json!({ "accountId": account_id, "filter": filter }),
            "ids",
        ),
        (
            "SearchSnippet/get",
            json!({ "accountId": account_id, "filter": filter, "emailIds": [email_id] }),
            "list",
        ),
    ] {
        assert_keyless(&bearer.call(account, method, args.clone()).await, method);
        let reply = account.jmap_method_call(method, args).await;
        assert_eq!(reply.name_at(0), method, "{reply:?}");
        assert!(
            reply.0["methodResponses"][0][1][field].is_array(),
            "{reply:?}"
        );
    }
    za_revoke_grant(&test.server, grant_id).await;
}

/// `mail::search_snippet::test` in key mode, with a keyed content-current
/// check and the keyless refusal (release 7 spec 6.2).
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

    za_index_current(account).await;
    let can_stem = true;

    // Run tests
    for (filter, email_name, snippet_subject, snippet_preview) in [
        (
            query::Filter::or(vec![
                query::Filter::or(vec![Filter::subject("friend"), Filter::subject("help")]),
                query::Filter::or(vec![Filter::body("secret"), Filter::body("call")]),
            ]),
            "text_plain",
            Some("<mark>Help</mark> a <mark>friend</mark> from Abidjan Côte d'Ivoire"),
            Some(concat!(
                "d'Ivoire. He <mark>secretly</mark> <mark>called</mark> me on his bedside ",
                "and told me that he has a sum of $7.5M (Seven Million five Hundred Thousand",
                " Dollars) left in a suspense account in a local bank here in Abidjan Côte ",
                "d'Ivoire, that he used my name a"
            )),
        ),
        (
            Filter::text("côte").into(),
            "text_plain",
            Some("Help a friend from Abidjan <mark>Côte</mark> d'Ivoire"),
            Some(concat!(
                "in Abidjan <mark>Côte</mark> d'Ivoire. He secretly called me on ",
                "his bedside and told me that he has a sum of $7.5M (Seven ",
                "Million five Hundred Thousand Dollars) left in a suspense ",
                "account in a local bank here in Abidjan <mark>Côte</mark> d'Ivoire, that "
            )),
        ),
        (
            Filter::text("\"your country\"").into(),
            "text_plain",
            None,
            Some(concat!(
                "over to <mark>your</mark> <mark>country</mark> to further my education and ",
                "to secure a residential permit for me in <mark>your</mark> <mark>country",
                "</mark>. Moreover, I am willing to offer you 30 percent of the total sum as ",
                "compensation for your effort inp",
            )),
        ),
        (
            Filter::text("overseas").into(),
            "text_plain",
            None,
            Some("nominated account <mark>overseas</mark>. "),
        ),
        (
            Filter::text("孫子兵法").into(),
            "text_plain_chinese",
            Some("<mark>孫</mark><mark>子</mark><mark>兵法</mark>"),
            Some(concat!(
                "&lt;&quot;<mark>孫</mark><mark>子</mark><mark>兵法</mark>：&quot;&gt; ",
                "<mark>孫</mark><mark>子</mark>曰：兵者，國之大事，死生之地，存亡之道，",
                "不可不察也。 <mark>孫</mark><mark>子</mark>曰：凡用兵之法，馳車千駟"
            )),
        ),
        (
            Filter::text("cia").into(),
            "subpart",
            None,
            Some("shouldn't the <mark>CIA</mark> have something like that? Bill"),
        ),
        (
            Filter::text("frösche").into(),
            "html",
            Some("Die Hasen und die <mark>Frösche</mark>"),
            Some(concat!(
                "und die <mark>Frösche</mark> Die Hasen klagten einst über ihre mißliche Lage; ",
                "&quot;wir leben&quot;, sprach ein Redner, &quot;in steter Furcht vor Menschen und ",
                "Tieren, eine Beute der Hunde, der Adler, ja fast aller Raubtiere! ",
                "Unsere stete Angst ist är"
            )),
        ),
        (
            Filter::text(if can_stem {
                "es:galería vasto biblioteca"
            } else {
                "es:galería vastos biblioteca"
            })
            .into(),
            "mixed",
            Some("<mark>Biblioteca</mark> de Babel"),
            Some(concat!(
                "llaman la *<mark>Biblioteca</mark>*) se compone de un número indefinido, y tal ",
                "vez infinito, de <mark>galerías</mark> hexagonales, con <mark>vastos</mark> ",
                "pozos de ventilación en el medio, cercados por barandas bajísimas. Desde ",
                "cualquier hexágono se "
            )),
        ),
    ] {
        let mut request = client.build();
        let result_ref = request
            .query_email()
            .filter(filter.clone())
            .result_reference();
        request
            .get_search_snippet()
            .filter(filter)
            .email_ids_ref(result_ref);
        let response = request
            .send()
            .await
            .unwrap()
            .unwrap_method_responses()
            .pop()
            .unwrap()
            .unwrap_get_search_snippet()
            .unwrap();
        let snippet = response
            .snippet(email_ids.get(email_name).unwrap())
            .unwrap_or_else(|| panic!("No snippet for {}", email_name));
        assert_eq!(snippet_subject, snippet.subject());
        assert_eq!(snippet_preview, snippet.preview());
        assert!(
            snippet.preview().map_or(0, |p| p.len()) <= 255,
            "len: {}",
            snippet.preview().map_or(0, |p| p.len())
        );
    }

    let (grant_id, token) = za_grant_token(&test.server, account.name(), OAuthScopes::ALL).await;
    let mut bearer = Keyless::Bearer(token);
    let response = bearer
        .call(
            account,
            "SearchSnippet/get",
            json!({
                "accountId": account.id_string(),
                "filter": { "text": "côte" },
                "emailIds": [&email_ids["text_plain"]],
            }),
        )
        .await;
    assert_keyless(&response, "SearchSnippet/get over Bearer");
    za_revoke_grant(&test.server, grant_id).await;

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

/// `mail::query_changes::test` in key mode: upstream's log and its five
/// queries per state; `Move` is a no-op here, since the stored thread id it
/// rewrites is not what a key account's clients see (release 7 spec 6.3).
/// `mail::threads::case_merge_rendering_parity` covers real merge rendering.
pub async fn query_changes(test: &TestServer) {
    println!("Running key-account Email QueryChanges tests...");
    let server = test.server.clone();
    // Initial query states must see only this fixture's changes.
    let admin = test.account("admin@example.com");
    let account = admin
        .create_key_user_account(
            "task17-key-query-changes@example.com",
            STRONG,
            "Query changes",
            &[],
            user_permissions(),
        )
        .await;
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
                // 1.0's `log_item_update` logs only under a current
                // account, so the batch names it.
                let mut batch = BatchBuilder::new();
                batch
                    .with_account_id(account.id().document_id())
                    .with_collection(Collection::Email)
                    .with_document(id.document_id())
                    .log_item_update(SyncCollection::Email, Some(id.prefix_id().into()));
                server.store().write_batch(&mut batch).await.unwrap();
                // A raw write skips `commit_batch`'s cache invalidation, and 1.0
                // serves a message-cache snapshot for up to 5 s without
                // revalidating it.
                server
                    .inner
                    .mark_cache_stale(account.id().document_id(), SyncCollection::Email);
                updated_ids.insert(id);
            }
            LogAction::Delete(id) => {
                let id = *id_map.get(id).unwrap();
                client.email_destroy(&id.to_string()).await.unwrap();
                removed_ids.insert(id);
            }
            LogAction::Move(from, to) => {
                id_map.insert(*to, id_map[from]);
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

                let changes = request.send_query_email_changes().await.unwrap();

                if test_num == 0 || test_num == 1 {
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
                if test_num == 1 || test_num == 2 {
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
    test.destroy_all_mailboxes(&account).await;
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
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
                "receivedAt": DateTime::from_timestamp(311923920).to_rfc3339(),
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
            .len(),
        0
    );

    // Empty store
    admin.destroy_all_mailboxes_for_account(1).await;
    test.destroy_all_mailboxes(target).await;
    test.assert_is_empty().await;
}

/// `mail::thread_get::test` in key mode, plus the keyless refusal
/// (release 7 spec 6.2).
pub async fn thread_get(test: &TestServer) {
    println!("Running key-account Email Thread tests...");
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;

    let mailbox_id = client
        .mailbox_create("JMAP Get", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    let mut expected_result = vec!["".to_string(); 5];
    let mut thread_id = "".to_string();

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
        thread_id = email.thread_id().unwrap().to_string();
        expected_result[num - 1] = email.take_id();
    }

    test.wait_for_tasks().await;

    let mut request = client.build();
    request.get_thread();
    let threads = request.send_get_thread().await.unwrap();
    let thread = threads
        .list()
        .iter()
        .find(|thread| thread.id() == thread_id)
        .expect("Thread/get without ids must include the imported thread");
    assert_eq!(
        thread.email_ids(),
        expected_result,
        "Thread/get without ids"
    );

    assert_eq!(
        client
            .thread_get(&thread_id)
            .await
            .unwrap()
            .unwrap()
            .email_ids(),
        expected_result
    );

    let (grant_id, token) = za_grant_token(&test.server, account.name(), OAuthScopes::ALL).await;
    let mut bearer = Keyless::Bearer(token);
    let response = bearer
        .call(
            account,
            "Thread/get",
            json!({
                "accountId": account.id_string(),
                "ids": [&thread_id],
            }),
        )
        .await;
    assert_keyless(&response, "Thread/get over Bearer");
    za_revoke_grant(&test.server, grant_id).await;

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

/// `mail::thread_merge::test` in key mode: the same trees and import orders,
/// with each key account using its own session and a content-current check.
/// The concurrent SMTP part also checks stored singleton ids (release 7
/// spec 6.1) and consistency between mapped query ids and thread membership.
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
        clients.push((is_key, account, client));
    }
    assert_eq!(
        clients.iter().filter(|(is_key, _, _)| *is_key).count(),
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
        for (_, _, client) in &clients {
            mailbox_ids.push(
                client
                    .mailbox_create("Thread nightmare", None::<String>, Role::None)
                    .await
                    .unwrap()
                    .take_id(),
            );
        }
        for (index, (_, _, client)) in clients.iter().enumerate() {
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
        for (is_key, account, _) in &clients {
            if *is_key {
                za_index_current(account).await;
            }
        }
        test.wait_for_tasks().await;

        for (test_num, (_, _, client)) in clients.iter().enumerate() {
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
            assert_eq!(
                messages_per_thread_db, messages_per_thread,
                "test# {test_group_num}/{test_num}"
            );
            println!("passed test# {}/{}", test_group_num, test_num);
        }

        for (_, _, client) in &clients {
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
                let parsed_message = MessageParser::new().parse(message.contents());
                match server
                    .email_ingest(IngestEmail {
                        raw_message: message.contents(),
                        message: parsed_message.as_ref(),
                        blob_hash: None,
                        access_token: &AccessToken::from_id_maybe_invalid(account_id),
                        mailbox_ids: vec![INBOX_ID],
                        keywords: vec![],
                        received_at: None,
                        source: IngestSource::Smtp {
                            deliver_to: "test@domain.org",
                            envelope_from: "sender@domain.org",
                            sender_authentication: SenderAuthentication::DmarcPass,
                            is_spam: false,
                        },
                        metadata: None,
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
    assert_eq!(messages, cached.emails.len());
    assert!(
        cached
            .emails
            .iter()
            .all(|m| m.thread_id() == m.document_id()),
        "stored thread ids remain singletons"
    );
    // Bulk setup may need more passes than one default content wait allows.
    finish_bulk_delivery_indexing(test, account).await;
    za_index_current(account).await;
    let reply = account
        .jmap_method_call("Email/query", json!({ "accountId": account.id_string() }))
        .await;
    assert_eq!(reply.name_at(0), "Email/query", "{reply:?}");
    let ids = reply.ids().collect::<AHashSet<_>>();
    assert_eq!(ids.len(), messages, "{reply:?}");
    let thread_ids = ids
        .iter()
        .map(|id| Id::from(Id::from_str(id).unwrap().prefix_id()).to_string())
        .collect::<AHashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut members = AHashSet::new();
    for chunk in thread_ids.chunks(100) {
        let reply = account
            .jmap_method_call(
                "Thread/get",
                json!({ "accountId": account.id_string(), "ids": chunk }),
            )
            .await;
        assert_eq!(reply.name_at(0), "Thread/get", "{reply:?}");
        for thread in reply.list() {
            members.extend(
                thread["emailIds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap().to_string()),
            );
        }
    }
    assert_eq!(
        members.iter().map(String::as_str).collect::<AHashSet<_>>(),
        ids
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
        assert_account_not_found(
            john_client
                .set_default_account_id(account.id_string())
                .email_get(&email, [Property::Subject].into())
                .await,
        );
        assert_account_not_found(
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
    assert_account_not_found(
        john_client
            .set_default_account_id(jane.id_string())
            .email_get(&first("jane"), [Property::Subject].into())
            .await,
    );
    assert_account_not_found(
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
    assert_from_account_not_found(
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
    // John has no access to Jane's account: 1.0 answers accountNotFound
    // before the fork's refusal is reached.
    assert_eq!(reply.0["methodResponses"][0][0], "error", "{reply:?}");
    assert_eq!(
        reply.0["methodResponses"][0][1]["type"], "accountNotFound",
        "{reply:?}"
    );
    assert_account_not_found(
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
    assert_account_not_found(
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
    assert_account_not_found(
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
    assert_account_not_found(
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
    assert_account_not_found(
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

/// See `core::push_subscription`: a throttled push is sent at the next retry
/// (`push_retry_wait`, 1 s by default).
const PUSH_SETTLE: Duration = Duration::from_millis(2500);

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
    // As upstream's test: a creation's later Mailbox change arrives at the
    // throttled batch's retry (1.0), and is drained before the type change.
    while let Ok(Some(push)) = tokio::time::timeout(PUSH_SETTLE, event_rx.recv()).await {
        assert_eq!(state_change(&push, account), names(&["Mailbox"]));
    }

    // Receive states just for the requested types
    client
        .push_subscription_update_types(&push_id, [jmap_client::DataType::Email].into())
        .await
        .unwrap();
    client
        .mailbox_update_sort_order(&mailbox_id, 123)
        .await
        .unwrap();
    if let Ok(message) = tokio::time::timeout(PUSH_SETTLE, event_rx.recv()).await {
        panic!("Received a message when expecting nothing: {message:?}");
    }
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
                json!({"@type": "Directory",  "name": "Root Folder", "parentId": null }),
                json!({"@type": "Directory",  "name": "Sub Folder", "parentId": "#i0" }),
                json!({"@type": "Directory",  "name": "Sub-sub Folder", "parentId": "#i1" }),
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
                "@type": "File",
                "name": "hello.txt",
                "parentId": &root_folder_id,
                "blobId": "#hello",
                "mediaType": "text/plain",
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
                FileNodeProperty::MediaType,
                FileNodeProperty::Size,
            ],
            [&file_id],
        )
        .await;
    let blob_id = response.list()[0].blob_id().to_string();
    response.list()[0].assert_is_equal(json!({
        "id": &file_id,
        "@type": "File",
        "name": "hello.txt",
        "parentId": &root_folder_id,
        "mediaType": "text/plain",
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
                json!({"@type": "Directory",  "name": "Sub Folder", "parentId": &root_folder_id }),
                json!({"@type": "Directory",  "name": "Folder under file", "parentId": &file_id }),
                json!({"@type": "Directory",  "name": "My/Sub/Folder" }),
                json!({"@type": "Directory",  "name": "." }),
                json!({"@type": "Directory",  "name": ".." }),
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
        "@type": "Directory",
        "name": "Renamed Sub Folder",
        "parentId": &root_folder_id,
    }));
    list[1].assert_is_equal(json!({
        "id": &file_id,
        "@type": "File",
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

/// The calendar block of `jmap_tests` in key mode (release 6 spec 9.4), in
/// upstream's order. `calendar::calendars`, `calendar::instance` and
/// `calendar::alarm` run upstream's bodies unchanged; the others are the
/// variants below, each naming the upstream body it mirrors and the rulings
/// that change its assertions.
pub async fn calendars(test: &TestServer) {
    println!("Running key-account Calendar tests...");
    calendar::calendars::test(test).await;
    calendar_event(test).await;
    calendar::instance::test(test).await;
    calendar_notification(test).await;
    calendar::alarm::test(test).await;
    calendar_identity(test).await;
    calendar_acl(test).await;
    calendar_availability(test).await;
}

/// `calendar::event::test` in key mode, line for line except where a ruling
/// changes an assertion (rule R44):
/// - release 6 spec 6.5: the create of event 1 with `isDraft`,
///   `mayInviteSelf`, `mayInviteOthers` and `hideAttendees` true is refused
///   with `invalidProperties`, and event 1 is created without them; 6.3:
///   those flags then read false;
/// - D11: `useDefaultAlerts: true` is applied by value, so event 2 and the
///   patched event 1 read `useDefaultAlerts: false`, and event 2's `alerts`
///   hold the calendar's default alert as `events::create_use_default_alerts`
///   pins it;
/// - D11, D1: the alarms are stored per component with the component's
///   title at the time the defaults were written, and a read is upstream's
///   projection of the stored tree, so in the windowed and property-subset
///   reads of event 2 an override whose title differed from the master's
///   holds the patch `alerts/abc/iCalendar` with that title as its
///   description;
/// - spec 3.3: a UID is unique within one calendar, so the duplicate UID is
///   created in calendar 1 and refused in calendar 2, which holds event 2.
///
/// `event_participants` and `EVENT_ICAL_1` are copies of the private
/// `test_jscalendar_participants` and `TEST_ICAL_1` of `event.rs`.
async fn calendar_event(test: &TestServer) {
    println!("Running key-account Calendar Event tests...");
    let account = test.account("jdoe@example.com");

    // Create test calendars
    let response = account
        .jmap_create(
            MethodObject::Calendar,
            [
                json!({
                    "name": "Holy Calendar, Batman!",
                    "timeZone": "Europe/Vatican",
                }),
                json!({
                    "name": "Calendar with Alerts",
                    "defaultAlertsWithTime": {
                        "abc": {
                            "action": "display",
                            "trigger": {
                                "relativeTo": "start",
                                "offset": "PT15M"
                            }
                        }
                    },
                }),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let calendar1_id = response.created(0).id().to_string();
    let calendar2_id = response.created(1).id().to_string();

    // Obtain state
    let change_id = account
        .jmap_get(
            MethodObject::CalendarEvent,
            Vec::<&str>::new(),
            Vec::<&str>::new(),
        )
        .await
        .state()
        .to_string();

    // Create test events
    let event_1 = test_jscalendar_1().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar1_id.as_str()].into_jmap_set(),
    );
    let event_2 = test_jscalendar_2().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar2_id.as_str()].into_jmap_set(),
    );
    let event_3 = test_jscalendar_3().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar1_id.as_str(), calendar2_id.as_str()].into_jmap_set(),
    );
    let event_4 = test_jscalendar_4().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar1_id.as_str()].into_jmap_set(),
    );
    // Release 6 spec 6.5: a key account refuses `isDraft`, `mayInviteSelf`,
    // `mayInviteOthers` and `hideAttendees` set true; event 1 is created
    // without them.
    assert_eq!(
        account
            .jmap_create(
                MethodObject::CalendarEvent,
                [event_1
                    .clone()
                    .with_property(JSCalendarProperty::<Id>::IsDraft, true)
                    .with_property(JSCalendarProperty::<Id>::MayInviteSelf, true)
                    .with_property(JSCalendarProperty::<Id>::MayInviteOthers, true)
                    .with_property(JSCalendarProperty::<Id>::HideAttendees, true)],
                Vec::<(&str, &str)>::new(),
            )
            .await
            .not_created(0)
            .typ(),
        "invalidProperties"
    );
    let response = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [
                event_1.clone(),
                event_2
                    .clone()
                    .with_property(JSCalendarProperty::<Id>::UseDefaultAlerts, true),
                event_3
                    .clone()
                    .with_property(JSCalendarProperty::<Id>::UseDefaultAlerts, false),
                event_4,
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let event_1_id = response.created(0).id().to_string();
    let event_2_id = response.created(1).id().to_string();
    let event_3_id = response.created(2).id().to_string();
    let event_4_id = response.created(3).id().to_string();

    // Destroy tmp event
    test.wait_for_tasks().await;
    assert_eq!(
        account
            .jmap_destroy(
                MethodObject::CalendarEvent,
                [event_4_id.as_str()],
                Vec::<(&str, &str)>::new(),
            )
            .await
            .destroyed()
            .next(),
        Some(event_4_id.as_str())
    );

    // Validate changes
    assert_eq!(
        account
            .jmap_changes(MethodObject::CalendarEvent, &change_id)
            .await
            .changes()
            .collect::<AHashSet<_>>(),
        [
            ChangeType::Created(&event_1_id),
            ChangeType::Created(&event_2_id),
            ChangeType::Created(&event_3_id)
        ]
        .into_iter()
        .collect::<AHashSet<_>>(),
    );

    // Verify event contents. A full get at 1.0 also returns the JMAP for
    // Calendars properties (asserted on their own below) and JSCalendar's
    // `version`, and a server-set `created` that `assert_eq_ignoring_updated`
    // leaves out like `updated`.
    let response = account
        .jmap_get(
            MethodObject::CalendarEvent,
            Vec::<&str>::new(),
            [&event_1_id, &event_2_id, &event_3_id],
        )
        .await;

    assert_eq_ignoring_updated(
        &response.list()[0],
        event_1
            .with_property(JSCalendarProperty::<Id>::Id, event_1_id.as_str())
            // Release 6 spec 6.3: the flags were not set (6.5) and read false.
            .with_property(JSCalendarProperty::<Id>::MayInviteSelf, false)
            .with_property(JSCalendarProperty::<Id>::MayInviteOthers, false)
            .with_property(JSCalendarProperty::<Id>::HideAttendees, false)
            .with_property(JSCalendarProperty::<Id>::UseDefaultAlerts, false)
            .with_property("version", "2.0")
            .with_property(JSCalendarProperty::<Id>::IsDraft, false)
            .with_property(JSCalendarProperty::<Id>::IsOrigin, true),
    );
    assert_eq_ignoring_updated(
        &response.list()[1],
        event_2
            .with_property(JSCalendarProperty::<Id>::Id, event_2_id.as_str())
            .with_property(JSCalendarProperty::<Id>::MayInviteSelf, false)
            .with_property(JSCalendarProperty::<Id>::MayInviteOthers, false)
            .with_property(JSCalendarProperty::<Id>::HideAttendees, false)
            // Release 6 D11: the default alerts were written into the event by
            // value, so the flag reads false and `alerts` holds the calendar's
            // default alert under its id, as `events::create_use_default_alerts`
            // pins it.
            .with_property(JSCalendarProperty::<Id>::UseDefaultAlerts, false)
            .with_property("version", "2.0")
            .with_property(JSCalendarProperty::<Id>::IsDraft, false)
            .with_property(JSCalendarProperty::<Id>::IsOrigin, true)
            // 1.0 keeps the calendar's default alert id ("abc"; 0.16 numbered
            // it k1) and leaves out an override's unchanged duration.
            .with_property(
                JSCalendarProperty::<Id>::Alerts,
                json!({
                  "abc": {
                    "action": "display",
                    "trigger": {
                      "@type": "OffsetTrigger",
                      "offset": "PT15M"
                    },
                    "@type": "Alert"
                  }
                }),
            )
            .with_property(
                JSCalendarProperty::<Id>::RecurrenceOverrides,
                json!({
                  "2006-01-04T12:00:00": {
                    "title": "Event #2 bis",
                    "start": "2006-01-04T14:00:00",
                    "updated": "2006-02-06T00:11:21Z"
                  },
                  "2006-01-06T12:00:00": {
                    "title": "Event #2 bis bis",
                    "start": "2006-01-06T14:00:00",
                    "updated": "2006-02-06T00:11:21Z"
                  }
                }),
            ),
    );
    assert_eq_ignoring_updated(
        &response.list()[2],
        event_3
            .with_property(JSCalendarProperty::<Id>::Id, event_3_id.as_str())
            .with_property(JSCalendarProperty::<Id>::MayInviteSelf, false)
            .with_property(JSCalendarProperty::<Id>::MayInviteOthers, false)
            .with_property(JSCalendarProperty::<Id>::HideAttendees, false)
            .with_property(JSCalendarProperty::<Id>::UseDefaultAlerts, false)
            .with_property("version", "2.0")
            .with_property(JSCalendarProperty::<Id>::IsDraft, false)
            .with_property(JSCalendarProperty::<Id>::IsOrigin, false),
    );

    // Verify JMAP for Calendars properties
    let response = account
        .jmap_get(
            MethodObject::CalendarEvent,
            [
                JSCalendarProperty::<Id>::Id,
                JSCalendarProperty::BaseEventId,
                JSCalendarProperty::MayInviteSelf,
                JSCalendarProperty::MayInviteOthers,
                JSCalendarProperty::HideAttendees,
                JSCalendarProperty::UseDefaultAlerts,
                JSCalendarProperty::UtcStart,
                JSCalendarProperty::UtcEnd,
            ],
            [&event_1_id, &event_2_id, &event_3_id],
        )
        .await;
    // Release 6 spec 6.3: event 1's flags read false; D11: event 2's
    // `useDefaultAlerts` reads false.
    response.list()[0].assert_is_equal(json!({
      "id": &event_1_id,
      "baseEventId": null,
      "mayInviteSelf": false,
      "mayInviteOthers": false,
      "hideAttendees": false,
      "useDefaultAlerts": false,
      "utcStart": "2006-01-02T15:00:00Z",
      "utcEnd": "2006-01-02T16:00:00Z"
    }));
    response.list()[1].assert_is_equal(json!({
      "id": &event_2_id,
      "baseEventId": null,
      "mayInviteSelf": false,
      "mayInviteOthers": false,
      "hideAttendees": false,
      "useDefaultAlerts": false,
      "utcStart": "2006-01-02T17:00:00Z",
      "utcEnd": "2006-01-02T18:00:00Z"
    }));
    response.list()[2].assert_is_equal(json!({
        "id": &event_3_id,
        "baseEventId": null,
        "mayInviteSelf": false,
        "mayInviteOthers": false,
        "hideAttendees": false,
        "useDefaultAlerts": false,
        "utcStart": "2006-01-04T15:00:00Z",
        "utcEnd": "2006-01-04T16:00:00Z"
    }));

    // Test /get parameters
    let response = account
        .jmap_method_calls(json!([[
            "CalendarEvent/get",
            {
                "accountId": account.id_string(),
                "properties": ["id", "title", "recurrenceOverrides", "participants"],
                "ids": [&event_2_id, &event_3_id],
                "recurrenceOverridesBefore": "2006-01-07T00:00:00Z",
                "recurrenceOverridesAfter": "2006-01-06T00:00:00Z",
                "reduceParticipants": true,
            },
            "0"
        ]]))
        .await;
    assert_eq_ignoring_updated(
        response.list_array(),
        json!([
          {
            "title": "Event #2",
            // Release 6 D11, D1: the default alert was written into every
            // component with that component's title as its description, so
            // the override's alarm differs from the master's and upstream's
            // projection shows it as a patch. The description is the title
            // at the time the defaults were written.
            "recurrenceOverrides": {
              "2006-01-06T12:00:00": {
                "updated": "2006-02-06T00:11:21Z",
                "start": "2006-01-06T14:00:00",
                "title": "Event #2 bis bis",
                "alerts/abc/iCalendar": {
                  "name": "valarm",
                  "properties": [["description", {}, "unknown", "Event #2 bis bis"]]
                }
              }
            },
            "id": "c"
          },
          {
            "title": "Event #3",
            // 1.0's reduceParticipants keeps the participants it returns
            // whole (their status and roles); 0.16 kept the address and the
            // owner role only.
            "participants": {
              "3f5bc8c0-c722-5345-b7d9-5a899db08a30": {
                "calendarAddress": "mailto:cyrus@example.com",
                "@type": "Participant",
                "roles": {
                  "owner": true,
                  "chair": true
                },
                "participationStatus": "accepted"
              }
            },
            "id": "d"
          }
        ]),
    );

    let response = account
        .jmap_method_calls(json!([[
            "CalendarEvent/get",
            {
                "accountId": account.id_string(),
                "properties": [],
                "ids": [&event_2_id, &event_3_id],
            },
            "0"
        ]]))
        .await;
    response.list()[0].assert_is_equal(json!({ "id": &event_2_id }));
    response.list()[1].assert_is_equal(json!({ "id": &event_3_id }));

    // Creating an event without calendar should fail
    assert_eq!(
        account
            .jmap_create(
                MethodObject::CalendarEvent,
                [json!({
                    "title": "Event #5",
                    "start": "2006-01-22T10:00:00",
                    "duration": "PT1H",
                    "timeZone": "US/Eastern",
                    "calendarIds": {},
                }),],
                Vec::<(&str, &str)>::new()
            )
            .await
            .not_created(0)
            .description(),
        "Event has to belong to at least one calendar."
    );

    // Release 6 spec 3.3: a key account's UID is unique within one calendar,
    // as CalDAV's is. Event 2 holds the UID in calendar 2, so the same UID is
    // created in calendar 1 (and destroyed at once) and refused in calendar 2.
    let duplicate_event_id = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "title": "Event #5",
                "start": "2006-01-22T10:00:00",
                "duration": "PT1H",
                "timeZone": "US/Eastern",
                "uid": "00959BC664CA650E933C892C@example.com",
                "calendarIds": {
                    &calendar1_id: true
                },
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    account
        .jmap_destroy(
            MethodObject::CalendarEvent,
            [duplicate_event_id.as_str()],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .assert_destroyed(&[Id::from_str(&duplicate_event_id).unwrap()]);
    let response = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "title": "Event #5",
                "start": "2006-01-22T10:00:00",
                "duration": "PT1H",
                "timeZone": "US/Eastern",
                "uid": "00959BC664CA650E933C892C@example.com",
                "calendarIds": {
                    &calendar2_id: true
                },
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    assert_eq!(response.not_created(0).typ(), "invalidProperties");
    assert_eq!(
        response.not_created(0).description(),
        "An event with UID 00959BC664CA650E933C892C@example.com already exists."
    );

    // Patching tests
    let response = account
        .jmap_update(
            MethodObject::CalendarEvent,
            [
                (
                    &event_1_id,
                    json!({
                        "isDraft": false,
                        "mayInviteSelf": false,
                        "mayInviteOthers": false,
                        "hideAttendees": false,
                        "useDefaultAlerts": true,
                        "description": null,
                        "title": "Event one",
                        "keywords": {"work": true},
                        format!("calendarIds/{calendar2_id}"): true
                    }),
                ),
                (
                    &event_2_id,
                    json!({
                        "calendarIds": {
                            &calendar1_id: true,
                            &calendar2_id: true
                        },
                        "title": "Event two",
                        "useDefaultAlerts": false,
                        "description": "Updated description",
                        "recurrenceOverrides/2006-01-04T12:00:00/title":
                        "Event two overridden",
                        "recurrenceOverrides/2006-01-06T12:00:00/title":
                        "Event two overridden twice",

                    }),
                ),
                (
                    &event_3_id,
                    json!({
                        format!("calendarIds/{calendar2_id}"): false,
                        "title": "Event three",
                        "utcStart": "2006-01-04T14:00:00Z",
                        "utcEnd": "2006-01-04T16:00:00Z",
                        "participants/3f5bc8c0-c722-5345-b7d9-5a899db08a30/roles/chair": false,
                        "participants/3f5bc8c0-c722-5345-b7d9-5a899db08a30/roles/owner": true,
                        "participants/ec5e7db5-22a3-5ed5-89bf-c8894ab86805" : null,
                        "participants/7f2bd210-6c66-5b64-8562-0176b74462b1": {
                            "calendarAddress": "mailto:rupert@example.com",
                            "@type": "Participant",
                            "participationStatus": "needs-action"
                        }
                    }),
                ),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;

    response.updated(&event_1_id);
    response.updated(&event_2_id);
    response.updated(&event_3_id);

    // Verify patches
    let response = account
        .jmap_get(
            MethodObject::CalendarEvent,
            [
                JSCalendarProperty::<Id>::Id,
                JSCalendarProperty::CalendarIds,
                JSCalendarProperty::Title,
                JSCalendarProperty::Start,
                JSCalendarProperty::Description,
                JSCalendarProperty::Keywords,
                JSCalendarProperty::RecurrenceOverrides,
                JSCalendarProperty::Participants,
                JSCalendarProperty::MayInviteOthers,
                JSCalendarProperty::MayInviteSelf,
                JSCalendarProperty::HideAttendees,
                JSCalendarProperty::UseDefaultAlerts,
                JSCalendarProperty::IsDraft,
            ],
            [&event_1_id, &event_2_id, &event_3_id],
        )
        .await;

    response.list()[0].assert_is_equal(json!({
      "id": &event_1_id,
      "calendarIds":  {
        &calendar1_id: true,
        &calendar2_id: true
      },
      "isDraft": false,
      "mayInviteSelf": false,
      "mayInviteOthers": false,
      "hideAttendees": false,
      // Release 6 D11: the patch's `useDefaultAlerts: true` was applied by
      // value and reads false.
      "useDefaultAlerts": false,
      "title": "Event one",
      "start": "2006-01-02T10:00:00",
      "keywords": {
        "work": true
      }
    }));

    assert_eq_ignoring_updated(
        &response.list()[1],
        json!({
            "id": &event_2_id,
            "calendarIds": {
              &calendar1_id: true,
              &calendar2_id: true
            },
            "title": "Event two",
            "start": "2006-01-02T12:00:00",
            "description": "Updated description",
            // Release 6 D11, D1: each override's alarm keeps the title it had
            // when the defaults were written; the patch renamed the
            // overrides and did not write the defaults again.
            "recurrenceOverrides": {
                "2006-01-04T12:00:00": {
                    "title": "Event two overridden",
                    "start": "2006-01-04T14:00:00",
                    "updated": "2006-02-06T00:11:21Z",
                    "alerts/abc/iCalendar": {
                        "name": "valarm",
                        "properties": [["description", {}, "unknown", "Event #2 bis"]]
                    }
                },
                "2006-01-06T12:00:00": {
                    "title": "Event two overridden twice",
                    "start": "2006-01-06T14:00:00",
                    "updated": "2006-02-06T00:11:21Z",
                    "alerts/abc/iCalendar": {
                        "name": "valarm",
                        "properties": [["description", {}, "unknown", "Event #2 bis bis"]]
                    }
                }
            },
            "title": "Event two",
            "start": "2006-01-02T12:00:00",
            "mayInviteOthers": false,
            "mayInviteSelf": false,
            "hideAttendees": false,
            "useDefaultAlerts": false,
            "isDraft": false
        }),
    );

    response.list()[2].assert_is_equal(json!({
        "id": event_3_id,
        "calendarIds": {
            &calendar1_id: true,
        },
        "title": "Event three",
        "start": "2006-01-04T09:00:00",
        "participants": {
            "3f5bc8c0-c722-5345-b7d9-5a899db08a30": {
                "calendarAddress": "mailto:cyrus@example.com",
                "@type": "Participant",
                "roles": {
                    "owner": true
                },
                "participationStatus": "accepted"
            },
            "7f2bd210-6c66-5b64-8562-0176b74462b1": {
                "calendarAddress": "mailto:rupert@example.com",
                "@type": "Participant",
                "participationStatus": "needs-action"
            }
        },
        "mayInviteOthers": false,
        "mayInviteSelf": false,
        "hideAttendees": false,
        "useDefaultAlerts": false,
        "isDraft": false
    }));

    // Query tests
    test.wait_for_tasks().await;
    assert_eq!(
        account
            .jmap_query(
                MethodObject::CalendarEvent,
                [
                    ("text", "Event one"),
                    ("inCalendar", calendar1_id.as_str()),
                    ("uid", "74855313FA803DA593CD579A@example.com"),
                    ("after", "2006-01-02T10:59:59"),
                    ("before", "2006-01-02T10:00:01"),
                ],
                ["start"],
                [("timeZone", "US/Eastern")],
            )
            .await
            .ids()
            .collect::<AHashSet<_>>(),
        [event_1_id.as_str()].into_iter().collect::<AHashSet<_>>()
    );

    // Recurrence expansion tests
    let response = account
        .jmap_query(
            MethodObject::CalendarEvent,
            [
                ("after", "2006-01-01T00:00:00"),
                ("before", "2006-01-08T00:00:00"),
            ],
            ["start"],
            [
                ("timeZone", Value::String("US/Eastern".into())),
                ("expandRecurrences", Value::Bool(true)),
            ],
        )
        .await;
    let ids = response.ids().collect::<Vec<_>>();
    assert_eq!(ids.len(), 7);
    account
        .jmap_get(
            MethodObject::CalendarEvent,
            [
                JSCalendarProperty::<Id>::Id,
                JSCalendarProperty::BaseEventId,
                JSCalendarProperty::Start,
                JSCalendarProperty::Duration,
                JSCalendarProperty::TimeZone,
                JSCalendarProperty::Title,
                JSCalendarProperty::RecurrenceId,
            ],
            ids.clone(),
        )
        .await
        .list_array()
        // 1.0: a non-recurring event's instance has no base event
        // (`baseEventId` null); 0.16 named the event itself.
        .assert_is_equal(json!([
          {
            "duration": "PT1H",
            "title": "Event one",
            "start": "2006-01-02T10:00:00",
            "timeZone": "US/Eastern",
            "id": &ids[0],
            "baseEventId": null
          },
          {
            "recurrenceId": "2006-01-02T12:00:00",
            "title": "Event two",
            "duration": "PT1H",
            "start": "2006-01-02T12:00:00",
            "timeZone": "US/Eastern",
            "id": &ids[1],
            "baseEventId": &event_2_id
          },
          {
            "duration": "PT1H",
            "start": "2006-01-03T12:00:00",
            "timeZone": "US/Eastern",
            "title": "Event two",
            "recurrenceId": "2006-01-03T12:00:00",
            "id": &ids[2],
            "baseEventId": &event_2_id
          },
          {
            "start": "2006-01-04T09:00:00",
            "timeZone": "US/Eastern",
            "duration": "PT2H",
            "title": "Event three",
            "id": &ids[3],
            "baseEventId": null
          },
          {
            "recurrenceId": "2006-01-04T12:00:00",
            "title": "Event two overridden",
            "start": "2006-01-04T14:00:00",
            "timeZone": "US/Eastern",
            "duration": "PT1H",
            "id": &ids[4],
            "baseEventId": &event_2_id
          },
          {
            "recurrenceId": "2006-01-05T12:00:00",
            "duration": "PT1H",
            "timeZone": "US/Eastern",
            "start": "2006-01-05T12:00:00",
            "title": "Event two",
            "id": &ids[5],
            "baseEventId": &event_2_id
          },
          {
            "recurrenceId": "2006-01-06T12:00:00",
            "duration": "PT1H",
            "title": "Event two overridden twice",
            "timeZone": "US/Eastern",
            "start": "2006-01-06T14:00:00",
            "id": &ids[6],
            "baseEventId": &event_2_id
          }
        ]));

    // Parse tests
    account
        .jmap_method_calls(json!([
         [
          "Blob/upload",
          {
           "accountId": account.id_string(),
           "create": {
            "ical": {
             "data": [
              {
               "data:asText": r#"BEGIN:VCALENDAR
PRODID:-//xyz Corp//NONSGML PDA Calendar Version 1.0//EN
VERSION:2.0
BEGIN:VEVENT
DTSTAMP:19960704T120000Z
UID:uid1@example.com
ORGANIZER:mailto:jsmith@example.com
DTSTART:19960918T143000Z
DTEND:19960920T220000Z
STATUS:CONFIRMED
CATEGORIES:CONFERENCE
SUMMARY:Networld+Interop Conference
DESCRIPTION:Networld+Interop Conference
 and Exhibit\nAtlanta World Congress Center\n
Atlanta\, Georgia
END:VEVENT
END:VCALENDAR
"#
              }
            ]
           }
          }
         },
         "S4"
        ],
        [
          "CalendarEvent/parse",
          {
           "accountId": account.id_string(),
           "blobIds": [
             "#ical"
           ]
          },
          "G4"
         ]
        ]))
        .await
        .pointer("/methodResponses/1/1/parsed")
        .unwrap()
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .1
        .assert_is_equal(json!([
  {
    "updated": "1996-07-04T12:00:00Z",
    "title": "Networld+Interop Conference",
    "description": "Networld+Interop Conferenceand Exhibit\nAtlanta World Congress Center\n",
    "timeZone": "Etc/UTC",
    "start": "1996-09-18T14:30:00",
    "status": "confirmed",
    "iCalendar": {
      "convertedProperties": {
        "duration": {
          "name": "dtend"
        }
      },
      "name": "vevent"
    },
    "@type": "Event",
    "uid": "uid1@example.com",
    "participants": {
      "25d7647e-52fc-559b-88df-d66f08da079c": {
        "calendarAddress": "mailto:jsmith@example.com",
        "@type": "Participant",
        "roles": {
          "owner": true
        }
      }
    },
    "keywords": {
      "CONFERENCE": true
    },
    "organizerCalendarAddress": "mailto:jsmith@example.com",
    "duration": "P2DT7H30M",
    // 1.0's parse also lists the server-set properties, null for a parsed
    // event, and the JSCalendar version.
    "id": null,
    "baseEventId": null,
    "calendarIds": null,
    "isDraft": null,
    "isOrigin": null,
    "version": "2.0"
  }
]));

    // Deletion tests
    test.wait_for_tasks().await;
    assert_eq!(
        account
            .jmap_destroy(
                MethodObject::CalendarEvent,
                [event_2_id.as_str(), event_3_id.as_str()],
                Vec::<(&str, &str)>::new()
            )
            .await
            .destroyed()
            .collect::<AHashSet<_>>(),
        [event_2_id.as_str(), event_3_id.as_str()]
            .into_iter()
            .collect::<AHashSet<_>>()
    );

    // CardDAV compatibility tests
    let account_id = account.id().document_id();
    let dav_client = account.webdav_client();
    let resources = test
        .server
        .fetch_groupware_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let path = format!(
        "{}{}",
        resources.base_path,
        resources
            .paths
            .iter()
            .find(|(_, v)| v.parent_id != common::NO_ID)
            .map(|(chunk, v)| chunk.path_str(v))
            .unwrap()
    );

    let ical = dav_client
        .request("GET", &path, "")
        .await
        .with_status(StatusCode::OK)
        .expect_body()
        .lines()
        .filter(|line| !line.starts_with("DTSTAMP") && !line.starts_with("CREATED"))
        .map(String::from)
        .collect::<AHashSet<_>>();
    let expected_ical = EVENT_ICAL_1
        .lines()
        .filter(|line| !line.starts_with("DTSTAMP"))
        .map(String::from)
        .collect::<AHashSet<_>>();
    assert_eq!(ical, expected_ical);

    // Organizer assignment tests
    let response = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [
                event_participants("organizer-auto@example.com", None).with_property(
                    JSCalendarProperty::<Id>::CalendarIds,
                    [calendar1_id.as_str()].into_jmap_set(),
                ),
                event_participants(
                    "organizer-explicit@example.com",
                    Some("mailto:cyrus@example.com"),
                )
                .with_property(
                    JSCalendarProperty::<Id>::CalendarIds,
                    [calendar1_id.as_str()].into_jmap_set(),
                ),
                test_jscalendar_4()
                    .with_property(JSCalendarProperty::<Id>::Uid, "organizer-none@example.com")
                    .with_property(
                        JSCalendarProperty::<Id>::CalendarIds,
                        [calendar1_id.as_str()].into_jmap_set(),
                    ),
            ],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let auto_event_id = response.created(0).id().to_string();
    let explicit_event_id = response.created(1).id().to_string();
    let no_participants_event_id = response.created(2).id().to_string();

    let response = account
        .jmap_get(
            MethodObject::CalendarEvent,
            [
                JSCalendarProperty::<Id>::Id,
                JSCalendarProperty::OrganizerCalendarAddress,
            ],
            [
                &auto_event_id,
                &explicit_event_id,
                &no_participants_event_id,
            ],
        )
        .await;

    // The server assigns an organizer when participants are present but none was supplied
    response.list()[0].assert_is_equal(json!({
        "id": &auto_event_id,
        "organizerCalendarAddress": "mailto:jdoe@example.com"
    }));

    // An organizer supplied by the client is never overwritten
    response.list()[1].assert_is_equal(json!({
        "id": &explicit_event_id,
        "organizerCalendarAddress": "mailto:cyrus@example.com"
    }));

    // An event without participants is left without an organizer
    response.list()[2].assert_is_equal(json!({
        "id": &no_participants_event_id
    }));

    // Adding participants to an event that had none assigns the organizer
    account
        .jmap_update(
            MethodObject::CalendarEvent,
            [(
                &no_participants_event_id,
                json!({
                    "participants": {
                        "8584f8f9-5414-55e3-8a1c-ad6fc2f3ffb6": {
                            "calendarAddress": "mailto:jdoe@example.com",
                            "participationStatus": "accepted",
                            "roles": {
                                "chair": true,
                                "owner": true
                            },
                            "@type": "Participant"
                        },
                        "a0171748-fe8d-57d8-879e-56036a5251d1": {
                            "calendarAddress": "mailto:rupert@example.com",
                            "participationStatus": "needs-action",
                            "kind": "individual",
                            "@type": "Participant"
                        }
                    }
                }),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .updated(&no_participants_event_id);

    account
        .jmap_get(
            MethodObject::CalendarEvent,
            [
                JSCalendarProperty::<Id>::Id,
                JSCalendarProperty::OrganizerCalendarAddress,
            ],
            [&no_participants_event_id],
        )
        .await
        .list()[0]
        .assert_is_equal(json!({
            "id": &no_participants_event_id,
            "organizerCalendarAddress": "mailto:jdoe@example.com"
        }));

    // Moving an event between calendars tombstones the previous CalDAV href
    test.wait_for_tasks().await;
    let cal_base_path = format!("{}/jdoe%40example.com/", DavResourceName::Cal.base_path());
    let sync_token = dav_client
        .sync_collection(&cal_base_path, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .sync_token()
        .to_string();
    let moved_event_id = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "@type": "Event",
                "uid": "d3a15a44-fe25-4b6a-9e2f-58d40f0f1d4c",
                "title": "Moving Event",
                "start": "2026-01-15T13:00:00",
                "timeZone": "America/New_York",
                "duration": "PT1H",
                "calendarIds": {
                    &calendar1_id: true
                },
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    let response = dav_client
        .sync_collection(
            &cal_base_path,
            &sync_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_href_count(1);
    let sync_token = response.sync_token().to_string();
    let href_in_calendar1 = response.hrefs()[0].to_string();

    account
        .jmap_update(
            MethodObject::CalendarEvent,
            [(
                &moved_event_id,
                json!({
                    "calendarIds": {
                        &calendar2_id: true
                    }
                }),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .updated(&moved_event_id);

    let response = dav_client
        .sync_collection(
            &cal_base_path,
            &sync_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_href_count(2);
    let sync_token = response.sync_token().to_string();
    let href_in_calendar2 = response
        .hrefs()
        .into_iter()
        .find(|href| *href != href_in_calendar1)
        .unwrap()
        .to_string();
    let response = response.into_propfind_response(None);
    response
        .properties(&href_in_calendar1)
        .with_status(StatusCode::NOT_FOUND);
    response
        .properties(&href_in_calendar2)
        .with_status(StatusCode::OK);

    account
        .jmap_destroy(
            MethodObject::CalendarEvent,
            [moved_event_id.as_str()],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .assert_destroyed(&[Id::from_str(&moved_event_id).unwrap()]);

    dav_client
        .sync_collection(
            &cal_base_path,
            &sync_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_href_count(1)
        .into_propfind_response(None)
        .properties(&href_in_calendar2)
        .with_status(StatusCode::NOT_FOUND);

    // Unbounded yearly recurrences remain queryable far beyond their first instance
    let yearly_event_id = account
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "@type": "Event",
                "uid": "yearly-unbounded@example.com",
                "title": "Unbounded yearly event",
                "start": "2018-06-01T09:00:00",
                "duration": "PT1H",
                "timeZone": "Etc/UTC",
                "calendarIds": {
                    &calendar1_id: true
                },
                "recurrenceRule": {
                    "@type": "RecurrenceRule",
                    "frequency": "yearly"
                }
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    test.wait_for_tasks().await;

    assert!(
        account
            .jmap_query(
                MethodObject::CalendarEvent,
                [
                    ("after", "2027-06-01T00:00:00"),
                    ("before", "2027-07-01T00:00:00"),
                ],
                ["start"],
                [("timeZone", "Etc/UTC")],
            )
            .await
            .ids()
            .any(|id| id == yearly_event_id),
        "unbounded yearly event was pruned from a June 2027 query"
    );

    account
        .jmap_destroy(
            MethodObject::CalendarEvent,
            [yearly_event_id.as_str()],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .assert_destroyed(&[Id::from_str(&yearly_event_id).unwrap()]);

    // Clean up
    test.wait_for_tasks().await;
    account.destroy_all_calendars().await;
    test.assert_is_empty().await;
}

fn event_participants(uid: &str, organizer: Option<&str>) -> Value {
    let mut event = json!({
      "@type": "Event",
      "uid": uid,
      "title": "Organizer assignment",
      "start": "2006-01-04T10:00:00",
      "duration": "PT1H",
      "timeZone": "US/Eastern",
      "updated": "2006-02-06T00:11:02Z",
      "participants": {
        "8584f8f9-5414-55e3-8a1c-ad6fc2f3ffb6": {
          "calendarAddress": "mailto:jdoe@example.com",
          "participationStatus": "accepted",
          "roles": {
            "chair": true,
            "owner": true
          },
          "@type": "Participant"
        },
        "a0171748-fe8d-57d8-879e-56036a5251d1": {
          "calendarAddress": "mailto:rupert@example.com",
          "participationStatus": "needs-action",
          "kind": "individual",
          "@type": "Participant"
        }
      }
    });

    if let Some(organizer) = organizer {
        event.as_object_mut().unwrap().insert(
            "organizerCalendarAddress".to_string(),
            Value::String(organizer.to_string()),
        );
    }

    event
}

// 1.0's CalDAV view of the JMAP event: its PRODID, the referenced time zone,
// the description, SEQUENCE and the alert (with its JSCalendar id) the event
// holds by then, and a server CREATED (left out of the comparison).
const EVENT_ICAL_1: &str = r#"BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Stalwart Labs LLC//Stalwart Server//EN
BEGIN:VEVENT
DTSTART;TZID=US/Eastern:20060102T100000
UID:74855313FA803DA593CD579A@example.com
DURATION:PT1H
SUMMARY:Event one
DESCRIPTION:Event one
DTSTAMP:20060206T001102Z
CATEGORIES:work
SEQUENCE:1
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:PT15M
JSID:abc
END:VALARM
END:VEVENT
BEGIN:VTIMEZONE
TZID:US/Eastern
BEGIN:STANDARD
DTSTART:20051230T190000
TZNAME:EST
TZOFFSETFROM:-0500
TZOFFSETTO:-0500
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:20060402T020000
TZNAME:EDT
TZOFFSETFROM:-0500
TZOFFSETTO:-0400
END:DAYLIGHT
BEGIN:STANDARD
DTSTART:20061029T020000
TZNAME:EST
TZOFFSETFROM:-0400
TZOFFSETTO:-0500
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:20070311T020000
TZNAME:EDT
TZOFFSETFROM:-0500
TZOFFSETTO:-0400
END:DAYLIGHT
BEGIN:STANDARD
DTSTART:20071104T020000
TZNAME:EST
TZOFFSETFROM:-0400
TZOFFSETTO:-0500
END:STANDARD
END:VTIMEZONE
END:VCALENDAR
"#;

/// `calendar::identity::test` in key mode (rule R44). Release 6 spec 6.9: a
/// key account's identities follow its addresses; destroy and create are
/// refused with `forbidden`, both identities remain, and the default is set
/// with a `/set` that holds only `onSuccessSetIsDefault`. The cleanup is
/// upstream's.
async fn calendar_identity(test: &TestServer) {
    println!("Running key-account Participant Identity tests...");
    let account = test.account("jdoe@example.com");

    // Obtain all identities
    let response = account
        .jmap_get(
            MethodObject::ParticipantIdentity,
            [
                ParticipantIdentityProperty::Id,
                ParticipantIdentityProperty::Name,
                ParticipantIdentityProperty::CalendarAddress,
                ParticipantIdentityProperty::IsDefault,
            ],
            Vec::<&str>::new(),
        )
        .await;
    response.list_array().assert_is_equal(json!([
      {
        "id": "a",
        "name": "John Doe",
        "calendarAddress": "mailto:jdoe@example.com",
        "isDefault": true
      },
      {
        "id": "b",
        "name": "John Doe",
        "calendarAddress": "mailto:john.doe@example.com",
        "isDefault": false
      }
    ]));

    // Destroy identity b. Release 6 spec 6.9: a key account's identities
    // follow its addresses, so a destroy is refused.
    let response = account
        .jmap_destroy(
            MethodObject::ParticipantIdentity,
            ["b"],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    assert_eq!(response.not_destroyed("b").typ(), "forbidden");
    let response = account
        .jmap_get(
            MethodObject::ParticipantIdentity,
            [
                ParticipantIdentityProperty::Id,
                ParticipantIdentityProperty::Name,
                ParticipantIdentityProperty::CalendarAddress,
                ParticipantIdentityProperty::IsDefault,
            ],
            Vec::<&str>::new(),
        )
        .await;
    // Release 6 spec 6.9: both identities remain, `a` the default.
    response.list_array().assert_is_equal(json!([
      {
        "id": "a",
        "name": "John Doe",
        "calendarAddress": "mailto:jdoe@example.com",
        "isDefault": true
      },
      {
        "id": "b",
        "name": "John Doe",
        "calendarAddress": "mailto:john.doe@example.com",
        "isDefault": false
      }
    ]));

    // Creating a new identity with an unauthorized calendar address should
    // fail. Release 6 spec 6.9: a key account refuses every create.
    let response = account
        .jmap_create(
            MethodObject::ParticipantIdentity,
            [
                json!({
                    "name": "Work",
                    "calendarAddress": "mailto:work@example.com"
                }),
                json!({
                    "name": "Work",
                    "calendarAddress": "work@example.com"
                }),
            ],
            [("onSuccessSetIsDefault", "#i0")],
        )
        .await;
    assert_eq!(response.not_created(0).typ(), "forbidden");
    assert_eq!(response.not_created(1).typ(), "forbidden");

    // Create a new identity and set it as default. Release 6 spec 6.9: the
    // create is refused, and the default is chosen among the account's own
    // identities with `onSuccessSetIsDefault` alone.
    let response = account
        .jmap_create(
            MethodObject::ParticipantIdentity,
            [json!({
                "name": "Johnny B Goode",
                "calendarAddress": "mailto:john.doe@example.com"
            })],
            [("onSuccessSetIsDefault", "#i0")],
        )
        .await;
    assert_eq!(response.not_created(0).typ(), "forbidden");
    let response = account
        .jmap_method_call(
            "ParticipantIdentity/set",
            json!({
                "accountId": account.id_string(),
                "onSuccessSetIsDefault": "b"
            }),
        )
        .await;
    assert_eq!(response.error_type_at(0), None, "{response:?}");
    let response = account
        .jmap_get(
            MethodObject::ParticipantIdentity,
            [
                ParticipantIdentityProperty::Id,
                ParticipantIdentityProperty::Name,
                ParticipantIdentityProperty::CalendarAddress,
                ParticipantIdentityProperty::IsDefault,
            ],
            Vec::<&str>::new(),
        )
        .await;
    response.list_array().assert_is_equal(json!([
      {
        "id": "a",
        "name": "John Doe",
        "calendarAddress": "mailto:jdoe@example.com",
        "isDefault": false
      },
      {
        "id": "b",
        "name": "John Doe",
        "calendarAddress": "mailto:john.doe@example.com",
        "isDefault": true
      }
    ]));

    // Cleanup
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account.id().document_id())
        .with_collection(Collection::Principal)
        .with_document(0)
        .clear(PrincipalField::ParticipantIdentities);
    test.server.commit_batch(batch).await.unwrap();
    test.assert_is_empty().await;
}

/// Release 6 spec 6.11: `jdoe`'s event notifications are refused over JMAP,
/// in place of `calendar::notification::test`.
async fn calendar_notification(test: &TestServer) {
    println!("Running key-account Calendar Event Notification tests...");
    let account = test.account("jdoe@example.com");
    let id = account.id_string();
    for (method, args) in [
        (
            "CalendarEventNotification/get",
            json!({ "accountId": id, "ids": null }),
        ),
        (
            "CalendarEventNotification/set",
            json!({ "accountId": id, "destroy": [] }),
        ),
        (
            "CalendarEventNotification/changes",
            json!({ "accountId": id, "sinceState": "n" }),
        ),
        (
            "CalendarEventNotification/query",
            json!({ "accountId": id }),
        ),
        (
            "CalendarEventNotification/queryChanges",
            json!({ "accountId": id, "sinceQueryState": "n" }),
        ),
    ] {
        let reply = account.jmap_method_call(method, args).await;
        assert_eq!(
            reply.error_type_at(0),
            Some("accountNotSupportedByMethod"),
            "{method}: {reply:?}"
        );
    }
}

/// `calendar::acl::test` in key mode, with the grant made through the
/// account API (`contacts/pin`, then `shares/grant` and `shares/revoke`), the
/// calendar named by its path from `fetch_groupware_resources` (rule R44):
/// - release 6 spec 6.11: `shareWith` is refused with `forbidden`, for the
///   owner and the grantee; `CalendarEvent/copy` answers
///   `accountNotSupportedByMethod`; the cleanup destroys no event
///   notification, which a key account refuses;
/// - 6.2: `ShareNotification/*` is unchanged; the share endpoints write the
///   calendar's ACL through upstream's builder, so each grant and the
///   revocation log upstream's notification, with the rights of the level
///   granted or revoked;
/// - 6.7: Jane's reads of John's calendar assert `id` and `myRights` (the
///   name is her own entry); the `edit` level holds no `Delete`, so her
///   destroy of the calendar is refused and John destroys it; at the `view`
///   level her update of the name, and even her empty patch, are refused;
/// - spec section 8: her event update and destroy at `view` answer
///   `forbidden`.
async fn calendar_acl(test: &TestServer) {
    println!("Running key-account Calendar ACL tests...");
    let john = test.account("jdoe@example.com");
    let jane = test.account("jane.smith@example.com");
    let john_id = john.id_string().to_string();
    let jane_id = jane.id_string().to_string();

    // Create test calendars
    let response = john
        .jmap_create(
            MethodObject::Calendar,
            [json!({
                "name": "Test #1",
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let john_calendar_id = response.created(0).id().to_string();
    let john_event_id = john
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "@type": "Event",
                "uid": "a8df6573-0474-496d-8496-033ad45d7fea",
                "updated": "2020-01-02T18:23:04Z",
                "title": "John's Simple Event",
                "start": "2020-01-15T13:00:00",
                "timeZone": "America/New_York",
                "duration": "PT1H",
                "calendarIds": {
                    &john_calendar_id: true
                },
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    let response = jane
        .jmap_create(
            MethodObject::Calendar,
            [json!({
                "name": "Test #1",
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let jane_calendar_id = response.created(0).id().to_string();
    let jane_event_id = jane
        .jmap_create(
            MethodObject::CalendarEvent,
            [json!({
                "uid": "a8df6575-0474-496d-8496-033ad45d7fea",
                "updated": "2020-01-02T18:23:04Z",
                "title": "Jane's Simple Event",
                "start": "2020-01-15T13:00:00",
                "timeZone": "America/New_York",
                "duration": "PT1H",
                "calendarIds": {
                    &jane_calendar_id: true
                },
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();

    // Verify myRights
    john.jmap_get(
        MethodObject::Calendar,
        [
            CalendarProperty::Id,
            CalendarProperty::Name,
            CalendarProperty::MyRights,
            CalendarProperty::ShareWith,
        ],
        [john_calendar_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_calendar_id,
        "name": "Test #1",
        "myRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayDelete": true,
            "mayShare": true,
            "mayWriteOwn": true,
            "mayReadFreeBusy": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "maySetMetadata": true
        },
        "shareWith": {}
        }));

    // Obtain share notifications
    let mut jane_share_change_id = jane
        .jmap_get(
            MethodObject::ShareNotification,
            Vec::<&str>::new(),
            Vec::<&str>::new(),
        )
        .await
        .state()
        .to_string();

    // Make sure Jane has no access
    assert_eq!(
        jane.jmap_get_account(
            john,
            MethodObject::Calendar,
            Vec::<&str>::new(),
            [john_calendar_id.as_str()],
        )
        .await
        .method_response()
        .typ(),
        // 1.0: an account the session cannot reach is not found (RFC 8620
        // 3.6.2); 0.16 answered forbidden.
        "accountNotFound"
    );

    // Share calendar with Jane. Release 6 spec 6.11: a key account refuses
    // `shareWith`; the grant is made through the account API, after the
    // owner pins Jane's key. The share endpoints take the calendar's path
    // name.
    assert_eq!(
        john.jmap_update(
            MethodObject::Calendar,
            [(
                &john_calendar_id,
                json!({
                    "shareWith": {
                       &jane_id : {
                         "mayReadItems": true,
                       }
                    }
                }),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&john_calendar_id)
        .typ(),
        "forbidden"
    );
    let john_account_id = john.id().document_id();
    let john_calendar_document_id = Id::from_str(&john_calendar_id).unwrap().document_id();
    let resources = test
        .server
        .fetch_groupware_resources(john_account_id, john_account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let john_calendar_path = resources
        .paths
        .iter()
        .find(|(_, v)| v.document_id == john_calendar_document_id && v.parent_id == common::NO_ID)
        .map(|(chunk, v)| chunk.path_str(v).to_string())
        .unwrap();
    let jane_key = test
        .server
        .za_served_public_key(jane.id().document_id())
        .await
        .unwrap()
        .unwrap();
    za_post(
        "contacts/pin",
        &json!({
            "username": john.name(),
            "password": john.secret(),
            "address": jane.name(),
            "fingerprint": vault::keys::display_fingerprint(&jane_key),
        }),
    )
    .await
    .expect(200);
    let mut sharing = json!({
        "username": john.name(),
        "password": john.secret(),
        "calendar": &john_calendar_path,
        "address": jane.name(),
        "level": "view",
    });
    za_post("shares/grant", &sharing).await.expect(200);
    john.jmap_get(
        MethodObject::Calendar,
        [
            CalendarProperty::Id,
            CalendarProperty::Name,
            CalendarProperty::ShareWith,
        ],
        [john_calendar_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_calendar_id,
        "name": "Test #1",
        "shareWith": {
            &jane_id : {
                "mayReadItems": true,
                "mayWriteAll": false,
                "mayDelete": false,
                "mayShare": false,
                "mayWriteOwn": false,
                "mayReadFreeBusy": false,
                "mayUpdatePrivate": false,
                "mayRSVP": false,
                "maySetMetadata": false
            }
        }
        }));

    // Verify Jane can access the event. Release 6 spec 6.7: Jane's name of
    // the calendar is her own entry, so only `id` and `myRights` are
    // asserted.
    let mut jane_view = jane
        .jmap_get_account(
            john,
            MethodObject::Calendar,
            [
                CalendarProperty::Id,
                CalendarProperty::Name,
                CalendarProperty::MyRights,
            ],
            [john_calendar_id.as_str()],
        )
        .await
        .list()[0]
        .clone();
    jane_view.as_object_mut().unwrap().remove("name");
    jane_view.assert_is_equal(json!({
        "id": john_calendar_id,
        "myRights": {
            "mayReadItems": true,
            "mayWriteAll": false,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": false,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "maySetMetadata": false
        }
    }));
    jane.jmap_get_account(
        john,
        MethodObject::CalendarEvent,
        [JSCalendarProperty::<Id>::Id, JSCalendarProperty::Title],
        [john_event_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_event_id,
        "title": "John's Simple Event",
        }));

    // Verify Jane received a share notification. Release 6 spec 6.2:
    // `ShareNotification/*` is unchanged. The share endpoints write the
    // calendar's ACL through upstream's builder, which logs one notification
    // per changed entry; the rights are those of the `view` level.
    let response = jane
        .jmap_changes(MethodObject::ShareNotification, &jane_share_change_id)
        .await;
    jane_share_change_id = response.new_state().to_string();
    let changes = response.changes().collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    let share_id = changes[0].as_created();
    jane.jmap_get(
        MethodObject::ShareNotification,
        [
            ShareNotificationProperty::Id,
            ShareNotificationProperty::ChangedBy,
            ShareNotificationProperty::ObjectType,
            ShareNotificationProperty::ObjectAccountId,
            ShareNotificationProperty::ObjectId,
            ShareNotificationProperty::OldRights,
            ShareNotificationProperty::NewRights,
            ShareNotificationProperty::Name,
        ],
        [share_id],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
          "id": &share_id,
          "changedBy": {
            "principalId": &john_id,
            "name": "John Doe",
            "email": "jdoe@example.com"
          },
          "objectType": "Calendar",
          "objectAccountId": &john_id,
          "objectId": &john_calendar_id,
          "oldRights": {
            "mayReadItems": false,
            "mayWriteAll": false,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": false,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "maySetMetadata": false
          },
          "newRights": {
            "mayReadItems": true,
            "mayWriteAll": false,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": false,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "maySetMetadata": false
          },
          "name": null
        }));

    // Updating and deleting should fail.
    // 1.0 checks a sharee's update by what it changes: an empty patch changes
    // nothing and is accepted; a sharing change needs `Share`. Release 6 spec
    // 6.7: a key account's writer asks for the right to modify the caller's
    // own entry before it looks at the patch, and a grantee at the `view`
    // level holds none, so even the empty patch is refused.
    assert_eq!(
        jane.jmap_update_account(
            john,
            MethodObject::Calendar,
            [(&john_calendar_id, json!({}))],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&john_calendar_id)
        .typ(),
        "forbidden"
    );
    assert_eq!(
        jane.jmap_update_account(
            john,
            MethodObject::Calendar,
            [(&john_calendar_id, json!({"shareWith": {}}))],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&john_calendar_id)
        .typ(),
        // Release 6 spec 6.11: `shareWith` is refused for a key account.
        "forbidden"
    );
    // Release 6 spec 6.7: at the `view` level a grantee may not change its
    // own entry of the calendar either.
    assert_eq!(
        jane.jmap_update_account(
            john,
            MethodObject::Calendar,
            [(
                &john_calendar_id,
                json!({"name": "Jane's version of John's Calendar"}),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&john_calendar_id)
        .typ(),
        "forbidden"
    );
    assert_eq!(
        jane.jmap_destroy_account(
            john,
            MethodObject::Calendar,
            [&john_calendar_id],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_destroyed(&john_calendar_id)
        .description(),
        "You are not allowed to delete this calendar."
    );
    // 1.0 checks an event update by what it changes (`groupware/src/calendar/
    // rights.rs`), so the read-only sharee's refusal is tested with a change;
    // 0.16 refused the empty patch with "You are not allowed to modify
    // calendar ...". Release 6 spec section 8: a key account's refusal is
    // `forbidden`.
    assert_eq!(
        jane.jmap_update_account(
            john,
            MethodObject::CalendarEvent,
            [(&john_event_id, json!({"title": "Changed by Jane"}))],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&john_event_id)
        .typ(),
        "forbidden"
    );
    assert_eq!(
        jane.jmap_destroy_account(
            john,
            MethodObject::CalendarEvent,
            [&john_event_id],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_destroyed(&john_event_id)
        .typ(),
        "forbidden"
    );

    // Grant Jane write access. Release 6 spec 6.11: through the account API,
    // at the `edit` level.
    sharing["level"] = json!("edit");
    za_post("shares/grant", &sharing).await.expect(200);
    // Release 6 spec 6.7: the `edit` level holds no `Delete`; Jane's name is
    // her own entry and is not asserted.
    let mut jane_view = jane
        .jmap_get_account(
            john,
            MethodObject::Calendar,
            [
                CalendarProperty::Id,
                CalendarProperty::Name,
                CalendarProperty::MyRights,
            ],
            [john_calendar_id.as_str()],
        )
        .await
        .list()[0]
        .clone();
    jane_view.as_object_mut().unwrap().remove("name");
    jane_view.assert_is_equal(json!({
        "id": john_calendar_id,
        "myRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": true,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "maySetMetadata": false
        }
    }));

    // Verify Jane received a share notification with the updated rights.
    // Release 6 spec 6.2: upstream's notification, with the `edit` level's
    // rights, which hold no `Delete` (6.7).
    let response = jane
        .jmap_changes(MethodObject::ShareNotification, &jane_share_change_id)
        .await;
    jane_share_change_id = response.new_state().to_string();
    let changes = response.changes().collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    let share_id = changes[0].as_created();
    jane.jmap_get(
        MethodObject::ShareNotification,
        [
            ShareNotificationProperty::Id,
            ShareNotificationProperty::ChangedBy,
            ShareNotificationProperty::ObjectType,
            ShareNotificationProperty::ObjectAccountId,
            ShareNotificationProperty::ObjectId,
            ShareNotificationProperty::OldRights,
            ShareNotificationProperty::NewRights,
            ShareNotificationProperty::Name,
        ],
        [share_id],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
          "id": &share_id,
          "changedBy": {
            "principalId": &john_id,
            "name": "John Doe",
            "email": "jdoe@example.com"
          },
          "objectType": "Calendar",
          "objectAccountId": &john_id,
          "objectId": &john_calendar_id,
          "oldRights": {
            "mayReadItems": true,
            "mayWriteAll": false,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": false,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "maySetMetadata": false
          },
          "newRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": true,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "maySetMetadata": false
          },
          "name": null
        }));

    // Creating a root folder should fail
    assert_eq!(
        jane.jmap_create_account(
            john,
            MethodObject::Calendar,
            [json!({
                "name": "A new shared calendar",
            })],
            Vec::<(&str, &str)>::new()
        )
        .await
        .not_created(0)
        .description(),
        "Cannot create calendars in a shared account."
    );

    // Copy Jane's event into John's calendar, and John's into Jane's.
    // Release 6 spec 6.11: `CalendarEvent/copy` is refused for key accounts.
    let response = jane
        .jmap_copy(
            jane,
            john,
            MethodObject::CalendarEvent,
            [(
                &jane_event_id,
                json!({
                    "calendarIds": {
                        &john_calendar_id: true
                    }
                }),
            )],
            false,
        )
        .await;
    assert_eq!(
        response.error_type_at(0),
        Some("accountNotSupportedByMethod"),
        "{response:?}"
    );
    let response = jane
        .jmap_copy(
            john,
            jane,
            MethodObject::CalendarEvent,
            [(
                &john_event_id,
                json!({
                    "calendarIds": {
                        &jane_calendar_id: true
                    }
                }),
            )],
            false,
        )
        .await;
    assert_eq!(
        response.error_type_at(0),
        Some("accountNotSupportedByMethod"),
        "{response:?}"
    );

    // Update John's event
    jane.jmap_update_account(
        john,
        MethodObject::CalendarEvent,
        [(
            &john_event_id,
            json!({
                "title": "John's Updated Event",
            }),
        )],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&john_event_id);
    jane.jmap_get_account(
        john,
        MethodObject::CalendarEvent,
        [JSCalendarProperty::<Id>::Id, JSCalendarProperty::Title],
        [john_event_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_event_id,
        "title": "John's Updated Event",
        }));

    // Update John's calendar name
    jane.jmap_update_account(
        john,
        MethodObject::Calendar,
        [(
            &john_calendar_id,
            json!({
                "name": "Jane's version of John's Calendar",
                "description": "This is John's calendar, but Jane can edit it now"
            }),
        )],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&john_calendar_id);
    jane.jmap_get_account(
        john,
        MethodObject::Calendar,
        [
            CalendarProperty::Id,
            CalendarProperty::Name,
            CalendarProperty::Description,
        ],
        [john_calendar_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_calendar_id,
        "name": "Jane's version of John's Calendar",
        "description": "This is John's calendar, but Jane can edit it now"
        }));

    // John should still see the old name
    john.jmap_get(
        MethodObject::Calendar,
        [
            CalendarProperty::Id,
            CalendarProperty::Name,
            CalendarProperty::Description,
        ],
        [john_calendar_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_calendar_id,
        "name": "Test #1",
        "description": null
        }));

    // Revoke Jane's access. Release 6 spec 6.11: through the account API.
    za_post(
        "shares/revoke",
        &json!({
            "username": john.name(),
            "password": john.secret(),
            "calendar": &john_calendar_path,
            "grant": jane.id().document_id(),
        }),
    )
    .await
    .expect(200);
    test.wait_for_tasks().await;
    john.jmap_get(
        MethodObject::Calendar,
        [
            CalendarProperty::Id,
            CalendarProperty::Name,
            CalendarProperty::ShareWith,
        ],
        [john_calendar_id.as_str()],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
        "id": john_calendar_id,
        "name": "Test #1",
        "shareWith": {}
        }));

    // Verify Jane can no longer access the calendar or its events
    assert_eq!(
        jane.jmap_get_account(
            john,
            MethodObject::Calendar,
            Vec::<&str>::new(),
            [john_calendar_id.as_str()],
        )
        .await
        .method_response()
        .typ(),
        // 1.0: an account the session cannot reach is not found (RFC 8620
        // 3.6.2); 0.16 answered forbidden.
        "accountNotFound"
    );

    // Verify Jane received a share notification with the updated rights.
    // Release 6 spec 6.2: upstream's notification; the old rights are the
    // `edit` level's, which hold no `Delete` (6.7).
    let response = jane
        .jmap_changes(MethodObject::ShareNotification, &jane_share_change_id)
        .await;
    let changes = response.changes().collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    let share_id = changes[0].as_created();
    jane.jmap_get(
        MethodObject::ShareNotification,
        [
            ShareNotificationProperty::Id,
            ShareNotificationProperty::ChangedBy,
            ShareNotificationProperty::ObjectType,
            ShareNotificationProperty::ObjectAccountId,
            ShareNotificationProperty::ObjectId,
            ShareNotificationProperty::OldRights,
            ShareNotificationProperty::NewRights,
            ShareNotificationProperty::Name,
        ],
        [share_id],
    )
    .await
    .list()[0]
        .assert_is_equal(json!({
          "id": &share_id,
          "changedBy": {
            "principalId": &john_id,
            "name": "John Doe",
            "email": "jdoe@example.com"
          },
          "objectType": "Calendar",
          "objectAccountId": &john_id,
          "objectId": &john_calendar_id,
          "oldRights": {
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": true,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "maySetMetadata": false
          },
          "newRights": {
            "mayReadItems": false,
            "mayWriteAll": false,
            "mayDelete": false,
            "mayShare": false,
            "mayWriteOwn": false,
            "mayReadFreeBusy": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "maySetMetadata": false
          },
          "name": null
        }));

    // Grant Jane delete access once again. Release 6 spec 6.7: the account
    // API's `edit` level holds no `Delete`, so Jane's destroy is refused and
    // John destroys the calendar.
    za_post("shares/grant", &sharing).await.expect(200);
    assert_eq!(
        jane.jmap_destroy_account(
            john,
            MethodObject::Calendar,
            [john_calendar_id.as_str()],
            [("onDestroyRemoveEvents", true)],
        )
        .await
        .not_destroyed(&john_calendar_id)
        .description(),
        "You are not allowed to delete this calendar."
    );
    assert_eq!(
        john.jmap_destroy(
            MethodObject::Calendar,
            [john_calendar_id.as_str()],
            [("onDestroyRemoveEvents", true)],
        )
        .await
        .destroyed()
        .collect::<Vec<_>>(),
        [john_calendar_id.as_str()]
    );

    // Destroy all mailboxes. Release 6 spec 6.11: a key account's event
    // notifications are refused over JMAP, so they are not destroyed here.
    john.destroy_all_calendars().await;
    jane.destroy_all_calendars().await;
    test.assert_is_empty().await;
}

/// `principal::availability::test` in key mode (rule R44). Release 6 spec
/// 6.11: `shareWith` is refused with `forbidden`; 6.2: every
/// `Principal/getAvailability` of John after it is empty.
async fn calendar_availability(test: &TestServer) {
    println!("Running key-account Principal Availability tests...");
    let john = test.account("jdoe@example.com");
    let jane = test.account("jane.smith@example.com");
    let john_id = john.id_string().to_string();
    let jane_id = jane.id_string().to_string();

    // Create test calendars
    let response = john
        .jmap_create(
            MethodObject::Calendar,
            [json!({
                "name": "Test Calendar",
                "includeInAvailability": "all"
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let calendar1_id = response.created(0).id().to_string();

    // Create test events
    let event_1 = test_jscalendar_1().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar1_id.as_str()].into_jmap_set(),
    );
    let event_2 = test_jscalendar_2().with_property(
        JSCalendarProperty::<Id>::CalendarIds,
        [calendar1_id.as_str()].into_jmap_set(),
    );
    let event_3 = test_jscalendar_3()
        .with_property(
            JSCalendarProperty::<Id>::CalendarIds,
            [calendar1_id.as_str()].into_jmap_set(),
        )
        .with_property(
            JSCalendarProperty::<Id>::Participants,
            json!({
              "3f5bc8c0-c722-5345-b7d9-5a899db08a30": {
                "calendarAddress": "mailto:jdoe@example.com",
                "@type": "Participant",
                "roles": {
                  "attendee": true,
                  "chair": true
                },
                "participationStatus": "accepted"
              }
            }),
        );
    let response = john
        .jmap_create(
            MethodObject::CalendarEvent,
            [event_1, event_2, event_3],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    let _event_1_id = response.created(0).id().to_string();
    let _event_2_id = response.created(1).id().to_string();
    let event_3_id = response.created(2).id().to_string();

    // Jane should not have access to John's availability
    let response = jane
        .jmap_method_calls(json!([[
            "Principal/getAvailability",
            {
                "accountId": &jane_id,
                "id": &john_id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            },
            "0"
        ]]))
        .await;
    response.list_array().assert_is_equal(json!([]));

    // Grant Jane free/busy access. Release 6 spec 6.11: a key account shares
    // through the account API only, so `shareWith` is refused.
    assert_eq!(
        john.jmap_update(
            MethodObject::Calendar,
            [(
                &calendar1_id,
                json!({
                    "shareWith": {
                       &jane_id : {
                         "mayReadFreeBusy": true,
                       }
                    }
                }),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .not_updated(&calendar1_id)
        .typ(),
        "forbidden"
    );

    // Jane should see John's availability now. Release 6 spec 6.2: a key
    // principal's availability is empty (6.10: `mayGetAvailability` false).
    let response = jane
        .jmap_method_calls(json!([[
            "Principal/getAvailability",
            {
                "accountId": &jane_id,
                "id": &john_id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            },
            "0"
        ]]))
        .await;
    response.list_array().assert_is_equal(json!([]));

    // Update availability to none
    john.jmap_update(
        MethodObject::Calendar,
        [(
            &calendar1_id,
            json!({
                "includeInAvailability": "none"
            }),
        )],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&calendar1_id);

    // Jane should not see any events now
    let response = jane
        .jmap_method_calls(json!([[
            "Principal/getAvailability",
            {
                "accountId": &jane_id,
                "id": &john_id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            },
            "0"
        ]]))
        .await;
    response.list_array().assert_is_equal(json!([]));

    // Update availability to attending
    john.jmap_update(
        MethodObject::Calendar,
        [(
            &calendar1_id,
            json!({
                "includeInAvailability": "attending"
            }),
        )],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&calendar1_id);

    // Jane should only see events where John is attending. Release 6 spec
    // 6.2: empty for a key principal.
    let response = jane
        .jmap_method_calls(json!([[
            "Principal/getAvailability",
            {
                "accountId": &jane_id,
                "id": &john_id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            },
            "0"
        ]]))
        .await;
    response.list_array().assert_is_equal(json!([]));

    // Update attending event to not attending
    john.jmap_update(
        MethodObject::CalendarEvent,
        [(
            &event_3_id,
            json!({
                "participants/3f5bc8c0-c722-5345-b7d9-5a899db08a30/participationStatus": "declined"
            }),
        )],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&event_3_id);

    // Jane should not see any events now
    let response = jane
        .jmap_method_calls(json!([[
            "Principal/getAvailability",
            {
                "accountId": &jane_id,
                "id": &john_id,
                "utcStart": "2006-01-01T00:00:00Z",
                "utcEnd": "2006-01-08T00:00:00Z",
            },
            "0"
        ]]))
        .await;
    response.list_array().assert_is_equal(json!([]));

    // Cleanup
    john.destroy_all_calendars().await;
    test.assert_is_empty().await;
}

/// `principal::get::test` in key mode: the session object and the principal
/// objects of key accounts are upstream's, less object metadata (upgrade
/// ruling 66), with `mayGetAvailability` false for key principals (release 6
/// spec 6.10); the plain Sales group's principal is upstream's.
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

    // 1.0's session object sets the per-account limits (mailboxes per email,
    // address books per card, file node depth), lists more file-node sort
    // options and the node types, CalendarEvent and ContactCard blobs and the
    // principals:owner capability, and drops imapsieve from the Sieve
    // extensions.
    // Validate session object capabilities: upstream's object, which a key
    // account's session now includes with the calendar capabilities (release
    // 6 spec 6.10, D13).
    let response = john.jmap_session_object().await.into_inner();
    let john_account = response["accounts"][john_id.to_string().as_str()].clone();
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
              "maxMailboxesPerEmail": 100,
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
              "maxAddressBooksPerCard": 10,
              "mayCreateAddressBook": true
            },
            "urn:ietf:params:jmap:contacts:parse": {},
            "urn:ietf:params:jmap:emailpush": {},
            // Release 6 spec 6.10: the account's calendar capability is
            // upstream's object, as `tests/src/jmap/principal/get.rs` has it.
            "urn:ietf:params:jmap:calendars": {
              "maxCalendarsPerEvent": 10,
              "minDateTime": "0001-01-01T00:00:00Z",
              "maxDateTime": "9999-12-31T23:59:59Z",
              "maxExpandedQueryDuration": "P52W1D",
              "maxParticipantsPerEvent": 20,
              "mayCreateCalendar": true
            },
            "urn:ietf:params:jmap:calendars:parse": {},
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
                "SieveScript",
                "CalendarEvent",
                "ContactCard"
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
              "maxFileNodeDepth": 64,
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
                "created",
                "modified",
                "mediaType",
                "nodeType",
                "tree"
              ],
              "supportedNodeTypes": [
                "Directory",
                "File",
                "SymLink"
              ],
              "mayCreateTopLevelFileNode": true,
              "caseInsensitiveNames": false,
              "webTrashUrl": null,
              "webUrlTemplate": null,
              "webWriteUrlTemplate": null
            },
            "urn:ietf:params:jmap:mail:share": {},
            "urn:ietf:params:jmap:principals:owner": {
              "accountIdForPrincipal": john_id,
              "principalId": john_id
            },
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
        // Release 6 spec 6.10 (D13): the calendar capabilities are primary
        // for a key account; `urn:ietf:params:jmap:metadata` stays absent.
        "urn:ietf:params:jmap:calendars": john_id,
        "urn:ietf:params:jmap:calendars:parse": john_id,
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
    // 1.0 lists a principal's accounts only where the session can reach them
    // (RFC 9670 `accounts`), as the session's Account objects: John's own
    // account is the one the session object above describes, and the other
    // principals list none. The time zone is `timeZone`. 0.16 listed every
    // principal's account capabilities and named the field `timezone`.
    // Release 6 spec 6.10: a key principal shows its calendar capability
    // block with `mayGetAvailability` false. `accountId` is the principal's
    // own account for the caller (John), null for the others; `mayShareWith`
    // is false for the caller's own principal.
    for (item, (id, email, description, calendar_account, may_share_with)) in list.iter().zip([
        (
            john_id,
            "jdoe@example.com",
            "John Doe",
            json!(john_id),
            false,
        ),
        (
            jane_id,
            "jane.smith@example.com",
            "Jane Smith",
            Value::Null,
            true,
        ),
        (
            bill_id,
            "bill@example.com",
            "Bill Foobar",
            Value::Null,
            true,
        ),
    ]) {
        let accounts = if id == john_id {
            json!({ id: john_account.clone() })
        } else {
            Value::Null
        };
        item.assert_is_equal(json!({
          "id": id,
          "type": "individual",
          "email": email,
          "description": description,
          "name": email,
          "timeZone": null,
          "capabilities": {
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:contacts": {},
            "urn:ietf:params:jmap:calendars": {
              "accountId": calendar_account,
              "mayGetAvailability": false,
              "mayShareWith": may_share_with,
              "calendarAddress": format!("mailto:{email}")
            },
            "urn:ietf:params:jmap:filenode": {},
            "urn:ietf:params:jmap:principals": {}
          },
          "accounts": accounts
        }));
    }
    list[3].assert_is_equal(json!({
      "id": sales_id,
      "type": "group",
      "email": "sales@example.com",
      "description": "Sales Group",
      "name": "sales@example.com",
      "timeZone": null,
      "capabilities": {
        "urn:ietf:params:jmap:mail": {},
        "urn:ietf:params:jmap:contacts": {},
        "urn:ietf:params:jmap:calendars": {
          "accountId": null,
          "mayGetAvailability": true,
          "mayShareWith": true,
          "calendarAddress": "mailto:sales@example.com"
        },
        "urn:ietf:params:jmap:filenode": {},
        "urn:ietf:params:jmap:principals": {}
      },
      "accounts": null
    }));
}
