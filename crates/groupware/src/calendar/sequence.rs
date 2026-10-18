/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key pinning spec 5.1: the SEQUENCE floor and the fan-out's
//! reconciliation, key organizers only (invariant 9); upstream's builders
//! run unchanged (invariant 15).
//!
//! The floor: on every PUT of an existing event a key account organizes,
//! before upstream's scheduling step, each scheduling component's SEQUENCE
//! is raised to the stored component's value when lower or absent, so a
//! client that never refetched the reconciled event cannot write its own
//! lower values back. SEQUENCE never decreases and no component is added.
//!
//! The reconciliation: upstream's partial branch of
//! `organizer_handle_update` loses a partial's increments to a local that
//! shadows the caller's, and its CANCELs are built from the old tree at
//! old+1. After the builders return and before any delivery, every
//! instance the organizer's REQUESTs and CANCELs carry (the full REQUEST
//! included) is set, in the committed tree and in every such message, to
//! the highest SEQUENCE among them; an instance the committed tree lacks
//! (a removed override) is set in the messages only.
//!
//! Instances are matched by the drain's `InstanceKey`, each tree's
//! RECURRENCE-ID resolved with its own VTIMEZONEs as `itip_snapshot`
//! resolves it, never by text or component id.

use crate::{
    calendar::drain::{InstanceKey, instance_key},
    scheduling::{
        Email, InstanceId, ItipMessage, RecurrenceId, inbound::itip_method, snapshot::itip_snapshot,
    },
};
use ahash::AHashMap;
use calcard::icalendar::{
    ICalendar, ICalendarComponent, ICalendarMethod, ICalendarParameterName,
    ICalendarParameterValue, ICalendarProperty, ICalendarValue,
};

/// One instance's SEQUENCE as the reconciliation set it in the committed
/// tree, for `za_set_sequences` on the sender's plaintext tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZaSequence {
    pub instance: InstanceKey,
    pub sequence: i64,
}

/// Every scheduling component's id and its `InstanceKey`: `None` for the
/// master, else the RECURRENCE-ID resolved to a UTC instant with the tree's
/// own VTIMEZONEs and its RANGE flag, exactly as `itip_snapshot` computes
/// `RecurrenceId::date` (no ORGANIZER needed).
pub fn za_instance_keys(ical: &ICalendar) -> Vec<(u16, InstanceKey)> {
    let mut tz_resolver = None;
    let mut keys = Vec::new();
    for (comp_id, comp) in ical.components.iter().enumerate() {
        if !comp.component_type.is_scheduling_object() {
            continue;
        }
        let mut instance_id = InstanceId::Main;
        for (entry_id, entry) in comp.entries.iter().enumerate() {
            if entry.name != ICalendarProperty::RecurrenceId {
                continue;
            }
            let Some(date) = entry.values.first().and_then(|v| v.as_partial_date_time()) else {
                continue;
            };
            let mut this_and_future = false;
            let mut tz_id = None;
            for param in &entry.params {
                match (&param.name, &param.value) {
                    (ICalendarParameterName::Tzid, ICalendarParameterValue::Text(id)) => {
                        tz_id = Some(id.as_str());
                    }
                    (ICalendarParameterName::Range, _) => {
                        this_and_future = true;
                    }
                    _ => (),
                }
            }
            instance_id = InstanceId::Recurrence(RecurrenceId {
                entry_id: entry_id as u16,
                date: date
                    .to_date_time_with_tz(
                        tz_resolver
                            .get_or_insert_with(|| ical.build_tz_resolver())
                            .resolve_or_default(tz_id),
                    )
                    .map(|dt| dt.timestamp())
                    .unwrap_or_else(|| date.to_timestamp().unwrap_or_default()),
                this_and_future,
            });
        }
        keys.push((comp_id as u16, instance_key(&instance_id)));
    }
    keys
}

/// The floor (spec 5.1): only when the stored tree's ORGANIZER is one of
/// `addresses`, each instance of `new` also in `stored` gets the stored
/// SEQUENCE when its own is lower or absent; a higher one stays, and no
/// component is added or removed. `true` when anything changed.
pub fn za_sequence_floor(new: &mut ICalendar, stored: &ICalendar, addresses: &[String]) -> bool {
    if !organizes(stored, addresses) {
        return false;
    }
    let mut floors: AHashMap<InstanceKey, i64> = AHashMap::new();
    for (comp_id, instance) in za_instance_keys(stored) {
        if let Some(sequence) = sequence_of(&stored.components[comp_id as usize]) {
            floors
                .entry(instance)
                .and_modify(|floor| *floor = (*floor).max(sequence))
                .or_insert(sequence);
        }
    }
    let mut changed = false;
    for (comp_id, instance) in za_instance_keys(new) {
        if let Some(floor) = floors.get(&instance) {
            changed |= raise_sequence(&mut new.components[comp_id as usize], *floor);
        }
    }
    changed
}

/// The reconciliation (spec 5.1): over every instance the organizer's
/// REQUESTs and CANCELs (`from_organizer`) carry, the full REQUEST
/// included, each matched by `InstanceKey` from the message's own tree,
/// sets the committed component and every such message's component to the
/// highest SEQUENCE among them; an instance `committed` lacks is set in the
/// messages only. REPLYs are untouched. Nothing when `committed` has no
/// snapshot with a local ORGANIZER (`itip_cancel`'s fallback, an
/// attendee's tree). Returns the committed values it set.
pub fn za_reconcile_sequences(
    committed: &mut ICalendar,
    messages: &mut [ItipMessage<ICalendar>],
    addresses: &[String],
) -> Vec<ZaSequence> {
    if !itip_snapshot(committed, addresses, false)
        .is_ok_and(|snapshots| snapshots.organizer.email.is_local)
    {
        return Vec::new();
    }
    let carriers: Vec<(usize, Vec<(u16, InstanceKey)>)> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.from_organizer
                && matches!(
                    itip_method(&message.message),
                    Ok(ICalendarMethod::Request | ICalendarMethod::Cancel)
                )
        })
        .map(|(at, message)| (at, za_instance_keys(&message.message)))
        .collect();
    let mut highest: AHashMap<InstanceKey, i64> = AHashMap::new();
    for (at, keys) in &carriers {
        let message = &messages[*at].message;
        for (comp_id, instance) in keys {
            let sequence = sequence_of(&message.components[*comp_id as usize]).unwrap_or_default();
            highest
                .entry(*instance)
                .and_modify(|highest| *highest = (*highest).max(sequence))
                .or_insert(sequence);
        }
    }
    let committed_keys = za_instance_keys(committed);
    for (comp_id, instance) in &committed_keys {
        if let Some(highest) = highest.get_mut(instance) {
            *highest = (*highest)
                .max(sequence_of(&committed.components[*comp_id as usize]).unwrap_or_default());
        }
    }
    for (at, keys) in &carriers {
        let message = &mut messages[*at].message;
        for (comp_id, instance) in keys {
            raise_sequence(
                &mut message.components[*comp_id as usize],
                highest[instance],
            );
        }
    }
    let mut set = Vec::new();
    for (comp_id, instance) in committed_keys {
        if let Some(sequence) = highest.get(&instance) {
            raise_sequence(&mut committed.components[comp_id as usize], *sequence);
            set.push(ZaSequence {
                instance,
                sequence: *sequence,
            });
        }
    }
    set
}

/// Applies the reconciliation's committed values to the sender's plaintext
/// tree before it is sealed, matched by `InstanceKey`; a SEQUENCE is only
/// raised, never lowered (a concurrent edit's higher value survives).
pub fn za_set_sequences(ical: &mut ICalendar, sequences: &[ZaSequence]) {
    if sequences.is_empty() {
        return;
    }
    for (comp_id, instance) in za_instance_keys(ical) {
        if let Some(sequence) = sequences.iter().find(|s| s.instance == instance) {
            raise_sequence(&mut ical.components[comp_id as usize], sequence.sequence);
        }
    }
}

/// Whether the tree's ORGANIZER (the first in a scheduling component) is
/// one of `addresses`.
fn organizes(ical: &ICalendar, addresses: &[String]) -> bool {
    ical.components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
        .find_map(|comp| {
            comp.entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::Organizer)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, addresses))
        })
        .is_some_and(|organizer| organizer.is_local)
}

/// The component's SEQUENCE as `itip_snapshot` reads it (the last integer
/// value); `None` when absent.
fn sequence_of(comp: &ICalendarComponent) -> Option<i64> {
    comp.entries
        .iter()
        .filter(|entry| entry.name == ICalendarProperty::Sequence)
        .filter_map(|entry| entry.values.first().and_then(|v| v.as_integer()))
        .last()
}

/// Raises the component's SEQUENCE to `sequence` when lower (an absent one
/// reads as 0, RFC 5545 3.8.7.4); never lowers it. `true` when it changed.
fn raise_sequence(comp: &mut ICalendarComponent, sequence: i64) -> bool {
    if sequence_of(comp).unwrap_or_default() >= sequence {
        return false;
    }
    let mut found = false;
    for entry in comp
        .entries
        .iter_mut()
        .filter(|entry| entry.name == ICalendarProperty::Sequence)
    {
        entry.values = vec![ICalendarValue::Integer(sequence)];
        found = true;
    }
    if !found {
        comp.add_sequence(sequence);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduling::ItipSummary;
    use calcard::{Entry, Parser};

    const MASTER: InstanceKey = None;
    /// 20990301T100000Z.
    const W: InstanceKey = Some((4_076_042_400, false));
    /// 20990302T100000Z.
    const Q: InstanceKey = Some((4_076_128_800, false));
    /// 20990303T100000Z, 11:00 in Etc/GMT-1.
    const X: InstanceKey = Some((4_076_215_200, false));
    /// 20990304T100000Z.
    const Y: InstanceKey = Some((4_076_301_600, false));
    /// 20990305T100000Z.
    const Z: InstanceKey = Some((4_076_388_000, false));
    /// 20990306T100000Z.
    const R: InstanceKey = Some((4_076_474_400, false));

    /// calcard resolves a VTIMEZONE by name, so the fixture uses a named
    /// fixed zone (UTC+1).
    const TIMEZONE: &str = concat!(
        "BEGIN:VTIMEZONE\r\nTZID:Etc/GMT-1\r\nBEGIN:STANDARD\r\n",
        "DTSTART:19700101T000000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\n",
        "END:STANDARD\r\nEND:VTIMEZONE\r\n"
    );
    const ALICE: &str = "key1@example.com";
    const BOB: &str = "key2@example.com";

    fn alice() -> Vec<String> {
        vec![ALICE.to_string()]
    }

    /// One VEVENT naming bob, organized by `organizer`: the master (daily)
    /// without `recurrence_id`, else an override with that RECURRENCE-ID
    /// value (and its parameters); no SEQUENCE when `sequence` is `None`.
    fn vevent(recurrence_id: Option<&str>, sequence: Option<i64>, organizer: &str) -> String {
        let instance = match recurrence_id {
            None => "DTSTART:20990301T100000Z\r\nRRULE:FREQ=DAILY;COUNT=7\r\n".to_string(),
            Some(id) => format!("RECURRENCE-ID{id}\r\nDTSTART{id}\r\n"),
        };
        let sequence = sequence
            .map(|sequence| format!("SEQUENCE:{sequence}\r\n"))
            .unwrap_or_default();
        format!(
            concat!(
                "BEGIN:VEVENT\r\nUID:seq-unit\r\nDTSTAMP:20240101T000000Z\r\n",
                "{instance}{sequence}ORGANIZER:mailto:{organizer}\r\n",
                "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:{bob}\r\nEND:VEVENT\r\n"
            ),
            instance = instance,
            sequence = sequence,
            organizer = organizer,
            bob = BOB,
        )
    }

    /// `vevent` organized by alice.
    fn by_alice(recurrence_id: Option<&str>, sequence: Option<i64>) -> String {
        vevent(recurrence_id, sequence, ALICE)
    }

    /// A VCALENDAR of `parts` in order, with `method` when given.
    fn calendar(method: Option<&str>, parts: &[String]) -> ICalendar {
        let method = method
            .map(|method| format!("METHOD:{method}\r\n"))
            .unwrap_or_default();
        let text = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n{method}{}END:VCALENDAR\r\n",
            parts.concat()
        );
        match Parser::new(&text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn message(from_organizer: bool, message: ICalendar) -> ItipMessage<ICalendar> {
        ItipMessage {
            from: if from_organizer { ALICE } else { BOB }.to_string(),
            from_organizer,
            to: vec![if from_organizer { BOB } else { ALICE }.to_string()],
            summary: ItipSummary::Invite(vec![]),
            message,
        }
    }

    /// The committed tree: the master at 3, X at 6 (stored 4, the client
    /// bumped by two), Z at 2, after the VTIMEZONE (so no message's
    /// component ids line up with it). The messages: [0] a partial REQUEST
    /// for Z at 3; [1] a CANCEL from the old tree, W (an override removed in
    /// the same PUT) then X in the TZID form at 5, its VTIMEZONE appended
    /// as `itip_add_tz` appends it; [2] a CANCEL for W at 2; [3] bob's
    /// REPLY for X at 0.
    fn fixture() -> (ICalendar, Vec<ItipMessage<ICalendar>>) {
        let committed = calendar(
            None,
            &[
                TIMEZONE.to_string(),
                by_alice(None, Some(3)),
                by_alice(Some(":20990303T100000Z"), Some(6)),
                by_alice(Some(":20990305T100000Z"), Some(2)),
            ],
        );
        let messages = vec![
            message(
                true,
                calendar(
                    Some("REQUEST"),
                    &[by_alice(Some(":20990305T100000Z"), Some(3))],
                ),
            ),
            message(
                true,
                calendar(
                    Some("CANCEL"),
                    &[
                        by_alice(Some(":20990301T100000Z"), Some(2)),
                        by_alice(Some(";TZID=Etc/GMT-1:20990303T110000"), Some(5)),
                        TIMEZONE.to_string(),
                    ],
                ),
            ),
            message(
                true,
                calendar(
                    Some("CANCEL"),
                    &[by_alice(Some(":20990301T100000Z"), Some(2))],
                ),
            ),
            message(
                false,
                calendar(
                    Some("REPLY"),
                    &[by_alice(Some(":20990303T100000Z"), Some(0))],
                ),
            ),
        ];
        (committed, messages)
    }

    fn component(ical: &ICalendar, key: InstanceKey) -> Option<&ICalendarComponent> {
        za_instance_keys(ical)
            .into_iter()
            .find(|(_, instance)| *instance == key)
            .map(|(comp_id, _)| &ical.components[comp_id as usize])
    }

    fn seq(ical: &ICalendar, key: InstanceKey) -> i64 {
        component(ical, key)
            .and_then(sequence_of)
            .unwrap_or_else(|| panic!("no SEQUENCE for {key:?}"))
    }

    #[test]
    fn the_reconciliation_takes_each_instances_maximum_with_the_committed_component() {
        let (mut committed, mut messages) = fixture();
        let before = committed.components.len();
        let mut set = za_reconcile_sequences(&mut committed, &mut messages, &alice());
        set.sort_by_key(|s| s.instance);
        assert_eq!(seq(&committed, X), 6, "the committed component counts");
        assert_eq!(
            seq(&messages[1].message, X),
            6,
            "matched by instant and instance, not text or component id"
        );
        assert_eq!((seq(&committed, Z), seq(&messages[0].message, Z)), (3, 3));
        assert_eq!(
            seq(&messages[2].message, W),
            2,
            "an instance the committed tree lacks"
        );
        assert_eq!(committed.components.len(), before, "no component is added");
        assert_eq!(seq(&messages[3].message, X), 0, "replies are untouched");
        assert_eq!(
            set,
            [
                ZaSequence {
                    instance: X,
                    sequence: 6
                },
                ZaSequence {
                    instance: Z,
                    sequence: 3
                }
            ]
        );
        let (mut applied, _) = fixture();
        za_set_sequences(&mut applied, &set);
        assert_eq!(applied, committed);
    }

    /// stored: master 4, X 9 (TZID form), Y 5, R 1, organized by
    /// `organizer`.
    fn stored_by(organizer: &str) -> ICalendar {
        calendar(
            None,
            &[
                TIMEZONE.to_string(),
                vevent(None, Some(4), organizer),
                vevent(Some(";TZID=Etc/GMT-1:20990303T110000"), Some(9), organizer),
                vevent(Some(":20990304T100000Z"), Some(5), organizer),
                vevent(Some(":20990306T100000Z"), Some(1), organizer),
            ],
        )
    }

    /// new: master 6, X 7 (UTC form), Y without SEQUENCE, a new override Q
    /// at 0, R removed.
    fn new_tree() -> ICalendar {
        calendar(
            None,
            &[
                by_alice(None, Some(6)),
                by_alice(Some(":20990303T100000Z"), Some(7)),
                by_alice(Some(":20990304T100000Z"), None),
                by_alice(Some(":20990302T100000Z"), Some(0)),
            ],
        )
    }

    #[test]
    fn the_floor_raises_lower_values_and_keeps_higher_ones() {
        let stored = stored_by(ALICE);
        let mut new = new_tree();
        let before = new.components.len();
        assert!(za_sequence_floor(&mut new, &stored, &alice()));
        assert_eq!(
            (seq(&new, MASTER), seq(&new, X), seq(&new, Y), seq(&new, Q)),
            (6, 9, 5, 0)
        );
        assert!(
            component(&new, R).is_none(),
            "a removed component is not re-added"
        );
        assert_eq!(new.components.len(), before);
    }

    #[test]
    fn the_floor_applies_only_to_a_local_organizer() {
        let stored_by_someone_else = stored_by(BOB);
        let mut new = new_tree();
        let untouched = new.clone();
        assert!(!za_sequence_floor(
            &mut new,
            &stored_by_someone_else,
            &alice()
        ));
        assert_eq!(new, untouched);
    }
}
