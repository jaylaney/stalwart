/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    KV_OAUTH, Server,
    auth::{
        oauth::{grant::GrantDeletion, token::TOKEN_HEADER},
        vault::{RawValue, vault_read_usable_store},
    },
    cache::invalidate::CacheInvalidationBuilder,
    ipc::{BroadcastEvent, CacheInvalidation},
};
use registry::schema::prelude::ObjectType;
use std::{
    collections::{HashMap, hash_map::Entry},
    fmt,
    sync::Arc,
};
use store::{
    Deserialize, Serialize, Store, Value, ValueKey,
    dispatch::lookup::KeyValue,
    registry::RegistryQuery,
    roaring::RoaringBitmap,
    write::{
        Archive, ArchiveBytes, Archiver, BatchBuilder, Compression, ValueClass,
        assert::AssertValue, now,
    },
};
use trc::AddContext;
use types::{collection::Collection, field::PrincipalField};
use vault::{
    keys::Secret,
    record::za_session_binding,
    session::SessionKeys,
    tokens::{
        TOKEN_RECORD_VERSION, ZaGrantState, ZaRefreshClass, ZaSuffix, ZaTokenRecord, open_handoff,
        seal_handoff, split_secret,
    },
};
use xxhash_rust::xxh3::xxh3_64;

const TOKEN_WRITE_ATTEMPTS: usize = 3;

/// Split only tokens that carry the server's header (spec 4.2, D26).
pub fn za_split_bearer(token: &str) -> (&str, ZaSuffix) {
    split_secret(token, TOKEN_HEADER)
}

pub struct ZaIssued {
    pub access: Secret,
    pub refresh: Secret,
}

pub enum ZaTokenIssue {
    Issued(ZaIssued),
    VaultChanged,
}

pub enum ZaTokenRefresh {
    Issued(ZaIssued),
    Superseded,
    Lost,
    VaultChanged,
}

pub enum ZaRefreshPrecheck {
    Keyless,
    Keyed,
    Refused,
}

pub struct ZaHandoff {
    pub account_id: u32,
    pub binding: u64,
    pub mk: Secret,
}

// In-memory reads require Debug, but the hand-off holds a wrapped master key.
struct HandoffValue(Vec<u8>);

impl fmt::Debug for HandoffValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl Deserialize for HandoffValue {
    fn deserialize(bytes: &[u8]) -> trc::Result<Self> {
        Ok(Self(bytes.to_vec()))
    }
}

impl From<Value<'static>> for HandoffValue {
    fn from(value: Value<'static>) -> Self {
        match value {
            Value::Blob(bytes) => Self(bytes.into_owned()),
            _ => Self(Vec::new()),
        }
    }
}

fn handoff_key(key_id: &str) -> Vec<u8> {
    KeyValue::<()>::build_key(KV_OAUTH, [b"~", key_id.as_bytes()].concat())
}

fn sign_in_vault_changed(account_id: u32) -> trc::Error {
    trc::AuthEvent::Failed
        .into_err()
        .details("zero-access: vault changed during sign-in")
        .account_id(account_id)
        .caused_by(trc::location!())
}

pub struct ZaTokenRead {
    pub raw: Option<Vec<u8>>,
    /// `None` when the record is absent or unusable.
    pub record: Option<ZaTokenRecord>,
}

impl ZaTokenRead {
    pub fn assertion(&self) -> AssertValue {
        self.raw
            .as_deref()
            .map_or(AssertValue::None, |raw| AssertValue::Hash(xxh3_64(raw)))
    }
}

pub(crate) fn tokens_key(account_id: u32) -> ValueKey<ValueClass> {
    ValueKey::property(
        account_id,
        Collection::Principal,
        0,
        PrincipalField::ZeroAccessTokens,
    )
}

pub(crate) async fn token_read_store(store: &Store, account_id: u32) -> trc::Result<ZaTokenRead> {
    let raw = store
        .get_value::<RawValue>(tokens_key(account_id))
        .await
        .caused_by(trc::location!())?
        .map(|raw| raw.0);
    let record = match raw.as_deref().and_then(|bytes| bytes.split_first()) {
        Some((&TOKEN_RECORD_VERSION, archive)) => {
            // Keep unusable bytes for the next assertion. Discard the store's
            // decode errors since they can carry the record's wraps.
            <Archive<ArchiveBytes> as Deserialize>::deserialize(archive)
                .and_then(|archive| archive.deserialize_untrusted::<ZaTokenRecord>())
                .ok()
                .filter(|record| record.version == TOKEN_RECORD_VERSION)
        }
        _ => None,
    };
    Ok(ZaTokenRead { raw, record })
}

/// A write with wraps asserts both records. A removal alone needs only the
/// token assertion and may retain existing entries under the same binding.
pub(crate) async fn token_write_store(
    store: &Store,
    account_id: u32,
    vault_cas: Option<u64>,
    previous: &ZaTokenRead,
    record: Option<&ZaTokenRecord>,
) -> trc::Result<()> {
    let record = record.filter(|record| !record.is_empty());
    if let Some(record) = record
        && vault_cas.is_none()
        && !previous.record.as_ref().is_some_and(|previous| {
            record.version == previous.version && record.binding == previous.binding && {
                let mut old_grants = previous.grants.iter();
                record.grants.iter().all(|grant| {
                    old_grants
                        .find(|old| old.grant_id == grant.grant_id)
                        .is_some_and(|old| {
                            retained_entries(&grant.refresh, &old.refresh)
                                && retained_entries(&grant.access, &old.access)
                        })
                })
            }
        })
    {
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access: a token-record write without a vault assertion may only remove entries")
            .account_id(account_id)
            .caused_by(trc::location!()));
    }

    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    if let Some(cas) = vault_cas {
        batch.assert_value(PrincipalField::ZeroAccessVault, AssertValue::Hash(cas));
    }
    batch.assert_value(PrincipalField::ZeroAccessTokens, previous.assertion());
    if let Some(record) = record {
        let archive = Archiver::with_compression(record.clone(), Compression::None)
            .serialize()
            .caused_by(trc::location!())?;
        let mut bytes = Vec::with_capacity(archive.len() + 1);
        bytes.push(TOKEN_RECORD_VERSION);
        bytes.extend_from_slice(&archive);
        batch.set(PrincipalField::ZeroAccessTokens, bytes);
    } else {
        batch.clear(PrincipalField::ZeroAccessTokens);
    }
    store.write_batch(&mut batch).await.map(|_| ())
}

/// Match each entry once so a removal cannot introduce copies of a wrap.
fn retained_entries<T: PartialEq>(entries: &[T], previous: &[T]) -> bool {
    let mut previous = previous.iter();
    entries.iter().all(|entry| previous.any(|old| old == entry))
}

impl Server {
    pub async fn za_token_read(&self, account_id: u32) -> trc::Result<ZaTokenRead> {
        token_read_store(self.store(), account_id).await
    }

    pub async fn za_handoff_put(
        &self,
        key_id: &str,
        account_id: u32,
        keys: &SessionKeys,
        expires: u64,
    ) -> trc::Result<Secret> {
        let Some(read) = self.za_vault_record(account_id).await?.filter(|read| {
            keys.account_id == account_id && read.record.revision == keys.generation
        }) else {
            return Err(sign_in_vault_changed(account_id));
        };
        let (secret, value) = seal_handoff(
            keys.mk(),
            account_id,
            za_session_binding(&read.record),
            key_id,
        );

        #[cfg(feature = "test_mode")]
        super::test_hooks::token_handoff_write_pause_point(account_id).await;
        self.in_memory_store()
            .key_set(KeyValue::new(handoff_key(key_id), value).expires(expires))
            .await?;
        // The entry is provisional until the vault snapshot is asserted
        // after the in-memory write (D6). This batch mutates nothing.
        let mut fence = BatchBuilder::new();
        fence
            .with_account_id(account_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .assert_value(PrincipalField::ZeroAccessVault, AssertValue::Hash(read.cas));
        if let Err(err) = self.store().write_batch(&mut fence).await {
            let _ = self.za_handoff_delete(key_id).await;
            return Err(if err.is_assertion_failure() {
                sign_in_vault_changed(account_id)
            } else {
                err
            });
        }

        Ok(secret)
    }

    pub async fn za_handoff_take(
        &self,
        key_id: &str,
        secret: &Secret,
    ) -> trc::Result<Option<ZaHandoff>> {
        let store = self.in_memory_store();
        let key = handoff_key(key_id);
        let Some(value) = store.key_get::<HandoffValue>(key.as_slice()).await? else {
            return Ok(None);
        };
        if !store.key_delete(key).await? {
            return Ok(None);
        }
        Ok(
            open_handoff(&value.0, secret, key_id).map(|(account_id, binding, mk)| ZaHandoff {
                account_id,
                binding,
                mk,
            }),
        )
    }

    pub async fn za_handoff_delete(&self, key_id: &str) -> trc::Result<()> {
        self.in_memory_store()
            .key_delete(handoff_key(key_id))
            .await
            .map(|_| ())
    }

    /// D22: classify the presented suffix before upstream rotates the grant.
    pub async fn za_token_refresh_precheck(
        &self,
        account_id: u32,
        grant_id: u64,
        suffix: &ZaSuffix,
    ) -> trc::Result<ZaRefreshPrecheck> {
        let read = token_read_store(self.store(), account_id).await?;
        Ok(match suffix {
            ZaSuffix::Secret(_) => ZaRefreshPrecheck::Keyed,
            ZaSuffix::None
                if read
                    .record
                    .as_ref()
                    .and_then(|record| record.grant(grant_id))
                    .is_none() =>
            {
                ZaRefreshPrecheck::Keyless
            }
            ZaSuffix::None | ZaSuffix::Malformed => ZaRefreshPrecheck::Refused,
        })
    }

    /// Spec 4.4: issue the first token entries under the binding of the MK.
    pub async fn za_token_issue(
        &self,
        account_id: u32,
        grant_id: u64,
        mk: &Secret,
        binding: u64,
        access_expires: u64,
        refresh_expires: u64,
    ) -> trc::Result<ZaTokenIssue> {
        let store = self.store();
        for remaining in (0..TOKEN_WRITE_ATTEMPTS).rev() {
            let Some(vault) = vault_read_usable_store(store, account_id).await? else {
                return Ok(ZaTokenIssue::VaultChanged);
            };
            if za_session_binding(&vault.record) != binding {
                return Ok(ZaTokenIssue::VaultChanged);
            }

            let read = token_read_store(store, account_id).await?;
            let mut record = read
                .record
                .as_ref()
                .filter(|record| record.binding == binding)
                .cloned()
                .unwrap_or_else(|| ZaTokenRecord::new(binding));
            let grants = self
                .token_grant_states(account_id, &record, Some(grant_id))
                .await?;
            let now = now();
            record.reconcile(account_id, now, &grants);
            let evicted = record.eviction(grant_id, &grants);
            if let Some(evicted) = evicted {
                record.remove_grant(evicted);
            }

            let refresh = Secret::random();
            record.put_refresh(account_id, grant_id, mk, None, 0, &refresh, refresh_expires);
            let access = record.put_access(account_id, grant_id, mk, access_expires, now);

            #[cfg(feature = "test_mode")]
            super::test_hooks::token_exchange_pause_point(account_id).await;
            match token_write_store(store, account_id, Some(vault.cas), &read, Some(&record)).await
            {
                Err(err) if remaining > 0 && err.is_assertion_failure() => continue,
                result => result?,
            }

            if let Some(evicted) = evicted {
                self.za_delete_evicted_grant(account_id, evicted).await;
            }
            return Ok(ZaTokenIssue::Issued(ZaIssued { access, refresh }));
        }
        unreachable!("the last write attempt returns");
    }

    /// Spec 4.5 steps 2 to 5, after `rotate_oauth_grant` issued `next`.
    /// Every attempt decides from its own reads: MK comes only from the
    /// presented entry of the record that attempt read.
    pub async fn za_token_refresh(
        &self,
        account_id: u32,
        grant_id: u64,
        presented: u64,
        secret: &Secret,
        next: u64,
        access_expires: u64,
        refresh_expires: u64,
    ) -> trc::Result<ZaTokenRefresh> {
        #[cfg(feature = "test_mode")]
        super::test_hooks::token_refresh_read_pause_point(account_id).await;

        let store = self.store();
        let mut first_binding = None;
        for remaining in (0..TOKEN_WRITE_ATTEMPTS).rev() {
            let Some(vault) = vault_read_usable_store(store, account_id).await? else {
                return Ok(ZaTokenRefresh::VaultChanged);
            };
            let binding = za_session_binding(&vault.record);
            if *first_binding.get_or_insert(binding) != binding {
                return Ok(ZaTokenRefresh::VaultChanged);
            }

            let read = token_read_store(store, account_id).await?;
            let Some(mut record) = read.record.clone() else {
                return Ok(ZaTokenRefresh::Lost);
            };
            if record.binding != binding {
                return Ok(ZaTokenRefresh::VaultChanged);
            }

            let grants = self.token_grant_states(account_id, &record, None).await?;
            // This request's rotation left the grant's stored generation at
            // `presented`, and the generation never falls.
            // A higher one is a later rotation: this refresh is superseded,
            // however old this attempt's record read is (spec 4.5 step 2).
            if grants
                .get(&grant_id)
                .and_then(Option::as_ref)
                .is_some_and(|state| state.generation > presented)
            {
                return Ok(ZaTokenRefresh::Superseded);
            }
            let now = now();
            record.reconcile(account_id, now, &grants);
            let (mk, sealed) =
                match record.classify_refresh(account_id, grant_id, presented, secret, now) {
                    ZaRefreshClass::Opened { mk, next } => (mk, next),
                    ZaRefreshClass::Superseded => return Ok(ZaTokenRefresh::Superseded),
                    ZaRefreshClass::Lost => return Ok(ZaTokenRefresh::Lost),
                };

            // A retry finds the next secret sealed in the presented entry.
            let next_secret = sealed.unwrap_or_else(Secret::random);
            record.put_refresh(
                account_id,
                grant_id,
                &mk,
                Some((presented, secret)),
                next,
                &next_secret,
                refresh_expires,
            );
            let access = record.put_access(account_id, grant_id, &mk, access_expires, now);

            #[cfg(feature = "test_mode")]
            {
                super::test_hooks::token_refresh_write_pause_point(account_id).await;
                if super::test_hooks::take_token_refresh_write_fail(account_id) {
                    return Err(trc::StoreEvent::UnexpectedError
                        .into_err()
                        .details("zero-access test hook: token refresh write failure"));
                }
            }
            match token_write_store(store, account_id, Some(vault.cas), &read, Some(&record)).await
            {
                Err(err) if remaining > 0 && err.is_assertion_failure() => continue,
                result => result?,
            }
            return Ok(ZaTokenRefresh::Issued(ZaIssued {
                access,
                refresh: next_secret,
            }));
        }
        unreachable!("the last write attempt returns");
    }

    /// Spec 4.6: attach the vault revision of this read to the opened MK.
    pub async fn za_token_open_access(
        &self,
        account_id: u32,
        grant_id: u64,
        secret: &Secret,
    ) -> trc::Result<Option<Arc<SessionKeys>>> {
        let store = self.store();
        let Some(record) = token_read_store(store, account_id).await?.record else {
            return Ok(None);
        };
        let Some(mk) = record.open_access(account_id, grant_id, secret, now()) else {
            return Ok(None);
        };
        let Some(vault) = vault_read_usable_store(store, account_id).await? else {
            return Ok(None);
        };
        if record.binding != za_session_binding(&vault.record) {
            return Ok(None);
        }
        Ok(Some(Arc::new(SessionKeys::new(
            account_id,
            vault.record.revision,
            mk,
        ))))
    }

    /// Spec 4.7: remove a grant's entries with only the token assertion.
    pub async fn za_token_forget_grant(&self, account_id: u32, grant_id: u64) -> trc::Result<()> {
        #[cfg(feature = "test_mode")]
        super::test_hooks::token_forget_pause_point(account_id).await;
        #[cfg(feature = "test_mode")]
        if super::test_hooks::take_token_forget_fail(account_id) {
            return Err(trc::StoreEvent::UnexpectedError.into_err());
        }

        let store = self.store();
        for remaining in (0..TOKEN_WRITE_ATTEMPTS).rev() {
            let read = token_read_store(store, account_id).await?;
            let Some(mut record) = read.record.clone() else {
                return Ok(());
            };
            if !record.remove_grant(grant_id) {
                return Ok(());
            }
            let grants = self.token_grant_states(account_id, &record, None).await?;
            record.reconcile(account_id, now(), &grants);
            match token_write_store(store, account_id, None, &read, Some(&record)).await {
                Err(err) if remaining > 0 && err.is_assertion_failure() => continue,
                result => return result,
            }
        }
        unreachable!("the last write attempt returns");
    }

    /// Spec 4.7 (D8, D21): reconcile each account's token record without
    /// opening wraps or reading the vault. Count only committed changes.
    pub async fn za_token_sweep(&self) -> trc::Result<usize> {
        let accounts = self
            .registry()
            .query::<RoaringBitmap>(RegistryQuery::new(ObjectType::Account))
            .await
            .caused_by(trc::location!())?;
        let store = self.store();
        let mut changed = 0;
        for account_id in accounts {
            let result = async {
                let read = token_read_store(store, account_id).await?;
                let Some(mut record) = read.record.clone() else {
                    // This also leaves records in an unknown format alone.
                    return Ok(false);
                };
                let grants = self.token_grant_states(account_id, &record, None).await?;
                if !record.reconcile(account_id, now(), &grants) {
                    return Ok(false);
                }
                match token_write_store(store, account_id, None, &read, Some(&record)).await {
                    Ok(()) => Ok(true),
                    // Another writer won the assertion. Do not retry.
                    Err(err) if err.is_assertion_failure() => Ok(false),
                    Err(err) => Err(err),
                }
            }
            .await;
            match result {
                Ok(true) => changed += 1,
                Ok(false) => (),
                Err(err) => {
                    // Backend error context can contain stored bytes.
                    trc::error!(
                        trc::Error::new(err.event_type())
                            .account_id(account_id)
                            .details("zero-access: token sweep failed for an account")
                            .caused_by(trc::location!())
                    );
                }
            }
        }
        Ok(changed)
    }

    /// PR #90 review C4617611602: a deleted grant's sessions are evicted on
    /// this node and the eviction broadcast before the token record's cleanup
    /// is awaited, so a slow or interrupted cleanup cannot delay it. The
    /// caller's queued invalidation applies the same eviction again later; it
    /// is idempotent.
    pub(crate) async fn za_evict_deleted_grant(&self, account_id: u32, grant_id: u64) {
        let evicted = vec![CacheInvalidation::OAuthGrant {
            account_id,
            grant_id,
        }];
        self.invalidate_local_caches(&evicted);
        self.cluster_broadcast(BroadcastEvent::CacheInvalidate(evicted))
            .await;
    }

    /// Grant deletion classifies the account before touching the token record.
    pub(crate) async fn za_forget_deleted_grant(&self, account_id: u32, grant_id: u64) {
        let result = match self.za_is_key_account(account_id).await {
            Ok(true) => self.za_token_forget_grant(account_id, grant_id).await,
            Ok(false) => return,
            Err(err) => Err(err),
        };
        if result.is_err() {
            trc::error!(
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .account_id(account_id)
                    .id(grant_id)
                    .details("zero-access: token entries not removed with their grant")
            );
        }
    }

    async fn token_grant_states(
        &self,
        account_id: u32,
        record: &ZaTokenRecord,
        required_grant: Option<u64>,
    ) -> trc::Result<HashMap<u64, Option<ZaGrantState>>> {
        let mut states =
            HashMap::with_capacity(record.grants.len() + usize::from(required_grant.is_some()));
        for grant_id in required_grant
            .into_iter()
            .chain(record.grants.iter().map(|grant| grant.grant_id))
        {
            let Entry::Vacant(entry) = states.entry(grant_id) else {
                continue;
            };
            let state = self
                .oauth_grant(grant_id)
                .await?
                .map(|stored| ZaGrantState {
                    account_id: stored.account_id(),
                    generation: stored.grant.generation,
                    last_used_at: stored.grant.last_used_at.timestamp(),
                });
            if required_grant == Some(grant_id)
                && !state
                    .as_ref()
                    .is_some_and(|state| state.account_id == account_id)
            {
                return Err(trc::StoreEvent::UnexpectedError
                    .into_err()
                    .account_id(account_id)
                    .id(grant_id)
                    .details("zero-access: OAuth grant not found"));
            }
            entry.insert(state);
        }
        Ok(states)
    }

    async fn za_delete_evicted_grant(&self, account_id: u32, grant_id: u64) {
        let result = async {
            if let Some(stored) = self.oauth_grant(grant_id).await?
                && stored.account_id() == account_id
            {
                let mut invalidator = CacheInvalidationBuilder::default();
                self.delete_oauth_grant(stored, GrantDeletion::Unconditional, &mut invalidator)
                    .await?;
                self.invalidate_caches(invalidator).await?;
            }
            Ok::<(), trc::Error>(())
        }
        .await;
        if result.is_err() {
            trc::error!(
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .account_id(account_id)
                    .id(grant_id)
                    .details("zero-access: evicted grant not deleted")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{token_read_store, token_write_store};
    use crate::auth::vault::{VaultRead, vault_read_store, vault_write_store};
    use store::{
        Serialize, Store,
        backend::ephemeral::EphemeralStore,
        write::{Archiver, BatchBuilder, Compression, assert::AssertValue},
    };
    use types::{collection::Collection, field::PrincipalField};
    use vault::{
        keys::Secret,
        record::{VAULT_RECORD_VERSION, VaultRecord},
        tokens::{TOKEN_RECORD_VERSION, ZaSuffix, ZaTokenRecord, append_secret},
    };

    use xxhash_rust::xxh3::xxh3_64;

    const ACCOUNT: u32 = 5;

    /// The hand-off's in-memory value: read from a blob, empty from any other
    /// value, and silent in `Debug`, since it holds a wrapped master key.
    #[test]
    fn a_hand_off_value_is_read_from_a_blob_and_redacted() {
        use super::HandoffValue;
        use std::borrow::Cow;
        use store::{Deserialize, Value};

        let value = HandoffValue::from(Value::Blob(Cow::Owned(b"wrapped".to_vec())));
        assert_eq!(value.0, b"wrapped");
        assert_eq!(format!("{value:?}"), "[redacted]");
        assert!(HandoffValue::from(Value::Integer(5)).0.is_empty());
        assert_eq!(HandoffValue::deserialize(b"stored").unwrap().0, b"stored");
    }

    #[test]
    fn a_value_without_the_token_header_is_not_split() {
        let (opaque, suffix) = super::za_split_bearer("opaque~value");
        assert!(opaque == "opaque~value");
        assert!(matches!(suffix, ZaSuffix::None));

        let secret = Secret::random();
        let mut keyed = "sw1.AAAA.BBBB".to_owned();
        append_secret(&mut keyed, &secret);
        let (upstream, suffix) = super::za_split_bearer(&keyed);
        assert!(upstream == "sw1.AAAA.BBBB");
        assert!(
            matches!(suffix, ZaSuffix::Secret(opened) if opened.as_bytes() == secret.as_bytes())
        );
    }

    async fn pending_vault(store: &Store) -> VaultRead {
        let record = VaultRecord::pending(vec![1; 32], 100);
        vault_write_store(store, ACCOUNT, &record, None)
            .await
            .unwrap();
        vault_read_store(store, ACCOUNT).await.unwrap().unwrap()
    }

    fn record_of(grants: &[u64]) -> ZaTokenRecord {
        let mut record = ZaTokenRecord::new(1);
        let mk = Secret::random();
        for &grant in grants {
            record.put_refresh(ACCOUNT, grant, &mk, None, 0, &Secret::random(), 1_000);
        }
        record
    }

    async fn write_raw(store: &Store, field: PrincipalField, raw: &[u8]) {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(ACCOUNT)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(field, raw.to_vec());
        store.write_batch(&mut batch).await.unwrap();
    }

    #[tokio::test]
    async fn token_record_round_trips_and_asserts() {
        let store = EphemeralStore::open();
        let vault = pending_vault(&store).await;
        let absent = token_read_store(&store, ACCOUNT).await.unwrap();
        assert!(absent.raw.is_none() && absent.record.is_none());
        assert_eq!(absent.assertion(), AssertValue::None);

        let record = record_of(&[9]);
        token_write_store(&store, ACCOUNT, Some(vault.cas), &absent, Some(&record))
            .await
            .unwrap();
        let written = token_read_store(&store, ACCOUNT).await.unwrap();
        assert!(written.record.as_ref() == Some(&record));
        let raw = written.raw.as_deref().unwrap();
        assert_eq!(raw[0], TOKEN_RECORD_VERSION);
        assert_eq!(written.assertion(), AssertValue::Hash(xxh3_64(raw)));

        assert!(
            token_write_store(
                &store,
                ACCOUNT,
                Some(vault.cas),
                &absent,
                Some(&record_of(&[10]))
            )
            .await
            .unwrap_err()
            .is_assertion_failure()
        );
        assert!(token_read_store(&store, ACCOUNT).await.unwrap().raw == written.raw);

        token_write_store(
            &store,
            ACCOUNT,
            None,
            &written,
            Some(&ZaTokenRecord::new(1)),
        )
        .await
        .unwrap();
        assert!(
            token_read_store(&store, ACCOUNT)
                .await
                .unwrap()
                .raw
                .is_none()
        );
    }

    #[tokio::test]
    async fn record_write_loses_to_vault_write() {
        let store = EphemeralStore::open();
        let before = pending_vault(&store).await;
        let absent = token_read_store(&store, ACCOUNT).await.unwrap();
        let tokens = record_of(&[9]);
        token_write_store(&store, ACCOUNT, Some(before.cas), &absent, Some(&tokens))
            .await
            .unwrap();

        // A noncredential vault write keeps the token record (spec D19).
        let mut vault = before.record.clone();
        vault.revision += 1;
        vault_write_store(&store, ACCOUNT, &vault, Some(&before))
            .await
            .unwrap();
        let kept = token_read_store(&store, ACCOUNT).await.unwrap();
        assert!(kept.record.as_ref() == Some(&tokens));
        assert!(
            token_write_store(
                &store,
                ACCOUNT,
                Some(before.cas),
                &kept,
                Some(&record_of(&[10]))
            )
            .await
            .unwrap_err()
            .is_assertion_failure()
        );
        assert!(token_read_store(&store, ACCOUNT).await.unwrap().raw == kept.raw);

        let current = vault_read_store(&store, ACCOUNT).await.unwrap().unwrap();
        vault.revision += 1;
        vault.verifier_hash = vec![2; 32];
        vault_write_store(&store, ACCOUNT, &vault, Some(&current))
            .await
            .unwrap();
        assert!(
            token_read_store(&store, ACCOUNT)
                .await
                .unwrap()
                .raw
                .is_none()
        );
    }

    #[tokio::test]
    async fn removal_write_asserts_the_record_alone() {
        let store = EphemeralStore::open();
        let before = pending_vault(&store).await;
        let absent = token_read_store(&store, ACCOUNT).await.unwrap();
        let mut record = record_of(&[9, 10]);
        token_write_store(&store, ACCOUNT, Some(before.cas), &absent, Some(&record))
            .await
            .unwrap();
        let previous = token_read_store(&store, ACCOUNT).await.unwrap();
        write_raw(
            &store,
            PrincipalField::ZeroAccessVault,
            &[VAULT_RECORD_VERSION, 0xff],
        )
        .await;

        assert!(record.remove_grant(9));
        assert!(
            token_write_store(&store, ACCOUNT, Some(before.cas), &previous, Some(&record))
                .await
                .unwrap_err()
                .is_assertion_failure()
        );
        token_write_store(&store, ACCOUNT, None, &previous, Some(&record))
            .await
            .unwrap();
        assert!(
            token_read_store(&store, ACCOUNT)
                .await
                .unwrap()
                .record
                .as_ref()
                == Some(&record)
        );

        // A concurrent removal loses on the token assertion even with no
        // vault assertion; it must not delete the remaining grant.
        assert!(
            token_write_store(&store, ACCOUNT, None, &previous, None)
                .await
                .unwrap_err()
                .is_assertion_failure()
        );
        assert!(
            token_read_store(&store, ACCOUNT)
                .await
                .unwrap()
                .record
                .as_ref()
                == Some(&record)
        );
    }

    #[tokio::test]
    async fn removal_write_cannot_add_an_entry() {
        let store = EphemeralStore::open();
        let vault = pending_vault(&store).await;
        let absent = token_read_store(&store, ACCOUNT).await.unwrap();
        let record = record_of(&[9]);
        let unexpected = trc::EventType::Store(trc::StoreEvent::UnexpectedError);
        assert!(
            token_write_store(&store, ACCOUNT, None, &absent, Some(&record))
                .await
                .unwrap_err()
                .matches(unexpected)
        );
        token_write_store(&store, ACCOUNT, Some(vault.cas), &absent, Some(&record))
            .await
            .unwrap();
        let previous = token_read_store(&store, ACCOUNT).await.unwrap();

        let mut new_access = record.clone();
        new_access.put_access(ACCOUNT, 9, &Secret::random(), 1_000, 0);
        let mut new_grant = record.clone();
        new_grant
            .grants
            .push(record_of(&[10]).grants.pop().unwrap());
        let mut rebound = record.clone();
        rebound.binding += 1;
        let mut duplicate = record.clone();
        duplicate.grants[0]
            .refresh
            .push(record.grants[0].refresh[0].clone());
        for changed in [new_access, new_grant, record_of(&[9]), rebound, duplicate] {
            assert!(
                token_write_store(&store, ACCOUNT, None, &previous, Some(&changed))
                    .await
                    .unwrap_err()
                    .matches(unexpected)
            );
            assert!(token_read_store(&store, ACCOUNT).await.unwrap().raw == previous.raw);
        }
    }

    #[tokio::test]
    async fn unusable_bytes_read_as_no_record() {
        let store = EphemeralStore::open();
        let vault = pending_vault(&store).await;
        let record = record_of(&[9]);
        let mut unknown_inner = record.clone();
        unknown_inner.version += 1;
        let mut unknown_inner_archive = vec![TOKEN_RECORD_VERSION];
        unknown_inner_archive.extend_from_slice(
            &Archiver::with_compression(unknown_inner, Compression::None)
                .serialize()
                .unwrap(),
        );
        for bytes in [
            &[1, 0xde, 0xad][..],
            &[2, 0, 0],
            &[1],
            &unknown_inner_archive,
        ] {
            write_raw(&store, PrincipalField::ZeroAccessTokens, bytes).await;
            let read = token_read_store(&store, ACCOUNT).await.unwrap();
            assert!(read.record.is_none());
            assert!(read.raw.as_deref() == Some(bytes));
            assert_eq!(read.assertion(), AssertValue::Hash(xxh3_64(bytes)));
            token_write_store(&store, ACCOUNT, Some(vault.cas), &read, Some(&record))
                .await
                .unwrap();
            assert!(
                token_read_store(&store, ACCOUNT)
                    .await
                    .unwrap()
                    .record
                    .as_ref()
                    == Some(&record)
            );
        }
    }
}
