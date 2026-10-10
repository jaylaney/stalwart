/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A WebSocket opened before its account became a key account. The socket
//! stays open; the key-account check runs on every method call, so the next
//! calendar call on it is refused as it would be over HTTP (spec 9).

use super::{
    STRONG,
    tracing::{ws_call, ws_close, ws_connect},
    user_permissions,
};
use crate::utils::{
    server::TestServer,
    za::{za_setup, za_setup_token},
};
use serde_json::{Value, json};

fn calendar_get(account_id: &str) -> String {
    json!({
        "@type": "Request",
        "id": "1",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"],
        "methodCalls": [["Calendar/get", { "accountId": account_id }, "c0"]]
    })
    .to_string()
}

/// The first method response's name and arguments.
fn first_response(reply: &str) -> (String, Value) {
    let reply: Value = serde_json::from_str(reply).unwrap();
    let call = &reply["methodResponses"][0];
    (
        call[0].as_str().unwrap_or_default().to_string(),
        call[1].clone(),
    )
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access WebSocket conversion test...");
    let admin = test.account("admin@example.com").clone();
    let mut account = admin
        .create_passwordless_user_account(
            "ws1@example.com",
            STRONG,
            "WebSocket User",
            &[],
            user_permissions(),
        )
        .await;
    let account_id = account.id_string().to_string();
    // A passwordless account has no login of its own before setup: open
    // the socket as the administrator acting as the account.
    let master = format!("ws1@example.com%{}", admin.name());
    let mut socket = ws_connect(&master, admin.secret()).await;

    let (name, args) = first_response(&ws_call(&mut socket, &calendar_get(&account_id)).await);
    assert_eq!(name, "Calendar/get", "{args}");

    // `setup-token` makes the account a key account at once.
    let token = za_setup_token(&admin, "ws1@example.com").await;
    assert!(
        test.server
            .account(account.id().document_id())
            .await
            .unwrap()
            .is_key_account()
    );

    // Same socket, same token: the calendar call is now refused.
    let (name, args) = first_response(&ws_call(&mut socket, &calendar_get(&account_id)).await);
    assert_eq!(
        (name.as_str(), args["type"].as_str()),
        ("error", Some("accountNotSupportedByMethod")),
        "{args}"
    );
    ws_close(socket).await;

    // Finish setup so the suite's key-account teardown destroys it.
    account.recovery_key = Some(za_setup("ws1@example.com", &token, STRONG).await);
    test.insert_account(account);
}
