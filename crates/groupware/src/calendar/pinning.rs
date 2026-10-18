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
    calendar::drain::instance_key,
    scheduling::{
        Email, InstanceId, ItipMessage, ItipSnapshot, ItipSnapshots, ItipSummary,
        itip::{ItipExportAs, itip_add_tz, itip_build_envelope, itip_export_component},
        organizer::organizer_request_full,
        snapshot::itip_snapshot,
    },
};
use ahash::AHashSet;
use calcard::{
    common::PartialDateTime,
    icalendar::{
        ICalendar, ICalendarComponent, ICalendarMethod, ICalendarParameter,
        ICalendarParticipationStatus, ICalendarProperty, ICalendarStatus, ICalendarValue,
    },
};
use common::auth::vault::za_normalize_address;
use vault::pins::ZaInstances;

/// The organizer's full REQUEST for one attendee (spec 6.4 step 2, the path
/// the drain's delegating merge uses for a new delegate):
/// `organizer_request_full` over the stored tree with no SEQUENCE bump
/// (no `itip_finalize`), its recipients restricted to `recipient`, compared
/// normalized (spec 4.3). `None` when the builder errs, the tree is not
/// organized by one of `addresses`, or the REQUEST does not reach the
/// recipient (DECLINED without SCHEDULE-FORCE-SEND, RSVP=FALSE,
/// SCHEDULE-AGENT=CLIENT or NONE: upstream's `send_update_messages`).
pub fn za_full_request_for(
    tree: &ICalendar,
    addresses: &[String],
    recipient: &str,
) -> Option<ItipMessage<ICalendar>> {
    let recipient = za_normalize_address(recipient)?;
    let snapshots = itip_snapshot(tree, addresses, false).ok()?;
    // Only the organizer's own copy builds a REQUEST.
    if !snapshots.organizer.email.is_local {
        return None;
    }
    organizer_request_full(tree, &snapshots, None, false)
        .ok()?
        .into_iter()
        .find_map(|mut message| {
            message
                .to
                .retain(|to| za_normalize_address(to).as_deref() == Some(recipient.as_str()));
            (!message.to.is_empty()).then_some(message)
        })
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

/// The address `za_participation_reply` replies from: the local attendee
/// of the first recorded instance present in `tree`, the master first.
pub fn za_reply_from(
    tree: &ICalendar,
    addresses: &[String],
    instances: &ZaInstances,
) -> Option<String> {
    let snapshots = itip_snapshot(tree, addresses, false).ok()?;
    recorded_instances(&snapshots, instances)
        .into_iter()
        .find_map(|(_, instance)| {
            instance
                .local_attendee()
                .map(|attendee| attendee.email.email.clone())
        })
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
        scheduling::{event_cancel::itip_cancel, snapshot::itip_snapshot},
    };
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
            itip_cancel(&declined, &bob(), true).is_err(),
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
            itip_cancel(&needs_action, &bob(), true).is_err(),
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
        let message = za_full_request_for(&organized, &alice(), BOB).unwrap();
        assert_eq!(
            (message.to.clone(), message.from_organizer),
            (vec![BOB.to_string()], true)
        );
        for silent in ["declined@example.com", "rsvp-false@example.com"] {
            assert!(
                za_full_request_for(&organized, &alice(), silent).is_none(),
                "{silent}"
            );
        }
    }
}
