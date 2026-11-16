/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.9: a key account's participant identities, where only the
//! default can be switched.

use super::{finish, key_account};
use crate::{
    utils::{account::Account, jmap::JmapResponse, server::TestServer},
    za::{STRONG, gating::method_error, user_permissions},
};
use serde_json::{Value, json};
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
};
use types::{collection::Collection, field::PrincipalField};

const FIXED: &str = "Participant identities in this account cannot be created, changed or destroyed; only the default identity can be switched.";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar refusal tests...");
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(
            "jmapcal-refusals@example.com",
            STRONG,
            "jmapcal-refusals",
            &["jmapcal-refusals-alias@example.com"],
            user_permissions(),
        )
        .await;
    test.insert_account(owner.clone());
    key_account(test, "refusals-2", user_permissions()).await;
    identity_set_only_switches_default(test, &owner).await;
    finish(test, "refusals").await;
}

async fn identities(owner: &Account) -> Value {
    let reply = owner
        .jmap_method_call(
            "ParticipantIdentity/get",
            json!({ "accountId": owner.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    Value::Array(reply.list().to_vec())
}

async fn set(owner: &Account, args: Value) -> JmapResponse {
    let mut args = args;
    args["accountId"] = json!(owner.id_string());
    owner
        .jmap_method_call("ParticipantIdentity/set", args)
        .await
}

/// Release 6 spec 6.9: every create, update and destroy of a key account's
/// identities is refused with one fixed text; the default can be switched.
async fn identity_set_only_switches_default(test: &TestServer, owner: &Account) {
    let account_id = owner.id().document_id();
    let identity = |id: &str, address: &str, default: bool| {
        json!({
            "id": id,
            "name": "jmapcal-refusals",
            "calendarAddress": format!("mailto:{address}"),
            "isDefault": default,
        })
    };
    let expected = |default: &str| {
        json!([
            identity("a", "jmapcal-refusals@example.com", default == "a"),
            identity("b", "jmapcal-refusals-alias@example.com", default == "b"),
        ])
    };

    let reply = set(
        owner,
        json!({
            "create": { "c": {
                "name": "jmapcal identity canary",
                "calendarAddress": "mailto:jmapcal-refusals-alias@example.com",
            } },
            "update": { "a": { "name": "jmapcal identity canary" } },
            "destroy": ["b"],
        }),
    )
    .await;
    let response = reply.response_at(0);
    let refused = json!({ "type": "forbidden", "description": FIXED });
    assert_eq!(response["notCreated"]["c"], refused, "{reply:?}");
    assert_eq!(response["notUpdated"]["a"], refused, "{reply:?}");
    assert_eq!(response["notDestroyed"]["b"], refused, "{reply:?}");
    assert_eq!(identities(owner).await, expected("a"));

    // Nothing of the refused request is stored.
    let record = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
            account_id,
            Collection::Principal,
            0,
            PrincipalField::ParticipantIdentities,
        ))
        .await
        .unwrap()
        .expect("the first read writes the identities record");
    let text = String::from_utf8_lossy(record.as_bytes());
    assert!(
        !text.contains("canary"),
        "the record holds the refused name"
    );

    // The default can be switched on its own.
    let reply = set(owner, json!({ "onSuccessSetIsDefault": "b" })).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(identities(owner).await, expected("b"));

    // A refused destroy keeps upstream's rule: the default does not move.
    let reply = set(
        owner,
        json!({ "destroy": ["b"], "onSuccessSetIsDefault": "a" }),
    )
    .await;
    assert_eq!(
        reply.response_at(0)["notDestroyed"]["b"],
        refused,
        "{reply:?}"
    );
    assert_eq!(identities(owner).await, expected("b"));
}
