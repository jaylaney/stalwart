/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.8 (D15, D29): the sync boundary of a key account's calendar
//! changes and the explicit origin of a paged key-calendar sync.

use super::get::IntermediateChangesResponse;
use crate::api::za_calendar::{calendar_read_error, za_admit_calendar_method, za_calendar_method};
use common::{Server, auth::AccessToken};
use groupware::{
    cache::GroupwareCache,
    calendar::pipeline::{context::za_foreign_calendar_contexts, error::ZaPipelineError},
};
use jmap_proto::{
    method::changes::ChangesRequest,
    request::method::{MethodFunction, MethodObject},
    types::state::{State, ZaCalendarOrigin},
};
use std::future::Future;
use store::{query::log::Query, write::LogCollection};
use trc::AddContext;
use types::collection::SyncCollection;

/// The answer to a `z` state outside the key-calendar changes methods, as the
/// request parser gives it (`jmap_proto::request::parser`).
pub(crate) fn za_state_out_of_scope() -> trc::Error {
    trc::JmapEvent::InvalidArguments
        .into_err()
        .details("This state is only valid for calendar changes.")
}

/// D15's `cannotCalculateChanges`, the JMAP form of CalDAV's `valid-sync-token`.
fn za_sync_restart() -> trc::Error {
    trc::JmapEvent::CannotCalculateChanges
        .into_err()
        .details("The calendar sync must start again.")
}

/// Calendar/changes and CalendarEvent/changes after their access check, in spec 8's
/// handler form (ruling R21-2): the classification captured at admission rebuilds every
/// Store error of a keyed path, from admission through the boundary to the page.
/// `page` runs upstream's algorithm over the state with a `z` replaced by its range;
/// every intermediate result continues the original origin, and `oldState` echoes the
/// request.
pub(crate) async fn za_calendar_changes<F, Fut>(
    server: &Server,
    access_token: &AccessToken,
    mut request: ChangesRequest,
    object: MethodObject,
    page: F,
) -> trc::Result<IntermediateChangesResponse>
where
    F: FnOnce(ChangesRequest) -> Fut + Send,
    Fut: Future<Output = trc::Result<IntermediateChangesResponse>> + Send,
{
    let owner = request.account_id.document_id();
    let za_key = server.za_is_key_account(owner).await?;
    let keyed = move |error| calendar_read_error(za_key, owner, error);
    za_admit_calendar_method(
        server,
        access_token,
        request.account_id,
        za_calendar_method(object, MethodFunction::Changes),
    )
    .await
    .map_err(keyed)?;
    let origin = za_calendar_sync_boundary(server, access_token, owner, &request.since_state)
        .await
        .map_err(keyed)?;
    let since = request.since_state.clone();
    if let State::ZaCalendarIntermediate { range, .. } = &since {
        request.since_state = State::Intermediate(range.clone());
    }
    // PR #90 review C4617611565: a grantee's final event state is raised to
    // the tip of a snapshot read before the page. Upstream ends at the last
    // item change, which can lie below a sharing boundary (a container
    // change). From an initial or exact state the page read the log to its
    // end, so it covered every item change up to the tip. A `z` continuation
    // that finishes its pinned range covered only the range: its state is
    // raised only when the log holds no item change after it up to the tip,
    // which the client's next call would otherwise not be sent.
    let continues = matches!(since, State::ZaCalendarIntermediate { .. });
    let tip =
        if za_key && object == MethodObject::CalendarEvent && owner != access_token.account_id() {
            Some(
                server
                    .fetch_groupware_resources(
                        access_token.account_id(),
                        owner,
                        SyncCollection::Calendar,
                    )
                    .await
                    .caused_by(trc::location!())
                    .map_err(keyed)?
                    .highest_change_id,
            )
        } else {
            None
        };
    let mut changes = page(request).await.map_err(keyed)?;
    changes.response.old_state = since;
    if let (Some(tip), State::Exact(change_id)) = (tip, &changes.response.new_state)
        && *change_id < tip
    {
        let raise = !continues
            || server
                .store()
                .changes(
                    owner,
                    LogCollection::Sync(SyncCollection::Calendar),
                    Query::RangeInclusive(*change_id + 1, tip),
                )
                .await
                .caused_by(trc::location!())
                .map(|log| !log.is_truncated && log.item_change_id.is_none())
                .map_err(keyed)?;
        if raise {
            changes.response.new_state = State::Exact(tip);
        }
    }
    changes.response.new_state = za_calendar_reattach_origin(changes.response.new_state, origin);
    Ok(changes)
}

/// D15/D29. Classifies the account to select the wire format: `Some(origin)` for a
/// key-calendar initial or exact start or a `z` continuation, `None` for an ordinary
/// account or a key owner's legacy `r`. The owner, and an initial origin, take no
/// sharing-boundary check; an exact-origin grantee is checked against its original
/// exact state on every page, with CalDAV's predicate. A grantee's legacy `r` has no
/// origin and restarts. An ordinary account's `z` is an invalid argument.
pub(crate) async fn za_calendar_sync_boundary(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    since: &State,
) -> trc::Result<Option<ZaCalendarOrigin>> {
    if !server
        .za_is_key_account(account_id)
        .await
        .caused_by(trc::location!())?
    {
        return if matches!(since, State::ZaCalendarIntermediate { .. }) {
            Err(za_state_out_of_scope())
        } else {
            Ok(None)
        };
    }
    let is_owner = account_id == access_token.account_id();
    let Some(origin) = za_calendar_sync_origin(since) else {
        return if is_owner {
            Ok(None)
        } else {
            Err(za_sync_restart())
        };
    };
    if let (ZaCalendarOrigin::Exact(original), false) = (&origin, is_owner) {
        let resources = server
            .fetch_groupware_resources(
                access_token.account_id(),
                account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?;
        let contexts = match za_foreign_calendar_contexts(
            server,
            access_token,
            account_id,
            resources.document_ids(true),
            None,
        )
        .await
        {
            Ok(contexts) => contexts,
            Err(ZaPipelineError::Store(error)) => return Err(error),
            Err(_) => return Err(za_sync_restart()),
        };
        if contexts.values().any(|context| {
            context
                .boundary
                .is_some_and(|boundary| boundary.refuses(*original))
        }) {
            return Err(za_sync_restart());
        }
    }
    Ok(Some(origin))
}

/// D29: the start a request's state carries. Initial and exact states start a sync;
/// `z` carries the start of its sync unchanged. A legacy `r` carries none, and none
/// is derived from its range, the retained log or the current boundary.
fn za_calendar_sync_origin(since: &State) -> Option<ZaCalendarOrigin> {
    match since {
        State::Initial | State::Exact(0) => Some(ZaCalendarOrigin::Initial),
        State::Exact(change_id) => Some(ZaCalendarOrigin::Exact(*change_id)),
        State::ZaCalendarIntermediate { origin, .. } => Some(origin.clone()),
        State::Intermediate(_) => None,
    }
}

/// D29: an upstream intermediate result continues the sync that `origin` started.
/// Every other state, and a legacy chain without origin, is returned unchanged.
pub(crate) fn za_calendar_reattach_origin(state: State, origin: Option<ZaCalendarOrigin>) -> State {
    match (state, origin) {
        (State::Intermediate(range), Some(origin)) => {
            State::ZaCalendarIntermediate { range, origin }
        }
        (state, _) => state,
    }
}

#[cfg(test)]
mod tests {
    use super::{za_calendar_reattach_origin, za_calendar_sync_origin};
    use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};

    fn range(from_id: u64, to_id: u64, items_sent: usize) -> JMAPIntermediateState {
        JMAPIntermediateState {
            from_id,
            to_id,
            items_sent,
        }
    }

    #[test]
    fn sync_boundary_origins() {
        use ZaCalendarOrigin::{Exact, Initial};

        // Extraction reads the origin the client's state carries, never its range.
        assert_eq!(za_calendar_sync_origin(&State::Initial), Some(Initial));
        assert_eq!(za_calendar_sync_origin(&State::Exact(41)), Some(Exact(41)));
        // A zero exact ID parses as Initial, so it starts an Initial sync here too.
        assert_eq!(za_calendar_sync_origin(&State::Exact(0)), Some(Initial));
        for origin in [Initial, Exact(41)] {
            for range in [range(42, 50, 3), range(45, 60, 1)] {
                let since = State::ZaCalendarIntermediate {
                    range,
                    origin: origin.clone(),
                };
                assert_eq!(za_calendar_sync_origin(&since), Some(origin.clone()));
            }
        }
        // A legacy continuation carries no origin; none is invented from its range.
        assert_eq!(
            za_calendar_sync_origin(&State::Intermediate(range(42, 50, 3))),
            None
        );

        // Reattachment wraps an intermediate result with the unchanged origin, even
        // when the range moved: 42..50 at the start, 45..60 on a later page.
        let first =
            za_calendar_reattach_origin(State::Intermediate(range(42, 50, 1)), Some(Exact(41)));
        assert_eq!(
            first,
            State::ZaCalendarIntermediate {
                range: range(42, 50, 1),
                origin: Exact(41),
            }
        );
        let origin = za_calendar_sync_origin(&first);
        let later = za_calendar_reattach_origin(State::Intermediate(range(45, 60, 2)), origin);
        assert_eq!(
            later,
            State::ZaCalendarIntermediate {
                range: range(45, 60, 2),
                origin: Exact(41),
            }
        );
        assert_eq!(
            za_calendar_reattach_origin(State::Intermediate(range(1, 9, 4)), Some(Initial)),
            State::ZaCalendarIntermediate {
                range: range(1, 9, 4),
                origin: Initial,
            }
        );

        // Exact and initial results stay as upstream built them, with or without origin.
        for origin in [None, Some(Initial), Some(Exact(41))] {
            for state in [State::Initial, State::Exact(50)] {
                assert_eq!(
                    za_calendar_reattach_origin(state.clone(), origin.clone()),
                    state
                );
            }
        }
        // A legacy chain without origin stays legacy.
        assert_eq!(
            za_calendar_reattach_origin(State::Intermediate(range(42, 50, 2)), None),
            State::Intermediate(range(42, 50, 2))
        );
    }
}
