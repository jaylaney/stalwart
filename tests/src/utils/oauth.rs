/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    account::Account, jmap::JmapResponse, server::TestServer, tls::fixture_https_client_builder,
    webdav::DummyWebDavClient, za::SERVER_URL,
};
use common::{
    BuildServer, Core, Inner, KV_OAUTH,
    auth::{
        oauth::{
            registration::{ClientRegistrationRequest, ClientRegistrationResponse},
            scope::OAuthScopes,
        },
        vault::RawValue,
    },
    za::tokens::{ZaIssued, ZaTokenIssue},
};
use http::auth::oauth::{
    TokenResponse,
    auth::{LoginRequest, LoginResponse},
};
use registry::schema::{
    enums::{OAuthGrantType, OAuthScope, TaskStoreMaintenanceType},
    prelude::ObjectType,
    structs::{Application, OAuthClient, Task, TaskStatus, TaskStoreMaintenance},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use store::{
    ValueKey,
    dispatch::lookup::KeyValue,
    registry::bootstrap::Bootstrap,
    write::{BatchBuilder, now},
};
use types::{collection::Collection, field::PrincipalField, id::Id};
use vault::{
    record::za_session_binding,
    tokens::{CODE_LEN, SECRET_SEPARATOR, ZaTokenRecord, append_secret},
};

const PKCE_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
pub const ZA_SIGNIN_SCOPE: &str =
    "urn:ietf:params:oauth:scope:calendars urn:ietf:params:oauth:scope:mail offline_access";

pub enum ZaClientKind {
    Registered,
    Application,
    #[allow(dead_code)] // Used by the later stateless sign-in cases.
    Stateless,
}

pub struct ZaOAuthClient {
    pub client_id: String,
    pub redirect_uri: String,
    ids: Vec<Id>,
}

pub struct ZaTokens {
    pub access: String,
    pub refresh: String,
    pub grant_id: u64,
    pub expires_in: u64,
}

pub async fn za_oauth_client(
    test: &TestServer,
    admin: &Account,
    client_id: &str,
    kind: ZaClientKind,
) -> ZaOAuthClient {
    let redirect_uri = format!("https://example.org/{client_id}/");
    if matches!(kind, ZaClientKind::Stateless) {
        let registration: ClientRegistrationResponse = oauth_json(
            bearer_client()
                .post(format!("{SERVER_URL}/auth/register"))
                .json(&ClientRegistrationRequest {
                    redirect_uris: vec![redirect_uri.clone()],
                    scope: Some(ZA_SIGNIN_SCOPE.to_owned()),
                    grant_types: vec!["authorization_code".into(), "refresh_token".into()],
                    ..Default::default()
                }),
        )
        .await;
        return ZaOAuthClient {
            client_id: registration.client_id,
            redirect_uri,
            ids: Vec::new(),
        };
    }

    let mut ids = vec![
        admin
            .registry_create_object(OAuthClient {
                client_id: client_id.to_owned(),
                redirect_uris: vec![redirect_uri.clone()].into(),
                scopes: vec![OAuthScope::Calendars, OAuthScope::Mail].into(),
                grant_types: vec![
                    OAuthGrantType::AuthorizationCode,
                    OAuthGrantType::RefreshToken,
                    OAuthGrantType::DeviceCode,
                ]
                .into(),
                ..Default::default()
            })
            .await,
    ];
    if matches!(kind, ZaClientKind::Application) {
        ids.push(
            admin
                .registry_create_object(Application {
                    enable: true,
                    description: "Zero-access OAuth test application".to_owned(),
                    resource_url: "file:///unused-za-token-app.zip".to_owned(),
                    url_prefix: vec!["/account".to_owned()].into(),
                    oauth_client_id: Some(client_id.to_owned()),
                    ..Default::default()
                })
                .await,
        );
        reload_applications(test).await;
        assert!(
            test.server
                .inner
                .data
                .applications
                .is_application_client_id(client_id)
        );
    }
    ZaOAuthClient {
        client_id: client_id.to_owned(),
        redirect_uri,
        ids,
    }
}

pub async fn za_drop_oauth_client(test: &TestServer, admin: &Account, client: ZaOAuthClient) {
    if let Some(&id) = client.ids.get(1) {
        admin
            .registry_destroy(ObjectType::Application, [id])
            .await
            .assert_destroyed(&[id]);
        reload_applications(test).await;
        assert!(
            !test
                .server
                .inner
                .data
                .applications
                .is_application_client_id(&client.client_id)
        );
    }
    if let Some(&id) = client.ids.first() {
        admin
            .registry_destroy(ObjectType::OAuthClient, [id])
            .await
            .assert_destroyed(&[id]);
    }
}

async fn reload_applications(test: &TestServer) {
    let mut bootstrap = Bootstrap::new_uninitialized(test.server.registry().clone());
    test.server
        .inner
        .data
        .applications
        .reload(&mut bootstrap)
        .await;
    assert!(bootstrap.errors.is_empty());
}

fn signin_query(client: &ZaOAuthClient) -> String {
    let mut url = reqwest::Url::parse("https://example.org/").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", client.redirect_uri.as_str()),
        ("scope", ZA_SIGNIN_SCOPE),
        ("code_challenge", PKCE_CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", "za-token-sign-in"),
    ]);
    url.query().unwrap().to_owned()
}

pub async fn za_authorize(
    client: &ZaOAuthClient,
    name: &str,
    secret: &str,
    mfa: Option<&str>,
    consent: Option<bool>,
    from_ip: Option<&str>,
) -> LoginResponse {
    let mut request =
        bearer_client()
            .post(format!("{SERVER_URL}/api/auth"))
            .json(&LoginRequest::AuthCode {
                account_name: name.to_owned(),
                account_secret: secret.to_owned(),
                mfa_token: mfa.map(str::to_owned),
                query: signin_query(client),
                consent,
            });
    if let Some(from_ip) = from_ip {
        request = request.header("X-Forwarded-For", from_ip);
    }
    oauth_json(request).await
}

pub async fn za_confirm_consent(
    client: &ZaOAuthClient,
    consent_token: &str,
    consent: bool,
) -> LoginResponse {
    oauth_json(bearer_client().post(format!("{SERVER_URL}/api/auth")).json(
        &LoginRequest::AuthConsent {
            token: consent_token.to_owned(),
            query: signin_query(client),
            consent,
        },
    ))
    .await
}

pub async fn za_exchange(client: &ZaOAuthClient, code: &str) -> TokenResponse {
    oauth_json(
        bearer_client()
            .post(format!("{SERVER_URL}/auth/token"))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client.client_id.as_str()),
                ("redirect_uri", client.redirect_uri.as_str()),
                ("code", code),
                ("code_verifier", PKCE_VERIFIER),
            ]),
    )
    .await
}

pub async fn za_refresh(client: &ZaOAuthClient, refresh: &str) -> TokenResponse {
    oauth_json(
        bearer_client()
            .post(format!("{SERVER_URL}/auth/token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client.client_id.as_str()),
                ("refresh_token", refresh),
            ]),
    )
    .await
}

pub async fn za_revoke_token(client: &ZaOAuthClient, token: &str) -> u16 {
    bearer_client()
        .post(format!("{SERVER_URL}/auth/revoke"))
        .form(&[("client_id", client.client_id.as_str()), ("token", token)])
        .send()
        .await
        .unwrap_or_else(|_| panic!("OAuth revocation request failed"))
        .status()
        .as_u16()
}

pub async fn za_sign_in(
    test: &TestServer,
    client: &ZaOAuthClient,
    name: &str,
    secret: &str,
    mfa: Option<&str>,
) -> ZaTokens {
    let LoginResponse::Authenticated { location } =
        za_authorize(client, name, secret, mfa, Some(true), None).await
    else {
        panic!("OAuth sign-in did not authorize a code");
    };
    let redirect = reqwest::Url::parse(&location).expect("OAuth redirect is a URL");
    let (_, code) = redirect
        .query_pairs()
        .find(|(key, _)| key == "code")
        .expect("OAuth redirect has a code");
    let TokenResponse::Granted(response) = za_exchange(client, &code).await else {
        panic!("OAuth sign-in did not grant tokens");
    };
    let grant = test
        .server
        .decode_access_token(None, &response.access_token)
        .unwrap_or_else(|_| panic!("could not decode the OAuth access token"))
        .grant
        .expect("access token has an OAuth grant");
    ZaTokens {
        access: response.access_token,
        refresh: response
            .refresh_token
            .expect("sign-in issued a refresh token"),
        grant_id: grant.id,
        expires_in: response.expires_in,
    }
}

async fn oauth_json<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> T {
    request
        .send()
        .await
        .unwrap_or_else(|_| panic!("OAuth request failed"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("OAuth response is missing or invalid"))
}

pub async fn handoff_exists(test: &TestServer, code_or_consent: &str) -> bool {
    let code = code_or_consent.get(..CODE_LEN).expect("OAuth code prefix");
    test.server
        .in_memory_store()
        .key_exists(KeyValue::<()>::build_key(KV_OAUTH, format!("~{code}")))
        .await
        .unwrap()
}

pub async fn run_purge_maintenance(test: &TestServer, admin: &Account) {
    admin
        .registry_create_object(Task::StoreMaintenance(TaskStoreMaintenance {
            maintenance_type: TaskStoreMaintenanceType::PurgeData,
            shard_index: None,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
}

#[must_use]
pub struct OAuthExpiryGuard {
    inner: Arc<Inner>,
    previous: Arc<Core>,
}

pub fn oauth_expiry(
    test: &TestServer,
    access: Option<u64>,
    refresh: Option<u64>,
    code: Option<u64>,
) -> OAuthExpiryGuard {
    let inner = test.server.inner.clone();
    let previous = inner.shared_core.load_full();
    let mut core = (*previous).clone();
    if let Some(secs) = access {
        core.oauth.oauth_expiry_token = secs;
    }
    if let Some(secs) = refresh {
        core.oauth.oauth_expiry_refresh_token = secs;
    }
    if let Some(secs) = code {
        core.oauth.oauth_expiry_auth_code = secs;
    }
    inner.shared_core.store(Arc::new(core));
    OAuthExpiryGuard { inner, previous }
}

impl Drop for OAuthExpiryGuard {
    fn drop(&mut self) {
        let mut core = (*self.inner.shared_core.load_full()).clone();
        core.oauth.oauth_expiry_token = self.previous.oauth.oauth_expiry_token;
        core.oauth.oauth_expiry_refresh_token = self.previous.oauth.oauth_expiry_refresh_token;
        core.oauth.oauth_expiry_auth_code = self.previous.oauth.oauth_expiry_auth_code;
        self.inner.shared_core.store(Arc::new(core));
    }
}

pub(crate) const CALENDAR_JMAP_USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:calendars:parse",
    "urn:ietf:params:jmap:principals",
    "urn:ietf:params:jmap:mail",
];

pub fn za_split_token(token: &str) -> (&str, Option<&str>) {
    token
        .split_once(SECRET_SEPARATOR)
        .map_or((token, None), |(upstream, secret)| (upstream, Some(secret)))
}

pub fn za_tamper_secret(token: &str) -> String {
    let (upstream, secret) = za_split_token(token);
    let secret = secret.expect("expected a token secret");
    let first = if secret.starts_with('A') { 'B' } else { 'A' };
    format!("{upstream}{SECRET_SEPARATOR}{first}{}", &secret[1..])
}

pub async fn za_bearer_jmap(token: &str, using: &[&str], calls: Value) -> (u16, JmapResponse) {
    let (status, body) = bearer_json(
        bearer_client()
            .post(format!("{SERVER_URL}/jmap"))
            .bearer_auth(token)
            .json(&json!({ "using": using, "methodCalls": calls })),
    )
    .await;
    (status, JmapResponse(body))
}

pub async fn za_bearer_session(token: &str) -> (u16, Value) {
    bearer_json(
        bearer_client()
            .get(format!("{SERVER_URL}/jmap/session"))
            .bearer_auth(token),
    )
    .await
}

pub fn za_bearer_dav(account: &Account, token: &str) -> DummyWebDavClient {
    let mut client = DummyWebDavClient::new(
        account.id().document_id(),
        account.name(),
        account.secret(),
        account.name(),
    );
    client.credentials = format!("Bearer {token}");
    client
}

fn bearer_client() -> reqwest::Client {
    fixture_https_client_builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

/// A keyed access token of `name` (account `account_id`): a grant restricted
/// to `scopes` (`za_grant_token`), the keys unlocked with `password`, and the
/// grant's record entry issued as the keyed code exchange issues it (an hour
/// for the access token, thirty days for the refresh token), its secret
/// appended to the token. The token is sealed after the entry is issued, by
/// a server built from the current configuration, so an `oauth_expiry` guard
/// sets its signed expiry and its lifetime starts after the slow steps.
/// Returns the grant id, the token and the entry.
pub async fn za_keyed_grant(
    test: &TestServer,
    name: &str,
    account_id: u32,
    password: &str,
    scopes: OAuthScopes,
) -> (u64, String, ZaIssued) {
    let server = test.server.inner.build_server();
    let (grant, _) = super::za::za_grant_token(&server, name, scopes).await;
    let keys = super::za::za_unlock(test, account_id, password).await;
    let vault = test
        .server
        .za_vault_record(account_id)
        .await
        .unwrap()
        .unwrap();
    let ZaTokenIssue::Issued(issued) = test
        .server
        .za_token_issue(
            account_id,
            grant,
            keys.mk(),
            za_session_binding(&vault.record),
            now() + 3_600,
            now() + 2_592_000,
        )
        .await
        .unwrap()
    else {
        panic!("the grant was not issued");
    };
    let credential_version = server
        .access_token(account_id)
        .await
        .unwrap()
        .credential_version();
    let mut token = server
        .encode_grant_access_token(account_id, name, grant, credential_version)
        .unwrap();
    append_secret(&mut token, &issued.access);
    (grant, token, issued)
}

async fn bearer_json(request: reqwest::RequestBuilder) -> (u16, Value) {
    let response = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("Bearer request failed"));
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("Bearer response read failed"));
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// The stored bytes of the account's token record (field 153).
pub async fn raw_token_record(test: &TestServer, account_id: u32) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<RawValue>(ValueKey::property(
            account_id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessTokens,
        ))
        .await
        .unwrap()
        .map(|raw| raw.0)
}

/// An unconditional set of field 153 to `raw`, or its removal with `None`.
pub async fn set_raw_token_record(test: &TestServer, account_id: u32, raw: Option<&[u8]>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    match raw {
        Some(raw) => batch.set(PrincipalField::ZeroAccessTokens, raw.to_vec()),
        None => batch.clear(PrincipalField::ZeroAccessTokens),
    };
    test.server.store().write_batch(&mut batch).await.unwrap();
}

pub async fn token_record(test: &TestServer, account_id: u32) -> Option<ZaTokenRecord> {
    test.server.za_token_read(account_id).await.unwrap().record
}
