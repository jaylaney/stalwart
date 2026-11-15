/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Server, auth::vault::RawValue};
use store::{
    Deserialize, Serialize, Store, ValueKey,
    write::{
        Archive, ArchiveBytes, Archiver, BatchBuilder, Compression, ValueClass, assert::AssertValue,
    },
};
use trc::AddContext;
use types::{collection::Collection, field::PrincipalField};
use vault::tokens::{TOKEN_RECORD_VERSION, ZaTokenRecord};
use xxhash_rust::xxh3::xxh3_64;

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
#[allow(dead_code)] // Task 15 supplies the first production caller.
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
        tokens::{TOKEN_RECORD_VERSION, ZaTokenRecord},
    };

    use xxhash_rust::xxh3::xxh3_64;

    const ACCOUNT: u32 = 5;

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
        new_access.put_access(ACCOUNT, 9, &Secret::random(), 1_000);
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
