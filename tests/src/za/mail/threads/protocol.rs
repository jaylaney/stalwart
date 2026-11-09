/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{email_create, email_id, get, mapped, method, reply_to, set, singleton};
use crate::{
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        server::TestServer,
    },
    za::mail::{message_ids, new_message},
};
use email::{mailbox::INBOX_ID, submission::EmailSubmission};
use imap_proto::ResponseType;
use serde_json::json;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
};
use types::{collection::Collection, id::Id};

pub(super) async fn test(test: &TestServer, key: &Account, root: &str, root_id: u32) {
    let doc = case_append_fetch_search(test, key, root, root_id).await;
    case_blob_lookup(key, root_id, doc).await;
    case_submission_same_snapshot(test, key, root, root_id).await;
}

async fn case_append_fetch_search(
    test: &TestServer,
    key: &Account,
    root: &str,
    root_id: u32,
) -> u32 {
    let account_id = key.id().document_id();
    let before = message_ids(test, account_id).await;
    let raw = reply_to(root, "Task 11 mapped id", "keyed APPEND reply");
    let mut imap = ImapConnection::connect(b"_t11 ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    imap.send(&format!("APPEND INBOX {{{}+}}\r\n{raw}", raw.len()))
        .await;
    let uid = imap
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .into_append_uid();
    let doc = new_message(test, account_id, &before).await;
    let expected = mapped(root_id, doc);
    assert_ne!(expected, singleton(doc));

    imap.send(&format!("UID FETCH {uid} (OBJECTID)")).await;
    let fetch = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let line = fetch
        .iter()
        .find(|line| line.contains("OBJECTID ("))
        .unwrap();
    let component = |name: &str| {
        line.split_once(name)
            .unwrap_or_else(|| panic!("missing {name}: {line}"))
            .1
            .split([' ', ')'])
            .next()
            .unwrap()
    };
    assert_eq!(component("EMAILID "), expected);
    assert_eq!(component("THREADID "), Id::from(root_id).to_string());

    for (supplied, expected_count) in [(&expected, 1), (&singleton(doc), 0)] {
        imap.send(&format!("UID SEARCH RETURN (COUNT) EMAILID {supplied}"))
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_contains(&format!("COUNT {expected_count}"));
    }
    imap.send(&format!(
        "UID SEARCH RETURN (ALL) THREADID {}",
        Id::from(root_id)
    ))
    .await;
    let search = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let all = search
        .iter()
        .find_map(|line| line.split_once(" ALL ").map(|(_, set)| set.trim()))
        .expect("THREADID search returned an ALL set");
    assert!(
        crate::imap::expand_uid_list(all).contains(&uid.parse().unwrap()),
        "{search:?}"
    );
    imap.send_ok(&format!("UID SEARCH RETURN (SAVE) EMAILID {expected}"))
        .await;
    imap.send("UID FETCH $ (OBJECTID)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(&format!("EMAILID {expected} "));
    imap.send_ok(&format!("UID SEARCH RETURN (SAVE) UID $,{uid}"))
        .await;
    imap.send("UID SEARCH RETURN (COUNT) $").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("COUNT 1");
    imap.send_ok("LOGOUT").await;
    doc
}

async fn case_blob_lookup(key: &Account, thread: u32, doc: u32) {
    let id = mapped(thread, doc);
    let get = key
        .jmap_method_call(
            "Email/get",
            json!({ "accountId": key.id_string(), "ids": [&id], "properties": ["id", "blobId"] }),
        )
        .await;
    let row = &method(&get, 0, "Email/get")["list"][0];
    assert_eq!(row["id"], id);
    let blob = row["blobId"].as_str().expect("linked Email blob ID");
    let response = key
        .jmap_method_call(
            "Blob/lookup",
            json!({
                "accountId": key.id_string(),
                "ids": [blob],
                "typeNames": ["Email", "Thread", "Mailbox"]
            }),
        )
        .await;
    let lookup = method(&response, 0, "Blob/lookup");
    assert_eq!(lookup["list"][0]["id"], blob);
    let matches = &lookup["list"][0]["matchedIds"];
    assert_eq!(matches["Email"], json!([id]));
    assert_eq!(matches["Thread"], json!([Id::from(thread).to_string()]));
    assert_eq!(matches["Mailbox"], json!([Id::from(INBOX_ID).to_string()]));
}

async fn case_submission_same_snapshot(test: &TestServer, key: &Account, root: &str, root_id: u32) {
    let draft = email_create(key, "Re: Task 11 mapped id", Some(root));
    let created = set(key, json!({ "create": { "submission-draft": draft } })).await;
    let draft = &method(&created, 0, "Email/set")["created"]["submission-draft"];
    let draft_id = email_id(&draft["id"]);
    assert_eq!(draft_id.prefix_id(), root_id);
    let doc = draft_id.document_id();
    assert_ne!(doc, root_id);
    let current = draft_id.to_string();

    let identities = key
        .jmap_method_call(
            "Identity/get",
            json!({ "accountId": key.id_string(), "properties": ["id", "email"] }),
        )
        .await;
    let identity = method(&identities, 0, "Identity/get")["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|identity| identity["email"] == key.name())
        .expect("the account's own identity")["id"]
        .as_str()
        .unwrap();

    // Hold delivery and use only the test account as the recipient. The
    // cancellation below removes the held queue item before test cleanup.
    let envelope = json!({
        "mailFrom": { "email": key.name(), "parameters": { "HOLDFOR": "300" } },
        "rcptTo": [{ "email": key.name() }]
    });
    let submit = |supplied: &str| {
        json!({
            "accountId": key.id_string(),
            "create": { "s": { "identityId": identity, "emailId": supplied, "envelope": envelope } },
            "onSuccessUpdateEmail": { "#s": { "keywords/$flagged": true } }
        })
    };
    let stale = key
        .jmap_method_call("EmailSubmission/set", submit(&singleton(doc)))
        .await;
    assert_eq!(
        method(&stale, 0, "EmailSubmission/set")["notCreated"]["s"]["type"],
        "notFound"
    );
    assert_eq!(stale.0["methodResponses"].as_array().unwrap().len(), 1);

    let submitted = key
        .jmap_method_call("EmailSubmission/set", submit(&current))
        .await;
    let outcome = &method(&submitted, 0, "EmailSubmission/set")["created"]["s"];
    let submission = outcome["id"].as_str().expect("submission ID");
    assert_eq!(outcome["undoStatus"], "pending");
    assert_email_updated(&submitted, &current);
    let response = key
        .jmap_method_call(
            "EmailSubmission/get",
            json!({
                "accountId": key.id_string(), "ids": [submission],
                "properties": ["id", "emailId", "threadId"]
            }),
        )
        .await;
    let row = &method(&response, 0, "EmailSubmission/get")["list"][0];
    assert_eq!(row["id"], submission);
    assert_eq!(row["emailId"], current);
    assert_eq!(row["threadId"], Id::from(root_id).to_string());

    let stored = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            key.id().document_id(),
            Collection::EmailSubmission,
            submission.parse::<Id>().unwrap().document_id(),
        ))
        .await
        .unwrap()
        .expect("stored submission");
    let stored = stored.unarchive::<EmailSubmission>().unwrap();
    assert_eq!(u32::from(stored.email_id), doc);
    assert_eq!(u32::from(stored.thread_id), doc);

    let after_create = get(key, json!([&current])).await;
    assert_eq!(
        method(&after_create, 0, "Email/get")["list"][0]["keywords"]["$flagged"],
        true
    );

    let canceled = key
        .jmap_method_call(
            "EmailSubmission/set",
            json!({
                "accountId": key.id_string(),
                "update": { (submission): { "undoStatus": "canceled" } },
                "onSuccessUpdateEmail": { (submission): { "keywords/$seen": true } }
            }),
        )
        .await;
    assert!(
        method(&canceled, 0, "EmailSubmission/set")["updated"]
            .get(submission)
            .is_some()
    );
    assert_email_updated(&canceled, &current);
    let after_cancel = get(key, json!([&current])).await;
    assert_eq!(
        method(&after_cancel, 0, "Email/get")["list"][0]["keywords"]["$seen"],
        true
    );

    let deleted = key
        .jmap_method_call(
            "EmailSubmission/set",
            json!({
                "accountId": key.id_string(),
                "destroy": [submission],
                "onSuccessDestroyEmail": [submission]
            }),
        )
        .await;
    assert_eq!(
        method(&deleted, 0, "EmailSubmission/set")["destroyed"],
        json!([submission])
    );
    assert_eq!(
        method(&deleted, 1, "Email/set")["destroyed"],
        json!([&current])
    );
    let readback = get(key, json!([&current])).await;
    assert_eq!(
        method(&readback, 0, "Email/get")["notFound"],
        json!([&current])
    );
}

fn assert_email_updated(response: &crate::utils::jmap::JmapResponse, expected_id: &str) {
    let response = method(response, 1, "Email/set");
    assert!(response["updated"].get(expected_id).is_some(), "{response}");
    assert!(response["notUpdated"].is_null(), "{response}");
}
