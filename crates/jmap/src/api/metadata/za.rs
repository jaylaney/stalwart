/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Upgrade design, "Dead properties and object metadata": JMAP object
//! metadata is refused for key accounts. At 1.0 it is a property layer inside
//! the existing `*/get`, `*/set`, `*/query`, `*/copy` and `*/changes`
//! methods, so the dispatcher gate cannot catch it; the refusal sits at the
//! layer's store boundary instead, where the account is known:
//! - writes (`MetadataWriter::prepare_for`, `prepare_new`, `prepare_ingest`
//!   for the patches a client names; `MetadataUpdate::prepare` and
//!   `prepare_ingest` for every non-empty update, a copy's included) answer
//!   `forbidden` per object;
//! - preloads (`ObjectMetadata::preload`) read no container;
//! - reads (`ObjectMetadata::load`) read no container and answer as upstream
//!   answers when metadata is not supported: the objects carry no metadata
//!   property;
//! - filters (`ObjectMetadata::evaluate`) answer `unsupportedFilter`;
//! - `*/changes` reports no metadata changes.
//!
//! A request is refused when its account or its caller is a key account: a
//! key account's own objects never hold object metadata, and a key account's
//! private metadata on an ordinary account's object would be its own data in
//! the clear. Requests between ordinary accounts take upstream's paths. The
//! session advertises no metadata capability on such an account
//! (`api::session::za_hidden_capability`).

use super::NewMetadata;
use common::Server;
use jmap_proto::{error::set::SetError, object::metadata::MetadataProperty};
use trc::AddContext;

/// The client text of a refused object-metadata write or filter.
pub const ZA_METADATA_REFUSED: &str = "Object metadata is not available for zero-access accounts.";

/// Whether object metadata is refused for a request by `caller_id` on
/// `account_id`'s objects: either is a key account. Both are read with
/// `account()`, which fails closed on an unknown id.
pub async fn za_metadata_refused(
    server: &Server,
    caller_id: u32,
    account_id: u32,
) -> trc::Result<bool> {
    if server
        .account(account_id)
        .await
        .caused_by(trc::location!())?
        .is_key_account()
    {
        return Ok(true);
    }
    Ok(caller_id != account_id
        && server
            .account(caller_id)
            .await
            .caused_by(trc::location!())?
            .is_key_account())
}

/// The per-object error of a refused object-metadata write.
pub(super) fn za_metadata_set_error<P: MetadataProperty>() -> SetError<P> {
    SetError::forbidden().with_description(ZA_METADATA_REFUSED)
}

/// The method error of a refused object-metadata filter.
pub(super) fn za_metadata_filter_error() -> trc::Error {
    trc::JmapEvent::UnsupportedFilter
        .into_err()
        .details(ZA_METADATA_REFUSED)
}

/// Whether the client named object-metadata patches, as a create's metadata
/// or alongside a copy, rather than a copy carrying its source's containers
/// alone (which `MetadataUpdate::prepare` refuses only when they are not
/// empty).
pub(super) fn za_names_patches(metadata: &NewMetadata) -> bool {
    matches!(
        metadata,
        NewMetadata::Create(_)
            | NewMetadata::Copy {
                patches: Some(_),
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{metadata::MetadataPatches, session::za_hidden_capability};
    use jmap_proto::request::capability::Capability;

    #[test]
    fn named_patches_are_the_clients() {
        assert!(za_names_patches(&NewMetadata::Create(
            MetadataPatches::for_create()
        )));
        assert!(za_names_patches(&NewMetadata::Copy {
            patches: Some(MetadataPatches::for_create()),
            source_id: 1,
        }));
        assert!(!za_names_patches(&NewMetadata::Copy {
            patches: None,
            source_id: 1,
        }));
    }

    #[test]
    fn key_accounts_are_offered_no_metadata() {
        for capability in Capability::all_capabilities() {
            // Ordinary account, ordinary caller: upstream's capabilities.
            assert!(!za_hidden_capability(false, false, *capability));
            let calendars = matches!(
                capability,
                Capability::Calendars | Capability::CalendarsParse
            );
            let metadata = *capability == Capability::Metadata;
            // A key account: no calendars (spec 9), no metadata.
            assert_eq!(
                za_hidden_capability(true, false, *capability),
                calendars || metadata,
                "{capability:?}"
            );
            assert_eq!(
                za_hidden_capability(true, true, *capability),
                calendars || metadata,
                "{capability:?}"
            );
            // An ordinary account read by a key account: no metadata only.
            assert_eq!(
                za_hidden_capability(false, true, *capability),
                metadata,
                "{capability:?}"
            );
        }
    }
}
