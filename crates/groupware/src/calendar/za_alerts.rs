/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 (spec 6.5, D11): a calendar's default alerts applied to an event by value. The
//! event's former alarms are removed from the tree and one alarm per default alert is written, so
//! a key account's stored event carries its reminders and no per-user data records that the
//! defaults were used (D10, invariant 59). Upstream's `apply_default_alerts` unlinks the alarms it
//! replaces, which leaves orphan components in the tree; this one removes them.
//!
//! The function writes into the plaintext tree of one request and nowhere else, before the
//! pipeline seals it. Nothing here is logged or traced.

use super::alerts::{
    DefaultAlarmText, DefaultAlerts, DefaultAlertsResolver, ICalendarShowWithoutTime,
};
use calcard::icalendar::{ICalendar, ICalendarComponentType, ICalendarProperty};
use common::Server;
use trc::AddContext;

const REMINDER: &str = "Reminder";

/// Replaces the alarms of `ical` by the default alerts of the calendars `calendar_ids` of
/// `owner_id`, as `writer_id` sees them. `writer_id` is
/// `access_token.personal_id(owner_id, Collection::Calendar)`.
///
/// The defaults are those `DefaultAlertsResolver::resolve` merges with `use_default_alerts` true,
/// chosen by `ical.shows_without_time()`.
pub async fn za_apply_default_alerts_by_value(
    server: &Server,
    resolver: &mut DefaultAlertsResolver,
    owner_id: u32,
    writer_id: u32,
    calendar_ids: &[u32],
    ical: &mut ICalendar,
) -> trc::Result<()> {
    let alerts = resolver
        .resolve(
            server,
            owner_id,
            writer_id,
            true,
            calendar_ids.iter().copied(),
            !ical.shows_without_time(),
        )
        .await?;

    // The rule of `DefaultAlertsResolver::recipient` (private): the account's name when it is not
    // empty. Only an email alert carries it.
    let recipient = if alerts.has_email_alerts() {
        let account = server
            .account(writer_id)
            .await
            .caused_by(trc::location!())?;
        (!account.name.is_empty()).then(|| account.name.to_string())
    } else {
        None
    };

    if za_replace_alarms(ical, &alerts, recipient.as_deref()) {
        Ok(())
    } else {
        Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access: default alerts not applied")
            .account_id(owner_id))
    }
}

/// Removes every VALARM of `ical` and gives each VEVENT and VTODO one alarm per alert of `alerts`.
/// `false`: the removal was refused and `ical` is unchanged.
fn za_replace_alarms(
    ical: &mut ICalendar,
    alerts: &DefaultAlerts,
    recipient: Option<&str>,
) -> bool {
    let former = ical
        .components
        .iter()
        .zip(0u32..)
        .filter(|(component, _)| component.component_type == ICalendarComponentType::VAlarm)
        .map(|(_, id)| id)
        .collect::<Vec<_>>();
    if !former.is_empty() && !ical.remove_component_ids(&former) {
        return false;
    }

    // The ids of the parents are read after the removal, which renumbers the components.
    let parents = ical
        .components
        .iter()
        .zip(0usize..)
        .filter(|(component, _)| {
            matches!(
                component.component_type,
                ICalendarComponentType::VEvent | ICalendarComponentType::VTodo
            )
        })
        .map(|(_, id)| id)
        .collect::<Vec<_>>();
    for parent in parents {
        let description = ical.components[parent]
            .property(&ICalendarProperty::Summary)
            .and_then(|entry| entry.values.first())
            .and_then(|value| value.as_text())
            .filter(|text| !text.is_empty())
            .unwrap_or(REMINDER)
            .to_string();
        let text = DefaultAlarmText {
            description: &description,
            recipient,
        };
        for (_, alert) in alerts.indexed_alerts() {
            let Ok(alarm_id) = u32::try_from(ical.components.len()) else {
                return false;
            };
            ical.components.push(alert.to_ical(Some(text)));
            ical.components[parent].component_ids.push(alarm_id);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        ALERT_EMAIL, ALERT_WITH_TIME, Calendar, CalendarPreferences, DefaultAlert,
        alerts::CalendarSettings,
    };
    use calcard::{common::IanaParse, icalendar::ICalendarDuration};

    const OWNER: u32 = 4;
    const WRITER: u32 = 9;

    fn alert(id: &str, offset: &str, flags: u16) -> DefaultAlert {
        DefaultAlert {
            id: id.to_string(),
            offset: ICalendarDuration::parse(offset.as_bytes()).unwrap(),
            flags,
        }
    }

    /// A calendar whose preference entries carry the given default alerts, as settings.
    fn settings(entries: &[(u32, Vec<DefaultAlert>)]) -> CalendarSettings {
        CalendarSettings::from(&Calendar {
            preferences: entries
                .iter()
                .map(|(account_id, alerts)| CalendarPreferences {
                    account_id: *account_id,
                    default_alerts: alerts.clone(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
    }

    fn parse(text: &str) -> ICalendar {
        ICalendar::parse(text).unwrap()
    }

    fn event(dtstart: &str, extra: &str) -> ICalendar {
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nBEGIN:VEVENT\r\nUID:a1\r\n\
             DTSTAMP:20300101T000000Z\r\n{dtstart}\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    fn defaults(
        settings: &CalendarSettings,
        personal_id: u32,
        inherit_from: Option<u32>,
        ical: &ICalendar,
    ) -> DefaultAlerts {
        DefaultAlerts::merge(
            [settings].into_iter(),
            personal_id,
            inherit_from,
            !ical.shows_without_time(),
        )
    }

    fn alarms(ical: &ICalendar) -> Vec<&calcard::icalendar::ICalendarComponent> {
        ical.components
            .iter()
            .filter(|c| c.component_type == ICalendarComponentType::VAlarm)
            .collect()
    }

    fn jsids(ical: &ICalendar) -> Vec<String> {
        alarms(ical)
            .into_iter()
            .map(|alarm| {
                alarm
                    .property(&ICalendarProperty::Jsid)
                    .and_then(|e| e.values.first())
                    .and_then(|v| v.as_text())
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    fn lines(ical: &ICalendar) -> Vec<String> {
        ical.to_string().lines().map(str::to_string).collect()
    }

    fn has_line(ical: &ICalendar, line: &str) -> bool {
        lines(ical).iter().any(|l| l == line)
    }

    #[test]
    fn timed_and_all_day_defaults() {
        let settings = settings(&[(
            OWNER,
            vec![
                alert("t1", "-PT15M", ALERT_WITH_TIME),
                alert("d1", "PT9H", 0),
            ],
        )]);

        let mut timed = event("DTSTART:20310304T100000Z", "SUMMARY:Title canary\r\n");
        let alerts = defaults(&settings, OWNER, None, &timed);
        assert!(za_replace_alarms(&mut timed, &alerts, None));
        assert_eq!(jsids(&timed), ["t1"]);
        for line in [
            "JSID:t1",
            "TRIGGER:-PT15M",
            "ACTION:DISPLAY",
            "DESCRIPTION:Title canary",
        ] {
            assert!(has_line(&timed, line), "{line} in {:?}", lines(&timed));
        }

        let mut all_day = event("DTSTART;VALUE=DATE:20310304", "SUMMARY:Title canary\r\n");
        let alerts = defaults(&settings, OWNER, None, &all_day);
        assert!(za_replace_alarms(&mut all_day, &alerts, None));
        assert_eq!(jsids(&all_day), ["d1"]);
        assert!(has_line(&all_day, "TRIGGER:PT9H"), "{:?}", lines(&all_day));
    }

    #[test]
    fn former_alarms_removed_not_orphaned() {
        let text = concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\n",
            "BEGIN:VEVENT\r\nUID:a1\r\nDTSTAMP:20300101T000000Z\r\nDTSTART:20310304T100000Z\r\n",
            "RRULE:FREQ=DAILY\r\n",
            "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT5M\r\nDESCRIPTION:old alarm canary\r\nEND:VALARM\r\n",
            "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT1H\r\nDESCRIPTION:other\r\nEND:VALARM\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:a1\r\nDTSTAMP:20300101T000000Z\r\nRECURRENCE-ID:20310305T100000Z\r\n",
            "DTSTART:20310305T110000Z\r\n",
            "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nDESCRIPTION:old alarm canary\r\nEND:VALARM\r\n",
            "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT2H\r\nDESCRIPTION:another\r\nEND:VALARM\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let mut ical = parse(text);
        assert_eq!(alarms(&ical).len(), 4);
        let settings = settings(&[(OWNER, vec![alert("t1", "-PT15M", ALERT_WITH_TIME)])]);
        let alerts = defaults(&settings, OWNER, None, &ical);
        assert!(za_replace_alarms(&mut ical, &alerts, None));

        assert_eq!(alarms(&ical).len(), 2, "one per VEVENT");
        let vevents = ical
            .components
            .iter()
            .filter(|c| c.component_type == ICalendarComponentType::VEvent)
            .collect::<Vec<_>>();
        assert_eq!(vevents.len(), 2);
        for vevent in &vevents {
            let own = vevent
                .component_ids
                .iter()
                .filter(|id| {
                    ical.components[**id as usize].component_type == ICalendarComponentType::VAlarm
                })
                .count();
            assert_eq!(own, 1);
        }
        for (index, component) in ical.components.iter().enumerate() {
            if component.component_type == ICalendarComponentType::VAlarm {
                let parents = ical
                    .components
                    .iter()
                    .filter(|c| c.component_ids.contains(&(index as u32)))
                    .count();
                assert_eq!(parents, 1, "alarm {index} is in exactly one component_ids");
            }
        }
        assert!(!ical.to_string().contains("old alarm canary"));
    }

    #[test]
    fn writer_entry_then_owner_entry() {
        let both = settings(&[
            (OWNER, vec![alert("o1", "-PT15M", ALERT_WITH_TIME)]),
            (WRITER, vec![alert("w1", "-PT20M", ALERT_WITH_TIME)]),
        ]);
        let owner_only = settings(&[(OWNER, vec![alert("o1", "-PT15M", ALERT_WITH_TIME)])]);

        for (settings, inherit_from, expected) in [
            (&both, Some(OWNER), vec!["w1"]),
            (&owner_only, Some(OWNER), vec!["o1"]),
            (&owner_only, None, vec![]),
        ] {
            let mut ical = event("DTSTART:20310304T100000Z", "SUMMARY:x\r\n");
            let alerts = defaults(settings, WRITER, inherit_from, &ical);
            assert!(za_replace_alarms(&mut ical, &alerts, None));
            assert_eq!(jsids(&ical), expected, "inherit_from {inherit_from:?}");
        }
    }

    #[test]
    fn email_alert_names_the_writer() {
        let settings = settings(&[(
            WRITER,
            vec![alert("e1", "-PT30M", ALERT_WITH_TIME | ALERT_EMAIL)],
        )]);

        let mut ical = event("DTSTART:20310304T100000Z", "SUMMARY:Mail canary\r\n");
        let alerts = defaults(&settings, WRITER, None, &ical);
        assert!(za_replace_alarms(
            &mut ical,
            &alerts,
            Some("writer@example.com")
        ));
        for line in [
            "ACTION:EMAIL",
            "ATTENDEE:mailto:writer@example.com",
            "SUMMARY:Mail canary",
            "DESCRIPTION:Mail canary",
        ] {
            assert!(has_line(&ical, line), "{line} in {:?}", lines(&ical));
        }

        let mut ical = event("DTSTART:20310304T100000Z", "SUMMARY:Mail canary\r\n");
        let alerts = defaults(&settings, WRITER, None, &ical);
        assert!(za_replace_alarms(&mut ical, &alerts, None));
        assert!(has_line(&ical, "ACTION:EMAIL"));
        assert!(
            !lines(&ical).iter().any(|l| l.starts_with("ATTENDEE")),
            "{:?}",
            lines(&ical)
        );
    }

    #[test]
    fn no_defaults_leaves_no_alarm() {
        let own =
            "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT5M\r\nDESCRIPTION:mine\r\nEND:VALARM\r\n";
        let none = settings(&[(OWNER, vec![])]);
        let mut ical = event("DTSTART:20310304T100000Z", &format!("SUMMARY:x\r\n{own}"));
        assert_eq!(alarms(&ical).len(), 1);
        let alerts = defaults(&none, OWNER, None, &ical);
        assert!(za_replace_alarms(&mut ical, &alerts, None));
        assert!(alarms(&ical).is_empty());
        assert!(!ical.to_string().contains("VALARM"));

        let one = settings(&[(OWNER, vec![alert("t1", "-PT15M", ALERT_WITH_TIME)])]);
        for extra in ["", "SUMMARY:\r\n"] {
            let mut ical = event("DTSTART:20310304T100000Z", extra);
            let alerts = defaults(&one, OWNER, None, &ical);
            assert!(za_replace_alarms(&mut ical, &alerts, None));
            assert_eq!(jsids(&ical), ["t1"]);
            assert!(
                has_line(&ical, "DESCRIPTION:Reminder"),
                "{extra:?}: {:?}",
                lines(&ical)
            );
        }
    }
}
