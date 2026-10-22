/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{push::spawn_mock_push_server, server::TestServer, smtp::SmtpConnection};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use email::push::{EmailPush, Urgency};
use jmap_client::{mailbox::Role, push_subscription::Keys};
use jmap_proto::{
    method::query::Filter,
    object::{email::EmailFilter, push_subscription::EmailPushProperty},
    request::capability::{Capabilities, Capability},
    types::state::State,
};
use serde_json::{Value, json};
use services::state_manager::ece::ece_encrypt;
use services::state_manager::email_push::build_email_push_object;
use std::{str::FromStr, sync::atomic::Ordering, time::Duration};
use store::ahash::AHashSet;
use tokio::sync::mpsc;
use types::{id::Id, keyword::Keyword, type_state::DataType};
use utils::map::vec_map::VecMap;

/// The mock push server, lifted into `utils::push` for the zero-access
/// suite's push test.
pub use crate::utils::push::{PushServer, SessionManager};

pub async fn test(test: &TestServer) {
    println!("Running Push Subscription tests...");

    // ECE roundtrip test
    ece_roundtrip();

    // Create test account
    let account = test.account("robert@example.com");
    let client = account.jmap_client().await;

    // The server must expose a VAPID key and advertise it in the session capabilities
    let vapid_public_key = test
        .server
        .core
        .jmap
        .vapid
        .as_ref()
        .expect("A VAPID key must be configured")
        .public_key()
        .to_string();
    let advertised_key = test
        .server
        .core
        .jmap
        .capabilities
        .session
        .iter()
        .find_map(
            |(capability, capabilities)| match (capability, capabilities) {
                (Capability::WebPushVapid, Capabilities::WebPush(webpush)) => {
                    Some(webpush.application_server_key.as_str())
                }
                _ => None,
            },
        )
        .expect("The webpush-vapid capability must be advertised");
    assert_eq!(
        advertised_key, vapid_public_key,
        "The advertised applicationServerKey must match the signing key"
    );

    // Start mock push server
    let (mut event_rx, push) = spawn_mock_push_server(test).await;
    let push_server = push.server.clone();

    // Create subscription keys
    let pubkey = push.public_key.clone();
    let auth_secret = push.auth_secret.clone();
    let keys = Keys::new(&pubkey, &auth_secret);

    // Register push notification (no encryption)
    let push_id = client
        .push_subscription_create("123", "https://127.0.0.1:19000/push", None)
        .await
        .unwrap()
        .take_id();

    // Expect push verification
    let verification = expect_push(&mut event_rx).await.unwrap_verification();
    assert_eq!(verification.push_subscription_id, push_id);

    // Update verification code
    client
        .push_subscription_verify(&push_id, verification.verification_code)
        .await
        .unwrap();

    // Create a mailbox and expect a state change
    let mailbox_id = client
        .mailbox_create("PushSubscription Test", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    assert_state(&mut event_rx, account.id(), &[DataType::Mailbox]).await;

    // Receive states just for the requested types
    client
        .push_subscription_update_types(&push_id, [jmap_client::DataType::Email].into())
        .await
        .unwrap();
    client
        .mailbox_update_sort_order(&mailbox_id, 123)
        .await
        .unwrap();
    expect_nothing(&mut event_rx).await;

    // Destroy subscription
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
    let verification = expect_push(&mut event_rx).await.unwrap_verification();
    assert_eq!(verification.push_subscription_id, first_id);
    let deferred_id = client
        .push_subscription_create("deferred", "https://127.0.0.1:19000/push", None)
        .await
        .unwrap()
        .take_id();
    expect_nothing(&mut event_rx).await;
    let verification = expect_push_within(&mut event_rx, Duration::from_secs(8))
        .await
        .unwrap_verification();
    assert_eq!(verification.push_subscription_id, deferred_id);
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
            "https://127.0.0.1:19000/push?skip_checks=true", // skip_checks only works in cfg(test)
            keys.into(),
        )
        .await
        .unwrap()
        .take_id();

    // Expect push verification
    let verification = expect_push(&mut event_rx).await.unwrap_verification();
    assert_eq!(verification.push_subscription_id, push_id);

    // Update verification code
    client
        .push_subscription_verify(&push_id, verification.verification_code)
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
    assert_state(&mut event_rx, account.id(), &[DataType::Mailbox]).await;

    // Make a mailbox change and expect state change
    client
        .mailbox_rename(&mailbox_id, "My Mailbox")
        .await
        .unwrap();
    assert_state(&mut event_rx, account.id(), &[DataType::Mailbox]).await;
    //expect_nothing(&mut event_rx).await;

    // Multiple change updates should be grouped and pushed in intervals
    for num in 0..5 {
        client
            .mailbox_update_sort_order(&mailbox_id, num)
            .await
            .unwrap();
    }
    assert_state(&mut event_rx, account.id(), &[DataType::Mailbox]).await;
    expect_nothing(&mut event_rx).await;

    // Destroy mailbox
    client.push_subscription_destroy(&push_id).await.unwrap();
    client.mailbox_destroy(&mailbox_id, true).await.unwrap();
    expect_nothing(&mut event_rx).await;

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

    let verification = expect_push(&mut event_rx).await.unwrap_verification();
    assert_eq!(verification.push_subscription_id, ep_id.to_string());
    account
        .jmap_request(
            &["urn:ietf:params:jmap:core"],
            json!([[
                "PushSubscription/set",
                { "update": { (ep_id.to_string()): { "verificationCode": verification.verification_code } } },
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

    let (push_account, emails, state) = expect_push(&mut event_rx).await.unwrap_email_push();
    assert_eq!(push_account.to_string(), account_id_str);
    assert!(state.is_some(), "EmailPush must carry the Email state");
    assert_eq!(emails.len(), 1, "expected exactly one email in the push");
    let email = emails[0].to_string();
    assert!(
        email.contains("Urgent: action required"),
        "subject missing: {email}"
    );
    assert!(
        email.contains("sender@example.com"),
        "from missing: {email}"
    );

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
    expect_nothing(&mut event_rx).await;

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

    // Test the EmailPush object builder (filters and size limits) directly
    test_email_push_object(test).await;

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

async fn test_email_push_object(test: &TestServer) {
    let account = test.account("robert@example.com");
    let client = account.jmap_client().await;
    let account_id = account.id().document_id();

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

    let properties = vec![
        EmailPushProperty::Id,
        EmailPushProperty::From,
        EmailPushProperty::Subject,
    ];
    let config = |filter: Vec<Filter<EmailFilter>>| EmailPush {
        account_id,
        properties: properties.clone(),
        filter,
        urgency: Urgency::Normal,
    };

    // Matching subject (case-insensitive substring), full object is produced
    let value = build_email_push_object(
        &test.server,
        account_id,
        document_id,
        &config(vec![Filter::Property(EmailFilter::Subject(
            "URGENT".into(),
        ))]),
        4096,
    )
    .await
    .unwrap()
    .expect("matching subject filter must produce an object");
    let json = serde_json::to_string(&value).unwrap();
    assert!(
        json.contains("Urgent meeting tonight"),
        "subject missing: {json}"
    );
    assert!(json.contains("alice@example.com"), "from missing: {json}");

    // A collection of filters that should each either match (Some) or not (None)
    for (expected_match, filter) in [
        (
            false,
            vec![Filter::Property(EmailFilter::Subject(
                "does-not-appear".into(),
            ))],
        ),
        (
            true,
            vec![Filter::Property(EmailFilter::InMailbox(Id::from(
                mailbox_doc_id,
            )))],
        ),
        (
            false,
            vec![Filter::Property(EmailFilter::InMailbox(Id::from(
                mailbox_doc_id + 1,
            )))],
        ),
        (
            true,
            vec![Filter::Property(EmailFilter::HasKeyword(Keyword::parse(
                "$notify",
            )))],
        ),
        (
            false,
            vec![Filter::Property(EmailFilter::NotKeyword(Keyword::parse(
                "$notify",
            )))],
        ),
        (true, vec![]),
        (
            true,
            vec![
                Filter::Or,
                Filter::Property(EmailFilter::Subject("urgent".into())),
                Filter::Property(EmailFilter::From("nobody@example.com".into())),
                Filter::Close,
            ],
        ),
        (
            false,
            vec![
                Filter::And,
                Filter::Property(EmailFilter::Subject("urgent".into())),
                Filter::Property(EmailFilter::From("nobody@example.com".into())),
                Filter::Close,
            ],
        ),
    ] {
        let result = build_email_push_object(
            &test.server,
            account_id,
            document_id,
            &config(filter.clone()),
            4096,
        )
        .await
        .unwrap();
        assert_eq!(
            result.is_some(),
            expected_match,
            "filter produced the wrong match result: {filter:?}"
        );
    }

    // Size limit: a generous budget keeps every property, a tiny budget drops some (in order)
    let (full, _) =
        build_email_push_object(&test.server, account_id, document_id, &config(vec![]), 4096)
            .await
            .unwrap()
            .expect("object");
    assert_eq!(
        full.as_object().unwrap().as_vec().len(),
        3,
        "all requested properties must fit under a generous budget"
    );
    let (truncated, _) =
        build_email_push_object(&test.server, account_id, document_id, &config(vec![]), 50)
            .await
            .unwrap()
            .expect("object");
    assert!(
        truncated.as_object().unwrap().as_vec().len() < 3,
        "a tiny size budget must drop properties: {}",
        serde_json::to_string(&truncated).unwrap()
    );
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum PushMessage {
    PushObject(PushObject),
    Verification(PushVerification),
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug)]
#[serde(tag = "@type")]
enum PushObject {
    StateChange {
        changed: VecMap<Id, VecMap<DataType, State>>,
    },
    EmailPush {
        #[serde(rename = "accountId")]
        account_id: Id,
        #[serde(default)]
        emails: Vec<serde_json::Value>,
        #[serde(default)]
        state: Option<State>,
    },
    CalendarAlert {
        #[serde(rename = "accountId")]
        account_id: Id,
        #[serde(rename = "calendarEventId")]
        calendar_event_id: Id,
        uid: String,
        #[serde(rename = "recurrenceId")]
        recurrence_id: Option<String>,
        #[serde(rename = "alertId")]
        alert_id: String,
    },
}

impl PushMessage {
    pub fn unwrap_state_change(self) -> VecMap<Id, VecMap<DataType, State>> {
        match self {
            PushMessage::PushObject(PushObject::StateChange { changed }) => changed,
            _ => panic!("Expected PushObject"),
        }
    }

    pub fn unwrap_verification(self) -> PushVerification {
        match self {
            PushMessage::Verification(verification) => verification,
            _ => panic!("Expected Verification"),
        }
    }

    pub fn unwrap_email_push(self) -> (Id, Vec<serde_json::Value>, Option<State>) {
        match self {
            PushMessage::PushObject(PushObject::EmailPush {
                account_id,
                emails,
                state,
            }) => (account_id, emails, state),
            other => panic!("Expected EmailPush, got: {other:?}"),
        }
    }
}

#[derive(serde::Deserialize, Debug)]
enum PushVerificationType {
    PushVerification,
}

#[derive(serde::Deserialize, Debug)]
struct PushVerification {
    #[serde(rename = "@type")]
    _type: PushVerificationType,
    #[serde(rename = "pushSubscriptionId")]
    pub push_subscription_id: String,
    #[serde(rename = "verificationCode")]
    pub verification_code: String,
}

async fn expect_push(event_rx: &mut mpsc::Receiver<Value>) -> PushMessage {
    expect_push_within(event_rx, Duration::from_millis(1500)).await
}

async fn expect_push_within(event_rx: &mut mpsc::Receiver<Value>, wait: Duration) -> PushMessage {
    match tokio::time::timeout(wait, event_rx.recv()).await {
        Ok(Some(push)) => {
            //println!("Push received: {:?}", push);
            // Parsed from text, as the mock server parsed the body: the
            // untagged `PushMessage` does not deserialize from a `Value`.
            serde_json::from_str(&push.to_string()).unwrap()
        }
        result => {
            panic!("Timeout waiting for push: {:?}", result);
        }
    }
}

async fn expect_nothing(event_rx: &mut mpsc::Receiver<Value>) {
    match tokio::time::timeout(Duration::from_millis(1000), event_rx.recv()).await {
        Err(_) => {}
        message => {
            panic!("Received a message when expecting nothing: {:?}", message);
        }
    }
}

async fn assert_state(event_rx: &mut mpsc::Receiver<Value>, id: Id, state: &[DataType]) {
    assert_eq!(
        expect_push(event_rx)
            .await
            .unwrap_state_change()
            .get(&id)
            .unwrap()
            .iter()
            .map(|x| x.0)
            .collect::<AHashSet<&DataType>>(),
        state.iter().collect::<AHashSet<&DataType>>()
    );
}

fn ece_roundtrip() {
    for len in [1, 2, 5, 16, 256, 1024, 2048, 4096, 1024 * 1024] {
        let (keypair, auth_secret) = ece::generate_keypair_and_auth_secret().unwrap();

        let bytes: Vec<u8> = (0..len).map(|_| store::rand::random::<u8>()).collect();

        let encrypted_bytes =
            ece_encrypt(&keypair.pub_as_raw().unwrap(), &auth_secret, &bytes).unwrap();

        let decrypted_bytes = ece::decrypt(
            &keypair.raw_components().unwrap(),
            &auth_secret,
            &encrypted_bytes,
        )
        .unwrap();

        assert_eq!(bytes, decrypted_bytes, "len: {}", len);
    }
}
