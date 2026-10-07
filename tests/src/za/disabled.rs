/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, user_permissions};
use crate::utils::{
    account::Account,
    server::TestServer,
    za::{caldav_from, za_post, za_post_from, za_setup, za_setup_token},
};
use hyper::StatusCode;
use registry::{
    schema::{
        enums::Permission,
        prelude::{ObjectType, Property},
        structs::{
            self, CertificateManagement, DkimManagement, DnsManagement, Domain, Permissions,
            PermissionsList, Tenant, UserAccount,
        },
    },
    types::map::Map,
};
use serde_json::json;
use types::id::Id;
use vault::record::VaultState;

const STRONG2: &str = "another long passphrase with 2 numbers";

/// Client address of the deliberate failures in this module.
const FAIL_IP: &str = "10.0.10.1";

fn authenticate_permissions(disabled: bool) -> Permissions {
    Permissions::Merge(PermissionsList {
        disabled_permissions: if disabled {
            Map::new(vec![Permission::Authenticate])
        } else {
            Map::default()
        },
        enabled_permissions: Map::default(),
    })
}

async fn set_disabled(admin: &Account, object: ObjectType, id: Id, disabled: bool) {
    admin
        .registry_update_object(
            object,
            id,
            json!({ Property::Permissions: authenticate_permissions(disabled) }),
        )
        .await;
}

async fn vault_state(test: &TestServer, id: Id) -> VaultState {
    test.server
        .za_vault_record(id.document_id())
        .await
        .unwrap()
        .expect("vault record")
        .record
        .state
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access disabled account tests...");
    let admin = test.account("admin@example.com").clone();

    // setup on a disabled account: 403, vault untouched, token still valid afterwards.
    let name = "disabled1@example.com";
    let account = admin
        .create_passwordless_user_account(name, STRONG, "Disabled One", &[], user_permissions())
        .await;
    let token = za_setup_token(&admin, name).await;
    set_disabled(&admin, ObjectType::Account, account.id(), true).await;
    let body = json!({ "username": name, "token": token, "password": STRONG });
    za_post_from(
        FAIL_IP,
        "setup",
        &json!({ "username": name, "token": "wrong", "password": STRONG }),
    )
    .await
    .expect(401);
    let reply = za_post("setup", &body).await.expect(403);
    assert!(!reply.to_string().contains(&token));
    assert_eq!(
        vault_state(test, account.id()).await,
        VaultState::PendingSetup
    );
    set_disabled(&admin, ObjectType::Account, account.id(), false).await;
    za_post("setup", &body).await.expect(200);
    admin.destroy_account(account).await;

    // recover on a disabled account.
    let name = "disabled2@example.com";
    let account = admin
        .create_key_user_account(name, STRONG, "Disabled Two", &[], user_permissions())
        .await;
    let recovery = account.recovery_key.clone().unwrap();
    set_disabled(&admin, ObjectType::Account, account.id(), true).await;
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .expect(403);
    za_post_from(
        FAIL_IP,
        "recover",
        &json!({
            "username": name,
            "recovery_key": vault::recovery::RecoveryKey::generate().encode(),
            "new_password": STRONG2
        }),
    )
    .await
    .expect(401);
    set_disabled(&admin, ObjectType::Account, account.id(), false).await;
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    caldav_from(FAIL_IP, name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav_from(FAIL_IP, name, STRONG2, StatusCode::MULTI_STATUS).await;
    admin.destroy_account(account).await;

    // recover on a disabled tenant: the account has no disabled permission of its own.
    let tenant_id = admin
        .registry_create_object(Tenant {
            name: "Disabled Tenant".to_string(),
            ..Default::default()
        })
        .await;
    let domain_id = admin
        .registry_create_object(Domain {
            name: "disabled-tenant.org".to_string(),
            member_tenant_id: tenant_id.into(),
            certificate_management: CertificateManagement::Manual,
            dns_management: DnsManagement::Manual,
            dkim_management: DkimManagement::Manual,
            ..Default::default()
        })
        .await;
    let name = "member@disabled-tenant.org";
    let account_id = admin
        .registry_create_object(structs::Account::User(UserAccount {
            name: "member".to_string(),
            domain_id,
            member_tenant_id: tenant_id.into(),
            permissions: Permissions::Merge(PermissionsList {
                disabled_permissions: Map::default(),
                enabled_permissions: Map::new(user_permissions()),
            }),
            ..Default::default()
        }))
        .await;
    let account = Account::new(name, STRONG, &[], "Tenant Member", account_id);
    let token = za_setup_token(&admin, name).await;
    let recovery = za_setup(name, &token, STRONG).await;
    set_disabled(&admin, ObjectType::Tenant, tenant_id, true).await;
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .expect(403);
    admin.destroy_account(account).await;
    admin
        .registry_destroy(ObjectType::Domain, [domain_id])
        .await
        .assert_destroyed(&[domain_id]);
    admin
        .registry_destroy(ObjectType::Tenant, [tenant_id])
        .await
        .assert_destroyed(&[tenant_id]);
    test.wait_for_tasks().await;
}
