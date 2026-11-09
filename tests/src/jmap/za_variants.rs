/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account JMAP variants (mail spec 11), run with `ZA_KEY_ACCOUNTS=1`.
//! Keyed search and threading use upstream's results (release 7 spec 9.3).
//! The remaining differences cover keyless mail calls, cross-account copy,
//! sharing, raw sealed sources and push content. The admin and Sales group
//! stay plain. The calendar variants assert the key-account refusal.

use crate::{
    jmap::{
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
        jmap::{ChangeType, JmapResponse, JmapUtils},
        push::spawn_mock_push_server,
        server::{DestroyAllMailboxes, TestServer},
        smtp::SmtpConnection,
        za::{
            Keyless, finish_bulk_delivery_indexing, za_grant_token, za_index_current,
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
use common::{
    auth::{AccessToken, oauth::scope::OAuthScopes},
    storage::index::ObjectIndexBuilder,
    za::ZA_MAIL_KEYLESS,
};
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
    rand::{self, RngExt},
    write::BatchBuilder,
};
use tokio::sync::mpsc;
use types::{
    collection::{Collection, SyncCollection},
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

/// `mail::query::test` in key mode: upstream's corpus, assertions and
/// options, with the index made current by a keyed content request in place
/// of the queue wait; a keyless token is refused on `Email/query` and
/// `SearchSnippet/get` (release 7 spec 6.2).
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
    let ids = account
        .jmap_method_call("Email/query", json!({ "accountId": account.id_string() }))
        .await;
    assert_eq!(ids.name_at(0), "Email/query", "{ids:?}");
    let ids = ids.ids().collect::<Vec<_>>();
    let mut threads = AHashSet::new();
    for chunk in ids.chunks(100) {
        let reply = account
            .jmap_method_call(
                "Email/get",
                json!({
                    "accountId": account.id_string(),
                    "ids": chunk,
                    "properties": ["id", "threadId"],
                }),
            )
            .await;
        assert_eq!(reply.name_at(0), "Email/get", "{reply:?}");
        assert_eq!(
            reply
                .list()
                .iter()
                .map(|email| email.text_field("id"))
                .collect::<AHashSet<_>>(),
            chunk.iter().copied().collect::<AHashSet<_>>(),
            "{reply:?}"
        );
        threads.extend(
            reply
                .list()
                .iter()
                .map(|email| email.text_field("threadId").to_string()),
        );
    }
    // MAX_THREADS in mail/query.rs. The cached rows hold singleton ids.
    assert_eq!(threads.len(), 100);

    upstream_query::query(&client, true).await;
    upstream_query::query_options(&client).await;
    keyless_query_refused(test, account, ids[0]).await;

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
        // New at 1.0 (0.16 answered `cannotCalculateChanges`): refused as
        // `/get` and `/set` are, naming the request's account.
        (
            "ParticipantIdentity/changes",
            json!({ "accountId": id, "sinceState": "n" }),
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

    // 1.0's session object sets the per-account limits (mailboxes per email,
    // address books per card, file node depth), lists more file-node sort
    // options and the node types, CalendarEvent and ContactCard blobs and the
    // principals:owner capability, and drops imapsieve from the Sieve
    // extensions.
    // Validate session object capabilities: upstream's object, minus the
    // account's calendar capabilities (release 1 spec 9).
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
    for (item, (id, email, description)) in list.iter().zip([
        (john_id, "jdoe@example.com", "John Doe"),
        (jane_id, "jane.smith@example.com", "Jane Smith"),
        (bill_id, "bill@example.com", "Bill Foobar"),
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
