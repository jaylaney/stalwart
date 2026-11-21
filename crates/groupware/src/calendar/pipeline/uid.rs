/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::error::{ZaBusy, ZaPipelineError};
use crate::calendar::{CalendarEvent, privacy::EventPrivacy};
use common::{
    GroupwareResources, Server,
    auth::AccessToken,
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use std::convert::Infallible;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes, BatchBuilder},
};
use trc::AddContext;
use types::{
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
};

/// Invitations spec 5.1, 6.3 step 1: takes the UID creation lease for a key
/// account's create, then checks the UID against current archives in each
/// target calendar while holding it. A lease that stays held through the
/// retries returns `Busy(LeaseBusy)` with a trace. A conflict releases the
/// lease before returning `UidConflict`.
pub async fn za_take_uid_lease(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    owner_id: u32,
    calendar_ids: &[u32],
    uid: &str,
) -> Result<ZaUidLeaseGuard, ZaPipelineError> {
    let Some(lease) = ZaUidLease::acquire(server.store(), owner_id, uid)
        .await
        .caused_by(trc::location!())?
    else {
        trc::event!(
            Store(trc::StoreEvent::AssertValueFailed),
            AccountId = owner_id,
            Details = "zero-access: UID creation lease busy",
        );
        return Err(ZaPipelineError::Busy(ZaBusy::LeaseBusy));
    };
    let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
    for &calendar_id in calendar_ids {
        if let Err(err) =
            za_assert_is_unique_uid(server, resources, access_token, owner_id, calendar_id, uid)
                .await
        {
            lease.release().await;
            return Err(err);
        }
    }
    Ok(lease)
}

/// Invitations spec 5.1: the archive-based counterpart of
/// `assert_is_unique_uid` for a key account's create, run under the
/// creation lease. The handler's resource snapshot can predate another
/// creator's commit, so this reads the fresh UID-index hits and each hit's
/// current archive, and refuses the UID when a stored copy names the target
/// calendar (upstream's rule: a UID is unique within one calendar). The
/// snapshot only formats the href.
pub async fn za_assert_is_unique_uid(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    uid: &str,
) -> Result<(), ZaPipelineError> {
    let hits = server
        .document_ids_matching(
            owner_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?;
    // Upstream's rule at 1.0 (`assert_is_unique_uid`): a conflict answers
    // 403 `NoUidConflict`, and a sharee whose only conflict is a secret
    // event gets a plain 403, without its href.
    let is_owner = access_token.is_member(owner_id);
    let mut has_hidden_conflict = false;
    for document_id in hits {
        let Some(stored) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                owner_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            continue;
        };
        let event = stored
            .unarchive::<CalendarEvent>()
            .caused_by(trc::location!())?;
        if let Some(name) = event
            .names
            .iter()
            .find(|name| name.parent_id.to_native() == calendar_id)
        {
            if !is_owner
                && EventPrivacy::from_flags(event.flags.to_native()) == EventPrivacy::Secret
            {
                has_hidden_conflict = true;
                continue;
            }
            let href = resources
                .container_resource_path_by_id(calendar_id)
                .map(|calendar| {
                    resources.format_item(&format!("{}/{}", calendar.path(), name.name))
                })
                .unwrap_or_default();
            return Err(ZaPipelineError::UidConflict { href: Some(href) });
        }
    }
    if has_hidden_conflict {
        Err(ZaPipelineError::UidConflict { href: None })
    } else {
        Ok(())
    }
}

/// Invitations spec 5.4 and 6.3 step 3: what a key account's create does
/// when its lease was taken over while the request ran, found by the
/// fan-out's fence or by the commit's. Traced first, so the retake's
/// outcome cannot hide the fence loss. The lease is abandoned (the key
/// holds a successor's value, or is gone), then taken again to re-check
/// current archives. If the retake succeeds, release it and return
/// `Busy(CreateLost)` so the client resends. Never `Ok`.
pub async fn za_lost_create_fence(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    lease: ZaUidLeaseGuard,
    owner_id: u32,
    calendar_ids: &[u32],
    uid: &str,
) -> Result<Infallible, ZaPipelineError> {
    let Err(error) = za_finish_create_loss(
        server,
        resources,
        access_token,
        lease,
        owner_id,
        calendar_ids,
        uid,
        CreateLoss::ConfirmedLease,
    )
    .await;
    Err(error)
}

enum CreateLoss {
    ConfirmedLease,
    OpaqueSecondCommit,
}

/// `za_lost_create_fence`'s body and the second commit's loss: its type
/// cannot be `Ok` (final review, ledger 181), so the compiler keeps a lost
/// create from reaching the mail submission (iMIP invariant 46).
#[allow(clippy::too_many_arguments)]
async fn za_finish_create_loss(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    lease: ZaUidLeaseGuard,
    owner_id: u32,
    calendar_ids: &[u32],
    uid: &str,
    loss: CreateLoss,
) -> Result<Infallible, ZaPipelineError> {
    trc::event!(
        Store(trc::StoreEvent::AssertValueFailed),
        AccountId = owner_id,
        Details = match &loss {
            CreateLoss::ConfirmedLease => "zero-access: UID creation lease lost before the commit",
            CreateLoss::OpaqueSecondCommit => "zero-access: create retry lost its assertion",
        },
    );
    match loss {
        CreateLoss::ConfirmedLease => lease.abandon(),
        CreateLoss::OpaqueSecondCommit => {
            // The second failure may still be the calendar assertion. Release
            // only our exact fence before the existing UID retake. A false
            // result is inconclusive; it does not prove successor ownership.
            let _ = lease.release().await;
        }
    }
    let retry =
        za_take_uid_lease(server, resources, access_token, owner_id, calendar_ids, uid).await?;
    retry.release().await;
    // The retake found no copy in the target calendars. Keep the existing resend
    // answer for either a confirmed lease loss or an opaque second loss.
    Err(ZaPipelineError::Busy(ZaBusy::CreateLost))
}

/// The bounded second create commit. Its assertion loss is opaque: release
/// our fenced lease before the existing UID retake/outcome, without another
/// calendar read, archive retry or fan-out.
#[allow(clippy::too_many_arguments)]
pub async fn za_commit_create(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    batch: BatchBuilder,
    lease: ZaUidLeaseGuard,
    owner_id: u32,
    calendar_ids: &[u32],
    uid: &str,
) -> Result<u64, ZaPipelineError> {
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_commit_fail(owner_id) {
        lease.release().await;
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: sender commit failure")
            .caused_by(trc::location!())
            .into());
    }
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::commit_pause_point(owner_id).await;
    match server.commit_batch(batch).await {
        Ok(assigned) => {
            lease.release().await;
            Ok(assigned.last_change_id(owner_id, SyncCollection::Calendar))
        }
        Err(err) if err.is_assertion_failure() => {
            let Err(error) = za_finish_create_loss(
                server,
                resources,
                access_token,
                lease,
                owner_id,
                calendar_ids,
                uid,
                CreateLoss::OpaqueSecondCommit,
            )
            .await;
            Err(error)
        }
        Err(err) => {
            lease.release().await;
            Err(err.caused_by(trc::location!()).into())
        }
    }
}
