/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 7 spec 7: metadata comes from the message cache, content search
//! comes from the sealed index, and snippets open only the requested mail.
//! The ordinary account controls pin parity. The upstream index for a key
//! account still holds ids only.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use super::{Traced, deliver, key_mail_account, message_ids, message_with, new_message};
use crate::{
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        jmap::JmapResponse,
        server::TestServer,
        za::{
            HookParked, manifest_bytes, run_search_pass, search_config_guard, search_keys,
            wait_for_delivery, write_manifest_bytes, za_bearer_token, za_unlock,
        },
    },
    za::{STRONG, user_permissions},
};
use common::za::{
    ZA_MAIL_SEARCH_REFUSED, ZA_MAIL_SEARCH_UNAVAILABLE,
    search::{
        seal::za_search_key,
        trace::{ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_UNAVAILABLE},
    },
    test_hooks::{
        set_search_pass_pause, set_search_snapshot_pause, set_search_swap_pause,
        set_search_swap_skip, set_search_view_pause,
    },
};
use common::{auth::AccessToken, network::stream::NullIo};
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    mailbox::{DRAFTS_ID, INBOX_ID},
    message::za::search::{
        ensure::{Unavailable, is_current},
        pass::{PassOutcome, ZaSearchPass},
        view::{ViewError, ViewNeed, counter_at_or_past, za_view},
    },
};
use imap::core::{MailboxId, MailboxView, SavedSearch, SelectedMailbox, SessionData};
use imap_proto::{
    ResponseType,
    protocol::{
        Sequence, fetch as imap_fetch,
        search::{self as imap_search, Filter as ImapFilter},
        thread as imap_thread,
    },
};
use jmap_proto::types::state::State;
use mail_parser::DateTime;
use serde_json::{Map, Value, json};
use store::{
    write::{SearchIndex, now},
    za_search::manifest::{ManifestValue, manifest_hash},
};
use tokio::sync::watch;
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

/// The message added after the first Email/query state.
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
    nested_search_content(&key, &plain).await;
    jmap_served_filters_and_sorts(&mut key, &mut plain).await;
    wait_for_delivery(test).await;
    imap_served_content_keys(&key, &plain).await;
    jmap_served_content_filters_and_sorts(&key, &plain).await;
    keyless_search_refused(test, &key.account).await;
    wait_for_delivery(test).await;
    visible_field_document(test, key_id, plain_id).await;
    query_changes_collapse_threads(&key.account).await;
    unavailable_refusal(test, &key.account).await;
    view_publication_while_parked(test, &key.account).await;
    view_delivery_between_reads_keeps_results_and_state_paired(test, &key.account).await;
    view_local_swap_while_acquiring(test, &key.account).await;
    acquired_view_survives_publication_and_a_queued_writer(test, &key.account).await;
    final_content_view_checks_cap(test, &key.account).await;
    content_view_uses_the_original_read_deadline(test, &key.account).await;
    content_view_resamples_after_nonpublishing_deletion(test, &key.account).await;
    saved_search_previous_result(&key, &plain).await;
    guarded_saved_search_dependency(test, &key.account).await;

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
            if id == fixture.fifth {
                return "echo";
            }
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
    let mut body = match seed.subject {
        "bravo" => "birds are flying swiftly",
        "delta" => "birds are flying slowly",
        "alpha" => "swiftly flying birds",
        "charlie" => "quiet evening",
        _ => "paper attachment",
    }
    .to_string();
    body.push(' ');
    body.push_str(&"z".repeat(seed.pad));

    let message = message_with(seed.subject, &body);
    let (headers, body) = message.split_once("\r\n\r\n").unwrap();
    let headers = headers
        .replace(
            "sender@example.org",
            &format!("{}@example.org", seed.subject),
        )
        .replace(
            "recipient@example.com",
            &format!("{}@example.com", seed.subject),
        )
        .replace(
            "Tue, 20 Oct 2026 10:00:00 +0000",
            &DateTime::from_timestamp(seed.received).to_rfc822(),
        );
    let message = if seed.subject == "echo" {
        format!(
            "{headers}\r\n\
             Cc: echo-copy@example.org\r\n\
             Bcc: echo-hidden@example.org\r\n\
             X-Private: enclosed note\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=echo-boundary\r\n\r\n\
             --echo-boundary\r\n\
             Content-Type: text/plain\r\n\r\n\
             {body}\r\n\
             --echo-boundary\r\n\
             Content-Type: application/octet-stream\r\n\
             Content-Disposition: attachment; filename=\"note.bin\"\r\n\r\n\
             note\r\n\
             --echo-boundary--\r\n"
        )
    } else {
        format!("{headers}\r\n\r\n{body}")
    };
    import_message(account, message, seed).await
}

async fn import_message(account: &Account, message: String, seed: &Seed) -> String {
    let blob = account
        .jmap_client()
        .await
        .upload(None, message.into_bytes(), None)
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

/// Deeply nested content uses the same evaluator and depth bound.
async fn nested_search_content(key: &Fixture, plain: &Fixture) {
    let nested = |depth, leaf| {
        let mut command = String::from("SEARCH ");
        for level in 0..depth {
            command.push_str(if level % 2 == 0 { "OR SEEN " } else { "NOT " });
        }
        command.push_str(leaf);
        command
    };
    let mut control = imap_session(&plain.account, b"_depth-plain ").await;
    let mut keyed = imap_session(&key.account, b"_depth-key ").await;
    for leaf in ["BODY flying", "FLAGGED"] {
        let command = nested(20, leaf);
        let expected = served(&mut control, &command, "* SEARCH").await;
        assert_eq!(served(&mut keyed, &command, "* SEARCH").await, expected);
    }
    for session in [&mut keyed, &mut control] {
        session.send(&nested(33, "FLAGGED")).await;
        let lines = session.assert_read(Type::Tagged, ResponseType::Bad).await;
        assert!(
            lines.last().unwrap().contains("Too many nested filters"),
            "{lines:?}"
        );
        session.send_ok("LOGOUT").await;
    }
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

/// Release 7 spec 7.2. Compare result IDs and THREAD groups with the
/// ordinary control, including mixed content and metadata expressions.
async fn imap_served_content_keys(key: &Fixture, plain: &Fixture) {
    println!("imap_served_content_keys");
    const TERM: &str = "search-term-canary";
    let traced = Traced::start(&[
        ZA_SEARCH_EVENT_TYPE,
        EventType::Imap(ImapEvent::Search),
        EventType::Imap(ImapEvent::Sort),
        EventType::Imap(ImapEvent::Thread),
        EventType::Imap(ImapEvent::Error),
        EventType::Security(SecurityEvent::Unauthorized),
    ]);
    let mut control = imap_session(&plain.account, b"_content-plain ").await;
    let mut keyed = imap_session(&key.account, b"_content-key ").await;
    for (condition, expected) in [
        ("BCC echo-hidden", "* SEARCH 5"),
        ("BODY flying", "* SEARCH 1 2 3"),
        ("CC echo-copy", "* SEARCH 5"),
        ("FROM bravo", "* SEARCH 1"),
        ("HEADER X-Private enclosed", "* SEARCH 5"),
        ("SENTBEFORE 1-Jan-2021", "* SEARCH 3"),
        ("SENTON 1-Jan-2020", "* SEARCH 3"),
        ("SENTSINCE 1-Jan-2021", "* SEARCH 1 2 4 5"),
        ("SUBJECT bravo", "* SEARCH 1"),
        ("TEXT quiet", "* SEARCH 4"),
        ("TO alpha", "* SEARCH 3"),
        ("OR FROM alpha SEEN", "* SEARCH 1 3"),
        ("KEYWORD $HasAttachment", "* SEARCH 5"),
        ("KEYWORD $HasNoAttachment", "* SEARCH 1 2 3 4"),
        ("UNKEYWORD $HasNoAttachment", "* SEARCH 5"),
        ("UNKEYWORD $HasAttachment", "* SEARCH 1 2 3 4"),
    ] {
        let command = format!("SEARCH {condition}");
        assert_eq!(served(&mut control, &command, "* SEARCH").await, expected);
        assert_eq!(served(&mut keyed, &command, "* SEARCH").await, expected);
    }
    for sort in [
        "SUBJECT",
        "DATE",
        "FROM",
        "DISPLAYFROM",
        "TO",
        "DISPLAYTO",
        "REVERSE SUBJECT ARRIVAL",
    ] {
        let command = format!("SORT ({sort}) UTF-8 ALL");
        let expected = served(&mut control, &command, "* SORT").await;
        assert_eq!(expected.split_whitespace().count(), 7, "{expected}");
        assert_eq!(served(&mut keyed, &command, "* SORT").await, expected);
    }
    let mut thread_results = Vec::new();
    for session in [&mut keyed, &mut control] {
        let answer = served(session, "THREAD REFERENCES UTF-8 BODY flying", "* THREAD").await;
        thread_results.push(answer);

        session.send("SORT (CC) UTF-8 ALL").await;
        let lines = session.assert_read(Type::Tagged, ResponseType::No).await;
        let lines = lines.assert_contains("Sorting by CC is not supported.");
        assert!(
            !lines.iter().any(|line| line.starts_with("* SORT")),
            "{lines:?}"
        );
        session.send_ok("NOOP").await;
    }
    assert_eq!(thread_results, ["* THREAD (1)(2)(3)", "* THREAD (1)(2)(3)"]);
    assert_eq!(
        served(&mut keyed, &format!("SEARCH SUBJECT {TERM}"), "* SEARCH").await,
        "* SEARCH"
    );
    keyed.send_ok("LOGOUT").await;
    control.send_ok("LOGOUT").await;
    let events = traced.finish().await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.typ, EventType::Security(SecurityEvent::Unauthorized))),
        "{events:?}"
    );
    assert!(
        events.iter().all(|event| !event.text.contains(TERM)),
        "{events:?}"
    );
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

/// Exercise collapse and queryChanges over independent threads. Spec 6.2
/// changes the related-thread collapse in Task 11 (covered in mail::reads).
/// Task 13 adds mapped historical deltas for related threads.
async fn query_changes_collapse_threads(account: &Account) {
    let seed = Seed {
        subject: "collapse first independent thread",
        ..fifth()
    };
    let removed = import_message(account, message_with(seed.subject, "first"), &seed).await;
    let args = query_args(account, Some(json!({ "inMailbox": inbox() })), None, true);
    let initial = account.jmap_method_call("Email/query", args.clone()).await;
    assert!(initial.ids().any(|id| id == removed));
    let mut args = args;
    args["sinceQueryState"] = initial.method_response()["queryState"].clone();

    let added = import_message(
        account,
        message_with("collapse second independent thread", "second"),
        &seed,
    )
    .await;
    let collapsed = account
        .jmap_method_call(
            "Email/query",
            query_args(account, Some(json!({ "inMailbox": inbox() })), None, true),
        )
        .await;
    let ids = collapsed.ids().collect::<Vec<_>>();
    assert!(
        ids.contains(&removed.as_str()) && ids.contains(&added.as_str()),
        "{ids:?}"
    );

    let destroyed = account
        .jmap_destroy("Email", [&removed], Vec::<(&str, &str)>::new())
        .await;
    assert_eq!(destroyed.destroyed().collect::<Vec<_>>(), [&removed]);

    let changes = account.jmap_method_call("Email/queryChanges", args).await;
    assert_eq!(changes.name_at(0), "Email/queryChanges", "{:?}", changes.0);
    let response = changes.method_response();
    let added_items = response["added"].as_array().unwrap();
    assert_eq!(added_items.len(), 1, "{response}");
    assert_eq!(added_items[0]["id"], added, "{response}");
    assert_eq!(response["removed"], json!([removed]), "{response}");
    for id in [&added_items[0]["id"], &response["removed"][0]] {
        let id = id.as_str().unwrap().parse::<Id>().unwrap();
        assert_eq!(id.prefix_id(), id.document_id(), "non-singleton id {id}");
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
/// Release 7 spec 7.1 and 7.3. Distinct matches catch an empty, broad or
/// silently dropped content predicate, and snippet fields match the control.
async fn jmap_served_content_filters_and_sorts(key: &Fixture, plain: &Fixture) {
    println!("jmap_served_content_filters_and_sorts");
    let filters = [
        (json!({ "text": "quiet" }), vec!["charlie"]),
        (json!({ "from": "bravo" }), vec!["bravo"]),
        (json!({ "to": "alpha" }), vec!["alpha"]),
        (json!({ "cc": "echo-copy" }), vec!["echo"]),
        (json!({ "bcc": "echo-hidden" }), vec!["echo"]),
        (json!({ "subject": "bravo" }), vec!["bravo"]),
        (json!({ "body": "\"flying swiftly\"" }), vec!["bravo"]),
        (json!({ "body": "fly" }), vec!["alpha", "bravo", "delta"]),
        (json!({ "header": ["X-Private"] }), vec!["echo"]),
        (json!({ "header": ["X-Private", "enclosed"] }), vec!["echo"]),
        (json!({ "hasAttachment": true }), vec!["echo"]),
        (
            json!({ "hasAttachment": false }),
            vec!["alpha", "bravo", "charlie", "delta"],
        ),
        (
            json!({ "sentBefore": "2021-01-01T00:00:00Z" }),
            vec!["alpha"],
        ),
        (
            json!({ "sentAfter": "2021-01-01T00:00:00Z" }),
            vec!["bravo", "charlie", "delta", "echo"],
        ),
        (json!({ "hasKeyword": "$hasattachment" }), vec!["echo"]),
        (json!({ "notKeyword": "$HasNoAttachment" }), vec!["echo"]),
        (
            json!({ "allInThreadHaveKeyword": "$HASATTACHMENT" }),
            vec!["echo"],
        ),
        (
            json!({ "someInThreadHaveKeyword": "$hasnoattachment" }),
            vec!["alpha", "bravo", "charlie", "delta"],
        ),
        (
            json!({ "noneInThreadHaveKeyword": "$HasAttachment" }),
            vec!["alpha", "bravo", "charlie", "delta"],
        ),
        (
            json!({
                "operator": "AND",
                "conditions": [
                    { "inMailbox": inbox() },
                    { "operator": "OR", "conditions": [
                        { "from": "alpha" },
                        { "operator": "NOT", "conditions": [{ "hasAttachment": false }] }
                    ] }
                ]
            }),
            vec!["alpha", "echo"],
        ),
    ];
    for (filter, expected) in filters {
        for fixture in [plain, key] {
            let reply = fixture
                .account
                .jmap_method_call(
                    "Email/query",
                    query_args(&fixture.account, Some(filter.clone()), None, false),
                )
                .await;
            let name = fixture.account.name();
            assert_eq!(
                reply.name_at(0),
                "Email/query",
                "{name}: {filter}: {:?}",
                reply.0
            );
            assert_eq!(
                sorted(subjects(fixture, reply.ids())),
                expected,
                "{name}: {filter}"
            );
        }
    }

    let metadata_exists = json!({ "metadataExists": "za-search/key" });
    let mixed_metadata = json!({
        "operator": "OR",
        "conditions": [
            { "text": "quiet" },
            { "operator": "NOT", "conditions": [metadata_exists.clone()] }
        ]
    });
    for (case, filter) in [
        ("standalone metadata", metadata_exists),
        ("metadata after a supported text leaf", mixed_metadata),
    ] {
        for fixture in [plain, key] {
            let reply = fixture
                .account
                .jmap_request(
                    &[
                        "urn:ietf:params:jmap:core",
                        "urn:ietf:params:jmap:mail",
                        "urn:ietf:params:jmap:metadata",
                    ],
                    json!([[
                        "Email/query",
                        query_args(&fixture.account, Some(filter.clone()), None, false),
                        "0"
                    ]]),
                )
                .await;
            let context = format!("{}: {case}", fixture.account.name());
            if std::ptr::eq(fixture, key) {
                assert_refusal(&reply, "unsupportedFilter", "metadataExists", &context);
            } else {
                assert_eq!(reply.name_at(0), "Email/query", "{context}: {:?}", reply.0);
            }
        }
    }

    let sort_properties = [
        ("from", None),
        ("to", None),
        ("subject", None),
        ("sentAt", None),
        ("hasKeyword", Some("$HasAttachment")),
        ("allInThreadHaveKeyword", Some("$hasnoattachment")),
        ("someInThreadHaveKeyword", Some("$hasattachment")),
    ];
    for (property, keyword) in sort_properties {
        let mut orders = Vec::new();
        for ascending in [true, false] {
            let mut sort = json!({ "property": property, "isAscending": ascending });
            if let Some(keyword) = keyword {
                sort["keyword"] = json!(keyword);
            }
            let sort = json!([sort, { "property": "receivedAt", "isAscending": true }]);
            let mut answers = Vec::new();
            for fixture in [plain, key] {
                let reply = fixture
                    .account
                    .jmap_method_call(
                        "Email/query",
                        query_args(&fixture.account, None, Some(sort.clone()), false),
                    )
                    .await;
                let name = fixture.account.name();
                assert_eq!(
                    reply.name_at(0),
                    "Email/query",
                    "{name}: {sort}: {:?}",
                    reply.0
                );
                let result = subjects(fixture, reply.ids());
                assert_eq!(
                    sorted(result.clone()),
                    ["alpha", "bravo", "charlie", "delta", "echo"]
                );
                answers.push(result);
            }
            assert_eq!(answers[0], answers[1], "{sort}");
            orders.push(answers.remove(0));
        }
        assert_ne!(orders[0], orders[1], "{property}: ascending and descending");
    }

    for fixture in [key, plain] {
        let current = fixture
            .account
            .jmap_method_call(
                "Email/query",
                query_args(&fixture.account, None, None, false),
            )
            .await;
        let current = current.method_response()["queryState"].as_str().unwrap();
        for state in [fixture.stale_state.as_str(), current] {
            for (filter, sort) in [
                (json!({ "from": "echo" }), None),
                (
                    json!({ "inMailbox": inbox() }),
                    Some(json!([{ "property": "subject", "isAscending": true }])),
                ),
            ] {
                let reply = query_changes(&fixture.account, filter, sort, state).await;
                assert_eq!(
                    reply.name_at(0),
                    "Email/queryChanges",
                    "{state}: {:?}",
                    reply.0
                );
                if state == current {
                    assert_eq!(reply.method_response()["added"], json!([]), "{:?}", reply.0);
                    assert_eq!(
                        reply.method_response()["removed"],
                        json!([]),
                        "{:?}",
                        reply.0
                    );
                } else {
                    assert!(
                        reply.method_response()["added"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|item| item["id"] == fixture.fifth),
                        "{:?}",
                        reply.0
                    );
                }
            }
        }
    }

    for (filter, field, highlight) in [
        (
            json!({ "text": "flying" }),
            "preview",
            Some("<mark>flying</mark>"),
        ),
        (
            json!({ "subject": "bravo" }),
            "subject",
            Some("<mark>bravo</mark>"),
        ),
        (
            json!({ "body": "flying" }),
            "preview",
            Some("<mark>flying</mark>"),
        ),
        (json!({ "text": "!!!" }), "preview", None),
        (
            json!({ "operator": "NOT", "conditions": [{ "text": "flying" }] }),
            "preview",
            None,
        ),
        (json!({ "inMailbox": inbox() }), "preview", None),
    ] {
        let bare = filter.get("inMailbox").is_some();
        let mut snippets = Vec::new();
        for fixture in [plain, key] {
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
            assert_eq!(
                reply.name_at(0),
                "SearchSnippet/get",
                "{filter}: {:?}",
                reply.0
            );
            assert_eq!(reply.list().len(), 1, "{:?}", reply.0);
            let snippet = &reply.list()[0];
            assert_eq!(snippet["emailId"], fixture.emails[0]);
            if let Some(highlight) = highlight {
                assert!(
                    snippet[field]
                        .as_str()
                        .unwrap_or_default()
                        .contains(highlight),
                    "{snippet}"
                );
            }
            if bare {
                assert!(
                    snippet["subject"].is_null() && snippet["preview"].is_null(),
                    "{snippet}"
                );
            }
            snippets.push((snippet["subject"].clone(), snippet["preview"].clone()));
        }
        assert_eq!(snippets[0], snippets[1], "{filter}");
    }

    for fixture in [key, plain] {
        let destroyed = fixture
            .account
            .jmap_method_call(
                "Email/set",
                json!({ "accountId": fixture.account.id_string(), "destroy": [&fixture.fifth] }),
            )
            .await;
        assert_eq!(
            destroyed.method_response()["destroyed"],
            json!([fixture.fifth])
        );
    }
}

async fn keyless_search_refused(test: &TestServer, account: &Account) {
    let token = za_bearer_token(&test.server, account.name()).await;
    let reply: Value = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .post(account.api_url())
        .bearer_auth(token)
        .body(
            json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                "methodCalls": [[
                    "Email/query",
                    query_args(account, Some(json!({ "text": "flying" })), None, false),
                    "0"
                ]]
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Search spec 6.2 gates Email/query before evaluating its filter.
    assert_eq!(reply["methodResponses"][0][0], "error", "{reply}");
    assert_eq!(
        reply["methodResponses"][0][1],
        json!({ "type": "forbidden", "description": common::za::ZA_MAIL_KEYLESS }),
        "{reply}"
    );
}

/// A content caller can wait only its request budget for a busy pass (spec 7.4).
async fn unavailable_refusal(test: &TestServer, account: &Account) {
    println!("unavailable_refusal");
    const TERM: &str = "zaunavailable";
    let account_id = account.id().document_id();
    let mut imap = imap_session(account, b"_unavailable ").await;
    let config = search_config_guard!(test, wait_secs: 1);
    deliver(
        test,
        "sender@example.org",
        account.name(),
        &message_with("unavailable fixture", TERM),
    )
    .await;

    let server = test.server.clone();
    let keys = za_unlock(test, account_id, account.secret()).await;
    let parked = HookParked::start(account_id, set_search_pass_pause, async move {
        server.za_search_pass(account_id, &keys).await.unwrap()
    })
    .await;
    let traced = Traced::start(&[
        ZA_SEARCH_EVENT_TYPE,
        EventType::Imap(ImapEvent::Error),
        EventType::Security(SecurityEvent::Unauthorized),
    ]);

    for query in [
        format!("SEARCH SUBJECT {TERM}"),
        format!("SEARCH OR BODY {TERM} SEEN"),
        format!("SEARCH OR SEEN NOT (FLAGGED OR BODY {TERM} ANSWERED)"),
    ] {
        let lines = tokio::time::timeout(Duration::from_secs(2), async {
            imap.send(&query).await;
            imap.assert_read(Type::Tagged, ResponseType::No).await
        })
        .await
        .expect("IMAP waited beyond the content budget");
        let lines = lines
            .assert_response_code("CANNOT")
            .assert_contains(ZA_MAIL_SEARCH_UNAVAILABLE);
        assert!(
            !lines.iter().any(|line| {
                ["* SEARCH", "* ESEARCH", "* SORT", "* THREAD"]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
            }),
            "{query}: {lines:?}"
        );
    }
    let filter = json!({ "text": TERM });
    let args = query_args(account, Some(filter.clone()), None, false);
    let reply = tokio::time::timeout(
        Duration::from_secs(2),
        account.jmap_method_call("Email/query", args.clone()),
    )
    .await
    .expect("JMAP waited beyond the content budget");
    assert_search_unavailable(&reply);

    // Even an unchanged queryChanges must check availability before its early return.
    let metadata = account
        .jmap_method_call("Email/query", query_args(account, None, None, false))
        .await;
    let state = metadata.method_response()["queryState"].as_str().unwrap();
    let reply = tokio::time::timeout(
        Duration::from_secs(2),
        query_changes(account, filter, None, state),
    )
    .await
    .expect("JMAP queryChanges waited beyond the content budget");
    assert_search_unavailable(&reply);

    let events = traced.finish().await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.typ, EventType::Security(SecurityEvent::Unauthorized))),
        "{events:?}"
    );
    assert!(
        events.iter().all(|event| !event.text.contains(TERM)),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event.details == ZA_SEARCH_UNAVAILABLE),
        "{events:?}"
    );

    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(20), parked.finish())
            .await
            .unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    drop(config);
    let reply = account.jmap_method_call("Email/query", args).await;
    assert_eq!(reply.name_at(0), "Email/query", "{:?}", reply.0);
    assert_eq!(reply.ids().count(), 1, "{:?}", reply.0);
    let result = served(&mut imap, &format!("SEARCH BODY {TERM}"), "* SEARCH").await;
    assert_eq!(result.split_whitespace().count(), 3, "{result}");
    imap.send_ok("LOGOUT").await;
}

fn assert_search_unavailable(reply: &JmapResponse) {
    let error = &reply.0["methodResponses"][0];
    assert_eq!(error[0], "error", "{error}");
    assert_eq!(error[1]["type"], "unsupportedFilter", "{error}");
    assert_eq!(
        error[1]["description"],
        format!("text: {ZA_MAIL_SEARCH_UNAVAILABLE}"),
        "{error}"
    );
}

const VIEW_TIMEOUT: Duration = Duration::from_secs(20);
const VIEW_TERM: &str = "birds";

#[derive(Debug, PartialEq, Eq)]
struct SearchAnswer {
    ids: Vec<Id>,
    state: State,
}

async fn content_answer(account: &Account) -> SearchAnswer {
    let reply = tokio::time::timeout(
        VIEW_TIMEOUT,
        account.jmap_method_call(
            "Email/query",
            query_args(account, Some(json!({ "body": VIEW_TERM })), None, false),
        ),
    )
    .await
    .expect("content query did not complete");
    assert_eq!(reply.name_at(0), "Email/query", "{:?}", reply.0);
    SearchAnswer {
        ids: reply.ids().map(|id| id.parse().unwrap()).collect(),
        state: State::parse(reply.method_response()["queryState"].as_str().unwrap()).unwrap(),
    }
}

fn assert_answer_advanced(before: &SearchAnswer, after: &SearchAnswer, added: u32) {
    assert!(matches!(before.state, State::Exact(_)), "{before:?}");
    assert!(matches!(after.state, State::Exact(_)), "{after:?}");
    assert!(
        before.state.get_change_id() < after.state.get_change_id(),
        "{before:?} {after:?}"
    );
    assert_eq!(
        after.ids.len(),
        before.ids.len() + 1,
        "{before:?} {after:?}"
    );
    assert!(
        before.ids.iter().all(|id| after.ids.contains(id)),
        "{before:?} {after:?}"
    );
    assert!(
        after.ids.iter().any(|id| id.document_id() == added),
        "{after:?}"
    );
}

async fn deliver_view_message(test: &TestServer, account: &Account, subject: &str) -> u32 {
    let account_id = account.id().document_id();
    let before = message_ids(test, account_id).await;
    deliver(
        test,
        "sender@example.org",
        account.name(),
        &message_with(subject, VIEW_TERM),
    )
    .await;
    new_message(test, account_id, &before).await
}

async fn park_swap(test: &TestServer, account: &Account) -> HookParked<PassOutcome> {
    let account_id = account.id().document_id();
    let keys = za_unlock(test, account_id, account.secret()).await;
    let server = test.server.clone();
    HookParked::start(account_id, set_search_swap_pause, async move {
        server.za_search_pass(account_id, &keys).await.unwrap()
    })
    .await
}

async fn assert_published(pass: HookParked<PassOutcome>) {
    assert!(matches!(
        tokio::time::timeout(VIEW_TIMEOUT, pass.finish())
            .await
            .unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
}

/// Another publication lands between this query's cache and manifest reads.
async fn view_publication_while_parked(test: &TestServer, account: &Account) {
    println!("view_publication_while_parked");
    let id = account.id().document_id();
    let before = content_answer(account).await;
    let old_hash = manifest_hash(&manifest_bytes(test, id).await.unwrap());
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let account_for_query = account.clone();
    let query = HookParked::start(id, set_search_snapshot_pause, async move {
        content_answer(&account_for_query).await
    })
    .await;
    let added = deliver_view_message(test, account, "publication between reads").await;
    set_search_swap_skip(Some(id));
    let pass =
        tokio::time::timeout(VIEW_TIMEOUT, run_search_pass(test, id, account.secret())).await;
    set_search_swap_skip(None);
    assert!(matches!(
        pass.unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    assert_ne!(
        old_hash,
        manifest_hash(&manifest_bytes(test, id).await.unwrap())
    );
    assert_eq!(
        entry.read_loaded().await.unwrap().manifest_bytes_hash,
        old_hash
    );

    let after = tokio::time::timeout(VIEW_TIMEOUT, query.finish())
        .await
        .unwrap();
    assert_answer_advanced(&before, &after, added);
    assert_eq!(after, content_answer(account).await);
}

/// Delivery alone changes the store but leaves the already sampled pair usable.
async fn view_delivery_between_reads_keeps_results_and_state_paired(
    test: &TestServer,
    account: &Account,
) {
    println!("view_delivery_between_reads_keeps_results_and_state_paired");
    let id = account.id().document_id();
    let before = content_answer(account).await;
    let old_manifest = manifest_bytes(test, id).await.unwrap();
    let account_for_query = account.clone();
    let query = HookParked::start(id, set_search_snapshot_pause, async move {
        content_answer(&account_for_query).await
    })
    .await;
    let added = deliver_view_message(test, account, "delivery between reads").await;
    assert_eq!(manifest_bytes(test, id).await.unwrap(), old_manifest);

    let after = tokio::time::timeout(VIEW_TIMEOUT, query.finish())
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_answer_advanced(&before, &content_answer(account).await, added);
}

/// Test the production Map acquisition directly until the Task 11/12 handlers use it.
async fn view_local_swap_while_acquiring(test: &TestServer, account: &Account) {
    println!("view_local_swap_while_acquiring");
    let id = account.id().document_id();
    let _ = content_answer(account).await;
    let old_hash = manifest_hash(&manifest_bytes(test, id).await.unwrap());
    let added = deliver_view_message(test, account, "committed before local swap").await;
    let pass = park_swap(test, account).await;
    let keys = za_unlock(test, id, account.secret()).await;
    let durable_hash = manifest_hash(&manifest_bytes(test, id).await.unwrap());
    assert_ne!(old_hash, durable_hash);
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    assert_eq!(
        entry.read_loaded().await.unwrap().manifest_bytes_hash,
        old_hash
    );
    assert!(entry.pass_lock.try_lock().is_err());

    let observe = || async {
        let view = tokio::time::timeout(
            VIEW_TIMEOUT,
            za_view(&test.server, id, &keys, ViewNeed::Map),
        )
        .await
        .expect("Map acquisition waited for the parked pass")
        .unwrap();
        assert_eq!(view.index.manifest_bytes_hash, durable_hash);
        assert_eq!(view.epoch, view.index.manifest.epoch);
        assert!(view.cache.last_change_id >= view.epoch);
        assert!(view.cache.last_change_id >= view.index.manifest.newest_row.change_id);
        assert!(view.cache.email_by_id(&added).is_some());
        let record = view.index.thread.record(added).expect("new assignment");
        (view.cache.last_change_id, record.thread_id)
    };
    let during = observe().await;
    assert!(entry.pass_lock.try_lock().is_err());
    assert_published(pass).await;
    assert_eq!(observe().await, during);
}

/// Publication after validation cannot split the result/state pair or make
/// this response queue a second read behind a writer.
async fn acquired_view_survives_publication_and_a_queued_writer(
    test: &TestServer,
    account: &Account,
) {
    println!("acquired_view_survives_publication_and_a_queued_writer");
    let id = account.id().document_id();
    let before = content_answer(account).await;
    let old_hash = manifest_hash(&manifest_bytes(test, id).await.unwrap());
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let account_for_query = account.clone();
    let query = HookParked::start(id, set_search_view_pause, async move {
        content_answer(&account_for_query).await
    })
    .await;
    assert!(
        entry.index.try_write().is_err(),
        "view did not pin the generation"
    );
    let added = deliver_view_message(test, account, "publication after validation").await;
    let pass = park_swap(test, account).await;
    assert_ne!(
        old_hash,
        manifest_hash(&manifest_bytes(test, id).await.unwrap())
    );
    assert_eq!(
        entry.read_loaded().await.unwrap().manifest_bytes_hash,
        old_hash
    );

    // One Pending poll installs the writer in the lock's fair queue. Keep
    // it there while the actual content handler finishes rendering.
    let mut writer = Box::pin(entry.index.write());
    assert!(futures::poll!(writer.as_mut()).is_pending());
    let after = tokio::time::timeout(VIEW_TIMEOUT, query.finish())
        .await
        .expect("the response blocked behind a queued writer");
    assert_eq!(after, before);
    drop(writer);

    assert_published(pass).await;
    assert_answer_advanced(&before, &content_answer(account).await, added);
}

/// A changed head after ensure-current must be checked by the returned guard.
async fn final_content_view_checks_cap(test: &TestServer, account: &Account) {
    println!("final_content_view_checks_cap");
    const CAP_BYTES: u64 = 1 << 20;
    let id = account.id().document_id();
    let _config = search_config_guard!(test, account_max_mb: 1);
    let args = query_args(
        account,
        Some(json!({
            "operator": "OR",
            "conditions": [{ "text": VIEW_TERM }, { "hasKeyword": "$flagged" }]
        })),
        None,
        false,
    );
    let before = account.jmap_method_call("Email/query", args.clone()).await;
    assert_eq!(before.name_at(0), "Email/query", "{:?}", before.0);
    assert!(before.ids().count() > 0);

    let keys = za_unlock(test, id, account.secret()).await;
    let key = za_search_key(keys.swk());
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let original_bytes = manifest_bytes(test, id).await.unwrap();
    let original_hash = manifest_hash(&original_bytes);
    let (original, counter) = {
        let map = za_view(&test.server, id, &keys, ViewNeed::Map)
            .await
            .unwrap();
        assert_eq!(map.index.manifest_bytes_hash, original_hash);
        assert!(map.index.manifest.content.as_ref().unwrap().size < CAP_BYTES);
        assert!(map.index.content.is_some());
        assert!(is_current(&map.index, map.cache.last_change_id));
        (map.index.manifest.clone(), map.cache.last_change_id)
    };
    let traced = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);

    for size in [CAP_BYTES, CAP_BYTES + 1] {
        let account_for_query = account.clone();
        let args_for_query = args.clone();
        let parked = HookParked::start(id, set_search_snapshot_pause, async move {
            tokio::time::timeout(
                VIEW_TIMEOUT,
                account_for_query.jmap_method_call("Email/query", args_for_query),
            )
            .await
        })
        .await;
        let updated_hash = {
            // Model a descriptor move with an already-current cache. A real
            // publishing pass here advances the cache counter and would
            // force a retry through ensure-current, masking the view check.
            let mut guard = entry.index.write().await;
            let index = guard.as_mut().unwrap();
            assert_eq!(index.manifest_bytes_hash, original_hash);
            assert!(is_current(index, counter));
            let resident = index.content.as_ref().unwrap();
            let mut moved = original.clone();
            let head = moved.content.as_mut().unwrap();
            assert_eq!(resident.segments.first(), Some(&head.segment));
            head.size = size;
            let bytes = ManifestValue::Index(moved.clone()).encode(&key);
            let hash = manifest_hash(&bytes);
            assert_ne!(hash, original_hash);
            write_manifest_bytes(test, id, Some(bytes)).await;
            index.manifest = moved;
            index.manifest_bytes_hash = hash;
            assert!(!index.manifest.content_to.suspended);
            assert!(is_current(index, counter));
            hash
        };

        let reply = tokio::time::timeout(VIEW_TIMEOUT, parked.finish()).await;
        let map = tokio::time::timeout(
            VIEW_TIMEOUT,
            za_view(&test.server, id, &keys, ViewNeed::Map),
        )
        .await
        .map(|result| {
            result.map(|view| (view.index.manifest_bytes_hash, view.cache.last_change_id))
        });

        // Restore the exact authenticated bytes and the matching resident
        // descriptor before asserting the responses.
        {
            let mut guard = entry.index.write().await;
            let index = guard.as_mut().unwrap();
            write_manifest_bytes(test, id, Some(original_bytes.clone())).await;
            index.manifest = original.clone();
            index.manifest_bytes_hash = original_hash;
        }
        assert_eq!(map.unwrap().unwrap(), (updated_hash, counter));
        let reply = reply
            .expect("parked query did not finish")
            .expect("content query timed out");
        if size == CAP_BYTES {
            assert_eq!(reply.name_at(0), "Email/query", "{:?}", reply.0);
            assert_eq!(
                reply.method_response()["ids"],
                before.method_response()["ids"]
            );
            assert_eq!(
                reply.method_response()["queryState"],
                before.method_response()["queryState"]
            );
        } else {
            assert_search_unavailable(&reply);
        }
    }

    let events = traced.finish().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.account_id == Some(u64::from(id))
                    && event.details == ZA_SEARCH_UNAVAILABLE
                    && event.reason == "over_cap"
            })
            .count(),
        1,
        "{events:?}"
    );
}

/// Content mode uses one deadline before and after ensure-current.
async fn content_view_uses_the_original_read_deadline(test: &TestServer, account: &Account) {
    println!("content_view_uses_the_original_read_deadline");
    let id = account.id().document_id();
    let before = content_answer(account).await;
    let keys = za_unlock(test, id, account.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();

    {
        // Model a pass reloading a moved manifest. An eager, unbounded
        // za_search_entry read would wait here before entering ensure-current.
        let _pass = entry.pass_lock.lock().await;
        let _reload = entry.index.write().await;
        let start = Instant::now();
        let deadline = start + Duration::from_millis(200);
        let result = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline + Duration::from_secs(1)),
            za_view(&test.server, id, &keys, ViewNeed::Content { deadline }),
        )
        .await
        .expect("content view blocked on initial entry acquisition");
        assert!(matches!(
            result,
            Err(ViewError::Unavailable(Unavailable::NotCurrent))
        ));
        assert!(start.elapsed() >= Duration::from_millis(190));
    }

    {
        // The hook proves that ensure-current already returned and the
        // immutable cache was captured. A writer now blocks the final read.
        let deadline = Instant::now() + Duration::from_secs(3);
        let server = test.server.clone();
        let keys = keys.clone();
        let parked = HookParked::start(id, set_search_snapshot_pause, async move {
            za_view(&server, id, &keys, ViewNeed::Content { deadline }).await
        })
        .await;
        let _writer = entry.index.write().await;
        // Spend most of the original budget before releasing the hook.
        // The lock and hook, rather than this timer, establish the order.
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            deadline - Duration::from_millis(400),
        ))
        .await;
        let result = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline + Duration::from_secs(1)),
            parked.finish(),
        )
        .await
        .expect("the final guard read exceeded the original deadline");
        assert!(matches!(
            result,
            Err(ViewError::Unavailable(Unavailable::NotCurrent))
        ));
        assert!(Instant::now() >= deadline - Duration::from_millis(10));
    }
    assert_eq!(content_answer(account).await, before);
}

/// A deletion-only swap must not combine the new postings with old presence.
async fn content_view_resamples_after_nonpublishing_deletion(test: &TestServer, account: &Account) {
    println!("content_view_resamples_after_nonpublishing_deletion");
    let id = account.id().document_id();
    let alpha = import_message(
        account,
        message_with("alpha deletion fixture", "message to destroy"),
        &fifth(),
    )
    .await;
    let beta = import_message(
        account,
        message_with("beta deletion fixture", "surviving control"),
        &fifth(),
    )
    .await;
    let alpha_doc = alpha.parse::<Id>().unwrap().document_id();
    let beta_doc = beta.parse::<Id>().unwrap().document_id();
    let filter_args = |condition| {
        query_args(
            account,
            Some(json!({
                "operator": "AND",
                "conditions": [{ "id": [&alpha, &beta] }, condition]
            })),
            None,
            false,
        )
    };
    let subject = json!({ "subject": "alpha" });
    let positive = account
        .jmap_method_call("Email/query", filter_args(subject.clone()))
        .await;
    assert_eq!(positive.name_at(0), "Email/query", "{:?}", positive.0);
    assert_eq!(positive.ids().collect::<Vec<_>>(), [alpha.as_str()]);
    let args = filter_args(json!({ "operator": "NOT", "conditions": [subject] }));
    let before = account.jmap_method_call("Email/query", args.clone()).await;
    assert_eq!(before.name_at(0), "Email/query", "{:?}", before.0);
    assert_eq!(before.ids().collect::<Vec<_>>(), [beta.as_str()]);

    let keys = za_unlock(test, id, account.secret()).await;
    let old_bytes = manifest_bytes(test, id).await.unwrap();
    let (entry, old_cache, old_manifest) = {
        let view = za_view(&test.server, id, &keys, ViewNeed::Map)
            .await
            .unwrap();
        assert_eq!(view.index.manifest_bytes_hash, manifest_hash(&old_bytes));
        let part = &view.index.content.as_ref().unwrap().part;
        assert!(part.contains(alpha_doc) && part.contains(beta_doc));
        assert!(is_current(&view.index, view.cache.last_change_id));
        (
            view.entry.clone(),
            view.cache.clone(),
            view.index.manifest.clone(),
        )
    };

    let account_for_query = account.clone();
    let args_for_query = args.clone();
    let parked = HookParked::start(id, set_search_snapshot_pause, async move {
        tokio::time::timeout(
            VIEW_TIMEOUT,
            account_for_query.jmap_method_call("Email/query", args_for_query),
        )
        .await
    })
    .await;

    let destroyed = account
        .jmap_destroy("Email", [&alpha], Vec::<(&str, &str)>::new())
        .await;
    assert_eq!(destroyed.destroyed().collect::<Vec<_>>(), [alpha.as_str()]);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), old_bytes);
    let pass = tokio::time::timeout(VIEW_TIMEOUT, run_search_pass(test, id, account.secret()))
        .await
        .unwrap();
    let PassOutcome::Advanced {
        published: false,
        indexed_to,
    } = pass
    else {
        panic!("expected a completed deletion-only pass, got {pass:?}");
    };

    let new_bytes = manifest_bytes(test, id).await.unwrap();
    assert_ne!(new_bytes, old_bytes);
    let ManifestValue::Index(new_manifest) =
        ManifestValue::decode(&new_bytes, &za_search_key(keys.swk())).unwrap()
    else {
        panic!("missing new index manifest");
    };
    assert_eq!(new_manifest.newest_row, old_manifest.newest_row);
    assert_eq!(new_manifest.epoch, old_manifest.epoch);
    assert_eq!(new_manifest.content, old_manifest.content);
    assert_eq!(new_manifest.indexed_to, indexed_to);
    assert_eq!(new_manifest.content_to.checkpoint, indexed_to);
    assert!(indexed_to.complete && indexed_to.change_id > old_manifest.indexed_to.change_id);
    {
        let loaded = entry.read_loaded().await.unwrap();
        assert_eq!(loaded.manifest_bytes_hash, manifest_hash(&new_bytes));
        assert_eq!(loaded.manifest, new_manifest);
        assert_eq!(loaded.thread.exists_at(alpha_doc, u64::MAX), Some(false));
        let part = &loaded.content.as_ref().unwrap().part;
        assert!(!part.contains(alpha_doc));
        assert!(part.contains(beta_doc));

        // Every older check accepts this mismatched generation and cache.
        // Only the Content checkpoint floor forces a cache refresh.
        assert!(old_cache.email_by_id(&alpha_doc).is_some());
        assert!(counter_at_or_past(
            old_cache.last_change_id,
            &loaded.manifest
        ));
        assert!(is_current(&loaded, old_cache.last_change_id));
        assert!(old_cache.last_change_id < indexed_to.change_id);
    }

    let after = tokio::time::timeout(VIEW_TIMEOUT, parked.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.name_at(0), "Email/query", "{:?}", after.0);
    assert_eq!(
        after.ids().collect::<Vec<_>>(),
        [beta.as_str()],
        "parked reply: {:?}, pre-delete state: {}",
        after.0,
        before.method_response()["queryState"],
    );
    assert_ne!(
        after.method_response()["queryState"],
        before.method_response()["queryState"]
    );
    let fresh = account.jmap_method_call("Email/query", args).await;
    assert_eq!(fresh.name_at(0), "Email/query", "{:?}", fresh.0);
    assert_eq!(fresh.ids().collect::<Vec<_>>(), [beta.as_str()]);
    assert_eq!(
        after.method_response()["queryState"],
        fresh.method_response()["queryState"]
    );
    let view = za_view(&test.server, id, &keys, ViewNeed::Map)
        .await
        .unwrap();
    assert!(view.cache.last_change_id >= indexed_to.change_id);
    assert!(view.cache.email_by_id(&alpha_doc).is_none());
}

/// SAVE must use the previous result even after publishing its own channel.
async fn saved_search_previous_result(key: &Fixture, plain: &Fixture) {
    println!("saved_search_previous_result");
    for fixture in [plain, key] {
        let mut imap = imap_session(&fixture.account, b"_save_previous ").await;
        tokio::time::timeout(VIEW_TIMEOUT, async {
            imap.send("UID SEARCH RETURN (SAVE) $ BODY flying").await;
            imap.assert_read(Type::Tagged, ResponseType::No)
                .await
                .assert_contains("No saved search found.");
            imap.send_ok("UID SEARCH RETURN (SAVE) UID 1,4").await;
            assert_eq!(
                served(&mut imap, "UID SEARCH $", "* SEARCH").await,
                "* SEARCH 1 4"
            );
            imap.send_ok("UID SEARCH RETURN (SAVE) $ BODY flying").await;
            assert_eq!(
                served(&mut imap, "UID SEARCH $", "* SEARCH").await,
                "* SEARCH 1"
            );
            assert_eq!(
                served(&mut imap, "UID SEARCH UID $,2 BODY flying", "* SEARCH").await,
                "* SEARCH 1 2"
            );
            if std::ptr::eq(fixture, key) {
                // Expunges earlier in this fixture leave a UID gap before
                // sequence number 5. A mixed set keeps its sequence numbers.
                let seq = served(&mut imap, "UID SEARCH 5", "* SEARCH").await;
                let uid: u32 = seq.strip_prefix("* SEARCH ").unwrap().parse().unwrap();
                assert!(uid > 5, "{seq}");
                assert_eq!(
                    served(&mut imap, "UID SEARCH $,5 OR ALL BODY flying", "* SEARCH").await,
                    format!("* SEARCH 1 {uid}")
                );
                imap.send_ok("UID SEARCH RETURN (SAVE) UID $,2 BODY flying")
                    .await;
                assert_eq!(
                    served(&mut imap, "UID SEARCH $", "* SEARCH").await,
                    "* SEARCH 1 2"
                );
            }
            imap.send_ok("LOGOUT").await;
        })
        .await
        .expect("SAVE must not wait on its own channel");
    }
}

/// Drive the actual selected-mailbox workers with the watch registration
/// used by handle_search. A fresh delivery forces the producer's real pass.
async fn guarded_saved_search_dependency(test: &TestServer, account: &Account) {
    println!("guarded_saved_search_dependency");
    #[derive(Clone, Copy, Debug)]
    enum Consumer {
        Search,
        Sort,
        Thread,
        MapSearch,
        MapThread,
        ObjectIdFetch,
    }

    let id = account.id().document_id();
    let keys = za_unlock(test, id, account.secret()).await;
    let mut documents = Vec::new();
    for (subject, body) in [
        ("saved intersection", "savealpha savebeta"),
        ("saved producer only", "savealpha"),
        ("saved consumer only", "savebeta"),
    ] {
        documents.push(
            import_message(account, message_with(subject, body), &fifth())
                .await
                .parse::<Id>()
                .unwrap()
                .document_id(),
        );
    }
    let _ = content_answer(account).await;
    let cache = test.server.get_cached_messages(id).await.unwrap();
    let mailbox_view = MailboxView::build(&cache, INBOX_ID);
    let rows = mailbox_view.live_rows_by_uid();
    let uid = |doc| rows.iter().find(|row| row.id == doc).unwrap().uid;
    let intersection = uid(documents[0]);
    let mut saved = vec![intersection, uid(documents[1])];
    saved.sort_unstable();

    let mailbox = Arc::new(SelectedMailbox {
        id: MailboxId {
            account_id: id,
            mailbox_id: INBOX_ID,
        },
        view: mailbox_view.into(),
        saved_search: SavedSearch::None.into(),
        is_select: true,
        is_condstore: false,
    });
    let (_, stream_tx) = tokio::io::split(NullIo::default());
    let data = Arc::new(SessionData {
        account_id: id,
        access_token: AccessToken::from_permissions(id, []).with_session_keys(keys),
        server: test.server.clone(),
        session_id: 0,
        mailboxes: Default::default(),
        stream_tx: Arc::new(tokio::sync::Mutex::new(stream_tx)),
        state: Default::default(),
        remote_addr: "127.0.0.1".parse().unwrap(),
        in_flight: None,
    });

    for command in [
        Consumer::Search,
        Consumer::Sort,
        Consumer::Thread,
        Consumer::MapSearch,
        Consumer::MapThread,
        Consumer::ObjectIdFetch,
    ] {
        println!("saved-search consumer {command:?}");
        let previous = mailbox.get_saved_search().await;
        let (tx, rx) = watch::channel(Arc::new(Vec::new()));
        *mailbox.saved_search.lock() = SavedSearch::InFlight { rx };
        assert_eq!(tx.receiver_count(), 1);

        let consumer_mailbox = mailbox.clone();
        let consumer_data = data.clone();
        let mapped_intersection = Id::from_parts(documents[0], documents[0]).to_string();
        let consumer = tokio::spawn(async move {
            let filter = vec![
                ImapFilter::Sequence(Sequence::SavedSearch, true),
                if matches!(command, Consumer::MapSearch | Consumer::MapThread) {
                    ImapFilter::EmailId(mapped_intersection.into())
                } else {
                    ImapFilter::Body("savebeta".into())
                },
            ];
            match command {
                Consumer::Thread | Consumer::MapThread => {
                    let response = consumer_data
                        .thread(
                            imap_thread::Arguments {
                                tag: "consumer".into(),
                                algorithm: imap_thread::Algorithm::References,
                                filter,
                            },
                            consumer_mailbox,
                            true,
                            Instant::now(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.threads, [vec![intersection]]);
                }
                Consumer::ObjectIdFetch => {
                    consumer_data
                        .fetch(
                            imap_fetch::Arguments {
                                tag: "consumer".into(),
                                sequence_set: Sequence::SavedSearch,
                                attributes: vec![imap_fetch::Attribute::ObjectId],
                                changed_since: None,
                                include_vanished: false,
                            },
                            consumer_mailbox,
                            None,
                            true,
                            false,
                            false,
                            false,
                            false,
                            100,
                            Instant::now(),
                        )
                        .await
                        .unwrap();
                }
                Consumer::Search | Consumer::Sort | Consumer::MapSearch => {
                    let (response, _) = consumer_data
                        .search(
                            imap_search::Arguments {
                                tag: "consumer".into(),
                                is_esearch: false,
                                sort: matches!(command, Consumer::Sort).then(|| {
                                    vec![imap_search::Comparator {
                                        sort: imap_search::Sort::Subject,
                                        ascending: true,
                                    }]
                                }),
                                result_options: vec![],
                                filter,
                            },
                            consumer_mailbox,
                            None,
                            None,
                            true,
                            100,
                            Instant::now(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.ids, [intersection]);
                }
            }
        });

        // The only extra receiver is cloned by get_saved_search. There is
        // no producer to resolve it until after we observe that dependency.
        tokio::time::timeout(VIEW_TIMEOUT, async {
            while tx.receiver_count() == 1 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("consumer never waited on the saved result");
        assert_eq!(tx.receiver_count(), 2);
        let old_manifest = manifest_bytes(test, id).await.unwrap();
        deliver(
            test,
            "sender@example.org",
            account.name(),
            &message_with(
                "saved producer backlog",
                "new arrival for the producer's pass",
            ),
        )
        .await;
        let producer_data = data.clone();
        let producer_mailbox = mailbox.clone();
        let producer = HookParked::start(id, set_search_swap_pause, async move {
            producer_data
                .search(
                    imap_search::Arguments {
                        tag: "producer".into(),
                        is_esearch: true,
                        sort: None,
                        result_options: vec![
                            imap_search::ResultOption::Save,
                            imap_search::ResultOption::All,
                        ],
                        filter: vec![ImapFilter::Body("savealpha".into())],
                    },
                    producer_mailbox,
                    Some(tx),
                    Some(previous),
                    true,
                    100,
                    Instant::now(),
                )
                .await
        })
        .await;
        assert_ne!(manifest_bytes(test, id).await.unwrap(), old_manifest);
        let response = match tokio::time::timeout(VIEW_TIMEOUT, producer.finish()).await {
            Ok(response) => response.unwrap().0,
            Err(_) => {
                consumer.abort();
                let _ = consumer.await;
                panic!(
                    "{command:?} consumer held the index while the SAVE producer waited to swap"
                );
            }
        };
        assert_eq!(response.ids, saved);
        tokio::time::timeout(VIEW_TIMEOUT, consumer)
            .await
            .expect("saved-result consumer did not finish")
            .unwrap();
        let saved_rows = mailbox.get_saved_search().await.unwrap();
        assert_eq!(
            saved_rows.iter().map(|row| row.uid).collect::<Vec<_>>(),
            saved
        );
    }
}

/// Mail spec 5.5 and ruling 64: after the index passes, the key account's
/// search records are one document per message holding its ids only (1.0's
/// Email index has no arrival-time or size field: both are served from the
/// message cache) and no term; the ordinary account's include terms. Read
/// from the internal search store's `SearchDocument` and `SearchTerm`
/// subspaces (`utils::za::search_keys`).
async fn visible_field_document(test: &TestServer, key_id: u32, plain_id: u32) {
    println!("visible_field_document");
    if test.server.search_store().internal_fts().is_none() {
        return;
    }
    let key = search_keys(test, key_id, SearchIndex::Email).await;
    let mut documents = key.document_ids();
    documents.sort_unstable();
    assert_eq!(documents, message_ids(test, key_id).await, "documents");
    assert_eq!(documents.len(), 4);
    assert_eq!(key.terms, 0, "a key account's index holds no term");
    assert!(
        key.documents_are_ids_only(),
        "a key account's search document holds more than its ids: {key:?}"
    );
    let plain = search_keys(test, plain_id, SearchIndex::Email).await;
    let mut documents = plain.document_ids();
    documents.sort_unstable();
    assert_eq!(documents, message_ids(test, plain_id).await);
    assert!(plain.terms > 0, "the control's index holds terms");
    assert!(
        !plain.documents_are_ids_only(),
        "the control's documents list their terms"
    );
}
