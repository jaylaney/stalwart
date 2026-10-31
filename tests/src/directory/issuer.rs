/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{containers, server::TestServerBuilder};
use base64::{Engine, engine::general_purpose};
use registry::{
    schema::{
        prelude::{ObjectType, Property},
        structs::{Action, Directory, OidcDirectory},
    },
    types::map::Map,
};
use serde_json::json;
use std::time::Duration;

const ISSUER: &str = "http://localhost:9080/realms/stalwart";
const DOMAIN: &str = "example.org";
const ACCOUNT: &str = "john.doe@example.org";
const PASSWORD: &str = "this is an OIDC password";

pub async fn test() {
    println!("Running OIDC issuer routing tests...");
    containers::ensure_keycloak().await;
    let test = TestServerBuilder::new("directory_issuer_test")
        .await
        .with_default_listeners()
        .await
        .disable_services()
        .build()
        .await;

    let admin = test.account("admin");
    let directory_id = admin
        .registry_create_object(Directory::Oidc(OidcDirectory {
            description: "Issuer routing test OIDC directory".to_string(),
            issuer_url: ISSUER.to_string(),
            claim_username: "email".to_string(),
            claim_name: Some("name".to_string()),
            claim_groups: Some("groups".to_string()),
            accepted_audiences: Map::new(vec!["stalwart".to_string()]),
            ..Default::default()
        }))
        .await;
    let domain_id = admin.find_or_create_domain(DOMAIN).await;
    admin
        .registry_update_object(
            ObjectType::Domain,
            domain_id,
            json!({ Property::DirectoryId: directory_id.to_string() }),
        )
        .await;
    admin.reload_settings().await;
    admin.registry_create_object(Action::InvalidateCaches).await;

    assert!(
        test.server.get_default_directory().is_none(),
        "The OIDC directory must be reachable only through the domain for this test to mean anything"
    );

    let token = get_token_for_client(
        "stalwart-fallback",
        "stalwart-fallback-secret",
        ACCOUNT,
        PASSWORD,
        "openid",
    )
    .await;
    let claims = access_token_claims(&token);
    for claim in ["email", "preferred_username", "upn"] {
        assert!(
            claims.get(claim).is_none(),
            "The access token carries a {claim} claim, so it no longer covers issuer based routing: {claims}"
        );
    }
    assert_eq!(claims["iss"], json!(ISSUER));

    assert_eq!(
        session_status(&token).await,
        200,
        "A bearer token without a username claim did not reach the domain's OIDC directory"
    );
}

fn access_token_claims(token: &str) -> serde_json::Value {
    let payload = token.split('.').nth(1).expect("The token is not a JWT");

    serde_json::from_slice(
        &general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("Failed to decode the token payload"),
    )
    .expect("Failed to parse the token claims")
}

async fn session_status(token: &str) -> u16 {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .get("https://127.0.0.1:8899/jmap/session")
        .bearer_auth(token)
        .send()
        .await
        .expect("Failed to send session request")
        .status()
        .as_u16()
}

/// Requests an access token for `username` from the test issuer with the
/// resource owner password credentials grant (RFC 6749, section 4.3). The
/// client authenticates with HTTP Basic, its identifier and secret each
/// form-urlencoded first (section 2.3.1). The token endpoint is the one the
/// issuer's discovery document names.
///
/// Zero-access fork (upgrade Task 2): written from RFC 6749. The helper this
/// test used at 0.16 sat in an SEL-only file, which the fork does not carry.
async fn get_token_for_client(
    client_id: &str,
    client_secret: &str,
    username: &str,
    password: &str,
    scope: &str,
) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    // The token endpoint
    let discovery: serde_json::Value = client
        .get(format!("{ISSUER}/.well-known/openid-configuration"))
        .send()
        .await
        .expect("Failed to fetch the issuer's discovery document")
        .json()
        .await
        .expect("Failed to parse the issuer's discovery document");
    let token_endpoint = discovery["token_endpoint"]
        .as_str()
        .expect("The discovery document names no token endpoint");

    // Client authentication (section 2.3.1)
    let credentials = format!(
        "{}:{}",
        form_urlencoded::byte_serialize(client_id.as_bytes()).collect::<String>(),
        form_urlencoded::byte_serialize(client_secret.as_bytes()).collect::<String>()
    );
    let authorization = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(credentials.as_bytes())
    );

    // Access token request (section 4.3.2)
    let body = form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "password")
        .append_pair("username", username)
        .append_pair("password", password)
        .append_pair("scope", scope)
        .finish();
    let response = client
        .post(token_endpoint)
        .header(reqwest::header::AUTHORIZATION, authorization)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .expect("Failed to send the token request");

    // Access token response (sections 4.3.3 and 5.1)
    let status = response.status();
    let response: serde_json::Value = response
        .json()
        .await
        .expect("Failed to parse the token response");
    assert_eq!(status, 200, "The token request failed: {response}");
    response["access_token"]
        .as_str()
        .expect("The token response holds no access token")
        .to_string()
}
