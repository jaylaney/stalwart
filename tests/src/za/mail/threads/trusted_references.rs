/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{email_create, email_id, import_args, mailbox_ids, mapped, method, upload};
use crate::utils::{account::Account, jmap::JmapResponse, server::TestServer};
use serde_json::json;
use types::id::Id;

pub(super) async fn test(test: &TestServer, key: &Account, root: &str, root_id: u32) {
    println!("Running same-request trusted Email reference tests...");
    let identities = key
        .jmap_method_call(
            "Identity/get",
            json!({
                "accountId": key.id_string(), "properties": ["id", "email"]
            }),
        )
        .await;
    let identity = method(&identities, 0, "Identity/get")["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["email"] == key.name())
        .unwrap()["id"]
        .clone();
    for importing in [true, false] {
        let first = if importing {
            let blob = upload(
                key,
                &super::reply_to(root, "Task 11 mapped id", "trusted import").replacen(
                    "sender@example.org",
                    key.name(),
                    1,
                ),
            )
            .await;
            json!([
                "Email/import",
                import_args(key, "earlier", &blob, mailbox_ids()),
                "earlier"
            ])
        } else {
            json!(["Email/set", {
                "accountId": key.id_string(),
                "create": {"earlier": email_create(key, "Re: Task 11 mapped id", Some(root))}
            }, "earlier"])
        };
        // The last adding call fails, but publishes the first call's insertion.
        // Its response therefore has a different mapping from the earlier ID.
        let reply = key
            .jmap_method_calls(json!([
                first,
                ["Email/set", {"accountId": key.id_string(), "create": {
                    "invalid": {"mailboxIds": {(Id::from(u32::MAX - 1).to_string()): true}}
                }}, "publish"],
                ["Email/get", {"accountId": key.id_string(), "ids": ["#earlier"],
                    "properties": ["id", "threadId"]}, "get"],
                ["Email/get", {"accountId": key.id_string(), "#ids": {
                    "resultOf": "earlier", "name": if importing {"Email/import"} else {"Email/set"},
                    "path": "/created/earlier/id"
                }, "properties": ["id"]}, "strict-result"],
                ["EmailSubmission/set", {"accountId": key.id_string(), "create": {
                    "s": {"emailId": "#earlier", "identityId": identity,
                        "envelope": {"mailFrom": {"email": key.name(), "parameters": {"HOLDFOR": "300"}},
                            "rcptTo": [{"email": key.name()}]}}
                }, "onSuccessUpdateEmail": {"#s": {"keywords/$flagged": true}}}, "submit"],
                ["Email/set", {"accountId": key.id_string(), "#destroy": {
                    "resultOf": "get", "name": "Email/get", "path": "/list/*/id"
                }}, "remove"],
                ["Email/get", {"accountId": key.id_string(), "ids": ["#earlier"],
                    "properties": ["id"]}, "deleted"],
                ["Email/get", {"accountId": key.id_string(), "ids": ["#invalid"],
                    "properties": ["id"]}, "failed-reference"]
            ]))
            .await;
        let earlier = method(
            &reply,
            0,
            if importing {
                "Email/import"
            } else {
                "Email/set"
            },
        );
        let early_id = email_id(&earlier["created"]["earlier"]["id"]);
        assert_eq!(early_id.prefix_id(), early_id.document_id(), "{earlier}");
        assert_ne!(early_id.prefix_id(), root_id);
        let published = method(&reply, 1, "Email/set");
        assert!(
            published["notCreated"].get("invalid").is_some(),
            "{published}"
        );
        let get = method(&reply, 2, "Email/get");
        assert_eq!(get["list"].as_array().unwrap().len(), 1, "{get}");
        assert_eq!(
            get["list"][0]["id"],
            mapped(root_id, early_id.document_id())
        );
        assert_eq!(get["state"], published["newState"]);
        let strict = method(&reply, 3, "Email/get");
        assert_eq!(strict["list"], json!([]));
        assert_eq!(strict["notFound"], json!([early_id.to_string()]));
        assert_eq!(
            method(&reply, 6, "Email/set")["destroyed"],
            json!([mapped(root_id, early_id.document_id())])
        );
        let deleted = method(&reply, 7, "Email/get");
        assert_eq!(deleted["list"], json!([]));
        assert_eq!(deleted["notFound"], json!([early_id.to_string()]));
        assert_eq!(method(&reply, 8, "error")["type"], "invalidResultReference");
        let submission = method(&reply, 4, "EmailSubmission/set")["created"]["s"]["id"]
            .as_str()
            .unwrap();
        assert!(
            method(&reply, 5, "Email/set")["updated"]
                .get(mapped(root_id, early_id.document_id()))
                .is_some()
        );
        key.jmap_method_call(
            "EmailSubmission/set",
            json!({"accountId": key.id_string(),
            "update": {(submission): {"undoStatus": "canceled"}}}),
        )
        .await;
        key.jmap_method_call(
            "EmailSubmission/set",
            json!({"accountId": key.id_string(),
            "destroy": [submission]}),
        )
        .await;
        case_returned_id_lifetime(key, root, root_id, importing).await;
    }
    case_generated_followup(test, key, false).await;
    case_generated_followup(test, key, true).await;
}

async fn case_returned_id_lifetime(key: &Account, root: &str, root_id: u32, importing: bool) {
    let creation_method = if importing {
        "Email/import"
    } else {
        "Email/set"
    };
    let arguments = if importing {
        let blob = upload(
            key,
            &super::reply_to(root, "Task 11 mapped id", "returned import ID lifetime"),
        )
        .await;
        import_args(key, "earlier", &blob, mailbox_ids())
    } else {
        json!({
            "accountId": key.id_string(),
            "create": {"earlier": email_create(key, "Re: Task 11 mapped id", Some(root))}
        })
    };
    // Opt in to top-level createdIds, and keep the created document alive
    // after the final adding method publishes its thread assignment.
    let raw = key
        .jmap_raw_post(
            json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                "createdIds": {},
                "methodCalls": [
                    [creation_method, arguments, "earlier"],
                    ["Email/set", {"accountId": key.id_string(), "create": {
                        "invalid": {"mailboxIds": {(Id::from(u32::MAX - 1).to_string()): true}}
                    }}, "publish"],
                    ["Email/get", {"accountId": key.id_string(), "ids": ["#earlier"],
                        "properties": ["id", "threadId"]}, "current"]
                ]
            })
            .to_string(),
            "application/json",
        )
        .await;
    assert_eq!(raw.status, 200, "{}", raw.text());
    let reply = JmapResponse(raw.json().expect("returned-ID lifetime response is JSON"));
    let earlier = method(&reply, 0, creation_method);
    let early_id = email_id(&earlier["created"]["earlier"]["id"]);
    assert_eq!(early_id.prefix_id(), early_id.document_id(), "{earlier}");
    assert_ne!(early_id.prefix_id(), root_id);
    assert_eq!(
        earlier["created"]["earlier"]["threadId"],
        Id::from(early_id.document_id()).to_string()
    );
    assert_eq!(
        reply.0["createdIds"],
        json!({"earlier": early_id.to_string()})
    );
    let published = method(&reply, 1, "Email/set");
    assert!(
        published["notCreated"].get("invalid").is_some(),
        "{published}"
    );
    assert_ne!(earlier["newState"], published["newState"]);
    let current_id = mapped(root_id, early_id.document_id());
    let current = method(&reply, 2, "Email/get");
    assert_eq!(
        current["list"],
        json!([{"id": &current_id, "threadId": Id::from(root_id).to_string()}])
    );
    assert_eq!(current["state"], published["newState"]);

    // A new request cannot inherit the earlier request's trusted binding,
    // including when it sends back the server's unchanged createdIds map.
    let raw = key
        .jmap_raw_post(
            json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                "createdIds": reply.0["createdIds"],
                "methodCalls": [
                    ["Email/get", {"accountId": key.id_string(), "ids": [early_id.to_string()],
                        "properties": ["id"]}, "old-literal"],
                    ["Email/get", {"accountId": key.id_string(), "ids": ["#earlier"],
                        "properties": ["id"]}, "old-client-reference"],
                    ["Email/get", {"accountId": key.id_string(), "ids": [&current_id],
                        "properties": ["id"]}, "current-literal"]
                ]
            })
            .to_string(),
            "application/json",
        )
        .await;
    assert_eq!(raw.status, 200, "{}", raw.text());
    let fresh = JmapResponse(
        raw.json()
            .expect("fresh returned-ID lifetime response is JSON"),
    );
    for ordinal in [0, 1] {
        let stale = method(&fresh, ordinal, "Email/get");
        assert_eq!(stale["list"], json!([]), "{stale}");
        assert_eq!(stale["notFound"], json!([early_id.to_string()]), "{stale}");
    }
    assert_eq!(
        method(&fresh, 2, "Email/get")["list"],
        json!([{"id": &current_id}])
    );
    let removed = key
        .jmap_method_call(
            "Email/set",
            json!({"accountId": key.id_string(), "destroy": [&current_id]}),
        )
        .await;
    assert_eq!(
        method(&removed, 0, "Email/set")["destroyed"],
        json!([current_id])
    );
}

// This pause is after the submission handler returned and dropped its view,
// immediately before its generated Email/set acquires a new one.
async fn case_generated_followup(test: &TestServer, key: &Account, destroy: bool) {
    use super::{deliver_doc, exact, get, message_with, reply_to, singleton, with_references};
    use crate::utils::za::{HookParked, run_search_pass};
    use jmap::api::request::za_reference_test::set_followup_pause;
    use std::time::Duration;

    let subject = format!("PR84 followup destroy={destroy}");
    let left = message_with(&subject, "left root");
    let left_id = deliver_doc(test, key, &left).await;
    for body in ["left one", "left two"] {
        deliver_doc(test, key, &reply_to(&left, &subject, body)).await;
    }
    let right = message_with(&subject, "right root").replacen("sender@example.org", key.name(), 1);
    let right_id = deliver_doc(test, key, &right).await;
    let unrelated = deliver_doc(test, key, &message_with("PR84 unrelated", "control")).await;
    let before = get(key, json!([singleton(right_id)])).await;
    assert_eq!(
        method(&before, 0, "Email/get")["list"][0]["id"],
        singleton(right_id)
    );
    let before_state = exact(&method(&before, 0, "Email/get")["state"]);
    let identities = key
        .jmap_method_call(
            "Identity/get",
            json!({
                "accountId": key.id_string(), "properties": ["id", "email"]
            }),
        )
        .await;
    let identity = method(&identities, 0, "Identity/get")["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["email"] == key.name())
        .unwrap()["id"]
        .clone();
    let reader = key.clone();
    let call = HookParked::start(key.id().document_id(), set_followup_pause, async move {
        let mut args = json!({
            "accountId": reader.id_string(),
            "create": {"s": {"identityId": identity, "emailId": singleton(right_id),
                "envelope": {"mailFrom": {"email": reader.name(), "parameters": {"HOLDFOR": "300"}},
                    "rcptTo": [{"email": reader.name()}]}}},
            "onSuccessUpdateEmail": {"#s": {"keywords/$flagged": true}}
        });
        if destroy {
            args["onSuccessDestroyEmail"] = json!(["#s"]);
        }
        parked_submission(&reader, args).await
    })
    .await;
    let mapped_id = tokio::time::timeout(Duration::from_secs(10), async {
        let entry = test
            .server
            .inner
            .cache
            .za_search
            .get(key.id().document_id())
            .unwrap();
        assert!(
            entry.index.try_write().is_ok(),
            "submission held its view across the method boundary"
        );
        let bridge = with_references(message_with(&subject, "bridge"), &[&left, &right]);
        let account_id = key.id().document_id();
        let before_bridge = super::message_ids(test, account_id).await;
        // The held submission deliberately keeps the outbound queue nonempty.
        // Wait only for this real LMTP insertion, not unrelated queued work.
        crate::utils::smtp::SmtpConnection::connect()
            .await
            .ingest(super::SENDER, &[key.name()], &bridge)
            .await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while super::message_ids(test, account_id).await == before_bridge {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the bridge LMTP insertion did not become visible");
        super::new_message(test, account_id, &before_bridge).await;
        let outcome = tokio::time::timeout(
            Duration::from_secs(4),
            run_search_pass(test, key.id().document_id(), key.secret()),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                outcome,
                email::message::za::search::pass::PassOutcome::Advanced {
                    published: true,
                    ..
                }
            ),
            "{outcome:?}"
        );
        let mapped_id = mapped(left_id, right_id);
        let mapped_get = get(key, json!([&mapped_id])).await;
        assert_eq!(
            method(&mapped_get, 0, "Email/get")["list"][0]["id"],
            mapped_id
        );
        assert!(exact(&method(&mapped_get, 0, "Email/get")["state"]) > before_state);
        mapped_id
    })
    .await
    .expect("the aggregate parked follow-up phase exceeded ten seconds");
    let reply = tokio::time::timeout(Duration::from_secs(10), call.finish())
        .await
        .expect("the released follow-up did not finish");
    let submission = method(&reply, 0, "EmailSubmission/set")["created"]["s"]["id"].clone();
    if !destroy {
        case_consumer_occurrences(
            test,
            key,
            Id::from_parts(right_id, right_id),
            mapped_id.clone(),
        )
        .await;
    }
    let followup = method(&reply, 1, "Email/set");
    if destroy {
        assert_eq!(followup["destroyed"], json!([&mapped_id]));
        assert_eq!(followup["notUpdated"][&mapped_id]["type"], "willDestroy");
    } else {
        assert!(followup["updated"].get(&mapped_id).is_some(), "{followup}");
    }
    let control = get(key, json!([singleton(unrelated)])).await;
    assert!(
        method(&control, 0, "Email/get")["list"][0]["keywords"]
            .get("$flagged")
            .is_none()
    );
    // Literal onSuccess keys name successful submission updates, not Emails.
    let cancelled = key.jmap_method_call("EmailSubmission/set", json!({
        "accountId": key.id_string(), "update": {(submission.as_str().unwrap()): {"undoStatus": "canceled"}},
        "onSuccessUpdateEmail": {(submission.as_str().unwrap()): {"keywords/$seen": true}}
    })).await;
    assert!(
        method(&cancelled, 0, "EmailSubmission/set")["updated"]
            .get(submission.as_str().unwrap())
            .is_some()
    );
    if !destroy {
        assert!(
            method(&cancelled, 1, "Email/set")["updated"]
                .get(&mapped_id)
                .is_some()
        );
    } else {
        assert!(method(&cancelled, 1, "Email/set")["notUpdated"].is_object());
    }
    key.jmap_method_call(
        "EmailSubmission/set",
        json!({
            "accountId": key.id_string(), "destroy": [submission]
        }),
    )
    .await;
}

/// Only this deliberately parked request gets a larger finite client deadline.
/// The server-side parked work remains bounded separately in the fixture.
async fn parked_submission(
    account: &Account,
    arguments: serde_json::Value,
) -> crate::utils::jmap::JmapResponse {
    let response = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
        .post(account.api_url())
        .header(reqwest::header::AUTHORIZATION, account.basic_auth())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(
            json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail",
                    "urn:ietf:params:jmap:submission"],
                "methodCalls": [["EmailSubmission/set", arguments, "0"]]
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    crate::utils::jmap::JmapResponse(serde_json::from_slice(&response).unwrap())
}

/// Exercise the resolver-to-consumer boundary against a real changed mapping.
/// Registry setup models the already-tested completed Email creation response;
/// both consumers and their keyed validated views are the production handlers.
async fn case_consumer_occurrences(test: &TestServer, key: &Account, old: Id, current: String) {
    use crate::utils::za::za_unlock;
    use common::{
        auth::AccessToken,
        network::memory::{MemoryBudget, MemoryClass},
    };
    use jmap::{email::get::EmailGet, submission::set::EmailSubmissionSet};
    use jmap_proto::{
        method::{
            get::GetRequest,
            set::{SetRequest, SetResponse},
        },
        object::{email::Email, email_submission::EmailSubmission},
        request::{GetRequestMethod, RequestMethod, SetRequestMethod},
        response::Response,
    };
    use store::ahash::AHashMap;
    let account_id = key.id().document_id();
    let keys = za_unlock(test, account_id, key.secret()).await;
    let token = AccessToken::from_permissions(account_id, crate::za::user_permissions())
        .with_session_keys(keys);
    let identities = key
        .jmap_method_call(
            "Identity/get",
            json!({
                "accountId": key.id_string(), "properties": ["id", "email"]
            }),
        )
        .await;
    let identity = method(&identities, 0, "Identity/get")["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["email"] == key.name())
        .unwrap()["id"]
        .clone();
    let instance = test.new_mta_session().instance;
    for source_account in [account_id, account_id + 1] {
        let mut registry = Response::new(
            0,
            AHashMap::from_iter([("client".to_string(), old.into())]),
            0,
        );
        let mut created = SetResponse::<Email>::from_request(&Default::default(), 0).unwrap();
        created.account_id = Some(Id::from(source_account));
        created.created("server".into(), old);
        created.update_created_ids(&mut registry);
        registry.trust_email_creations(&created);
        let mut request = RequestMethod::Get(GetRequestMethod::Email(Box::new(
            serde_json::from_str::<GetRequest<Email>>(
                &json!({
                    "accountId": key.id_string(), "properties": ["id"],
                    "ids": ["invalid-id!", "#server", old.to_string(), "#client", &current]
                })
                .to_string(),
            )
            .unwrap(),
        )));
        registry.resolve_references(&mut request).unwrap();
        let RequestMethod::Get(GetRequestMethod::Email(request)) = request else {
            panic!()
        };
        let unavailable_request = request.clone();
        let mut reservation = MemoryBudget::default().reservation(MemoryClass::Read, 1 << 28);
        let response = test
            .server
            .email_get(*request, &token, Default::default(), &mut reservation)
            .await
            .unwrap();
        let response = serde_json::to_value(response).unwrap();
        let expected = if source_account == account_id { 2 } else { 1 };
        assert_eq!(
            response["list"].as_array().unwrap().len(),
            expected,
            "{response}"
        );
        assert!(
            response["list"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["id"] == current)
        );
        assert_eq!(
            response["notFound"].as_array().unwrap().len(),
            5 - expected,
            "{response}"
        );
        if source_account == account_id {
            common::za::test_hooks::set_search_manifest_fail_once(Some(account_id));
            let unavailable = test
                .server
                .email_get(
                    *unavailable_request,
                    &token,
                    Default::default(),
                    &mut reservation,
                )
                .await;
            common::za::test_hooks::set_search_manifest_fail_once(None);
            assert!(
                unavailable.is_err(),
                "trusted references must not bypass an unavailable map"
            );
        }
        let object = |id: &str| {
            json!({"emailId": id, "identityId": identity,
            "envelope": {"mailFrom": {"email": key.name(), "parameters": {"HOLDFOR": "300"}},
                "rcptTo": [{"email": key.name()}]}})
        };
        let arguments = json!({"accountId": key.id_string(), "create": {
            "trusted": object("#server"), "literal": object(&old.to_string()),
            "incoming": object("#client"), "current": object(&current)
        }})
        .to_string();
        let mut request = RequestMethod::Set(SetRequestMethod::EmailSubmission(Box::new(
            serde_json::from_str::<SetRequest<EmailSubmission>>(&arguments).unwrap(),
        )));
        registry.resolve_references(&mut request).unwrap();
        let RequestMethod::Set(SetRequestMethod::EmailSubmission(request)) = request else {
            panic!()
        };
        let mut next_call = None;
        let response = test
            .server
            .email_submission_set(*request, &token, &instance, &mut next_call)
            .await
            .unwrap();
        assert!(next_call.is_none());
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(
            response["created"].as_object().unwrap().len(),
            expected,
            "{response}"
        );
        for name in ["literal", "incoming"] {
            assert_eq!(
                response["notCreated"][name]["type"], "notFound",
                "{response}"
            );
        }
        if source_account != account_id {
            assert_eq!(
                response["notCreated"]["trusted"]["type"], "notFound",
                "{response}"
            );
        }
        let ids = response["created"]
            .as_object()
            .unwrap()
            .values()
            .map(|object| object["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let updates = ids
            .iter()
            .map(|id| (id.clone(), json!({"undoStatus": "canceled"})))
            .collect::<serde_json::Map<_, _>>();
        let canceled = key
            .jmap_method_call(
                "EmailSubmission/set",
                json!({
                    "accountId": key.id_string(), "update": updates
                }),
            )
            .await;
        assert_eq!(
            method(&canceled, 0, "EmailSubmission/set")["updated"]
                .as_object()
                .unwrap()
                .len(),
            expected
        );
        key.jmap_method_call(
            "EmailSubmission/set",
            json!({
                "accountId": key.id_string(), "destroy": ids
            }),
        )
        .await;
    }
}
