/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{http::HttpRequest, pop3::Pop3Connection, server::TestServer, za::Conn};
use base64::{Engine, engine::general_purpose};
use biscuit::{JWT, SingleOrMultiple, jwk::JWKSet};
use bytes::Bytes;
use common::BuildServer;
use common::auth::oauth::{
    GrantType,
    introspect::OAuthIntrospect,
    oidc::StandardClaims,
    registration::{
        ClientRegistrationRequest, ClientRegistrationResponse, TokenEndpointAuthMethod,
    },
};
use http::auth::oauth::{
    DeviceAuthResponse, ErrorType, TokenResponse,
    auth::{LoginRequest, LoginResponse},
};
use jmap_client::{
    client::{Client, Credentials},
    mailbox::query::Filter,
};
use registry::schema::{
    enums::{JwtSignatureAlgorithm, OAuthGrantType, OAuthScope},
    prelude::{ObjectType, Property},
    structs::{OAuthClient, OidcProvider, SecretText, SecretTextValue},
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::{Duration, Instant};
use store::ahash::AHashMap;

#[derive(Debug, serde::Deserialize)]
pub struct OAuthMetadata {
    pub issuer: String,
    pub token_endpoint: String,
    pub authorization_endpoint: String,
    pub device_authorization_endpoint: String,
    pub registration_endpoint: String,
    pub introspection_endpoint: String,
    pub grant_types_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub authorization_response_iss_parameter_supported: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub bearer_methods_supported: Vec<String>,
}

const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
// 1.0 issues an ID token only for the `openid` scope (OpenID Connect Core
// 3.1.2.1); 0.16 issued one for any authorization code.
const PROFILE_SCOPE: &str = "openid urn:ietf:params:oauth:scope:mail offline_access";
const REFRESH_TOKEN_LIFETIME: u64 = 10;

#[derive(Debug, serde::Deserialize)]
pub struct OpenIdMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub jwks_uri: String,
    pub registration_endpoint: String,
    pub device_authorization_endpoint: String,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub subject_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    pub claims_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub authorization_response_iss_parameter_supported: bool,
}

pub async fn test(test: &mut TestServer) {
    println!("Running OIDC tests...");

    let admin = test.account("admin@example.org");

    // Set test parameters. Upgrade ruling 74: 1.0 removed
    // `auth_code_max_attempts`, `refresh_token_renewal`,
    // `anonymous_client_registration` and `require_client_registration`
    // (subject deleted); the assertions that depended on them are marked
    // where they stood.
    // 1.0 refuses an `authCodeExpiry` or `accessTokenExpiry` under ten
    // minutes and a `refreshTokenExpiry` under a day, so the short lifetimes
    // this test waits out are set on the running core after the settings
    // load, as 0.16 set them through the registry.
    let settings = OidcProvider {
        user_code_expiry: registry::schema::prelude::Duration::from_millis(1000),
        signature_algorithm: JwtSignatureAlgorithm::Rs256,
        signature_key: SecretText::Text(SecretTextValue {
            secret: OIDC_SIGNATURE_KEY_RS256.to_string(),
        }),
        ..Default::default()
    };
    admin
        .registry_update_setting(
            settings,
            &[
                Property::UserCodeExpiry,
                Property::SignatureAlgorithm,
                Property::SignatureKey,
            ],
        )
        .await;
    admin.reload_settings().await;
    let mut core = (*test.server.inner.shared_core.load_full()).clone();
    core.oauth.oauth_expiry_token = 1;
    core.oauth.oauth_expiry_auth_code = 1;
    core.oauth.oauth_expiry_refresh_token = REFRESH_TOKEN_LIFETIME;
    test.server
        .inner
        .shared_core
        .store(std::sync::Arc::new(core));

    // Create test account
    let user = test
        .create_user_account(
            "admin@example.org",
            "user@example.org",
            "this is a very strong password",
            &[],
            "user@example.org",
        )
        .await;
    let user_id = user.id();

    // Build API
    let http = HttpRequest::new();

    // Obtain OAuth metadata
    let metadata: OAuthMetadata =
        get("https://127.0.0.1:8899/.well-known/oauth-authorization-server").await;
    let oidc_metadata: OpenIdMetadata =
        get("https://127.0.0.1:8899/.well-known/openid-configuration").await;
    let jwk_set: JWKSet<()> = get(&oidc_metadata.jwks_uri).await;

    // OAuth Public Clients profile: the authorization server metadata must advertise the
    // mandatory properties (RFC 8414 + draft-ietf-mailmaint-oauth-public).
    assert!(
        metadata
            .grant_types_supported
            .iter()
            .any(|g| g == "authorization_code")
    );
    assert!(
        metadata
            .grant_types_supported
            .iter()
            .any(|g| g == "refresh_token")
    );
    assert!(
        metadata
            .response_types_supported
            .iter()
            .any(|r| r == "code")
    );
    assert!(
        metadata
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "none")
    );
    assert!(
        metadata
            .code_challenge_methods_supported
            .iter()
            .any(|m| m == "S256")
    );
    assert!(metadata.authorization_response_iss_parameter_supported);
    for scope in [
        "urn:ietf:params:oauth:scope:mail",
        "urn:ietf:params:oauth:scope:contacts",
        "urn:ietf:params:oauth:scope:calendars",
        "offline_access",
    ] {
        assert!(
            metadata.scopes_supported.iter().any(|s| s == scope),
            "missing scope {scope}"
        );
    }
    assert!(
        oidc_metadata
            .grant_types_supported
            .iter()
            .any(|g| g == "refresh_token")
    );
    assert!(
        oidc_metadata
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "none")
    );
    assert!(oidc_metadata.authorization_response_iss_parameter_supported);

    // Protected Resource Metadata (RFC 9728)
    let resource_metadata: ProtectedResourceMetadata =
        get("https://127.0.0.1:8899/.well-known/oauth-protected-resource").await;
    assert_eq!(
        resource_metadata.authorization_servers,
        vec![metadata.issuer.clone()]
    );
    assert!(
        resource_metadata
            .bearer_methods_supported
            .iter()
            .any(|m| m == "header")
    );
    assert!(!resource_metadata.resource.is_empty());

    // Dynamic Client Registration: invalid redirect URIs are rejected (RFC 7591 §3.2.2)
    for bad_uri in [
        "http://example.com/cb",
        "http://127.0.0.1/cb#frag",
        "http://127.0.0.1/../cb",
    ] {
        let (status, body) = post_json_raw(
            &metadata.registration_endpoint,
            &ClientRegistrationRequest {
                redirect_uris: vec![bad_uri.to_string()],
                ..Default::default()
            },
        )
        .await;
        assert_eq!(status, 400, "expected rejection for {bad_uri}: {body}");
        assert_eq!(body["error"], "invalid_redirect_uri", "for {bad_uri}");
    }

    // A loopback redirect URI is accepted and registration returns 201 Created,
    // including loopback URIs that specify an ephemeral port (RFC 8252 §7.3).
    for good_uri in [
        "http://127.0.0.1/cb",
        "http://127.0.0.1:54321/cb",
        "http://[::1]:8080/cb",
        "http://127.0.0.1/",
        "http://127.0.0.1:54321/",
        "http://[::1]:8080/",
    ] {
        let (status, body) = post_json_raw(
            &metadata.registration_endpoint,
            &ClientRegistrationRequest {
                redirect_uris: vec![good_uri.to_string()],
                scope: Some(PROFILE_SCOPE.to_string()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            status, 201,
            "registration should return 201 for {good_uri}: {body}"
        );
    }
    // 1.0 requires a loopback redirect URI to carry a path, at least "/"
    // (`loopback_redirect_parts`); 0.16 accepted these without one.
    for pathless_uri in [
        "http://127.0.0.1",
        "http://127.0.0.1:54321",
        "http://[::1]:8080",
    ] {
        let (status, body) = post_json_raw(
            &metadata.registration_endpoint,
            &ClientRegistrationRequest {
                redirect_uris: vec![pathless_uri.to_string()],
                scope: Some(PROFILE_SCOPE.to_string()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(status, 400, "expected rejection for {pathless_uri}: {body}");
        assert_eq!(body["error"], "invalid_redirect_uri", "for {pathless_uri}");
    }

    // Register the client used for the flow with a private-use scheme redirect URI
    let registration: ClientRegistrationResponse = post_json(
        &metadata.registration_endpoint,
        None,
        &ClientRegistrationRequest {
            redirect_uris: vec!["com.example.app:/cb".to_string()],
            scope: Some(PROFILE_SCOPE.to_string()),
            ..Default::default()
        },
    )
    .await;
    let client_id = registration.client_id;

    // Public client ids are stateless (self-describing) and issued deterministically
    assert!(
        client_id.starts_with("swc1."),
        "expected stateless client id, got {client_id}"
    );
    let registration2: ClientRegistrationResponse = post_json(
        &metadata.registration_endpoint,
        None,
        &ClientRegistrationRequest {
            redirect_uris: vec!["com.example.app:/cb".to_string()],
            scope: Some(PROFILE_SCOPE.to_string()),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        registration2.client_id, client_id,
        "identical registration must be deterministic"
    );

    /*println!("OAuth metadata: {:#?}", metadata);
    println!("OpenID metadata: {:#?}", oidc_metadata);
    println!("JWKSet: {:#?}", jwk_set);*/

    // ------------------------
    // Authorization code flow
    // ------------------------

    // A redirect URI that does not match the client registration must be rejected
    // and the authorization server must not issue a code (OAuth Public Clients §3.4)
    let (status, body) = post_login_raw(&auth_code_request(AuthCodeQuery {
        client_id: &client_id,
        redirect_uri: "com.example.app:/evil",
        scope: Some(PROFILE_SCOPE),
        code_challenge: Some(PKCE_CHALLENGE),
        ..Default::default()
    }))
    .await;
    assert!(
        !is_authorized(status, &body),
        "mismatched redirect URI must not be authorized: {status} {body}"
    );

    // An unknown resource indicator must be rejected (RFC 8707)
    let (status, body) = post_login_raw(&auth_code_request(AuthCodeQuery {
        client_id: &client_id,
        redirect_uri: "com.example.app:/cb",
        scope: Some(PROFILE_SCOPE),
        code_challenge: Some(PKCE_CHALLENGE),
        resources: &["https://evil.example.com/jmap"],
        ..Default::default()
    }))
    .await;
    assert!(
        !is_authorized(status, &body),
        "unknown resource indicator must not be authorized: {status} {body}"
    );

    // Authenticate with the correct password, PKCE (S256), scope and a valid resource indicator
    let response = http
        .post::<LoginResponse>(
            "/api/auth",
            &auth_code_request(AuthCodeQuery {
                client_id: &client_id,
                redirect_uri: "com.example.app:/cb",
                nonce: Some("abc1234"),
                scope: Some(PROFILE_SCOPE),
                code_challenge: Some(PKCE_CHALLENGE),
                resources: &["https://mail.example.org/jmap/session"],
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    // The issuer returned in the authorization response must match the metadata issuer (RFC 9207)
    // 1.0 answers with the redirect location, which carries the code and
    // `iss` (RFC 9207 2).
    assert_eq!(
        response.location_param("iss").as_deref(),
        Some(metadata.issuer.as_str()),
        "{response:?}"
    );

    // Both client_id and redirect_uri have to match
    let mut token_params = AHashMap::from_iter([
        ("client_id".to_string(), "invalid_client".to_string()),
        (
            "redirect_uri".to_string(),
            "com.example.app:/cb".to_string(),
        ),
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), response.unwrap_code()),
        ("code_verifier".to_string(), PKCE_VERIFIER.to_string()),
    ]);
    // 1.0 answers a code presented by another client, or with another
    // redirect URI, `invalid_grant` (RFC 6749 5.2); 0.16 answered
    // `invalid_client`.
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    );
    token_params.insert("client_id".to_string(), client_id.to_string());
    token_params.insert(
        "redirect_uri".to_string(),
        "com.example.app:/other".to_string(),
    );
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    );

    // A missing or invalid PKCE verifier must be rejected (RFC 7636)
    token_params.insert(
        "redirect_uri".to_string(),
        "com.example.app:/cb".to_string(),
    );
    token_params.insert(
        "code_verifier".to_string(),
        "the-wrong-verifier".to_string(),
    );
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    );

    // Obtain token and verify the granted scope is echoed back. The token
    // lives a minute: the SASL steps below outlast the one-second lifetime
    // since 1.0 delays the answer to the invalid bearer (`auth_errors_wait`)
    // before the valid one is tried; the one-second lifetime is restored
    // after them.
    set_access_token_expiry(test, 60);
    token_params.insert("code_verifier".to_string(), PKCE_VERIFIER.to_string());
    let granted = post::<TokenResponse>(&metadata.token_endpoint, &token_params).await;
    if let TokenResponse::Granted(response) = &granted {
        assert_eq!(response.scope, PROFILE_SCOPE);
    }
    let (token, refresh_token, id_token) = unwrap_oidc_token_response(granted);

    // Connect to account using token and attempt to search
    let john_client = Client::new()
        .credentials(Credentials::bearer(&token))
        .accept_invalid_certs(true)
        .follow_redirects(["127.0.0.1"])
        .connect("https://127.0.0.1:8899")
        .await
        .unwrap();
    assert_eq!(john_client.default_account_id(), user_id.to_string());
    assert!(
        !john_client
            .mailbox_query(None::<Filter>, None::<Vec<_>>)
            .await
            .unwrap()
            .ids()
            .is_empty()
    );

    // Verify ID token using the JWK set
    let id_token = JWT::<StandardClaims, biscuit::Empty>::new_encoded(&id_token)
        .decode_with_jwks(&jwk_set, None)
        .unwrap();
    let claims = id_token.payload().unwrap();
    let registered_claims = &claims.registered;
    let private_claims = &claims.private;
    assert_eq!(registered_claims.issuer, Some(oidc_metadata.issuer));
    assert_eq!(
        registered_claims.subject,
        Some(user_id.document_id().to_string())
    );
    assert_eq!(
        registered_claims.audience,
        Some(SingleOrMultiple::Single(client_id.to_string()))
    );
    assert_eq!(private_claims.nonce, Some("abc1234".into()));
    assert_eq!(
        private_claims.preferred_username,
        Some("user@example.org".into())
    );
    assert_eq!(private_claims.email, Some("user@example.org".into()));

    // Introspect token
    let access_introspect: OAuthIntrospect = post_with_auth::<OAuthIntrospect>(
        &metadata.introspection_endpoint,
        token.as_str().into(),
        &AHashMap::from_iter([("token".to_string(), token.to_string())]),
    )
    .await;
    assert_eq!(access_introspect.username.unwrap(), "user@example.org");
    assert_eq!(access_introspect.token_type.unwrap(), "bearer");
    // 1.0 names the token's client in the introspection (RFC 7662 2.2);
    // 0.16 left `client_id` out.
    assert_eq!(
        access_introspect.client_id.as_deref(),
        Some(client_id.as_str())
    );
    assert!(access_introspect.active);
    let refresh_introspect = post_with_auth::<OAuthIntrospect>(
        &metadata.introspection_endpoint,
        token.as_str().into(),
        &AHashMap::from_iter([("token".to_string(), refresh_token.unwrap())]),
    )
    .await;
    assert_eq!(refresh_introspect.username.unwrap(), "user@example.org");
    assert_eq!(
        refresh_introspect.client_id.as_deref(),
        Some(client_id.as_str())
    );
    assert!(refresh_introspect.active);
    assert_eq!(
        refresh_introspect.iat.unwrap(),
        access_introspect.iat.unwrap()
    );

    // Try SMTP OAUTHBEARER auth
    let oauth_bearer_invalid_sasl = general_purpose::STANDARD.encode(format!(
        "n,a={},\u{1}auth=Bearer {}\u{1}\u{1}",
        "user@domain", "invalid_token"
    ));
    let oauth_bearer_sasl = general_purpose::STANDARD.encode(format!(
        "n,a={},\u{1}auth=Bearer {}\u{1}\u{1}",
        "user@domain", token
    ));
    // 1.0 offers the bearer mechanisms only over TLS, on SMTP and IMAP alike.
    let mut smtp = Conn::open_smtp(super::SUBMISSION_PORT).await;
    smtp.send("EHLO localhost\r\n").await;
    smtp.until(&["250 "]).await;
    smtp.send(&format!("AUTH OAUTHBEARER {oauth_bearer_invalid_sasl}\r\n"))
        .await;
    // 1.0 answers an invalid bearer token with RFC 7628's 334 error challenge
    // (0.16 answered 4xx); the client's dummy response then gets 535.
    let line = smtp.until(&["2", "3", "4", "5"]).await;
    let challenge = line
        .strip_prefix("334 ")
        .and_then(|challenge| general_purpose::STANDARD.decode(challenge.trim_end()).ok())
        .map(|challenge| String::from_utf8(challenge).unwrap())
        .unwrap_or_else(|| panic!("invalid bearer answered {line:?}"));
    assert!(
        challenge.contains("\"status\":\"invalid_token\""),
        "invalid bearer challenge {challenge:?}"
    );
    smtp.exchange("AQ==\r\n", "535").await;
    smtp.send(&format!("AUTH OAUTHBEARER {oauth_bearer_sasl}\r\n"))
        .await;
    let line = smtp.until(&["2", "3", "4", "5"]).await;
    assert!(line.starts_with("235"), "valid bearer answered {line:?}");

    // Try IMAP OAUTHBEARER auth
    let mut imap = Conn::open(9992).await;
    imap.until(&["* OK"]).await;
    imap.exchange(
        &format!("a1 AUTHENTICATE OAUTHBEARER {oauth_bearer_sasl}\r\n"),
        "a1 OK",
    )
    .await;

    // Try POP3 OAUTHBEARER auth
    let mut pop3 = Pop3Connection::connect().await;
    pop3.send(&format!("AUTH OAUTHBEARER {oauth_bearer_sasl}"))
        .await;
    pop3.assert_read(crate::utils::pop3::ResponseType::Ok).await;
    set_access_token_expiry(test, 1);

    // ------------------------
    // Confidential client with client_secret
    // ------------------------

    // Registering a confidential client requires authentication and returns a
    // generated client_secret exactly once. Web (https) redirect URIs are allowed.
    let confidential_redirect = "https://confidential.example.org/callback";
    let confidential: ClientRegistrationResponse = post_json_basic(
        &metadata.registration_endpoint,
        "admin",
        "popolna_zapora",
        &ClientRegistrationRequest {
            redirect_uris: vec![confidential_redirect.to_string()],
            scope: Some(PROFILE_SCOPE.to_string()),
            token_endpoint_auth_method: Some(TokenEndpointAuthMethod::ClientSecretPost),
            ..Default::default()
        },
    )
    .await;
    let confidential_id = confidential.client_id;
    let confidential_secret = confidential
        .client_secret
        .expect("confidential client must receive a client_secret");
    assert!(
        !confidential_id.starts_with("swc1."),
        "confidential client id must be registry-backed, got {confidential_id}"
    );
    assert!(
        confidential_secret.len() >= 40,
        "client secret is too short: {confidential_secret}"
    );

    // Registering a confidential client anonymously must be rejected
    let (status, _) = post_json_raw(
        &metadata.registration_endpoint,
        &ClientRegistrationRequest {
            redirect_uris: vec![confidential_redirect.to_string()],
            token_endpoint_auth_method: Some(TokenEndpointAuthMethod::ClientSecretBasic),
            ..Default::default()
        },
    )
    .await;
    assert_ne!(
        status, 201,
        "anonymous confidential client registration must be rejected"
    );

    let base_params = || {
        AHashMap::from_iter([
            ("client_id".to_string(), confidential_id.to_string()),
            (
                "redirect_uri".to_string(),
                confidential_redirect.to_string(),
            ),
            ("grant_type".to_string(), "authorization_code".to_string()),
        ])
    };

    // A confidential client that omits its secret must be rejected
    let mut params = base_params();
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, &confidential_id, confidential_redirect).await,
    );
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidClient
        },
        "token request without client_secret must be rejected"
    );

    // A confidential client that presents a wrong secret must be rejected
    let mut params = base_params();
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, &confidential_id, confidential_redirect).await,
    );
    params.insert("client_secret".to_string(), "not-the-secret".to_string());
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidClient
        },
        "token request with a wrong client_secret must be rejected"
    );

    // The correct secret in the request body (client_secret_post) grants a usable token
    let mut params = base_params();
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, &confidential_id, confidential_redirect).await,
    );
    params.insert("client_secret".to_string(), confidential_secret.to_string());
    let (token, _, _) = unwrap_token_response(post(&metadata.token_endpoint, &params).await);
    let confidential_client = Client::new()
        .credentials(Credentials::bearer(&token))
        .accept_invalid_certs(true)
        .follow_redirects(["127.0.0.1"])
        .connect("https://127.0.0.1:8899")
        .await
        .unwrap();
    assert_eq!(
        confidential_client.default_account_id(),
        user_id.to_string()
    );

    // The correct secret in the Authorization header (client_secret_basic) also works
    let mut params = base_params();
    params.remove("client_id");
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, &confidential_id, confidential_redirect).await,
    );
    let granted: TokenResponse = post_form_basic(
        &metadata.token_endpoint,
        &confidential_id,
        &confidential_secret,
        &params,
    )
    .await;
    unwrap_token_response(granted);

    // A confidential client created through the management API must have its
    // secret hashed before storage; authenticating with the plaintext secret
    // only succeeds if the stored value is a verifiable hash.
    let managed_secret = "managed-client-secret-abcdefghijklmnopqrstuvwxyz";
    let managed_id = "managed-confidential-client";
    admin
        .registry_create_object(OAuthClient {
            client_id: managed_id.to_string(),
            redirect_uris: vec![confidential_redirect.to_string()].into(),
            secret: Some(managed_secret.to_string()),
            // 1.0 limits a registry client to the scopes it lists (beyond
            // `openid` and `offline_access`); 0.16 allowed any.
            scopes: vec![OAuthScope::Mail].into(),
            ..Default::default()
        })
        .await;

    let managed_params = || {
        AHashMap::from_iter([
            ("client_id".to_string(), managed_id.to_string()),
            (
                "redirect_uri".to_string(),
                confidential_redirect.to_string(),
            ),
            ("grant_type".to_string(), "authorization_code".to_string()),
        ])
    };

    let mut params = managed_params();
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, managed_id, confidential_redirect).await,
    );
    params.insert("client_secret".to_string(), "wrong-secret".to_string());
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidClient
        },
        "management-api client must reject a wrong secret"
    );

    let mut params = managed_params();
    params.insert(
        "code".to_string(),
        obtain_auth_code(&http, managed_id, confidential_redirect).await,
    );
    params.insert("client_secret".to_string(), managed_secret.to_string());
    unwrap_token_response(post(&metadata.token_endpoint, &params).await);

    // ------------------------
    // Device code flow
    // ------------------------

    // 1.0 grants the device flow only to a registry client that lists it
    // (`ResolvedClient::allows_grant`); a self-describing public client, as
    // registered above, never gets it. 0.16 allowed the registered client.
    let device_client_id = "device-flow-client";
    admin
        .registry_create_object(OAuthClient {
            client_id: device_client_id.to_string(),
            scopes: vec![OAuthScope::Mail].into(),
            grant_types: vec![OAuthGrantType::DeviceCode, OAuthGrantType::RefreshToken].into(),
            ..Default::default()
        })
        .await;

    // Request a device code
    let device_code_params =
        AHashMap::from_iter([("client_id".to_string(), device_client_id.to_string())]);
    let device_response: DeviceAuthResponse =
        post(&metadata.device_authorization_endpoint, &device_code_params).await;
    //println!("Device response: {:#?}", device_response);

    // Status should be pending
    let mut token_params = AHashMap::from_iter([
        ("client_id".to_string(), device_client_id.to_string()),
        (
            "grant_type".to_string(),
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        ),
        (
            "device_code".to_string(),
            device_response.device_code.to_string(),
        ),
    ]);
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::AuthorizationPending
        }
    );

    // Let the code expire and make sure it's invalidated
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        http.post::<LoginResponse>(
            "/api/auth",
            &LoginRequest::AuthDevice {
                account_name: "user@example.org".to_string(),
                account_secret: "this is a very strong password".to_string(),
                mfa_token: None,
                code: device_response.user_code.clone(),
                consent: Some(true),
            },
        )
        .await
        .unwrap(),
        LoginResponse::Failure
    );
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::ExpiredToken
        }
    );

    // Authenticate account using a valid code
    let device_response: DeviceAuthResponse =
        post(&metadata.device_authorization_endpoint, &device_code_params).await;
    token_params.insert(
        "device_code".to_string(),
        device_response.device_code.to_string(),
    );
    assert_eq!(
        http.post::<LoginResponse>(
            "/api/auth",
            &LoginRequest::AuthDevice {
                account_name: "user@example.org".to_string(),
                account_secret: "this is a very strong password".to_string(),
                mfa_token: None,
                code: device_response.user_code.clone(),
                consent: Some(true),
            },
        )
        .await
        .unwrap(),
        LoginResponse::Verified
    );

    // Obtain token
    let (token, refresh_token, _) =
        unwrap_token_response(post(&metadata.token_endpoint, &token_params).await);
    let refresh_token = refresh_token.unwrap();
    let (refresh_issued_at, refresh_expiry, refresh_expires_in) = match test
        .server
        .inner
        .build_server()
        .validate_access_token(Some(GrantType::RefreshToken), &refresh_token)
        .await
    {
        Ok(info) => (info.issued_at, info.expiry, info.expires_in),
        Err(err) => panic!(
            "Refresh metadata validation failed: event_type={}",
            err.event_type().to_id()
        ),
    };
    assert_eq!(
        refresh_expiry.checked_sub(refresh_issued_at),
        Some(REFRESH_TOKEN_LIFETIME),
        "issued_at={refresh_issued_at} expiry={refresh_expiry}"
    );
    // Upgrade ruling 74: 0.16 also required more than the removed
    // `refresh_token_renewal` window to remain (subject deleted).
    assert!(
        refresh_expires_in <= REFRESH_TOKEN_LIFETIME,
        "issued_at={refresh_issued_at} expiry={refresh_expiry} expires_in={refresh_expires_in}"
    );

    // Authorization codes can only be used once
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &token_params).await,
        TokenResponse::Error {
            error: ErrorType::ExpiredToken
        }
    );

    // Connect to account using token and attempt to search
    let john_client = Client::new()
        .credentials(Credentials::bearer(&token))
        .accept_invalid_certs(true)
        .follow_redirects(["127.0.0.1"])
        .connect("https://127.0.0.1:8899")
        .await
        .unwrap();
    assert_eq!(john_client.default_account_id(), user_id.to_string());
    assert!(
        !john_client
            .mailbox_query(None::<Filter>, None::<Vec<_>>)
            .await
            .unwrap()
            .ids()
            .is_empty()
    );

    // Connecting using the refresh token should not work
    assert_unauthorized("https://127.0.0.1:8899", &refresh_token).await;

    // Refreshing a token using the access token should not work
    assert_eq!(
        post::<TokenResponse>(
            &metadata.token_endpoint,
            &AHashMap::from_iter([
                ("client_id".to_string(), device_client_id.to_string()),
                ("grant_type".to_string(), "refresh_token".to_string()),
                ("refresh_token".to_string(), token),
            ]),
        )
        .await,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    );

    // Refresh the access token. Upgrade ruling 74: 0.16's renewal-window
    // checks (no new refresh token while more than `refresh_token_renewal`
    // remained, a new one inside the window) depended on the removed
    // setting (subject deleted); 1.0 rotates the refresh token on every
    // refresh.
    let refresh_params = AHashMap::from_iter([
        ("client_id".to_string(), device_client_id.to_string()),
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token),
    ]);
    let (token, _, _) =
        unwrap_token_response(post(&metadata.token_endpoint, &refresh_params).await);

    // The access token still has a one-second lifetime.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_unauthorized("https://127.0.0.1:8899", &token).await;

    // Reject the original refresh token at its recorded absolute expiry.
    wait_until_epoch(refresh_expiry).await;
    assert_eq!(
        post::<TokenResponse>(&metadata.token_endpoint, &refresh_params).await,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    );

    // Clean up
    admin.registry_destroy_all(ObjectType::OAuthClient).await;
    admin.destroy_account(user).await;
    test.cleanup().await;
}

fn set_access_token_expiry(test: &TestServer, secs: u64) {
    let mut core = (*test.server.inner.shared_core.load_full()).clone();
    core.oauth.oauth_expiry_token = secs;
    test.server
        .inner
        .shared_core
        .store(std::sync::Arc::new(core));
}

async fn wait_until_epoch(target: u64) -> u64 {
    // Bound wall-clock waits even if the clock stops advancing or moves backwards.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let now = store::write::now();
        if now >= target {
            return now;
        }
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for epoch: now={now} target={target}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn post_bytes(
    url: &str,
    auth_token: Option<&str>,
    params: &AHashMap<String, String>,
) -> Bytes {
    let mut client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .post(url);

    if let Some(auth_token) = auth_token {
        client = client.bearer_auth(auth_token);
    }

    client
        .form(params)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
}

async fn post_json<D: DeserializeOwned>(
    url: &str,
    auth_token: Option<&str>,
    body: &impl Serialize,
) -> D {
    let mut client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .post(url);

    if let Some(auth_token) = auth_token {
        client = client.bearer_auth(auth_token);
    }

    serde_json::from_slice(
        &client
            .body(serde_json::to_string(body).unwrap().into_bytes())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn post_json_basic<D: DeserializeOwned>(
    url: &str,
    username: &str,
    password: &str,
    body: &impl Serialize,
) -> D {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .post(url)
        .basic_auth(username, Some(password))
        .body(serde_json::to_string(body).unwrap().into_bytes())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    serde_json::from_slice(&response).unwrap()
}

async fn post_form_basic<T: DeserializeOwned>(
    url: &str,
    username: &str,
    password: &str,
    params: &AHashMap<String, String>,
) -> T {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .post(url)
        .basic_auth(username, Some(password))
        .form(params)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    serde_json::from_slice(&response).unwrap()
}

async fn obtain_auth_code(http: &HttpRequest, client_id: &str, redirect_uri: &str) -> String {
    http.post::<LoginResponse>(
        "/api/auth",
        &auth_code_request(AuthCodeQuery {
            client_id,
            redirect_uri,
            scope: Some(PROFILE_SCOPE),
            ..Default::default()
        }),
    )
    .await
    .unwrap()
    .unwrap_code()
}

async fn post_json_raw(url: &str, body: &impl Serialize) -> (u16, serde_json::Value) {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .post(url)
        .body(serde_json::to_string(body).unwrap().into_bytes())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let value =
        serde_json::from_slice(&response.bytes().await.unwrap()).unwrap_or(serde_json::Value::Null);
    (status, value)
}

async fn post_login_raw(body: &impl Serialize) -> (u16, serde_json::Value) {
    post_json_raw("https://127.0.0.1:8899/api/auth", body).await
}

async fn post<T: DeserializeOwned>(url: &str, params: &AHashMap<String, String>) -> T {
    post_with_auth(url, None, params).await
}
async fn post_with_auth<T: DeserializeOwned>(
    url: &str,
    auth_token: Option<&str>,
    params: &AHashMap<String, String>,
) -> T {
    serde_json::from_slice(&post_bytes(url, auth_token, params).await).unwrap()
}

async fn get_bytes(url: &str) -> Bytes {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default()
        .get(url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap()
}

async fn get<T: DeserializeOwned>(url: &str) -> T {
    serde_json::from_slice(&get_bytes(url).await).unwrap()
}

async fn assert_unauthorized(base_url: &str, token: &str) {
    match Client::new()
        .credentials(Credentials::bearer(token))
        .accept_invalid_certs(true)
        .follow_redirects(["127.0.0.1"])
        .connect(base_url)
        .await
    {
        Ok(_) => panic!("Expected unauthorized access."),
        Err(err) => {
            let err = err.to_string();
            assert!(err.contains("Unauthorized"), "{}", err);
        }
    }
}

#[track_caller]
fn unwrap_token_response(response: TokenResponse) -> (String, Option<String>, u64) {
    match response {
        TokenResponse::Granted(granted) => {
            assert_eq!(granted.token_type, "bearer");
            (
                granted.access_token,
                granted.refresh_token,
                granted.expires_in,
            )
        }
        TokenResponse::Error { error } => panic!("Expected granted, got {:?}", error),
    }
}

#[track_caller]
fn unwrap_oidc_token_response(response: TokenResponse) -> (String, Option<String>, String) {
    match response {
        TokenResponse::Granted(granted) => {
            assert_eq!(granted.token_type, "bearer");
            (
                granted.access_token,
                granted.refresh_token,
                granted.id_token.unwrap(),
            )
        }
        TokenResponse::Error { error } => panic!("Expected granted, got {:?}", error),
    }
}

/// The parameters of an authorization request (RFC 6749 4.1.1, RFC 7636
/// 4.3, RFC 8707 2). 1.0's login API takes them as the authorization
/// endpoint's query string, where 0.16 took them as fields.
#[derive(Default)]
struct AuthCodeQuery<'x> {
    client_id: &'x str,
    redirect_uri: &'x str,
    nonce: Option<&'x str>,
    scope: Option<&'x str>,
    /// An S256 challenge.
    code_challenge: Option<&'x str>,
    state: Option<&'x str>,
    resources: &'x [&'x str],
}

/// The account's login for `query`, with consent given in the same request
/// (`consent: true`, as the login page submits it).
fn auth_code_request(query: AuthCodeQuery<'_>) -> LoginRequest {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer
        .append_pair("response_type", "code")
        .append_pair("client_id", query.client_id)
        .append_pair("redirect_uri", query.redirect_uri);
    if let Some(nonce) = query.nonce {
        serializer.append_pair("nonce", nonce);
    }
    if let Some(scope) = query.scope {
        serializer.append_pair("scope", scope);
    }
    if let Some(code_challenge) = query.code_challenge {
        serializer
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
    }
    // 1.0 refuses an authorization request without `state`
    // (`invalid_request`, "The state parameter is missing."); a request that
    // names none sends a fixed one.
    serializer.append_pair("state", query.state.unwrap_or("oidc-test-state"));
    for resource in query.resources {
        serializer.append_pair("resource", resource);
    }
    LoginRequest::AuthCode {
        account_name: "user@example.org".to_string(),
        account_secret: "this is a very strong password".to_string(),
        mfa_token: None,
        query: serializer.finish(),
        consent: Some(true),
    }
}

/// Whether a raw login response issued an authorization code: 1.0 answers
/// every login with 200 and a typed body, so a refusal is any other answer.
fn is_authorized(status: u16, body: &serde_json::Value) -> bool {
    status == 200 && body["type"] == "authenticated"
}

pub trait LoginResponseTest {
    fn unwrap_code(self) -> String;
    fn location_param(&self, name: &str) -> Option<String>;
}

impl LoginResponseTest for LoginResponse {
    fn unwrap_code(self) -> String {
        self.location_param("code")
            .unwrap_or_else(|| panic!("Expected auth code response, got {:?}", self))
    }

    /// A query parameter of an authenticated response's redirect location.
    fn location_param(&self, name: &str) -> Option<String> {
        match self {
            LoginResponse::Authenticated { location } => {
                let (_, query) = location.split_once('?')?;
                form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
            }
            _ => None,
        }
    }
}

const OIDC_SIGNATURE_KEY_RS256: &str = "-----BEGIN PRIVATE KEY-----
MIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQDMXJI1bL3z8gaF
Ze/6493VjL+jHkFMP2Pc7fLwRF1fhkuIdYTp69LabzrSEJCRCz0UI2NHqPOgtOta
+zRHKAMr7c7Z6uKO0K+aXiQYHw4Y70uSG8CnmNl7kb4OM/CAcoO6fePmvBsyESfn
TmkJ5bfHEZQFDQEAoDlDjtjxuwYsAQQVQXuAydi8j8pyTWKAJ1RDgnUT+HbOub7j
JrQ7sPe6MPCjXv5N76v9RMHKktfYwRNMlkLkxImQU55+vlvghNztgFlIlJDFfNiy
UQPV5FTEZJli9BzMoj1JQK3sZyV8WV0W1zN41QQ+glAAC6+K7iTDPRMINBSwbHyn
6Lb9Q6U7AgMBAAECggEAB93qZ5xrhYgEFeoyKO4mUdGsu4qZyJB0zNeWGgdaXCfZ
zC4l8zFM+R6osix0EY6lXRtC95+6h9hfFQNa5FWseupDzmIQiEnim1EowjWef87l
Eayi0nDRB8TjqZKjR/aLOUhzrPlXHKrKEUk/RDkacCiDklwz9S0LIfLOSXlByBDM
/n/eczfX2gUATexMHSeIXs8vN2jpuiVv0r+FPXcRvqdzDZnYSzS8BJ9k6RYXVQ4o
NzCbfqgFIpVryB7nHgSTrNX9G7299If8/dXmesXWSFEJvvDSSpcBoINKbfgSlrxd
6ubjiotcEIBUSlbaanRrydwShhLHnXyupNAb7tlvyQKBgQDsIipSK4+H9FGl1rAk
Gg9DLJ7P/94sidhoq1KYnj/CxwGLoRq22khZEUYZkSvYXDu1Qkj9Avi3TRhw8uol
l2SK1VylL5FQvTLKhWB7b2hjrUd5llMRgS3/NIdLhOgDMB7w3UxJnCA/df/Rj+dM
WhkyS1f0x3t7XPLwWGurW0nJcwKBgQDdjhrNfabrK7OQvDpAvNJizuwZK9WUL7CD
rR0V0MpDGYW12BTEOY6tUK6XZgiRitAXf4EkEI6R0Q0bFzwDDLrg7TvGdTuzNeg/
8vm8IlRlOkrdihtHZI4uRB7Ytmz24vzywEBE0p6enA7v4oniscUks/KKmDGr0V90
yT9gIVrjGQKBgQCjnWC5otlHGLDiOgm+WhgtMWOxN9dYAQNkMyF+Alinu4CEoVKD
VGhA3sk1ufMpbW8pvw4X0dFIITFIQeift3DBCemxw23rBc2FqjkaDi3EszINO22/
eUTHyjvcxfCFFPi7aHsNnhJyJm7lY9Kegudmg/Ij93zGE7d5darVBuHvpQKBgBBY
YovUgFMLR1UfPeD2zUKy52I4BKrJFemxBNtOKw3mPSIcTfPoFymcMTVENs+eARoq
svlZK1uAo8ni3e+Pqd3cQrOyhHQFPxwwrdH+amGJemp7vOV4erDZH7l3Q/S27Fhw
bI1nSIKFGukBupB58wRxLiyha9C0QqmYC0/pRg5JAn8Rbj5tP26oVCXjZEfWJL8J
axxSxsGA4Vol6i6LYnVgZG+1ez2rP8vUORo1lRzmdeP4o1BSJf9TPwXkuppE5J+t
UZVKtYGlEn1RqwGNd8I9TiWvU84rcY9nsxlDR86xwKRWFvYqVOiGYtzRyewYRdjU
rTs9aqB3v1+OVxGxR6Na
-----END PRIVATE KEY-----
";

#[allow(dead_code)]
const OIDC_SIGNATURE_KEY_ES256: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQggybcqc86ulFFiOon
WiYrLO4z8/kmkqvA7wGElBok9IqhRANCAAQxZK68FnQtHC0eyh8CA05xRIvxhVHn
0ymka6XBh9aFtW4wfeoKhTkSKjHc/zjh9Rr2dr3kvmYe80fMGhW4ycGA
-----END PRIVATE KEY-----
";
