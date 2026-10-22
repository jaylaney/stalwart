/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Server, cache::invalidate::CacheInvalidationBuilder, ipc::CacheInvalidation};
use ::vault::{
    Zeroizing,
    keys::{
        AAD_PASSWORD, Argon2Params, KeyError, Secret, WrapMark, aad, app_aad, derive_app_kek,
        derive_kek, derive_public_key, derive_root, unwrap_key, unwrap_private_key,
        verifier_matches, wrap_private_key,
    },
    pins::{ZaPinRecord, open_pin_record, seal_pin_record},
    record::{VAULT_RECORD_VERSION, VaultRecord, VaultState, WrapState},
    session::SessionKeys,
};
use directory::core::secret::verify_totp_code;
use registry::{
    schema::{
        prelude::{ObjectType, Property},
        structs::Task,
    },
    types::EnumImpl,
};
use std::sync::{Arc, LazyLock};
use store::{
    Deserialize, IterateParams, Serialize, SerializeInfallible, Store, ValueKey,
    search::IndexDocument,
    write::{
        AlignedBytes, Archive, Archiver, BatchBuilder, RegistryClass, SearchIndex,
        SearchIndexClass, SearchIndexId, SearchIndexType, TaskQueueClass, ValueClass,
        assert::AssertValue, now,
    },
};
use trc::AddContext;
use types::{
    collection::Collection,
    field::{Field, PrincipalField},
};
use utils::{DomainPart, sanitize_email};
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

/// The pin record (key pinning spec 4.1), keyed like the vault record.
fn pins_key(account_id: u32) -> ValueKey<ValueClass> {
    ValueKey::property(
        account_id,
        Collection::Principal,
        0,
        PrincipalField::ZeroAccessPins,
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
    vault_batch_write(store, account_id, record, previous, None).await
}

/// The recheck's abort (mail spec 6.1): one batch on Principal document 0
/// asserting the bytes `previous` read, then clearing the record. A lost
/// assertion is `Err(e)` with `e.is_assertion_failure()` and nothing
/// removed.
pub(crate) async fn vault_delete_store(
    store: &Store,
    account_id: u32,
    previous: &VaultRead,
) -> trc::Result<()> {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(
            PrincipalField::ZeroAccessVault,
            AssertValue::Hash(previous.cas),
        )
        .clear(PrincipalField::ZeroAccessVault);
    store.write(batch.build_all()).await.map(|_| ())
}

/// The one vault batch, shared by the raw write (`vault_write_store`) and
/// the commit (`vault_commit_store`), on Principal document 0: the vault
/// assertion (the bytes `previous` read, or absence), then, with `pins`,
/// field 151's assertion (`pins.previous.assertion()`), then the vault set,
/// then field 151's set. Each key's assertion precedes its set (key pinning
/// spec 4.4), and both assertions precede both sets, so a lost assertion
/// writes nothing even on a store that applies sets as it goes (the
/// ephemeral store does not roll back).
async fn vault_batch_write(
    store: &Store,
    account_id: u32,
    record: &VaultRecord,
    previous: Option<&VaultRead>,
    pins: Option<ZaPinWrite<'_>>,
) -> trc::Result<()> {
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
        );
    if let Some(pins) = &pins {
        batch.assert_value(PrincipalField::ZeroAccessPins, pins.previous.assertion());
    }
    batch.set(PrincipalField::ZeroAccessVault, value);
    if let Some(pins) = pins {
        batch.set(PrincipalField::ZeroAccessPins, pins.sealed);
    }
    store.write(batch.build_all()).await.map(|_| ())
}

/// Attempts of one pin-record write, a single assertion or a vault batch
/// (key pinning spec 4.2); losing the last one is the failure.
pub const ZA_PIN_WRITE_ATTEMPTS: usize = 8;

/// Key pinning spec 4.3: the one normalization of pin addresses, for the
/// fan-out (applied to what `Email::new` produced), the pins and the
/// endpoints (applied to the string as given). No `mailto:` decoding and no
/// addr-spec extraction, so those forms are `None`.
pub fn za_normalize_address(address: &str) -> Option<String> {
    sanitize_email(address).map(|address| address.to_canonical_address().into_owned())
}

/// One pin-record read (key pinning spec 4.2): the stored bytes, whatever
/// they are, and the record they open to. No `Debug`: the record holds
/// addresses.
pub struct ZaPinRead {
    pub raw: Option<Vec<u8>>,
    /// `None` when absent or unusable (spec 4.4).
    pub record: Option<ZaPinRecord>,
}

impl ZaPinRead {
    /// What a write on top of this read asserts: the hash of the bytes read
    /// whenever there were any, garbage included; absence only when the read
    /// returned nothing (spec 4.2).
    pub fn assertion(&self) -> AssertValue {
        self.raw
            .as_deref()
            .map_or(AssertValue::None, |raw| AssertValue::Hash(xxh3_64(raw)))
    }

    /// The record opened, decoded and has version 1 (spec 4.4).
    pub fn is_usable(&self) -> bool {
        self.record.is_some()
    }
}

/// The associated data the private-key wrap opened under (key pinning spec
/// 4.4); `Neither` is tampered, corrupt or missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaMark {
    Pinned,
    Legacy,
    Neither,
}

/// The mark of one vault read (spec 4.4): the mark, the hash a batch asserts
/// and the wrap it rewraps all come from `vault`. A missing or unusable
/// record is `Neither` with `vault: None`; a record whose wrap opens under
/// neither purpose is `Neither` with the read kept.
pub struct ZaMarkRead {
    pub vault: Option<VaultRead>,
    pub mark: ZaMark,
    /// The unwrapped private key, `Some` for `Pinned` and `Legacy`.
    pub private_key: Option<Secret>,
}

/// The state a pin decision runs against. `mark` is read only when `pins` is
/// not usable: a record that opens is proof enough (spec 4.4).
pub struct ZaPinState {
    pub pins: ZaPinRead,
    pub mark: Option<ZaMarkRead>,
}

/// A pin-record write: `sealed` replaces the value `previous` read, asserted
/// as `previous.assertion()`.
pub struct ZaPinWrite<'a> {
    pub previous: &'a ZaPinRead,
    pub sealed: Vec<u8>,
}

/// A decision of `za_pin_transact`: write the record (its revision is set by
/// the write) or keep the stored state.
pub enum ZaPinStep<T> {
    Write(ZaPinRecord, T),
    Keep(T),
}

/// The outcome of `za_pin_transact`. `Written` carries the state as written
/// (`mark: None`, the record usable); `Conflict` is the last attempt lost.
pub enum ZaPinTransact<T> {
    Written { state: ZaPinState, value: T },
    Kept { state: ZaPinState, value: T },
    Conflict,
}

pub(crate) async fn pin_read_store(
    store: &Store,
    account_id: u32,
    mk: &Secret,
) -> trc::Result<ZaPinRead> {
    let raw = store
        .get_value::<RawValue>(pins_key(account_id))
        .await
        .caused_by(trc::location!())?
        .map(|raw| raw.0);
    let record = raw
        .as_deref()
        .and_then(|raw| open_pin_record(raw, mk, account_id));
    Ok(ZaPinRead { raw, record })
}

/// A pin-record write alone: field 151's assertion, then its set.
pub(crate) async fn pin_write_store(
    store: &Store,
    account_id: u32,
    write: ZaPinWrite<'_>,
) -> trc::Result<()> {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(PrincipalField::ZeroAccessPins, write.previous.assertion())
        .set(PrincipalField::ZeroAccessPins, write.sealed);
    store.write(batch.build_all()).await.map(|_| ())
}

/// Key pinning spec 4.4-4.5: sets `record.revision` to the previous revision
/// plus one (1 when `previous` is `None`) and writes one batch on Principal
/// document 0, in this order: the vault assertion (the bytes `previous`
/// read, or absence), with `pins` the pin-record assertion, the vault set,
/// with `pins` the pin-record set (`vault_batch_write`). A lost assertion is
/// `Err(e)` with `e.is_assertion_failure()` and nothing written.
pub(crate) async fn vault_commit_store(
    store: &Store,
    account_id: u32,
    record: &mut VaultRecord,
    previous: Option<&VaultRead>,
    pins: Option<ZaPinWrite<'_>>,
) -> trc::Result<()> {
    record.revision = previous.map_or(1, |previous| previous.record.revision + 1);
    vault_batch_write(store, account_id, record, previous, pins).await
}

/// One vault read, the wrap opened pinned first, then legacy (spec 4.4). A
/// missing or unusable record is `Neither` with `vault: None`; other store
/// errors propagate. Logs nothing: callers trace their own outcome.
pub(crate) async fn mark_read_store(store: &Store, keys: &SessionKeys) -> trc::Result<ZaMarkRead> {
    let account_id = keys.account_id;
    let vault = match vault_read_store(store, account_id).await {
        Ok(read) => read,
        Err(err) if is_vault_unusable(&err) => None,
        Err(err) => return Err(err),
    };
    let opened = vault.as_ref().and_then(|read| {
        unwrap_private_key(&read.record.private_key_wrap, keys.mk(), account_id).ok()
    });
    let (mark, private_key) = match opened {
        Some((key, WrapMark::Pinned)) => (ZaMark::Pinned, Some(key)),
        Some((key, WrapMark::Legacy)) => (ZaMark::Legacy, Some(key)),
        None => (ZaMark::Neither, None),
    };
    Ok(ZaMarkRead {
        vault,
        mark,
        private_key,
    })
}

/// Key pinning spec 5.1: the recipient's `public_key` when its record reads,
/// is `Active` and holds 32 bytes; `None` for a missing, unusable or pending
/// record. Logs nothing (an unusable record is not traced here) and returns
/// nothing else of the record.
pub(crate) async fn served_public_key_store(
    store: &Store,
    account_id: u32,
) -> trc::Result<Option<[u8; 32]>> {
    match vault_read_store(store, account_id).await {
        Ok(Some(read)) if read.record.state == VaultState::Active => {
            Ok(<[u8; 32]>::try_from(read.record.public_key.as_slice()).ok())
        }
        Ok(_) => Ok(None),
        Err(err) if is_vault_unusable(&err) => Ok(None),
        Err(err) => Err(err),
    }
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
    /// which endpoints map to 409. It neither sets the revision nor
    /// invalidates: product code calls `za_vault_commit` (or the HTTP
    /// module's `za_commit`, which wraps it); test helpers, acting as the
    /// store writer, call this.
    pub async fn za_vault_write(
        &self,
        account_id: u32,
        record: &VaultRecord,
        previous: Option<&VaultRead>,
    ) -> trc::Result<()> {
        vault_write_store(self.store(), account_id, record, previous).await
    }

    /// Every vault write of product code (key pinning spec 4.5): sets the
    /// next revision, writes conditionally on `previous` (absence when
    /// `None`), with `pins` in the same batch (spec 4.4: the vault assertion,
    /// the pin-record assertion, the vault set, the pin-record set), then
    /// invalidates cached classification and authentication. A lost
    /// assertion is `Err(e)` with `e.is_assertion_failure()`, nothing
    /// written. An invalidation failure is logged, never returned, so a
    /// committed write never reports an error.
    pub async fn za_vault_commit(
        &self,
        account_id: u32,
        record: &mut VaultRecord,
        previous: Option<&VaultRead>,
        pins: Option<ZaPinWrite<'_>>,
    ) -> trc::Result<()> {
        vault_commit_store(self.store(), account_id, record, previous, pins).await?;
        self.za_invalidate_committed(account_id).await;
        Ok(())
    }

    /// The recheck's abort (mail spec 6.1): removes a pending record this
    /// process wrote, conditional on the bytes it wrote (`previous`), then
    /// invalidates as `za_vault_commit` does. A lost assertion is `Err(e)`
    /// with `e.is_assertion_failure()`, nothing removed.
    pub async fn za_vault_delete(&self, account_id: u32, previous: &VaultRead) -> trc::Result<()> {
        vault_delete_store(self.store(), account_id, previous).await?;
        self.za_invalidate_committed(account_id).await;
        Ok(())
    }

    /// Mail spec 6.1, the provisioning fence: appended as the last ops of a
    /// batch that stores into `account_id` read as an ordinary account, it
    /// commits the batch only while the account holds no vault record. Last,
    /// because the context it selects is not restored for later ops. Nothing
    /// on PostgreSQL or MySQL: key accounts cannot exist there (`setup-token`
    /// refuses) and the assertion would be no fence.
    pub fn za_provisioning_fence(&self, batch: &mut BatchBuilder, account_id: u32) {
        if !self.store().is_pg_or_mysql() {
            batch
                .with_account_id(account_id)
                .with_collection(Collection::Principal)
                .with_document(0)
                .assert_value(PrincipalField::ZeroAccessVault, AssertValue::None);
        }
    }

    /// Mail ruling R58, spec 12's rule 32 for the index worker: a search
    /// document built from `account_id` read as an ordinary account is
    /// written by the internal search store only while the account holds no
    /// vault record, asserted in the document's first commit chunk (the one
    /// with its `Document` key). A superseded execution (a worker that
    /// outlived its task lease) therefore cannot write after provisioning,
    /// and a write that commits first is seen by the recheck's document scan
    /// (R51). Nothing on PostgreSQL or MySQL, as `za_provisioning_fence`, or
    /// without an internal search store. On a search store other than the
    /// data store (a separate FoundationDB store among them) the assertion
    /// reads the search store's key space and passes vacuously: R51's
    /// recorded limit.
    pub fn za_index_fence(&self, document: IndexDocument, account_id: u32) -> IndexDocument {
        if self.store().is_pg_or_mysql() || self.search_store().internal_fts().is_none() {
            document
        } else {
            document.with_assert_absent(
                account_id,
                Collection::Principal,
                0,
                PrincipalField::ZeroAccessVault,
            )
        }
    }

    /// The invalidation after a committed vault write; a failure is logged,
    /// never returned.
    async fn za_invalidate_committed(&self, account_id: u32) {
        if let Err(err) = self.za_invalidate_account(account_id).await {
            trc::error!(
                err.account_id(account_id)
                    .details("Zero-access cache invalidation failed after a committed write")
            );
        }
    }

    /// The session account's pin record, read fresh (key pinning spec 4.2).
    pub async fn za_pin_read(&self, keys: &SessionKeys) -> trc::Result<ZaPinRead> {
        pin_read_store(self.store(), keys.account_id, keys.mk()).await
    }

    /// The session account's mark, from one fresh vault read (spec 4.4).
    pub async fn za_mark_read(&self, keys: &SessionKeys) -> trc::Result<ZaMarkRead> {
        mark_read_store(self.store(), keys).await
    }

    /// The pin record, and the mark only when the record is not usable.
    pub async fn za_pin_state(&self, keys: &SessionKeys) -> trc::Result<ZaPinState> {
        let pins = self.za_pin_read(keys).await?;
        let mark = if pins.is_usable() {
            None
        } else {
            Some(self.za_mark_read(keys).await?)
        };
        Ok(ZaPinState { pins, mark })
    }

    /// Key pinning spec 4.2's attempts: `decide` runs against `start` on the
    /// first attempt and a fresh `za_pin_state` on every later one. A `Write`
    /// gets the next record revision and is sealed under MK; under a legacy
    /// wrap it is the first-pin or heal batch (the rewrap to the pinned wrap
    /// and the record, asserted at the same vault read, spec 4.4), otherwise
    /// a pin-record write alone. A `Write` with no usable record under a
    /// wrap that opens under neither purpose writes nothing and is an error
    /// (invariant 22: no record is created in that state). A lost assertion
    /// re-reads and decides again, up to `ZA_PIN_WRITE_ATTEMPTS`; then
    /// `Conflict`.
    pub async fn za_pin_transact<T: Send>(
        &self,
        keys: &SessionKeys,
        mut start: Option<ZaPinState>,
        mut decide: impl FnMut(&ZaPinState) -> ZaPinStep<T> + Send,
    ) -> trc::Result<ZaPinTransact<T>> {
        let account_id = keys.account_id;
        for _ in 0..ZA_PIN_WRITE_ATTEMPTS {
            let state = match start.take() {
                Some(state) => state,
                None => self.za_pin_state(keys).await?,
            };
            let (mut record, value) = match decide(&state) {
                ZaPinStep::Keep(value) => return Ok(ZaPinTransact::Kept { state, value }),
                ZaPinStep::Write(record, value) => (record, value),
            };
            let ZaPinState { pins, mark } = state;
            // The same vault read throughout: the mark, the asserted hash
            // and the wrap the batch rewraps.
            let mark = match mark {
                Some(mark) => mark,
                None => self.za_mark_read(keys).await?,
            };
            if mark.mark == ZaMark::Neither && !pins.is_usable() {
                return Err(trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details(
                        "zero-access: no pin record is created under a wrap that opens under neither purpose",
                    )
                    .account_id(account_id)
                    .caused_by(trc::location!()));
            }
            record.revision = pins
                .record
                .as_ref()
                .map_or(1, |previous| previous.revision + 1);
            let sealed = seal_pin_record(&record, keys.mk(), account_id);
            let write = ZaPinWrite {
                previous: &pins,
                sealed: sealed.clone(),
            };
            let result = match (&mark.vault, &mark.private_key) {
                (Some(vault), Some(private_key)) if mark.mark == ZaMark::Legacy => {
                    let mut rewrapped = vault.record.clone();
                    rewrapped.private_key_wrap =
                        wrap_private_key(private_key, keys.mk(), account_id, WrapMark::Pinned);
                    self.za_vault_commit(account_id, &mut rewrapped, Some(vault), Some(write))
                        .await
                }
                // The pin record alone: under a pinned wrap (the record
                // usable, absent or unusable), or a usable record under a
                // wrap that opens under neither purpose (no usable record
                // under neither was refused above).
                _ => pin_write_store(self.store(), account_id, write).await,
            };
            match result {
                Ok(()) => {
                    return Ok(ZaPinTransact::Written {
                        state: ZaPinState {
                            pins: ZaPinRead {
                                raw: Some(sealed),
                                record: Some(record),
                            },
                            mark: None,
                        },
                        value,
                    });
                }
                Err(err) if err.is_assertion_failure() => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(ZaPinTransact::Conflict)
    }

    /// Key pinning spec 5.1: the served key of a recipient, read fresh; see
    /// `served_public_key_store`.
    pub async fn za_served_public_key(&self, account_id: u32) -> trc::Result<Option<[u8; 32]>> {
        served_public_key_store(self.store(), account_id).await
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
        self.za_has_property(account_id, collection, Field::ARCHIVE)
            .await
    }

    /// True when any document of `collection` in the account holds the
    /// property `field`. Mail ruling R46: a destroyed message's metadata
    /// archive (`EmailField::Metadata`) outlives its archive until the
    /// scheduled unindex task runs.
    pub async fn za_has_property(
        &self,
        account_id: u32,
        collection: Collection,
        field: impl Into<u8>,
    ) -> trc::Result<bool> {
        let field = field.into();
        let mut found = false;
        self.store()
            .iterate(
                IterateParams::new(
                    ValueKey::property(account_id, collection, 0, field),
                    ValueKey::property(account_id, collection, u32::MAX, field),
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

    /// Mail ruling R52: true when any spam training sample is indexed under
    /// the account (the `AccountId` index the trainer's `delete_samples`
    /// clears). A sample keeps its message's blob linked and its sender and
    /// subject in the registry until it expires.
    pub async fn za_has_spam_samples(&self, account_id: u32) -> trc::Result<bool> {
        let index = |item_id| {
            ValueKey::from(ValueClass::Registry(RegistryClass::Index {
                index_id: Property::AccountId.to_id(),
                object_id: ObjectType::SpamTrainingSample.to_id(),
                item_id,
                key: (account_id as u64).serialize(),
            }))
        };
        let mut found = false;
        self.store()
            .iterate(
                IterateParams::new(index(0), index(u64::MAX))
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

    /// Mail ruling R54: true when the task queue holds an `IndexDocument`,
    /// `UnindexDocument`, `MergeThreads` or `CalendarItipMessage` task of the
    /// account, pending, retrying, in flight or failed. A task's record stays
    /// in the queue from the batch that schedules it until the worker's
    /// `update_tasks`, so an index write still in flight is seen here before
    /// it lands; a retained merge task holds a subject of up to 15 bytes
    /// verbatim (`CheekyHash`). The queue has no per-account range: one pass
    /// over every task record, stopping at the first hit.
    pub async fn za_has_tasks(&self, account_id: u32) -> trc::Result<bool> {
        let task = |id| ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id }));
        let mut found = false;
        self.store()
            .iterate(
                IterateParams::new(task(0), task(u64::MAX)).ascending(),
                |_, value| {
                    let owner = match Task::deserialize(value)? {
                        Task::IndexDocument(task) | Task::UnindexDocument(task) => {
                            Some(task.account_id)
                        }
                        Task::MergeThreads(task) => Some(task.account_id),
                        Task::CalendarItipMessage(task) => Some(task.account_id),
                        // Content-free (ids and times) and possibly due days
                        // ahead.
                        Task::CalendarAlarmEmail(_) | Task::CalendarAlarmNotification(_) => None,
                        // Scheduled for every account by the store's purge and
                        // reindex sweeps.
                        Task::AccountMaintenance(_) => None,
                        // Not a provisioning state.
                        Task::DestroyAccount(_) | Task::RestoreArchivedItem(_) => None,
                        // No account.
                        Task::IndexTrace(_)
                        | Task::DmarcReport(_)
                        | Task::TlsReport(_)
                        | Task::TenantMaintenance(_)
                        | Task::StoreMaintenance(_)
                        | Task::SpamFilterMaintenance(_)
                        | Task::AcmeRenewal(_)
                        | Task::DkimManagement(_)
                        | Task::DnsManagement(_) => None,
                    };
                    found = owner.is_some_and(|owner| owner.document_id() == account_id);
                    Ok(!found)
                },
            )
            .await
            .caused_by(trc::location!())?;
        Ok(found)
    }

    /// Mail ruling R51: true when the internal search store holds an Email
    /// document of the account. Each indexed document has one `Document`
    /// key, the ground truth there; the unindex task deletes it in a write
    /// after the one that clears the metadata archive. Any search store
    /// other than the internal one (ElasticSearch, MeiliSearch, or a
    /// PostgreSQL or MySQL search store) is not checked, a recorded limit.
    pub async fn za_has_email_search_documents(&self, account_id: u32) -> trc::Result<bool> {
        let Some(store) = self.search_store().internal_fts() else {
            return Ok(false);
        };
        let document = |document_id| {
            ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                index: SearchIndex::Email,
                id: SearchIndexId::Account {
                    account_id,
                    document_id,
                },
                typ: SearchIndexType::Document,
            }))
        };
        let mut found = false;
        store
            .iterate(
                IterateParams::new(document(0), document(u32::MAX))
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

/// Key pinning spec 3.3: what the self-check found. No `Debug`: no key is
/// ever traced (invariant 21).
pub struct ZaSelfCheck {
    /// Whether the stored `public_key` matched the derived key at the first
    /// read, before any repair (spec 6.2).
    pub stored_key_matches: bool,
    /// Derived from the private key, never the stored copy: the source of
    /// the account's own fingerprint (spec 3.2).
    pub public_key: [u8; 32],
}

/// Key pinning spec 3.3, the self-check; it replaces release 2's
/// legacy-only private-key reader (invitations spec 3.2). Each attempt is
/// one vault read, the wrap opened pinned first, then legacy (spec 4.4),
/// and one pin record read. The public key derived from the private key is
/// compared with the record's. On a mismatch it traces once per call and
/// commits one batch (`vault_commit_store`, asserted at the bytes read):
/// the vault record with `public_key` rewritten from the derived key; with
/// a usable pin record, that record at its next revision with
/// `key_repaired_at` set; and, when that record sits under a legacy wrap,
/// the heal's rewrap. With `heal` (the drain's first pass) a usable record
/// under a legacy wrap is healed even when the key matches; without it
/// only a repair heals. It never creates a pin record (invariant 22). A
/// lost assertion re-reads and checks again, up to `ZA_PIN_WRITE_ATTEMPTS`
/// attempts; after the last, the next session checks again. The `bool` is
/// whether it committed a write.
///
/// The unwrap outcome is recorded in `keys`, the first one standing:
/// - a wrap that opens under neither purpose, or a missing or unusable
///   record, is a failure, traced once per session (account id and a
///   `zero-access:` description only) and an error;
/// - a store error after the key opened (the pin read, the write, a later
///   attempt's read) records the key and returns the error; one on the
///   first vault read records nothing, so a later call retries.
///
/// With `once` (the drain), a recorded outcome is returned without a store
/// read: `stored_key_matches` true and the key derived from the recorded
/// private key, or the recorded failure's error. Without it (the contacts
/// endpoints) the check always runs and leaves a recorded outcome as it is.
pub(crate) async fn self_check_store(
    store: &Store,
    keys: &SessionKeys,
    heal: bool,
    once: bool,
) -> trc::Result<(ZaSelfCheck, bool)> {
    let account_id = keys.account_id;
    if once {
        match keys.private_key() {
            Some(Ok(private_key)) => {
                let check = ZaSelfCheck {
                    stored_key_matches: true,
                    public_key: derive_public_key(private_key),
                };
                return Ok((check, false));
            }
            Some(Err(_)) => return Err(private_key_unavailable(account_id)),
            None => {}
        }
    }
    // The first attempt's comparison, and the key an earlier attempt opened.
    let mut first_matches = None;
    let mut opened = None;
    let mut traced = false;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let read = match mark_read_store(store, keys).await {
            Ok(read) => read,
            Err(err) => {
                record_private_key(keys, opened);
                return Err(err);
            }
        };
        // The hook fails a session's first unwrap only.
        #[cfg(feature = "test_mode")]
        let read = if keys.private_key().is_none()
            && crate::za::test_hooks::take_private_key_fail(account_id)
        {
            ZaMarkRead {
                mark: ZaMark::Neither,
                private_key: None,
                ..read
            }
        } else {
            read
        };
        let ZaMarkRead {
            vault,
            mark,
            private_key,
        } = read;
        let (Some(vault), Some(private_key)) = (vault, private_key) else {
            if keys.set_private_key(Err(KeyError::Aead)) {
                trc::error!(private_key_unavailable(account_id));
            }
            return Err(private_key_unavailable(account_id));
        };
        let derived = derive_public_key(&private_key);
        let matches = vault.record.public_key.as_slice() == derived.as_slice();
        let check = ZaSelfCheck {
            stored_key_matches: *first_matches.get_or_insert(matches),
            public_key: derived,
        };
        let pins = match pin_read_store(store, account_id, keys.mk()).await {
            Ok(pins) => pins,
            Err(err) => {
                record_private_key(keys, Some(private_key));
                return Err(err);
            }
        };
        let rewrap = mark == ZaMark::Legacy && pins.is_usable() && (heal || !matches);
        if matches && !rewrap {
            record_private_key(keys, Some(private_key));
            return Ok((check, false));
        }
        if !matches && !traced {
            traced = true;
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = account_id,
                Details = "zero-access: stored public key does not match the private key",
            );
        }
        let mut record = vault.record.clone();
        if !matches {
            record.public_key = derived.to_vec();
        }
        if rewrap {
            record.private_key_wrap =
                wrap_private_key(&private_key, keys.mk(), account_id, WrapMark::Pinned);
        }
        // A usable record is rewritten at its next revision (spec 4.1);
        // without one nothing is written to field 151 (invariant 22).
        let write = pins.record.as_ref().map(|current| {
            let mut next = current.clone();
            next.revision = current.revision + 1;
            if !matches {
                next.key_repaired_at = Some(now() as i64);
            }
            ZaPinWrite {
                previous: &pins,
                sealed: seal_pin_record(&next, keys.mk(), account_id),
            }
        });
        match vault_commit_store(store, account_id, &mut record, Some(&vault), write).await {
            Ok(()) => {
                record_private_key(keys, Some(private_key));
                return Ok((check, true));
            }
            // The last attempt lost: the next session checks again.
            Err(err) if err.is_assertion_failure() && attempt >= ZA_PIN_WRITE_ATTEMPTS => {
                record_private_key(keys, Some(private_key));
                return Ok((check, false));
            }
            // Re-read and check again: a key that now matches writes
            // nothing.
            Err(err) if err.is_assertion_failure() => opened = Some(private_key),
            Err(err) => {
                record_private_key(keys, Some(private_key));
                return Err(err);
            }
        }
    }
}

/// Records an opened private key in the session; a no-op when nothing
/// opened or once an outcome is recorded (the first stands).
fn record_private_key(keys: &SessionKeys, private_key: Option<Secret>) {
    if let Some(private_key) = private_key {
        keys.set_private_key(Ok(private_key));
    }
}

fn private_key_unavailable(account_id: u32) -> trc::Error {
    corrupt(account_id, "zero-access: session private key does not open")
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

    /// Key pinning spec 3.3: `self_check_store`, then, when it wrote, the
    /// account's invalidation (the write's generation bump ends its cached
    /// logins and resident keys); an invalidation failure is logged, never
    /// returned. `heal` is the drain's first pass; the endpoints heal only
    /// by writing the record (spec 4.4).
    pub async fn za_self_check(
        &self,
        keys: &SessionKeys,
        heal: bool,
        once: bool,
    ) -> trc::Result<ZaSelfCheck> {
        let (check, wrote) = self_check_store(self.store(), keys, heal, once).await?;
        if wrote {
            self.za_invalidate_committed(keys.account_id).await;
        }
        Ok(check)
    }

    /// Invitations spec 3.2: the session's X25519 private key, from the
    /// self-check (`za_self_check(keys, true, true)`, key pinning spec 3.3),
    /// which runs once per session; a recorded outcome returns at once. The
    /// drain is the caller. A recorded failure (the wrap does not open, the
    /// record is missing or unusable) was traced once for the session and is
    /// an error. A store error after the key opened is traced here and the
    /// key returned (spec 8: the drain proceeds); one before it was not
    /// traced and is not recorded, so the caller traces it and a later call
    /// retries.
    pub async fn za_private_key<'k>(&self, keys: &'k SessionKeys) -> trc::Result<&'k Secret> {
        if let Err(err) = self.za_self_check(keys, true, true).await {
            if !matches!(keys.private_key(), Some(Ok(_))) {
                return Err(err);
            }
            trc::error!(
                err.account_id(keys.account_id)
                    .details("zero-access: self-check failed")
                    .caused_by(trc::location!())
            );
        }
        match keys.private_key() {
            Some(Ok(private_key)) => Ok(private_key),
            _ => Err(private_key_unavailable(keys.account_id)),
        }
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
    use ::vault::{
        keys::{AAD_PRIVATE_KEY, derive_kek, derive_verifier_hash, generate_keypair, wrap_key},
        pins::PinSource,
    };
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

    /// Mail spec 6.1: the recheck's abort removes only the bytes it read.
    #[tokio::test]
    async fn a_pending_record_is_deleted_only_at_its_own_revision() {
        let store = EphemeralStore::open();
        let mut record = VaultRecord::pending(vec![1; 32], 0);
        vault_commit_store(&store, 9, &mut record, None, None)
            .await
            .unwrap();
        let read = vault_read_store(&store, 9).await.unwrap().unwrap();
        let mut rotated = read.record.clone();
        vault_commit_store(&store, 9, &mut rotated, Some(&read), None)
            .await
            .unwrap();
        assert!(
            vault_delete_store(&store, 9, &read)
                .await
                .unwrap_err()
                .is_assertion_failure(),
            "stale cas"
        );
        let read = vault_read_store(&store, 9).await.unwrap().unwrap();
        assert_eq!(read.record.revision, 2, "the lost delete removed nothing");
        vault_delete_store(&store, 9, &read).await.unwrap();
        assert!(vault_read_store(&store, 9).await.unwrap().is_none());
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

    fn record_with_private_key(account_id: u32, mk: &Secret, private: &Secret) -> VaultRecord {
        let mut record = VaultRecord::pending(vec![], 0);
        record.state = VaultState::Active;
        record.private_key_wrap = wrap_key(private, mk, &aad(AAD_PRIVATE_KEY, account_id));
        record
    }

    #[tokio::test]
    async fn private_key_unwraps_from_the_record_once_per_session() {
        let store = EphemeralStore::open();
        let mk = Secret::random();
        let (public, private) = ::vault::keys::generate_keypair();
        let mut record = record_with_private_key(3, &mk, &private);
        record.public_key = public.to_vec();
        vault_write_store(&store, 3, &record, None).await.unwrap();

        let keys = SessionKeys::new(3, 1, mk.clone());
        assert!(keys.private_key().is_none());
        self_check_store(&store, &keys, false, true).await.unwrap();
        assert_eq!(
            keys.private_key().unwrap().unwrap().as_bytes(),
            private.as_bytes()
        );
        // Kept in the session: no second record read (the record is now
        // unusable, and the call still succeeds).
        write_raw(&store, 3, Vec::new()).await;
        self_check_store(&store, &keys, false, true).await.unwrap();
        assert_eq!(
            keys.private_key().unwrap().unwrap().as_bytes(),
            private.as_bytes()
        );
        // A new session reads the record again.
        assert!(
            self_check_store(&store, &SessionKeys::new(3, 1, mk.clone()), false, true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_private_key_wrap_that_does_not_open_is_an_error_and_no_key() {
        let store = EphemeralStore::open();
        let mk = Secret::random();
        let private = Secret::random();
        let mut record = record_with_private_key(3, &mk, &private);
        record.private_key_wrap[40] ^= 1;
        vault_write_store(&store, 3, &record, None).await.unwrap();

        let keys = SessionKeys::new(3, 1, mk.clone());
        let err = self_check_store(&store, &keys, false, true)
            .await
            .err()
            .unwrap();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)));
        assert!(format!("{err:?}").contains("zero-access:"), "{err:?}");
        assert!(matches!(keys.private_key(), Some(Err(_))), "no key is kept");

        // Recorded (and traced) once: repairing the record does not change
        // this session's outcome; a fresh session opens the repaired wrap.
        let read = vault_read_store(&store, 3).await.unwrap().unwrap();
        let mut repaired = record_with_private_key(3, &mk, &private);
        repaired.revision = read.record.revision + 1;
        vault_write_store(&store, 3, &repaired, Some(&read))
            .await
            .unwrap();
        assert!(self_check_store(&store, &keys, false, true).await.is_err());
        let fresh = SessionKeys::new(3, 1, mk.clone());
        self_check_store(&store, &fresh, false, true).await.unwrap();
        assert_eq!(
            fresh.private_key().unwrap().unwrap().as_bytes(),
            private.as_bytes()
        );

        // A missing record, and a wrap bound to another account.
        assert!(
            self_check_store(&store, &SessionKeys::new(4, 1, mk.clone()), false, true)
                .await
                .is_err()
        );
        let moved = record_with_private_key(3, &mk, &private);
        vault_write_store(&store, 5, &moved, None).await.unwrap();
        assert!(
            self_check_store(&store, &SessionKeys::new(5, 1, mk.clone()), false, true)
                .await
                .is_err()
        );
    }

    /// The store writer replaces the record's `public_key` at the next
    /// revision.
    async fn write_public_key(store: &Store, account_id: u32, key: &[u8; 32]) {
        let read = vault_read_store(store, account_id).await.unwrap().unwrap();
        let mut record = read.record.clone();
        record.public_key = key.to_vec();
        record.revision += 1;
        vault_write_store(store, account_id, &record, Some(&read))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_ungated_self_check_reads_the_store_again() {
        // An Active record with a matching public key; keys recorded by a
        // first gated call.
        let store = EphemeralStore::open();
        let mk = Secret::random();
        let (public, private) = generate_keypair();
        let (writer, _) = generate_keypair();
        let mut record = record_with_private_key(3, &mk, &private);
        record.public_key = public.to_vec();
        vault_write_store(&store, 3, &record, None).await.unwrap();
        let keys = SessionKeys::new(3, 1, mk.clone());

        self_check_store(&store, &keys, false, true).await.unwrap();
        write_public_key(&store, 3, &writer).await; // the stored key replaced at the next revision
        let (check, wrote) = self_check_store(&store, &keys, false, true).await.unwrap();
        assert!(
            check.stored_key_matches && !wrote,
            "gated: the recorded outcome, no read"
        );
        let (check, wrote) = self_check_store(&store, &keys, false, false).await.unwrap();
        assert!(
            !check.stored_key_matches && wrote,
            "ungated: compared and repaired"
        );
        assert_eq!(
            vault_read_store(&store, 3)
                .await
                .unwrap()
                .unwrap()
                .record
                .public_key,
            public.to_vec()
        );
        assert_eq!(
            keys.private_key().unwrap().unwrap().as_bytes(),
            private.as_bytes(),
            "the recorded outcome stays"
        );
    }

    fn one_pin() -> ZaPinRecord {
        let mut record = ZaPinRecord::new();
        record.set_pin("key2@example.com", [2; 32], PinSource::FirstUse, 100);
        record
    }

    fn two_pins() -> ZaPinRecord {
        let mut record = one_pin();
        record.set_pin("key3@example.com", [3; 32], PinSource::Confirmed, 200);
        record
    }

    async fn write_raw_pins(store: &Store, account_id: u32, value: Vec<u8>) {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(PrincipalField::ZeroAccessPins, value);
        store.write(batch.build_all()).await.unwrap();
    }

    #[tokio::test]
    async fn pin_writes_assert_absence_or_the_stored_bytes() {
        let (store, mk) = (EphemeralStore::open(), Secret::random());
        let absent = pin_read_store(&store, 5, &mk).await.unwrap();
        assert!(absent.raw.is_none() && matches!(absent.assertion(), AssertValue::None));
        pin_write_store(
            &store,
            5,
            ZaPinWrite {
                previous: &absent,
                sealed: seal_pin_record(&one_pin(), &mk, 5),
            },
        )
        .await
        .unwrap();
        let err = pin_write_store(
            &store,
            5,
            ZaPinWrite {
                previous: &absent,
                sealed: seal_pin_record(&one_pin(), &mk, 5),
            },
        )
        .await
        .unwrap_err();
        assert!(err.is_assertion_failure(), "a second create loses");
        let first = pin_read_store(&store, 5, &mk).await.unwrap();
        assert_eq!(first.record, Some(one_pin()));
        write_raw_pins(&store, 5, b"garbage".to_vec()).await;
        let garbage = pin_read_store(&store, 5, &mk).await.unwrap();
        assert!(garbage.raw.is_some() && !garbage.is_usable());
        let err = pin_write_store(
            &store,
            5,
            ZaPinWrite {
                previous: &first,
                sealed: seal_pin_record(&two_pins(), &mk, 5),
            },
        )
        .await
        .unwrap_err();
        assert!(err.is_assertion_failure(), "the bytes read before are gone");
        pin_write_store(
            &store,
            5,
            ZaPinWrite {
                previous: &garbage,
                sealed: seal_pin_record(&two_pins(), &mk, 5),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            pin_read_store(&store, 5, &mk).await.unwrap().record,
            Some(two_pins())
        );
    }

    #[tokio::test]
    async fn the_first_pin_batch_writes_both_fields_or_neither() {
        // A legacy-wrapped Active record at revision 3, read as `fresh`;
        // `stale` is a read taken at revision 2.
        let (store, mk) = (EphemeralStore::open(), Secret::random());
        let keys = SessionKeys::new(5, 1, mk.clone());
        let (public, private) = generate_keypair();
        let mut record = record_with_private_key(5, &mk, &private);
        record.public_key = public.to_vec();
        record.revision = 2;
        vault_write_store(&store, 5, &record, None).await.unwrap();
        let stale = vault_read_store(&store, 5).await.unwrap().unwrap();
        record.revision = 3;
        vault_write_store(&store, 5, &record, Some(&stale))
            .await
            .unwrap();
        let fresh = vault_read_store(&store, 5).await.unwrap().unwrap();
        let absent = pin_read_store(&store, 5, &mk).await.unwrap();
        let rewrapped = |read: &VaultRead| {
            let mut record = read.record.clone();
            record.private_key_wrap = wrap_private_key(&private, &mk, 5, WrapMark::Pinned);
            record
        };

        let err = vault_commit_store(
            &store,
            5,
            &mut rewrapped(&stale),
            Some(&stale),
            Some(ZaPinWrite {
                previous: &absent,
                sealed: seal_pin_record(&one_pin(), &mk, 5),
            }),
        )
        .await
        .unwrap_err();
        assert!(err.is_assertion_failure());
        assert!(
            pin_read_store(&store, 5, &mk).await.unwrap().raw.is_none(),
            "no record without the rewrap"
        );
        assert_eq!(
            mark_read_store(&store, &keys).await.unwrap().mark,
            ZaMark::Legacy
        );

        // The other direction: the vault read is fresh, the pin read is
        // stale (field 151 changed after it), so the batch loses on the pin
        // assertion and neither field changes.
        let changed = seal_pin_record(&two_pins(), &mk, 5);
        write_raw_pins(&store, 5, changed.clone()).await;
        let err = vault_commit_store(
            &store,
            5,
            &mut rewrapped(&fresh),
            Some(&fresh),
            Some(ZaPinWrite {
                previous: &absent,
                sealed: seal_pin_record(&one_pin(), &mk, 5),
            }),
        )
        .await
        .unwrap_err();
        assert!(err.is_assertion_failure());
        let mark = mark_read_store(&store, &keys).await.unwrap();
        let vault = mark.vault.as_ref().unwrap();
        assert_eq!(
            (mark.mark, vault.record.revision, vault.cas),
            (ZaMark::Legacy, 3, fresh.cas),
            "no rewrap without the record"
        );
        let current = pin_read_store(&store, 5, &mk).await.unwrap();
        assert_eq!(current.raw.as_deref(), Some(changed.as_slice()));

        vault_commit_store(
            &store,
            5,
            &mut rewrapped(&fresh),
            Some(&fresh),
            Some(ZaPinWrite {
                previous: &current,
                sealed: seal_pin_record(&one_pin(), &mk, 5),
            }),
        )
        .await
        .unwrap();
        let mark = mark_read_store(&store, &keys).await.unwrap();
        assert_eq!(
            (mark.mark, mark.vault.unwrap().record.revision),
            (ZaMark::Pinned, 4)
        );
        assert!(pin_read_store(&store, 5, &mk).await.unwrap().is_usable());
    }

    #[tokio::test]
    async fn the_mark_is_read_pinned_first_then_legacy() {
        // Records with a pinned wrap, a legacy wrap, a wrap of other bytes,
        // and no record.
        let (store, mk) = (EphemeralStore::open(), Secret::random());
        let (public, private) = generate_keypair();
        let wraps = [
            wrap_private_key(&private, &mk, 1, WrapMark::Pinned),
            wrap_private_key(&private, &mk, 2, WrapMark::Legacy),
            wrap_key(&private, &mk, &aad(AAD_PASSWORD, 3)),
        ];
        for (account_id, wrap) in (1..).zip(wraps) {
            let mut record = record_with_private_key(account_id, &mk, &private);
            record.private_key_wrap = wrap;
            vault_write_store(&store, account_id, &record, None)
                .await
                .unwrap();
        }
        let read = |account_id| {
            let keys = SessionKeys::new(account_id, 1, mk.clone());
            let store = store.clone();
            async move { mark_read_store(&store, &keys).await.unwrap() }
        };
        let (pinned, legacy, neither, missing) =
            (read(1).await, read(2).await, read(3).await, read(4).await);
        let marks = [pinned.mark, legacy.mark, neither.mark, missing.mark];
        assert_eq!(
            marks,
            [
                ZaMark::Pinned,
                ZaMark::Legacy,
                ZaMark::Neither,
                ZaMark::Neither
            ]
        );
        assert!(neither.private_key.is_none() && missing.vault.is_none());
        assert_eq!(
            derive_public_key(pinned.private_key.as_ref().unwrap()),
            public
        );
        assert_eq!(
            derive_public_key(legacy.private_key.as_ref().unwrap()),
            public
        );
    }

    #[tokio::test]
    async fn the_served_key_is_a_usable_public_key_or_nothing() {
        const ACTIVE: u32 = 1;
        const PENDING: u32 = 2;
        const MISSING: u32 = 3;
        const UNUSABLE: u32 = 4;
        const SHORT: u32 = 5;
        let store = EphemeralStore::open();
        let (public, _) = generate_keypair();
        let mut active = VaultRecord::pending(vec![], 0);
        active.state = VaultState::Active;
        active.public_key = public.to_vec();
        vault_write_store(&store, ACTIVE, &active, None)
            .await
            .unwrap();
        let mut pending = VaultRecord::pending(vec![1; 32], 100);
        pending.public_key = public.to_vec();
        vault_write_store(&store, PENDING, &pending, None)
            .await
            .unwrap();
        write_raw(&store, UNUSABLE, vec![VAULT_RECORD_VERSION + 1, 0, 0]).await;
        let mut short = active.clone();
        short.public_key.pop();
        vault_write_store(&store, SHORT, &short, None)
            .await
            .unwrap();

        assert_eq!(
            served_public_key_store(&store, ACTIVE).await.unwrap(),
            Some(public)
        );
        for id in [PENDING, MISSING, UNUSABLE, SHORT] {
            assert_eq!(
                served_public_key_store(&store, id).await.unwrap(),
                None,
                "{id}"
            );
        }
    }

    #[test]
    fn addresses_normalize_as_pins_are_keyed() {
        assert_eq!(
            za_normalize_address(" Bob@Example.COM ").as_deref(),
            Some("bob@example.com")
        );
        assert_eq!(
            za_normalize_address("bob@bücher.test").as_deref(),
            Some("bob@xn--bcher-kva.test")
        );
        for bad in ["mailto:bob@example.com", "Bob <bob@example.com>", "bob", ""] {
            assert_eq!(za_normalize_address(bad), None, "{bad}");
        }
    }
}
