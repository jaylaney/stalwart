/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Server, cache::invalidate::CacheInvalidationBuilder, ipc::CacheInvalidation};
use ::vault::record::{VAULT_RECORD_VERSION, VaultRecord};
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
    /// xxh3 of the stored bytes; pass back to `za_vault_write` as `expected_cas`.
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
        Some((&version, _)) => {
            return Err(trc::StoreEvent::DataCorruption
                .into_err()
                .details("unsupported zero-access vault record version")
                .account_id(account_id)
                .id(u64::from(version))
                .caused_by(trc::location!()));
        }
        None => return Err(vault_corrupted(account_id)),
    };
    let record = <Archive<AlignedBytes> as Deserialize>::deserialize(archived)
        .and_then(|archive| archive.deserialize::<VaultRecord>())
        .map_err(|_| vault_corrupted(account_id))?;
    if record.version != VAULT_RECORD_VERSION {
        return Err(trc::StoreEvent::DataCorruption
            .into_err()
            .details("unsupported zero-access vault record version")
            .account_id(account_id)
            .id(u64::from(record.version))
            .caused_by(trc::location!()));
    }
    Ok(Some(VaultRead { record, cas }))
}

pub(crate) async fn vault_write_store(
    store: &Store,
    account_id: u32,
    record: &VaultRecord,
    expected_cas: Option<u64>,
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
            expected_cas.map_or(AssertValue::None, AssertValue::Hash),
        )
        .set(PrincipalField::ZeroAccessVault, value);
    store.write(batch.build_all()).await.map(|_| ())
}

impl Server {
    pub async fn za_vault_record(&self, account_id: u32) -> trc::Result<Option<VaultRead>> {
        vault_read_store(self.store(), account_id).await
    }

    /// Conditional write (spec 3.1). `expected_cas: None` means "must not
    /// exist". A lost race surfaces as `StoreEvent::AssertValueFailed`
    /// (`err.is_assertion_failure()`), which endpoints map to 409.
    pub async fn za_vault_write(
        &self,
        account_id: u32,
        record: &VaultRecord,
        expected_cas: Option<u64>,
    ) -> trc::Result<()> {
        vault_write_store(self.store(), account_id, record, expected_cas).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::vault::record::VaultState;
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
            vault_write_store(&store, 5, &record, Some(read.cas))
                .await
                .is_ok()
        );
        assert!(
            vault_write_store(&store, 5, &record, Some(read.cas))
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
    }
}
