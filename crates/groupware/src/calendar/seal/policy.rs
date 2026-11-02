/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use calcard::icalendar::{
    ICalendarClassification, ICalendarComponentType, ICalendarEntry, ICalendarParameterName,
    ICalendarProperty, ICalendarValue,
};

/// Recorded with every sealed object (spec 6). Field policy version 2 (spec
/// 6, revision 8) keeps this byte at 1: it opens every key-wrap envelope and
/// is bound into the associated data, and version 2 changes no format. It is
/// a property of the store: a Stalwart 1.0 store starts fresh, and every
/// object in it is written under version 2.
pub const POLICY_VERSION: u8 = 1;

/// Field policy version 2, levels 1 and 2 with the value level for CLASS:
/// whether `entry` stays visible in a component of type `component`. CLASS
/// is visible on VEVENT, VTODO and VJOURNAL when its one value is PUBLIC,
/// PRIVATE or CONFIDENTIAL; any other CLASS is sealed. Upstream derives
/// `EVENT_PRIVATE` and `EVENT_SECRET` from the stored tree, so no fork code
/// computes them. Every other property follows `is_visible_property`.
pub fn is_visible_entry(component: &ICalendarComponentType, entry: &ICalendarEntry) -> bool {
    match &entry.name {
        ICalendarProperty::Class => {
            matches!(
                component,
                ICalendarComponentType::VEvent
                    | ICalendarComponentType::VTodo
                    | ICalendarComponentType::VJournal
            ) && matches!(
                entry.values.as_slice(),
                [ICalendarValue::Classification(
                    ICalendarClassification::Public
                        | ICalendarClassification::Private
                        | ICalendarClassification::Confidential
                )]
            )
        }
        name => is_visible_property(component, name),
    }
}

/// Three-level allowlist, level 1 and 2: component type and property.
/// Anything not listed is sealed, including every `X-` property, with two
/// exceptions:
/// - UID is visible on every component type: it is identity, not content, and
///   free/busy- or availability-only objects need it for their associated data
///   and for upstream's UID index.
/// - On VTIMEZONE, `X-LIC-LOCATION` and `X-MICROSOFT-CDO-TZID` are visible
///   (case-insensitively): calcard resolves a timezone by name only (TZID, else
///   these two), so they must survive sealing.
///
/// CLASS is decided by its value (`is_visible_entry`), so it is not listed
/// here; callers decide entries, never bare names.
fn is_visible_property(component: &ICalendarComponentType, property: &ICalendarProperty) -> bool {
    use ICalendarComponentType as C;
    use ICalendarProperty as P;
    if matches!(property, P::Uid) {
        return true;
    }
    match component {
        C::VCalendar => matches!(property, P::Prodid | P::Version | P::Calscale | P::Method),
        C::VEvent | C::VTodo | C::VJournal => matches!(
            property,
            P::Dtstart
                | P::Dtend
                | P::Duration
                | P::Due
                | P::Rrule
                | P::Rdate
                | P::Exdate
                | P::RecurrenceId
                | P::Sequence
                | P::Status
                | P::Transp
                | P::Dtstamp
                | P::Created
                | P::LastModified
        ),
        C::VAlarm => matches!(property, P::Trigger | P::Action | P::Repeat | P::Duration),
        C::VTimezone => match property {
            P::Tzid | P::LastModified => true,
            P::Other(name) => {
                name.eq_ignore_ascii_case("X-LIC-LOCATION")
                    || name.eq_ignore_ascii_case("X-MICROSOFT-CDO-TZID")
            }
            _ => false,
        },
        C::Standard | C::Daylight => matches!(
            property,
            P::Dtstart | P::Tzoffsetfrom | P::Tzoffsetto | P::Rrule | P::Rdate
        ),
        _ => false,
    }
}

/// Level 3: parameters of a visible property. The period and date forms of
/// RDATE and EXDATE are expressed through VALUE, which is visible.
pub fn is_visible_parameter(parameter: &ICalendarParameterName) -> bool {
    matches!(
        parameter,
        ICalendarParameterName::Value
            | ICalendarParameterName::Tzid
            | ICalendarParameterName::Range
            | ICalendarParameterName::Related
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use calcard::icalendar::{
        ICalendarComponentType as C, ICalendarParameterName as N, ICalendarProperty as P,
    };

    fn entry(name: P, values: Vec<ICalendarValue>) -> ICalendarEntry {
        ICalendarEntry {
            name,
            params: vec![],
            values: values.into(),
        }
    }

    fn class(class: ICalendarClassification) -> ICalendarEntry {
        entry(P::Class, vec![ICalendarValue::Classification(class)])
    }

    #[test]
    fn standard_class_is_visible_on_event_task_and_journal() {
        for ct in [C::VEvent, C::VTodo, C::VJournal] {
            for value in [
                ICalendarClassification::Public,
                ICalendarClassification::Private,
                ICalendarClassification::Confidential,
            ] {
                assert!(
                    is_visible_entry(&ct, &class(value.clone())),
                    "{value:?} in {ct:?}"
                );
            }
        }
    }

    #[test]
    fn other_class_values_and_components_stay_sealed() {
        let public = class(ICalendarClassification::Public);
        for ct in [
            C::VCalendar,
            C::VAlarm,
            C::VFreebusy,
            C::VTimezone,
            C::Standard,
            C::Daylight,
            C::VAvailability,
            C::Other("X-THING".into()),
        ] {
            assert!(!is_visible_entry(&ct, &public), "CLASS in {ct:?}");
        }
        for values in [
            vec![ICalendarValue::Text("X-SOMETHING".into())],
            vec![ICalendarValue::Text("PUBLIC".into())],
            vec![],
            vec![
                ICalendarValue::Classification(ICalendarClassification::Public),
                ICalendarValue::Classification(ICalendarClassification::Private),
            ],
        ] {
            assert!(
                !is_visible_entry(&C::VEvent, &entry(P::Class, values.clone())),
                "{values:?}"
            );
        }
    }

    #[test]
    fn a_parsed_extension_class_stays_sealed() {
        for (text, visible) in [
            ("PUBLIC", true),
            ("PRIVATE", true),
            ("CONFIDENTIAL", true),
            ("X-SOMETHING", false),
        ] {
            let ical = calcard::icalendar::ICalendar::parse(format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:x\r\n\
                 DTSTART:20250101T000000Z\r\nCLASS:{text}\r\nEND:VEVENT\r\n\
                 END:VCALENDAR\r\n"
            ))
            .unwrap();
            let entry = ical.components[1]
                .entries
                .iter()
                .find(|e| e.name == P::Class)
                .unwrap();
            assert_eq!(is_visible_entry(&C::VEvent, entry), visible, "{text}");
        }
    }

    #[test]
    fn event_allowlist_matches_spec_section_6() {
        for ct in [C::VEvent, C::VTodo, C::VJournal] {
            for p in [
                P::Uid,
                P::Dtstart,
                P::Dtend,
                P::Duration,
                P::Due,
                P::Rrule,
                P::Rdate,
                P::Exdate,
                P::RecurrenceId,
                P::Sequence,
                P::Status,
                P::Transp,
                P::Dtstamp,
                P::Created,
                P::LastModified,
            ] {
                assert!(is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
            for p in [
                P::Summary,
                P::Description,
                P::Location,
                P::Geo,
                P::Url,
                P::Attendee,
                P::Organizer,
                P::Categories,
                P::Comment,
                P::Contact,
                P::Resources,
                P::Attach,
                P::RelatedTo,
                P::Priority,
                P::Color,
                P::Conference,
                P::Image,
                P::StructuredData,
                P::Other("X-APPLE-TRAVEL".into()),
                P::Other("x-za-sealed".into()),
            ] {
                assert!(!is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
            // Policy version 2: CLASS is decided by its value.
            assert!(!is_visible_property(&ct, &P::Class));
            assert!(is_visible_entry(
                &ct,
                &class(ICalendarClassification::Confidential)
            ));
            assert!(!is_visible_entry(
                &ct,
                &entry(P::Class, vec![ICalendarValue::Text("X-SOMETHING".into())])
            ));
        }
    }

    #[test]
    fn alarm_timezone_and_root_allowlists() {
        for p in [P::Trigger, P::Action, P::Repeat, P::Duration] {
            assert!(is_visible_property(&C::VAlarm, &p));
        }
        for p in [P::Summary, P::Description, P::Attendee, P::Attach] {
            assert!(!is_visible_property(&C::VAlarm, &p));
        }
        assert!(is_visible_property(&C::VTimezone, &P::Tzid));
        assert!(is_visible_property(&C::VTimezone, &P::LastModified));
        assert!(is_visible_property(
            &C::VTimezone,
            &P::Other("X-LIC-LOCATION".into())
        ));
        assert!(is_visible_property(
            &C::VTimezone,
            &P::Other("x-microsoft-cdo-tzid".into())
        ));
        assert!(!is_visible_property(
            &C::VTimezone,
            &P::Other("X-WR-TIMEZONE".into())
        ));
        assert!(!is_visible_property(&C::VTimezone, &P::Tzurl));
        assert!(!is_visible_property(&C::VTimezone, &P::Comment));
        for ct in [C::Standard, C::Daylight] {
            for p in [
                P::Dtstart,
                P::Tzoffsetfrom,
                P::Tzoffsetto,
                P::Rrule,
                P::Rdate,
            ] {
                assert!(is_visible_property(&ct, &p));
            }
            assert!(!is_visible_property(&ct, &P::Tzname));
            assert!(!is_visible_property(&ct, &P::Comment));
            assert!(!is_visible_property(
                &ct,
                &P::Other("X-LIC-LOCATION".into())
            ));
        }
        for p in [P::Prodid, P::Version, P::Calscale, P::Method] {
            assert!(is_visible_property(&C::VCalendar, &p));
        }
        assert!(!is_visible_property(&C::VCalendar, &P::Name));
        assert!(!is_visible_property(
            &C::VCalendar,
            &P::Other("X-WR-CALNAME".into())
        ));
    }

    #[test]
    fn unknown_components_show_only_uid() {
        for ct in [
            C::VFreebusy,
            C::VAvailability,
            C::Available,
            C::Participant,
            C::VLocation,
            C::VResource,
            C::Other("X-THING".into()),
        ] {
            assert!(is_visible_property(&ct, &P::Uid), "Uid in {ct:?}");
            for p in [P::Dtstart, P::Summary] {
                assert!(!is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
        }
    }

    #[test]
    fn parameter_allowlist() {
        for n in [N::Value, N::Tzid, N::Range, N::Related] {
            assert!(is_visible_parameter(&n));
        }
        for n in [
            N::Cn,
            N::Partstat,
            N::Role,
            N::Rsvp,
            N::Altrep,
            N::Language,
            N::Member,
            N::Email,
            N::Other("X-APPLE-RADIUS".into()),
        ] {
            assert!(!is_visible_parameter(&n));
        }
    }
}
