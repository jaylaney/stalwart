/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::pass::{PassOutcome, ZaSearchPass, read_change_id};
use common::{
    Server,
    za::search::{
        cache::{IndexEntry, LoadedIndex},
        error::{LoadFailure, NoMapReason, TransientClass},
        trace::{ZA_SEARCH_REBUILD, ZA_SEARCH_UNAVAILABLE},
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
        match self.za_search_pass(account_id, keys).await {
            Ok(
                PassOutcome::Current
                | PassOutcome::Advanced { .. }
                | PassOutcome::Running
                | PassOutcome::Lost
                | PassOutcome::Aborted(_),
            ) => {}
            Ok(PassOutcome::NoMap(reason)) => za_search_trace!(
                ZA_SEARCH_UNAVAILABLE,
                AccountId = account_id,
                Reason = reason.as_str()
            ),
            Ok(PassOutcome::Recovery(cause)) => za_search_trace!(
                ZA_SEARCH_REBUILD,
                AccountId = account_id,
                Reason = cause.as_str()
            ),
            Err(err) => {
                trc::error!(err.account_id(account_id));
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
        while tokio::time::Instant::now() < deadline {
            // A pass owns its transaction through its outcome. Check the
            // deadline between passes and bound waits separately.
            match self.za_search_pass(account_id, keys).await {
                Ok(PassOutcome::Running) => {
                    // A busy pass can hold the index write guard. Find its
                    // slot without awaiting any reads or loads first.
                    if let Some(entry) = self.inner.cache.za_search.get(account_id) {
                        drop(
                            timeout_at(deadline, entry.pass_lock.lock())
                                .await
                                .map_err(|_| Unavailable::NotCurrent)?,
                        );
                    }
                }
                Ok(PassOutcome::Current | PassOutcome::Advanced { .. } | PassOutcome::Lost) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(Unavailable::NotCurrent);
                    }
                    let entry = timeout_at(deadline, self.za_search_entry(account_id, keys))
                        .await
                        .map_err(|_| Unavailable::NotCurrent)?
                        .map_err(map_load)?;
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
                        timeout_at(deadline, self.za_search_load_content(&entry, keys))
                            .await
                            .map_err(|_| Unavailable::NotCurrent)?
                            .map_err(map_load)?;
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
                Ok(PassOutcome::Aborted(class)) => {
                    if timeout_at(deadline, sleep(Duration::from_millis(50)))
                        .await
                        .is_err()
                    {
                        return Err(Unavailable::Transient(class));
                    }
                }
                Ok(PassOutcome::NoMap(reason)) => return Err(map_no_map(reason)),
                Ok(PassOutcome::Recovery(_)) => return Err(Unavailable::Rebuilding),
                Err(_) => return Err(Unavailable::Transient(TransientClass::Store)),
            }
        }
        Err(Unavailable::NotCurrent)
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
