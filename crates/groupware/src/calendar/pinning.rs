/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key pinning spec 6.4 step 2: the full current state of one withheld
//! delivery, rebuilt from the sender's stored event for one recipient
//! through upstream's builders and export helpers, unchanged (invariant
//! 15). `contacts/pin` sends it alone; the fan-out's two-item rule sends it
//! ahead of a change's own messages (spec 5.1). Also the fork's DECLINED
//! for an attendee's DELETE that upstream sends nothing for (spec 5.1 item
//! 2, `za_decline_all`).

use crate::{
    calendar::{drain::instance_key, seal::TokenPairs},
    scheduling::{
        Email, InstanceId, ItipError, ItipField, ItipMessage, ItipSnapshot, ItipSnapshots,
        ItipSummary, ItipValue, RecurrenceId,
        event_cancel::CancelScope,
        itip::{
            ItipExportAs, itip_add_tz, itip_build_envelope, itip_date_entry, itip_export_component,
        },
        organizer::organizer_request_full,
        recipient::{RecipientPolicy, itip_messages_per_recipient},
        snapshot::itip_snapshot,
    },
};
use ahash::AHashSet;
use calcard::{
    common::PartialDateTime,
    icalendar::{
        ICalendar, ICalendarComponent, ICalendarEntry, ICalendarMethod, ICalendarParameter,
        ICalendarParticipationStatus, ICalendarProperty, ICalendarStatus, ICalendarValue,
    },
};
use common::auth::vault::za_normalize_address;
use vault::pins::ZaInstances;

/// The organizer's full REQUEST for one attendee (spec 6.4 step 2, the path
/// the drain's delegating merge uses for a new delegate):
/// `organizer_request_full` over the stored tree with no SEQUENCE bump
/// (no `itip_finalize`), its recipients restricted to `recipient`, compared
/// normalized (spec 4.3), then split per recipient under `policy` as 1.0's
/// `itip_update` splits upstream's full update (`itip_messages_per_recipient`,
/// upgrade ruling 91): each address gets its own slice, the instances it
/// attends, and no attendee list `policy` hides. One message per address
/// the recipient normalizes from, usually one. Empty when the builder
/// errs, the tree is not organized by one of `addresses`, or the REQUEST
/// does not reach the recipient (DECLINED without SCHEDULE-FORCE-SEND,
/// RSVP=FALSE, SCHEDULE-AGENT=CLIENT or NONE: upstream's
/// `send_update_messages`). The split's error is returned (upgrade ruling
/// 135, M10): it is a failure, not "nothing to send", so the entry stays.
pub fn za_full_request_for(
    tree: &ICalendar,
    addresses: &[String],
    recipient: &str,
    policy: RecipientPolicy,
) -> Result<Vec<ItipMessage<ICalendar>>, ItipError> {
    let Some(recipient) = za_normalize_address(recipient) else {
        return Ok(Vec::new());
    };
    let Ok(snapshots) = itip_snapshot(tree, addresses, false) else {
        return Ok(Vec::new());
    };
    // Only the organizer's own copy builds a REQUEST.
    if !snapshots.organizer.email.is_local {
        return Ok(Vec::new());
    }
    let Some(message) = organizer_request_full(tree, &snapshots, None, false)
        .ok()
        .and_then(|messages| {
            messages.into_iter().find_map(|mut message| {
                message
                    .to
                    .retain(|to| za_normalize_address(to).as_deref() == Some(recipient.as_str()));
                (!message.to.is_empty()).then_some(message)
            })
        })
    else {
        return Ok(Vec::new());
    };
    itip_messages_per_recipient(vec![message], policy)
}

/// An organizer-side entry's full state (spec 5.1 item 1, 6.4 step 2):
/// `za_full_request_for`, then the per-instance CANCELs of the entry's
/// recorded instances (`za_withheld_cancels`, upgrade ruling 132) to the
/// address forms the full REQUEST reached. When the slice is empty (the
/// address DECLINED every instance it attends without SCHEDULE-FORCE-SEND,
/// or has RSVP=FALSE there), the CANCELs still go, to the address forms the
/// tree names for the recipient on an ATTENDEE the server schedules
/// (`za_scheduled_forms`, upgrade ruling 153): the replay does not depend on
/// a REQUEST going out. A form the tree marks SCHEDULE-AGENT=CLIENT or NONE
/// gets nothing (RFC 6638: the server does not schedule for it). A split
/// error of either is returned.
pub fn za_full_state_for(
    tree: &ICalendar,
    addresses: &[String],
    recipient: &str,
    instances: &ZaInstances,
    policy: RecipientPolicy,
) -> Result<Vec<ItipMessage<ICalendar>>, ItipError> {
    let mut messages = za_full_request_for(tree, addresses, recipient, policy)?;
    let mut to: Vec<String> = Vec::new();
    for address in messages.iter().flat_map(|message| message.to.iter()) {
        if !to.contains(address) {
            to.push(address.clone());
        }
    }
    if to.is_empty() {
        to = za_scheduled_forms(tree, addresses, recipient);
    }
    messages.extend(za_withheld_cancels(
        tree, addresses, recipient, &to, instances, policy,
    )?);
    Ok(messages)
}

/// Upgrade ruling 153: the address forms of every ATTENDEE of the organized
/// `tree` that normalizes to `recipient` and that the server schedules (no
/// SCHEDULE-AGENT=CLIENT or NONE), each once, sorted. Empty when the tree is
/// not organized by one of `addresses`, or names no such ATTENDEE.
fn za_scheduled_forms(tree: &ICalendar, addresses: &[String], recipient: &str) -> Vec<String> {
    let Some(recipient) = za_normalize_address(recipient) else {
        return Vec::new();
    };
    let Ok(snapshots) = itip_snapshot(tree, addresses, false) else {
        return Vec::new();
    };
    if !snapshots.organizer.email.is_local {
        return Vec::new();
    }
    let mut forms: Vec<String> = Vec::new();
    for attendee in snapshots
        .components
        .values()
        .flat_map(|instance| instance.attendees.iter())
    {
        if attendee.is_server_scheduling
            && za_normalize_address(&attendee.email.email).as_deref() == Some(recipient.as_str())
            && !forms.contains(&attendee.email.email)
        {
            forms.push(attendee.email.email.clone());
        }
    }
    forms.sort_unstable();
    forms
}

/// Upgrade ruling 152: the properties a replayed CANCEL component keeps
/// (UID, ORGANIZER, RECURRENCE-ID, SEQUENCE, DTSTAMP and STATUS); every
/// content property upstream's cancel builder copies (SUMMARY, DESCRIPTION,
/// LOCATION, CREATED, LAST-MODIFIED, DTSTART, DTEND, DURATION, DUE) and any
/// copied ATTENDEE line go. The caller then adds DTSTART at the
/// RECURRENCE-ID's value and the recipient's own ATTENDEE lines.
fn za_cancel_only(component: &mut ICalendarComponent) {
    component.entries.retain(|entry| {
        matches!(
            entry.name,
            ICalendarProperty::Uid
                | ICalendarProperty::Organizer
                | ICalendarProperty::RecurrenceId
                | ICalendarProperty::Sequence
                | ICalendarProperty::Dtstamp
                | ICalendarProperty::Status
        )
    });
}

/// Upgrade ruling 132 (spec 5.1 item 1): the removals a full state cannot
/// express. An organizer-side entry records the instance keys of the
/// per-instance CANCELs the fan-out refused for its address (spec 4.1).
/// The address's slice carries only the instances it attends. In 1.0.3 a
/// full REQUEST removes an omitted override only when its stored SEQUENCE
/// is at or below the master's. A slice without the master removes nothing.
/// After the slice, one CANCEL from the organizer to `to` carries a
/// component for each recorded instance the committed `tree` no longer has
/// the address attend, built by upstream's cancel builder
/// (`CancelScope::Instance` over the address forms of `to`, so the
/// component lists none of the instance's current attendees) at the
/// instance's committed SEQUENCE, then split per recipient under `policy`.
/// Upgrade ruling 152: the component carries no content. Only UID,
/// ORGANIZER, RECURRENCE-ID, SEQUENCE, DTSTAMP and STATUS:CANCELLED are
/// kept from the builder's (`za_cancel_only`), so neither the instance's
/// current content (edits made after the address left it) nor the
/// master's reaches the recipient, and DTSTART is the RECURRENCE-ID's
/// value. Neither merge fails without it (an override present in the copy
/// keeps its own DTSTART, and calcard's expansion falls back to the
/// RECURRENCE-ID), but upstream's CANCEL merge stores a component the copy
/// lacks as the copy's cancelled override, and a stored VEVENT needs a
/// DTSTART (RFC 5545 3.6.1), as upstream's own exclusion CANCEL carries. The
/// iTIP summary lists the cancelled instances' RECURRENCE-ID times alone.
/// Each component names the recipient with a bare ATTENDEE line per
/// address of `to`: the tree no longer holds its line, which upstream's
/// own CANCEL copies from the superseded tree, and the recipient's
/// snapshot of a message that names none of its addresses fails
/// (`NotOrganizerNorAttendee`), so its drain would not merge it.
/// A recorded instance the tree has no component for is built from the
/// master's UID and ORGANIZER, its RECURRENCE-ID and DTSTART the recorded
/// UTC instant (with RANGE=THISANDFUTURE when recorded so), at the master's
/// SEQUENCE, as upstream builds an exclusion's CANCEL; it is skipped when
/// the address attends the master (a master instance it attends, or an
/// EXDATE the slice carries). An instance the address attends again gets no
/// component.
/// Empty when the entry recorded no instance key, overflowed (`all`: today's
/// behaviour, a recorded limit), `to` is empty, the tree is not organized
/// by one of `addresses`, or no recorded instance qualifies. Only the
/// split's error is returned.
pub fn za_withheld_cancels(
    tree: &ICalendar,
    addresses: &[String],
    recipient: &str,
    to: &[String],
    instances: &ZaInstances,
    policy: RecipientPolicy,
) -> Result<Vec<ItipMessage<ICalendar>>, ItipError> {
    if instances.all || instances.keys.is_empty() || to.is_empty() {
        return Ok(Vec::new());
    }
    let Some(recipient) = za_normalize_address(recipient) else {
        return Ok(Vec::new());
    };
    let Ok(snapshots) = itip_snapshot(tree, addresses, false) else {
        return Ok(Vec::new());
    };
    if !snapshots.organizer.email.is_local {
        return Ok(Vec::new());
    }
    let attends = |snapshot: &ItipSnapshot<'_>| {
        snapshot.attendees.iter().any(|attendee| {
            za_normalize_address(&attendee.email.email).as_deref() == Some(recipient.as_str())
        })
    };
    let scope: Vec<&str> = to.iter().map(String::as_str).collect();
    let main = snapshots.main_instance();
    let dt_stamp = PartialDateTime::now();
    let mut message = ICalendar {
        components: Vec::with_capacity(instances.keys.len() + 1),
    };
    message
        .components
        .push(itip_build_envelope(ICalendarMethod::Cancel));
    let mut cancelled: Vec<InstanceId> = Vec::new();
    let mut keys = instances.keys.clone();
    keys.sort_unstable();
    for (date, this_and_future) in keys {
        let present = snapshots
            .components
            .iter()
            .find(|(id, _)| instance_key(id) == Some((date, this_and_future)));
        let mut component = match present {
            Some((_, snapshot)) if attends(snapshot) => continue,
            Some((id, snapshot)) => {
                cancelled.push(id.clone());
                let mut component = CancelScope::Instance(&scope).build_component(
                    snapshot.comp,
                    snapshot.sequence.unwrap_or_default(),
                    dt_stamp.clone(),
                );
                za_cancel_only(&mut component);
                let dt_start = component
                    .entries
                    .iter()
                    .find(|entry| entry.name == ICalendarProperty::RecurrenceId)
                    .map(|entry| itip_date_entry(ICalendarProperty::Dtstart, entry));
                component.entries.extend(dt_start);
                component
            }
            None => {
                let Some(main) = main.filter(|main| !attends(main)) else {
                    continue;
                };
                cancelled.push(InstanceId::Recurrence(RecurrenceId {
                    entry_id: 0,
                    date,
                    this_and_future,
                }));
                let mut component = CancelScope::Instance(&scope).build_component(
                    main.comp,
                    main.sequence.unwrap_or_default(),
                    dt_stamp.clone(),
                );
                za_cancel_only(&mut component);
                let value =
                    ICalendarValue::PartialDateTime(PartialDateTime::from_utc_timestamp(date));
                component.entries.push(ICalendarEntry {
                    name: ICalendarProperty::RecurrenceId,
                    params: if this_and_future {
                        vec![ICalendarParameter::range(true)]
                    } else {
                        vec![]
                    },
                    values: [value.clone()].into(),
                });
                component.entries.push(ICalendarEntry {
                    name: ICalendarProperty::Dtstart,
                    params: vec![],
                    values: [value].into(),
                });
                component
            }
        };
        for address in to {
            component.add_property(
                ICalendarProperty::Attendee,
                ICalendarValue::Text(format!("mailto:{address}")),
            );
        }
        let comp_id = message.components.len() as u32;
        message.components[0].component_ids.push(comp_id);
        message.components.push(component);
    }
    if cancelled.is_empty() {
        return Ok(Vec::new());
    }
    itip_add_tz(&mut message, tree);
    itip_messages_per_recipient(
        vec![ItipMessage {
            from: snapshots.organizer.email.email.clone(),
            from_organizer: true,
            to: to.to_vec(),
            summary: ItipSummary::Cancel(recurrence_times(&snapshots, &cancelled)),
            message,
        }],
        policy,
    )
}

/// Upgrade ruling 152: a replayed CANCEL's iTIP summary, one DTSTART field
/// per cancelled instance at its RECURRENCE-ID's time, and no SUMMARY,
/// DESCRIPTION, LOCATION, CONFERENCE or guest list. An instance in the tree
/// takes its RECURRENCE-ID's own time (`recurrence_time`), not its DTSTART;
/// one generated from the master takes the recorded instant in the master's
/// time zone (`instance_time` of an absent instance).
fn recurrence_times(snapshots: &ItipSnapshots<'_>, cancelled: &[InstanceId]) -> Vec<ItipField> {
    let mut fields: Vec<ItipField> = Vec::with_capacity(cancelled.len());
    for id in cancelled {
        let time = match snapshots.components.get(id) {
            Some(instance) => instance.recurrence_time,
            None => snapshots.instance_time(id),
        };
        let Some(time) = time else {
            continue;
        };
        let field = ItipField {
            name: ICalendarProperty::Dtstart,
            value: ItipValue::Time(time),
        };
        if !fields.contains(&field) {
            fields.push(field);
        }
    }
    fields
}

/// The refused instances' current participation (spec 6.4 step 2): a REPLY
/// to the ORGANIZER of every recorded instance (every instance under
/// `all`) present in `tree`, the master first, whose local attendee has
/// exactly the address `from` (`handle_reply` matches per instance by exact
/// string) and is scheduled by the server, and whose STATUS is not
/// CANCELLED. An ORGANIZER with SCHEDULE-AGENT=CLIENT or NONE fails the
/// snapshot, as it fails upstream's reply path, so nothing is built. Each
/// instance is exported as upstream's attendee update exports it
/// (`attendee_handle_update`): the local attendee's line plus the lines of
/// the attendees delegated from or to it. RSVP is not consulted, as
/// upstream's reply path does not. `None` when no instance qualifies.
pub fn za_participation_reply(
    tree: &ICalendar,
    addresses: &[String],
    instances: &ZaInstances,
    from: &str,
) -> Option<ItipMessage<ICalendar>> {
    let snapshots = itip_snapshot(tree, addresses, false).ok()?;
    let dt_stamp = PartialDateTime::now();
    let mut message = ICalendar {
        components: Vec::with_capacity(2),
    };
    message
        .components
        .push(itip_build_envelope(ICalendarMethod::Reply));
    let mut part_stat = None;
    for (_, instance) in recorded_instances(&snapshots, instances) {
        if matches!(instance.comp.status(), Some(ICalendarStatus::Cancelled)) {
            continue;
        }
        let Some(local_attendee) = instance
            .attendees
            .iter()
            .find(|attendee| attendee.email.is_local && attendee.email.email == from)
            .filter(|attendee| attendee.is_server_scheduling)
        else {
            continue;
        };
        let mut attendee_entry_ids = vec![local_attendee.entry_id];
        for external_attendee in instance.external_attendees() {
            if external_attendee.is_delegated_from(local_attendee)
                || external_attendee.is_delegated_to(local_attendee)
            {
                attendee_entry_ids.push(external_attendee.entry_id);
            }
        }
        part_stat.get_or_insert(local_attendee.part_stat);
        let comp_id = message.components.len() as u32;
        message.components[0].component_ids.push(comp_id);
        message.components.push(itip_export_component(
            instance.comp,
            snapshots.uid,
            &dt_stamp,
            instance.sequence.unwrap_or_default(),
            ItipExportAs::Attendee(attendee_entry_ids),
            None,
        ));
    }
    let part_stat = part_stat?;
    itip_add_tz(&mut message, tree);
    Some(ItipMessage {
        from: from.to_string(),
        from_organizer: false,
        to: vec![snapshots.organizer.email.email.clone()],
        summary: ItipSummary::Rsvp {
            part_stat: part_stat
                .cloned()
                .unwrap_or(ICalendarParticipationStatus::NeedsAction),
            current: snapshots
                .main_instance_or_default()
                .build_summary(Some(&snapshots.organizer), &[]),
        },
        message,
    })
}

/// The addresses `za_participation_reply` replies from, one REPLY each
/// (spec 6.4 step 2): the local attendee of each recorded instance present
/// in `tree` (the first local one, as upstream's attendee update takes it),
/// the master first, then in instance order, each address once. Empty when
/// the tree has no snapshot.
pub fn za_reply_addresses(
    tree: &ICalendar,
    addresses: &[String],
    instances: &ZaInstances,
) -> Vec<String> {
    let Ok(snapshots) = itip_snapshot(tree, addresses, false) else {
        return Vec::new();
    };
    let mut from: Vec<String> = Vec::new();
    for (_, instance) in recorded_instances(&snapshots, instances) {
        if let Some(attendee) = instance.local_attendee()
            && !from.contains(&attendee.email.email)
        {
            from.push(attendee.email.email.clone());
        }
    }
    from
}

/// The refused instances' current participation as sent (spec 5.1 item 1,
/// 6.4 step 2): one `za_participation_reply` per address of
/// `za_reply_addresses` whose token the attendee's copy holds in `pairs`
/// (the lookup the fan-out's route makes for a REPLY's `from`). A REPLY
/// without its sender's token is never attempted and the organizer's drain
/// would drop it as a forgery, so none is built. Empty when no address
/// qualifies.
pub fn za_participation_replies(
    tree: &ICalendar,
    addresses: &[String],
    instances: &ZaInstances,
    pairs: Option<&TokenPairs>,
) -> Vec<ItipMessage<ICalendar>> {
    let Some(pairs) = pairs else {
        return Vec::new();
    };
    za_reply_addresses(tree, addresses, instances)
        .iter()
        .filter(|from| pairs.get(from).is_some())
        .filter_map(|from| za_participation_reply(tree, addresses, instances, from))
        .collect()
}

/// Key pinning spec 5.1 item (2): an attendee's DECLINED of every instance
/// when it deletes its copy while holding a withheld entry for the
/// organizer, built even where upstream builds nothing. A fork copy of
/// upstream's `itip_cancel` attendee branch (`event_cancel.rs` lines
/// 82-127, `is_deletion` true) over `decline_instance`, a fork copy of
/// `attendee_decline` (`attendee.rs` lines 279-364). The one difference is
/// that copy's local-attendee gate: `is_server_scheduling` and the RSVP
/// check without the PARTSTAT match of `attendee.rs` lines 301-308, so a
/// DECLINED or DELEGATED attendee without SCHEDULE-FORCE-SEND (and one at
/// NEEDS-ACTION) declines too. `None` when the tree has no snapshot, is the
/// organizer's own, or no instance has a qualifying local attendee.
pub fn za_decline_all(ical: &ICalendar, addresses: &[String]) -> Option<ItipMessage<ICalendar>> {
    let itip = itip_snapshot(ical, addresses, false).ok()?;
    if itip.organizer.email.is_local {
        return None;
    }
    let dt_stamp = PartialDateTime::now();
    let mut message = ICalendar {
        components: Vec::with_capacity(2),
    };
    message
        .components
        .push(itip_build_envelope(ICalendarMethod::Reply));

    // Decline attendance for all instances that have local attendees
    let mut mail_from = None;
    let mut email_rcpt = AHashSet::new();
    for (instance_id, comp) in &itip.components {
        if let Some((cancel_comp, attendee_email)) =
            decline_instance(instance_id, &itip, comp, &dt_stamp, &mut email_rcpt)
        {
            // Add cancel component
            let comp_id = message.components.len() as u32;
            message.components[0].component_ids.push(comp_id);
            message.components.push(cancel_comp);
            mail_from = Some(&attendee_email.email);
        }
    }

    let from = mail_from?;
    // Add timezone information if needed
    itip_add_tz(&mut message, ical);

    email_rcpt.insert(&itip.organizer.email.email);

    Some(ItipMessage {
        from: from.to_string(),
        from_organizer: false,
        to: email_rcpt.into_iter().map(|e| e.to_string()).collect(),
        summary: ItipSummary::Rsvp {
            part_stat: ICalendarParticipationStatus::Declined,
            current: itip.main_instance_or_default().build_summary(None, &[]),
        },
        message,
    })
}

/// `attendee_decline` (`attendee.rs` lines 279-364) with its local-attendee
/// gate reduced to `is_server_scheduling` and the RSVP check (see
/// `za_decline_all`); otherwise line for line.
fn decline_instance<'x>(
    instance_id: &'x InstanceId,
    itip: &'x ItipSnapshots<'x>,
    comp: &'x ItipSnapshot<'x>,
    dt_stamp: &'x PartialDateTime,
    email_rcpt: &mut AHashSet<&'x str>,
) -> Option<(ICalendarComponent, &'x Email)> {
    let component = comp.comp;
    let mut cancel_comp = ICalendarComponent {
        component_type: component.component_type.clone(),
        entries: Vec::with_capacity(5),
        component_ids: vec![],
    };

    let mut local_attendee = None;
    let mut delegated_from = None;

    for attendee in &comp.attendees {
        if attendee.email.is_local {
            if attendee.is_server_scheduling && attendee.rsvp.is_none_or(|rsvp| rsvp) {
                local_attendee = Some(attendee);
            }
        } else if attendee.delegated_to.iter().any(|d| d.is_local) {
            cancel_comp
                .entries
                .push(component.entries[attendee.entry_id as usize].clone());
            delegated_from = Some(&attendee.email.email);
        }
    }

    local_attendee.map(|local_attendee| {
        cancel_comp.add_property(
            ICalendarProperty::Organizer,
            ICalendarValue::Text(itip.organizer.email.to_string()),
        );
        cancel_comp.add_property_with_params(
            ICalendarProperty::Attendee,
            [ICalendarParameter::partstat(
                ICalendarParticipationStatus::Declined,
            )],
            ICalendarValue::Text(local_attendee.email.to_string()),
        );
        cancel_comp.add_uid(itip.uid);
        cancel_comp.add_dtstamp(dt_stamp.clone());
        cancel_comp.add_sequence(comp.sequence.unwrap_or_default());
        cancel_comp.entries.extend(
            component
                .entries
                .iter()
                .filter(|e| {
                    matches!(
                        e.name,
                        ICalendarProperty::Dtstart
                            | ICalendarProperty::Dtend
                            | ICalendarProperty::Duration
                            | ICalendarProperty::Due
                            | ICalendarProperty::Description
                            | ICalendarProperty::Summary
                    )
                })
                .cloned(),
        );

        if let InstanceId::Recurrence(recurrence_id) = instance_id {
            cancel_comp
                .entries
                .push(component.entries[recurrence_id.entry_id as usize].clone());
        }
        if let Some(delegated_from) = delegated_from {
            email_rcpt.insert(delegated_from);
        }

        (cancel_comp, &local_attendee.email)
    })
}

/// The instances of `snapshots` that `instances` records, the master first,
/// then by instant (the drain's `InstanceKey`).
fn recorded_instances<'s, 'x>(
    snapshots: &'s ItipSnapshots<'x>,
    instances: &ZaInstances,
) -> Vec<(&'s InstanceId, &'s ItipSnapshot<'x>)> {
    let mut recorded: Vec<(&InstanceId, &ItipSnapshot<'x>)> = snapshots
        .components
        .iter()
        .filter(|(id, _)| instances.contains(instance_key(id)))
        .collect();
    recorded.sort_by_key(|(id, _)| instance_key(id));
    recorded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        calendar::drain::{InstanceKey, instance_key},
        scheduling::{
            event_cancel::itip_cancel,
            inbound::itip_method,
            recipient::{AttendeeVisibility, RecipientPolicy},
            snapshot::itip_snapshot,
        },
    };

    fn policy() -> RecipientPolicy {
        RecipientPolicy {
            visibility: AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        }
    }
    use calcard::{Entry, Parser, common::IanaString};

    const ALICE: &str = "key1@example.com";
    const BOB: &str = "key2@example.com";
    // The instants of the overrides of `COPY`: 2099-03-02 .. 2099-03-06 at
    // 10:00 UTC.
    const A: (i64, bool) = (4_076_128_800, false);
    const B: (i64, bool) = (4_076_215_200, false);
    const C: (i64, bool) = (4_076_301_600, false);
    const D: (i64, bool) = (4_076_388_000, false);
    const E: (i64, bool) = (4_076_474_400, false);

    /// bob's copy of alice's daily event: the master ACCEPTED; A TENTATIVE
    /// with RSVP=FALSE; B DECLINED; C STATUS:CANCELLED naming bob; D with
    /// bob SCHEDULE-AGENT=CLIENT; E naming bob.alias@example.com only.
    const COPY: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=10\r\n",
        "SEQUENCE:0\r\nSUMMARY:pin-unit\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=ACCEPTED:mailto:key2@example.com\r\n",
        "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:key3@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990302T100000Z\r\nDTSTART:20990302T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=TENTATIVE;RSVP=FALSE:mailto:key2@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990303T100000Z\r\nDTSTART:20990303T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=DECLINED:mailto:key2@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990304T100000Z\r\nDTSTART:20990304T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:1\r\nSTATUS:CANCELLED\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=ACCEPTED:mailto:key2@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990305T100000Z\r\nDTSTART:20990305T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;SCHEDULE-AGENT=CLIENT;PARTSTAT=ACCEPTED:mailto:key2@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990306T100000Z\r\nDTSTART:20990306T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob.alias@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    /// bob DELEGATED to carol on the master, carol DELEGATED-FROM bob.
    const DELEGATED: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit-d\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
        "ORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\":mailto:key2@example.com\r\n",
        "ATTENDEE;DELEGATED-FROM=\"mailto:key2@example.com\";PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key3@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    /// alice's event: bob expects a reply, one attendee declined, one
    /// takes no reply.
    const ORGANIZED: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit-o\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nSEQUENCE:2\r\n",
        "SUMMARY:pin-unit-o\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
        "ATTENDEE;PARTSTAT=DECLINED:mailto:declined@example.com\r\n",
        "ATTENDEE;RSVP=FALSE:mailto:rsvp-false@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    /// bob's copy of a daily event: bob SCHEDULE-AGENT=CLIENT on the master,
    /// RSVP=FALSE on override A. A's server-scheduled line gives the copy a
    /// snapshot (a copy whose only local line is client-scheduled has none),
    /// so each instance reaches the gate.
    const CLIENT_SCHEDULED: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit-c\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=10\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;SCHEDULE-AGENT=CLIENT;PARTSTAT=DECLINED:mailto:key2@example.com\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:pin-unit-c\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20990302T100000Z\r\nDTSTART:20990302T100000Z\r\nDURATION:PT1H\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:key1@example.com\r\n",
        "ATTENDEE;PARTSTAT=DECLINED;RSVP=FALSE:mailto:key2@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    /// bob's copy of alice's event with bob's line `attendee` (its
    /// parameters, ahead of the address).
    fn copy_with(attendee: &str) -> ICalendar {
        parse(&format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
                "BEGIN:VEVENT\r\nUID:pin-unit-x\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
                "ORGANIZER:mailto:key1@example.com\r\n",
                "ATTENDEE;{attendee}:mailto:key2@example.com\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            attendee = attendee
        ))
    }

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn alice() -> Vec<String> {
        vec![ALICE.to_string()]
    }

    fn bob() -> Vec<String> {
        vec![BOB.to_string(), "bob.alias@example.com".to_string()]
    }

    fn recorded(keys: &[(i64, bool)]) -> ZaInstances {
        ZaInstances {
            master: false,
            keys: keys.to_vec(),
            all: false,
        }
    }

    fn master_only() -> ZaInstances {
        ZaInstances {
            master: true,
            keys: vec![],
            all: false,
        }
    }

    /// The instance keys of a REPLY's components, in message order.
    fn reply_instances(reply: &ItipMessage<ICalendar>) -> Vec<InstanceKey> {
        let snapshots = itip_snapshot(&reply.message, &bob(), false).expect("a REPLY snapshot");
        let mut instances: Vec<(u16, InstanceKey)> = snapshots
            .components
            .iter()
            .map(|(id, snapshot)| (snapshot.comp_id, instance_key(id)))
            .collect();
        instances.sort();
        instances.into_iter().map(|(_, key)| key).collect()
    }

    /// The PARTSTAT of `address` in the REPLY's component for `instance`.
    fn reply_partstat(
        reply: &ItipMessage<ICalendar>,
        instance: InstanceKey,
        address: &str,
    ) -> Option<String> {
        let snapshots = itip_snapshot(&reply.message, &bob(), false).expect("a REPLY snapshot");
        snapshots
            .components
            .iter()
            .find(|(id, _)| instance_key(id) == instance)
            .and_then(|(_, snapshot)| snapshot.attendee_by_email(address))
            .and_then(|attendee| attendee.part_stat)
            .map(|part_stat| part_stat.as_str().to_string())
    }

    #[test]
    fn the_participation_reply_exports_only_recorded_qualifying_instances() {
        let copy = parse(COPY);
        let reply = za_participation_reply(&copy, &bob(), &recorded(&[A, C, D, E]), BOB)
            .expect("A qualifies");
        assert_eq!(reply_instances(&reply), [Some(A)]);
        assert_eq!(
            reply_partstat(&reply, Some(A), BOB).as_deref(),
            Some("TENTATIVE"),
            "RSVP is not consulted"
        );
        assert_eq!(
            (reply.from.as_str(), reply.to.clone()),
            (BOB, vec![ALICE.to_string()])
        );
        let every = ZaInstances {
            master: false,
            keys: vec![],
            all: true,
        };
        assert_eq!(
            reply_instances(&za_participation_reply(&copy, &bob(), &every, BOB).unwrap()),
            [None, Some(A), Some(B)]
        );
        assert!(za_participation_reply(&copy, &bob(), &recorded(&[C, D, E]), BOB).is_none());
    }

    #[test]
    fn a_reply_per_local_address() {
        let copy = parse(COPY);
        let every = ZaInstances {
            master: false,
            keys: vec![],
            all: true,
        };
        let alias = "bob.alias@example.com";
        assert_eq!(
            za_reply_addresses(&copy, &bob(), &every),
            [BOB, alias],
            "the master's address first, each once"
        );
        assert_eq!(
            za_reply_addresses(&copy, &bob(), &recorded(&[E, A])),
            [BOB, alias]
        );
        assert_eq!(za_reply_addresses(&copy, &bob(), &recorded(&[E])), [alias]);
        assert!(za_reply_addresses(&copy, &alice(), &every).is_empty());
        let reply = za_participation_reply(&copy, &bob(), &recorded(&[A, E]), alias)
            .expect("E qualifies under the alias");
        assert_eq!(reply_instances(&reply), [Some(E)]);
        assert_eq!(reply.from, alias);
    }

    #[test]
    fn replies_only_from_addresses_holding_a_token() {
        let copy = parse(COPY);
        let every = ZaInstances {
            master: false,
            keys: vec![],
            all: true,
        };
        let mut pairs = TokenPairs::new();
        pairs.insert(BOB, [7; crate::calendar::seal::TOKEN_LEN]);
        let replies = za_participation_replies(&copy, &bob(), &every, Some(&pairs));
        assert_eq!(
            replies
                .iter()
                .map(|reply| reply.from.as_str())
                .collect::<Vec<_>>(),
            [BOB],
            "the alias holds no token"
        );
        pairs.insert(
            "bob.alias@example.com",
            [8; crate::calendar::seal::TOKEN_LEN],
        );
        assert_eq!(
            za_participation_replies(&copy, &bob(), &every, Some(&pairs)).len(),
            2
        );
        assert!(za_participation_replies(&copy, &bob(), &every, None).is_empty());
    }

    #[test]
    fn the_participation_reply_carries_the_delegation_lines() {
        let delegated = parse(DELEGATED);
        let text = za_participation_reply(&delegated, &bob(), &master_only(), BOB)
            .unwrap()
            .message
            .to_string();
        assert!(
            text.contains("DELEGATED-TO") && text.contains("DELEGATED-FROM"),
            "{text}"
        );
    }

    #[test]
    fn the_fork_decline_ignores_partstat() {
        // bob's copy with bob DECLINED and no SCHEDULE-FORCE-SEND
        let declined = copy_with("PARTSTAT=DECLINED");
        let needs_action = copy_with("PARTSTAT=NEEDS-ACTION;RSVP=TRUE");
        let rsvp_false = copy_with("PARTSTAT=DECLINED;RSVP=FALSE");
        let client_scheduled = parse(CLIENT_SCHEDULED);
        assert!(
            itip_cancel(&declined, &bob(), true, policy()).is_err(),
            "upstream builds nothing"
        );
        let message = za_decline_all(&declined, &bob()).expect("the fork declines");
        assert_eq!(
            (message.from.as_str(), message.from_organizer),
            (BOB, false)
        );
        assert!(message.to.contains(&ALICE.to_string()));
        assert_eq!(
            reply_partstat(&message, None, BOB).as_deref(),
            Some("DECLINED")
        );
        // A deletion skips NEEDS-ACTION upstream (`skip_needs_action`); the
        // fork's gate has no PARTSTAT match at all.
        assert!(
            itip_cancel(&needs_action, &bob(), true, policy()).is_err(),
            "upstream builds nothing at NEEDS-ACTION"
        );
        let message = za_decline_all(&needs_action, &bob()).expect("the fork declines");
        assert_eq!(
            reply_partstat(&message, None, BOB).as_deref(),
            Some("DECLINED")
        );
        for silent in [&rsvp_false, &client_scheduled] {
            assert!(
                za_decline_all(silent, &bob()).is_none(),
                "RSVP and SCHEDULE-AGENT still gate"
            );
        }
    }

    #[test]
    fn the_full_request_names_only_its_recipient() {
        let organized = parse(ORGANIZED);
        let messages = za_full_request_for(&organized, &alice(), BOB, policy()).unwrap();
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(
            (message.to.clone(), message.from_organizer),
            (vec![BOB.to_string()], true)
        );
        for silent in ["declined@example.com", "rsvp-false@example.com"] {
            assert!(
                za_full_request_for(&organized, &alice(), silent, policy())
                    .unwrap()
                    .is_empty(),
                "{silent}"
            );
        }
    }

    /// alice's daily event (5 days from 2099-03-01): carol on the master,
    /// override X (day 2, SEQUENCE 3) naming `x_attendees`, override Z
    /// (day 4) naming bob.
    fn removal_event(master: &[&str], x_attendees: &[&str]) -> ICalendar {
        let attendees = |list: &[&str]| {
            list.iter()
                .map(|address| {
                    format!("ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{address}\r\n")
                })
                .collect::<String>()
        };
        parse(&format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
                "BEGIN:VEVENT\r\nUID:pin-unit-r\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
                "SEQUENCE:1\r\nSUMMARY:master\r\nORGANIZER:mailto:key1@example.com\r\n{}",
                "END:VEVENT\r\n",
                "BEGIN:VEVENT\r\nUID:pin-unit-r\r\nDTSTAMP:20240101T000000Z\r\n",
                "RECURRENCE-ID:20990302T100000Z\r\nDTSTART:20990302T100000Z\r\n",
                "DURATION:PT1H\r\nSEQUENCE:3\r\nSUMMARY:x\r\n",
                "ORGANIZER:mailto:key1@example.com\r\n{}END:VEVENT\r\n",
                "BEGIN:VEVENT\r\nUID:pin-unit-r\r\nDTSTAMP:20240101T000000Z\r\n",
                "RECURRENCE-ID:20990304T100000Z\r\nDTSTART:20990304T100000Z\r\n",
                "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:z\r\n",
                "ORGANIZER:mailto:key1@example.com\r\n{}END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            attendees(master),
            attendees(x_attendees),
            attendees(&[BOB]),
        ))
    }

    /// (instance key, SEQUENCE) of each scheduling component, in order.
    fn components_of(message: &ItipMessage<ICalendar>) -> Vec<(InstanceKey, i64)> {
        let snapshots = itip_snapshot(&message.message, &alice(), false).expect("a snapshot");
        let mut found: Vec<(u16, InstanceKey, i64)> = snapshots
            .components
            .iter()
            .map(|(id, snapshot)| {
                (
                    snapshot.comp_id,
                    instance_key(id),
                    snapshot.sequence.unwrap_or_default(),
                )
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, key, seq)| (key, seq)).collect()
    }

    /// Upgrade ruling 132: the recorded removals bob no longer attends, X
    /// (an override without him, at its committed SEQUENCE) and the
    /// master-generated day 5 (at the master's), in one CANCEL that names
    /// bob alone on each, not X's remaining attendee; not Z, which he
    /// attends; nothing under `all`, for an empty `to`, or for a master
    /// attendee's master-generated instance.
    #[test]
    fn withheld_cancels_carry_only_the_removals() {
        // A is X (day 2), C is Z (day 4), D the master-generated day 5.
        let to = vec![BOB.to_string()];
        let tree = removal_event(&["key3@example.com"], &["key3@example.com"]);
        let recorded_xzw = recorded(&[D, C, A]);
        let cancels = za_withheld_cancels(&tree, &alice(), BOB, &to, &recorded_xzw, policy())
            .expect("no split error");
        assert_eq!(cancels.len(), 1);
        let cancel = &cancels[0];
        assert_eq!(
            (
                cancel.from.as_str(),
                cancel.from_organizer,
                cancel.to.clone()
            ),
            (ALICE, true, to.clone())
        );
        assert_eq!(
            itip_method(&cancel.message).ok(),
            Some(&ICalendarMethod::Cancel)
        );
        assert_eq!(components_of(cancel), [(Some(A), 3), (Some(D), 1)]);
        let text = cancel.message.to_string();
        assert!(
            !text.contains("key3@example.com")
                && text.matches(&format!("ATTENDEE:mailto:{BOB}")).count() == 2
                && text.matches("ATTENDEE").count() == 2
                && text.matches("STATUS:CANCELLED").count() == 2,
            "{text}"
        );
        // bob's own snapshot of it, as his drain takes it, holds both.
        let bob_view = itip_snapshot(&cancel.message, &bob(), false).expect("bob is named");
        assert_eq!(bob_view.components.len(), 2);

        assert!(
            za_withheld_cancels(&tree, &alice(), BOB, &[], &recorded_xzw, policy())
                .unwrap()
                .is_empty()
        );
        let every = ZaInstances {
            master: false,
            keys: vec![],
            all: true,
        };
        assert!(
            za_withheld_cancels(&tree, &alice(), BOB, &to, &every, policy())
                .unwrap()
                .is_empty(),
            "an overflowed entry keeps today's behaviour"
        );
        // bob on X again, and on the master (so day 5 is his).
        let tree = removal_event(&[BOB], &[BOB]);
        assert!(
            za_withheld_cancels(&tree, &alice(), BOB, &to, &recorded_xzw, policy())
                .unwrap()
                .is_empty()
        );
    }

    /// Upgrade ruling 152: canary content on the master and on X, written
    /// after bob left X (a new title, description, location, creation and
    /// modification time, and a moved time), reaches neither component of the
    /// replayed CANCEL: X (the present-component arm) nor the master-generated
    /// day 5 (the master arm). Each keeps UID, ORGANIZER, RECURRENCE-ID,
    /// SEQUENCE, DTSTAMP, STATUS:CANCELLED, bob's own ATTENDEE line and a
    /// DTSTART at the RECURRENCE-ID's value; the iTIP summary holds the two
    /// RECURRENCE-ID times alone.
    #[test]
    fn withheld_cancels_carry_no_content() {
        let canaries = [
            "canary-master-summary",
            "canary-master-description",
            "canary-master-location",
            "canary-x-summary",
            "canary-x-description",
            "canary-x-location",
            "20990302T150000Z",
            "20990302T170000Z",
            "PT1H",
            "20240202T000000Z",
            "20240303T000000Z",
        ];
        let text = removal_event(&["key3@example.com"], &["key3@example.com"])
            .to_string()
            .replace(
                "SUMMARY:master\r\n",
                concat!(
                    "SUMMARY:canary-master-summary\r\n",
                    "DESCRIPTION:canary-master-description\r\n",
                    "LOCATION:canary-master-location\r\n",
                    "CREATED:20240202T000000Z\r\nLAST-MODIFIED:20240303T000000Z\r\n",
                ),
            )
            .replace(
                "DTSTART:20990302T100000Z\r\nDURATION:PT1H\r\n",
                "DTSTART:20990302T150000Z\r\nDTEND:20990302T170000Z\r\n",
            )
            .replace(
                "SUMMARY:x\r\n",
                concat!(
                    "SUMMARY:canary-x-summary\r\n",
                    "DESCRIPTION:canary-x-description\r\n",
                    "LOCATION:canary-x-location\r\n",
                    "CREATED:20240202T000000Z\r\nLAST-MODIFIED:20240303T000000Z\r\n",
                ),
            );
        for canary in &canaries[..7] {
            assert!(text.contains(canary), "the fixture plants {canary}: {text}");
        }
        let tree = parse(&text);
        let to = vec![BOB.to_string()];
        let cancels =
            za_withheld_cancels(&tree, &alice(), BOB, &to, &recorded(&[D, C, A]), policy())
                .expect("no split error");
        assert_eq!(cancels.len(), 1);
        let cancel = &cancels[0];
        assert_eq!(components_of(cancel), [(Some(A), 3), (Some(D), 1)]);
        let text = cancel.message.to_string();
        for canary in canaries {
            assert!(
                !text.contains(canary),
                "{canary} reached the CANCEL: {text}"
            );
        }
        let summary = serde_json::to_string(&cancel.summary).unwrap();
        for canary in canaries {
            assert!(
                !summary.contains(canary),
                "{canary} reached the summary: {summary}"
            );
        }
        let scheduling: Vec<&ICalendarComponent> = cancel
            .message
            .components
            .iter()
            .filter(|comp| comp.component_type.is_scheduling_object())
            .collect();
        assert_eq!(scheduling.len(), 2);
        for (comp, (rid, date)) in scheduling
            .into_iter()
            .zip([("20990302T100000Z", A.0), ("20990305T100000Z", D.0)])
        {
            let mut names: Vec<&str> = comp
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            names.sort_unstable();
            assert_eq!(
                names,
                [
                    "ATTENDEE",
                    "DTSTAMP",
                    "DTSTART",
                    "ORGANIZER",
                    "RECURRENCE-ID",
                    "SEQUENCE",
                    "STATUS",
                    "UID"
                ],
                "{text}"
            );
            assert_eq!(comp.status(), Some(&ICalendarStatus::Cancelled), "{text}");
            let value_of = |name: ICalendarProperty| {
                let entry = comp.property(&name).expect("the property");
                let mut value = String::new();
                entry
                    .values
                    .first()
                    .and_then(|value| value.as_partial_date_time())
                    .unwrap()
                    .format_as_ical(
                        &mut value,
                        &calcard::icalendar::ICalendarValueType::DateTime,
                    )
                    .unwrap();
                value
            };
            assert_eq!(value_of(ICalendarProperty::RecurrenceId), rid, "{text}");
            assert_eq!(value_of(ICalendarProperty::Dtstart), rid, "{text}");
            assert_eq!(
                comp.property(&ICalendarProperty::Attendee)
                    .and_then(|entry| entry.values.first())
                    .and_then(|value| value.as_text()),
                Some(format!("mailto:{BOB}").as_str()),
                "{text}"
            );
            assert!(
                matches!(&cancel.summary, ItipSummary::Cancel(fields)
                    if fields.iter().any(|field| matches!(&field.value,
                        ItipValue::Time(time) if time.start == date))),
                "{summary}"
            );
        }
        assert!(
            matches!(&cancel.summary, ItipSummary::Cancel(fields)
                if fields.len() == 2
                    && fields.iter().all(|field| field.name == ICalendarProperty::Dtstart)),
            "{summary}"
        );
    }

    /// Upgrade ruling 153: bob's slice is empty (he DECLINED Z, his only
    /// instance, without SCHEDULE-FORCE-SEND, or has RSVP=FALSE there), yet
    /// the recorded removals are replayed to him, a CANCEL alone; where the
    /// tree marks him SCHEDULE-AGENT=CLIENT or NONE nothing is sent.
    #[test]
    fn a_recorded_removal_is_replayed_without_a_slice() {
        const ON_Z: &str = "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com";
        let base = removal_event(&["key3@example.com"], &["key3@example.com"]).to_string();
        assert_eq!(base.matches(ON_Z).count(), 1, "{base}");
        let recorded_xw = recorded(&[D, A]);
        for line in [
            "ATTENDEE;PARTSTAT=DECLINED:mailto:key2@example.com",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=FALSE:mailto:key2@example.com",
        ] {
            let tree = parse(&base.replace(ON_Z, line));
            assert!(
                za_full_request_for(&tree, &alice(), BOB, policy())
                    .unwrap()
                    .is_empty(),
                "{line}: the slice is empty"
            );
            let messages = za_full_state_for(&tree, &alice(), BOB, &recorded_xw, policy())
                .expect("no split error");
            assert_eq!(messages.len(), 1, "{line}");
            assert_eq!(
                (
                    itip_method(&messages[0].message).ok(),
                    messages[0].to.clone()
                ),
                (Some(&ICalendarMethod::Cancel), vec![BOB.to_string()]),
                "{line}"
            );
            assert_eq!(
                components_of(&messages[0]),
                [(Some(A), 3), (Some(D), 1)],
                "{line}"
            );
        }
        for agent in ["CLIENT", "NONE"] {
            let line = format!(
                "ATTENDEE;SCHEDULE-AGENT={agent};PARTSTAT=DECLINED:mailto:key2@example.com"
            );
            let tree = parse(&base.replace(ON_Z, &line));
            assert!(
                za_full_state_for(&tree, &alice(), BOB, &recorded_xw, policy())
                    .unwrap()
                    .is_empty(),
                "{agent}: the server does not schedule for him"
            );
        }
    }

    /// Upgrade ruling 135 (M10): the split's error is returned, not read as
    /// nothing to send (`itip_outbound_max_recipients` at most 1).
    #[test]
    fn a_split_error_is_not_nothing_to_send() {
        let one = RecipientPolicy {
            max_recipients: 1,
            ..policy()
        };
        let organized = parse(ORGANIZED);
        assert!(matches!(
            za_full_request_for(&organized, &alice(), BOB, one),
            Err(ItipError::TooManyRecipients)
        ));
        assert!(matches!(
            za_full_state_for(&organized, &alice(), BOB, &ZaInstances::default(), one),
            Err(ItipError::TooManyRecipients)
        ));
        let tree = removal_event(&["key3@example.com"], &["key3@example.com"]);
        assert!(matches!(
            za_withheld_cancels(
                &tree,
                &alice(),
                BOB,
                &[BOB.to_string()],
                &recorded(&[A]),
                one
            ),
            Err(ItipError::TooManyRecipients)
        ));
    }
}
