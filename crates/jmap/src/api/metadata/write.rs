/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    prepared::PreparedMetadata,
    za::{za_metadata_refused, za_metadata_set_error},
};
use common::{
    Server,
    auth::AccountCache,
    storage::metadata::{ContainerChange, MetadataLog, PrivateMetadataWrite},
};
use email::message::ingest_metadata::IngestMetadata;
use jmap_proto::{error::set::SetError, object::metadata::MetadataProperty};
use std::sync::Arc;
use types::collection::Collection;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetadataUpdate {
    shared: Option<ContainerChange>,
    private: Option<ContainerChange>,
}

pub(super) struct ContainerTarget<'x> {
    pub owner: &'x AccountCache,
    pub viewer_id: u32,
    pub viewer: &'x mut Option<Arc<AccountCache>>,
    pub collection: Collection,
    pub log: MetadataLog,
}

impl MetadataUpdate {
    pub(super) fn new(shared: Option<ContainerChange>, private: Option<ContainerChange>) -> Self {
        MetadataUpdate { shared, private }
    }

    pub fn is_empty(&self) -> bool {
        self.shared.is_none() && self.private.is_none()
    }

    pub fn shared(&self) -> Option<&ContainerChange> {
        self.shared.as_ref()
    }

    pub fn private(&self) -> Option<&ContainerChange> {
        self.private.as_ref()
    }

    pub(super) async fn prepare<P: MetadataProperty>(
        self,
        server: &Server,
        target: ContainerTarget<'_>,
    ) -> trc::Result<Result<PreparedMetadata, SetError<P>>> {
        if target.za_refused(server, &self).await? {
            return Ok(Err(za_metadata_set_error()));
        }
        let shared = match self.shared {
            Some(change) => {
                if !server.has_metadata_quota(target.owner, &change).await? {
                    return Ok(Err(SetError::over_quota()));
                }
                Some(change.into_write(target.owner, target.collection, target.log))
            }
            None => None,
        };
        let private = match self.private {
            Some(change) => match target.private_write(server, change).await? {
                Ok(write) => Some(write),
                Err(err) => return Ok(Err(err)),
            },
            None => None,
        };
        Ok(Ok(PreparedMetadata { shared, private }))
    }

    pub(super) async fn prepare_ingest<P: MetadataProperty>(
        self,
        server: &Server,
        target: ContainerTarget<'_>,
    ) -> trc::Result<Result<Option<Box<IngestMetadata>>, SetError<P>>> {
        if target.za_refused(server, &self).await? {
            return Ok(Err(za_metadata_set_error()));
        }
        let private = match self.private {
            Some(change) => match target.private_write(server, change).await? {
                Ok(write) => Some(write),
                Err(err) => return Ok(Err(err)),
            },
            None => None,
        };
        Ok(Ok(IngestMetadata::new(
            self.shared.and_then(ContainerChange::into_next),
            private,
        )))
    }
}

impl ContainerTarget<'_> {
    /// Task 6: no container, shared or private, is written for a key
    /// account's object or by a key account (`za_metadata_refused`), a
    /// copy's included.
    async fn za_refused(&self, server: &Server, update: &MetadataUpdate) -> trc::Result<bool> {
        Ok(
            !update.is_empty()
                && za_metadata_refused(server, self.viewer_id, self.owner.id).await?,
        )
    }

    async fn private_write<P: MetadataProperty>(
        self,
        server: &Server,
        change: ContainerChange,
    ) -> trc::Result<Result<PrivateMetadataWrite, SetError<P>>> {
        let owner = self.owner;
        let viewer: &AccountCache = if self.viewer_id == owner.id {
            owner
        } else {
            let viewer = match self.viewer.take() {
                Some(viewer) => viewer,
                None => server.account(self.viewer_id).await?,
            };
            self.viewer.insert(viewer)
        };
        if !server.has_metadata_quota(viewer, &change).await? {
            return Ok(Err(SetError::over_quota()));
        }
        Ok(Ok(change.into_private_write(
            owner.id,
            viewer,
            self.collection,
            self.log,
        )))
    }
}
