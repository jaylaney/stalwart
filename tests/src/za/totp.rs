/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    za::{VaultReply, assert_nothing_cached, caldav_from, park_login, za_post, za_post_from},
};
use common::ipc::CacheInvalidation;
use http::api::vault::MAX_OTP_AUTH_URL;
use hyper::StatusCode;
use registry::{
    schema::{
        prelude::{ObjectInner, ObjectType},
        structs,
    },
    types::id::ObjectId,
};
use serde_json::{Value, json};

const STRONG2: &str = "another long passphrase with 2 numbers";

/// Client address of every deliberate failure in this module.
const FAIL_IP: &str = "10.0.9.3";

/// A fresh key account: its fail2ban login-name budget is not shared with
/// the other modules' deliberate failures.
const NAME: &str = "key8@example.com";

/// Status of a CalDAV Basic request that does not log in: a password
/// without the code once TOTP is enrolled.
const MFA_REFUSED: StatusCode = StatusCode::PAYMENT_REQUIRED;

async fn caldav(secret: &str, status: StatusCode) {
    caldav_from(FAIL_IP, NAME, secret, status).await;
}

/// Deliberate failure: sent from `FAIL_IP`.
async fn za_fail(path: &str, body: &Value) -> VaultReply {
    za_post_from(FAIL_IP, path, body).await
}

fn totp_url() -> String {
    totp_rs::Builder::new()
        .with_secret(store::rand::random::<[u8; 20]>())
        .with_account_name("key8")
        .with_issuer(Some("Stalwart"))
        .build()
        .unwrap()
        .to_url()
        .unwrap()
}

fn code(url: &str) -> String {
    totp_rs::Totp::from_url(url)
        .unwrap()
        .generate_current()
        .to_string()
}

/// The codes `check_current` could accept now, including the next step in
/// case it rolls over mid-test.
fn accepted_codes(url: &str) -> Vec<String> {
    let totp = totp_rs::Totp::from_url(url).unwrap();
    let now = store::write::now();
    [now.saturating_sub(30), now, now + 30, now + 60]
        .into_iter()
        .map(|time| totp.generate(time).to_string())
        .collect()
}

/// A six-digit code that matches no window `check_current` could accept.
fn wrong_code(url: &str) -> String {
    let accepted = accepted_codes(url);
    (0u32..)
        .map(|n| format!("{n:06}"))
        .find(|code| !accepted.contains(code))
        .unwrap()
}

/// The registry's own TOTP field and the account object's revision, read
/// directly (the JMAP view masks secrets).
async fn registry_state(test: &TestServer) -> (Option<String>, u64) {
    let object = test
        .server
        .registry()
        .get(ObjectId::new(ObjectType::Account, test.account(NAME).id()))
        .await
        .unwrap()
        .unwrap();
    let ObjectInner::Account(structs::Account::User(account)) = object.inner else {
        panic!("not a user account");
    };
    (
        account.password_credential().unwrap().otp_auth.clone(),
        object.revision,
    )
}

async fn totp_url_of(test: &TestServer, id: u32) -> Option<String> {
    test.server
        .za_vault_record(id)
        .await
        .unwrap()
        .unwrap()
        .record
        .totp_url
}

trait Detail {
    fn assert_detail(&self, expected: &str);
}

impl Detail for Value {
    /// The problem detail names the refusal.
    fn assert_detail(&self, expected: &str) {
        let detail = self["detail"].as_str().unwrap_or_default();
        assert!(detail.contains(expected), "{detail}");
    }
}

/// One endpoint with TOTP enrolled: no code 402, a wrong code 401, the
/// current code 200 (spec 4.1).
async fn requires_code(path: &str, body: Value, url: &str) -> Value {
    za_fail(path, &body).await.expect(402);
    let mut wrong = body.clone();
    wrong["totp"] = json!(wrong_code(url));
    za_fail(path, &wrong).await.expect(401);
    let mut right = body;
    right["totp"] = json!(code(url));
    za_post(path, &right).await.expect(200)
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access TOTP tests...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(NAME, STRONG, "Key Eight", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    test.insert_account(account);
    let url = totp_url();

    // Wrong password 401; non-key accounts 409; a malformed URL 400 without
    // quoting it (it carries the secret).
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": "nope nope nope", "otp_auth": url, "confirm": code(&url) }),
    )
    .await
    .expect(401);
    za_fail(
        "totp",
        &json!({ "username": "plain@example.com", "password": "plain secret with entropy 9", "otp_auth": url, "confirm": code(&url) }),
    )
    .await
    .expect(409);
    // `otp_auth` is required (absent 400) and capped before parsing, both
    // checked before verification: a wrong password still gets the 400.
    let long = format!(
        "{url}&image=https://x.example/{}",
        "a".repeat(MAX_OTP_AUTH_URL)
    );
    for password in [STRONG, "nope nope nope"] {
        za_fail("totp", &json!({ "username": NAME, "password": password }))
            .await
            .expect(400)
            .assert_detail("otp_auth is required");
        za_fail(
            "totp",
            &json!({ "username": NAME, "password": password, "otp_auth": long }),
        )
        .await
        .expect(400)
        .assert_detail("at most 1024 bytes");
    }
    for bad in [
        "",
        "garbage",
        "otpauth://totp/Leaky?secret=SHORTSECRET&issuer=Leaky",
        "otpauth://hotp/Leaky?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&counter=1",
    ] {
        let body = za_fail(
            "totp",
            &json!({ "username": NAME, "password": STRONG, "otp_auth": bad, "confirm": "000000" }),
        )
        .await
        .expect(400)
        .to_string();
        assert!(
            !body.contains("Leaky") && !body.contains("SHORTSECRET") && !body.contains("garbage"),
            "the error does not quote the URL: {body}"
        );
    }
    assert_eq!(totp_url_of(test, id).await, None);

    // Enrolment requires a current code of the new secret (`confirm`):
    // missing 400 before verification, wrong 401; neither writes anything.
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    for password in [STRONG, "nope nope nope"] {
        for confirm in [json!(null), json!("")] {
            za_fail(
                "totp",
                &json!({ "username": NAME, "password": password, "otp_auth": url, "confirm": confirm }),
            )
            .await
            .expect(400)
            .assert_detail("confirm is required");
        }
        za_fail(
            "totp",
            &json!({ "username": NAME, "password": password, "otp_auth": url }),
        )
        .await
        .expect(400)
        .assert_detail("confirm is required");
    }
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url, "confirm": wrong_code(&url) }),
    )
    .await
    .expect(401);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision, "no write");
    assert_eq!(after.record.totp_url, None);
    caldav(STRONG, StatusCode::MULTI_STATUS).await;

    // Paused verification across the enrolment (spec 11): a login verified
    // with the password alone before the enrolment succeeds, but nothing is
    // cached, and the password alone no longer logs in afterwards.
    caldav(STRONG, StatusCode::MULTI_STATUS).await;
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let (registry_totp, registry_revision) = registry_state(test).await;
    assert_eq!(registry_totp, None);
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    let parked = park_login(id, NAME, STRONG).await;
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url, "confirm": code(&url) }),
    )
    .await
    .expect(200);
    assert_eq!(parked.finish().await, 207, "verified before the enrolment");
    assert_nothing_cached(test, id);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(
        after.record.revision,
        before.record.revision + 1,
        "enrolment is one write and bumps the generation"
    );
    assert_eq!(after.record.totp_url.as_deref(), Some(url.as_str()));
    assert_eq!(
        registry_state(test).await,
        (None, registry_revision),
        "nothing is written to the registry"
    );
    caldav(STRONG, MFA_REFUSED).await;

    // Every password-verified endpoint requires the code once enrolled.
    requires_code(
        "recovery-key",
        json!({ "username": NAME, "password": STRONG }),
        &url,
    )
    .await;
    requires_code(
        "password",
        json!({ "username": NAME, "password": STRONG, "new_password": STRONG2 }),
        &url,
    )
    .await;
    za_post(
        "password",
        &json!({ "username": NAME, "password": STRONG2, "totp": code(&url), "new_password": STRONG }),
    )
    .await
    .expect(200);
    let reply = requires_code(
        "app-password",
        json!({ "username": NAME, "password": STRONG, "description": "Phone" }),
        &url,
    )
    .await;
    let phone = reply["app_password"].as_str().unwrap().to_string();
    let phone_id = reply["credential_id"].as_u64().unwrap();
    // App passwords log in without the code, like upstream's.
    caldav(&phone, StatusCode::MULTI_STATUS).await;
    requires_code(
        "app-password/revoke",
        json!({ "username": NAME, "password": STRONG, "credential_id": phone_id }),
        &url,
    )
    .await;
    caldav(&phone, StatusCode::UNAUTHORIZED).await;

    // Replacement requires the current code and a confirmation from the new
    // secret; a malformed URL is refused even with both.
    let url2 = totp_url();
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url2, "confirm": code(&url2) }),
    )
    .await
    .expect(402);
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": wrong_code(&url), "otp_auth": url2, "confirm": code(&url2) }),
    )
    .await
    .expect(401);
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url), "otp_auth": url2 }),
    )
    .await
    .expect(400)
    .assert_detail("confirm is required");
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url), "otp_auth": url2, "confirm": wrong_code(&url2) }),
    )
    .await
    .expect(401);
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url), "otp_auth": "garbage", "confirm": "000000" }),
    )
    .await
    .expect(400);
    assert_eq!(totp_url_of(test, id).await.as_deref(), Some(url.as_str()));
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url), "otp_auth": url2, "confirm": code(&url2) }),
    )
    .await
    .expect(200);
    assert_eq!(totp_url_of(test, id).await.as_deref(), Some(url2.as_str()));
    assert_eq!(registry_state(test).await, (None, registry_revision));
    // The old secret's code stops working (unless it happens to collide with
    // a window of the new one).
    let old = code(&url);
    if !accepted_codes(&url2).contains(&old) {
        za_fail(
            "recovery-key",
            &json!({ "username": NAME, "password": STRONG, "totp": old }),
        )
        .await
        .expect(401);
    }
    za_post(
        "recovery-key",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url2) }),
    )
    .await
    .expect(200);

    // An absent `otp_auth` is refused, even with the code, and removes nothing.
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    za_fail(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url2) }),
    )
    .await
    .expect(400)
    .assert_detail("otp_auth is required");
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision);
    assert_eq!(after.record.totp_url.as_deref(), Some(url2.as_str()));

    // Removal: `otp_auth` null, with the current code.
    let removal = json!({ "username": NAME, "password": STRONG, "otp_auth": null });
    za_fail("totp", &removal).await.expect(402);
    let mut with_code = removal.clone();
    with_code["totp"] = json!(code(&url2));
    za_post("totp", &with_code).await.expect(200);
    assert_eq!(totp_url_of(test, id).await, None);
    assert_eq!(registry_state(test).await, (None, registry_revision));
    caldav(STRONG, StatusCode::MULTI_STATUS).await;
    za_post(
        "recovery-key",
        &json!({ "username": NAME, "password": STRONG }),
    )
    .await
    .expect(200);

    // A cached password login does not survive an enrolment.
    caldav(STRONG, StatusCode::MULTI_STATUS).await;
    assert!(test.server.inner.cache.za_keys.contains_account(id));
    let url3 = totp_url();
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url3, "confirm": code(&url3) }),
    )
    .await
    .expect(200);
    assert!(!test.server.inner.cache.za_keys.contains_account(id));
    caldav(STRONG, MFA_REFUSED).await;
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url3), "otp_auth": null }),
    )
    .await
    .expect(200);
    assert_eq!(registry_state(test).await, (None, registry_revision));
    caldav(STRONG, StatusCode::MULTI_STATUS).await;

    // Removal when not enrolled changes nothing: no write.
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": null }),
    )
    .await
    .expect(200);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision);

    // Recovery removes TOTP (the recovery key is the stronger factor) and
    // says so; the new password then logs in without a code.
    let recovery = za_post(
        "recovery-key",
        &json!({ "username": NAME, "password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    let url4 = totp_url();
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url4, "confirm": code(&url4) }),
    )
    .await
    .expect(200);
    caldav(STRONG, MFA_REFUSED).await;
    let reply = za_post(
        "recover",
        &json!({ "username": NAME, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    assert_eq!(reply["totp_removed"], json!(true));
    assert_eq!(totp_url_of(test, id).await, None);
    caldav(STRONG2, StatusCode::MULTI_STATUS).await;
    // Without TOTP enrolled, recovery reports nothing removed.
    let reply = za_post(
        "recover",
        &json!({ "username": NAME, "recovery_key": reply["recovery_key"], "new_password": STRONG }),
    )
    .await
    .expect(200);
    assert_eq!(reply["totp_removed"], json!(false));
    caldav(STRONG, StatusCode::MULTI_STATUS).await;
    assert_eq!(registry_state(test).await, (None, registry_revision));
}
