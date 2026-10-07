/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access account API (spec 4.1). Every state change is one conditional
//! write of the vault record; the registry is written only for the marker and
//! for app-password hashes.

use crate::auth::authenticate::Authenticator;
use ::vault::{
    ZA_MARKER,
    keys::{
        Argon2Params, Secret, aad, app_aad, derive_app_kek, derive_kek, derive_recovery_kek,
        derive_verifier_hash, generate_keypair, unwrap_key, wrap_key,
    },
    record::{AppWrap, VaultRecord, VaultState, WrapState},
    recovery::RecoveryKey,
    session::SessionKeys,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{
    Server,
    auth::{
        AuthRequest,
        credential::AppPassword,
        vault::{VaultRead, za_derive_root},
    },
};
use directory::{
    Credentials,
    core::secret::{hash_secret, verify_totp_code},
};
use http_proto::{HttpRequest, HttpResponse, HttpSessionData};
use hyper::{
    StatusCode,
    header::{self, HeaderValue},
};
use registry::{
    schema::{
        enums::{Permission, StorageQuota},
        prelude::{Object, ObjectInner, ObjectType},
        structs::{Account, Credential, PasswordCredential, SecondaryCredential, UserAccount},
    },
    types::{datetime::UTCDateTime, id::ObjectId},
};
use sha2::{Digest, Sha256};
use std::{future::Future, net::IpAddr, sync::Arc};
use store::{
    registry::write::{RegistryWrite, RegistryWriteResult},
    write::now,
};
use subtle::ConstantTimeEq;
use trc::AddContext;
use types::{collection::Collection, id::Id};

/// Lifetime of a setup token (spec 4.1).
pub const SETUP_TOKEN_TTL_SECS: i64 = 7 * 86400;

/// Longest app-password description, in bytes (spec 10).
pub const MAX_APP_PASSWORD_DESCRIPTION: usize = 255;

/// Longest `otp_auth` URL accepted by `totp`, in bytes. Checked before parsing.
pub const MAX_OTP_AUTH_URL: usize = 1024;

/// Attempts at publishing a Pending app-password wrap before giving up.
const PUBLISH_RETRIES: usize = 5;

#[derive(serde::Deserialize)]
struct SetupTokenRequest {
    account: String,
}

#[derive(serde::Serialize)]
struct SetupTokenResponse {
    token: String,
    expires: i64,
}

#[derive(serde::Deserialize)]
struct SetupRequest {
    username: String,
    token: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct PasswordRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    new_password: String,
}

#[derive(serde::Deserialize)]
struct RecoverRequest {
    username: String,
    recovery_key: String,
    new_password: String,
}

#[derive(serde::Deserialize)]
struct CredentialsRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(serde::Deserialize)]
struct AppPasswordRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    description: String,
}

#[derive(serde::Serialize)]
struct AppPasswordResponse {
    app_password: String,
    credential_id: u32,
}

#[derive(serde::Deserialize)]
struct RevokeRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    credential_id: u32,
}

#[derive(serde::Deserialize)]
struct TotpRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    /// Required: absent `None`, `null` `Some(None)` (removes TOTP), a URL
    /// `Some(Some(url))` (enrols or replaces).
    #[serde(default, deserialize_with = "deserialize_some")]
    otp_auth: Option<Option<String>>,
}

/// Double option: a present field, `null` included, becomes `Some`.
fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(serde::Serialize)]
struct RecoveryKeyResponse {
    recovery_key: String,
}

#[derive(serde::Serialize)]
struct ErrorResponse {
    error: &'static str,
}

#[derive(serde::Serialize)]
struct OkResponse {
    ok: bool,
}

fn ok() -> HttpResponse {
    json(OkResponse { ok: true })
}

fn json<T: serde::Serialize>(value: T) -> HttpResponse {
    json_with_status(StatusCode::OK, value)
}

/// Responses carry tokens and recovery keys. A binary body keeps them out of
/// the `HttpEvent::ResponseBody` trace, which records text bodies verbatim.
fn json_with_status<T: serde::Serialize>(status: StatusCode, value: T) -> HttpResponse {
    HttpResponse::new(status)
        .with_content_type("application/json; charset=utf-8")
        .with_binary_body(serde_json::to_vec(&value).unwrap_or_default())
        .with_no_store()
}

/// 409: wrong state, ineligible account, or a lost revision check (spec 10).
fn conflict(error: &'static str) -> HttpResponse {
    json_with_status(StatusCode::CONFLICT, ErrorResponse { error })
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn bad_request(details: impl Into<String>) -> trc::Error {
    trc::ResourceEvent::BadParameters
        .into_err()
        .details(details.into())
}

/// The request body carries passwords, tokens and recovery keys, and serde's
/// messages can quote input values, so only the error category and position
/// are reported.
fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> trc::Result<T> {
    serde_json::from_slice::<T>(body).map_err(|err| {
        bad_request(format!(
            "Invalid JSON request ({:?} error at line {}, column {}).",
            err.classify(),
            err.line(),
            err.column()
        ))
    })
}

/// Argon2 parameters for every new password. Stored in the record, so test
/// builds use cheap parameters without affecting production records.
fn za_argon2_params() -> Argon2Params {
    #[cfg(feature = "test_mode")]
    {
        Argon2Params {
            m_cost_kib: 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }
    #[cfg(not(feature = "test_mode"))]
    {
        Argon2Params::default()
    }
}

/// CORS preflight on `/api/vault/*` (spec 4.1). Without a configured
/// account-page origin this is upstream's bare 204.
pub fn za_cors_preflight(origin: Option<&HeaderValue>) -> HttpResponse {
    let response = HttpResponse::new(StatusCode::NO_CONTENT);
    if origin.is_none() {
        return response;
    }
    za_with_cors(response, origin)
        .with_header(header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS")
        .with_header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Content-Type, Authorization",
        )
        .with_header(header::ACCESS_CONTROL_MAX_AGE, "600")
}

/// Every `/api/vault/*` response allows the configured account-page origin
/// (spec 4.1); without one the response is unchanged.
pub fn za_with_cors(response: HttpResponse, origin: Option<&HeaderValue>) -> HttpResponse {
    match origin {
        Some(origin) => response
            .with_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone())
            .with_header(header::VARY, "Origin"),
        None => response,
    }
}

pub trait VaultApi: Sync + Send {
    fn handle_vault_request(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
        endpoint: &str,
        sub: Option<&str>,
        body: Vec<u8>,
    ) -> impl Future<Output = trc::Result<HttpResponse>> + Send;
}

impl VaultApi for Server {
    async fn handle_vault_request(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
        endpoint: &str,
        sub: Option<&str>,
        body: Vec<u8>,
    ) -> trc::Result<HttpResponse> {
        if endpoint == "setup-token" && sub.is_none() {
            let (_in_flight, access_token) = self.authenticate_headers(req, session).await?;
            access_token.enforce_permission(Permission::SysAccountUpdate)?;
            return za_setup_token(self, access_token.tenant_id(), parse(&body)?).await;
        }

        // Credentials travel in the body; rate-limit like the other anonymous endpoints.
        self.is_http_anonymous_request_allowed(session.remote_ip)
            .await?;

        match (endpoint, sub) {
            ("setup", None) => za_setup(self, session, parse(&body)?).await,
            ("password", None) => za_password(self, session, parse(&body)?).await,
            ("recover", None) => za_recover(self, session, parse(&body)?).await,
            ("recovery-key", None) => za_recovery_key(self, session, parse(&body)?).await,
            ("app-password", None) => za_app_password(self, session, parse(&body)?).await,
            ("app-password", Some("revoke")) => {
                za_app_password_revoke(self, session, parse(&body)?).await
            }
            ("totp", None) => za_totp(self, session, parse(&body)?).await,
            _ => Err(trc::ResourceEvent::NotFound.into_err()),
        }
    }
}

struct RegistryAccount {
    account: UserAccount,
    revision: u64,
}

async fn za_registry_account(
    server: &Server,
    account_id: u32,
) -> trc::Result<Option<RegistryAccount>> {
    let Some(object) = server
        .registry()
        .get(ObjectId::new(ObjectType::Account, Id::from(account_id)))
        .await?
    else {
        return Ok(None);
    };
    let revision = object.revision;
    match object.inner {
        ObjectInner::Account(Account::User(account)) => {
            Ok(Some(RegistryAccount { account, revision }))
        }
        _ => Ok(None),
    }
}

/// Names the rejection without its payload: validation errors can quote
/// credential values.
fn registry_rejection(result: &RegistryWriteResult) -> &'static str {
    match result {
        RegistryWriteResult::Success(_) => "success",
        RegistryWriteResult::CannotDeleteLinked { .. } => "cannot delete linked",
        RegistryWriteResult::InvalidSingletonId => "invalid singleton id",
        RegistryWriteResult::CannotDeleteSingleton => "cannot delete singleton",
        RegistryWriteResult::NotFound { .. } => "not found",
        RegistryWriteResult::InvalidForeignKey { .. } => "invalid foreign key",
        RegistryWriteResult::PrimaryKeyConflict { .. } => "primary key conflict",
        RegistryWriteResult::ValidationError { .. } => "validation error",
        RegistryWriteResult::NotSupported => "not supported",
    }
}

/// Direct registry write (bypasses the JMAP mapping validators, which is
/// how the `$za$` marker gets stored). Returns false when the write was
/// rejected or lost a revision race.
async fn za_registry_update(
    server: &Server,
    account_id: u32,
    old: &RegistryAccount,
    new: UserAccount,
) -> trc::Result<bool> {
    let object = Object::new(ObjectInner::Account(Account::User(new)));
    let old_object = Object::with_revision(
        ObjectInner::Account(Account::User(old.account.clone())),
        old.revision,
    );
    match server
        .registry()
        .write(RegistryWrite::Update {
            object: &object,
            id: Id::from(account_id),
            old_object: &old_object,
        })
        .await
    {
        Ok(RegistryWriteResult::Success(_)) => Ok(true),
        Ok(other) => {
            trc::event!(
                Auth(trc::AuthEvent::Error),
                AccountId = account_id,
                Reason = "Zero-access registry write rejected",
                Details = registry_rejection(&other),
            );
            Ok(false)
        }
        Err(err) if err.is_assertion_failure() => Ok(false),
        Err(err) => Err(err),
    }
}

async fn za_delete_registry_credential(
    server: &Server,
    account_id: u32,
    credential_id: u32,
) -> trc::Result<bool> {
    let Some(reg) = za_registry_account(server, account_id).await? else {
        return Ok(false);
    };
    let mut account = reg.account.clone();
    let credentials = &mut account.credentials.inner_mut().inner;
    let before = credentials.len();
    credentials.retain(|c| c.value.credential_id().document_id() != credential_id);
    if credentials.len() == before {
        return Ok(true);
    }
    za_registry_update(server, account_id, &reg, account).await
}

fn za_registry_secondary_ids(account: &UserAccount) -> Vec<u32> {
    account
        .credentials
        .values()
        .filter_map(|c| {
            c.as_secondary_credential()
                .map(|s| s.credential_id.document_id())
        })
        .collect()
}

/// Orphan pruning runs on every vault write (spec 4.1).
async fn za_prune(
    server: &Server,
    record: &mut VaultRecord,
    account_id: u32,
    now: i64,
) -> trc::Result<()> {
    if let Some(reg) = za_registry_account(server, account_id).await? {
        record.prune_orphans(&za_registry_secondary_ids(&reg.account), now);
    }
    Ok(())
}

/// The only vault write path of this module: prunes orphans, sets the next
/// revision, writes conditionally on `previous` (absence when `None`) and
/// invalidates cached classification and authentication. A write on top of a
/// state other than the verified generation (`expected_revision`, invariant
/// 8) and a lost race are both 409.
async fn za_commit(
    server: &Server,
    account_id: u32,
    record: &mut VaultRecord,
    previous: Option<&VaultRead>,
    expected_revision: Option<u64>,
    now: i64,
) -> trc::Result<Result<(), HttpResponse>> {
    if let Some(expected) = expected_revision
        && previous.map(|p| p.record.revision) != Some(expected)
    {
        return Ok(Err(conflict("vault changed since verification")));
    }
    za_prune(server, record, account_id, now).await?;
    record.revision = previous.map_or(1, |p| p.record.revision + 1);
    match server.za_vault_write(account_id, record, previous).await {
        Ok(()) => (),
        Err(err) if err.is_assertion_failure() => return Ok(Err(conflict("conflict"))),
        Err(err) => return Err(err),
    }
    za_invalidate_after_commit(server, account_id).await;
    Ok(Ok(()))
}

/// Invalidation after a committed write: a failure is logged, never turned
/// into an error response, so a committed `setup` still returns its one-time
/// recovery key.
async fn za_invalidate_after_commit(server: &Server, account_id: u32) {
    if let Err(err) = server.za_invalidate_account(account_id).await {
        trc::error!(
            err.account_id(account_id)
                .details("Zero-access cache invalidation failed after a committed write")
        );
    }
}

/// Refusal with a status other than 401 (e.g. 409 on an active account):
/// counted like a failed attempt, so probing for account states is delayed,
/// rate-limited and banned like guessing credentials. A ban or a fail2ban
/// lookup error replaces `response`.
async fn za_refuse(
    server: &Server,
    remote_ip: IpAddr,
    username: &str,
    response: HttpResponse,
) -> trc::Result<HttpResponse> {
    let err = za_auth_failure(server, remote_ip, username).await;
    if err.matches(trc::EventType::Auth(trc::AuthEvent::Failed)) {
        Ok(response)
    } else {
        Err(err)
    }
}

/// Failure path for endpoints that verify something other than the primary
/// password: same delay and fail2ban accounting as `authenticate`.
async fn za_auth_failure(server: &Server, remote_ip: IpAddr, username: &str) -> trc::Error {
    server
        .authentication_failure(
            trc::AuthEvent::Failed
                .into_err()
                .ctx(trc::Key::AccountName, username.to_string())
                .details("Zero-access verification failed"),
            remote_ip,
            Some(username),
        )
        .await
}

struct ZaVerified {
    account_id: u32,
    keys: Arc<SessionKeys>,
}

/// Fresh, full verification of the primary password and TOTP (spec 4.1).
/// Cached CalDAV authentication is never consulted; app passwords and
/// master logins are refused with 400.
async fn za_verify_primary(
    server: &Server,
    session: &HttpSessionData,
    username: &str,
    password: &str,
    totp: Option<String>,
) -> trc::Result<Result<ZaVerified, HttpResponse>> {
    if username.contains('%') {
        return Err(bad_request(
            "Master-user logins are not accepted on this endpoint.",
        ));
    }
    if AppPassword::parse(password).is_some() {
        return Err(bad_request(
            "App passwords are not accepted on this endpoint.",
        ));
    }
    let (token, keys) = server
        .authenticate_with_keys(&AuthRequest::from_credentials(
            Credentials::Basic {
                username: username.to_string(),
                secret: password.to_string(),
                mfa_token: totp.filter(|t| !t.is_empty()),
            },
            session.session_id,
            session.remote_ip,
        ))
        .await?;
    match keys {
        Some(keys) if keys.account_id == token.account_id() => Ok(Ok(ZaVerified {
            account_id: token.account_id(),
            keys,
        })),
        _ => Ok(Err(conflict("not a zero-access account"))),
    }
}

/// Second record read after `za_verify_primary`: the `previous` of the
/// commit, which checks it against the verified generation. Missing record
/// or a state other than Active is 409.
async fn za_reread_verified(
    server: &Server,
    verified: &ZaVerified,
) -> trc::Result<Result<VaultRead, HttpResponse>> {
    #[cfg(feature = "test_mode")]
    crate::auth::authenticate::za_test::endpoint_pause_point(verified.account_id).await;

    let Some(read) = server.za_vault_record(verified.account_id).await? else {
        return Ok(Err(conflict("vault record missing")));
    };
    if read.record.state != VaultState::Active {
        return Ok(Err(conflict("account is not active")));
    }
    Ok(Ok(read))
}

/// Fresh salt, Argon2 parameters, verifier hash and password wrap.
async fn za_set_password(
    record: &mut VaultRecord,
    account_id: u32,
    mk: &Secret,
    password: &str,
) -> trc::Result<()> {
    let salt = store::rand::random::<[u8; 16]>();
    let params = za_argon2_params();
    let root = za_derive_root(password, salt, params).await?;
    record.salt = salt.to_vec();
    record.set_argon2_params(params);
    record.verifier_hash = derive_verifier_hash(&root).to_vec();
    record.password_wrap = wrap_key(mk, &derive_kek(&root), &aad("password", account_id));
    Ok(())
}

fn za_set_recovery(record: &mut VaultRecord, account_id: u32, mk: &Secret) -> RecoveryKey {
    let key = RecoveryKey::generate();
    record.recovery_wrap = wrap_key(
        mk,
        &derive_recovery_kek(key.as_bytes()),
        &aad("recovery", account_id),
    );
    key
}

async fn za_setup_token(
    server: &Server,
    caller_tenant_id: Option<u32>,
    request: SetupTokenRequest,
) -> trc::Result<HttpResponse> {
    let Some(account_id) = server
        .account_id_from_email(&request.account, false)
        .await?
    else {
        return Err(trc::ResourceEvent::NotFound.into_err());
    };
    let reg = za_registry_account(server, account_id).await?;
    // A tenant administrator only sees its own tenant's accounts, like the
    // registry set path; a tenant-less caller sees all.
    if let Some(tenant_id) = caller_tenant_id
        && reg
            .as_ref()
            .is_none_or(|reg| reg.account.member_tenant_id != Some(Id::from(tenant_id)))
    {
        return Err(trc::ResourceEvent::NotFound.into_err());
    }
    let Some(reg) = reg else {
        return Ok(conflict("not a user account"));
    };
    let Some(domain) = server
        .domain_by_id(reg.account.domain_id.document_id())
        .await?
    else {
        return Ok(conflict("account domain not found"));
    };
    if server.get_directory_for_cached_domain(&domain).is_some() {
        return Ok(conflict(
            "accounts in external directories are not supported",
        ));
    }

    // State first, so an active account is refused as such.
    let previous = server.za_vault_record(account_id).await?;
    if previous
        .as_ref()
        .is_some_and(|read| read.record.state != VaultState::PendingSetup)
    {
        return Ok(conflict("account is already active"));
    }

    let has_marker = reg
        .account
        .password_credential()
        .is_some_and(|c| c.secret == ZA_MARKER);
    // Eligibility: no password credential of any kind (spec 4.1).
    let has_credentials = reg.account.credentials.values().next().is_some();

    // Data checks apply to initial issuance and to reissuance (spec 4.1).
    for collection in [
        Collection::Calendar,
        Collection::CalendarEvent,
        Collection::CalendarEventNotification,
    ] {
        if server.za_has_documents(account_id, collection).await? {
            return Ok(conflict("account already holds calendar data"));
        }
    }

    let token = URL_SAFE_NO_PAD.encode(store::rand::random::<[u8; 32]>());
    let now = now() as i64;
    let expires = now + SETUP_TOKEN_TTL_SECS;

    // (1) PendingSetup record: created conditional on no record existing, or
    // a reissue that rotates the token (the old token becomes invalid).
    let mut record = match &previous {
        None if has_marker => {
            // Marker without record: data loss, operator intervention (spec 3.2).
            return Ok(conflict("marker credential without vault record"));
        }
        None if has_credentials => return Ok(conflict("account already has credentials")),
        None => VaultRecord::pending(token_hash(&token), expires),
        Some(_) if !has_marker && has_credentials => {
            // Crash recovery writes the marker below: same eligibility as issuance.
            return Ok(conflict("account already has credentials"));
        }
        Some(read) => {
            let mut record = read.record.clone();
            record.setup_token_hash = token_hash(&token);
            record.setup_token_expires = expires;
            record
        }
    };
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        previous.as_ref(),
        None,
        now,
    )
    .await?
    {
        return Ok(response);
    }

    // (2) Registry marker. A crash before this point leaves a record without
    // a marker; reissuing writes the marker (we are here).
    if !has_marker {
        let mut account = reg.account.clone();
        let credential_id = Id::from(account.next_credential_id());
        account
            .credentials
            .push(Credential::Password(PasswordCredential {
                credential_id,
                secret: ZA_MARKER.to_string(),
                ..Default::default()
            }));
        if !za_registry_update(server, account_id, &reg, account).await? {
            return Ok(conflict("registry write failed, re-issue the token"));
        }

        // (3) Invalidate classification and cached authentication again: the
        // marker is what classifies the account.
        za_invalidate_after_commit(server, account_id).await;
    }

    Ok(json(SetupTokenResponse { token, expires }))
}

async fn za_setup(
    server: &Server,
    session: &HttpSessionData,
    request: SetupRequest,
) -> trc::Result<HttpResponse> {
    let Some(account_id) = server
        .account_id_from_email(&request.username, false)
        .await?
    else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    let Some(read) = server.za_vault_record(account_id).await? else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    if read.record.state != VaultState::PendingSetup {
        return za_refuse(
            server,
            session.remote_ip,
            &request.username,
            conflict("account is already active"),
        )
        .await;
    }
    let now = now() as i64;
    let presented = token_hash(&request.token);
    let token_ok = read.record.setup_token_expires >= now
        && bool::from(
            presented
                .as_slice()
                .ct_eq(read.record.setup_token_hash.as_slice()),
        );
    if !token_ok {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    }
    if let Err(err) = server.is_secure_password(&request.password, &[&request.username]) {
        return Err(bad_request(err));
    }

    // Generate everything (spec 3). Nothing comes from an admin-supplied password.
    let mk = Secret::random();
    let mut record = read.record.clone();
    za_set_password(&mut record, account_id, &mk, &request.password).await?;
    let recovery = za_set_recovery(&mut record, account_id, &mk);
    let (public, private) = generate_keypair();
    record.public_key = public.to_vec();
    record.private_key_wrap = wrap_key(&private, &mk, &aad("private-key", account_id));
    record.state = VaultState::Active;
    record.setup_token_hash.clear();
    record.setup_token_expires = 0;

    // Single use: a concurrent duplicate loses the revision check.
    if let Err(response) =
        za_commit(server, account_id, &mut record, Some(&read), None, now).await?
    {
        return Ok(response);
    }
    Ok(json(RecoveryKeyResponse {
        recovery_key: recovery.encode(),
    }))
}

/// Re-derives the password root and rewraps MK; the recovery and
/// app-password wraps are untouched (spec 4.1).
async fn za_password(
    server: &Server,
    session: &HttpSessionData,
    request: PasswordRequest,
) -> trc::Result<HttpResponse> {
    let verified = match za_verify_primary(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    if let Err(err) = server.is_secure_password(&request.new_password, &[&request.username]) {
        return Err(bad_request(err));
    }
    let account_id = verified.account_id;
    let read = match za_reread_verified(server, &verified).await? {
        Ok(read) => read,
        Err(response) => return Ok(response),
    };
    let mut record = read.record.clone();
    za_set_password(
        &mut record,
        account_id,
        verified.keys.mk(),
        &request.new_password,
    )
    .await?;
    // Only on top of the verified generation (invariant 8).
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        Some(verified.keys.generation),
        now() as i64,
    )
    .await?
    {
        return Ok(response);
    }
    Ok(ok())
}

/// Opens MK with the recovery key, then sets the new password and a fresh
/// recovery wrap in one write; the old recovery key stops working.
async fn za_recover(
    server: &Server,
    session: &HttpSessionData,
    request: RecoverRequest,
) -> trc::Result<HttpResponse> {
    let Some(account_id) = server
        .account_id_from_email(&request.username, false)
        .await?
    else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    let Some(read) = server.za_vault_record(account_id).await? else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    if read.record.state != VaultState::Active {
        return za_refuse(
            server,
            session.remote_ip,
            &request.username,
            conflict("account is not active"),
        )
        .await;
    }
    let Some(key) = RecoveryKey::parse(&request.recovery_key) else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    let Ok(mk) = unwrap_key(
        &read.record.recovery_wrap,
        &derive_recovery_kek(key.as_bytes()),
        &aad("recovery", account_id),
    ) else {
        return Err(za_auth_failure(server, session.remote_ip, &request.username).await);
    };
    if let Err(err) = server.is_secure_password(&request.new_password, &[&request.username]) {
        return Err(bad_request(err));
    }
    let mut record = read.record.clone();
    za_set_password(&mut record, account_id, &mk, &request.new_password).await?;
    let new_key = za_set_recovery(&mut record, account_id, &mk);
    // Conditional on the record whose recovery wrap was opened: a concurrent
    // recovery with the same key loses the revision check.
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        None,
        now() as i64,
    )
    .await?
    {
        return Ok(response);
    }
    Ok(json(RecoveryKeyResponse {
        recovery_key: new_key.encode(),
    }))
}

/// Fresh recovery wrap; the previous recovery key stops working.
async fn za_recovery_key(
    server: &Server,
    session: &HttpSessionData,
    request: CredentialsRequest,
) -> trc::Result<HttpResponse> {
    let verified = match za_verify_primary(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    let account_id = verified.account_id;
    let read = match za_reread_verified(server, &verified).await? {
        Ok(read) => read,
        Err(response) => return Ok(response),
    };
    let mut record = read.record.clone();
    let new_key = za_set_recovery(&mut record, account_id, verified.keys.mk());
    // Only on top of the verified generation (invariant 8).
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        Some(verified.keys.generation),
        now() as i64,
    )
    .await?
    {
        return Ok(response);
    }
    Ok(json(RecoveryKeyResponse {
        recovery_key: new_key.encode(),
    }))
}

/// Creates an app password (spec 4.1): the secret is generated as the
/// registry generates it, and published in three steps so that a registry
/// credential never outlives its wrap: (1) Pending wrap, (2) registry
/// credential, (3) wrap Published, conditional on the same Pending entry.
async fn za_app_password(
    server: &Server,
    session: &HttpSessionData,
    request: AppPasswordRequest,
) -> trc::Result<HttpResponse> {
    let verified = match za_verify_primary(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    let description = request.description.trim();
    if description.is_empty() || description.len() > MAX_APP_PASSWORD_DESCRIPTION {
        return Err(bad_request(format!(
            "Description must be between 1 and {MAX_APP_PASSWORD_DESCRIPTION} bytes."
        )));
    }
    let account_id = verified.account_id;
    let Some(reg) = za_registry_account(server, account_id).await? else {
        return Ok(conflict("not a user account"));
    };
    let quota = server.object_quota(
        server.account(account_id).await?.object_quotas(),
        StorageQuota::MaxAppPasswords,
    );
    let existing = reg
        .account
        .credentials
        .values()
        .filter(|c| matches!(c, Credential::AppPassword(_)))
        .count();
    if existing >= quota as usize {
        return Ok(conflict("app password quota exceeded"));
    }
    let credential_id = reg.account.next_credential_id() as u32;
    let app_pass = AppPassword::new(credential_id);
    let secret_hash = hash_secret(
        server.core.network.security.password_hash_algorithm,
        app_pass.secret.to_vec(),
    )
    .await
    .caused_by(trc::location!())?;
    let publication_id = store::rand::random::<u64>();

    // (1) Pending wrap, on top of the verified generation (invariant 8).
    let read = match za_reread_verified(server, &verified).await? {
        Ok(read) => read,
        Err(response) => return Ok(response),
    };
    let created = now() as i64;
    let mut record = read.record.clone();
    // Pruning first: ids are reused once the highest credential is deleted,
    // and a stale Published wrap under a reused id is an orphan. Any wrap
    // left under the id belongs to a concurrent creation and is never
    // touched (spec 4.1).
    za_prune(server, &mut record, account_id, created).await?;
    if record.app_wrap(credential_id).is_some() {
        return Ok(conflict("app password publication in progress"));
    }
    record.app_wraps.push(AppWrap {
        credential_id,
        wrap: wrap_key(
            verified.keys.mk(),
            &derive_app_kek(&app_pass.secret, credential_id),
            &app_aad(account_id, credential_id),
        ),
        state: WrapState::Pending,
        created,
        publication_id,
    });
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        Some(verified.keys.generation),
        created,
    )
    .await?
    {
        return Ok(response);
    }

    #[cfg(feature = "test_mode")]
    crate::auth::authenticate::za_test::publish_pause_point(account_id).await;

    // (2) Registry credential, as the registry mapping creates it.
    let mut account = reg.account.clone();
    account
        .credentials
        .push(Credential::AppPassword(SecondaryCredential {
            credential_id: Id::from(credential_id),
            description: description.to_string(),
            secret: secret_hash,
            created_at: UTCDateTime::now(),
            ..Default::default()
        }));
    let registered = za_registry_update(server, account_id, &reg, account).await;
    if !matches!(registered, Ok(true)) {
        za_withdraw_pending(server, account_id, credential_id, publication_id).await;
        return registered.and(Ok(conflict("registry write failed")));
    }

    // (3) Publish. Unless it lands, the registry credential must not live
    // on, also when a store error interrupts it (best effort, then the error).
    let published = za_publish(server, account_id, credential_id, publication_id).await;
    if matches!(published, Ok(true)) {
        return Ok(json(AppPasswordResponse {
            app_password: app_pass.build(),
            credential_id,
        }));
    }
    za_withdraw_pending(server, account_id, credential_id, publication_id).await;
    za_delete_registry_credential_logged(server, account_id, credential_id).await;
    published.and(Ok(conflict("app password publication failed")))
}

/// Step (3) of the publication: Published, conditional on the same Pending
/// entry. False when the entry is gone (pruned or revoked), when the
/// commit's pruning removed it (its registry credential vanished since step
/// (2)), or when every attempt lost a race.
async fn za_publish(
    server: &Server,
    account_id: u32,
    credential_id: u32,
    publication_id: u64,
) -> trc::Result<bool> {
    for _ in 0..PUBLISH_RETRIES {
        let Some(read) = server.za_vault_record(account_id).await? else {
            return Ok(false);
        };
        let mut record = read.record.clone();
        match record.app_wrap_mut(credential_id) {
            Some(wrap)
                if wrap.state == WrapState::Pending && wrap.publication_id == publication_id =>
            {
                wrap.state = WrapState::Published;
            }
            _ => return Ok(false),
        }
        // Without an expected revision the only refusal is a lost race.
        if za_commit(
            server,
            account_id,
            &mut record,
            Some(&read),
            None,
            now() as i64,
        )
        .await?
        .is_ok()
        {
            return Ok(record
                .app_wrap(credential_id)
                .is_some_and(|w| w.state == WrapState::Published));
        }
    }
    Ok(false)
}

/// Removes this publication's Pending wrap, conditional on it still being
/// that entry. Failure is logged: the wrap cannot log in, and pruning
/// removes it after `PENDING_WRAP_MAX_AGE_SECS`.
async fn za_withdraw_pending(
    server: &Server,
    account_id: u32,
    credential_id: u32,
    publication_id: u64,
) {
    let result = async {
        for _ in 0..PUBLISH_RETRIES {
            let Some(read) = server.za_vault_record(account_id).await? else {
                return Ok(true);
            };
            let mut record = read.record.clone();
            let before = record.app_wraps.len();
            record.app_wraps.retain(|w| {
                !(w.credential_id == credential_id
                    && w.state == WrapState::Pending
                    && w.publication_id == publication_id)
            });
            if record.app_wraps.len() == before {
                return Ok(true);
            }
            if za_commit(
                server,
                account_id,
                &mut record,
                Some(&read),
                None,
                now() as i64,
            )
            .await?
            .is_ok()
            {
                return Ok(true);
            }
        }
        trc::Result::Ok(false)
    }
    .await;
    match result {
        Ok(true) => (),
        Ok(false) => {
            trc::error!(
                trc::AuthEvent::Error
                    .into_err()
                    .account_id(account_id)
                    .id(credential_id)
                    .details("Zero-access: pending app-password wrap removal kept losing")
            );
        }
        Err(err) => {
            trc::error!(
                err.account_id(account_id)
                    .id(credential_id)
                    .details("Zero-access: pending app-password wrap removal failed")
            );
        }
    }
}

/// Registry credential delete, retried on a lost revision race. Failure
/// leaves a dead credential (no Published wrap, so no login): logged, and
/// reported as false.
async fn za_delete_registry_credential_logged(
    server: &Server,
    account_id: u32,
    credential_id: u32,
) -> bool {
    let mut result = Ok(false);
    for _ in 0..PUBLISH_RETRIES {
        result = za_delete_registry_credential(server, account_id, credential_id).await;
        if !matches!(result, Ok(false)) {
            break;
        }
    }
    match result {
        Ok(true) => true,
        Ok(false) => {
            trc::error!(
                trc::AuthEvent::Error
                    .into_err()
                    .account_id(account_id)
                    .id(credential_id)
                    .details("Zero-access: app-password registry credential delete rejected")
            );
            false
        }
        Err(err) => {
            trc::error!(
                err.account_id(account_id)
                    .id(credential_id)
                    .details("Zero-access: app-password registry credential delete failed")
            );
            false
        }
    }
}

/// Revokes an app password (spec 4.1): the wrap goes in one conditional
/// write on top of the verified generation, then the registry credential.
async fn za_app_password_revoke(
    server: &Server,
    session: &HttpSessionData,
    request: RevokeRequest,
) -> trc::Result<HttpResponse> {
    let verified = match za_verify_primary(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    let account_id = verified.account_id;
    let read = match za_reread_verified(server, &verified).await? {
        Ok(read) => read,
        Err(response) => return Ok(response),
    };
    let mut record = read.record.clone();
    let before = record.app_wraps.len();
    record
        .app_wraps
        .retain(|w| w.credential_id != request.credential_id);
    if record.app_wraps.len() == before {
        // No wrap: a registry credential left by a publication that failed
        // after step (2) is dead (no login without a Published wrap), but it
        // holds a quota slot until deleted.
        let dangling = za_registry_account(server, account_id)
            .await?
            .is_some_and(|reg| {
                reg.account.credentials.values().any(|c| {
                    matches!(c, Credential::AppPassword(c)
                        if c.credential_id.document_id() == request.credential_id)
                })
            });
        if !dangling {
            return Ok(conflict("unknown app password"));
        }
        return Ok(
            if za_delete_registry_credential_logged(server, account_id, request.credential_id).await
            {
                ok()
            } else {
                conflict("conflict")
            },
        );
    }
    // The commit invalidates cached authentication; from here the
    // credential cannot log in even if the registry delete fails (spec 4.3).
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        Some(verified.keys.generation),
        now() as i64,
    )
    .await?
    {
        return Ok(response);
    }
    za_delete_registry_credential_logged(server, account_id, request.credential_id).await;
    Ok(ok())
}

/// Enrols, replaces or removes TOTP (spec 4.1): the password and, when
/// enrolled, the current code are verified, then the new URL (or its
/// removal) is committed to the vault record in one conditional write.
/// Nothing is written to the registry, whose TOTP field stays empty.
async fn za_totp(
    server: &Server,
    session: &HttpSessionData,
    request: TotpRequest,
) -> trc::Result<HttpResponse> {
    // Removal is explicit: a client that omits the field changes nothing.
    let Some(otp_auth) = request.otp_auth else {
        return Err(bad_request(
            "otp_auth is required (URL to enrol or replace, null to remove).",
        ));
    };
    if otp_auth
        .as_ref()
        .is_some_and(|url| url.len() > MAX_OTP_AUTH_URL)
    {
        return Err(bad_request(format!(
            "otp_auth must be at most {MAX_OTP_AUTH_URL} bytes."
        )));
    }
    let verified = match za_verify_primary(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    // The URL carries the secret: the error names neither it nor the
    // parser's message. An empty string is not a URL.
    if let Some(url) = &otp_auth
        && verify_totp_code(url, "000000").is_err()
    {
        return Err(bad_request("otp_auth is not a valid otpauth:// URL."));
    }
    let account_id = verified.account_id;
    let read = match za_reread_verified(server, &verified).await? {
        Ok(read) => read,
        Err(response) => return Ok(response),
    };
    if read.record.totp_url == otp_auth {
        // Nothing to change (e.g. removal when not enrolled): no write, no
        // generation bump, no cache drop.
        return Ok(ok());
    }
    let mut record = read.record.clone();
    record.totp_url = otp_auth;
    // Only on top of the verified generation (invariant 8); the commit
    // invalidates cached authentication, so a password-only login cached
    // before an enrolment does not outlive it.
    if let Err(response) = za_commit(
        server,
        account_id,
        &mut record,
        Some(&read),
        Some(verified.keys.generation),
        now() as i64,
    )
    .await?
    {
        return Ok(response);
    }
    Ok(ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://account.example.com";

    fn header_of(response: &HttpResponse, name: header::HeaderName) -> Option<&str> {
        response
            .headers()
            .and_then(|headers| headers.get(name))
            .map(|value| value.to_str().unwrap())
    }

    #[test]
    fn cors_unset_changes_nothing() {
        let preflight = za_cors_preflight(None);
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert!(preflight.headers().is_none_or(|headers| {
            !headers
                .keys()
                .any(|name| name.as_str().starts_with("access-control-") || name == header::VARY)
        }));
        let response = za_with_cors(ok(), None);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            None
        );
        assert_eq!(header_of(&response, header::VARY), None);
    }

    #[test]
    fn cors_set_allows_the_account_page() {
        let origin = HeaderValue::from_static(ORIGIN);
        let preflight = za_cors_preflight(Some(&origin));
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        for (name, value) in [
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN),
            (header::ACCESS_CONTROL_ALLOW_METHODS, "POST, OPTIONS"),
            (
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                "Content-Type, Authorization",
            ),
            (header::ACCESS_CONTROL_MAX_AGE, "600"),
            (header::VARY, "Origin"),
        ] {
            assert_eq!(header_of(&preflight, name), Some(value));
        }
        let response = za_with_cors(conflict("conflict"), Some(&origin));
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(ORIGIN)
        );
        assert_eq!(header_of(&response, header::VARY), Some("Origin"));
    }
}
