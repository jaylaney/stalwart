/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A WebSocket opened before account conversion checks the account on each
//! call. A socket opened after setup is keyless and receives state-only push.

use super::{
    STRONG,
    tracing::{WsStream, ws_call, ws_close, ws_connect, ws_recv, ws_send},
    user_permissions,
};
use crate::utils::{
    account::Account,
    server::TestServer,
    za::{za_setup, za_setup_token},
};
use common::za::ZA_MAIL_KEYLESS;
use serde_json::{Value, json};
use std::time::Duration;

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
    keyless_socket_after_setup(&account).await;
    test.insert_account(account);
}

async fn keyless_socket_after_setup(account: &Account) {
    println!("Running keyless WebSocket mail and state push test...");
    let account_id = account.id_string();
    let mut socket = ws_connect(account.name(), STRONG).await;

    let request = json!({
        "@type": "Request",
        "id": "keyless-mail",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/query", { "accountId": account_id }, "email"],
            ["Mailbox/query", { "accountId": account_id }, "mailbox"],
        ],
    });
    let reply = ws_call(&mut socket, &request.to_string()).await;
    let (name, args) = first_response(&reply);
    assert_eq!(name, "error", "{reply}");
    assert_eq!(
        args,
        json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS })
    );
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["methodResponses"][1][0], "Mailbox/query", "{reply}");
    assert!(reply["methodResponses"][1][1]["ids"].is_array(), "{reply}");

    enable_state_push(&mut socket, "Mailbox").await;
    let reply = account
        .jmap_method_call(
            "Mailbox/set",
            json!({
                "accountId": account_id,
                "create": { "push": { "name": "keyed mailbox push" } },
            }),
        )
        .await;
    assert_eq!(reply.name_at(0), "Mailbox/set", "{reply:?}");
    let id = reply.0["methodResponses"][0][1]["created"]["push"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Mailbox/set did not create a mailbox: {reply:?}"));
    assert_state_push(&mut socket, account_id, "Mailbox", reply.new_state(), &[id]).await;
    ws_close(socket).await;

    let reply = account
        .jmap_method_call(
            "Mailbox/set",
            json!({ "accountId": account_id, "destroy": [id] }),
        )
        .await;
    assert_eq!(
        reply.0["methodResponses"][0][1]["destroyed"],
        json!([id]),
        "{reply:?}"
    );
}

/// PushEnable has no response. A following ping establishes that the server
/// processed it before the separate HTTP client makes its mutation.
pub(super) async fn enable_state_push(socket: &mut WsStream, data_type: &str) {
    let enable = json!({ "@type": "WebSocketPushEnable", "dataTypes": [data_type] });
    ws_send(socket, 0x1, enable.to_string().as_bytes()).await;
    let barrier = b"push-enabled";
    ws_send(socket, 0x9, barrier).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_recv(socket)
                .await
                .expect("socket closed before push barrier")
            {
                (0xa, payload) if payload == barrier => break,
                (0x9, payload) => ws_send(socket, 0xa, &payload).await,
                (0x1, payload) => {
                    let stale: Value = serde_json::from_slice(&payload).unwrap();
                    assert_eq!(stale["@type"], "StateChange", "{stale}");
                }
                (opcode, _) => panic!("unexpected WebSocket frame {opcode:#x}"),
            }
        }
    })
    .await
    .expect("WebSocket push barrier timed out");
}

/// Accept the mutation's own state, ignoring earlier queued state changes.
pub(super) async fn assert_state_push(
    socket: &mut WsStream,
    account_id: &str,
    data_type: &str,
    state: &str,
    forbidden_ids: &[&str],
) {
    let expected =
        json!({ "@type": "StateChange", "changed": { account_id: { data_type: state } } });
    let mut last = Value::Null;
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_recv(socket)
                .await
                .expect("socket closed before state push")
            {
                (0x9, payload) => ws_send(socket, 0xa, &payload).await,
                (0x1, payload) => {
                    last = serde_json::from_slice(&payload).unwrap();
                    assert_eq!(last["@type"], "StateChange", "{last}");
                    if last["changed"][account_id][data_type] == state {
                        assert_eq!(last, expected);
                        // The exact shape above prohibits ID fields. Compare whole
                        // tokens so an ID substring inside the opaque state is fine.
                        for id in forbidden_ids {
                            assert!(![account_id, data_type, state].contains(id), "{last}");
                        }
                        break;
                    }
                }
                (opcode, _) => panic!("unexpected WebSocket frame {opcode:#x}"),
            }
        }
    })
    .await;
    assert!(
        found.is_ok(),
        "missing state push {expected}, last received {last}"
    );
}
