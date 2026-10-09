/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Server, cache::invalidate::CacheInvalidationBuilder, ipc::CacheInvalidation};
use ::vault::{
    Zeroizing,
    keys::{
        AAD_PASSWORD, Argon2Params, Secret, aad, app_aad, derive_app_kek, derive_kek, derive_root,
        unwrap_key, verifier_matches,
    },
    record::{VAULT_RECORD_VERSION, VaultRecord, VaultState, WrapState},
    session::SessionKeys,
};
use directory::core::secret::verify_totp_code;
use std::sync::{Arc, LazyLock};
use store::{
    Deserialize, IterateParams, Serialize, Store, ValueKey,
    write::{AlignedBytes, Archive, Archiver, BatchBuilder, ValueClass, assert::AssertValue},
};
use trc::AddContext;
use types::{collection::Collection, field::PrincipalField};
use xxhash_rust::xxh3::xxh3_64;

/// Raw stored bytes, so the revision hash can be computed exactly as the
/// store's `AssertValue::Hash` computes it.
pub struct RawValue(pub Vec<u8>);

impl Deserialize for RawValue {
    fn deserialize(bytes: &[u8]) -> trc::Result<Self> {
        Ok(RawValue(bytes.to_vec()))
    }
}

pub struct VaultRead {
    pub record: VaultRecord,
    /// xxh3 of the stored bytes; `za_vault_write(.., Some(&read))` asserts it.
    pub cas: u64,
}

fn vault_key(account_id: u32) -> ValueKey<ValueClass> {
    ValueKey::property(
        account_id,
        Collection::Principal,
        0,
        PrincipalField::ZeroAccessVault,
    )
}

/// A fresh error that carries no stored bytes: the record holds the TOTP URL
/// and key wraps, which the store's own deserialization errors would attach.
fn vault_corrupted(account_id: u32) -> trc::Error {
    trc::StoreEvent::DataCorruption
        .into_err()
        .details("corrupted zero-access vault record")
        .account_id(account_id)
        .caused_by(trc::location!())
}

fn vault_unsupported(account_id: u32, version: u8) -> trc::Error {
    trc::StoreEvent::DataCorruption
        .into_err()
        .details("unsupported zero-access vault record version")
        .account_id(account_id)
        .id(u64::from(version))
        .caused_by(trc::location!())
}

/// True for a record that exists but cannot be used (corrupt or unknown
/// version). Classification treats it like a missing record (spec 3.2); any
/// other error is a store failure and propagates.
pub(crate) fn is_vault_unusable(err: &trc::Error) -> bool {
    err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption))
}

/// The stored value is one version byte followed by the record's archive.
pub(crate) async fn vault_read_store(
    store: &Store,
    account_id: u32,
) -> trc::Result<Option<VaultRead>> {
    let Some(raw) = store
        .get_value::<RawValue>(vault_key(account_id))
        .await
        .caused_by(trc::location!())?
    else {
        return Ok(None);
    };
    let cas = xxh3_64(&raw.0);
    let archived = match raw.0.split_first() {
        Some((&version, archived)) if version == VAULT_RECORD_VERSION => archived,
        Some((&version, _)) => return Err(vault_unsupported(account_id, version)),
        None => return Err(vault_corrupted(account_id)),
    };
    let record = <Archive<AlignedBytes> as Deserialize>::deserialize(archived)
        .and_then(|archive| archive.deserialize::<VaultRecord>())
        .map_err(|_| vault_corrupted(account_id))?;
    if record.version != VAULT_RECORD_VERSION {
        return Err(vault_unsupported(account_id, record.version));
    }
    Ok(Some(VaultRead { record, cas }))
}

/// Like `vault_read_store`, but an unusable record (corrupt or unknown
/// version) is logged once and treated as missing; other store errors
/// propagate. The logged error carries the account id and a fixed
/// description, never the stored bytes.
pub(crate) async fn vault_read_usable_store(
    store: &Store,
    account_id: u32,
) -> trc::Result<Option<VaultRead>> {
    match vault_read_store(store, account_id).await {
        Err(err) if is_vault_unusable(&err) => {
            trc::error!(err);
            Ok(None)
        }
        result => result,
    }
}

pub(crate) async fn vault_write_store(
    store: &Store,
    account_id: u32,
    record: &VaultRecord,
    previous: Option<&VaultRead>,
) -> trc::Result<()> {
    if let Some(previous) = previous
        && record.revision <= previous.record.revision
    {
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access vault revision must increase on every write")
            .account_id(account_id)
            .ctx(trc::Key::From, previous.record.revision)
            .ctx(trc::Key::To, record.revision)
            .caused_by(trc::location!()));
    }

    let archive = Archiver::new(record.clone())
        .serialize()
        .caused_by(trc::location!())?;
    let mut value = Vec::with_capacity(archive.len() + 1);
    value.push(VAULT_RECORD_VERSION);
    value.extend_from_slice(&archive);

    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(
            PrincipalField::ZeroAccessVault,
            previous.map_or(AssertValue::None, |previous| {
                AssertValue::Hash(previous.cas)
            }),
        )
        .set(PrincipalField::ZeroAccessVault, value);
    store.write(batch.build_all()).await.map(|_| ())
}

impl Server {
    /// Spec 3: whether `account_id` is a key account. An unknown account id
    /// is not a key account, so upstream's outcome stands for it (plan 3
    /// ruling R3); a lookup error propagates. A caller that must fail closed
    /// maps the error to `true`. An unknown account id still yields
    /// `Ok(false)`, so a caller that must fail closed on unknown ids uses
    /// `account()` instead.
    pub async fn za_is_key_account(&self, account_id: u32) -> trc::Result<bool> {
        Ok(self
            .try_account(account_id)
            .await
            .caused_by(trc::location!())?
            .is_some_and(|account| account.is_key_account()))
    }

    pub async fn za_vault_record(&self, account_id: u32) -> trc::Result<Option<VaultRead>> {
        vault_read_store(self.store(), account_id).await
    }

    /// Conditional write (spec 3.1). `previous: None` means "must not exist";
    /// `Some(read)` requires the stored bytes to be unchanged since `read` and
    /// `record.revision` to exceed `read.record.revision` (otherwise a
    /// `StoreEvent::UnexpectedError`, nothing written). A lost race surfaces
    /// as `StoreEvent::AssertValueFailed` (`err.is_assertion_failure()`),
    /// which endpoints map to 409.
    pub async fn za_vault_write(
        &self,
        account_id: u32,
        record: &VaultRecord,
        previous: Option<&VaultRead>,
    ) -> trc::Result<()> {
        vault_write_store(self.store(), account_id, record, previous).await
    }

    /// After any successful vault write (spec 4.1): drops cached authentication
    /// and resident keys and rebuilds classification, locally and cluster-wide.
    pub async fn za_invalidate_account(&self, account_id: u32) -> trc::Result<()> {
        self.invalidate_caches(
            CacheInvalidationBuilder::default()
                .with_invalidation(CacheInvalidation::AccessToken(account_id))
                .with_invalidation(CacheInvalidation::Account(account_id)),
        )
        .await
    }

    /// True when the account holds at least one archive in `collection`.
    /// Reads the store directly: `fetch_dav_resources` would create the
    /// default calendar as a side effect.
    pub async fn za_has_documents(
        &self,
        account_id: u32,
        collection: Collection,
    ) -> trc::Result<bool> {
        let mut found = false;
        self.store()
            .iterate(
                IterateParams::new(
                    ValueKey::archive(account_id, collection, 0),
                    ValueKey::archive(account_id, collection, u32::MAX),
                )
                .no_values()
                .only_first(),
                |_, _| {
                    found = true;
                    Ok(false)
                },
            )
            .await
            .caused_by(trc::location!())?;
        Ok(found)
    }
}

/// Reason carried by the refusal of a marker credential whose vault record is
/// missing or unusable (spec 4.3).
pub const ZA_REASON_VAULT_MISSING: &str = "zero-access vault record missing";

pub enum ZaVerification {
    Valid(Arc<SessionKeys>),
    Invalid,
    MissingMfaToken,
    NoRecord,
}

/// Argon2 parameters for every new password. Stored in the record, so test
/// builds use cheap parameters without affecting production records. Under
/// the same feature as the startup warning that says so (`manager::boot`).
pub fn za_argon2_params() -> Argon2Params {
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

/// Process-wide bound on concurrent Argon2 derivations. Each one allocates
/// `m_cost_kib` (64 MiB with the production parameters), so the transient
/// memory of key derivation is at most one such allocation per available
/// core (64 MiB x `available_parallelism`); further logins queue for a
/// permit instead of each spawning its own blocking derivation.
static ZA_DERIVE_PERMITS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| {
    tokio::sync::Semaphore::new(
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .max(1),
    )
});

/// Argon2id on the blocking pool, like `hash_secret` (spec 10), at most
/// `ZA_DERIVE_PERMITS` at a time. A failure is a server fault, not an
/// authentication failure; it never carries the password or the salt.
pub async fn za_derive_root(
    password: &str,
    salt: [u8; 16],
    params: Argon2Params,
) -> trc::Result<Secret> {
    let password = Zeroizing::new(password.as_bytes().to_vec());
    let permit = ZA_DERIVE_PERMITS.acquire().await.map_err(|_| {
        trc::EventType::Server(trc::ServerEvent::ThreadError)
            .caused_by(trc::location!())
            .details("Zero-access key derivation limiter closed")
    })?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        // The permit lives as long as the derivation, also when the caller
        // stops waiting for it.
        let _permit = permit;
        tx.send(derive_root(&password, &salt, params)).ok();
    });
    match rx.await {
        Ok(Ok(root)) => Ok(root),
        Ok(Err(err)) => Err(trc::StoreEvent::CryptoError
            .into_err()
            .reason(err)
            .details("Zero-access key derivation failed")
            .caused_by(trc::location!())),
        Err(err) => Err(trc::EventType::Server(trc::ServerEvent::ThreadError)
            .caused_by(trc::location!())
            .reason(err)),
    }
}

fn corrupt(account_id: u32, what: &'static str) -> trc::Error {
    trc::StoreEvent::DataCorruption
        .into_err()
        .details(what)
        .account_id(account_id)
        .caused_by(trc::location!())
}

/// A record that was read but cannot be used is treated like a missing one:
/// logged once (account id and a fixed description only), then `NoRecord`.
fn unusable(account_id: u32, what: &'static str) -> ZaVerification {
    trc::error!(corrupt(account_id, what));
    ZaVerification::NoRecord
}

/// Spec 4.3 against one record read: Argon2 once, constant-time verifier
/// compare, TOTP with `verify_mfa_secret_hash` semantics (a missing code is
/// only reported once the password is correct), MK unwrapped on success.
async fn za_check_password(
    account_id: u32,
    record: VaultRecord,
    password: &str,
    totp_code: Option<&str>,
) -> trc::Result<ZaVerification> {
    if record.state != VaultState::Active || password.is_empty() {
        return Ok(ZaVerification::Invalid);
    }
    let Ok(salt) = <[u8; 16]>::try_from(record.salt.as_slice()) else {
        return Ok(unusable(account_id, "vault salt length"));
    };
    let Ok(verifier) = <[u8; 32]>::try_from(record.verifier_hash.as_slice()) else {
        return Ok(unusable(account_id, "vault verifier length"));
    };
    let root = za_derive_root(password, salt, record.argon2_params()).await?;
    if !verifier_matches(&root, &verifier) {
        return Ok(ZaVerification::Invalid);
    }
    if let Some(uri) = record.totp_url.as_deref() {
        match totp_code {
            Some(code) => {
                if !verify_totp_code(uri, code)? {
                    return Ok(ZaVerification::Invalid);
                }
            }
            None => return Ok(ZaVerification::MissingMfaToken),
        }
    }
    let kek = derive_kek(&root);
    let Ok(mk) = unwrap_key(&record.password_wrap, &kek, &aad(AAD_PASSWORD, account_id)) else {
        return Ok(unusable(account_id, "vault password wrap does not open"));
    };
    Ok(ZaVerification::Valid(Arc::new(SessionKeys::new(
        account_id,
        record.revision,
        mk,
    ))))
}

/// Spec 4.3, app passwords: the wrap under this credential id must be
/// Published. The generation is the record's revision.
fn za_open_app_record(
    account_id: u32,
    record: &VaultRecord,
    credential_id: u32,
    secret: &[u8],
) -> trc::Result<Option<Arc<SessionKeys>>> {
    let Some(wrap) = record
        .app_wrap(credential_id)
        .filter(|w| w.state == WrapState::Published)
    else {
        return Ok(None);
    };
    let akek = derive_app_kek(secret, credential_id);
    let mk = unwrap_key(&wrap.wrap, &akek, &app_aad(account_id, credential_id))
        .map_err(|_| corrupt(account_id, "vault app-password wrap does not open"))?;
    Ok(Some(Arc::new(SessionKeys::new(
        account_id,
        record.revision,
        mk,
    ))))
}

impl Server {
    /// One record read. A missing or unusable record is `NoRecord`; any
    /// other store error propagates.
    pub async fn za_verify_password(
        &self,
        account_id: u32,
        password: &str,
        totp_code: Option<&str>,
    ) -> trc::Result<ZaVerification> {
        let read = match self.za_vault_record(account_id).await {
            Ok(Some(read)) => read,
            Ok(None) => return Ok(ZaVerification::NoRecord),
            Err(err) if is_vault_unusable(&err) => return Ok(ZaVerification::NoRecord),
            Err(err) => return Err(err),
        };
        za_check_password(account_id, read.record, password, totp_code).await
    }

    /// One record read; the generation is this read's revision. A missing or
    /// unusable record yields `None`, so the login is refused as invalid.
    pub async fn za_open_app_wrap(
        &self,
        account_id: u32,
        credential_id: u32,
        secret: &[u8],
    ) -> trc::Result<Option<Arc<SessionKeys>>> {
        let Some(read) = vault_read_usable_store(self.store(), account_id).await? else {
            return Ok(None);
        };
        za_open_app_record(account_id, &read.record, credential_id, secret)
    }
}

/// `ZA_ACCOUNT_PAGE_ORIGIN` (spec 4.1): the origin of the account page,
/// allowed by CORS on `/api/vault/*`. Read once at startup; unset, empty or
/// not a valid header value leaves the vault API without CORS headers.
pub fn za_account_page_origin_from_env() -> Option<hyper::header::HeaderValue> {
    za_parse_origin(std::env::var("ZA_ACCOUNT_PAGE_ORIGIN").ok().as_deref())
}

fn za_parse_origin(origin: Option<&str>) -> Option<hyper::header::HeaderValue> {
    origin
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .and_then(|origin| hyper::header::HeaderValue::from_str(origin).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::vault::keys::{derive_kek, derive_verifier_hash, wrap_key};
    use store::backend::ephemeral::EphemeralStore;

    #[tokio::test]
    async fn record_write_is_conditional_on_cas() {
        let store = EphemeralStore::open();
        let mut record = VaultRecord::pending(vec![1; 32], 100);
        assert!(vault_write_store(&store, 5, &record, None).await.is_ok());
        assert!(
            vault_write_store(&store, 5, &record, None)
                .await
                .unwrap_err()
                .is_assertion_failure(),
            "a second unconditional create loses"
        );
        let read = vault_read_store(&store, 5).await.unwrap().unwrap();
        assert_eq!(read.record, record);
        record.revision += 1;
        record.state = VaultState::Active;
        assert!(
            vault_write_store(&store, 5, &record, Some(&read))
                .await
                .is_ok()
        );
        assert!(
            vault_write_store(&store, 5, &record, Some(&read))
                .await
                .unwrap_err()
                .is_assertion_failure(),
            "a stale cas loses"
        );
        let read2 = vault_read_store(&store, 5).await.unwrap().unwrap();
        assert_eq!(read2.record.revision, 2);
        assert_ne!(read2.cas, read.cas);
        assert!(vault_read_store(&store, 6).await.unwrap().is_none());
    }

    async fn write_raw(store: &Store, account_id: u32, value: Vec<u8>) {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(PrincipalField::ZeroAccessVault, value);
        store.write(batch.build_all()).await.unwrap();
    }

    #[tokio::test]
    async fn unknown_layout_and_corruption_are_rejected_without_payload() {
        let store = EphemeralStore::open();
        let mut record = VaultRecord::pending(vec![7; 32], 100);
        record.totp_url = Some("otpauth://totp/secret-marker".into());
        let archive = Archiver::new(record).serialize().unwrap();

        // Unknown leading version byte, valid archive after it.
        let mut raw = vec![VAULT_RECORD_VERSION + 1];
        raw.extend_from_slice(&archive);
        write_raw(&store, 9, raw).await;
        let err = vault_read_store(&store, 9).await.err().unwrap();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)));
        assert!(err.key(trc::Key::Value).is_none());
        assert!(!format!("{err:?}").contains("secret-marker"));

        // Correct version byte, corrupted archive.
        let mut raw = vec![VAULT_RECORD_VERSION];
        raw.extend_from_slice(&archive[..archive.len() / 2]);
        raw.extend_from_slice(b"otpauth://totp/secret-marker");
        write_raw(&store, 9, raw).await;
        let err = vault_read_store(&store, 9).await.err().unwrap();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)));
        assert!(err.key(trc::Key::Value).is_none());
        assert!(!format!("{err:?}").contains("secret-marker"));

        // Empty value.
        write_raw(&store, 9, Vec::new()).await;
        let err = vault_read_store(&store, 9).await.err().unwrap();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)));
        assert!(is_vault_unusable(&err), "call sites classify on this");
        assert!(!is_vault_unusable(
            &trc::StoreEvent::AssertValueFailed.into_err()
        ));
    }

    #[tokio::test]
    async fn app_login_read_treats_unusable_records_as_missing() {
        let store = EphemeralStore::open();
        assert!(vault_read_usable_store(&store, 9).await.unwrap().is_none());
        write_raw(&store, 9, Vec::new()).await;
        assert!(vault_read_usable_store(&store, 9).await.unwrap().is_none());
        write_raw(&store, 9, vec![VAULT_RECORD_VERSION + 1, 0, 0]).await;
        assert!(vault_read_usable_store(&store, 9).await.unwrap().is_none());
        let record = VaultRecord::pending(vec![3; 32], 100);
        vault_write_store(&store, 10, &record, None).await.unwrap();
        let read = vault_read_usable_store(&store, 10).await.unwrap().unwrap();
        assert_eq!(read.record, record);
    }

    #[tokio::test]
    async fn write_requires_increasing_revision() {
        let store = EphemeralStore::open();
        let record = VaultRecord::pending(vec![3; 32], 100);
        vault_write_store(&store, 4, &record, None).await.unwrap();
        let read = vault_read_store(&store, 4).await.unwrap().unwrap();

        let mut same = read.record.clone();
        same.state = VaultState::Active;
        let err = vault_write_store(&store, 4, &same, Some(&read))
            .await
            .unwrap_err();
        assert!(
            !err.is_assertion_failure(),
            "an invariant violation, not a race"
        );
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::UnexpectedError)));

        let after = vault_read_store(&store, 4).await.unwrap().unwrap();
        assert_eq!(after.cas, read.cas);
        assert_eq!(after.record, record);
    }

    fn test_totp_url() -> String {
        totp_rs::Builder::new()
            .with_secret(store::rand::random::<[u8; 20]>())
            .with_account_name("john")
            .with_issuer(Some("Stalwart"))
            .build()
            .unwrap()
            .to_url()
            .unwrap()
    }

    /// A six-digit code that matches no window `check_current` could accept,
    /// including the next one in case the step rolls over mid-test.
    fn wrong_totp_code(uri: &str) -> String {
        let totp = totp_rs::Totp::from_url(uri).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let accepted: Vec<String> = [now.saturating_sub(30), now, now + 30, now + 60]
            .into_iter()
            .map(|time| totp.generate(time).to_string())
            .collect();
        (0u32..)
            .map(|n| format!("{n:06}"))
            .find(|code| !accepted.contains(code))
            .unwrap()
    }

    #[test]
    fn totp_helper_accepts_current_code_and_rejects_garbage() {
        use directory::core::secret::verify_totp_code;
        let uri = test_totp_url();
        let code = totp_rs::Totp::from_url(&uri)
            .unwrap()
            .generate_current()
            .to_string();
        assert!(verify_totp_code(&uri, &code).unwrap());
        let wrong = wrong_totp_code(&uri);
        assert!(!verify_totp_code(&uri, &wrong).unwrap());
        let err = verify_totp_code("not a url", "000000").unwrap_err();
        assert!(!format!("{err:?}").contains("not a url"));
    }

    const TEST_PARAMS: Argon2Params = Argon2Params {
        m_cost_kib: 64,
        t_cost: 1,
        p_cost: 1,
    };

    async fn active_record(password: &str, mk: &Secret) -> VaultRecord {
        let salt = [5u8; 16];
        let root = za_derive_root(password, salt, TEST_PARAMS).await.unwrap();
        let mut record = VaultRecord::pending(vec![], 0);
        record.revision = 7;
        record.state = VaultState::Active;
        record.salt = salt.to_vec();
        record.set_argon2_params(TEST_PARAMS);
        record.verifier_hash = derive_verifier_hash(&root).to_vec();
        record.password_wrap = wrap_key(mk, &derive_kek(&root), &aad(AAD_PASSWORD, 1));
        record
    }

    fn expect_valid(result: ZaVerification) -> Arc<SessionKeys> {
        match result {
            ZaVerification::Valid(keys) => keys,
            _ => panic!("expected a valid verification"),
        }
    }

    #[tokio::test]
    async fn password_verification_follows_the_record() {
        let mk = Secret::random();
        let record = active_record("correct horse", &mk).await;

        let keys = expect_valid(
            za_check_password(1, record.clone(), "correct horse", None)
                .await
                .unwrap(),
        );
        assert_eq!(keys.account_id, 1);
        assert_eq!(keys.generation, 7, "generation is the read's revision");
        assert_eq!(keys.mk().as_bytes(), mk.as_bytes());

        for wrong in ["wrong horse", "", ::vault::ZA_MARKER] {
            assert!(matches!(
                za_check_password(1, record.clone(), wrong, None)
                    .await
                    .unwrap(),
                ZaVerification::Invalid
            ));
        }

        let mut pending = record.clone();
        pending.state = VaultState::PendingSetup;
        assert!(matches!(
            za_check_password(1, pending, "correct horse", None)
                .await
                .unwrap(),
            ZaVerification::Invalid
        ));

        // A wrap bound to another account does not open: the record is
        // unusable, refused like a missing one.
        assert!(matches!(
            za_check_password(2, record.clone(), "correct horse", None)
                .await
                .unwrap(),
            ZaVerification::NoRecord
        ));

        // Structural corruption is unusable whatever the password.
        let mut short_salt = record.clone();
        short_salt.salt.pop();
        let mut short_verifier = record;
        short_verifier.verifier_hash.pop();
        for corrupted in [short_salt, short_verifier] {
            for password in ["correct horse", "wrong horse"] {
                assert!(matches!(
                    za_check_password(1, corrupted.clone(), password, None)
                        .await
                        .unwrap(),
                    ZaVerification::NoRecord
                ));
            }
        }
    }

    #[tokio::test]
    async fn totp_is_checked_only_after_the_password() {
        use directory::core::secret::verify_totp_code;
        let mut record = active_record("correct horse", &Secret::random()).await;
        let uri = test_totp_url();
        let code = totp_rs::Totp::from_url(&uri)
            .unwrap()
            .generate_current()
            .to_string();
        let wrong = wrong_totp_code(&uri);
        assert!(!verify_totp_code(&uri, &wrong).unwrap());
        record.totp_url = Some(uri);

        assert!(matches!(
            za_check_password(1, record.clone(), "correct horse", None)
                .await
                .unwrap(),
            ZaVerification::MissingMfaToken
        ));
        assert!(matches!(
            za_check_password(1, record.clone(), "wrong horse", None)
                .await
                .unwrap(),
            ZaVerification::Invalid
        ));
        assert!(matches!(
            za_check_password(1, record.clone(), "wrong horse", Some(&code))
                .await
                .unwrap(),
            ZaVerification::Invalid
        ));
        assert!(matches!(
            za_check_password(1, record.clone(), "correct horse", Some(&wrong))
                .await
                .unwrap(),
            ZaVerification::Invalid
        ));
        expect_valid(
            za_check_password(1, record, "correct horse", Some(&code))
                .await
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn key_derivation_waits_for_a_permit() {
        // Hold every permit: a derivation must queue until one is released.
        let permits = u32::try_from(
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .max(1),
        )
        .unwrap();
        let held = ZA_DERIVE_PERMITS.acquire_many(permits).await.unwrap();
        let derivation = tokio::spawn(za_derive_root("queued", [1u8; 16], TEST_PARAMS));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !derivation.is_finished(),
            "no derivation runs beyond the bound"
        );
        drop(held);
        tokio::time::timeout(std::time::Duration::from_secs(10), derivation)
            .await
            .expect("the derivation runs once a permit is free")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn key_derivation_failure_is_a_server_fault_without_secrets() {
        let params = Argon2Params {
            m_cost_kib: 0,
            t_cost: 0,
            p_cost: 0,
        };
        let err = za_derive_root("hunter2-secret", [9u8; 16], params)
            .await
            .err()
            .unwrap();
        assert!(!matches!(err.as_ref(), trc::EventType::Auth(_)));
        assert!(!format!("{err:?}").contains("hunter2-secret"));
    }

    #[test]
    fn app_wrap_opens_only_when_published() {
        use ::vault::record::{AppWrap, WrapState};
        let mk = Secret::random();
        let secret = b"app-password-secret";
        let mut record = VaultRecord::pending(vec![], 0);
        record.revision = 9;
        record.state = VaultState::Active;
        record.app_wraps.push(AppWrap {
            credential_id: 4,
            wrap: wrap_key(&mk, &derive_app_kek(secret, 4), &app_aad(1, 4)),
            state: WrapState::Published,
            created: 0,
            publication_id: 1,
        });

        let keys = za_open_app_record(1, &record, 4, secret).unwrap().unwrap();
        assert_eq!(keys.account_id, 1);
        assert_eq!(keys.generation, 9);
        assert_eq!(keys.mk().as_bytes(), mk.as_bytes());

        assert!(za_open_app_record(1, &record, 5, secret).unwrap().is_none());
        let err = za_open_app_record(1, &record, 4, b"other").err().unwrap();
        assert!(is_vault_unusable(&err));

        record.app_wraps[0].state = WrapState::Pending;
        assert!(za_open_app_record(1, &record, 4, secret).unwrap().is_none());
    }

    #[test]
    fn za_keys_for_only_yields_the_authenticated_account() {
        use crate::auth::AccessToken;
        use ::vault::{keys::Secret, session::SessionKeys};
        use std::sync::Arc;

        let token = AccessToken::from_permissions(7, []).with_session_keys(Arc::new(
            SessionKeys::new(7, 1, Secret::from_bytes([1; 32])),
        ));
        assert!(token.session_keys().is_some());
        assert_eq!(token.za_keys_for(7).map(|k| k.account_id), Some(7));
        assert!(token.za_keys_for(8).is_none());
        assert!(
            AccessToken::from_permissions(7, [])
                .za_keys_for(7)
                .is_none()
        );
    }

    #[test]
    fn account_page_origin_parsing() {
        assert_eq!(za_parse_origin(None), None);
        assert_eq!(za_parse_origin(Some("")), None);
        assert_eq!(za_parse_origin(Some("  ")), None);
        assert_eq!(za_parse_origin(Some("bad\norigin")), None);
        assert_eq!(
            za_parse_origin(Some(" https://account.example.com ")).unwrap(),
            "https://account.example.com"
        );
    }
}
