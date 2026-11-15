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
        token_record, totp_url, trigger_drain, vault_revision, wait_for_index_queue, wrap_mark,
        writer_key, za_grant_token, za_post, za_revoke_grant, za_unlock,
    },
};
use common::{
    auth::{
        oauth::{grant::GrantRotation, scope::OAuthScopes},
        vault::{RawValue, ZaMark},
    },
    za::{
        test_hooks,
        tokens::{ZaIssued, ZaTokenIssue},
    },
};
use http::auth::authenticate::za_test;
use registry::{
    schema::{
        prelude::{Object, ObjectInner, ObjectType},
        structs,
    },
    types::id::ObjectId,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use store::{
    ValueKey,
    registry::write::{RegistryWrite, RegistryWriteResult},
    write::{BatchBuilder, now},
};
use types::{collection::Collection, field::PrincipalField};
use vault::{
    keys::{Secret, derive_token_tag},
    record::za_session_binding,
    session::SessionKeys,
    tokens::{TOKEN_RECORD_VERSION, ZaRefreshClass},
};

const OWNER: &str = "token-owner@example.com";
const MFA: &str = "token-mfa@example.com";
const CAP: &str = "token-cap@example.com";
const PLAIN: &str = "token-plain@example.com";
const STRONG2: &str = "another long passphrase with 2 numbers";
const PLANTED: &[u8] = &[1, 0xde, 0xad];
const HOUR: u64 = 3_600;
const MONTH: u64 = 2_592_000;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access token tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description) in [
        (OWNER, "Token Owner"),
        (MFA, "Token MFA"),
        (CAP, "Token Cap"),
    ] {
        let account = admin
            .create_key_user_account(name, STRONG, description, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    let owner = test.account(OWNER).id().document_id();
    let mfa = test.account(MFA).id().document_id();
    let cap = test.account(CAP).id().document_id();
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Token Plain", &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();

    pin_rewrap_keeps_session(test, owner).await;
    credential_changes_end_sessions(test, owner, mfa).await;
    other_vault_writes_keep_sessions(test, owner).await;
    issue_writes_first_entries(test, owner).await;
    issue_refuses_changed_binding(test, owner).await;
    open_access_needs_entry_and_binding(test, owner).await;
    forget_grant_asserts_the_record_alone(test, owner).await;
    revocation_removes_entries(test, owner).await;
    cap_evicts_least_recently_used(test, cap).await;
    forget_failure_does_not_fail_deletion(test, owner).await;
    ordinary_grant_deletion_reads_no_record(test, plain_id).await;

    wait_for_index_queue(test, plain_id, Duration::from_secs(30)).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

async fn minted(
    test: &TestServer,
    name: &str,
    id: u32,
    password: &str,
) -> (u64, String, Arc<SessionKeys>, u64) {
    let (grant, access) = za_grant_token(&test.server, name, OAuthScopes::ALL).await;
    let keys = za_unlock(test, id, password).await;
    let vault = test.server.za_vault_record(id).await.unwrap().unwrap();
    (grant, access, keys, za_session_binding(&vault.record))
}

async fn issued_for(
    test: &TestServer,
    name: &str,
    id: u32,
) -> (u64, ZaIssued, Arc<SessionKeys>, u64) {
    let (grant, _, keys, binding) = minted(test, name, id, STRONG).await;
    let ZaTokenIssue::Issued(issued) = issue(test, id, grant, keys.mk(), binding).await else {
        panic!("the grant was not issued");
    };
    (grant, issued, keys, binding)
}

async fn issue(test: &TestServer, id: u32, grant: u64, mk: &Secret, binding: u64) -> ZaTokenIssue {
    test.server
        .za_token_issue(id, grant, mk, binding, now() + HOUR, now() + MONTH)
        .await
        .unwrap()
}

async fn issue_writes_first_entries(test: &TestServer, id: u32) {
    println!("  issue_writes_first_entries");
    let (grant, issued, keys, binding) = issued_for(test, OWNER, id).await;
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.version, TOKEN_RECORD_VERSION);
    assert_eq!(record.binding, binding);
    assert_eq!(record.grants.len(), 1);
    let entries = record.grant(grant).unwrap();
    assert_eq!(entries.refresh.len(), 1);
    assert_eq!(entries.refresh[0].generation, 0);
    assert!(entries.refresh[0].next.is_none());
    assert_eq!(entries.access.len(), 1);
    assert!(entries.access[0].tag == derive_token_tag(&issued.access));
    let opened = record
        .open_access(id, grant, &issued.access, now())
        .unwrap();
    assert!(opened.as_bytes() == keys.mk().as_bytes());
    assert!(matches!(
        record.classify_refresh(id, grant, 0, &issued.refresh, now()),
        ZaRefreshClass::Opened { next: None, .. }
    ));
    za_revoke_grant(&test.server, grant).await;
}

async fn issue_refuses_changed_binding(test: &mut TestServer, id: u32) {
    println!("  issue_refuses_changed_binding");
    let (grant, _, keys, stale) = minted(test, OWNER, id, STRONG).await;
    new_recovery_key(test).await;
    assert!(matches!(
        issue(test, id, grant, keys.mk(), stale).await,
        ZaTokenIssue::Issued(_)
    ));
    assert_eq!(token_record(test, id).await.unwrap().binding, stale);
    za_revoke_grant(&test.server, grant).await;

    let (grant, _, keys, stale) = minted(test, OWNER, id, STRONG).await;
    change_password(STRONG, STRONG2).await;
    assert!(matches!(
        issue(test, id, grant, keys.mk(), stale).await,
        ZaTokenIssue::VaultChanged
    ));
    assert!(raw_token_record(test, id).await.is_none());
    za_revoke_grant(&test.server, grant).await;
    change_password(STRONG2, STRONG).await;
}

async fn open_access_needs_entry_and_binding(test: &mut TestServer, id: u32) {
    println!("  open_access_needs_entry_and_binding");
    let (grant, issued, keys, _) = issued_for(test, OWNER, id).await;
    let (other, _, _, _) = issued_for(test, OWNER, id).await;
    let raw = raw_token_record(test, id).await.unwrap();
    let revision = vault_revision(test, id).await;
    let opened = test
        .server
        .za_token_open_access(id, grant, &issued.access)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(opened.generation, revision);
    assert_eq!(opened.account_id, id);
    assert!(opened.mk().as_bytes() == keys.mk().as_bytes());
    let random = Secret::random();
    for (grant, secret) in [
        (grant, &random),
        (other, &issued.access),
        (grant, &issued.refresh),
    ] {
        assert!(
            test.server
                .za_token_open_access(id, grant, secret)
                .await
                .unwrap()
                .is_none()
        );
    }

    new_recovery_key(test).await;
    let reopened = test
        .server
        .za_token_open_access(id, grant, &issued.access)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.generation, revision + 1);
    assert!(reopened.mk().as_bytes() == keys.mk().as_bytes());

    change_password(STRONG, STRONG2).await;
    assert!(
        test.server
            .za_token_open_access(id, grant, &issued.access)
            .await
            .unwrap()
            .is_none()
    );
    assert!(raw_token_record(test, id).await.is_none());

    set_raw_token_record(test, id, Some(&raw)).await;
    assert!(
        test.server
            .za_token_open_access(id, grant, &issued.access)
            .await
            .unwrap()
            .is_none()
    );
    set_raw_token_record(test, id, None).await;
    change_password(STRONG2, STRONG).await;
    za_revoke_grant(&test.server, grant).await;
    za_revoke_grant(&test.server, other).await;
}

async fn new_recovery_key(test: &mut TestServer) {
    let reply = za_post(
        "recovery-key",
        &json!({ "username": OWNER, "password": STRONG }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(OWNER).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
}

async fn change_password(old: &str, new: &str) {
    let reply = za_post(
        "password",
        &json!({ "username": OWNER, "password": old, "new_password": new }),
    )
    .await;
    assert_eq!(reply.status, 200);
}

async fn forget_grant_asserts_the_record_alone(test: &TestServer, id: u32) {
    println!("  forget_grant_asserts_the_record_alone");
    let (g1, _, _, _) = issued_for(test, OWNER, id).await;
    let (g2, _, _, _) = issued_for(test, OWNER, id).await;
    let raw_vault = test
        .server
        .store()
        .get_value::<RawValue>(ValueKey::property(
            id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessVault,
        ))
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut broken_vault = raw_vault.clone();
    *broken_vault.last_mut().unwrap() ^= u8::MAX;
    set_raw_vault(test, id, broken_vault).await;
    assert!(test.server.za_vault_record(id).await.is_err());

    test.server.za_token_forget_grant(id, g1).await.unwrap();
    let record = token_record(test, id).await.unwrap();
    assert!(record.grant(g1).is_none());
    assert!(record.grant(g2).is_some());
    let first = raw_token_record(test, id).await.unwrap();
    test.server.za_token_forget_grant(id, g1).await.unwrap();
    assert!(raw_token_record(test, id).await.as_ref() == Some(&first));

    set_raw_vault(test, id, raw_vault).await;
    test.server.za_invalidate_account(id).await.unwrap();
    za_revoke_grant(&test.server, g1).await;
    za_revoke_grant(&test.server, g2).await;
}

async fn set_raw_vault(test: &TestServer, id: u32, raw: Vec<u8>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(PrincipalField::ZeroAccessVault, raw);
    test.server.store().write_batch(&mut batch).await.unwrap();
}

async fn revocation_removes_entries(test: &TestServer, id: u32) {
    println!("  revocation_removes_entries");
    let (g1, _, _, _) = issued_for(test, OWNER, id).await;
    let (g2, _, _, _) = issued_for(test, OWNER, id).await;
    za_revoke_grant(&test.server, g1).await;
    assert!(token_record(test, id).await.unwrap().grant(g1).is_none());
    za_revoke_grant(&test.server, g2).await;
    assert!(raw_token_record(test, id).await.is_none());
}

async fn cap_evicts_least_recently_used(test: &TestServer, id: u32) {
    println!("  cap_evicts_least_recently_used");
    let mut grants = Vec::with_capacity(32);
    for _ in 0..32 {
        let (grant, _, _, _) = issued_for(test, CAP, id).await;
        grants.push(grant);
    }
    assert_eq!(token_record(test, id).await.unwrap().grants.len(), 32);

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let stored = test.server.oauth_grant(grants[0]).await.unwrap().unwrap();
    assert!(matches!(
        test.server.rotate_oauth_grant(stored, 0).await.unwrap(),
        GrantRotation::Issue { .. }
    ));
    let (last, _, _, _) = issued_for(test, CAP, id).await;
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.grants.len(), 32);
    assert!(record.grant(grants[1]).is_none());
    assert!(record.grant(grants[0]).is_some());
    assert!(record.grant(last).is_some());
    assert!(test.server.oauth_grant(grants[1]).await.unwrap().is_none());
    assert!(test.server.oauth_grant(grants[0]).await.unwrap().is_some());

    grants.remove(1);
    for grant in grants.into_iter().chain([last]) {
        za_revoke_grant(&test.server, grant).await;
    }
    assert!(raw_token_record(test, id).await.is_none());
}

async fn forget_failure_does_not_fail_deletion(test: &TestServer, id: u32) {
    println!("  forget_failure_does_not_fail_deletion");
    let (grant, _, _, _) = issued_for(test, OWNER, id).await;
    test_hooks::set_token_forget_fail_once(Some(id));
    za_revoke_grant(&test.server, grant).await;
    assert!(test.server.oauth_grant(grant).await.unwrap().is_none());
    assert!(token_record(test, id).await.unwrap().grant(grant).is_some());
    test.server.za_token_forget_grant(id, grant).await.unwrap();
    assert!(raw_token_record(test, id).await.is_none());
}

async fn ordinary_grant_deletion_reads_no_record(test: &TestServer, id: u32) {
    println!("  ordinary_grant_deletion_reads_no_record");
    set_raw_token_record(test, id, Some(PLANTED)).await;
    test_hooks::set_token_forget_fail_once(Some(id));
    let (grant, _) = za_grant_token(&test.server, PLAIN, OAuthScopes::ALL).await;
    za_revoke_grant(&test.server, grant).await;
    assert!(raw_token_record(test, id).await.as_deref() == Some(PLANTED));
    assert!(test_hooks::take_token_forget_fail(id));
    set_raw_token_record(test, id, None).await;
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
