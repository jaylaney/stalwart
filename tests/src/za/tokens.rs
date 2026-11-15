/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    drain::DrainOn,
    pinning::{forget, lookup, pin},
    user_permissions,
};
use crate::utils::{
    server::TestServer,
    za::{
        Parked, VaultReply, code, raw_token_record, set_raw_token_record, substitute_public_key,
        totp_url, trigger_drain, vault_revision, wrap_mark, writer_key, za_post,
    },
};
use common::auth::vault::ZaMark;
use http::auth::authenticate::za_test;
use registry::{
    schema::{
        prelude::{Object, ObjectInner, ObjectType},
        structs,
    },
    types::id::ObjectId,
};
use serde_json::json;
use store::registry::write::{RegistryWrite, RegistryWriteResult};

const OWNER: &str = "token-owner@example.com";
const MFA: &str = "token-mfa@example.com";
const STRONG2: &str = "another long passphrase with 2 numbers";
const PLANTED: &[u8] = &[1, 0xde, 0xad];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access token tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description) in [(OWNER, "Token Owner"), (MFA, "Token MFA")] {
        let account = admin
            .create_key_user_account(name, STRONG, description, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    let owner = test.account(OWNER).id().document_id();
    let mfa = test.account(MFA).id().document_id();

    pin_rewrap_keeps_session(test, owner).await;
    credential_changes_end_sessions(test, owner, mfa).await;
    other_vault_writes_keep_sessions(test, owner).await;
    test.wait_for_tasks().await;
}

async fn pin_rewrap_keeps_session(test: &TestServer, owner: u32) {
    const FIRST: &str = "key1@example.com";
    assert_eq!(wrap_mark(test, owner, STRONG).await, ZaMark::Legacy);

    // D19: the first pin increments the vault revision but keeps the binding.
    vault_write(test, owner, "first pin", Some(PLANTED), async {
        let fingerprint = lookup(OWNER, FIRST).await.str(200, "fingerprint");
        assert_eq!(pin(OWNER, FIRST, &fingerprint).await.status, 200);
        assert_eq!(wrap_mark(test, owner, STRONG).await, ZaMark::Pinned);
    })
    .await;

    // The next pin writes only the pin record.
    let revision = vault_revision(test, owner).await;
    let fingerprint = lookup(OWNER, MFA).await.str(200, "fingerprint");
    assert_eq!(pin(OWNER, MFA, &fingerprint).await.status, 200);
    assert_eq!(vault_revision(test, owner).await, revision);
    assert!(raw_token_record(test, owner).await.as_deref() == Some(PLANTED));

    set_raw_token_record(test, owner, None).await;
    assert_eq!(forget(OWNER, Some(FIRST)).await.status, 200);
    assert_eq!(forget(OWNER, Some(MFA)).await.status, 200);
}

async fn credential_changes_end_sessions(test: &mut TestServer, owner: u32, mfa: u32) {
    vault_write(test, owner, "password change", None, async {
        let reply = za_post(
            "password",
            &json!({ "username": OWNER, "password": STRONG, "new_password": STRONG2 }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;

    let recovery_key = vault_write(test, owner, "recovery", None, async {
        let reply = za_post(
            "recover",
            &json!({
                "username": OWNER,
                "recovery_key": test.account(OWNER).recovery_key.as_deref().unwrap(),
                "new_password": STRONG,
            }),
        )
        .await;
        assert_eq!(reply.status, 200);
        reply.body["recovery_key"].as_str().unwrap().to_owned()
    })
    .await;
    test.accounts.get_mut(OWNER).unwrap().recovery_key = Some(recovery_key);

    let url = totp_url();
    vault_write(test, mfa, "second factor enrolled", None, async {
        let reply = za_post(
            "totp",
            &json!({
                "username": MFA, "password": STRONG, "otp_auth": url, "confirm": code(&url),
            }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;
    vault_write(test, mfa, "second factor removed", None, async {
        let reply = za_post(
            "totp",
            &json!({ "username": MFA, "password": STRONG, "otp_auth": null, "totp": code(&url) }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;
}

async fn other_vault_writes_keep_sessions(test: &mut TestServer, owner: u32) {
    let recovery_key = vault_write(test, owner, "new recovery key", Some(PLANTED), async {
        let reply = za_post(
            "recovery-key",
            &json!({ "username": OWNER, "password": STRONG }),
        )
        .await;
        assert_eq!(reply.status, 200);
        reply.body["recovery_key"].as_str().unwrap().to_owned()
    })
    .await;
    test.accounts.get_mut(OWNER).unwrap().recovery_key = Some(recovery_key);

    let parked = vault_write(
        test,
        owner,
        "app password pending",
        Some(PLANTED),
        park_app_password(owner, "Publish"),
    )
    .await;
    let credential_id = vault_write(
        test,
        owner,
        "app password published",
        Some(PLANTED),
        async {
            let reply = parked.finish().await;
            assert_eq!(reply.status, 200);
            reply.body["credential_id"].as_u64().unwrap() as u32
        },
    )
    .await;
    vault_write(test, owner, "app password revoked", Some(PLANTED), async {
        let reply = za_post(
            "app-password/revoke",
            &json!({ "username": OWNER, "password": STRONG, "credential_id": credential_id }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;

    let parked = park_app_password(owner, "Withdraw").await;
    bump_registry_revision(test).await;
    vault_write(
        test,
        owner,
        "app password withdrawn",
        Some(PLANTED),
        async {
            let reply = parked.finish().await;
            assert_eq!(reply.status, 409);
            assert_eq!(reply.body["error"], "registry write failed");
        },
    )
    .await;

    {
        let _drain = DrainOn::new();
        let original = substitute_public_key(test, owner, &writer_key(), true).await;
        vault_write(test, owner, "self-check repair", Some(PLANTED), async {
            assert_eq!(trigger_drain(OWNER, STRONG).await, 207);
            let repaired = test.server.za_vault_record(owner).await.unwrap().unwrap();
            assert_eq!(repaired.record.public_key, original);
        })
        .await;
    }
    set_raw_token_record(test, owner, None).await;
}

/// All the vault-write cases check the revision and the stored bytes
/// independently. Unusable planted bytes still participate in the clear.
async fn vault_write<T>(
    test: &TestServer,
    account_id: u32,
    row: &str,
    expected: Option<&[u8]>,
    write: impl Future<Output = T>,
) -> T {
    set_raw_token_record(test, account_id, Some(PLANTED)).await;
    let before = vault_revision(test, account_id).await;
    let result = write.await;
    assert_eq!(vault_revision(test, account_id).await, before + 1, "{row}");
    assert!(
        raw_token_record(test, account_id).await.as_deref() == expected,
        "unexpected token record after {row}"
    );
    result
}

async fn park_app_password(owner: u32, description: &'static str) -> Parked<VaultReply> {
    Parked::start(owner, za_test::set_publish, async move {
        za_post(
            "app-password",
            &json!({ "username": OWNER, "password": STRONG, "description": description }),
        )
        .await
    })
    .await
}

/// Bump the registry object's revision so a write based on the old read fails.
async fn bump_registry_revision(test: &TestServer) {
    let id = test.account(OWNER).id();
    let registry = test.server.registry();
    let object = registry
        .get(ObjectId::new(ObjectType::Account, id))
        .await
        .unwrap()
        .unwrap();
    let old_object = Object::with_revision(object.inner.clone(), object.revision);
    let mut inner = object.inner;
    let ObjectInner::Account(account) = &mut inner else {
        panic!("not an account");
    };
    let structs::Account::User(user) = account.as_mut() else {
        panic!("not a user account");
    };
    user.description = Some(format!("Token Owner, revision {}", object.revision));
    let result = registry
        .write(RegistryWrite::Update {
            object: &Object::new(inner),
            id,
            old_object: &old_object,
        })
        .await
        .unwrap();
    assert!(matches!(result, RegistryWriteResult::Success(_)));
}
