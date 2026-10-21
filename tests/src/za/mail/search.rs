/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 8 and 5.5: a key account's search index holds the arrival time
//! and the size only, so IMAP SEARCH, SORT and THREAD and JMAP Email/query,
//! Email/queryChanges and SearchSnippet/get serve what those fields (and the
//! flags, mailboxes and threads) answer and refuse everything that needs a
//! message's content, with a clear error and not an empty result. An
//! ordinary account's control answers the same requests with results. The
//! index holds one content-free document per message.

use super::{Traced, key_mail_account, message_ids, message_with};
use crate::{
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        jmap::JmapResponse,
        server::TestServer,
        za::wait_for_delivery,
    },
    za::{STRONG, user_permissions},
};
use common::za::ZA_MAIL_SEARCH_REFUSED;
use email::mailbox::{DRAFTS_ID, INBOX_ID};
use imap_proto::ResponseType;
use mail_parser::DateTime;
use serde_json::{Map, Value, json};
use store::{
    IterateParams, SUBSPACE_SEARCH_INDEX,
    write::{AnyKey, SearchIndex, now},
};
use trc::{EventType, ImapEvent, SecurityEvent};
use types::id::Id;

const KEY: &str = "keymail10@example.com";
// `plainmail10` to `plainmail13` belong to `gates.rs`.
const PLAIN: &str = "plainmail14@example.com";

/// One message to import: distinct subject, arrival time, size and flags, in
/// an order that is neither the arrival order nor the size order.
struct Seed {
    subject: &'static str,
    received: i64,
    pad: usize,
    keywords: &'static [&'static str],
}

fn seeds() -> [Seed; 4] {
    [
        Seed {
            subject: "bravo",
            received: 1_623_758_400, // 2021-06-15T12:00:00Z
            pad: 300,
            keywords: &["$seen", "$flagged"],
        },
        Seed {
            subject: "delta",
            received: now() as i64 - 3600,
            pad: 100,
            keywords: &["$Label1"],
        },
        Seed {
            subject: "alpha",
            received: 1_577_880_000, // 2020-01-01T12:00:00Z
            pad: 700,
            keywords: &["$answered"],
        },
        Seed {
            subject: "charlie",
            received: 1_646_913_600, // 2022-03-10T12:00:00Z
            pad: 500,
            keywords: &["$draft"],
        },
    ]
}

/// What `jmap_served_filters_and_sorts` adds and `jmap_refused_filters_and_sorts`
/// removes.
fn fifth() -> Seed {
    Seed {
        subject: "echo",
        received: now() as i64 - 60,
        pad: 50,
        keywords: &[],
    }
}

/// An account with the four seeded messages in its INBOX: IMAP uid and
/// sequence number `n + 1` is `emails[n]`.
struct Fixture {
    account: Account,
    emails: Vec<String>,
    threads: Vec<String>,
    sizes: Vec<u64>,
    /// The `queryState` of `Email/query` before `fifth` was imported.
    stale_state: String,
    fifth: String,
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail search tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let mut key = fixture(key).await;
    let mut plain = fixture(plain).await;
    wait_for_delivery(test).await;

    imap_served_keys(&key, &plain).await;
    imap_refused_keys(&key, &plain).await;
    jmap_served_filters_and_sorts(&mut key, &mut plain).await;
    jmap_refused_filters_and_sorts(&key, &plain).await;
    wait_for_delivery(test).await;
    visible_field_document(test, key_id, plain_id).await;

    for account in [&key.account, &plain.account] {
        test.destroy_all_mailboxes(account).await;
    }
    wait_for_delivery(test).await;
}

/// The subject of each imported message, by JMAP id.
fn subjects<'a>(fixture: &Fixture, ids: impl IntoIterator<Item = &'a str>) -> Vec<&'static str> {
    let seeds = seeds();
    ids.into_iter()
        .map(|id| {
            let at = fixture
                .emails
                .iter()
                .position(|email| email == id)
                .unwrap_or_else(|| panic!("{}: unknown message {id}", fixture.account.name()));
            seeds[at].subject
        })
        .collect()
}

fn sorted(mut subjects: Vec<&'static str>) -> Vec<&'static str> {
    subjects.sort_unstable();
    subjects
}

fn inbox() -> String {
    Id::from(INBOX_ID).to_string()
}

fn mailbox_ids() -> Value {
    let mut ids = Map::new();
    ids.insert(inbox(), Value::Bool(true));
    Value::Object(ids)
}

/// Email/import of one seeded message into the INBOX; returns its JMAP id.
async fn import(account: &Account, seed: &Seed) -> String {
    let blob = account
        .jmap_client()
        .await
        .upload(
            None,
            message_with(seed.subject, &"z".repeat(seed.pad)).into_bytes(),
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let keywords = seed
        .keywords
        .iter()
        .map(|keyword| (keyword.to_string(), Value::Bool(true)))
        .collect::<Map<_, _>>();
    let reply = account
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": account.id_string(),
                "emails": { "i": {
                    "blobId": blob,
                    "mailboxIds": mailbox_ids(),
                    "keywords": keywords,
                    "receivedAt": DateTime::from_timestamp(seed.received).to_rfc3339(),
                } }
            }),
        )
        .await;
    reply
        .pointer("/methodResponses/0/1/created/i/id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("Email/import: {:?}", reply.0))
        .to_string()
}

async fn fixture(account: Account) -> Fixture {
    let mut emails = Vec::new();
    for seed in seeds() {
        emails.push(import(&account, &seed).await);
    }
    let reply = account
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account.id_string(),
                "ids": &emails,
                "properties": ["id", "threadId", "size"],
            }),
        )
        .await;
    let (mut threads, mut sizes) = (Vec::new(), Vec::new());
    for email in &emails {
        let found = reply
            .list()
            .iter()
            .find(|item| item["id"] == *email)
            .unwrap_or_else(|| panic!("{}: Email/get {:?}", account.name(), reply.0));
        threads.push(found["threadId"].as_str().unwrap().to_string());
        sizes.push(found["size"].as_u64().unwrap());
    }
    // Four distinct sizes, so the size searches discriminate.
    let mut distinct = sizes.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "{}: {sizes:?}", account.name());
    Fixture {
        account,
        emails,
        threads,
        sizes,
        stale_state: String::new(),
        fifth: String::new(),
    }
}

/// An IMAP session of `account` with INBOX selected.
async fn imap_session(account: &Account, tag: &'static [u8]) -> ImapConnection {
    let mut imap = ImapConnection::connect(tag).await;
    imap.authenticate(account.name(), account.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    imap
}

/// The answer line of a command the server served (`* SEARCH ..`,
/// `* SORT ..`), without the CONDSTORE `(MODSEQ n)` suffix, which differs
/// between two accounts.
async fn served(imap: &mut ImapConnection, command: &str, prefix: &str) -> String {
    imap.send(command).await;
    let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    lines
        .iter()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("{command}: no {prefix:?} line in {lines:?}"))
        .split(" (MODSEQ")
        .next()
        .unwrap()
        .trim_end()
        .to_string()
}

/// Mail spec 8: the keys the search index and the message cache answer are
/// served, with the ids the ordinary account returns; THREAD answers
/// singletons.
async fn imap_served_keys(key: &Fixture, plain: &Fixture) {
    println!("imap_served_keys");
    let all = "* SEARCH 1 2 3 4";
    let sorts = [
        // by arrival: alpha, bravo, charlie, delta
        ("SORT (ARRIVAL) UTF-8 ALL", "* SORT 3 1 4 2"),
        // alpha (700), charlie (500), bravo (300), delta (100)
        ("SORT (REVERSE SIZE) UTF-8 ALL", "* SORT 3 4 1 2"),
    ];
    let mut no_ops = Vec::new();
    for fixture in [key, plain] {
        let name = fixture.account.name();
        let mut searches = vec![
            ("ALL", Some(all)),
            ("1:2", Some("* SEARCH 1 2")),
            ("UID 1:2", Some("* SEARCH 1 2")),
            ("ANSWERED", Some("* SEARCH 3")),
            ("UNANSWERED", Some("* SEARCH 1 2 4")),
            ("DELETED", Some("* SEARCH")),
            ("UNDELETED", Some(all)),
            ("DRAFT", Some("* SEARCH 4")),
            ("UNDRAFT", Some("* SEARCH 1 2 3")),
            ("FLAGGED", Some("* SEARCH 1")),
            ("UNFLAGGED", Some("* SEARCH 2 3 4")),
            ("SEEN", Some("* SEARCH 1")),
            ("UNSEEN", Some("* SEARCH 2 3 4")),
            ("KEYWORD $Label1", Some("* SEARCH 2")),
            ("UNKEYWORD $Label1", Some("* SEARCH 1 3 4")),
            ("RECENT", None),
            ("NEW", None),
            ("OLD", None),
            ("MODSEQ 1", Some(all)),
            ("BEFORE 1-Jan-2030", Some(all)),
            ("BEFORE 1-Jan-2022", Some("* SEARCH 1 3")),
            ("ON 1-Jan-2020", Some("* SEARCH 3")),
            ("SINCE 1-Jan-2020", Some(all)),
            ("OLDER 1", Some(all)),
            ("YOUNGER 100000", Some("* SEARCH 2")),
            ("LARGER 100", Some(all)),
            ("SMALLER 100000", Some(all)),
            ("UIDAFTER 1", Some("* SEARCH 2 3 4")),
            ("UIDBEFORE 5", Some(all)),
        ]
        .into_iter()
        .map(|(command, expected)| (command.to_string(), expected.map(String::from)))
        .collect::<Vec<_>>();
        // The ids differ between the accounts, and the sizes sit between the
        // messages': bravo (300) < charlie (500) < alpha (700).
        searches.extend([
            (
                format!("EMAILID {}", fixture.emails[1]),
                Some("* SEARCH 2".to_string()),
            ),
            (
                format!("THREADID {}", fixture.threads[2]),
                Some("* SEARCH 3".to_string()),
            ),
            (
                format!("LARGER {}", fixture.sizes[0]),
                Some("* SEARCH 3 4".to_string()),
            ),
            (
                format!("SMALLER {}", fixture.sizes[0]),
                Some("* SEARCH 2".to_string()),
            ),
        ]);

        let mut imap = imap_session(&fixture.account, b"_s ").await;
        let mut unchecked = Vec::new();
        for (key, expected) in &searches {
            let command = format!("SEARCH {key}");
            let answer = served(&mut imap, &command, "* SEARCH").await;
            match expected {
                Some(expected) => assert_eq!(&answer, expected, "{name}: {command}"),
                None => unchecked.push(answer),
            }
        }
        for (command, expected) in sorts {
            assert_eq!(
                served(&mut imap, command, "* SORT").await,
                expected,
                "{name}: {command}"
            );
        }
        let threads = served(&mut imap, "THREAD REFERENCES UTF-8 ALL", "* THREAD").await;
        let mut groups = threads["* THREAD ".len()..]
            .split_inclusive(')')
            .collect::<Vec<_>>();
        groups.sort_unstable();
        assert_eq!(groups, ["(1)", "(2)", "(3)", "(4)"], "{name}: {threads}");
        imap.send_ok("LOGOUT").await;
        no_ops.push(unchecked);
    }
    // RECENT, NEW and OLD are no-ops in the IMAP front end: the accounts
    // agree.
    assert_eq!(no_ops[0], no_ops[1], "RECENT, NEW and OLD");
}

/// The commands a key account's IMAP session refuses, and the prefix of the
/// answer line the ordinary account's control returns.
fn refused_commands() -> Vec<(String, &'static str)> {
    let mut commands = Vec::new();
    for key in [
        "BCC x",
        "BODY x",
        "CC x",
        "FROM x",
        "HEADER X-Private x",
        "SENTBEFORE 1-Jan-2030",
        "SENTON 1-Jan-2020",
        "SENTSINCE 1-Jan-2020",
        "SUBJECT x",
        "TEXT x",
        "TO x",
        "OR FROM x SEEN",
    ] {
        commands.push((format!("SEARCH {key}"), "* SEARCH"));
    }
    for sort in [
        "SUBJECT",
        "DATE",
        "FROM",
        "DISPLAYFROM",
        "TO",
        "DISPLAYTO",
        "CC",
    ] {
        commands.push((format!("SORT ({sort}) UTF-8 ALL"), "* SORT"));
    }
    commands.push(("THREAD REFERENCES UTF-8 FROM x".to_string(), "* THREAD"));
    commands
}

/// Mail spec 8: a content filter or sort, alone or mixed with a served one,
/// answers `NO [CANNOT]` with the fixed text, no result line, and leaves the
/// connection open. The refusal is an `ImapEvent::Error` and no
/// `SecurityEvent::Unauthorized`, and the search term is not traced.
async fn imap_refused_keys(key: &Fixture, plain: &Fixture) {
    println!("imap_refused_keys");
    const TERM: &str = "search-term-canary";
    let traced = Traced::start(&[
        EventType::Imap(ImapEvent::Error),
        EventType::Security(SecurityEvent::Unauthorized),
    ]);
    let mut commands = refused_commands();
    commands.push((format!("SEARCH SUBJECT {TERM}"), "* SEARCH"));
    let mut imap = imap_session(&key.account, b"_s ").await;
    for (command, _) in &commands {
        imap.send(command).await;
        let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
        for line in &lines {
            assert!(
                !["* SEARCH", "* SORT", "* THREAD", "* ESEARCH"]
                    .iter()
                    .any(|prefix| line.starts_with(prefix)),
                "{command}: {lines:?}"
            );
        }
        lines
            .assert_response_code("CANNOT")
            .assert_contains(ZA_MAIL_SEARCH_REFUSED);
        // The connection stays open and serves the next command.
        imap.send_ok("NOOP").await;
    }
    // Served commands still work after the refusals.
    assert_eq!(
        served(&mut imap, "SEARCH ALL", "* SEARCH").await,
        "* SEARCH 1 2 3 4"
    );
    imap.send_ok("LOGOUT").await;
    let events = traced.finish().await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.typ, EventType::Security(SecurityEvent::Unauthorized))),
        "a refused search is no SecurityEvent::Unauthorized: {events:?}"
    );
    let errors = events
        .iter()
        .filter(|event| matches!(event.typ, EventType::Imap(ImapEvent::Error)))
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), commands.len(), "{events:?}");
    for error in errors {
        assert_eq!(error.details, ZA_MAIL_SEARCH_REFUSED, "{error:?}");
        assert!(!error.text.contains(TERM), "{error:?}");
    }

    // The control: the ordinary account answers every one of them.
    let mut imap = imap_session(&plain.account, b"_s ").await;
    for (command, prefix) in &commands {
        imap.send(command).await;
        if command == "SORT (CC) UTF-8 ALL" {
            // Upstream does not sort by CC at all.
            let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
            assert!(
                !lines.join("\n").contains(ZA_MAIL_SEARCH_REFUSED),
                "{command}: {lines:?}"
            );
            continue;
        }
        let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
        if *prefix != "* THREAD" {
            assert!(
                lines.iter().any(|line| line.starts_with(prefix)),
                "{command}: {lines:?}"
            );
        }
    }
    // Content search works for the control: it finds a subject.
    assert_eq!(
        served(&mut imap, "SEARCH SUBJECT bravo", "* SEARCH").await,
        "* SEARCH 1"
    );
    imap.send_ok("LOGOUT").await;
}

/// The `queryState`, ids and subjects of an `Email/query`.
fn query_args(
    account: &Account,
    filter: Option<Value>,
    sort: Option<Value>,
    collapse: bool,
) -> Value {
    let mut args = json!({ "accountId": account.id_string() });
    if let Some(filter) = filter {
        args["filter"] = filter;
    }
    if let Some(sort) = sort {
        args["sort"] = sort;
    }
    if collapse {
        args["collapseThreads"] = json!(true);
    }
    args
}

/// Mail spec 8: the filters and sorts the visible fields answer are served,
/// with the ordinary account's results.
async fn jmap_served_filters_and_sorts(key: &mut Fixture, plain: &mut Fixture) {
    println!("jmap_served_filters_and_sorts");
    let received = json!([{ "property": "receivedAt", "isAscending": true }]);
    let thread_keyword_sort = |property: &str| {
        json!([
            { "property": property, "keyword": "$seen", "isAscending": false },
            { "property": "receivedAt", "isAscending": true },
        ])
    };
    let all = vec!["alpha", "bravo", "charlie", "delta"];
    let filters = |f: &Fixture| -> Vec<(&'static str, Value, Option<Vec<&'static str>>)> {
        vec![
            (
                "inMailbox",
                json!({ "inMailbox": inbox() }),
                Some(all.clone()),
            ),
            (
                "inMailboxOtherThan",
                json!({ "inMailboxOtherThan": [Id::from(DRAFTS_ID).to_string()] }),
                Some(all.clone()),
            ),
            (
                "hasKeyword",
                json!({ "hasKeyword": "$seen" }),
                Some(vec!["bravo"]),
            ),
            (
                "notKeyword",
                json!({ "notKeyword": "$seen" }),
                Some(vec!["alpha", "charlie", "delta"]),
            ),
            (
                "allInThreadHaveKeyword",
                json!({ "allInThreadHaveKeyword": "$seen" }),
                Some(vec!["bravo"]),
            ),
            (
                "someInThreadHaveKeyword",
                json!({ "someInThreadHaveKeyword": "$seen" }),
                Some(vec!["bravo"]),
            ),
            (
                "noneInThreadHaveKeyword",
                json!({ "noneInThreadHaveKeyword": "$seen" }),
                Some(vec!["alpha", "charlie", "delta"]),
            ),
            (
                "inThread",
                json!({ "inThread": &f.threads[0] }),
                Some(vec!["bravo"]),
            ),
            (
                "id",
                json!({ "id": [&f.emails[0], &f.emails[2]] }),
                Some(vec!["alpha", "bravo"]),
            ),
            (
                "before",
                json!({ "before": "2021-01-01T00:00:00Z" }),
                Some(vec!["alpha"]),
            ),
            (
                "after",
                json!({ "after": "2021-01-01T00:00:00Z" }),
                Some(vec!["bravo", "charlie", "delta"]),
            ),
            ("minSize", json!({ "minSize": f.sizes[0] }), None),
            ("maxSize", json!({ "maxSize": f.sizes[0] }), None),
        ]
    };
    let sorts: Vec<(&str, Value, Option<Vec<&str>>)> = vec![
        (
            "receivedAt",
            received.clone(),
            Some(vec!["alpha", "bravo", "charlie", "delta"]),
        ),
        (
            "size",
            json!([{ "property": "size", "isAscending": true }]),
            Some(vec!["delta", "bravo", "charlie", "alpha"]),
        ),
        ("hasKeyword", thread_keyword_sort("hasKeyword"), None),
        (
            "allInThreadHaveKeyword",
            thread_keyword_sort("allInThreadHaveKeyword"),
            None,
        ),
        (
            "someInThreadHaveKeyword",
            thread_keyword_sort("someInThreadHaveKeyword"),
            None,
        ),
    ];

    let mut answers: Vec<Vec<Vec<&'static str>>> = Vec::new();
    for fixture in [&*key, &*plain] {
        let name = fixture.account.name();
        let mut found = Vec::new();
        for (property, filter, expected) in filters(fixture) {
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(&fixture.account, Some(filter), None, false),
                )
                .await;
            assert_eq!(
                reply.name_at(0),
                "Email/query",
                "{name}: {property}: {:?}",
                reply.0
            );
            let result = sorted(subjects(fixture, reply.ids()));
            if let Some(expected) = expected {
                assert_eq!(result, expected, "{name}: filter {property}");
            } else {
                assert!(
                    !result.is_empty() && result.len() < 4,
                    "{name}: filter {property}: {result:?}"
                );
            }
            found.push(result);
        }
        for (property, sort, expected) in &sorts {
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(&fixture.account, None, Some(sort.clone()), false),
                )
                .await;
            assert_eq!(
                reply.name_at(0),
                "Email/query",
                "{name}: {property}: {:?}",
                reply.0
            );
            let result = subjects(fixture, reply.ids());
            if let Some(expected) = expected {
                assert_eq!(&result, expected, "{name}: sort {property}");
            }
            assert_eq!(result.len(), 4, "{name}: sort {property}");
            found.push(result);
        }
        // Threads are singletons for a key account, and four distinct
        // subjects make four threads for the control.
        let collapsed = fixture
            .account
            .jmap_method_call(
                "Email/query",
                query_args(&fixture.account, None, Some(received.clone()), true),
            )
            .await;
        assert_eq!(collapsed.ids().count(), 4, "{name}: collapseThreads");
        answers.push(found);
    }
    assert_eq!(
        answers[0], answers[1],
        "the key and plain accounts' answers"
    );

    // Email/queryChanges: a message imported after the query state is added.
    for fixture in [key, plain] {
        let name = fixture.account.name().to_string();
        let filter = json!({ "inMailbox": inbox() });
        let queried = fixture
            .account
            .jmap_method_call(
                "Email/query",
                query_args(
                    &fixture.account,
                    Some(filter.clone()),
                    Some(received.clone()),
                    false,
                ),
            )
            .await;
        fixture.stale_state = queried.method_response()["queryState"]
            .as_str()
            .unwrap()
            .to_string();
        fixture.fifth = import(&fixture.account, &fifth()).await;
        let mut args = query_args(
            &fixture.account,
            Some(filter),
            Some(received.clone()),
            false,
        );
        args["sinceQueryState"] = json!(fixture.stale_state);
        let changes = fixture
            .account
            .jmap_method_call("Email/queryChanges", args)
            .await;
        assert_eq!(
            changes.name_at(0),
            "Email/queryChanges",
            "{name}: {:?}",
            changes.0
        );
        let response = changes.method_response();
        let added = response["added"].as_array().unwrap();
        assert_eq!(added.len(), 1, "{name}: {response}");
        assert_eq!(added[0]["id"], fixture.fifth, "{name}: {response}");
        assert_eq!(response["removed"], json!([]), "{name}: {response}");
    }
}

/// `Email/queryChanges` of the `filter` and `sort` since `state`.
async fn query_changes(
    account: &Account,
    filter: Value,
    sort: Option<Value>,
    state: &str,
) -> JmapResponse {
    let mut args = query_args(account, Some(filter), sort, false);
    args["sinceQueryState"] = json!(state);
    account.jmap_method_call("Email/queryChanges", args).await
}

/// A method error of `kind`, with the description `"{what}: {text}"`.
fn assert_refusal(reply: &JmapResponse, kind: &str, what: &str, context: &str) {
    let response = &reply.0["methodResponses"][0];
    assert_eq!(response[0], "error", "{context}: {response}");
    assert_eq!(response[1]["type"], kind, "{context}: {response}");
    assert_eq!(
        response[1]["description"],
        format!("{what}: {ZA_MAIL_SEARCH_REFUSED}"),
        "{context}: {response}"
    );
}

/// Mail spec 8: the filters and sorts that need content answer
/// `unsupportedFilter` and `unsupportedSort`, named and with the fixed text,
/// before any query runs; the ordinary account answers them all.
async fn jmap_refused_filters_and_sorts(key: &Fixture, plain: &Fixture) {
    println!("jmap_refused_filters_and_sorts");
    let filters = [
        ("text", json!({ "text": "x" })),
        ("from", json!({ "from": "x" })),
        ("to", json!({ "to": "x" })),
        ("cc", json!({ "cc": "x" })),
        ("bcc", json!({ "bcc": "x" })),
        ("subject", json!({ "subject": "x" })),
        ("body", json!({ "body": "x" })),
        ("header", json!({ "header": ["X-Private"] })),
        ("hasAttachment", json!({ "hasAttachment": true })),
        (
            "sentBefore",
            json!({ "sentBefore": "2030-01-01T00:00:00Z" }),
        ),
        ("sentAfter", json!({ "sentAfter": "2020-01-01T00:00:00Z" })),
    ];
    let sorts = ["from", "to", "cc", "subject", "sentAt"];

    for fixture in [key, plain] {
        let is_key = std::ptr::eq(fixture, key);
        let context = fixture.account.name();
        for (property, filter) in &filters {
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(&fixture.account, Some(filter.clone()), None, false),
                )
                .await;
            if is_key {
                assert_refusal(&reply, "unsupportedFilter", property, context);
            } else {
                assert_eq!(
                    reply.name_at(0),
                    "Email/query",
                    "{context}: {property}: {:?}",
                    reply.0
                );
            }
        }
        if !is_key {
            // The control finds content: one message by its subject.
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(
                        &fixture.account,
                        Some(json!({ "subject": "bravo" })),
                        None,
                        false,
                    ),
                )
                .await;
            assert_eq!(
                reply.ids().collect::<Vec<_>>(),
                [fixture.emails[0].as_str()],
                "{context}: {:?}",
                reply.0
            );
        }
        let mixed = json!({
            "operator": "AND",
            "conditions": [{ "inMailbox": inbox() }, { "from": "x" }],
        });
        let reply = fixture
            .account
            .jmap_method_call(
                "Email/query",
                query_args(&fixture.account, Some(mixed), None, false),
            )
            .await;
        if is_key {
            assert_refusal(&reply, "unsupportedFilter", "from", context);
        } else {
            assert_eq!(reply.name_at(0), "Email/query", "{context}: {:?}", reply.0);
        }
        for property in sorts {
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(
                        &fixture.account,
                        None,
                        Some(json!([{ "property": property, "isAscending": true }])),
                        false,
                    ),
                )
                .await;
            if is_key {
                assert_refusal(&reply, "unsupportedSort", property, context);
            } else {
                assert_eq!(
                    reply.name_at(0),
                    "Email/query",
                    "{context}: {property}: {:?}",
                    reply.0
                );
                assert_eq!(reply.ids().count(), 5, "{context}: sort {property}");
            }
        }

        // Email/queryChanges refuses with a stale state (the message imported
        // after it is a change) and with the current one (nothing to report).
        let current = fixture
            .account
            .jmap_method_call(
                "Email/query",
                query_args(&fixture.account, None, None, false),
            )
            .await
            .method_response()["queryState"]
            .as_str()
            .unwrap()
            .to_string();
        for (state_name, state) in [("stale", &fixture.stale_state), ("current", &current)] {
            let context = format!("{context}: {state_name} state");
            let reply = query_changes(&fixture.account, json!({ "from": "x" }), None, state).await;
            if is_key {
                assert_refusal(&reply, "unsupportedFilter", "from", &context);
            } else {
                assert_eq!(
                    reply.name_at(0),
                    "Email/queryChanges",
                    "{context}: {:?}",
                    reply.0
                );
            }
            let reply = query_changes(
                &fixture.account,
                json!({ "inMailbox": inbox() }),
                Some(json!([{ "property": "subject", "isAscending": true }])),
                state,
            )
            .await;
            if is_key {
                assert_refusal(&reply, "unsupportedSort", "subject", &context);
            } else {
                assert_eq!(
                    reply.name_at(0),
                    "Email/queryChanges",
                    "{context}: {:?}",
                    reply.0
                );
            }
        }

        // SearchSnippet/get: a text, subject or body property is refused and
        // named, even one that tokenises to nothing or sits under NOT;
        // without one the snippets are bare.
        for (property, filter) in [
            ("text", json!({ "text": "x" })),
            ("subject", json!({ "subject": "x" })),
            ("body", json!({ "body": "x" })),
            ("text", json!({ "text": "!!!" })),
            (
                "text",
                json!({ "operator": "NOT", "conditions": [{ "text": "x" }] }),
            ),
        ] {
            let reply = fixture
                .account
                .jmap_method_call(
                    "SearchSnippet/get",
                    json!({
                        "accountId": fixture.account.id_string(),
                        "filter": filter,
                        "emailIds": [&fixture.emails[0]],
                    }),
                )
                .await;
            if is_key {
                assert_refusal(&reply, "unsupportedFilter", property, context);
            } else {
                assert_eq!(
                    reply.name_at(0),
                    "SearchSnippet/get",
                    "{context}: {:?}",
                    reply.0
                );
                assert_eq!(reply.list().len(), 1, "{context}: {:?}", reply.0);
            }
        }
        let reply = fixture
            .account
            .jmap_method_call(
                "SearchSnippet/get",
                json!({
                    "accountId": fixture.account.id_string(),
                    "filter": { "inMailbox": inbox() },
                    "emailIds": [&fixture.emails[0]],
                }),
            )
            .await;
        assert_eq!(
            reply.name_at(0),
            "SearchSnippet/get",
            "{context}: {:?}",
            reply.0
        );
        let snippet = &reply.list()[0];
        assert_eq!(
            snippet["emailId"], fixture.emails[0],
            "{context}: {snippet}"
        );
        assert!(snippet["subject"].is_null(), "{context}: {snippet}");
        assert!(snippet["preview"].is_null(), "{context}: {snippet}");

        // The message imported by the served step is destroyed.
        let destroyed = fixture
            .account
            .jmap_method_call(
                "Email/set",
                json!({ "accountId": fixture.account.id_string(), "destroy": [&fixture.fifth] }),
            )
            .await;
        assert_eq!(
            destroyed.method_response()["destroyed"],
            json!([fixture.fifth]),
            "{context}: {:?}",
            destroyed.0
        );
    }
}

/// What the account's Email search records hold: the documents' ids, the
/// term and the field entries (`store/src/write/key.rs`: the class byte is
/// the index, plus 0 for a term, 1 << 6 for a field, 2 << 6 for a document).
#[derive(Default)]
struct SearchRecords {
    documents: Vec<u32>,
    terms: usize,
    fields: usize,
}

async fn search_records(test: &TestServer, account_id: u32) -> SearchRecords {
    let mut records = SearchRecords::default();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: SUBSPACE_SEARCH_INDEX,
                    key: vec![0u8],
                },
                AnyKey {
                    subspace: SUBSPACE_SEARCH_INDEX,
                    key: vec![u8::MAX; 64],
                },
            )
            .no_values(),
            |key, _| {
                if SearchIndex::try_from_u8(key[0] & 0x3f) == Some(SearchIndex::Email)
                    && key.get(1..5) == Some(account_id.to_be_bytes().as_slice())
                {
                    match key[0] >> 6 {
                        0 => records.terms += 1,
                        1 => records.fields += 1,
                        2 => {
                            assert_eq!(key.len(), 9, "a document key: {key:?}");
                            records
                                .documents
                                .push(u32::from_be_bytes(key[5..9].try_into().unwrap()));
                        }
                        class => panic!("search key class {class}: {key:?}"),
                    }
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    records.documents.sort_unstable();
    records
}

/// Mail spec 5.5: after the index tasks, the key account's search records
/// are one document per message, with the two visible fields and no term;
/// the ordinary account's include terms.
async fn visible_field_document(test: &TestServer, key_id: u32, plain_id: u32) {
    println!("visible_field_document");
    if test.server.store().is_pg_or_mysql() {
        return;
    }
    let key = search_records(test, key_id).await;
    assert_eq!(key.documents, message_ids(test, key_id).await, "documents");
    assert_eq!(key.documents.len(), 4);
    assert_eq!(key.terms, 0, "a key account's index holds no term");
    assert_eq!(
        key.fields, 8,
        "a key account's index holds the arrival time and the size of each message"
    );
    let plain = search_records(test, plain_id).await;
    assert_eq!(plain.documents, message_ids(test, plain_id).await);
    assert!(plain.terms > 0, "the control's index holds terms");
}
