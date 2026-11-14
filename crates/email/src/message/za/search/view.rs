/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ensure::{
        MapRecoveryError, Unavailable, ZaSearchEnsure, check_content_cap, is_current, map_load,
        recover_map, trace_recovery_limit,
    },
    pass::{PassOutcome, ZaSearchPass},
};
use crate::cache::MessageCacheFetch;
#[cfg(feature = "test_mode")]
use common::za::test_hooks::{SearchStage, note_search_count};
use common::{
    MessageStoreCache, Server,
    za::search::{
        cache::{IndexEntry, LoadedGuard},
        error::{LoadFailure, NoMapReason, TransientClass},
        load::{MapLoss, ObservedLoadError},
        seal::za_search_key,
    },
};
use std::{future::Future, sync::Arc, time::Instant};
use store::za_search::{
    content::ContentPart,
    manifest::{Manifest, ManifestValue, manifest_hash},
};
use types::collection::SyncCollection;
use vault::session::SessionKeys;

pub struct ZaView {
    pub entry: Arc<IndexEntry>,
    pub index: LoadedGuard,
    pub cache: Arc<MessageStoreCache>,
    pub epoch: u64,
}

impl ZaView {
    pub fn content(&self) -> &ContentPart {
        &self.index.content.as_ref().expect("a content view").part
    }
}

#[derive(Clone, Copy)]
pub enum ViewNeed {
    Map,
    Content { deadline: Instant },
}

#[derive(Clone, Copy, Debug)]
pub enum ViewError {
    NoMap(NoMapReason),
    Unavailable(Unavailable),
}

impl ViewError {
    pub fn into_unavailable(self) -> Unavailable {
        match self {
            Self::NoMap(reason) => map_load(LoadFailure::NoMap(reason)),
            Self::Unavailable(reason) => reason,
        }
    }
}

pub fn counter_at_or_past(last_change_id: u64, manifest: &Manifest) -> bool {
    last_change_id >= manifest.newest_row.change_id.max(manifest.epoch)
}

/// Acquire the cache and the exact index generation the response will use.
pub async fn za_view(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    need: ViewNeed,
) -> Result<ZaView, ViewError> {
    za_view_with_minimum(server, account_id, keys, need, None).await
}

pub async fn za_view_with_minimum(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    need: ViewNeed,
    minimum_cache_counter: Option<u64>,
) -> Result<ZaView, ViewError> {
    let deadline = match need {
        ViewNeed::Map => None,
        ViewNeed::Content { deadline } => Some(tokio::time::Instant::from_std(deadline)),
    };
    let failure = |error| view_load_failure(error, deadline);
    let mut recovered = false;
    let mut map_entry = if deadline.is_none() {
        Some(load_map_entry(server, account_id, keys, &mut recovered).await?)
    } else {
        None
    };
    let key = za_search_key(keys.swk());
    let mut rounds = 0;
    loop {
        let entry = match need {
            ViewNeed::Map => map_entry.as_ref().expect("a map entry").clone(),
            ViewNeed::Content { deadline: original } => {
                // Ensure-current bounds observer and loader waits by the
                // original deadline while allowing an owned transaction to settle.
                let entry = server
                    .za_search_ensure_current(account_id, keys, original)
                    .await
                    .map_err(ViewError::Unavailable)?;
                check_deadline(deadline)?;
                entry
            }
        };
        let cache = wait(deadline, server.get_cached_messages(account_id))
            .await?
            .map_err(|_| failure(LoadFailure::Transient(TransientClass::Store)))?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::search_snapshot_pause_point(account_id).await;

        let observed = wait(deadline, server.za_search_read_manifest(account_id, &key))
            .await?
            .and_then(|value| value.ok_or_else(|| ObservedLoadError::MapLoss(MapLoss::missing())));
        let (bytes, value) = match observed {
            Ok(value) => value,
            Err(error) => {
                recover_view_loss(server, account_id, keys, error, &mut recovered, deadline)
                    .await?;
                #[cfg(feature = "test_mode")]
                note_search_count(account_id, SearchStage::ViewRetry);
                continue;
            }
        };
        if matches!(value, ManifestValue::Destroying(_)) {
            return Err(failure(LoadFailure::NoMap(NoMapReason::Destroying)));
        }

        if let Some(guard) = wait(deadline, entry.read_loaded()).await? {
            if manifest_hash(&bytes) != guard.manifest_bytes_hash {
                drop(guard);
                if let Err(error) =
                    wait(deadline, server.za_search_reload_if_moved(&entry, keys)).await?
                {
                    recover_view_loss(server, account_id, keys, error, &mut recovered, deadline)
                        .await?;
                    #[cfg(feature = "test_mode")]
                    note_search_count(account_id, SearchStage::ViewRetry);
                    continue;
                }
            } else if !counter_at_or_past(cache.last_change_id, &guard.manifest)
                || minimum_cache_counter.is_some_and(|minimum| cache.last_change_id < minimum)
                || (deadline.is_some()
                    && cache.last_change_id < guard.manifest.indexed_to.change_id)
            {
                drop(guard);
                // commit_batch marks this same Email change group stale.
                server
                    .inner
                    .mark_cache_stale(account_id, SyncCollection::Email);
            } else {
                if deadline.is_some() {
                    check_content_cap(&guard, server.za_search_config().account_max_mb << 20)
                        .map_err(ViewError::Unavailable)?;
                    if guard.content.is_none() || !is_current(&guard, cache.last_change_id) {
                        drop(guard);
                        check_deadline(deadline)?;
                        #[cfg(feature = "test_mode")]
                        note_search_count(account_id, SearchStage::ViewRetry);
                        continue;
                    }
                }
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::search_view_pause_point(account_id).await;
                check_deadline(deadline)?;
                return Ok(ZaView {
                    entry,
                    epoch: guard.manifest.epoch,
                    index: guard,
                    cache,
                });
            }
        } else if deadline.is_none() {
            map_entry = Some(load_map_entry(server, account_id, keys, &mut recovered).await?);
        }
        rounds += 1;
        if rounds > 3 {
            return Err(failure(LoadFailure::Transient(TransientClass::Validation)));
        }
        #[cfg(feature = "test_mode")]
        note_search_count(account_id, SearchStage::ViewRetry);
    }
}

async fn load_map_entry(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    recovered: &mut bool,
) -> Result<Arc<IndexEntry>, ViewError> {
    match server.za_search_entry(account_id, keys).await {
        Ok(entry) => Ok(entry),
        Err(error) => {
            recover_view_loss(server, account_id, keys, error, recovered, None).await?;
            server
                .za_search_entry(account_id, keys)
                .await
                .map_err(|error| view_load_failure(error.classification(), None))
        }
    }
}

fn view_load_failure(failure: LoadFailure, deadline: Option<tokio::time::Instant>) -> ViewError {
    if deadline.is_some() {
        ViewError::Unavailable(map_load(failure))
    } else {
        ViewError::NoMap(match failure {
            LoadFailure::Loss(_) => NoMapReason::Transient(TransientClass::Store),
            LoadFailure::Transient(class) => NoMapReason::Transient(class),
            LoadFailure::NoMap(reason) => reason,
            LoadFailure::OverCap => NoMapReason::Transient(TransientClass::Budget),
        })
    }
}

async fn recover_view_loss(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    error: ObservedLoadError,
    recovered: &mut bool,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), ViewError> {
    match error {
        ObservedLoadError::MapLoss(loss) if !*recovered => {
            *recovered = true;
            recover_map(server, account_id, keys, loss, deadline)
                .await
                .map_err(|error| match error {
                    MapRecoveryError::NoMap(reason) => {
                        view_load_failure(LoadFailure::NoMap(reason), deadline)
                    }
                    MapRecoveryError::Unavailable(reason) if deadline.is_some() => {
                        ViewError::Unavailable(reason)
                    }
                    MapRecoveryError::Unavailable(_) => {
                        ViewError::NoMap(NoMapReason::Transient(TransientClass::Store))
                    }
                })?;
            // Map views give the cursor one re-entry. Content views next
            // enter ensure-current under the same deadline.
            if deadline.is_none()
                && let Ok(PassOutcome::Recovery(cause)) =
                    server.za_search_pass(account_id, keys).await
            {
                trace_recovery_limit(account_id, &cause);
            }
            Ok(())
        }
        other => Err(view_load_failure(other.classification(), deadline)),
    }
}

fn check_deadline(deadline: Option<tokio::time::Instant>) -> Result<(), ViewError> {
    if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
        return Err(ViewError::Unavailable(Unavailable::NotCurrent));
    }
    Ok(())
}

async fn wait<F: Future>(
    deadline: Option<tokio::time::Instant>,
    future: F,
) -> Result<F::Output, ViewError> {
    match deadline {
        Some(deadline) => {
            check_deadline(Some(deadline))?;
            tokio::time::timeout_at(deadline, future)
                .await
                .map_err(|_| ViewError::Unavailable(Unavailable::NotCurrent))
        }
        None => Ok(future.await),
    }
}

#[cfg(test)]
mod tests {
    use super::counter_at_or_past;
    use store::za_search::{manifest::Manifest, thread::RevisionRow};

    fn manifest_with(change_id: u64, epoch: u64) -> Manifest {
        Manifest {
            newest_row: RevisionRow {
                revision: 3,
                change_id,
            },
            epoch,
            ..Default::default()
        }
    }

    #[test]
    fn group_counter_covers_the_publication_and_epoch() {
        assert!(counter_at_or_past(0, &Manifest::default()));
        assert!(!counter_at_or_past(41, &manifest_with(42, 0)));
        assert!(counter_at_or_past(42, &manifest_with(42, 0)));
        // The empty account's mailbox-only witness advanced the group.
        assert!(counter_at_or_past(7, &manifest_with(7, 7)));
        assert!(!counter_at_or_past(6, &manifest_with(5, 7)));
        assert!(counter_at_or_past(7, &manifest_with(5, 7)));
    }
}
