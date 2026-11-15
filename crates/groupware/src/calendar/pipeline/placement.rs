/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    context::za_mutation_context,
    error::{ZaPipelineError, ZaRefusal},
};
use crate::calendar::{
    CalendarEvent,
    seal::{ZaContainerOf, ZaContainerScope, ZaKeyDomain, reseal_container, seal_error},
    sharing::resolve::ZaCalendarContext,
};
use common::{
    Server,
    auth::{AccessToken, AccountCache},
    storage::metadata::{ContainerChange, MetadataLog},
};
use std::collections::BTreeSet;
use store::write::BatchBuilder;
use trc::AddContext;
use types::{
    acl::Acl,
    collection::Collection,
    metadata::{MetadataEdit, MetadataKinds},
};
use utils::map::bitmap::Bitmap;

/// A change of an event's calendars.
pub struct ZaPlacementPlan {
    /// The event's calendars now: the parents of its stored names, without duplicates, ascending.
    pub parents: Vec<u32>,
    /// How many names the stored event has. CalDAV's test for a linked event counts names.
    pub names: usize,
    /// The calendar the request addresses, resolved first. It must be one of `parents`.
    pub source: Option<u32>,
    /// The calendars the event leaves. CalDAV MOVE: the source. CalDAV COPY: none.
    pub left: Vec<u32>,
    /// The calendars the event enters, resolved last, in this order.
    /// CalDAV: the target, which may also be the source.
    pub entered: Vec<u32>,
    /// JMAP: the update also changes the content.
    pub content_changed: bool,
    /// CalDAV: the request overwrites a resource in the target.
    pub overwrite: bool,
    /// CalDAV: the event is copied or moved as part of a collection.
    pub collection: bool,
}

impl ZaPlacementPlan {
    pub fn caldav(
        event: &CalendarEvent,
        source: u32,
        target: u32,
        is_move: bool,
        overwrite: bool,
        collection: bool,
    ) -> Self {
        Self {
            parents: event
                .names
                .iter()
                .map(|name| name.parent_id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            names: event.names.len(),
            source: Some(source),
            left: if is_move { vec![source] } else { Vec::new() },
            entered: vec![target],
            content_changed: false,
            overwrite,
            collection,
        }
    }

    /// Whether the event leaves or enters any calendar.
    pub fn is_change(&self) -> bool {
        !self.left.is_empty() || !self.entered.is_empty()
    }

    pub fn parent_rights(&self) -> Bitmap<Acl> {
        let mut rights = Bitmap::new().with_item(if self.left.is_empty() {
            Acl::ReadItems
        } else {
            Acl::RemoveItems
        });
        if self.content_changed {
            rights = rights.with_item(Acl::ModifyItems);
        }
        rights
    }

    pub fn entered_rights(&self) -> Bitmap<Acl> {
        let mut rights = Bitmap::new().with_item(Acl::AddItems);
        if self.overwrite {
            rights = rights.with_item(Acl::RemoveItems);
        }
        rights
    }

    /// The refusal for a shared current calendar or a shared entered calendar.
    pub fn shared_refusal(&self, entered: bool) -> Option<ZaRefusal> {
        if self.left.is_empty() || self.collection {
            Some(ZaRefusal::SharedCopy)
        } else if self.names != 1 || (entered && self.entered.len() != 1) {
            Some(ZaRefusal::LinkedMove)
        } else {
            None
        }
    }

    fn steps(&self) -> impl Iterator<Item = (u32, bool)> + '_ {
        self.source
            .into_iter()
            .chain(
                self.parents
                    .iter()
                    .copied()
                    .filter(|&calendar_id| Some(calendar_id) != self.source),
            )
            .map(|calendar_id| (calendar_id, false))
            .chain(self.entered.iter().map(|&calendar_id| (calendar_id, true)))
    }
}

pub struct ZaPlacementContexts {
    /// One context per current calendar: the source first when named, the rest in ascending id.
    pub parents: Vec<ZaCalendarContext>,
    /// One context per entered calendar, in the plan's order, even when it is also a parent.
    pub entered: Vec<ZaCalendarContext>,
}

impl ZaPlacementContexts {
    /// A CalDAV PUT, or a JMAP update that keeps the event's calendars.
    pub fn unchanged(contexts: Vec<ZaCalendarContext>) -> Self {
        Self {
            parents: contexts,
            entered: Vec::new(),
        }
    }

    /// The domain the stored event is sealed in.
    pub fn from_domain(&self) -> &ZaKeyDomain {
        &self.parents[0].domain
    }

    /// The last entered calendar's domain, or the stored event's domain when none is entered.
    pub fn to_domain(&self) -> &ZaKeyDomain {
        self.entered
            .last()
            .map_or_else(|| self.from_domain(), |context| &context.domain)
    }

    pub fn assert_in(&self, batch: &mut BatchBuilder) {
        for context in self.parents.iter().chain(&self.entered) {
            context.assert_in(batch);
        }
    }

    /// Parents, then entered: CalDAV indexes the source first and the target last.
    pub fn into_vec(mut self) -> Vec<ZaCalendarContext> {
        self.parents.extend(self.entered);
        self.parents
    }
}

/// Fresh contexts for a placement change, checked in the plan's order.
pub async fn za_placement_contexts(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    plan: &ZaPlacementPlan,
) -> Result<ZaPlacementContexts, ZaPipelineError> {
    let parent_rights = plan.parent_rights();
    let entered_rights = plan.entered_rights();
    let mut contexts = ZaPlacementContexts {
        parents: Vec::with_capacity(plan.parents.len()),
        entered: Vec::with_capacity(plan.entered.len()),
    };

    for (calendar_id, entered) in plan.steps() {
        let required = if entered {
            entered_rights
        } else {
            parent_rights
        };
        let context = za_mutation_context(server, token, owner_id, calendar_id, required)
            .await
            .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, calendar_id))?
            .ok_or(ZaPipelineError::Forbidden(ZaRefusal::Context))?;
        if !entered && plan.source == Some(calendar_id) && !plan.parents.contains(&calendar_id) {
            return Err(ZaPipelineError::NotFound);
        }
        if context.domain.calendar_id().is_some() {
            match plan.shared_refusal(entered) {
                Some(ZaRefusal::SharedCopy) => return Err(za_shared_copy_refusal(owner_id)),
                Some(ZaRefusal::LinkedMove) => return Err(za_linked_move_refusal(owner_id)),
                None => (),
                Some(ZaRefusal::NoKeys | ZaRefusal::Context) => {
                    unreachable!("unexpected shared placement refusal")
                }
            }
        }
        if entered {
            contexts.entered.push(context);
        } else {
            contexts.parents.push(context);
        }
    }

    Ok(contexts)
}

pub fn za_shared_copy_refusal(owner_id: u32) -> ZaPipelineError {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: copy into or out of a shared calendar"
    );
    ZaPipelineError::Forbidden(ZaRefusal::SharedCopy)
}

pub fn za_linked_move_refusal(owner_id: u32) -> ZaPipelineError {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: event linked into a shared calendar"
    );
    ZaPipelineError::Forbidden(ZaRefusal::LinkedMove)
}

/// Open the stored event container in `from` and queue its reseal in `to` under a fresh data key.
pub async fn za_reseal_event_container(
    server: &Server,
    account: &AccountCache,
    document_id: u32,
    uid: &str,
    kinds: MetadataKinds,
    from: &ZaKeyDomain,
    to: &ZaKeyDomain,
    batch: &mut BatchBuilder,
) -> trc::Result<()> {
    if kinds.is_empty() {
        return Ok(());
    }
    let Some(stored) = server
        .metadata_container(account.id, Collection::CalendarEvent, document_id)
        .await?
    else {
        return Ok(());
    };
    let resealed = reseal_container(
        stored.view().as_bytes(),
        from,
        ZaContainerOf::Event(uid),
        to,
        ZaContainerScope::Event(uid),
    )
    .map_err(|error| seal_error(error, account.id, document_id))?;
    if let Some(change) = ContainerChange::new(Some(&stored), Some(resealed), MetadataEdit::Write) {
        change
            .into_write(account, Collection::CalendarEvent, MetadataLog::None)
            .build(document_id.into(), batch)
            .caused_by(trc::location!())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::calendar::{
        CalendarEvent,
        pipeline::{error::ZaRefusal, placement::ZaPlacementPlan},
    };
    use common::DavName;
    use types::acl::Acl;
    use utils::map::bitmap::Bitmap;

    fn event_with_parents(parents: &[u32]) -> CalendarEvent {
        CalendarEvent {
            names: parents
                .iter()
                .enumerate()
                .map(|(index, &parent_id)| DavName {
                    name: format!("event-{index}.ics"),
                    parent_id,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn caldav_plans_keep_todays_rule() {
        let read = Bitmap::new().with_item(Acl::ReadItems);
        let remove = Bitmap::new().with_item(Acl::RemoveItems);
        let add = Bitmap::new().with_item(Acl::AddItems);
        let replace = add.with_item(Acl::RemoveItems);
        let copy = Some(ZaRefusal::SharedCopy);
        let linked = Some(ZaRefusal::LinkedMove);

        // is_move, overwrite, collection, multiple names, parent rights,
        // entered rights, shared source or target, shared other parent.
        let rows = [
            (false, false, false, false, read, add, copy, copy),
            (false, false, false, true, read, add, copy, copy),
            (false, false, true, false, read, add, copy, copy),
            (false, false, true, true, read, add, copy, copy),
            (false, true, false, false, read, replace, copy, copy),
            (false, true, false, true, read, replace, copy, copy),
            (false, true, true, false, read, replace, copy, copy),
            (false, true, true, true, read, replace, copy, copy),
            (true, false, false, false, remove, add, None, linked),
            (true, false, false, true, remove, add, linked, linked),
            (true, false, true, false, remove, add, copy, copy),
            (true, false, true, true, remove, add, copy, copy),
            (true, true, false, false, remove, replace, None, linked),
            (true, true, false, true, remove, replace, linked, linked),
            (true, true, true, false, remove, replace, copy, copy),
            (true, true, true, true, remove, replace, copy, copy),
        ];
        let placements: [(&[u32], &[u32]); 4] = [
            (&[1], &[1]),
            (&[1, 1], &[1]),
            (&[1, 2], &[1, 2]),
            (&[1, 2, 3], &[1, 2, 3]),
        ];

        for (is_move, overwrite, collection, multiple, current, entered, shared, other) in rows {
            for (parents, distinct) in placements {
                if multiple != (parents.len() > 1) {
                    continue;
                }
                let event = event_with_parents(parents);
                for target in [1, 2, 9] {
                    let plan =
                        ZaPlacementPlan::caldav(&event, 1, target, is_move, overwrite, collection);
                    assert_eq!(plan.parent_rights(), current);
                    assert_eq!(plan.entered_rights(), entered);
                    assert_eq!(plan.shared_refusal(false), shared);
                    assert_eq!(plan.shared_refusal(true), shared);
                    if distinct.len() > 1 {
                        assert_eq!(plan.shared_refusal(false), other);
                    }
                    assert_eq!(plan.parents, distinct);
                    assert_eq!(plan.names, parents.len());
                    assert_eq!(plan.source, Some(1));
                    if is_move {
                        assert_eq!(plan.left, [1]);
                    } else {
                        assert!(plan.left.is_empty());
                    }
                    assert_eq!(plan.entered, [target]);
                    assert!(!plan.content_changed);
                    assert!(plan.is_change());
                }
            }
        }
    }

    #[test]
    fn caldav_plan_order() {
        let event = event_with_parents(&[7, 3, 5]);
        let cases: [(u32, u32, bool, &[(u32, bool)]); 4] = [
            (5, 9, true, &[(5, false), (3, false), (7, false), (9, true)]),
            (5, 5, true, &[(5, false), (3, false), (7, false), (5, true)]),
            (
                5,
                3,
                false,
                &[(5, false), (3, false), (7, false), (3, true)],
            ),
            (
                4,
                9,
                true,
                &[(4, false), (3, false), (5, false), (7, false), (9, true)],
            ),
        ];
        for (source, target, is_move, expected) in cases {
            let plan = ZaPlacementPlan::caldav(&event, source, target, is_move, false, false);
            assert_eq!(plan.steps().collect::<Vec<_>>(), expected);
        }
    }
}
