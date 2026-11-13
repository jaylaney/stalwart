/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    pass::{PassOutcome, RecoveryCause, ZaSearchPass, read_change_id, run_pass},
    rebuild::{ContentRebuildReason, RebuildStarted, ZaSearchRebuild, recovery_timed_out},
};
#[cfg(feature = "test_mode")]
use common::za::test_hooks::{SearchStage, note_search_count, observe_search_wait};
use common::{
    Server,
    za::search::{
        cache::{IndexEntry, LoadedIndex},
        error::{LoadFailure, NoMapReason, TransientClass},
        load::{ContentLoss, MapLoss, ObservedLoadError},
        trace::{ZA_SEARCH_LOST, ZA_SEARCH_REBUILD, ZA_SEARCH_UNAVAILABLE},
    },
    za_search_trace,
};
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout_at};
use vault::session::SessionKeys;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    Rebuilding,
    Transient(TransientClass),
    OverCap,
    Budget,
    NewerFormat,
    NotCurrent,
    Unresolved,
}

impl Unavailable {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rebuilding => "rebuilding",
            Self::Transient(class) => class.as_str(),
            Self::OverCap => "over_cap",
            Self::Budget => "budget",
            Self::NewerFormat => "newer_format",
            Self::NotCurrent => "not_current",
            Self::Unresolved => "unresolved",
        }
    }
}

pub trait ZaSearchEnsure: Sync + Send {
    /// Attempt one opportunistic pass without failing the enclosing request.
    fn za_search_pass_hook(
        &self,
        account_id: u32,
        keys: &SessionKeys,
    ) -> impl Future<Output = ()> + Send;

    /// Complete passes or wait for a running one, then load current content.
    fn za_search_ensure_current(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        deadline: Instant,
    ) -> impl Future<Output = Result<Arc<IndexEntry>, Unavailable>> + Send;
}

impl ZaSearchEnsure for Server {
    async fn za_search_pass_hook(&self, account_id: u32, keys: &SessionKeys) {
        let mut reentered = false;
        loop {
            match self.za_search_pass(account_id, keys).await {
                Ok(PassOutcome::Recovery(cause)) if !reentered => {
                    reentered = true;
                    if let RecoveryCause::Loss(loss) = cause
                        && let Err(reason) = recover_map(self, account_id, keys, loss, None).await
                    {
                        za_search_trace!(
                            ZA_SEARCH_UNAVAILABLE,
                            AccountId = account_id,
                            Reason = reason.as_str()
                        );
                        break;
                    }
                }
                Ok(PassOutcome::Recovery(cause)) => {
                    trace_recovery_limit(account_id, &cause);
                    break;
                }
                Ok(PassOutcome::ContentLoss(loss)) if !reentered => {
                    reentered = true;
                    if let Err(reason) = recover_content(self, account_id, keys, loss, None).await {
                        za_search_trace!(
                            ZA_SEARCH_UNAVAILABLE,
                            AccountId = account_id,
                            Reason = reason.as_str()
                        );
                        break;
                    }
                }
                Ok(PassOutcome::NoMap(reason)) => {
                    za_search_trace!(
                        ZA_SEARCH_UNAVAILABLE,
                        AccountId = account_id,
                        Reason = reason.as_str()
                    );
                    break;
                }
                Ok(_) => break,
                Err(err) => {
                    trc::error!(err.account_id(account_id));
                    break;
                }
            }
        }
    }

    async fn za_search_ensure_current(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        deadline: Instant,
    ) -> Result<Arc<IndexEntry>, Unavailable> {
        let deadline = tokio::time::Instant::from_std(deadline);
        let cap = self.za_search_config().account_max_mb << 20;
        let mut reentered = false;
        let mut recovered_content = false;
        while tokio::time::Instant::now() < deadline {
            #[cfg(feature = "test_mode")]
            note_search_count(account_id, SearchStage::EnsureRound);
            // Do not wait for the resident guard here. A running pass can
            // hold it, in which case the pass-lock wait below owns the wait.
            if let Some(entry) = self.inner.cache.za_search.get(account_id) {
                let backfill = {
                    let guard = entry.index.try_read().ok();
                    if let Some(index) = guard.as_deref().and_then(Option::as_ref) {
                        check_content_cap(index, cap)?;
                        index.manifest.content_to.cursor.is_some()
                            && !index.manifest.content_to.suspended
                    } else {
                        false
                    }
                };
                let cooling = entry
                    .retry_schedule()
                    .is_some_and(|(_, retry_at, _)| Instant::now() < retry_at);
                if backfill
                    && !cooling
                    && load_content(self, &entry, keys, deadline, &mut recovered_content).await?
                {
                    continue;
                }
            }
            // Observer and loader waits retain this request's deadline.
            // Once a pass owns its transaction, let that transaction settle.
            match run_pass(self, account_id, keys, Some(deadline)).await {
                Ok(PassOutcome::Deadline) => return Err(Unavailable::NotCurrent),
                Ok(PassOutcome::Running) => {
                    // A busy pass can hold the index write guard. Find its
                    // slot without awaiting any reads or loads first.
                    if let Some(entry) = self.inner.cache.za_search.get(account_id) {
                        let wait = timeout_at(deadline, entry.pass_lock.lock());
                        #[cfg(feature = "test_mode")]
                        let wait = observe_search_wait(account_id, SearchStage::PassLockWait, wait);
                        drop(wait.await.map_err(|_| Unavailable::NotCurrent)?);
                    }
                }
                Ok(PassOutcome::Current | PassOutcome::Advanced { .. } | PassOutcome::Lost) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(Unavailable::NotCurrent);
                    }
                    let entry = match timeout_at(deadline, self.za_search_entry(account_id, keys))
                        .await
                        .map_err(|_| Unavailable::NotCurrent)?
                    {
                        Ok(entry) => entry,
                        Err(ObservedLoadError::MapLoss(loss)) if !reentered => {
                            reentered = true;
                            recover_map(self, account_id, keys, loss, Some(deadline))
                                .await
                                .map_err(MapRecoveryError::unavailable)?;
                            continue;
                        }
                        Err(error) => return Err(map_load(error.classification())),
                    };
                    let state = timeout_at(deadline, read_change_id(self, account_id))
                        .await
                        .map_err(|_| Unavailable::NotCurrent)?
                        .map_err(|_| Unavailable::Transient(TransientClass::Store))?;
                    let current = {
                        let Some(index) = timeout_at(deadline, entry.read_loaded())
                            .await
                            .map_err(|_| Unavailable::NotCurrent)?
                        else {
                            continue;
                        };
                        check_content_cap(&index, cap)?;
                        is_current(&index, state)
                    };
                    if current {
                        // The loader can retry moving heads. Use the request's
                        // original deadline for that entire load and its retries.
                        if load_content(self, &entry, keys, deadline, &mut recovered_content)
                            .await?
                        {
                            continue;
                        }
                        if let Some(index) = timeout_at(deadline, entry.read_loaded())
                            .await
                            .map_err(|_| Unavailable::NotCurrent)?
                        {
                            // A publishing pass can advance the resident head
                            // across the cap after the loader has returned.
                            check_content_cap(&index, cap)?;
                            if index.content.is_some() && is_current(&index, state) {
                                drop(index);
                                return Ok(entry);
                            }
                        }
                    }
                    if timeout_at(deadline, sleep(Duration::from_millis(20)))
                        .await
                        .is_err()
                    {
                        return Err(Unavailable::NotCurrent);
                    }
                }
                Ok(PassOutcome::Deferred { class, retry_at }) => {
                    let wake = self
                        .inner
                        .cache
                        .za_search
                        .get(account_id)
                        .and_then(|entry| entry.retry_schedule())
                        .map_or_else(Instant::now, |(_, _, observe_at)| retry_at.min(observe_at));
                    tokio::time::sleep_until(tokio::time::Instant::from_std(wake).min(deadline))
                        .await;
                    if tokio::time::Instant::now() >= deadline {
                        return Err(Unavailable::Transient(class));
                    }
                }
                Ok(PassOutcome::Aborted(class)) => {
                    if timeout_at(deadline, sleep(Duration::from_millis(50)))
                        .await
                        .is_err()
                    {
                        return Err(Unavailable::Transient(class));
                    }
                }
                Ok(PassOutcome::ContentLoss(loss)) if !recovered_content => {
                    recovered_content = true;
                    recover_content(self, account_id, keys, loss, Some(deadline)).await?;
                }
                Ok(PassOutcome::ContentLoss(_)) => return Err(Unavailable::Rebuilding),
                Ok(PassOutcome::NoMap(reason)) => return Err(map_no_map(reason)),
                Ok(PassOutcome::Recovery(cause)) if !reentered => {
                    reentered = true;
                    if let RecoveryCause::Loss(loss) = cause {
                        recover_map(self, account_id, keys, loss, Some(deadline))
                            .await
                            .map_err(MapRecoveryError::unavailable)?;
                    }
                }
                Ok(PassOutcome::Recovery(cause)) => {
                    trace_recovery_limit(account_id, &cause);
                    return Err(Unavailable::Rebuilding);
                }
                Err(_) => return Err(Unavailable::Transient(TransientClass::Store)),
            }
        }
        Err(Unavailable::NotCurrent)
    }
}

pub(super) fn trace_recovery_limit(account_id: u32, cause: &RecoveryCause) {
    za_search_trace!(
        ZA_SEARCH_REBUILD,
        AccountId = account_id,
        Reason = cause.as_str()
    );
}

pub(super) enum MapRecoveryError {
    NoMap(NoMapReason),
    Unavailable(Unavailable),
}

impl MapRecoveryError {
    fn unavailable(self) -> Unavailable {
        match self {
            Self::NoMap(reason) => map_no_map(reason),
            Self::Unavailable(reason) => reason,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::NoMap(reason) => reason.as_str(),
            Self::Unavailable(reason) => reason.as_str(),
        }
    }
}

impl From<Unavailable> for MapRecoveryError {
    fn from(reason: Unavailable) -> Self {
        Self::Unavailable(reason)
    }
}

pub(super) async fn recover_map(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    loss: MapLoss,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), MapRecoveryError> {
    recovery_pause(account_id, deadline).await?;
    match server
        .za_search_rebuild_start(account_id, keys, loss, deadline)
        .await
    {
        Ok(RebuildStarted::Destroying) => {
            return Err(MapRecoveryError::NoMap(NoMapReason::Destroying));
        }
        Ok(RebuildStarted::Written | RebuildStarted::LostToAnotherNode) => (),
        Err(error) if recovery_timed_out(&error) => return Err(Unavailable::NotCurrent.into()),
        Err(error) => {
            trc::error!(error.account_id(account_id));
            return Err(Unavailable::Transient(TransientClass::Store).into());
        }
    }
    wait_for_recovery(deadline, async {
        let entry = server.za_search_entry(account_id, keys).await?;
        server.za_search_reload_if_moved(&entry, keys).await
    })
    .await?
    .map_err(|error| match error.classification() {
        LoadFailure::NoMap(reason) => MapRecoveryError::NoMap(reason),
        other => map_load(other).into(),
    })?;
    Ok(())
}

async fn load_content(
    server: &Server,
    entry: &IndexEntry,
    keys: &SessionKeys,
    deadline: tokio::time::Instant,
    recovered: &mut bool,
) -> Result<bool, Unavailable> {
    let wait = async {
        timeout_at(deadline, async {
            #[cfg(feature = "test_mode")]
            common::za::test_hooks::search_content_wait_pause_point(entry.account_id).await;
            server.za_search_load_content(entry, keys).await
        })
        .await
        .map_err(|_| ())
    };
    #[cfg(feature = "test_mode")]
    let wait = common::za::test_hooks::search_deadline_wait(entry.account_id, Some(deadline), wait);
    match wait.await.map_err(|_| Unavailable::NotCurrent)? {
        Ok(()) => Ok(false),
        Err(ObservedLoadError::ContentLoss(loss)) if !*recovered => {
            *recovered = true;
            recover_content(server, entry.account_id, keys, loss, Some(deadline)).await?;
            Ok(true)
        }
        Err(error) => Err(map_load(error.classification())),
    }
}

async fn recover_content(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    loss: ContentLoss,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), Unavailable> {
    recovery_pause(account_id, deadline).await?;
    match server
        .za_search_content_rebuild_start(
            account_id,
            keys,
            ContentRebuildReason::ContentLoss(loss),
            deadline,
        )
        .await
    {
        Ok(()) => (),
        Err(error) if error.is_assertion_failure() => {
            za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
        }
        Err(error) if recovery_timed_out(&error) => return Err(Unavailable::NotCurrent),
        Err(error) => {
            trc::error!(error.account_id(account_id));
            return Err(Unavailable::Transient(TransientClass::Store));
        }
    }
    if let Some(entry) = server.inner.cache.za_search.get(account_id) {
        wait_for_recovery(deadline, server.za_search_reload_if_moved(&entry, keys))
            .await?
            .map_err(|error| map_load(error.classification()))?;
    }
    Ok(())
}

async fn recovery_pause(
    _account_id: u32,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), Unavailable> {
    #[cfg(feature = "test_mode")]
    wait_for_recovery(
        deadline,
        common::za::test_hooks::search_recovery_pause_point(_account_id),
    )
    .await?;
    if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
        return Err(Unavailable::NotCurrent);
    }
    Ok(())
}

async fn wait_for_recovery<F: Future>(
    deadline: Option<tokio::time::Instant>,
    future: F,
) -> Result<F::Output, Unavailable> {
    if let Some(deadline) = deadline {
        if tokio::time::Instant::now() >= deadline {
            return Err(Unavailable::NotCurrent);
        }
        timeout_at(deadline, future)
            .await
            .map_err(|_| Unavailable::NotCurrent)
    } else {
        Ok(future.await)
    }
}

/// Apply the account cap to the manifest held by the caller's read guard.
pub fn check_content_cap(index: &LoadedIndex, cap_bytes: u64) -> Result<(), Unavailable> {
    let size = index.manifest.content.as_ref().map_or(0, |head| head.size);
    if size > cap_bytes || (index.manifest.content_to.suspended && size >= cap_bytes) {
        Err(Unavailable::OverCap)
    } else {
        Ok(())
    }
}

pub fn is_current(index: &LoadedIndex, state: u64) -> bool {
    let manifest = &index.manifest;
    index.scanned_to >= state
        && manifest.indexed_to.complete
        && manifest.content_to.checkpoint == manifest.indexed_to
        && !manifest.content_to.suspended
        && manifest.content_to.cursor.is_none()
        && manifest.cursor.is_none()
        && manifest.reconciliation.is_none()
}

fn map_no_map(reason: NoMapReason) -> Unavailable {
    match reason {
        NoMapReason::NewerFormat => Unavailable::NewerFormat,
        NoMapReason::Destroying => Unavailable::Transient(TransientClass::Store),
        NoMapReason::Transient(class) => Unavailable::Transient(class),
    }
}

pub(super) fn map_load(failure: LoadFailure) -> Unavailable {
    match failure {
        LoadFailure::Loss(_) => Unavailable::Rebuilding,
        LoadFailure::Transient(TransientClass::Budget) => Unavailable::Budget,
        LoadFailure::Transient(class) => Unavailable::Transient(class),
        LoadFailure::NoMap(NoMapReason::NewerFormat) => Unavailable::NewerFormat,
        LoadFailure::NoMap(_) => Unavailable::Transient(TransientClass::Store),
        LoadFailure::OverCap => Unavailable::OverCap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::za::search::cache::{BudgetCharge, LoadedContent};
    use store::za_search::{
        Checkpoint,
        manifest::{ContentCursor, ContentHead, ContentTo, Manifest, RebuildCursor},
    };

    #[test]
    fn current_resident_content_must_remain_within_the_cap() {
        let cap = 1 << 20;
        let checkpoint = Checkpoint {
            change_id: 8,
            position: 0,
            complete: true,
        };
        let head = ContentHead {
            size: cap - 1,
            ..ContentHead::default()
        };
        let segment = head.segment;
        let mut index = LoadedIndex::empty(Manifest {
            indexed_to: checkpoint,
            content: Some(head),
            content_to: ContentTo {
                checkpoint,
                ..ContentTo::default()
            },
            ..Manifest::default()
        });
        index.content = Some(LoadedContent {
            part: Default::default(),
            segments: vec![segment],
            bytes: 0,
            charge: BudgetCharge::detached(),
        });

        assert!(is_current(&index, 8));
        assert_eq!(check_content_cap(&index, cap), Ok(()));
        index.manifest.content.as_mut().unwrap().size = cap;
        assert_eq!(check_content_cap(&index, cap), Ok(()));

        // A crossing publication can leave coverage complete and suspension
        // false while moving the resident content head above the account cap.
        index.manifest.content.as_mut().unwrap().size = cap + 1;
        assert!(is_current(&index, 8));
        assert_eq!(check_content_cap(&index, cap), Err(Unavailable::OverCap));

        index.manifest.content_to.suspended = true;
        index.manifest.content.as_mut().unwrap().size = cap;
        assert_eq!(check_content_cap(&index, cap), Err(Unavailable::OverCap));
        index.manifest.content.as_mut().unwrap().size = cap - 1;
        assert_eq!(check_content_cap(&index, cap), Ok(()));
        assert!(!is_current(&index, 8));
    }

    #[test]
    fn is_current_needs_no_pending_content_cursor() {
        let checkpoint = Checkpoint {
            change_id: 8,
            position: 0,
            complete: true,
        };
        let mut index = LoadedIndex::empty(Manifest {
            indexed_to: checkpoint,
            content_to: ContentTo {
                checkpoint,
                cursor: Some(ContentCursor { next_below: 7 }),
                ..ContentTo::default()
            },
            ..Manifest::default()
        });
        assert!(!is_current(&index, 8));

        index.manifest.cursor = Some(RebuildCursor { next: 1, end: 3 });
        assert!(!is_current(&index, 8));
        index.manifest.cursor = None;
        assert!(!is_current(&index, 8));

        index.manifest.content_to.cursor = None;
        assert!(is_current(&index, 8));
        assert!(!is_current(&index, 9));
    }
}
