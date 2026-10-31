/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Object metadata at provisioning (ruling 66; the Task 6 review's F1).
//!
//! 1.0 keeps object metadata in `Collection::Metadata` containers, in the
//! clear. A key account's own objects never hold it, and a key account holds
//! none on other accounts' objects, so two things guard the window between an
//! account's creation and `setup-token`:
//!
//! - `za_has_object_metadata`, a term of `za_assert_no_data`: setup refuses
//!   an account that already holds a container;
//! - `za_metadata_fence`, the ordinary-writer assertion of mail spec 6.1 for
//!   the container writers that classified the accounts as ordinary (JMAP's
//!   `PreparedMetadata`, IMAP `SETMETADATA`, and the private containers that
//!   ingest and copy carry): a write committed after the vault record exists
//!   fails, so a superseded writer cannot land after provisioning.

use crate::Server;
use store::{
    Deserialize, IterateParams, U32_LEN, ValueKey,
    write::{
        BatchBuilder, ValueClass,
        assert::AssertValue,
        metadata::{MetadataClass, ViewerState},
    },
};
use trc::AddContext;
use types::{collection::Collection, field::PrincipalField};

/// `MetadataClass::Shared` and `Private`'s key bytes (`store/src/write/
/// metadata.rs`, crate-private there); `VIEWER` and `OWNER` are public.
const METADATA_SHARED: u8 = 0;
const METADATA_PRIVATE: u8 = 1;

/// Where a metadata record's class byte sits: after the account id and the
/// `Collection::Metadata` byte (`store/src/write/key.rs`).
const CLASS_OFFSET: usize = U32_LEN + 1;

/// The vault-absence assertion of mail spec 6.1: the chunk that holds it
/// commits only while `account_id` holds no vault record. It selects the
/// `Principal` collection and document 0 and leaves them selected.
pub(crate) fn za_assert_no_vault(batch: &mut BatchBuilder, account_id: u32) {
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(PrincipalField::ZeroAccessVault, AssertValue::None);
}

/// The accounts a container write must find without a vault record: the
/// object's owner, and for a private container its viewer, whose own data
/// it is and under whose id its `Owner` counter is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZaMetadataFence {
    owner_id: u32,
    viewer_id: Option<u32>,
}

impl ZaMetadataFence {
    /// Adds the assertion for each account to `batch`, immediately before
    /// the container's own operations, so the commit chunk that holds them
    /// holds it too (no writer adds a commit point between the two). The
    /// caller's account, collection and document selection is restored, as
    /// `MetadataWrite::build` restores it.
    pub fn apply(self, batch: &mut BatchBuilder) {
        let account_id = batch.last_account_id();
        let collection = batch.last_collection();
        let document_id = batch.last_document_id();
        za_assert_no_vault(batch, self.owner_id);
        if let Some(viewer_id) = self
            .viewer_id
            .filter(|viewer_id| *viewer_id != self.owner_id)
        {
            za_assert_no_vault(batch, viewer_id);
        }
        if let Some(account_id) = account_id {
            batch.with_account_id(account_id);
        }
        if let Some(collection) = collection {
            batch.with_collection(collection);
        }
        if let Some(document_id) = document_id {
            batch.with_pending_document(document_id);
        }
    }
}

impl Server {
    /// The fence of a container write on `owner_id`'s object, with
    /// `viewer_id` for a private container, by a writer that read both
    /// accounts as ordinary (a key account's write is refused before it is
    /// built). `None` on PostgreSQL or MySQL, as `za_provisioning_fence`:
    /// key accounts cannot exist there.
    pub fn za_metadata_fence(
        &self,
        owner_id: u32,
        viewer_id: Option<u32>,
    ) -> Option<ZaMetadataFence> {
        (!self.store().is_pg_or_mysql()).then_some(ZaMetadataFence {
            owner_id,
            viewer_id,
        })
    }

    /// Ruling 66 (the Task 6 review's F1): true when the account holds
    /// object metadata that a key account may not hold in the clear. One
    /// pass over the account's `Collection::Metadata` records:
    ///
    /// - a `Shared` container on any of its objects except an address book,
    ///   a card or a file, whose DAV dead properties DAV keeps in the clear
    ///   for every account (design, "Other object kinds"): a mailbox's,
    ///   message's, Sieve script's or calendar's container, a server-level
    ///   IMAP annotation (`Collection::Principal`), or one left on a
    ///   destroyed document;
    /// - any `Private` container on its objects (another account's private
    ///   metadata, which stays until `purge_private_metadata` runs when its
    ///   object is destroyed);
    /// - an `Owner` counter that counts containers: the account's own private
    ///   metadata on another account's objects. A counter that counts none is
    ///   checked against that owner's private containers of the account too.
    ///
    /// `Viewer` counters are counts only; the containers they count are the
    /// `Private` records above.
    pub async fn za_has_object_metadata(&self, account_id: u32) -> trc::Result<bool> {
        let key = |class, collection: u8, document_id| ValueKey {
            account_id,
            collection,
            document_id,
            class: ValueClass::Metadata(class),
        };
        let mut found = false;
        let mut owners: Vec<(u32, u8)> = Vec::new();
        self.store()
            .iterate(
                IterateParams::new(
                    key(MetadataClass::Shared, 0, 0),
                    key(MetadataClass::Owner { owner: u32::MAX }, u8::MAX, u32::MAX),
                )
                .ascending(),
                |key, value| {
                    found = match key.get(CLASS_OFFSET).copied() {
                        Some(METADATA_SHARED) => key
                            .get(CLASS_OFFSET + 1)
                            .is_none_or(|collection| !is_dav_plaintext(*collection)),
                        Some(METADATA_PRIVATE) => true,
                        Some(MetadataClass::VIEWER) => false,
                        Some(MetadataClass::OWNER) => {
                            let owner = key
                                .get(CLASS_OFFSET + 1..CLASS_OFFSET + 1 + U32_LEN)
                                .and_then(|owner| owner.try_into().ok())
                                .map(u32::from_be_bytes);
                            let collection = key.get(CLASS_OFFSET + 1 + U32_LEN).copied();
                            match owner.zip(collection) {
                                Some(owner) => {
                                    let counts = ViewerState::deserialize(value)?.containers > 0;
                                    if !counts {
                                        owners.push(owner);
                                    }
                                    counts
                                }
                                None => true,
                            }
                        }
                        // Fails closed on a record this rule cannot read.
                        _ => true,
                    };
                    Ok(!found)
                },
            )
            .await
            .add_context(|err| err.caused_by(trc::location!()).account_id(account_id))?;
        if found {
            return Ok(true);
        }

        for (owner_id, collection) in owners {
            let class = MetadataClass::Private { viewer: account_id };
            let key = |document_id| ValueKey {
                account_id: owner_id,
                collection,
                document_id,
                class: ValueClass::Metadata(class),
            };
            self.store()
                .iterate(
                    IterateParams::new(key(0), key(u32::MAX))
                        .no_values()
                        .only_first(),
                    |_, _| {
                        found = true;
                        Ok(false)
                    },
                )
                .await
                .add_context(|err| err.caused_by(trc::location!()).account_id(owner_id))?;
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// The collections whose `Shared` containers hold DAV dead properties that
/// DAV keeps in the clear for every account, a key account's included.
fn is_dav_plaintext(collection: u8) -> bool {
    matches!(
        Collection::from(collection),
        Collection::AddressBook | Collection::ContactCard | Collection::FileNode
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use store::write::PendingId;

    #[test]
    fn the_fence_restores_the_callers_selection() {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(7)
            .with_collection(Collection::Mailbox)
            .with_document(3);
        ZaMetadataFence {
            owner_id: 7,
            viewer_id: Some(9),
        }
        .apply(&mut batch);
        assert_eq!(batch.last_account_id(), Some(7));
        assert_eq!(batch.last_collection(), Some(Collection::Mailbox));
        assert_eq!(batch.last_document_id(), Some(PendingId::Assigned(3)));
    }

    #[test]
    fn the_fence_selects_nothing_new_for_an_unselected_batch() {
        let mut batch = BatchBuilder::new();
        ZaMetadataFence {
            owner_id: 7,
            viewer_id: None,
        }
        .apply(&mut batch);
        // The assertion's own selection stays, as after
        // `za_provisioning_fence`; the container writes select their keys.
        assert_eq!(batch.last_account_id(), Some(7));
        assert_eq!(batch.last_collection(), Some(Collection::Principal));
        assert_eq!(batch.last_document_id(), Some(PendingId::Assigned(0)));
    }

    #[test]
    fn only_contacts_and_files_keep_plaintext_containers() {
        for collection in [
            Collection::AddressBook,
            Collection::ContactCard,
            Collection::FileNode,
        ] {
            assert!(is_dav_plaintext(collection.into()), "{collection:?}");
        }
        for collection in [
            Collection::Email,
            Collection::Mailbox,
            Collection::SieveScript,
            Collection::Principal,
            Collection::Calendar,
            Collection::CalendarEvent,
            Collection::CalendarEventNotification,
            Collection::Identity,
        ] {
            assert!(!is_dav_plaintext(collection.into()), "{collection:?}");
        }
        assert!(!is_dav_plaintext(u8::MAX));
    }
}
