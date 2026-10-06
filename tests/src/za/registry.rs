/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, za::za_post};
use jmap_proto::error::set::SetErrorType;
use registry::schema::{
    prelude::{ObjectType, Property},
    structs,
};
use serde_json::json;
use types::id::Id;

/// Every refusal names the account API.
const API_HINT: &str = "zero-access account API";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access registry refusal tests...");
    let admin = test.account("admin@example.com").clone();
    let key = test.account("key1@example.com").clone();

    // Admin password reset through the registry.
    admin
        .registry_update_object_expect_err(
            ObjectType::Account,
            key.id(),
            json!({ "credentials/0/secret": "an admin supplied strong password 1" }),
        )
        .await
        .assert_type(SetErrorType::Forbidden)
        .assert_description_contains(API_HINT);
    // Admin TOTP edit through the registry.
    admin
        .registry_update_object_expect_err(
            ObjectType::Account,
            key.id(),
            json!({ "credentials/0/otpAuth": "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP" }),
        )
        .await
        .assert_type(SetErrorType::Forbidden)
        .assert_description_contains(API_HINT);
    // Edits that leave the credentials alone still go through.
    admin
        .registry_update_object(
            ObjectType::Account,
            key.id(),
            json!({ "description": "Key One, edited" }),
        )
        .await;

    // Self-service singleton: password and TOTP changes.
    key.registry_update_object_expect_err(
        ObjectType::AccountPassword,
        Id::singleton(),
        json!({ Property::CurrentSecret: STRONG, Property::Secret: "yet another strong password 2" }),
    )
    .await
    .assert_type(SetErrorType::Forbidden)
    .assert_description_contains(API_HINT);
    key.registry_update_object_expect_err(
        ObjectType::AccountPassword,
        Id::singleton(),
        json!({
            Property::CurrentSecret: STRONG,
            "otpAuth/otpUrl": "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP"
        }),
    )
    .await
    .assert_type(SetErrorType::Forbidden)
    .assert_description_contains(API_HINT);

    // App-password and API-key creation through the registry.
    key.registry_create_object_expect_err(structs::AppPassword {
        description: "Registry phone".to_string(),
        ..Default::default()
    })
    .await
    .assert_type(SetErrorType::Forbidden)
    .assert_description_contains(API_HINT);
    key.registry_create_object_expect_err(structs::ApiKey {
        description: "Registry key".to_string(),
        ..Default::default()
    })
    .await
    .assert_type(SetErrorType::Forbidden)
    .assert_description_contains(API_HINT);

    // App-password deletion through the registry, by the account and by the
    // admin; the account API revokes it.
    let created = za_post(
        "app-password",
        &json!({ "username": "key1@example.com", "password": STRONG, "description": "Laptop" }),
    )
    .await
    .expect(200);
    let credential_id = created["credential_id"].as_u64().unwrap();
    key.registry_destroy_object_expect_err(ObjectType::AppPassword, Id::from(credential_id))
        .await
        .assert_type(SetErrorType::Forbidden)
        .assert_description_contains(API_HINT);
    admin
        .registry_update_object_expect_err(
            ObjectType::Account,
            key.id(),
            json!({ "credentials/1": null }),
        )
        .await
        .assert_type(SetErrorType::Forbidden)
        .assert_description_contains(API_HINT);
    za_post(
        "app-password/revoke",
        &json!({ "username": "key1@example.com", "password": STRONG, "credential_id": credential_id }),
    )
    .await
    .expect(200);
}
