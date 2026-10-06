# Zero-Access Calendar, Plan 2 of 3: Sealed Storage and the CalDAV Paths

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Calendar events, tasks, journals and calendar collections of key accounts are stored sealed (field-level encryption under a per-object data key wrapped to the account's event key), every CalDAV read path unseals them, every write path seals them, and no session without the account's keys can reach that account's calendar data.

**Architecture:** A `seal` module in the `groupware` crate implements the field policy (three-level allowlist), the per-component removal bundle (`X-ZA-SEALED`), the per-event key envelope (`X-ZA-KEY`), the extra-field bundle (`X-ZA-EXTRA`) and the collection bundle carried in the owner's `CalendarPreferences.name`. Ciphertext rides inside existing fields; no stored struct changes layout. Unsealing re-serializes the owned struct into a fresh archive buffer whose `version` is copied from the stored archive, so ETags and conditional headers stay bound to the stored bytes. The `dav` crate gains a gate in URI validation and one-line seal/unseal calls at each read and write site.

**Tech Stack:** `calcard 0.3.14` (`ICalendar`, `ICalendarComponent`, `ICalendarEntry`, `ICalendarParameter`, rkyv derives enabled), `rkyv 0.8.18`, `base64 0.23`, the `vault` crate from plan 1, Stalwart `groupware`, `dav`, `store`.

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (revision 4), sections 6, 7, 8, 10 and invariants 1-5. Plan 1 (`2026-10-06-zero-access-1-accounts.md`) must be complete first; plan 3 follows.

## Global Constraints

- Policy version 1 (spec 6). Visible properties: VCALENDAR root: PRODID, VERSION, CALSCALE, METHOD. VEVENT/VTODO/VJOURNAL: UID, DTSTART, DTEND, DURATION, DUE, RRULE, RDATE, EXDATE, RECURRENCE-ID, SEQUENCE, STATUS, TRANSP, DTSTAMP, CREATED, LAST-MODIFIED. VALARM: TRIGGER, ACTION, REPEAT, DURATION. VTIMEZONE: TZID, LAST-MODIFIED; STANDARD/DAYLIGHT: DTSTART, TZOFFSETFROM, TZOFFSETTO, RRULE, RDATE. Visible parameters on visible properties: VALUE, TZID, RANGE, RELATED. Every other component type has no visible properties. Everything else is sealed, including every `X-` property and parameter.
- Carrier properties: `X-ZA-SEALED` (one per component with removals, always the component's last entry), `X-ZA-EXTRA` and `X-ZA-KEY` on the VCALENDAR root (in that order, after `X-ZA-SEALED`). A collection's bundle is `CalendarPreferences.name = "$za$" + base64(envelope) + "|" + sealed bundle`.
- Bundle plaintext: 4-byte little-endian length, rkyv bytes, zero padding to a multiple of 256 bytes; ciphertext text is base64 of `format byte 0x01 || nonce || ciphertext` (spec 7.1).
- Associated data: trees `za/v1|tree|<account>|<scope>|<component index>|<policy>` where scope is the event UID or `calendar-tz`; event key `za/v1|event-key|<account>|<uid>`; event extra `za/v1|event-extra|<account>|<uid>`; collection key `za/v1|calendar-key|<account>`; collection bundle `za/v1|calendar-bundle|<account>` (collections bound to the account only, spec 7.2).
- A new DEK per write; wrap type `mk` only in this release (byte `0x01`; `0x02` reserved for `pk`).
- The stored `size` stays `bytes.len()` of the PUT body (spec 7.1). Response identity (ETag, schedule tag, modified time, sync tokens) comes only from the stored archive; the unsealed archive is a content view (spec 7.3, invariant 5). The index builder's `current` is always the stored sealed archive.
- Sealing never adds, removes or reorders components or `component_ids`; unsealing restores entries and parameters at their original indices (invariant 2).
- Unseal failure is a 500 for DAV with a logged error naming account, collection and document id and never any content; a multi-item report fails that one item (spec 10). A key-account calendar operation without `SessionKeys` is a 403 with a `SecurityEvent::Unauthorized` event whose details start with `zero-access:` (spec 10).
- Non-key accounts take unchanged code paths: every new call is behind `if let Some(keys) = &za_keys` (invariant 9).
- **Shipping build excludes the enterprise feature.** The product binary is built with `cargo build --release -p stalwart --no-default-features --features rocks` (add other store backends by name as needed, never `enterprise`). Code under `cfg(feature = "enterprise")` and the whole `scim` crate are licensed only under the Stalwart Enterprise License and are not part of the product. Every compile check in these plans that builds the server uses the same flags, so fork code is always verified in the shipping configuration. The `tests` crate enables `enterprise` on `store`, `directory` and `coordinator` for upstream's own test modules; that is test-only and stays as is.
- Code in this plan was written without a compiler; small type and import fixes are expected.

## Review Focus

1. **A client echoing the server's carriers back** (`X-ZA-KEY`, `X-ZA-SEALED` in a PUT body, or a crafted trailing `X-ZA-SEALED` with garbage): sealing must treat them as ordinary sealed `X-` properties and restore them unchanged; unsealing a component whose last entry is a client-supplied carrier must fail cleanly, never panic (Task 2 test `client_supplied_carriers_round_trip_and_garbage_fails`).
2. **Repeated parameters of one name** (`ATTENDEE;MEMBER=a;MEMBER=b`, `RDATE;VALUE=PERIOD;TZID=..`): parameter indices must restore exact order, and calcard's writer joins same-named adjacent parameters with commas only when they are adjacent, so order matters (Task 2 corpus case `repeated_parameters`).
3. **A tampered or truncated bundle, or a bundle moved to another component index** must yield `SealError::Aead` or `Format`, never a panic and never partial data (Task 2 test `tampering_is_an_error_not_a_panic`).
4. **A PUT whose only change is in a sealed field** (for example SUMMARY) must not hit the no-change shortcut: the comparison runs against the unsealed view, and the new write gets a fresh DEK and a new ETag (Task 6 test in `dav_seal.rs`, "sealed-only change").
5. **A collection created by the server itself** (the default calendar, built by the resource cache without a key) has a plaintext name and no bundle; unsealing must pass it through, and the first owner PROPPATCH seals it (Task 4 test `plaintext_collection_passes_through`; Task 8 PROPPATCH test on `default`).

---

### Task 1: Field policy

**Files:**
- Create: `crates/groupware/src/calendar/seal/mod.rs`
- Create: `crates/groupware/src/calendar/seal/policy.rs`
- Modify: `crates/groupware/src/calendar/mod.rs` (`pub mod seal;`)
- Modify: `crates/groupware/Cargo.toml` (add `vault`, `base64`)

**Interfaces:**
- Produces: `groupware::calendar::seal::policy::{POLICY_VERSION: u8 = 1, is_visible_property(&ICalendarComponentType, &ICalendarProperty) -> bool, is_visible_parameter(&ICalendarParameterName) -> bool}`.

- [ ] **Step 1: Write the failing tests**

`crates/groupware/src/calendar/seal/policy.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

#[cfg(test)]
mod tests {
    use super::*;
    use calcard::icalendar::{ICalendarComponentType as C, ICalendarParameterName as N, ICalendarProperty as P};

    #[test]
    fn event_allowlist_matches_spec_section_6() {
        for ct in [C::VEvent, C::VTodo, C::VJournal] {
            for p in [P::Uid, P::Dtstart, P::Dtend, P::Duration, P::Due, P::Rrule, P::Rdate, P::Exdate, P::RecurrenceId, P::Sequence, P::Status, P::Transp, P::Dtstamp, P::Created, P::LastModified] {
                assert!(is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
            for p in [P::Summary, P::Description, P::Location, P::Geo, P::Url, P::Attendee, P::Organizer, P::Categories, P::Comment, P::Contact, P::Resources, P::Attach, P::RelatedTo, P::Class, P::Priority, P::Color, P::Conference, P::Image, P::StructuredData, P::Other("X-APPLE-TRAVEL".into()), P::Other("x-za-sealed".into())] {
                assert!(!is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
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
        assert!(!is_visible_property(&C::VTimezone, &P::Tzurl));
        assert!(!is_visible_property(&C::VTimezone, &P::Comment));
        for ct in [C::Standard, C::Daylight] {
            for p in [P::Dtstart, P::Tzoffsetfrom, P::Tzoffsetto, P::Rrule, P::Rdate] {
                assert!(is_visible_property(&ct, &p));
            }
            assert!(!is_visible_property(&ct, &P::Tzname));
            assert!(!is_visible_property(&ct, &P::Comment));
        }
        for p in [P::Prodid, P::Version, P::Calscale, P::Method] {
            assert!(is_visible_property(&C::VCalendar, &p));
        }
        assert!(!is_visible_property(&C::VCalendar, &P::Name));
        assert!(!is_visible_property(&C::VCalendar, &P::Other("X-WR-CALNAME".into())));
    }

    #[test]
    fn unknown_components_have_nothing_visible() {
        for ct in [C::VFreebusy, C::VAvailability, C::Available, C::Participant, C::VLocation, C::VResource, C::Other("X-THING".into())] {
            for p in [P::Uid, P::Dtstart, P::Summary] {
                assert!(!is_visible_property(&ct, &p), "{p:?} in {ct:?}");
            }
        }
    }

    #[test]
    fn parameter_allowlist() {
        for n in [N::Value, N::Tzid, N::Range, N::Related] {
            assert!(is_visible_parameter(&n));
        }
        for n in [N::Cn, N::Partstat, N::Role, N::Rsvp, N::Altrep, N::Language, N::Member, N::Email, N::Other("X-APPLE-RADIUS".into())] {
            assert!(!is_visible_parameter(&n));
        }
    }
}
```

`crates/groupware/src/calendar/seal/mod.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).

pub mod collection;
pub mod event;
pub mod policy;
pub mod tree;

pub use collection::{seal_calendar, unseal_calendar, unseal_calendar_archive};
pub use event::{seal_event, unseal_event, unseal_event_archive};
pub use tree::{SealError, seal_error, tree_has_carriers};
```

(Create `collection.rs`, `event.rs` and `tree.rs` as empty files for now; they are filled in Tasks 2-4. Add `pub mod seal;` to `crates/groupware/src/calendar/mod.rs`, and `vault = { path = "../vault" }` plus `base64 = "0.23"` to `crates/groupware/Cargo.toml`.)

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p groupware seal::policy 2>&1 | tail -5
```

Expected: unresolved `is_visible_property`.

- [ ] **Step 3: Implement**

Prepend to `policy.rs`:

```rust
use calcard::icalendar::{ICalendarComponentType, ICalendarParameterName, ICalendarProperty};

/// Recorded with every sealed object (spec 6).
pub const POLICY_VERSION: u8 = 1;

/// Three-level allowlist, level 1 and 2: component type and property.
/// Anything not listed is sealed, including every `X-` property.
pub fn is_visible_property(component: &ICalendarComponentType, property: &ICalendarProperty) -> bool {
    use ICalendarComponentType as C;
    use ICalendarProperty as P;
    match component {
        C::VCalendar => matches!(property, P::Prodid | P::Version | P::Calscale | P::Method),
        C::VEvent | C::VTodo | C::VJournal => matches!(
            property,
            P::Uid
                | P::Dtstart
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
        C::VTimezone => matches!(property, P::Tzid | P::LastModified),
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
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p groupware seal::policy 2>&1 | tail -5
git add crates/groupware
git commit -m "Add the zero-access field policy for iCalendar trees"
```

---

### Task 2: Sealing and unsealing iCalendar trees

**Files:**
- Create (fill): `crates/groupware/src/calendar/seal/tree.rs`

**Interfaces:**
- Consumes: `policy`, `vault::keys::{Secret, seal, open, aad}`.
- Produces: `tree::{SEALED_PROP, KEY_PROP, EXTRA_PROP, SealError, seal_error(SealError, account_id, document_id) -> trc::Error, seal_tree(&mut ICalendar, &Secret, account_id, scope: &str) -> bool, unseal_tree(&mut ICalendar, &Secret, account_id, scope) -> Result<(), SealError>, tree_has_carriers(&ICalendar) -> bool, seal_bytes(&Secret, aad, plaintext) -> String, open_bytes(&Secret, aad, text) -> Result<Vec<u8>, SealError>, text_entry(name, text) -> ICalendarEntry, entry_text(&ICalendarEntry) -> Option<&str>, is_carrier(&ICalendarEntry, name) -> bool}`.

- [ ] **Step 1: Write the failing tests**

Tests go at the bottom of `tree.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use calcard::{Entry, Parser};
    use vault::keys::Secret;

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("not an icalendar: {other:?}"),
        }
    }

    fn visible_only(ical: &ICalendar) -> bool {
        ical.components.iter().all(|c| {
            c.entries.iter().all(|e| {
                is_carrier(e, SEALED_PROP)
                    || (is_visible_property(&c.component_type, &e.name)
                        && e.params.iter().all(|p| is_visible_parameter(&p.name)))
            })
        })
    }

    fn round_trip(text: &str) -> ICalendar {
        let original = parse(text);
        let mut sealed = original.clone();
        let dek = Secret::random();
        seal_tree(&mut sealed, &dek, 7, "uid-1");
        assert!(visible_only(&sealed), "{}", sealed.to_string());
        assert_eq!(sealed.components.len(), original.components.len());
        for (a, b) in sealed.components.iter().zip(original.components.iter()) {
            assert_eq!(a.component_ids, b.component_ids);
            assert_eq!(a.component_type, b.component_type);
            if let Some(last) = a.entries.last().filter(|e| is_carrier(e, SEALED_PROP)) {
                let raw = base64::engine::general_purpose::STANDARD.decode(entry_text(last).unwrap()).unwrap();
                // format byte + nonce + (padded plaintext + tag): padded part is a multiple of 256
                assert_eq!((raw.len() - 1 - 24 - 16) % 256, 0, "bundle sizes are on 256-byte boundaries");
            }
        }
        let mut unsealed = sealed.clone();
        unseal_tree(&mut unsealed, &dek, 7, "uid-1").unwrap();
        assert_eq!(unsealed, original, "entry-for-entry, in order");
        assert_eq!(unsealed.to_string(), original.to_string());
        sealed
    }

    const APPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//macOS 14.0//EN\r\nCALSCALE:GREGORIAN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nDTSTART:19810329T020000\r\nTZNAME:CEST\r\nTZOFFSETTO:+0200\r\nEND:DAYLIGHT\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nDTSTART:19961027T030000\r\nTZNAME:CET\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nCREATED:20240101T100000Z\r\nUID:1A2B3C4D-APPLE\r\nDTEND;TZID=Europe/Berlin:20240115T110000\r\nTRANSP:OPAQUE\r\nX-APPLE-TRAVEL-ADVISORY-BEHAVIOR:AUTOMATIC\r\nSUMMARY:Dentist canary-apple\r\nLAST-MODIFIED:20240101T100000Z\r\nDTSTAMP:20240101T100000Z\r\nDTSTART;TZID=Europe/Berlin:20240115T100000\r\nLOCATION:Hauptstrasse 1\\, Berlin\r\nX-APPLE-STRUCTURED-LOCATION;VALUE=URI;X-APPLE-RADIUS=70;X-TITLE=Hauptstrasse 1:geo:52.52,13.40\r\nSEQUENCE:1\r\nBEGIN:VALARM\r\nX-WR-ALARMUID:9F8E7D6C\r\nUID:9F8E7D6C\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Event reminder\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const THUNDERBIRD: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nCREATED:20240201T090000Z\r\nLAST-MODIFIED:20240201T091500Z\r\nDTSTAMP:20240201T091500Z\r\nUID:tb-5e6f7a8b\r\nSUMMARY:Team sync canary-tb\r\nCATEGORIES:Work,Meetings\r\nSTATUS:CONFIRMED\r\nORGANIZER;CN=Jane:mailto:jane@example.com\r\nATTENDEE;CN=John;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:john@example.com\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE:20240219T100000Z\r\nDTSTART:20240205T100000Z\r\nDTEND:20240205T103000Z\r\nTRANSP:OPAQUE\r\nX-MOZ-GENERATION:3\r\nX-MOZ-LASTACK:20240201T091500Z\r\nDESCRIPTION:Weekly\\nAgenda canary-tb-desc\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER;VALUE=DURATION;RELATED=END:-PT5M\r\nDESCRIPTION:Default Mozilla Description\r\nSUMMARY:Default Mozilla Summary\r\nATTENDEE:mailto:john@example.com\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nVERSION:2.0\r\nCALSCALE:GREGORIAN\r\nMETHOD:PUBLISH\r\nX-WR-CALNAME:canary-calname\r\nX-WR-TIMEZONE:America/New_York\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20240301\r\nDTEND;VALUE=DATE:20240302\r\nDTSTAMP:20240210T120000Z\r\nUID:google-abc123@google.com\r\nCREATED:20240210T120000Z\r\nDESCRIPTION:All day canary-google\r\nLAST-MODIFIED:20240210T120000Z\r\nSEQUENCE:0\r\nSTATUS:CONFIRMED\r\nSUMMARY:Holiday\r\nTRANSP:TRANSPARENT\r\nATTACH;FMTTYPE=application/pdf;X-GOOGLE-ID=1:https://drive.google.com/file/d/x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const DAVX5: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:+//IDN bitfire.at//DAVx5/4.3.9 ical4j/3.2.14\r\nBEGIN:VEVENT\r\nDTSTAMP:20240305T080000Z\r\nUID:davx5-77\r\nSEQUENCE:2\r\nSUMMARY:Run canary-davx5\r\nDTSTART;TZID=Europe/Vienna:20240306T070000\r\nDURATION:PT1H\r\nRDATE;VALUE=PERIOD;TZID=Europe/Vienna:20240308T070000/PT30M\r\nRECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/Vienna:20240306T070000\r\nCLASS:PRIVATE\r\nPRIORITY:5\r\nGEO:48.2082;16.3738\r\nURL:https://example.com/run\r\nCOLOR:tomato\r\nCONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=Call:https://meet.example.com/run\r\nX-RADICALE-NAME:run.ics\r\nBEGIN:VALARM\r\nTRIGGER;RELATED=START:-PT10M\r\nACTION:AUDIO\r\nREPEAT:2\r\nDURATION:PT1M\r\nATTACH;VALUE=URI:Basso\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Vienna\r\nX-LIC-LOCATION:Europe/Vienna\r\nLAST-MODIFIED:20230101T000000Z\r\nTZURL:http://tzurl.org/zoneinfo/Europe/Vienna\r\nBEGIN:STANDARD\r\nTZNAME:CET\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nDTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nCOMMENT:canary-tz-comment\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n";

    const SYNTHETIC_INTERLEAVED: &str = "BEGIN:VCALENDAR\r\nX-FIRST:1\r\nVERSION:2.0\r\nX-SECOND:2\r\nPRODID:-//x//EN\r\nNAME:canary-name\r\nBEGIN:VEVENT\r\nSUMMARY:a\r\nUID:u\r\nSUMMARY:b\r\nDTSTART;X-ORIGIN=canary-param;TZID=UTC;X-OTHER=2:20240101T000000\r\nDESCRIPTION:c\r\nDTEND;VALUE=DATE-TIME;X-TAIL=1:20240101T010000\r\nCATEGORIES:x,y\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const REPEATED_PARAMETERS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:rp\r\nDTSTART;TZID=UTC:20240101T000000\r\nATTENDEE;MEMBER=\"mailto:a@x\";MEMBER=\"mailto:b@x\";CN=Zed;DELEGATED-FROM=\"mailto:c@x\";DELEGATED-FROM=\"mailto:d@x\":mailto:z@x\r\nRDATE;VALUE=DATE;X-ONE=1;X-ONE=2:20240102,20240103\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn corpus_round_trips() {
        for text in [APPLE, THUNDERBIRD, GOOGLE, DAVX5, SYNTHETIC_INTERLEAVED] {
            let sealed = round_trip(text);
            let dump = sealed.to_string();
            for canary in ["canary-", "Dentist", "Hauptstrasse", "Team sync", "jane@example.com", "Holiday", "tomato", "Basso", "CET", "CEST"] {
                assert!(!dump.contains(canary), "{canary} leaked in {dump}");
            }
        }
    }

    #[test]
    fn repeated_parameters() {
        let sealed = round_trip(REPEATED_PARAMETERS);
        assert!(!sealed.to_string().contains("MEMBER"));
    }

    #[test]
    fn nothing_to_seal_adds_no_carrier() {
        let mut ical = parse("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u\r\nDTSTART;TZID=UTC:20240101T000000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let original = ical.clone();
        assert!(!seal_tree(&mut ical, &Secret::random(), 1, "u"));
        assert_eq!(ical, original);
        assert!(!tree_has_carriers(&ical));
    }

    #[test]
    fn client_supplied_carriers_round_trip_and_garbage_fails() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-ZA-KEY:AQE=\r\nBEGIN:VEVENT\r\nUID:u\r\nX-ZA-SEALED:not-ours\r\nDTSTART;TZID=UTC:20240101T000000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let sealed = round_trip(text);
        // Our carrier is the last entry; the client's are inside the bundle.
        assert_eq!(sealed.components[1].entries.iter().filter(|e| is_carrier(e, SEALED_PROP)).count(), 1);
        // A tree whose trailing carrier is client garbage: error, no panic.
        let mut garbage = parse(text);
        assert!(matches!(unseal_tree(&mut garbage, &Secret::random(), 1, "u"), Err(SealError::Decode | SealError::Format | SealError::Aead)));
    }

    #[test]
    fn tampering_is_an_error_not_a_panic() {
        let dek = Secret::random();
        let mut sealed = parse(THUNDERBIRD);
        seal_tree(&mut sealed, &dek, 7, "uid-1");
        // Wrong account, wrong scope, wrong key.
        for (account, scope, key) in [(8u32, "uid-1", dek.clone()), (7, "uid-2", dek.clone()), (7, "uid-1", Secret::random())] {
            let mut t = sealed.clone();
            assert_eq!(unseal_tree(&mut t, &key, account, scope), Err(SealError::Aead));
        }
        // Bundle moved to another component index (VCALENDAR root has no sealed props here? it has X- none; use the event bundle on the alarm).
        let mut moved = sealed.clone();
        let event_carrier = moved.components[1].entries.pop().unwrap();
        moved.components[2].entries.pop();
        moved.components[2].entries.push(event_carrier);
        assert!(matches!(unseal_tree(&mut moved, &dek, 7, "uid-1"), Err(SealError::Aead)));
        // Truncated and corrupted base64.
        let mut truncated = sealed.clone();
        if let Some(ICalendarValue::Text(t)) = truncated.components[1].entries.last_mut().unwrap().values.first_mut() {
            t.truncate(10);
        }
        assert!(matches!(unseal_tree(&mut truncated, &dek, 7, "uid-1"), Err(SealError::Decode | SealError::Format | SealError::Aead)));
        let mut flipped = sealed.clone();
        if let Some(ICalendarValue::Text(t)) = flipped.components[1].entries.last_mut().unwrap().values.first_mut() {
            let mut bytes = base64::engine::general_purpose::STANDARD.decode(t.as_str()).unwrap();
            bytes[40] ^= 1;
            *t = base64::engine::general_purpose::STANDARD.encode(bytes);
        }
        assert_eq!(unseal_tree(&mut flipped, &dek, 7, "uid-1"), Err(SealError::Aead));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p groupware seal::tree 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

Top of `tree.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::policy::{POLICY_VERSION, is_visible_parameter, is_visible_property};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::icalendar::{
    ICalendar, ICalendarComponent, ICalendarEntry, ICalendarParameter, ICalendarProperty,
    ICalendarValue,
};
use vault::keys::{Secret, aad, open, seal};

pub const SEALED_PROP: &str = "X-ZA-SEALED";
pub const KEY_PROP: &str = "X-ZA-KEY";
pub const EXTRA_PROP: &str = "X-ZA-EXTRA";
pub(crate) const FORMAT_V1: u8 = 1;
const PAD: usize = 256;

/// One component's removals. Entry indices refer to the original entry list;
/// parameter indices to the original parameter list of that entry.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct Removals {
    pub entries: Vec<RemovedEntry>,
    pub params: Vec<RemovedParam>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct RemovedEntry {
    pub index: u32,
    pub entry: ICalendarEntry,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
pub struct RemovedParam {
    pub entry_index: u32,
    pub param_index: u32,
    pub param: ICalendarParameter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// No key envelope on an object that must have one.
    NotSealed,
    /// Already sealed where plaintext was expected, or an unexpected layout.
    Structure(&'static str),
    Format,
    Decode,
    Aead,
    Policy(u8),
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealError::NotSealed => f.write_str("object is not sealed"),
            SealError::Structure(what) => write!(f, "unexpected structure: {what}"),
            SealError::Format => f.write_str("invalid bundle format"),
            SealError::Decode => f.write_str("invalid bundle encoding"),
            SealError::Aead => f.write_str("bundle authentication failed"),
            SealError::Policy(v) => write!(f, "unknown policy version {v}"),
        }
    }
}

impl std::error::Error for SealError {}

/// Spec 10: a logged error naming the object, never its content.
pub fn seal_error(err: SealError, account_id: u32, document_id: u32) -> trc::Error {
    trc::StoreEvent::DataCorruption
        .into_err()
        .details(format!("zero-access unseal failed: {err}"))
        .account_id(account_id)
        .document_id(document_id)
        .caused_by(trc::location!())
}

pub fn tree_aad(account_id: u32, scope: &str, component_index: usize) -> Vec<u8> {
    let mut out = aad("tree", account_id);
    out.push(b'|');
    out.extend_from_slice(scope.as_bytes());
    out.push(b'|');
    out.extend_from_slice(&(component_index as u32).to_be_bytes());
    out.push(b'|');
    out.push(POLICY_VERSION);
    out
}

pub fn is_carrier(entry: &ICalendarEntry, name: &str) -> bool {
    matches!(&entry.name, ICalendarProperty::Other(n) if n.eq_ignore_ascii_case(name))
}

pub fn text_entry(name: &str, text: String) -> ICalendarEntry {
    ICalendarEntry {
        name: ICalendarProperty::Other(name.to_string()),
        params: Vec::new(),
        values: vec![ICalendarValue::Text(text)],
    }
}

pub fn entry_text(entry: &ICalendarEntry) -> Option<&str> {
    match entry.values.first() {
        Some(ICalendarValue::Text(text)) => Some(text.as_str()),
        _ => None,
    }
}

pub fn tree_has_carriers(ical: &ICalendar) -> bool {
    ical.components.iter().any(|c| {
        c.entries.iter().any(|e| {
            matches!(&e.name, ICalendarProperty::Other(n) if n.len() > 5 && n[..5].eq_ignore_ascii_case("X-ZA-"))
        })
    })
}

/// Length prefix, zero padding to 256 bytes, XChaCha20-Poly1305, base64 of
/// `format || nonce || ciphertext`.
pub fn seal_bytes(dek: &Secret, aad: &[u8], plaintext: &[u8]) -> String {
    let mut padded = Vec::with_capacity(4 + plaintext.len() + PAD);
    padded.extend_from_slice(&(plaintext.len() as u32).to_le_bytes());
    padded.extend_from_slice(plaintext);
    let target = padded.len().div_ceil(PAD) * PAD;
    padded.resize(target, 0);
    let mut out = Vec::with_capacity(1 + padded.len() + 40);
    out.push(FORMAT_V1);
    out.extend_from_slice(&seal(dek, aad, &padded));
    STANDARD.encode(out)
}

pub fn open_bytes(dek: &Secret, aad: &[u8], text: &str) -> Result<Vec<u8>, SealError> {
    let raw = STANDARD.decode(text.trim()).map_err(|_| SealError::Decode)?;
    if raw.first() != Some(&FORMAT_V1) {
        return Err(SealError::Format);
    }
    let padded = open(dek, aad, &raw[1..]).map_err(|_| SealError::Aead)?;
    let len = padded
        .get(..4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        .ok_or(SealError::Format)?;
    padded
        .get(4..4 + len)
        .map(|s| s.to_vec())
        .ok_or(SealError::Format)
}

fn seal_component(component: &mut ICalendarComponent, dek: &Secret, aad: &[u8]) -> bool {
    let component_type = component.component_type.clone();
    let mut removals = Removals {
        entries: Vec::new(),
        params: Vec::new(),
    };
    let original = std::mem::take(&mut component.entries);
    let mut kept = Vec::with_capacity(original.len());
    for (index, entry) in original.into_iter().enumerate() {
        if !is_visible_property(&component_type, &entry.name) {
            removals.entries.push(RemovedEntry {
                index: index as u32,
                entry,
            });
            continue;
        }
        let mut visible = ICalendarEntry {
            name: entry.name,
            params: Vec::with_capacity(entry.params.len()),
            values: entry.values,
        };
        for (param_index, param) in entry.params.into_iter().enumerate() {
            if is_visible_parameter(&param.name) {
                visible.params.push(param);
            } else {
                removals.params.push(RemovedParam {
                    entry_index: index as u32,
                    param_index: param_index as u32,
                    param,
                });
            }
        }
        kept.push(visible);
    }
    component.entries = kept;
    if removals.entries.is_empty() && removals.params.is_empty() {
        return false;
    }
    let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&removals)
        .expect("rkyv serialization of in-memory removals cannot fail");
    component
        .entries
        .push(text_entry(SEALED_PROP, seal_bytes(dek, aad, &plain)));
    true
}

/// Seals every component in place. Components, their order and
/// `component_ids` are untouched (invariant 2). Returns true if any
/// property or parameter was sealed.
pub fn seal_tree(ical: &mut ICalendar, dek: &Secret, account_id: u32, scope: &str) -> bool {
    let mut any = false;
    for (index, component) in ical.components.iter_mut().enumerate() {
        any |= seal_component(component, dek, &tree_aad(account_id, scope, index));
    }
    any
}

fn unseal_component(
    component: &mut ICalendarComponent,
    dek: &Secret,
    aad: &[u8],
) -> Result<(), SealError> {
    if !component.entries.last().is_some_and(|e| is_carrier(e, SEALED_PROP)) {
        return Ok(());
    }
    let carrier = component.entries.pop().unwrap();
    let text = entry_text(&carrier).ok_or(SealError::Format)?;
    let plain = open_bytes(dek, aad, text)?;
    let mut removals = rkyv::from_bytes::<Removals, rkyv::rancor::Error>(&plain)
        .map_err(|_| SealError::Format)?;
    // Entries first, ascending: each index refers to the original list, so
    // inserting in ascending order restores every original position.
    removals.entries.sort_by_key(|r| r.index);
    for removed in removals.entries {
        let index = removed.index as usize;
        if index > component.entries.len() {
            return Err(SealError::Structure("entry index"));
        }
        component.entries.insert(index, removed.entry);
    }
    removals.params.sort_by_key(|r| (r.entry_index, r.param_index));
    for removed in removals.params {
        let entry = component
            .entries
            .get_mut(removed.entry_index as usize)
            .ok_or(SealError::Structure("parameter entry index"))?;
        let index = removed.param_index as usize;
        if index > entry.params.len() {
            return Err(SealError::Structure("parameter index"));
        }
        entry.params.insert(index, removed.param);
    }
    Ok(())
}

pub fn unseal_tree(
    ical: &mut ICalendar,
    dek: &Secret,
    account_id: u32,
    scope: &str,
) -> Result<(), SealError> {
    for (index, component) in ical.components.iter_mut().enumerate() {
        unseal_component(component, dek, &tree_aad(account_id, scope, index))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p groupware seal::tree 2>&1 | tail -12
git add crates/groupware
git commit -m "Seal and unseal iCalendar trees with per-component removal bundles"
```

---

### Task 3: Sealing events

**Files:**
- Create (fill): `crates/groupware/src/calendar/seal/event.rs`

**Interfaces:**
- Consumes: Task 2, `CalendarEvent`, `types::dead_property::DeadProperty`, `store::write::{Archive, AlignedBytes, Archiver}`.
- Produces: `seal_event(&mut CalendarEvent, &SessionKeys, account_id) -> Result<(), SealError>`, `unseal_event(&mut CalendarEvent, &SessionKeys, account_id) -> Result<(), SealError>`, `unseal_event_archive(&Archive<AlignedBytes>, &SessionKeys, account_id) -> Result<Archive<AlignedBytes>, SealError>` (the returned archive's `version` equals the stored one).

- [ ] **Step 1: Write the failing tests**

Bottom of `event.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CalendarEvent, CalendarEventData};
    use calcard::{Entry, Parser, common::timezone::Tz};
    use store::{Serialize, write::Archiver};
    use types::dead_property::{DeadElementTag, DeadPropertyTag};
    use vault::keys::Secret;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nX-WR-CALNAME:cal canary\r\nBEGIN:VEVENT\r\nUID:ev-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART;TZID=UTC:20240102T090000\r\nDTEND;TZID=UTC:20240102T100000\r\nSUMMARY:secret summary canary\r\nATTENDEE;CN=Bob:mailto:bob@example.com\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT5M\r\nSUMMARY:alarm canary\r\nATTENDEE:mailto:me@example.com\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn event() -> CalendarEvent {
        let ical = match Parser::new(ICS).entry() {
            Entry::ICalendar(ical) => ical,
            _ => panic!(),
        };
        let mut next = None;
        CalendarEvent {
            display_name: Some("display canary".into()),
            dead_properties: DeadProperty(vec![DeadPropertyTag::ElementStart(DeadElementTag::new("X:dead".into(), None)), DeadPropertyTag::Text("dead canary".into()), DeadPropertyTag::ElementEnd]),
            data: CalendarEventData::new(ical, Tz::Floating, 100, &mut next),
            size: ICS.len() as u32,
            ..Default::default()
        }
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(9, 1, Secret::random())
    }

    #[test]
    fn seal_then_unseal_restores_everything_and_keeps_precomputed_data() {
        let original = event();
        let keys = keys();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9).unwrap();
        assert!(sealed.display_name.is_none());
        assert!(sealed.dead_properties.0.is_empty());
        assert_eq!(sealed.data.time_ranges, original.data.time_ranges);
        assert_eq!(sealed.data.alarms, original.data.alarms, "email-alarm flag precomputed before sealing");
        assert_eq!(sealed.size, original.size);
        let root = &sealed.data.event.components[0];
        assert!(is_carrier(root.entries.last().unwrap(), KEY_PROP));
        assert!(is_carrier(&root.entries[root.entries.len() - 2], EXTRA_PROP));
        let dump = sealed.data.event.to_string();
        for canary in ["canary", "bob@example.com", "me@example.com"] {
            assert!(!dump.contains(canary), "{dump}");
        }
        let mut back = sealed.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn every_write_gets_a_new_dek() {
        let keys = keys();
        let mut a = event();
        let mut b = event();
        seal_event(&mut a, &keys, 9).unwrap();
        seal_event(&mut b, &keys, 9).unwrap();
        assert_ne!(a.data.event.components[0].entries.last(), b.data.event.components[0].entries.last());
    }

    #[test]
    fn wrong_account_wrong_keys_and_unsealed_input_fail() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9).unwrap();
        let mut t = sealed.clone();
        assert_eq!(unseal_event(&mut t, &keys, 10), Err(SealError::Aead));
        let mut t = sealed.clone();
        assert_eq!(unseal_event(&mut t, &SessionKeys::new(9, 1, Secret::random()), 9), Err(SealError::Aead));
        let mut plain = event();
        assert_eq!(unseal_event(&mut plain, &keys, 9), Err(SealError::NotSealed));
        let mut twice = sealed.clone();
        assert_eq!(seal_event(&mut twice, &keys, 9), Err(SealError::Structure("already sealed")));
    }

    #[test]
    fn archive_view_keeps_stored_version() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9).unwrap();
        let bytes = Archiver::new(sealed.clone()).serialize().unwrap();
        let stored = <Archive<AlignedBytes> as store::Deserialize>::deserialize(&bytes).unwrap();
        let view = unseal_event_archive(&stored, &keys, 9).unwrap();
        assert_eq!(view.version, stored.version, "response identity comes from the stored archive");
        let unarchived = view.unarchive::<CalendarEvent>().unwrap();
        assert_eq!(unarchived.display_name.as_deref(), Some("display canary"));
        assert!(unarchived.data.event.to_string().contains("secret summary canary"));
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("canary"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p groupware seal::event 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

Top of `event.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    policy::POLICY_VERSION,
    tree::{
        EXTRA_PROP, KEY_PROP, SealError, entry_text, is_carrier, open_bytes, seal_bytes,
        seal_tree, text_entry, unseal_tree,
    },
};
use crate::calendar::CalendarEvent;
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::icalendar::ICalendarComponentType;
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{Secret, aad, unwrap_key, wrap_key},
    session::SessionKeys,
};

/// Wrap type `mk` (spec 7.1). `0x02` is reserved for `pk`.
const WRAP_MK: u8 = 1;

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
struct Extra {
    display_name: Option<String>,
    dead_properties: DeadProperty,
}

fn key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad("event-key", account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

fn extra_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad("event-extra", account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

fn uid_of(event: &CalendarEvent) -> String {
    event.data.event.uids().next().unwrap_or_default().to_string()
}

/// Seals an event immediately before the store write (spec 8.1). Time
/// ranges and alarms were computed on the plaintext tree by the caller.
pub fn seal_event(event: &mut CalendarEvent, keys: &SessionKeys, account_id: u32) -> Result<(), SealError> {
    let root = event
        .data
        .event
        .components
        .first()
        .ok_or(SealError::Structure("empty tree"))?;
    if root.component_type != ICalendarComponentType::VCalendar {
        return Err(SealError::Structure("root is not VCALENDAR"));
    }
    if root.entries.last().is_some_and(|e| is_carrier(e, KEY_PROP)) {
        return Err(SealError::Structure("already sealed"));
    }
    let uid = uid_of(event);
    let dek = Secret::random();
    seal_tree(&mut event.data.event, &dek, account_id, &uid);

    if event.display_name.is_some() || !event.dead_properties.0.is_empty() {
        let extra = Extra {
            display_name: event.display_name.take(),
            dead_properties: std::mem::replace(&mut event.dead_properties, DeadProperty(Vec::new())),
        };
        let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
            .expect("rkyv serialization of in-memory extra cannot fail");
        event.data.event.components[0].entries.push(text_entry(
            EXTRA_PROP,
            seal_bytes(&dek, &extra_aad(account_id, &uid), &plain),
        ));
    }

    let mut envelope = vec![POLICY_VERSION, WRAP_MK];
    envelope.extend_from_slice(&wrap_key(&dek, keys.ewk(), &key_aad(account_id, &uid)));
    event.data.event.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

pub fn unseal_event(event: &mut CalendarEvent, keys: &SessionKeys, account_id: u32) -> Result<(), SealError> {
    let uid = uid_of(event);
    let root = event
        .data
        .event
        .components
        .first_mut()
        .ok_or(SealError::Structure("empty tree"))?;
    if !root.entries.last().is_some_and(|e| is_carrier(e, KEY_PROP)) {
        return Err(SealError::NotSealed);
    }
    let key_entry = root.entries.pop().unwrap();
    let envelope = STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)?;
    let dek = match envelope.as_slice() {
        [POLICY_VERSION, WRAP_MK, wrapped @ ..] => {
            unwrap_key(wrapped, keys.ewk(), &key_aad(account_id, &uid)).map_err(|_| SealError::Aead)?
        }
        [version, ..] if *version != POLICY_VERSION => return Err(SealError::Policy(*version)),
        _ => return Err(SealError::Format),
    };
    let extra = if root.entries.last().is_some_and(|e| is_carrier(e, EXTRA_PROP)) {
        root.entries.pop()
    } else {
        None
    };
    if let Some(extra) = extra {
        let plain = open_bytes(
            &dek,
            &extra_aad(account_id, &uid),
            entry_text(&extra).ok_or(SealError::Format)?,
        )?;
        let extra = rkyv::from_bytes::<Extra, rkyv::rancor::Error>(&plain)
            .map_err(|_| SealError::Format)?;
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)
}

/// Spec 7.3: a read view of the content. `version` is copied from the
/// stored archive so ETags and conditional headers stay bound to it.
pub fn unseal_event_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    unseal_event(&mut event, keys, account_id)?;
    let bytes = Archiver::new(event)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p groupware seal::event 2>&1 | tail -8
git add crates/groupware
git commit -m "Seal calendar events with a per-event key envelope and extra-field bundle"
```

---

### Task 4: Sealing calendar collections and custom timezones

**Files:**
- Create (fill): `crates/groupware/src/calendar/seal/collection.rs`

**Interfaces:**
- Consumes: Task 2, `Calendar`, `CalendarPreferences`, `Timezone`.
- Produces: `COLLECTION_MARKER: &str = "$za$"`, `seal_calendar(&mut Calendar, &SessionKeys, account_id) -> Result<(), SealError>`, `unseal_calendar(&mut Calendar, &SessionKeys, account_id) -> Result<(), SealError>` (a plaintext collection passes through unchanged), `unseal_calendar_archive(&Archive<AlignedBytes>, &SessionKeys, account_id) -> Result<Archive<AlignedBytes>, SealError>`, `calendar_is_sealed(&Calendar, account_id) -> bool`.

- [ ] **Step 1: Write the failing tests**

Bottom of `collection.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{Calendar, CalendarPreferences, Timezone};
    use calcard::{Entry, Parser};
    use store::{Serialize, write::Archiver};
    use types::dead_property::{DeadElementTag, DeadPropertyTag};
    use vault::keys::Secret;

    const TZ: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Example Corp.//CalDAV Client//EN\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:US-Eastern\r\nLAST-MODIFIED:19870101T000000Z\r\nX-LIC-LOCATION:America/New_York\r\nBEGIN:STANDARD\r\nDTSTART:19671029T020000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nTZNAME:Eastern Standard canary\r\nCOMMENT:tz comment canary\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n";

    fn tz() -> calcard::icalendar::ICalendar {
        match Parser::new(TZ).entry() {
            Entry::ICalendar(ical) => ical,
            _ => panic!(),
        }
    }

    fn calendar() -> Calendar {
        Calendar {
            name: "work-slug".into(),
            preferences: vec![CalendarPreferences {
                account_id: 4,
                name: "Work canary".into(),
                description: Some("desc canary".into()),
                color: Some("#ff0000".into()),
                sort_order: 3,
                flags: 1,
                time_zone: Timezone::Custom(tz()),
                ..Default::default()
            }],
            dead_properties: DeadProperty(vec![DeadPropertyTag::ElementStart(DeadElementTag::new("A:calendar-color".into(), None)), DeadPropertyTag::Text("#00ff00 canary".into()), DeadPropertyTag::ElementEnd]),
            ..Default::default()
        }
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(4, 1, Secret::random())
    }

    #[test]
    fn seal_unseal_round_trip_with_custom_timezone() {
        let original = calendar();
        let keys = keys();
        let mut sealed = original.clone();
        seal_calendar(&mut sealed, &keys, 4).unwrap();
        assert!(calendar_is_sealed(&sealed, 4));
        assert_eq!(sealed.name, "work-slug", "slug stays visible");
        let pref = sealed.preferences(4);
        assert!(pref.name.starts_with(COLLECTION_MARKER));
        assert!(pref.description.is_none());
        assert!(pref.color.is_none());
        assert_eq!(pref.sort_order, 3);
        assert_eq!(pref.flags, 1);
        assert!(sealed.dead_properties.0.is_empty());
        let Timezone::Custom(tz) = &pref.time_zone else { panic!() };
        let dump = tz.to_string();
        assert!(dump.contains("TZID:US-Eastern") && dump.contains("TZOFFSETFROM:-0400"), "{dump}");
        assert!(!dump.contains("canary") && !dump.contains("X-LIC-LOCATION"), "{dump}");
        assert_eq!(pref.time_zone.tz(), original.preferences(4).time_zone.tz(), "timezone resolution works without a key");
        let mut back = sealed.clone();
        unseal_calendar(&mut back, &keys, 4).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn plaintext_collection_passes_through() {
        let mut plain = calendar();
        plain.preferences[0].time_zone = Timezone::Default;
        let before = plain.clone();
        unseal_calendar(&mut plain, &keys(), 4).unwrap();
        assert_eq!(plain, before);
        assert!(!calendar_is_sealed(&plain, 4));
    }

    #[test]
    fn timezone_only_collection_still_carries_the_bundle() {
        let keys = keys();
        let mut c = calendar();
        c.preferences[0].description = None;
        c.preferences[0].color = None;
        c.dead_properties = DeadProperty(Vec::new());
        let original = c.clone();
        seal_calendar(&mut c, &keys, 4).unwrap();
        assert!(c.preferences(4).name.starts_with(COLLECTION_MARKER));
        unseal_calendar(&mut c, &keys, 4).unwrap();
        assert_eq!(c, original);
    }

    #[test]
    fn errors() {
        let keys = keys();
        let mut sealed = calendar();
        seal_calendar(&mut sealed, &keys, 4).unwrap();
        assert_eq!(seal_calendar(&mut sealed.clone(), &keys, 4), Err(SealError::Structure("already sealed")));
        assert_eq!(unseal_calendar(&mut sealed.clone(), &keys, 5), Err(SealError::Aead));
        assert_eq!(unseal_calendar(&mut sealed.clone(), &SessionKeys::new(4, 1, Secret::random()), 4), Err(SealError::Aead));
        // Copying to a new document id needs no resealing: the AAD binds the account only.
        let bytes = Archiver::new(sealed.clone()).serialize().unwrap();
        let stored = <Archive<AlignedBytes> as store::Deserialize>::deserialize(&bytes).unwrap();
        let view = unseal_calendar_archive(&stored, &keys, 4).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(view.unarchive::<Calendar>().unwrap().preferences(4).name, "Work canary");
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p groupware seal::collection 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

Top of `collection.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    policy::POLICY_VERSION,
    tree::{SealError, open_bytes, seal_bytes, seal_tree, unseal_tree},
};
use crate::calendar::{Calendar, Timezone};
use base64::{Engine, engine::general_purpose::STANDARD};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{Secret, aad, unwrap_key, wrap_key},
    session::SessionKeys,
};

/// Prefix of the owner's `CalendarPreferences.name` when sealed (spec 7.2).
pub const COLLECTION_MARKER: &str = "$za$";
const TZ_SCOPE: &str = "calendar-tz";

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
struct Bundle {
    name: String,
    description: Option<String>,
    color: Option<String>,
    dead_properties: DeadProperty,
}

pub fn calendar_is_sealed(calendar: &Calendar, account_id: u32) -> bool {
    calendar.preferences(account_id).name.starts_with(COLLECTION_MARKER)
}

/// Seals name, description, colour, dead properties and a custom timezone
/// into the owner's preferences entry. The bundle is always written for a
/// key account's collection, so the wrapped key is present whenever any
/// ciphertext in the collection depends on it (spec 7.2).
pub fn seal_calendar(calendar: &mut Calendar, keys: &SessionKeys, account_id: u32) -> Result<(), SealError> {
    if calendar_is_sealed(calendar, account_id) {
        return Err(SealError::Structure("already sealed"));
    }
    let dek = Secret::random();
    let dead_properties = std::mem::replace(&mut calendar.dead_properties, DeadProperty(Vec::new()));
    let pref = calendar.preferences_mut(account_id);
    let bundle = Bundle {
        name: std::mem::take(&mut pref.name),
        description: pref.description.take(),
        color: pref.color.take(),
        dead_properties,
    };
    if let Timezone::Custom(tz) = &mut pref.time_zone {
        seal_tree(tz, &dek, account_id, TZ_SCOPE);
    }
    let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&bundle)
        .expect("rkyv serialization of in-memory bundle cannot fail");
    let mut envelope = vec![POLICY_VERSION];
    envelope.extend_from_slice(&wrap_key(&dek, keys.ewk(), &aad("calendar-key", account_id)));
    pref.name = format!(
        "{COLLECTION_MARKER}{}|{}",
        STANDARD.encode(envelope),
        seal_bytes(&dek, &aad("calendar-bundle", account_id), &plain)
    );
    Ok(())
}

/// Restores the owner's entry. A collection without the marker (for example
/// the server-created default calendar) is left unchanged.
pub fn unseal_calendar(calendar: &mut Calendar, keys: &SessionKeys, account_id: u32) -> Result<(), SealError> {
    let pref = calendar.preferences_mut(account_id);
    let Some(rest) = pref.name.strip_prefix(COLLECTION_MARKER) else {
        return Ok(());
    };
    let (envelope, sealed) = rest.split_once('|').ok_or(SealError::Format)?;
    let envelope = STANDARD.decode(envelope).map_err(|_| SealError::Decode)?;
    let dek = match envelope.as_slice() {
        [POLICY_VERSION, wrapped @ ..] => {
            unwrap_key(wrapped, keys.ewk(), &aad("calendar-key", account_id)).map_err(|_| SealError::Aead)?
        }
        [version, ..] => return Err(SealError::Policy(*version)),
        [] => return Err(SealError::Format),
    };
    let plain = open_bytes(&dek, &aad("calendar-bundle", account_id), sealed)?;
    let bundle = rkyv::from_bytes::<Bundle, rkyv::rancor::Error>(&plain)
        .map_err(|_| SealError::Format)?;
    pref.name = bundle.name;
    pref.description = bundle.description;
    pref.color = bundle.color;
    if let Timezone::Custom(tz) = &mut pref.time_zone {
        unseal_tree(tz, &dek, account_id, TZ_SCOPE)?;
    }
    calendar.dead_properties = bundle.dead_properties;
    Ok(())
}

pub fn unseal_calendar_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut calendar = stored
        .deserialize::<Calendar>()
        .map_err(|_| SealError::Format)?;
    unseal_calendar(&mut calendar, keys, account_id)?;
    let bytes = Archiver::new(calendar)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p groupware seal 2>&1 | tail -8
git add crates/groupware
git commit -m "Seal calendar collections and custom timezones into the owner's preferences"
```

---

### Task 5: The DAV gate and session-key plumbing

**Files:**
- Create: `crates/dav/src/common/za.rs`
- Modify: `crates/dav/src/common/mod.rs` (`pub mod za;`)
- Modify: `crates/dav/src/common/uri.rs` (gate in `validate_uri_with_status`)
- Modify: `crates/dav/src/calendar/copy_move.rs` (cross-account refusal)
- Modify: `crates/dav/Cargo.toml` (add `vault`)
- Create: `tests/src/za/dav_gate.rs`
- Modify: `tests/src/za/mod.rs`

**Interfaces:**
- Produces: `dav::common::za::ZeroAccessGate` with `Server::za_session_keys(&self, access_token: &AccessToken, account_id: u32) -> crate::Result<Option<Arc<SessionKeys>>>` (`None` for non-key accounts; `Err(403)` for a key account without that account's keys). Every calendar URI of a key account is refused unless the session holds that account's keys.

- [ ] **Step 1: Write the failing tests**

`tests/src/za/dav_gate.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use hyper::StatusCode;
use registry::schema::structs;
use serde_json::json;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access DAV gate tests...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").clone();
    let key1_id = key1.id().document_id();
    let plain = test.account("plain@example.com").clone();

    // Owner with keys: allowed.
    DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com")
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);

    // Master-user login (admin impersonating key1): token has no keys -> 403.
    let master = Box::leak(format!("key1@example.com%{}", admin.name()).into_boxed_str());
    DummyWebDavClient::new(key1_id, master, admin.secret(), "key1@example.com")
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Admin with Impersonate permission addressing the account directly: 403.
    DummyWebDavClient::new(key1_id, admin.name(), admin.secret(), admin.name())
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    DummyWebDavClient::new(key1_id, admin.name(), admin.secret(), admin.name())
        .request("PROPFIND", "/dav/itip/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Address book and principal paths are not gated.
    DummyWebDavClient::new(key1_id, admin.name(), admin.secret(), admin.name())
        .request("PROPFIND", "/dav/card/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Another user without any grant: 403 (as upstream), unchanged.
    DummyWebDavClient::new(key1_id, plain.name(), plain.secret(), plain.name())
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);

    // Bearer with an API key of the key account itself: no keys -> 403 on calendar paths.
    let api_key = admin
        .jmap_create_account(&key1, "x:ApiKey", [json!({ "description": "no keys", "permissions": {} })], Vec::<(&str, &str)>::new())
        .await
        .created(0)["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let response = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), "https://127.0.0.1:8899/dav/cal/key1@example.com/")
        .bearer_auth(&api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);

    // Cross-account COPY where either side is a key account: 403 (both directions).
    let plain_client = plain.webdav_client();
    plain_client
        .request_with_headers("PUT", "/dav/cal/plain@example.com/default/x.ics", [("content-type", "text/calendar")], crate::webdav::TEST_ICAL_1)
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .request_with_headers("COPY", "/dav/cal/plain@example.com/default/x.ics", [("destination", "/dav/cal/key1@example.com/default/x.ics")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    let key_client = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    key_client
        .request_with_headers("PUT", "/dav/cal/key1@example.com/default/y.ics", [("content-type", "text/calendar")], crate::webdav::TEST_ICAL_1)
        .await
        .with_status(StatusCode::CREATED);
    key_client
        .request_with_headers("COPY", "/dav/cal/key1@example.com/default/y.ics", [("destination", "/dav/cal/plain@example.com/default/y.ics")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    key_client
        .request("DELETE", "/dav/cal/key1@example.com/default/y.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", "/dav/cal/plain@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let _ = structs::Account::default();
}
```

Register `pub mod dav_gate;` and call `dav_gate::test(&mut test).await;` after `caches::test` in `tests/src/za/mod.rs`. (The admin API-key creation shape is from `tests/src/scim/mod.rs:212`; the cross-account COPY to another account is refused at the destination gate even before the explicit check, because the admin's or plain user's token holds no keys for the key account, and the key user's token holds no `access_to` grant on `plain`. The explicit check in step 3 covers the case where a grant exists, which plan 3 forbids for key owners but not for a key account that was granted access by a non-key account.)

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: the master-user PROPFIND returns 207 (fails the 403 assertion).

- [ ] **Step 3: Implement the gate**

`crates/dav/src/common/za.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::DavError;
use common::{Server, auth::AccessToken};
use hyper::StatusCode;
use std::{future::Future, sync::Arc};
use trc::AddContext;
use vault::session::SessionKeys;

pub(crate) trait ZeroAccessGate: Sync + Send {
    /// `None` for a non-key account (unchanged upstream path). For a key
    /// account, the keys of that account held by this session, or 403.
    fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<Option<Arc<SessionKeys>>>> + Send;
}

impl ZeroAccessGate for Server {
    async fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<Option<Arc<SessionKeys>>> {
        let account = self.account(account_id).await.caused_by(trc::location!())?;
        if !account.is_key_account() {
            return Ok(None);
        }
        match access_token.za_keys_for(account_id) {
            Some(keys) => Ok(Some(keys.clone())),
            None => {
                trc::event!(
                    Security(trc::SecurityEvent::Unauthorized),
                    AccountId = account_id,
                    Details = "zero-access: calendar access without session keys",
                );
                Err(DavError::Code(StatusCode::FORBIDDEN))
            }
        }
    }
}
```

Add `pub mod za;` to `crates/dav/src/common/mod.rs` and `vault = { path = "../vault" }` to `crates/dav/Cargo.toml`.

`crates/dav/src/common/uri.rs`, in `validate_uri_with_status`, right after the existing `// Validate access` block (before `resource.account_id = Some(account_id);`):

```rust
            // Zero-access gate (spec 9): a key account's calendar and
            // scheduling data is reachable only with that account's keys.
            if matches!(
                resource.collection,
                Collection::Calendar | Collection::CalendarEventNotification
            ) {
                self.za_session_keys(access_token, account_id).await?;
            }
```

(import `crate::common::za::ZeroAccessGate`).

`crates/dav/src/calendar/copy_move.rs`, after `let to_account_id = destination.account_id.ok_or(..)?;`:

```rust
        // Spec 8.1: across accounts, refused when either side is a key account.
        if to_account_id != from_account_id
            && (self
                .account(from_account_id)
                .await
                .caused_by(trc::location!())?
                .is_key_account()
                || self
                    .account(to_account_id)
                    .await
                    .caused_by(trc::location!())?
                    .is_key_account())
        {
            return Err(DavError::Code(StatusCode::FORBIDDEN));
        }
```

- [ ] **Step 4: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
git add crates/dav tests/src/za
git commit -m "Gate key-account calendar paths on resident session keys"
```

---

### Task 6: PUT and GET

**Files:**
- Modify: `crates/dav/src/calendar/update.rs`
- Modify: `crates/dav/src/calendar/get.rs`
- Create: `tests/src/za/dav_seal.rs`
- Modify: `tests/src/za/mod.rs`

**Interfaces:**
- Consumes: Tasks 3, 5.
- Produces: events of key accounts are stored sealed on PUT and returned unsealed on GET/HEAD.

- [ ] **Step 1: Write the failing tests**

`tests/src/za/dav_seal.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use calcard::{Entry, Parser};
use groupware::{cache::GroupwareCache, calendar::{Calendar, CalendarEvent}};
use hyper::StatusCode;
use store::{ValueKey, write::{AlignedBytes, Archive}};
use types::collection::{Collection, SyncCollection};

pub const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nX-WR-CALNAME:calname-canary\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19961027T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:tzname-canary\r\nCOMMENT:tzcomment-canary\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:za-event-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART;TZID=Europe/Berlin;X-PARAM=param-canary:20240102T090000\r\nDTEND;TZID=Europe/Berlin:20240102T100000\r\nSUMMARY:summary-canary\r\nDESCRIPTION:description-canary\r\nLOCATION:location-canary\r\nATTENDEE;CN=attendee-canary:mailto:attendee-canary@example.com\r\nX-CUSTOM:xprop-canary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub const CANARIES: &[&str] = &["calname-canary", "tzname-canary", "tzcomment-canary", "param-canary", "summary-canary", "description-canary", "location-canary", "attendee-canary", "xprop-canary"];

pub async fn raw_event(test: &TestServer, account_id: u32, path: &str) -> (Archive<AlignedBytes>, u32) {
    let resources = test.server.fetch_dav_resources(account_id, account_id, SyncCollection::Calendar).await.unwrap();
    let resource = resources.by_path(path).unwrap_or_else(|| panic!("{path} not found"));
    let document_id = resource.document_id();
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(account_id, Collection::CalendarEvent, document_id))
        .await
        .unwrap()
        .expect("event archive");
    (archive, document_id)
}

pub async fn raw_calendar(test: &TestServer, account_id: u32, path: &str) -> Archive<AlignedBytes> {
    let resources = test.server.fetch_dav_resources(account_id, account_id, SyncCollection::Calendar).await.unwrap();
    let resource = resources.by_path(path).unwrap_or_else(|| panic!("{path} not found"));
    test.server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(account_id, Collection::Calendar, resource.document_id()))
        .await
        .unwrap()
        .expect("calendar archive")
}

fn parse(text: &str) -> calcard::icalendar::ICalendar {
    match Parser::new(text).entry() {
        Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access PUT/GET sealing tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/sealed-1.ics";

    let created = client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    let etag1 = created.etag().to_string();

    // Stored record: no canary, carriers present, size is the body length.
    let (archive, _) = raw_event(test, id, "default/sealed-1.ics").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in CANARIES {
        assert!(!raw.contains(canary), "{canary} leaked into the stored event");
    }
    let stored = archive.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(stored.size.to_native() as usize, EVENT.len());
    let stored_text = stored.data.event.to_string();
    assert!(stored_text.contains("X-ZA-KEY:") && stored_text.contains("X-ZA-SEALED:"), "{stored_text}");
    assert!(stored_text.contains("TZID:Europe/Berlin") && stored_text.contains("DTSTART;TZID=Europe/Berlin:20240102T090000"), "visible fields stay visible: {stored_text}");

    // GET: byte-faithful tree, no carrier, same ETag as the PUT.
    let got = client.request("GET", path, "").await.with_status(StatusCode::OK);
    assert_eq!(got.etag(), etag1);
    let body = got.body.clone().unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    for canary in CANARIES {
        assert!(body.contains(canary), "{canary} missing from GET body");
    }
    assert_eq!(parse(&body), parse(EVENT), "GET returns the original tree entry for entry");
    client.request("HEAD", path, "").await.with_status(StatusCode::OK).with_header("content-length", &EVENT.len().to_string());

    // Unchanged PUT hits the no-change shortcut (compared against the unsealed view).
    client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], EVENT)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (again, _) = raw_event(test, id, "default/sealed-1.ics").await;
    assert_eq!(again.version, archive.version, "no rewrite on an unchanged PUT");

    // A sealed-only change (SUMMARY) is a real change: new DEK, new ETag.
    let changed = EVENT.replace("summary-canary", "summary-canary-2");
    let updated = client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], changed.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_ne!(updated.etag(), etag1);
    let (after, _) = raw_event(test, id, "default/sealed-1.ics").await;
    assert_ne!(after.version, archive.version);
    assert_ne!(after.unarchive::<CalendarEvent>().unwrap().data.event.components[0].entries.last().unwrap().values, stored.data.event.components[0].entries.last().unwrap().values, "fresh key envelope");
    let body = client.request("GET", path, "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(body.contains("summary-canary-2"));

    // If-Match works against the stored ETag.
    client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8"), ("if-match", &etag1)], EVENT)
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);

    client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);

    // Repeated PUT and PROPPATCH followed by DELETE: quota back at baseline.
    let baseline = client.available_quota().await;
    for _ in 0..3 {
        client
            .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], EVENT)
            .await;
        client
            .proppatch(path, [("D:displayname", "quota-canary")], [])
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }
    assert!(client.available_quota().await < baseline, "sealed events still count against quota");
    client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);
    assert_eq!(client.available_quota().await, baseline, "quota returns to baseline after DELETE");
    let _ = Calendar::default();
}
```

Register `pub mod dav_seal;` and call it after `dav_gate::test`.

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: FAIL at "leaked into the stored event".

- [ ] **Step 3: Implement PUT**

`crates/dav/src/calendar/update.rs`. Imports: `use crate::common::za::ZeroAccessGate; use groupware::calendar::seal::{seal_error, seal_event, unseal_event_archive};`.

1. After `let account_id = resource.account_id;` add:

```rust
        let za_keys = self.za_session_keys(access_token, account_id).await?;
```

2. In the update branch, right after `let event = event_.to_unarchived::<CalendarEvent>().caused_by(trc::location!())?;` add the unsealed view and use it for every content read:

```rust
            // Spec 8.1: the stored sealed archive stays `current` for the
            // index builder; the unsealed view is used for comparison, editing
            // and response bodies only.
            let view_;
            let view = if let Some(keys) = &za_keys {
                view_ = unseal_event_archive(&event_, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                view_.to_unarchived::<CalendarEvent>().caused_by(trc::location!())?
            } else {
                event_.to_unarchived::<CalendarEvent>().caused_by(trc::location!())?
            };
```

   Then change these reads from `event` to `view`: the 412 representation body `.with_binary_body(event.inner.data.event.to_string())` → `view.inner.data.event.to_string()`; the no-change check `if ical == event.inner.data.event` → `if ical == view.inner.data.event`; `let mut new_event = event.deserialize::<CalendarEvent>()` → `view.deserialize::<CalendarEvent>()`. Leave `event.etag()`, `event.inner.modified`, `event.inner.schedule_tag`, `event.inner.data.next_alarm(..)`, `u32::from(event.inner.size)` and the `.update(.., event, ..)` call on the stored `event`.

3. In the update branch, immediately before `// Prepare write batch` (after the quota check) add:

```rust
            if let Some(keys) = &za_keys {
                seal_event(&mut new_event, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
            }
```

4. In the create branch, immediately before `// Prepare write batch` add:

```rust
            if let Some(keys) = &za_keys {
                seal_event(&mut event, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, u32::MAX))?;
            }
```

   (`CalendarEventData::new` and the alarm computation already ran on the plaintext tree above, which is what spec 8.1 requires.)

- [ ] **Step 4: Implement GET/HEAD**

`crates/dav/src/calendar/get.rs`: imports as above. After `let account_id = resource.account_id;` add `let za_keys = self.za_session_keys(access_token, account_id).await?;`. Replace the two lines `let event = event_.unarchive::<CalendarEvent>().caused_by(trc::location!())?;` with:

```rust
        let etag = event_.etag();
        let view_;
        let event = if let Some(keys) = &za_keys {
            view_ = unseal_event_archive(&event_, keys, account_id)
                .map_err(|err| seal_error(err, account_id, resource.document_id()))?;
            view_.unarchive::<CalendarEvent>().caused_by(trc::location!())?
        } else {
            event_.unarchive::<CalendarEvent>().caused_by(trc::location!())?
        };
```

and delete the later `let etag = event_.etag();` line (it is now above). `event.schedule_tag`, `event.modified` and `event.data.event.to_string()` keep working on the view, and the ETag comes from the stored archive.

- [ ] **Step 5: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
git add crates/dav tests/src/za
git commit -m "Seal events on PUT and unseal them on GET for key accounts"
```

---

### Task 7: PROPFIND, REPORT (query, multiget, sync, expand) and free-busy

**Files:**
- Modify: `crates/dav/src/common/propfind.rs`
- Modify: `crates/dav/src/calendar/freebusy.rs`
- Modify: `tests/src/za/dav_seal.rs`

**Interfaces:**
- Consumes: Tasks 3, 4, 5.
- Produces: every response carrying `calendar-data`, display names, descriptions or `calendar-timezone` for a key account is unsealed; query filters run on unsealed candidates; an unseal failure fails that one item with 500.

- [ ] **Step 1: Extend the tests**

Append to `tests/src/za/dav_seal.rs` a second function and call it from `mod.rs` after `test`:

```rust
pub async fn test_reports(test: &mut TestServer) {
    println!("Running zero-access report tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/report-1.ics";
    client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], EVENT)
        .await
        .with_status(StatusCode::CREATED);

    // PROPFIND with calendar-data on the collection.
    let response = client
        .request_with_headers(
            "PROPFIND",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let body = response.body.clone().unwrap();
    assert!(body.contains("summary-canary") && !body.contains("X-ZA-"), "{body}");

    // calendar-query with a text match on a sealed property.
    let query = "<?xml version=\"1.0\"?><A:calendar-query xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><A:calendar-data/></D:prop><A:filter><A:comp-filter name=\"VCALENDAR\"><A:comp-filter name=\"VEVENT\"><A:prop-filter name=\"SUMMARY\"><A:text-match>summary-canary</A:text-match></A:prop-filter></A:comp-filter></A:comp-filter></A:filter></A:calendar-query>";
    let body = client
        .request_with_headers("REPORT", "/dav/cal/key1@example.com/default/", [("depth", "1")], query)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(body.contains("report-1.ics") && body.contains("location-canary") && !body.contains("X-ZA-"), "{body}");
    let miss = query.replace("summary-canary", "no-such-summary");
    let body = client
        .request_with_headers("REPORT", "/dav/cal/key1@example.com/default/", [("depth", "1")], miss)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(!body.contains("report-1.ics"), "{body}");

    // multiget and sync-collection.
    let body = client
        .multiget_calendar("/dav/cal/key1@example.com/default/", [path])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(body.contains("description-canary") && !body.contains("X-ZA-"), "{body}");
    let body = client
        .sync_collection("/dav/cal/key1@example.com/default/", "", crate::utils::webdav::Depth::One, None, ["A:calendar-data"])
        .await
        .body
        .unwrap();
    assert!(body.contains("summary-canary") && !body.contains("X-ZA-"), "{body}");

    // free-busy by the owner.
    let fb = "<?xml version=\"1.0\"?><A:free-busy-query xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><A:time-range start=\"20240101T000000Z\" end=\"20240103T000000Z\"/></A:free-busy-query>";
    let body = client
        .request("REPORT", "/dav/cal/key1@example.com/default/", fb)
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("FREEBUSY") && body.contains("20240102T080000Z/20240102T090000Z"), "{body}");

    // A corrupted stored record fails that one item with 500 and leaves the rest readable.
    let (archive, document_id) = raw_event(test, id, "default/report-1.ics").await;
    let mut broken = archive.deserialize::<CalendarEvent>().unwrap();
    let root = &mut broken.data.event.components[0];
    let last = root.entries.last_mut().unwrap();
    last.values = vec![calcard::icalendar::ICalendarValue::Text("AQI=".into())];
    let account_info = test.server.account_info(id).await.unwrap();
    let mut batch = store::write::BatchBuilder::new();
    broken
        .update(account_info.account_tenant_ids(), archive.to_unarchived::<CalendarEvent>().unwrap(), id, document_id, &mut batch)
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    client.request("GET", path, "").await.with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body = client
        .multiget_calendar("/dav/cal/key1@example.com/default/", [path])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(body.contains("HTTP/1.1 500"), "{body}");
    client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);
}
```

(`multiget_calendar(path, hrefs)` and `Depth` exist in `tests/src/utils/webdav.rs`; match their exact signatures.)

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: FAIL at the PROPFIND `calendar-data` assertion (body contains `X-ZA-`).

- [ ] **Step 3: Implement the central loader**

`crates/dav/src/common/propfind.rs`, in `handle_dav_query`, replace the archive load (lines ~438-460) with:

```rust
            // Unarchive resource
            let archive_;
            let archive = if is_scheduling && item.is_container {
                archive_ = Archive::default();
                ArchivedResource::CalendarEventNotificationCollection(
                    item.document_id == SCHEDULE_INBOX_ID,
                )
            } else if let Some(stored) = self
                .store()
                .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                    account_id,
                    collection,
                    document_id,
                ))
                .await
                .caused_by(trc::location!())?
            {
                // Spec 8.2: unseal before any use of the tree. The view keeps
                // the stored version, so `archive_.etag()` below is unchanged.
                archive_ = match (
                    self.za_session_keys(access_token, account_id).await?,
                    collection,
                ) {
                    (Some(keys), Collection::CalendarEvent) => {
                        match unseal_event_archive(&stored, &keys, account_id) {
                            Ok(view) => view,
                            Err(err) => {
                                trc::error!(seal_error(err, account_id, document_id));
                                response.add_response(Response::new_status(
                                    [item.name],
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                ));
                                continue;
                            }
                        }
                    }
                    (Some(keys), Collection::Calendar) => {
                        match unseal_calendar_archive(&stored, &keys, account_id) {
                            Ok(view) => view,
                            Err(err) => {
                                trc::error!(seal_error(err, account_id, document_id));
                                response.add_response(Response::new_status(
                                    [item.name],
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                ));
                                continue;
                            }
                        }
                    }
                    _ => stored,
                };
                ArchivedResource::from_archive(&archive_, collection).caused_by(trc::location!())?
            } else {
                response.add_response(Response::new_status([item.name], StatusCode::NOT_FOUND));
                continue;
            };
```

(imports: `crate::common::za::ZeroAccessGate`, `groupware::calendar::seal::{seal_error, unseal_calendar_archive, unseal_event_archive}`. `item.name` may be moved by the earlier `continue` arms in the original code; keep the same ownership pattern they use.) The ETag at line ~552 (`archive_.etag()`) now reads the copied version, so sync and conditional metadata stay bound to the stored bytes (invariant 5).

- [ ] **Step 4: Free-busy (owner's own calendars)**

`crates/dav/src/calendar/freebusy.rs`, inside `build_freebusy_object` after the account id is known, add `let za_keys = self.za_session_keys(access_token, account_id).await?;` before the `for document_id in document_ids` loop, and replace `let event = archive.unarchive::<CalendarEvent>().caused_by(trc::location!())?;` with:

```rust
                let view_;
                let event = if let Some(keys) = &za_keys {
                    view_ = unseal_event_archive(&archive, keys, account_id)
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                    view_.unarchive::<CalendarEvent>().caused_by(trc::location!())?
                } else {
                    archive.unarchive::<CalendarEvent>().caused_by(trc::location!())?
                };
```

- [ ] **Step 5: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
git add crates/dav tests/src/za
git commit -m "Unseal key-account events and collections on every DAV report path"
```

---

### Task 8: MKCALENDAR, PROPPATCH and collection properties

**Files:**
- Modify: `crates/dav/src/calendar/mkcol.rs`
- Modify: `crates/dav/src/calendar/proppatch.rs`
- Modify: `tests/src/za/dav_seal.rs`

**Interfaces:**
- Consumes: Tasks 3, 4, 5.
- Produces: collections of key accounts are stored with a sealed bundle (name, description, colour dead property, custom timezone); event PROPPATCH seals display name and dead properties.

- [ ] **Step 1: Extend the tests**

Append to `tests/src/za/dav_seal.rs` and call after `test_reports`:

```rust
pub async fn test_collections(test: &mut TestServer) {
    println!("Running zero-access collection sealing tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let cal = "/dav/cal/key1@example.com/work/";

    client
        .mkcol(
            "MKCALENDAR",
            cal,
            [],
            [
                ("D:displayname", "Work displayname-canary"),
                ("A:calendar-description", "coldesc-canary"),
                ("C:calendar-color", "#aabbcc-canary"),
            ],
        )
        .await
        .with_status(StatusCode::CREATED);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in ["displayname-canary", "coldesc-canary", "#aabbcc-canary"] {
        assert!(!raw.contains(canary), "{canary} leaked into the stored collection");
    }
    let stored = archive.unarchive::<Calendar>().unwrap();
    assert_eq!(stored.name, "work", "slug visible");
    let pref = stored.preferences(id);
    assert!(pref.name.starts_with("$za$"));
    assert!(pref.description.is_none());
    assert!(stored.dead_properties.0.is_empty());
    let props = client
        .propfind(cal, ["D:displayname", "A:calendar-description", "C:calendar-color"])
        .await;
    props.properties(cal).get("D:displayname").with_values(["Work displayname-canary"]);
    props.properties(cal).get("A:calendar-description").with_values(["coldesc-canary"]);
    props.properties(cal).get("C:calendar-color").with_values(["#aabbcc-canary"]);

    // Custom timezone: calculation rules visible, names and comments sealed, round trip intact.
    let tz = crate::webdav::TEST_VTIMEZONE_1.replace("Eastern Standard Time (US Canada)", "tzname-canary");
    client
        .proppatch(cal, [("A:calendar-timezone", tz.as_str())], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(!raw.contains("tzname-canary"));
    let stored = archive.unarchive::<Calendar>().unwrap();
    let groupware::calendar::ArchivedTimezone::Custom(stored_tz) = &stored.preferences(id).time_zone else { panic!("custom timezone expected") };
    let dump = stored_tz.to_string();
    assert!(dump.contains("TZID:US-Eastern") && dump.contains("TZOFFSETFROM:-0400") && dump.contains("RRULE:"), "{dump}");
    let back = client.propfind(cal, ["A:calendar-timezone"]).await;
    let value = back.properties(cal).get("A:calendar-timezone").value();
    assert!(value.contains("tzname-canary") && value.contains("TZID:US-Eastern") && !value.contains("X-ZA-"), "{value}");

    // Clearing the last ordinary value keeps the timezone readable.
    client
        .proppatch(cal, [], ["A:calendar-description", "C:calendar-color"])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let back = client.propfind(cal, ["A:calendar-timezone", "D:displayname"]).await;
    assert!(back.properties(cal).get("A:calendar-timezone").value().contains("tzname-canary"));
    back.properties(cal).get("D:displayname").with_values(["Work displayname-canary"]);

    // The server-created default calendar: plaintext until the owner first
    // writes a property, sealed afterwards.
    let default = "/dav/cal/key1@example.com/default/";
    client
        .proppatch(default, [("D:displayname", "Default displayname-canary")], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "default").await;
    assert!(archive.unarchive::<Calendar>().unwrap().preferences(id).name.starts_with("$za$"));
    assert!(!String::from_utf8_lossy(archive.as_bytes()).contains("displayname-canary"));
    client.propfind(default, ["D:displayname"]).await.properties(default).get("D:displayname").with_values(["Default displayname-canary"]);

    // Event PROPPATCH: display name and dead properties go into X-ZA-EXTRA.
    let path = "/dav/cal/key1@example.com/work/evt.ics";
    client
        .request_with_headers("PUT", path, [("content-type", "text/calendar; charset=utf-8")], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(path, [("D:displayname", "evtname-canary"), ("X:dead xmlns:X=\"urn:x\"", "dead-canary")], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let (archive, _) = raw_event(test, id, "work/evt.ics").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(!raw.contains("evtname-canary") && !raw.contains("dead-canary"));
    let stored = archive.unarchive::<CalendarEvent>().unwrap();
    assert!(stored.display_name.is_none() && stored.dead_properties.0.is_empty());
    assert!(stored.data.event.to_string().contains("X-ZA-EXTRA:"));
    client.propfind(path, ["D:displayname"]).await.properties(path).get("D:displayname").with_values(["evtname-canary"]);
    let body = client.request("GET", path, "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(!body.contains("X-ZA-") && body.contains("summary-canary"), "{body}");

    // Collection COPY within the account: copied as stored, readable at the new id.
    client
        .request_with_headers("COPY", cal, [("destination", "/dav/cal/key1@example.com/work-copy/"), ("depth", "infinity")], "")
        .await
        .with_status(StatusCode::CREATED);
    let copy = "/dav/cal/key1@example.com/work-copy/";
    client.propfind(copy, ["D:displayname"]).await.properties(copy).get("D:displayname").with_values(["Work displayname-canary"]);
    let body = client.request("GET", "/dav/cal/key1@example.com/work-copy/evt.ics", "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(body.contains("summary-canary"));
    // COPY over an existing collection (Overwrite: T): destination replaced, still readable.
    client
        .proppatch(copy, [("D:displayname", "stale-name")], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    client
        .request_with_headers("COPY", cal, [("destination", copy), ("depth", "infinity"), ("overwrite", "T")], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client.propfind(copy, ["D:displayname"]).await.properties(copy).get("D:displayname").with_values(["Work displayname-canary"]);
    let body = client.request("GET", "/dav/cal/key1@example.com/work-copy/evt.ics", "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(body.contains("summary-canary") && !body.contains("X-ZA-"));

    for path in [copy, cal] {
        client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);
    }
}
```

(`proppatch(path, set, remove)` is in `tests/src/utils/webdav.rs`; match its signature. The `C:` prefix is `http://calendarserver.org/ns/`, declared by the client's PROPFIND; the dead-property syntax follows `tests/src/webdav/prop.rs`.)

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: FAIL at "leaked into the stored collection".

- [ ] **Step 3: MKCALENDAR**

`crates/dav/src/calendar/mkcol.rs`: after `let account_id = ...` add `let za_keys = self.za_session_keys(access_token, account_id).await?;` and immediately before `// Prepare write batch`:

```rust
        if let Some(keys) = &za_keys {
            seal_calendar(&mut calendar, keys, account_id)
                .map_err(|err| seal_error(err, account_id, u32::MAX))?;
        }
```

(imports: `crate::common::za::ZeroAccessGate`, `groupware::calendar::seal::{seal_calendar, seal_error}`.)

- [ ] **Step 4: PROPPATCH**

`crates/dav/src/calendar/proppatch.rs`: after `let account_id = ...` add `let za_keys = self.za_session_keys(access_token, account_id).await?;`.

Calendar branch (lines ~156-161): keep `let calendar = archive.to_unarchived::<Calendar>()?;` as the stored current, and build `new_calendar` from the unsealed view:

```rust
                let view_;
                let mut new_calendar = if let Some(keys) = &za_keys {
                    view_ = unseal_calendar_archive(&archive, keys, account_id)
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                    view_.deserialize::<Calendar>().caused_by(trc::location!())?
                } else {
                    archive.deserialize::<Calendar>().caused_by(trc::location!())?
                };
```

   and right before `new_calendar.update(access_token.account_tenant_ids(), calendar, ..)` add:

```rust
                    if let Some(keys) = &za_keys {
                        seal_calendar(&mut new_calendar, keys, account_id)
                            .map_err(|err| seal_error(err, account_id, document_id))?;
                    }
```

Event branch (lines ~209-214): same shape with `unseal_event_archive` / `seal_event` and `CalendarEvent`.

(imports: `groupware::calendar::seal::{seal_calendar, seal_event, seal_error, unseal_calendar_archive, unseal_event_archive}`.) The colour arrives as the Apple `calendar-color` dead property and is sealed with the other dead properties; the `CalendarPreferences.color` field stays `None` through DAV, as upstream.

- [ ] **Step 5: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
git add crates/dav tests/src/za
git commit -m "Seal key-account collections on MKCALENDAR and PROPPATCH"
```

---

### Task 9: CalDAV suite in both modes

**Files:**
- Modify: `tests/src/webdav/mod.rs`
- Modify: `docs/superpowers/plans/README-dev.md`

**Interfaces:**
- Produces: `ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests` runs `basic`, `put_get`, `mkcol`, `prop`, `multiget`, `sync`, `lock`, `principals`, `card_query`, `cal_query` and `cal_itip` unchanged against key accounts; `copy_move`, `acl`, `cal_alarm` and `cal_scheduling` are skipped in that mode until plan 3 adds their variants.

- [ ] **Step 1: Skip the modules whose variants come in plan 3**

In `webdav_tests`, wrap the four calls:

```rust
    if !key_accounts_mode() {
        copy_move::test(&test, assisted_discovery).await;
    } else {
        println!("copy_move: skipped in key-account mode until plan 3 adds the variant");
    }
    ...
    if !key_accounts_mode() {
        acl::test(&test).await;
        cal_alarm::test(&test).await;
        cal_scheduling::test(&test).await;
    } else {
        println!("acl, cal_alarm, cal_scheduling: skipped in key-account mode until plan 3 adds the variants");
    }
```

keeping every other call in its original position and order.

- [ ] **Step 2: Run both modes and the unit suites**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -3
cargo test -p vault -p groupware -p common 2>&1 | tail -3
```

Expected: all `test result: ok`. `put_get` in key mode is the byte-exact fidelity check (spec 11). If a `prop` sub-test fails on a calendar display name, check that `ArchivedResource::display_name` reads the unsealed view (Task 7's loader).

- [ ] **Step 3: Note and commit**

Append to `README-dev.md`: `- In ZA_KEY_ACCOUNTS=1 mode the acl, cal_alarm, cal_scheduling and copy_move modules are skipped until plan 3 (variants).`

```bash
git add tests docs/superpowers/plans/README-dev.md
git commit -m "Run the CalDAV suite against sealed key accounts"
```

Plan 2 is complete when step 2 passes. Continue with plan 3.
