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
        Argon2Params, Secret, aad, derive_kek, derive_recovery_kek, derive_verifier_hash,
        generate_keypair, wrap_key,
    },
    record::{VaultRecord, VaultState},
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
use directory::Credentials;
use http_proto::{HttpRequest, HttpResponse, HttpSessionData, JsonResponse, ToHttpResponse};
use hyper::StatusCode;
use registry::{
    schema::{
        enums::Permission,
        prelude::{Object, ObjectInner, ObjectType},
        structs::{Account, Credential, PasswordCredential, UserAccount},
    },
    types::id::ObjectId,
};
use sha2::{Digest, Sha256};
use std::{future::Future, net::IpAddr, sync::Arc};
use store::{
    registry::write::{RegistryWrite, RegistryWriteResult},
    write::now,
};
use subtle::ConstantTimeEq;
use types::{collection::Collection, id::Id};

/// Lifetime of a setup token (spec 4.1).
pub const SETUP_TOKEN_TTL_SECS: i64 = 7 * 86400;

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

// First used by the endpoints of Tasks 8-10.
#[allow(dead_code)]
fn ok() -> HttpResponse {
    JsonResponse::new(OkResponse { ok: true })
        .no_cache()
        .into_http_response()
}

fn json<T: serde::Serialize>(value: T) -> HttpResponse {
    JsonResponse::new(value).no_cache().into_http_response()
}

/// 409: wrong state, ineligible account, or a lost revision check (spec 10).
fn conflict(error: &'static str) -> HttpResponse {
    JsonResponse::with_status(StatusCode::CONFLICT, ErrorResponse { error })
        .no_cache()
        .into_http_response()
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
            return za_setup_token(self, parse(&body)?).await;
        }

        // Credentials travel in the body; rate-limit like the other anonymous endpoints.
        self.is_http_anonymous_request_allowed(session.remote_ip)
            .await?;

        match (endpoint, sub) {
            ("setup", None) => za_setup(self, session, parse(&body)?).await,
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

// First used by the app-password endpoints (Task 9).
#[allow(dead_code)]
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
    server.za_invalidate_account(account_id).await?;
    Ok(Ok(()))
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

// First used by the password endpoints (Task 8).
#[allow(dead_code)]
struct ZaVerified {
    account_id: u32,
    keys: Arc<SessionKeys>,
}

/// Fresh, full verification of the primary password and TOTP (spec 4.1).
/// Cached CalDAV authentication is never consulted; app passwords and
/// master logins are refused with 400.
// First used by the password endpoints (Task 8).
#[allow(dead_code)]
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
    let token = server
        .authenticate(&AuthRequest::from_credentials(
            Credentials::Basic {
                username: username.to_string(),
                secret: password.to_string(),
                mfa_token: totp.filter(|t| !t.is_empty()),
            },
            session.session_id,
            session.remote_ip,
        ))
        .await?;
    match token.session_keys() {
        Some(keys) if keys.account_id == token.account_id() => Ok(Ok(ZaVerified {
            account_id: token.account_id(),
            keys: keys.clone(),
        })),
        _ => Ok(Err(conflict("not a zero-access account"))),
    }
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

async fn za_setup_token(server: &Server, request: SetupTokenRequest) -> trc::Result<HttpResponse> {
    let Some(account_id) = server
        .account_id_from_email(&request.account, false)
        .await?
    else {
        return Err(trc::ResourceEvent::NotFound.into_err());
    };
    let Some(reg) = za_registry_account(server, account_id).await? else {
        return Ok(conflict("not a user account"));
    };
    if let Some(domain) = server
        .domain_by_id(reg.account.domain_id.document_id())
        .await?
        && server.get_directory_for_cached_domain(&domain).is_some()
    {
        return Ok(conflict(
            "accounts in external directories are not supported",
        ));
    }
    let has_marker = reg
        .account
        .password_credential()
        .is_some_and(|c| c.secret == ZA_MARKER);
    // Eligibility: no password credential of any kind (spec 4.1).
    let has_credentials = reg.account.credentials.values().next().is_some();

    // Data checks apply to initial issuance and to reissuance (spec 4.1).
    if server
        .za_has_documents(account_id, Collection::Calendar)
        .await?
        || server
            .za_has_documents(account_id, Collection::CalendarEvent)
            .await?
    {
        return Ok(conflict("account already holds calendar data"));
    }

    let token = URL_SAFE_NO_PAD.encode(store::rand::random::<[u8; 32]>());
    let now = now() as i64;
    let expires = now + SETUP_TOKEN_TTL_SECS;

    // (1) PendingSetup record: created conditional on no record existing, or
    // a reissue that rotates the token (the old token becomes invalid).
    let previous = server.za_vault_record(account_id).await?;
    let mut record = match &previous {
        None if has_marker => {
            // Marker without record: data loss, operator intervention (spec 3.2).
            return Ok(conflict("marker credential without vault record"));
        }
        None if has_credentials => return Ok(conflict("account already has credentials")),
        None => VaultRecord::pending(token_hash(&token), expires),
        Some(read) if read.record.state != VaultState::PendingSetup => {
            return Ok(conflict("account is already active"));
        }
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
        server.za_invalidate_account(account_id).await?;
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
        return Ok(conflict("account is already active"));
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
