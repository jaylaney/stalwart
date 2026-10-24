/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::scheduling::{
    Email, ItipError, ItipMessage, ItipSnapshots,
    event_cancel::itip_cancel_from_snapshot,
    organizer::{organizer_handle_update, organizer_request_full},
    snapshot::itip_snapshot,
};
use ahash::{AHashMap, AHashSet};
use calcard::icalendar::{ICalendar, ICalendarProperty, ICalendarScheduleForceSendValue};
use common::auth::{AccountInfo, vault::za_normalize_address};

pub struct ZaSelfMessages {
    pub(crate) messages: Vec<ItipMessage<ICalendar>>,
    pub(crate) increments: Vec<u16>,
    pub(crate) finalize: bool,
}

impl ZaSelfMessages {
    pub fn messages(&self) -> &[ItipMessage<ICalendar>] {
        &self.messages
    }
    pub fn increments(&self) -> &[u16] {
        &self.increments
    }
    pub fn finalize(&self) -> bool {
        self.finalize
    }
}

pub enum ZaSelfOperation {
    Create,
    Update,
    Delete,
}

#[allow(dead_code)] // The request-local lookup outcomes are consumed by Task 9 fan-out.
pub(crate) enum ZaResolvedAccount {
    Account(u32),
    NotFound,
    Failed,
}

pub struct ZaDelegationResolution {
    pub(crate) writer_id: u32,
    pub(crate) owner_id: u32,
    #[allow(dead_code)] // Classified by Task 9's resolver for fan-out routing.
    pub(crate) listed: AHashSet<String>,
    pub(crate) recipients: AHashSet<String>,
    pub(crate) override_aliases: AHashSet<String>,
    #[allow(dead_code)] // Consumed by the Task 9 fan-out, not by the pure builders.
    pub(crate) accounts: AHashMap<String, ZaResolvedAccount>,
}

pub fn za_finalize(tree: &mut ICalendar, increments: &[u16]) {
    crate::scheduling::itip::itip_finalize(tree, increments);
}

/// The organizer gate is evaluated on the new tree for create, and on the
/// prior tree for update and delete. The owner-only address list is essential:
/// a shared owner/writer group is still owner-local here.
fn delegated_organizer(
    tree: &ICalendar,
    writer: &AccountInfo,
    owner: &AccountInfo,
    resolution: &ZaDelegationResolution,
) -> bool {
    writer.account_id != owner.account_id
        && resolution.writer_id == writer.account_id
        && resolution.owner_id == owner.account_id
        && tree.components.iter().any(|component| {
            component.component_type.is_scheduling_object()
                && component.entries.iter().any(|entry| {
                    entry.name == ICalendarProperty::Organizer
                        && entry
                            .values
                            .first()
                            .and_then(|value| value.as_text())
                            .and_then(|address| Email::new(address, &owner.addresses))
                            .is_some_and(|email| email.is_local)
                })
        })
}

static FORCE_REQUEST: ICalendarScheduleForceSendValue = ICalendarScheduleForceSendValue::Request;

fn owner_snapshot<'a>(
    tree: &'a ICalendar,
    owner: &AccountInfo,
    resolution: &ZaDelegationResolution,
) -> Result<ItipSnapshots<'a>, ItipError> {
    let mut snapshot = itip_snapshot(tree, &owner.addresses, false)?;
    for instance in snapshot.components.values_mut() {
        // `Attendee` is a hash-set key. Remove and rebuild rather than changing
        // one of its fields while it is inside the set.
        let attendees = std::mem::take(&mut instance.attendees);
        instance.attendees = attendees
            .into_iter()
            .map(|mut attendee| {
                if !attendee.email.is_local
                    && za_normalize_address(&attendee.email.email)
                        .is_some_and(|key| resolution.override_aliases.contains(&key))
                {
                    attendee.force_send = Some(&FORCE_REQUEST);
                    attendee.rsvp = None;
                }
                attendee
            })
            .collect();
    }
    Ok(snapshot)
}

fn retain_writer_messages(
    messages: Vec<ItipMessage<ICalendar>>,
    increments: Vec<u16>,
    finalize: bool,
    resolution: &ZaDelegationResolution,
) -> ZaSelfMessages {
    let messages = messages
        .into_iter()
        .filter_map(|mut message| {
            message.to.retain(|address| {
                za_normalize_address(address)
                    .is_some_and(|key| resolution.recipients.contains(&key))
            });
            (!message.to.is_empty()).then_some(message)
        })
        .collect();
    ZaSelfMessages {
        messages,
        increments,
        finalize,
    }
}

pub fn za_self_create(
    new: &ICalendar,
    writer: &AccountInfo,
    owner: &AccountInfo,
    resolution: &ZaDelegationResolution,
) -> Result<Option<ZaSelfMessages>, ItipError> {
    if !delegated_organizer(new, writer, owner, resolution) {
        return Ok(None);
    }
    let snapshot = owner_snapshot(new, owner, resolution)?;
    if !snapshot.organizer.is_server_scheduling {
        Err(ItipError::OtherSchedulingAgent)
    } else if !snapshot.organizer.email.is_local {
        Err(ItipError::NotOrganizer)
    } else {
        organizer_request_full(new, &snapshot, None, true)
            .map(|messages| Some(retain_writer_messages(messages, vec![], true, resolution)))
    }
}

pub fn za_self_update(
    old: &ICalendar,
    new: &ICalendar,
    writer: &AccountInfo,
    owner: &AccountInfo,
    resolution: &ZaDelegationResolution,
) -> Result<Option<ZaSelfMessages>, ItipError> {
    if !delegated_organizer(old, writer, owner, resolution) {
        return Ok(None);
    }
    let old_snapshot = owner_snapshot(old, owner, resolution)?;
    match owner_snapshot(new, owner, resolution) {
        Ok(new_snapshot) => {
            if old_snapshot.organizer.email != new_snapshot.organizer.email {
                return Err(ItipError::OrganizerMismatch);
            }
            let mut increments = Vec::new();
            organizer_handle_update(old, new, old_snapshot, new_snapshot, &mut increments).map(
                |messages| {
                    Some(retain_writer_messages(
                        messages, increments, true, resolution,
                    ))
                },
            )
        }
        Err(err) => match err {
            ItipError::NoSchedulingInfo
            | ItipError::NotOrganizer
            | ItipError::NotOrganizerNorAttendee
            | ItipError::OtherSchedulingAgent => {
                itip_cancel_from_snapshot(old, old_snapshot, false).map(|message| {
                    Some(retain_writer_messages(
                        vec![message],
                        vec![],
                        false,
                        resolution,
                    ))
                })
            }
            _ => Err(err),
        },
    }
}

pub fn za_self_cancel(
    old: &ICalendar,
    writer: &AccountInfo,
    owner: &AccountInfo,
    resolution: &ZaDelegationResolution,
) -> Result<Option<ZaSelfMessages>, ItipError> {
    if !delegated_organizer(old, writer, owner, resolution) {
        return Ok(None);
    }
    let snapshot = owner_snapshot(old, owner, resolution)?;
    itip_cancel_from_snapshot(old, snapshot, true).map(|message| {
        Some(retain_writer_messages(
            vec![message],
            vec![],
            false,
            resolution,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduling::{
        event_cancel::{itip_cancel, itip_cancel_from_snapshot},
        event_create::itip_create,
        event_update::itip_update,
        snapshot::itip_snapshot,
    };
    use calcard::icalendar::{
        ICalendarParameterName, ICalendarParameterValue, ICalendarParticipationStatus,
        ICalendarProperty, ICalendarValue,
    };
    use calcard::{Entry, Parser};
    use common::auth::AccountCache;
    use std::sync::Arc;

    const OWNER: &str = "owner@example.com";
    const WRITER: &str = "writer@example.com";
    const WRITER_ALIAS: &str = "writer.alias@example.com";
    const SHARED: &str = "shared@example.com";
    const GROUP: &str = "writers@example.com";
    const EXTERNAL: &str = "external@example.com";

    fn account(id: u32, addresses: &[&str]) -> AccountInfo {
        AccountInfo {
            account_id: id,
            account: Arc::new(AccountCache::default()),
            addresses: addresses.iter().map(|s| (*s).into()).collect(),
        }
    }

    fn resolution(
        writer: &AccountInfo,
        owner: &AccountInfo,
        listed: &[&str],
        recipients: &[&str],
        aliases: &[&str],
    ) -> ZaDelegationResolution {
        let normalized = |items: &[&str]| items.iter().map(|s| (*s).to_string()).collect();
        ZaDelegationResolution {
            writer_id: writer.account_id,
            owner_id: owner.account_id,
            listed: normalized(listed),
            recipients: normalized(recipients),
            override_aliases: normalized(aliases),
            accounts: AHashMap::new(),
        }
    }

    fn event(
        organizer: &str,
        attendees: &[(&str, &str)],
        summary: &str,
        start: &str,
        extra: &str,
    ) -> ICalendar {
        let mut text = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:delegation-unit\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{start}\r\nDURATION:PT1H\r\nSEQUENCE:2\r\nSUMMARY:{summary}\r\nORGANIZER:mailto:{organizer}\r\n"
        );
        for (address, params) in attendees {
            text.push_str(&format!("ATTENDEE{params}:mailto:{address}\r\n"));
        }
        text.push_str(extra);
        text.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
        match Parser::new(&text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn canonical_without_dtstamp(mut tree: ICalendar) -> ICalendar {
        for comp in &mut tree.components {
            comp.entries
                .retain(|entry| entry.name != ICalendarProperty::Dtstamp);
        }
        tree
    }

    fn sequence(tree: &ICalendar) -> i64 {
        tree.components
            .iter()
            .find(|c| c.component_type.is_scheduling_object())
            .unwrap()
            .entries
            .iter()
            .find(|e| e.name == ICalendarProperty::Sequence)
            .unwrap()
            .values
            .first()
            .and_then(ICalendarValue::as_integer)
            .unwrap()
    }

    #[test]
    fn second_run_uses_owner_locality() {
        let owner = account(1, &[OWNER, SHARED]);
        let writer = account(2, &[WRITER, SHARED, GROUP]);
        let targets = [
            (WRITER, ""),
            (GROUP, ""),
            ("writer+tag@example.com", ""),
            ("custom@example.com", ""),
            ("catch@example.com", ""),
            (EXTERNAL, ""),
        ];
        let tree = event(OWNER, &targets, "original", "20990301T100000Z", "");
        let res = resolution(
            &writer,
            &owner,
            &[WRITER, GROUP],
            &[
                WRITER,
                GROUP,
                "writer+tag@example.com",
                "custom@example.com",
                "catch@example.com",
            ],
            &[WRITER],
        );
        let actual = za_self_create(&tree, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(actual.finalize());
        assert!(actual.increments().is_empty());
        assert_eq!(actual.messages().len(), 1);
        let mut to = actual.messages()[0].to.clone();
        to.sort();
        assert_eq!(
            to,
            vec![
                "catch@example.com",
                "custom@example.com",
                "writer+tag@example.com",
                WRITER,
                GROUP
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        let mut reference = tree.clone();
        let expected = itip_create(&mut reference, &owner.addresses).unwrap();
        assert_eq!(
            canonical_without_dtstamp(actual.messages()[0].message.clone()),
            canonical_without_dtstamp(expected[0].message.clone())
        );
        assert_eq!(
            tree,
            event(OWNER, &targets, "original", "20990301T100000Z", "")
        );
        for organizer in [WRITER, EXTERNAL] {
            let other = event(organizer, &[(WRITER, "")], "x", "20990301T100000Z", "");
            assert!(
                za_self_create(&other, &writer, &owner, &res)
                    .unwrap()
                    .is_none()
            );
            assert!(
                za_self_update(&other, &other, &writer, &owner, &res)
                    .unwrap()
                    .is_none()
            );
            assert!(
                za_self_cancel(&other, &writer, &owner, &res)
                    .unwrap()
                    .is_none()
            );
        }
        let attendee_side = event(
            EXTERNAL,
            &[(OWNER, ""), (WRITER, "")],
            "x",
            "20990301T100000Z",
            "",
        );
        assert!(
            za_self_update(&attendee_side, &attendee_side, &writer, &owner, &res)
                .unwrap()
                .is_none()
        );
        let shared = event(SHARED, &[(WRITER, "")], "x", "20990301T100000Z", "");
        assert!(
            za_self_create(&shared, &writer, &owner, &res)
                .unwrap()
                .is_some()
        );
        let shared_only = event(SHARED, &[(SHARED, "")], "x", "20990301T100000Z", "");
        assert!(matches!(
            za_self_create(&shared_only, &writer, &owner, &res),
            Err(ItipError::NothingToSend)
        ));
        assert!(
            za_self_create(&tree, &owner, &owner, &res)
                .unwrap()
                .is_none()
        );

        // A writer-only membership group is local in the merged first run,
        // but an ordinary external attendee in the owner-only second run.
        let group_only = event(OWNER, &[(GROUP, "")], "x", "20990301T100000Z", "");
        let group_res = resolution(&writer, &owner, &[GROUP], &[GROUP], &[]);
        let mut first_run = group_only.clone();
        assert!(matches!(
            itip_create(
                &mut first_run,
                &[WRITER.into(), SHARED.into(), GROUP.into(), OWNER.into()]
            ),
            Err(ItipError::NothingToSend)
        ));
        let group_second = za_self_create(&group_only, &writer, &owner, &group_res)
            .unwrap()
            .unwrap();
        assert_eq!(group_second.messages()[0].to, vec![GROUP.to_string()]);
        assert!(group_second.finalize());

        // An upstream builder's Ok is still built when R filtering is empty.
        let external_only = event(OWNER, &[(EXTERNAL, "")], "x", "20990301T100000Z", "");
        let empty_res = resolution(&writer, &owner, &[], &[], &[]);
        let built = za_self_create(&external_only, &writer, &owner, &empty_res)
            .unwrap()
            .unwrap();
        assert!(built.messages().is_empty());
        assert!(built.finalize());
    }

    #[test]
    fn snapshot_exception_leaves_scheduling_agent_and_payload_untouched() {
        let owner = account(1, &[OWNER]);
        let writer = account(2, &[WRITER, WRITER_ALIAS, GROUP]);
        let tree = event(
            OWNER,
            &[
                (WRITER, ";PARTSTAT=DECLINED;RSVP=FALSE"),
                (WRITER_ALIAS, ";PARTSTAT=DECLINED;RSVP=FALSE"),
                (GROUP, ";PARTSTAT=DECLINED;RSVP=FALSE"),
                ("custom@example.com", ";PARTSTAT=DECLINED;RSVP=FALSE"),
                (EXTERNAL, ";PARTSTAT=NEEDS-ACTION"),
            ],
            "x",
            "20990301T100000Z",
            "",
        );
        let original = tree.clone();
        let mut res = resolution(
            &writer,
            &owner,
            &[WRITER, WRITER_ALIAS, GROUP],
            &[WRITER, WRITER_ALIAS, GROUP, "custom@example.com"],
            &[WRITER, WRITER_ALIAS],
        );
        res.accounts
            .insert(WRITER_ALIAS.into(), ZaResolvedAccount::Failed);
        let actual = za_self_create(&tree, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert_eq!(actual.messages().len(), 1);
        let mut recipients = actual.messages()[0].to.clone();
        recipients.sort();
        assert_eq!(
            recipients,
            vec![WRITER_ALIAS.to_string(), WRITER.to_string()]
        );
        assert_eq!(tree, original);
        let payload = &actual.messages()[0].message;
        let payload_line = payload
            .components
            .iter()
            .flat_map(|c| &c.entries)
            .find(|e| {
                e.name == ICalendarProperty::Attendee
                    && e.values
                        .first()
                        .and_then(ICalendarValue::as_text)
                        .is_some_and(|v| v.contains(WRITER))
            })
            .unwrap();
        let payload_rsvp = payload_line
            .params
            .iter()
            .find(|p| p.name == ICalendarParameterName::Rsvp)
            .map(|p| &p.value);
        assert!(matches!(
            payload_rsvp,
            Some(ICalendarParameterValue::Bool(false))
        ));
        assert!(
            payload_line
                .params
                .iter()
                .any(|p| p.name == ICalendarParameterName::Partstat
                    && matches!(
                        p.value,
                        ICalendarParameterValue::Partstat(ICalendarParticipationStatus::Declined)
                    ))
        );
        let alias_payload = payload
            .components
            .iter()
            .flat_map(|c| &c.entries)
            .find(|entry| {
                entry.name == ICalendarProperty::Attendee
                    && entry
                        .values
                        .first()
                        .and_then(ICalendarValue::as_text)
                        .is_some_and(|value| value.contains(WRITER_ALIAS))
            })
            .unwrap();
        assert!(
            alias_payload
                .params
                .iter()
                .any(|param| param.name == ICalendarParameterName::Rsvp
                    && matches!(param.value, ICalendarParameterValue::Bool(false)))
        );
        assert!(alias_payload.params.iter().any(|param| param.name
            == ICalendarParameterName::Partstat
            && matches!(
                param.value,
                ICalendarParameterValue::Partstat(ICalendarParticipationStatus::Declined)
            )));
        let client = event(
            OWNER,
            &[(
                WRITER,
                ";SCHEDULE-AGENT=CLIENT;PARTSTAT=DECLINED;RSVP=FALSE",
            )],
            "x",
            "20990301T100000Z",
            "",
        );
        assert!(matches!(
            za_self_create(&client, &writer, &owner, &res),
            Err(ItipError::NothingToSend)
        ));
        let none = event(
            OWNER,
            &[(WRITER, ";SCHEDULE-AGENT=NONE;PARTSTAT=DECLINED;RSVP=FALSE")],
            "x",
            "20990301T100000Z",
            "",
        );
        assert!(matches!(
            za_self_create(&none, &writer, &owner, &res),
            Err(ItipError::NothingToSend)
        ));

        let accepted = event(
            OWNER,
            &[(WRITER, ";PARTSTAT=ACCEPTED;RSVP=FALSE")],
            "x",
            "20990301T100000Z",
            "",
        );
        let accepted_result = za_self_create(&accepted, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert_eq!(accepted_result.messages()[0].to, vec![WRITER.to_string()]);

        let mut added = tree.clone();
        let occurrence = event(
            OWNER,
            &[(WRITER, ";PARTSTAT=ACCEPTED;RSVP=FALSE")],
            "x",
            "20990302T100000Z",
            "RECURRENCE-ID:20990302T100000Z\r\n",
        );
        let instance_id = added.components.len() as u32;
        added.components.push(occurrence.components[1].clone());
        added.components[0].component_ids.push(instance_id);
        let added_result = za_self_update(&tree, &added, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(
            added_result
                .messages()
                .iter()
                .any(|message| message.to.contains(&WRITER.to_string()))
        );
    }

    #[test]
    fn second_run_precedes_finalize() {
        let owner = account(1, &[OWNER]);
        let writer = account(2, &[WRITER]);
        let res = resolution(&writer, &owner, &[WRITER], &[WRITER], &[WRITER]);
        let old = event(OWNER, &[(WRITER, "")], "old", "20990301T100000Z", "");
        let new = event(
            OWNER,
            &[(WRITER, ";SCHEDULE-FORCE-SEND=REQUEST")],
            "new",
            "20990302T100000Z",
            "",
        );
        let unchanged = new.clone();
        let actual = za_self_update(&old, &new, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert_eq!(new, unchanged);
        assert!(!actual.messages().is_empty());
        assert!(!actual.increments().is_empty());
        assert!(actual.finalize());
        let mut owner_reference = new.clone();
        let owner_messages = itip_update(&mut owner_reference, &old, &owner.addresses).unwrap();
        assert_eq!(
            sequence(&actual.messages()[0].message),
            sequence(&owner_messages[0].message)
        );
        let mut first_run = new.clone();
        assert!(matches!(
            itip_update(&mut first_run, &old, &[WRITER.into(), OWNER.into()]),
            Err(ItipError::NothingToSend)
        ));
        za_finalize(&mut first_run, actual.increments());
        assert_eq!(sequence(&first_run), sequence(&owner_reference));
        assert!(
            first_run
                .components
                .iter()
                .flat_map(|c| &c.entries)
                .all(|e| e
                    .params
                    .iter()
                    .all(|p| format!("{:?}", p.name) != "ScheduleForceSend"))
        );
        let create_branch = za_self_create(&new, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(create_branch.finalize());
        assert!(create_branch.increments().is_empty());
        let deleted = za_self_cancel(&old, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(!deleted.finalize());
        assert_eq!(deleted.messages().len(), 1);
        assert_eq!(deleted.messages()[0].to, vec![WRITER.to_string()]);

        let summary_only = event(
            OWNER,
            &[(WRITER, "")],
            "changed summary",
            "20990301T100000Z",
            "",
        );
        let summary_result = za_self_update(&old, &summary_only, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(summary_result.increments().is_empty());
        assert_eq!(sequence(&summary_result.messages()[0].message), 2);

        // An existing-event PUT without a schedule tag explicitly dispatches
        // to create. The old tree remains available at the caller.
        assert!(matches!(
            za_self_update(&old, &old, &writer, &owner, &res),
            Err(ItipError::NothingToSend)
        ));
        let no_tag = za_self_create(&old, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert_eq!(no_tag.messages()[0].to, vec![WRITER.to_string()]);

        let mut no_organizer = old.clone();
        for component in &mut no_organizer.components {
            component
                .entries
                .retain(|e| e.name != ICalendarProperty::Organizer);
        }
        let fallback = za_self_update(&old, &no_organizer, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        assert!(!fallback.finalize());
        assert!(fallback.increments().is_empty());
        assert_eq!(fallback.messages()[0].to, vec![WRITER.to_string()]);

        let mut with_instance = old.clone();
        let recurrence = event(
            OWNER,
            &[(WRITER, "")],
            "old",
            "20990303T100000Z",
            "RECURRENCE-ID:20990303T100000Z\r\n",
        );
        let instance_id = with_instance.components.len() as u32;
        with_instance
            .components
            .push(recurrence.components[1].clone());
        with_instance.components[0].component_ids.push(instance_id);
        let removed = za_self_update(&with_instance, &old, &writer, &owner, &res)
            .unwrap()
            .unwrap();
        let mut owner_removal_tree = old.clone();
        let owner_removal =
            itip_update(&mut owner_removal_tree, &with_instance, &owner.addresses).unwrap();
        assert_eq!(removed.messages().len(), 1);
        assert_eq!(owner_removal.len(), 1);
        assert_eq!(removed.messages()[0].to, vec![WRITER.to_string()]);
        assert!(
            removed.messages()[0].message.components[0]
                .entries
                .iter()
                .any(|entry| entry.name == ICalendarProperty::Method
                    && matches!(
                        entry.values.first(),
                        Some(ICalendarValue::Method(
                            calcard::icalendar::ICalendarMethod::Cancel
                        ))
                    ))
        );
        assert_eq!(
            canonical_without_dtstamp(removed.messages()[0].message.clone()),
            canonical_without_dtstamp(owner_removal[0].message.clone()),
        );
        assert_eq!(
            sequence(&removed.messages()[0].message),
            sequence(&owner_removal[0].message)
        );
    }

    #[test]
    fn cancel_split_preserves_public_behavior() {
        let cases = [
            (OWNER, vec![(WRITER, "")], vec![OWNER.into()], true),
            (EXTERNAL, vec![(OWNER, "")], vec![OWNER.into()], true),
            (OWNER, vec![(WRITER, "")], vec![OWNER.into()], false),
            (EXTERNAL, vec![(OWNER, "")], vec![OWNER.into()], false),
            (OWNER, vec![], vec![OWNER.into()], true),
        ];
        for (organizer, attendees, addresses, is_deletion) in cases {
            let tree = event(organizer, &attendees, "x", "20990301T100000Z", "");
            let public = itip_cancel(&tree, &addresses, is_deletion);
            let from_snapshot = itip_cancel_from_snapshot(
                &tree,
                itip_snapshot(&tree, &addresses, false).unwrap(),
                is_deletion,
            );
            match (public, from_snapshot) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a.from, b.from);
                    assert_eq!(a.from_organizer, b.from_organizer);
                    assert_eq!(
                        a.to.into_iter().collect::<AHashSet<_>>(),
                        b.to.into_iter().collect::<AHashSet<_>>()
                    );
                    assert_eq!(
                        canonical_without_dtstamp(a.message),
                        canonical_without_dtstamp(b.message)
                    );
                }
                (Err(ItipError::NothingToSend), Err(ItipError::NothingToSend)) => (),
                other => panic!("unexpected cancel parity result: {other:?}"),
            }
        }
        let invalid = event(OWNER, &[(WRITER, "")], "x", "20990301T100000Z", "");
        let mut invalid = invalid;
        for comp in &mut invalid.components {
            comp.entries.retain(|e| e.name != ICalendarProperty::Uid);
        }
        assert!(matches!(
            itip_cancel(&invalid, &[OWNER.into()], true),
            Err(ItipError::MissingUid)
        ));
    }
}
