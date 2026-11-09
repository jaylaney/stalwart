/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        jmap::JmapResponse,
        server::TestServer,
        za::{
            HookParked, log_records_since, manifest_bytes, read_manifest, wait_for_delivery,
            za_unlock,
        },
    },
    za::{
        mail::{deliver, key_mail_account, message_with, new_message, reply_to},
        user_permissions,
    },
};
use common::za::test_hooks::set_search_swap_pause;
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    mailbox::INBOX_ID,
    message::za::search::pass::{PassOutcome, ZaSearchPass},
};
use imap_proto::ResponseType;
use jmap_proto::types::state::State;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
use store::{
    query::log::Change,
    za_search::manifest::{ManifestValue, manifest_hash},
};
use types::{collection::SyncCollection, id::Id};

const PASSWORD: &str = "A strong password for the thread controls";
const SENDER: &str = "sender@example.org";

pub(super) async fn test(test: &mut TestServer) {
    println!("Running zero-access mapped thread-valued read tests...");
    let admin = test.account("admin@example.com").clone();
    let key52 = key_mail_account(test, "keymail52@example.com", PASSWORD).await;
    let plain52 = admin
        .create_user_account(
            "plainmail52@example.com",
            PASSWORD,
            "Plain mail",
            &[],
            user_permissions(),
        )
        .await;
    let key53 = key_mail_account(test, "keymail53@example.com", PASSWORD).await;
    let plain53 = admin
        .create_user_account(
            "plainmail53@example.com",
            PASSWORD,
            "Plain mail",
            &[],
            user_permissions(),
        )
        .await;

    case_conversation_parity(test, &key52, &plain52).await;
    case_g7_mailbox_counts(&key52, &plain52).await;
    case_pl9_query_fast_paths(test, &key53, &plain53).await;
    case_singleton_until_keyed_request(test, &key52).await;
    case_thread_get_during_local_swap(test, &key52).await;
    // Clear pair 53 before the K fixture so its memberships are unambiguous.
    for account in [&key53, &plain53] {
        destroy(
            account,
            messages(account)
                .await
                .into_values()
                .map(|(id, _)| id)
                .collect(),
        )
        .await;
    }
    case_f1_k_bound(test, &key53, &plain53).await;
    // Move cleanup to the enclosing test's end when later tasks consume pair 52.
    for account in [&key52, &plain52, &key53, &plain53] {
        test.destroy_all_mailboxes(account).await;
    }
    wait_for_delivery(test).await;
}

async fn call(account: &Account, method: &str, mut args: Value) -> JmapResponse {
    args["accountId"] = json!(account.id_string());
    let result = account.jmap_method_call(method, args).await;
    assert_eq!(result.name_at(0), method, "{result:?}");
    result
}

fn message_id(raw: &str) -> &str {
    raw.lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap()
        .trim_matches(['<', '>'])
}

// Fetch current IDs anew after a merge, and identify messages by Message-ID.
async fn messages(account: &Account) -> HashMap<String, (String, String)> {
    let result = call(
        account,
        "Email/get",
        json!({"properties": ["id", "threadId", "messageId"]}),
    )
    .await;
    result
        .list()
        .iter()
        .map(|email| {
            (
                email["messageId"][0].as_str().unwrap().to_owned(),
                (
                    email["id"].as_str().unwrap().to_owned(),
                    email["threadId"].as_str().unwrap().to_owned(),
                ),
            )
        })
        .collect()
}

fn names<'a>(
    current: &HashMap<String, (String, String)>,
    ids: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    ids.into_iter()
        .map(|id| {
            current
                .iter()
                .find(|(_, (mapped, _))| mapped == id)
                .unwrap_or_else(|| panic!("unknown current Email id {id}"))
                .0
                .clone()
        })
        .collect()
}

fn group_names(current: &HashMap<String, (String, String)>, thread: &Value) -> Vec<String> {
    let mut members = names(
        current,
        thread["emailIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap()),
    );
    members.sort_unstable();
    members
}

async fn import(account: &Account, raw: &str, position: usize) {
    let blob = account
        .jmap_client()
        .await
        .upload(None, raw.as_bytes().to_vec(), None)
        .await
        .unwrap()
        .take_blob_id();
    let inbox = Id::from(INBOX_ID).to_string();
    let result = call(
        account,
        "Email/import",
        json!({"emails": {"i": {
            "blobId": blob,
            "mailboxIds": {(inbox): true},
            "receivedAt": format!("2026-10-20T10:00:{position:02}Z"),
        }}}),
    )
    .await;
    assert!(
        result
            .pointer("/methodResponses/0/1/created/i/id")
            .is_some(),
        "{result:?}"
    );
}

async fn seen(account: &Account, id: &str) {
    let result = call(
        account,
        "Email/set",
        json!({"update": {(id): {"keywords/$seen": true}}}),
    )
    .await;
    result.updated(id);
}

async fn destroy(account: &Account, ids: Vec<String>) {
    let result = call(account, "Email/set", json!({"destroy": ids})).await;
    assert_eq!(result.destroyed().count(), ids.len(), "{result:?}");
}

fn references(raw: String, ids: &[&str]) -> String {
    let header = ids
        .iter()
        .map(|id| format!("<{id}>"))
        .collect::<Vec<_>>()
        .join(" ");
    raw.replacen("\r\n\r\n", &format!("\r\nReferences: {header}\r\n\r\n"), 1)
}

async fn case_conversation_parity(test: &TestServer, key: &Account, plain: &Account) {
    let r = message_with("Task12 conversation", "root");
    let a = reply_to(&r, "Task12 conversation", "reply");
    let b = references(
        message_with("Re: Task12 conversation", "nested"),
        &[message_id(&r), message_id(&a)],
    );
    let f = references(
        message_with("Fwd: Task12 conversation", "forward"),
        &[message_id(&r)],
    );
    let u = message_with("Task12 unrelated", "unrelated");
    let c = reply_to(&a, "Task12 changed subject", "changed");
    let mut expected_group = [&r, &a, &b, &f]
        .map(|raw| message_id(raw).to_owned())
        .to_vec();
    expected_group.sort_unstable();
    let mut expected_groups = vec![
        expected_group.clone(),
        vec![message_id(&u).to_owned()],
        vec![message_id(&c).to_owned()],
    ];
    expected_groups.sort_unstable();

    for account in [plain, key] {
        for (n, raw) in [&r, &a, &b, &f, &u, &c].into_iter().enumerate() {
            import(account, raw, n).await;
        }
        wait_for_delivery(test).await;
        let current = messages(account).await;
        let result = call(account, "Thread/get", json!({})).await;
        let mut groups = result
            .list()
            .iter()
            .map(|thread| group_names(&current, thread))
            .collect::<Vec<_>>();
        groups.sort_unstable();
        assert_eq!(groups, expected_groups, "{}", account.name());

        let thread = &current[message_id(&r)].1;
        let absent = Id::from(u32::MAX - 1).to_string();
        let explicit = call(account, "Thread/get", json!({"ids": [thread, &absent]})).await;
        assert_eq!(explicit.list().len(), 1, "{explicit:?}");
        assert_eq!(group_names(&current, &explicit.list()[0]), expected_group);
        assert_eq!(explicit.not_found().collect::<Vec<_>>(), [absent.as_str()]);

        let collapsed = call(
            account,
            "Email/query",
            json!({
                "collapseThreads": true, "sort": [{"property": "receivedAt", "isAscending": true}],
            }),
        )
        .await;
        assert_eq!(
            names(&current, collapsed.ids()),
            [&r, &u, &c].map(|raw| message_id(raw).to_owned())
        );

        let mut imap = ImapConnection::connect(b"_t12 ").await;
        imap.authenticate(account.name(), account.secret()).await;
        imap.send_ok("SELECT INBOX").await;
        for (filter, expected) in [
            ("REFERENCES UTF-8 ALL", "(1 2 3 4)(5)(6)"),
            ("ORDEREDSUBJECT UTF-8 ALL", "(1 2 3 4)(5)(6)"),
            ("REFERENCES UTF-8 SUBJECT conversation", "(1 2 3 4)"),
        ] {
            imap.send(&format!("UID THREAD {filter}")).await;
            let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
            let groups = lines
                .iter()
                .find_map(|line| line.strip_prefix("* THREAD "))
                .unwrap();
            // Fresh INBOXes receive the exact same inserts in the same order.
            assert_eq!(groups, expected, "{}: {filter}", account.name());
        }
        imap.send_ok("LOGOUT").await;

        seen(account, &current[message_id(&a)].0).await;
    }
}

async fn case_g7_mailbox_counts(key: &Account, plain: &Account) {
    for account in [plain, key] {
        let result = call(
            account,
            "Mailbox/get",
            json!({
                "ids": [Id::from(INBOX_ID).to_string()],
                "properties": ["totalEmails", "unreadEmails", "totalThreads", "unreadThreads"],
            }),
        )
        .await;
        assert_eq!(result.list().len(), 1, "{result:?}");
        let inbox = &result.list()[0];
        assert_eq!(
            [
                "totalEmails",
                "unreadEmails",
                "totalThreads",
                "unreadThreads"
            ]
            .map(|property| inbox[property].as_u64().unwrap()),
            [6, 5, 3, 3],
            "{}",
            account.name(),
        );
    }
}

fn attachment_body() -> &'static str {
    concat!(
        "--task12\r\nContent-Type: text/plain\r\n\r\nBody\r\n",
        "--task12\r\nContent-Type: text/plain; name=note.txt\r\n",
        "Content-Disposition: attachment; filename=note.txt\r\n\r\nAttachment payload\r\n",
        "--task12--",
    )
}

fn multipart(raw: String) -> String {
    raw.replacen(
        "\r\n\r\n",
        "\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=task12\r\n\r\n",
        1,
    )
}

async fn case_pl9_query_fast_paths(test: &TestServer, key: &Account, plain: &Account) {
    let a = message_with("Task12 flags", "root");
    let b = multipart(reply_to(&a, "Task12 flags", attachment_body()));
    let c = multipart(message_with("Task12 attached", attachment_body()));
    let [a_name, b_name, c_name] = [&a, &b, &c].map(|raw| message_id(raw).to_owned());
    let received = json!({"property": "receivedAt", "isAscending": true});
    for account in [plain, key] {
        for (n, raw) in [&a, &b, &c].into_iter().enumerate() {
            import(account, raw, n).await;
        }
        wait_for_delivery(test).await;
        let current = messages(account).await;
        seen(account, &current[&b_name].0).await;
        let in_thread = json!({"inThread": current[&a_name].1});
        for (filter, expected) in [
            (in_thread.clone(), vec![a_name.clone(), b_name.clone()]),
            (
                json!({"operator": "OR", "conditions": [in_thread, {"hasKeyword": "$flagged"}]}),
                vec![a_name.clone(), b_name.clone()],
            ),
            (
                json!({"someInThreadHaveKeyword": "$seen"}),
                vec![a_name.clone(), b_name.clone()],
            ),
            (json!({"allInThreadHaveKeyword": "$seen"}), vec![]),
            (
                json!({"noneInThreadHaveKeyword": "$seen"}),
                vec![c_name.clone()],
            ),
            (
                json!({"hasAttachment": true}),
                vec![b_name.clone(), c_name.clone()],
            ),
            (
                json!({"someInThreadHaveKeyword": "$hasattachment"}),
                vec![a_name.clone(), b_name.clone(), c_name.clone()],
            ),
            (
                json!({"allInThreadHaveKeyword": "$hasattachment"}),
                vec![c_name.clone()],
            ),
            (json!({"noneInThreadHaveKeyword": "$hasattachment"}), vec![]),
        ] {
            let result = call(
                account,
                "Email/query",
                json!({"filter": filter, "sort": [&received]}),
            )
            .await;
            assert_eq!(
                names(&current, result.ids()),
                expected,
                "{}: {filter}",
                account.name()
            );
        }

        for (position, expected) in [(0, &a_name), (-1, &c_name)] {
            let result = call(account, "Email/query", json!({
                "collapseThreads": true, "limit": 1, "calculateTotal": true, "position": position,
                "sort": [&received],
            })).await;
            assert_eq!(
                result.method_response()["total"].as_u64(),
                Some(2),
                "{result:?}"
            );
            assert_eq!(names(&current, result.ids()), [expected.to_owned()]);
        }

        let result = call(
            account,
            "Email/query",
            json!({"sort": [
                {"property": "someInThreadHaveKeyword", "keyword": "$seen", "isAscending": true},
                &received,
            ]}),
        )
        .await;
        assert_eq!(
            names(&current, result.ids()),
            [c_name.clone(), a_name.clone(), b_name.clone()]
        );

        for (property, ascending, expected) in [
            (
                "hasKeyword",
                true,
                [a_name.clone(), b_name.clone(), c_name.clone()],
            ),
            (
                "hasKeyword",
                false,
                [b_name.clone(), c_name.clone(), a_name.clone()],
            ),
            (
                "someInThreadHaveKeyword",
                false,
                [a_name.clone(), b_name.clone(), c_name.clone()],
            ),
            (
                "allInThreadHaveKeyword",
                false,
                [c_name.clone(), a_name.clone(), b_name.clone()],
            ),
        ] {
            let result = call(
                account,
                "Email/query",
                json!({"sort": [
                    {"property": property, "keyword": "$hasattachment", "isAscending": ascending},
                    &received,
                ]}),
            )
            .await;
            assert_eq!(
                names(&current, result.ids()),
                expected,
                "{}",
                account.name()
            );
        }
    }
}

async fn case_singleton_until_keyed_request(test: &TestServer, key: &Account) {
    let id = key.id().document_id();
    let r = message_with("Task12 offline", "root");
    let a = reply_to(&r, "Task12 offline", "reply");
    import(key, &r, 10).await;
    let root_thread = messages(key).await[message_id(&r)].1.clone();
    let Some(ManifestValue::Index(before)) = read_manifest(test, id).await else {
        panic!("missing manifest after root");
    };
    let documents = crate::za::mail::message_ids(test, id).await;
    deliver(test, SENDER, key.name(), &a).await;
    let reply_doc = new_message(test, id, &documents).await;
    let cache = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(
        cache.email_by_id(&reply_doc).unwrap().thread_id(),
        reply_doc
    );
    let delivery = log_records_since(test, id, SyncCollection::Email, before.indexed_to.change_id)
        .await
        .into_iter()
        .find(|(_, changes)| {
            changes.contains(&Change::InsertItem(
                Id::from_parts(reply_doc, reply_doc).id(),
            ))
        })
        .expect("LMTP delivery logged the singleton")
        .0;
    let Some(ManifestValue::Index(offline)) = read_manifest(test, id).await else {
        panic!("missing manifest after offline reply");
    };
    assert_eq!(offline.indexed_to, before.indexed_to);
    assert!(offline.indexed_to.change_id < delivery);

    let result = call(key, "Thread/get", json!({"ids": [root_thread]})).await;
    assert_eq!(result.list().len(), 1, "{result:?}");
    let Some(ManifestValue::Index(after)) = read_manifest(test, id).await else {
        panic!("missing manifest after Thread/get");
    };
    assert!(after.indexed_to.change_id >= delivery);
    let current = messages(key).await;
    let mut expected = [&r, &a].map(|raw| message_id(raw).to_owned()).to_vec();
    expected.sort_unstable();
    assert_eq!(group_names(&current, &result.list()[0]), expected);
}

// As in Task 10's Map acquisition test, the pass is durable but the
// resident entry has not swapped. Exercise a merged thread at the wire.
async fn case_thread_get_during_local_swap(test: &TestServer, key: &Account) {
    let account_id = key.id().document_id();
    let subject = "Task12 local swap";
    let a = message_with(subject, "independent root A");
    let b = message_with(subject, "independent root B");
    import(key, &a, 30).await;
    import(key, &b, 31).await;
    let current = messages(key).await;
    let a_thread = &current[message_id(&a)].1;
    let b_thread = &current[message_id(&b)].1;
    assert_ne!(a_thread, b_thread);
    let before = call(key, "Thread/get", json!({"ids": [a_thread, b_thread]})).await;
    assert_eq!(before.list().len(), 2);
    assert!(
        before
            .list()
            .iter()
            .all(|row| row["emailIds"].as_array().unwrap().len() == 1)
    );

    let docs = crate::za::mail::message_ids(test, account_id).await;
    let bridge = references(
        message_with(subject, "bridge"),
        &[message_id(&a), message_id(&b)],
    );
    let old_hash = manifest_hash(&manifest_bytes(test, account_id).await.unwrap());
    deliver(test, SENDER, key.name(), &bridge).await;
    let bridge_doc = new_message(test, account_id, &docs).await;
    let server = test.server.clone();
    let keys = za_unlock(test, account_id, key.secret()).await;
    let pass = HookParked::start(account_id, set_search_swap_pause, async move {
        server.za_search_pass(account_id, &keys).await.unwrap()
    })
    .await;
    let entry = test.server.inner.cache.za_search.get(account_id).unwrap();
    assert_eq!(
        entry.read_loaded().await.unwrap().manifest_bytes_hash,
        old_hash
    );
    assert!(entry.pass_lock.try_lock().is_err());
    assert_ne!(
        old_hash,
        manifest_hash(&manifest_bytes(test, account_id).await.unwrap())
    );
    let Some(ManifestValue::Index(manifest)) = read_manifest(test, account_id).await else {
        panic!("missing manifest for the parked publication");
    };

    let a_id: Id = current[message_id(&a)].0.parse().unwrap();
    let b_id: Id = current[message_id(&b)].0.parse().unwrap();
    let survivor = a_id.document_id().min(b_id.document_id());
    let loser = a_id.document_id().max(b_id.document_id());
    let ids = json!([Id::from(survivor).to_string(), Id::from(loser).to_string()]);
    let timeout = Duration::from_secs(5);
    let during = tokio::time::timeout(timeout, call(key, "Thread/get", json!({"ids": &ids})))
        .await
        .expect("Thread/get waited for the parked swap");
    assert!(entry.pass_lock.try_lock().is_err());
    assert_eq!(
        State::parse(during.state()),
        Some(State::Exact(manifest.newest_row.change_id))
    );

    assert!(matches!(
        tokio::time::timeout(timeout, pass.finish()).await.unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        },
    ));
    let fresh = call(key, "Thread/get", json!({"ids": ids})).await;
    assert_eq!(during.method_response(), fresh.method_response());
    let mut docs = [a_id.document_id(), b_id.document_id(), bridge_doc];
    docs.sort_unstable();
    assert_eq!(during.list().len(), 1, "{during:?}");
    assert_eq!(
        during.list()[0]["emailIds"],
        json!(docs.map(|doc| Id::from_parts(survivor, doc).to_string()))
    );
    assert_eq!(
        during.not_found().collect::<Vec<_>>(),
        [Id::from(loser).to_string().as_str()]
    );
}

async fn case_f1_k_bound(test: &TestServer, key: &Account, plain: &Account) {
    let roots = (1..=20)
        .map(|n| message_with("Task12 long references", &format!("root {n}")))
        .collect::<Vec<_>>();
    let ids = roots.iter().map(|raw| message_id(raw)).collect::<Vec<_>>();
    let m = references(
        message_with("Re: Task12 long references", "nearest present"),
        &ids,
    );
    let n = references(
        message_with("Re: Task12 long references", "nearest absent"),
        &ids,
    );
    for (account, is_key) in [(plain, false), (key, true)] {
        for (position, raw) in roots.iter().chain([&m]).enumerate() {
            import(account, raw, position).await;
        }
        wait_for_delivery(test).await;
        let current = messages(account).await;
        let m_thread = &current[message_id(&m)].1;
        assert!(ids[5..].iter().all(|id| &current[*id].1 == m_thread));
        for id in &ids[..5] {
            assert_eq!(
                &current[*id].1 == m_thread,
                !is_key,
                "{}: {id}",
                account.name()
            );
        }
        let joined = call(account, "Thread/get", json!({"ids": [m_thread]})).await;
        assert_eq!(joined.list().len(), 1, "{joined:?}");
        let mut expected = ids[if is_key { 5 } else { 0 }..]
            .iter()
            .copied()
            .chain([message_id(&m)])
            .map(str::to_owned)
            .collect::<Vec<_>>();
        expected.sort_unstable();
        assert_eq!(group_names(&current, &joined.list()[0]), expected);
        // Remove M as well as r5..r20. A surviving M would match N's
        // retained references even with the nearest roots absent.
        let current = messages(account).await;
        destroy(
            account,
            ids[4..]
                .iter()
                .copied()
                .chain([message_id(&m)])
                .map(|id| current[id].0.clone())
                .collect(),
        )
        .await;
        wait_for_delivery(test).await;
        import(account, &n, 21).await;
        wait_for_delivery(test).await;
        let current = messages(account).await;
        assert_eq!(current.len(), 5, "{}", account.name());
        let n_thread = &current[message_id(&n)].1;
        let r1_thread = &current[ids[0]].1;
        if is_key {
            assert_ne!(
                n_thread, r1_thread,
                "7.5: retained references have no live match"
            );
        } else {
            assert_eq!(n_thread, r1_thread, "plain retains the older references");
        }
        let result = call(account, "Thread/get", json!({"ids": [n_thread]})).await;
        assert_eq!(result.list().len(), 1, "{result:?}");
        let mut expected = vec![message_id(&n).to_owned()];
        if !is_key {
            expected.extend(ids[..4].iter().map(|id| (*id).to_owned()));
        }
        expected.sort_unstable();
        assert_eq!(
            group_names(&current, &result.list()[0]),
            expected,
            "{}",
            account.name()
        );
    }
}
