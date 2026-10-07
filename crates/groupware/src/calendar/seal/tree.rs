/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Per-component removal bundles (spec 6, 7.1).
//!
//! Every property and parameter the policy does not list as visible is moved
//! out of its component into one encrypted bundle, carried as the
//! component's last entry (`X-ZA-SEALED`). Unsealing puts every removed item
//! back at its original index.

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

/// Carries no content, key or ciphertext: safe to log.
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

/// `za/v1|tree|<account>|<scope>|<component index>|<policy>`
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

/// True if any component carries an `X-ZA-*` property, ours or a client's.
pub fn tree_has_carriers(ical: &ICalendar) -> bool {
    ical.components.iter().any(|c| {
        c.entries.iter().any(|e| {
            matches!(&e.name, ICalendarProperty::Other(n)
                if n.len() > 5 && n.as_bytes()[..5].eq_ignore_ascii_case(b"X-ZA-"))
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
    let raw = STANDARD
        .decode(text.trim())
        .map_err(|_| SealError::Decode)?;
    let Some((&FORMAT_V1, sealed)) = raw.split_first() else {
        return Err(SealError::Format);
    };
    let padded = open(dek, aad, sealed).map_err(|_| SealError::Aead)?;
    let len = padded
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(|b| u32::from_le_bytes(b) as usize)
        .ok_or(SealError::Format)?;
    padded
        .get(4..)
        .and_then(|rest| rest.get(..len))
        .map(|s| s.to_vec())
        .ok_or(SealError::Format)
}

fn seal_component(component: &mut ICalendarComponent, dek: &Secret, aad: &[u8]) -> bool {
    let mut removals = Removals {
        entries: Vec::new(),
        params: Vec::new(),
    };
    let original = std::mem::take(&mut component.entries);
    let mut kept = Vec::with_capacity(original.len());
    for (index, entry) in original.into_iter().enumerate() {
        if !is_visible_property(&component.component_type, &entry.name) {
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
    let Some(carrier) = component.entries.pop_if(|e| is_carrier(e, SEALED_PROP)) else {
        return Ok(());
    };
    let text = entry_text(&carrier).ok_or(SealError::Format)?;
    let plain = open_bytes(dek, aad, text)?;
    // rkyv validates alignment; the decrypted bytes carry no guarantee.
    let mut aligned = rkyv::util::AlignedVec::<16>::with_capacity(plain.len());
    aligned.extend_from_slice(&plain);
    let mut removals = rkyv::from_bytes::<Removals, rkyv::rancor::Error>(&aligned)
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
    removals
        .params
        .sort_by_key(|r| (r.entry_index, r.param_index));
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

/// Restores every component sealed by `seal_tree`. On error the tree is in
/// an undefined state and must be discarded.
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
                let raw = base64::engine::general_purpose::STANDARD
                    .decode(entry_text(last).unwrap())
                    .unwrap();
                // format byte + nonce + (padded plaintext + tag): padded part is a multiple of 256
                assert_eq!(
                    (raw.len() - 1 - 24 - 16) % 256,
                    0,
                    "bundle sizes are on 256-byte boundaries"
                );
            }
        }
        let mut unsealed = sealed.clone();
        unseal_tree(&mut unsealed, &dek, 7, "uid-1").unwrap();
        assert_eq!(unsealed, original, "entry-for-entry, in order");
        assert_eq!(unsealed.to_string(), original.to_string());
        sealed
    }

    const APPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//macOS 14.0//EN\r\nCALSCALE:GREGORIAN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nDTSTART:19810329T020000\r\nTZNAME:CEST-canary\r\nTZOFFSETTO:+0200\r\nEND:DAYLIGHT\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nDTSTART:19961027T030000\r\nTZNAME:CET-canary\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nCREATED:20240101T100000Z\r\nUID:1A2B3C4D-APPLE\r\nDTEND;TZID=Europe/Berlin:20240115T110000\r\nTRANSP:OPAQUE\r\nX-APPLE-TRAVEL-ADVISORY-BEHAVIOR:AUTOMATIC\r\nSUMMARY:Dentist canary-apple\r\nLAST-MODIFIED:20240101T100000Z\r\nDTSTAMP:20240101T100000Z\r\nDTSTART;TZID=Europe/Berlin:20240115T100000\r\nLOCATION:Hauptstrasse 1\\, Berlin\r\nX-APPLE-STRUCTURED-LOCATION;VALUE=URI;X-APPLE-RADIUS=70;X-TITLE=Hauptstrasse 1:geo:52.52,13.40\r\nSEQUENCE:1\r\nBEGIN:VALARM\r\nX-WR-ALARMUID:9F8E7D6C\r\nUID:9F8E7D6C\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Event reminder\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const THUNDERBIRD: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nCREATED:20240201T090000Z\r\nLAST-MODIFIED:20240201T091500Z\r\nDTSTAMP:20240201T091500Z\r\nUID:tb-5e6f7a8b\r\nSUMMARY:Team sync canary-tb\r\nCATEGORIES:Work,Meetings\r\nSTATUS:CONFIRMED\r\nORGANIZER;CN=Jane:mailto:jane@example.com\r\nATTENDEE;CN=John;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:john@example.com\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE:20240219T100000Z\r\nDTSTART:20240205T100000Z\r\nDTEND:20240205T103000Z\r\nTRANSP:OPAQUE\r\nX-MOZ-GENERATION:3\r\nX-MOZ-LASTACK:20240201T091500Z\r\nDESCRIPTION:Weekly\\nAgenda canary-tb-desc\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER;VALUE=DURATION;RELATED=END:-PT5M\r\nDESCRIPTION:Default Mozilla Description\r\nSUMMARY:Default Mozilla Summary\r\nATTENDEE:mailto:john@example.com\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nVERSION:2.0\r\nCALSCALE:GREGORIAN\r\nMETHOD:PUBLISH\r\nX-WR-CALNAME:canary-calname\r\nX-WR-TIMEZONE:America/New_York\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20240301\r\nDTEND;VALUE=DATE:20240302\r\nDTSTAMP:20240210T120000Z\r\nUID:google-abc123@google.com\r\nCREATED:20240210T120000Z\r\nDESCRIPTION:All day canary-google\r\nLAST-MODIFIED:20240210T120000Z\r\nSEQUENCE:0\r\nSTATUS:CONFIRMED\r\nSUMMARY:Holiday\r\nTRANSP:TRANSPARENT\r\nATTACH;FMTTYPE=application/pdf;X-GOOGLE-ID=1:https://drive.google.com/file/d/x\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const DAVX5: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:+//IDN bitfire.at//DAVx5/4.3.9 ical4j/3.2.14\r\nBEGIN:VEVENT\r\nDTSTAMP:20240305T080000Z\r\nUID:davx5-77\r\nSEQUENCE:2\r\nSUMMARY:Run canary-davx5\r\nDTSTART;TZID=Europe/Vienna:20240306T070000\r\nDURATION:PT1H\r\nRDATE;VALUE=PERIOD;TZID=Europe/Vienna:20240308T070000/PT30M\r\nRECURRENCE-ID;RANGE=THISANDFUTURE;TZID=Europe/Vienna:20240306T070000\r\nCLASS:PRIVATE\r\nPRIORITY:5\r\nGEO:48.2082;16.3738\r\nURL:https://example.com/run\r\nCOLOR:tomato\r\nCONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=Call:https://meet.example.com/run\r\nX-RADICALE-NAME:run.ics\r\nBEGIN:VALARM\r\nTRIGGER;RELATED=START:-PT10M\r\nACTION:AUDIO\r\nREPEAT:2\r\nDURATION:PT1M\r\nATTACH;VALUE=URI:Basso\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Vienna\r\nX-LIC-LOCATION:Europe/Vienna\r\nLAST-MODIFIED:20230101T000000Z\r\nTZURL:http://tzurl.org/zoneinfo/Europe/Vienna\r\nBEGIN:STANDARD\r\nTZNAME:CET-canary\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nDTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nCOMMENT:canary-tz-comment\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n";

    const SYNTHETIC_INTERLEAVED: &str = "BEGIN:VCALENDAR\r\nX-FIRST:1\r\nVERSION:2.0\r\nX-SECOND:2\r\nPRODID:-//x//EN\r\nNAME:canary-name\r\nBEGIN:VEVENT\r\nSUMMARY:a\r\nUID:u\r\nSUMMARY:b\r\nDTSTART;X-ORIGIN=canary-param;TZID=UTC;X-OTHER=2:20240101T000000\r\nDESCRIPTION:c\r\nDTEND;VALUE=DATE-TIME;X-TAIL=1:20240101T010000\r\nCATEGORIES:x,y\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const REPEATED_PARAMETERS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:rp\r\nDTSTART;TZID=UTC:20240101T000000\r\nATTENDEE;MEMBER=\"mailto:a@x\";MEMBER=\"mailto:b@x\";CN=Zed;DELEGATED-FROM=\"mailto:c@x\";DELEGATED-FROM=\"mailto:d@x\":mailto:z@x\r\nRDATE;VALUE=DATE;X-ONE=1;X-ONE=2:20240102,20240103\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn corpus_round_trips() {
        for text in [APPLE, THUNDERBIRD, GOOGLE, DAVX5, SYNTHETIC_INTERLEAVED] {
            let sealed = round_trip(text);
            let dump = sealed.to_string();
            for canary in [
                "canary-",
                "Dentist",
                "Hauptstrasse",
                "Team sync",
                "jane@example.com",
                "Holiday",
                "tomato",
                "Basso",
                "CET-canary",
                "CEST-canary",
            ] {
                assert!(!dump.contains(canary), "{canary} leaked in {dump}");
            }
        }
        // Policy exception: calcard resolves a VTIMEZONE by X-LIC-LOCATION,
        // so it stays visible after sealing.
        assert!(
            round_trip(DAVX5)
                .to_string()
                .contains("X-LIC-LOCATION:Europe/Vienna")
        );
    }

    #[test]
    fn repeated_parameters() {
        let sealed = round_trip(REPEATED_PARAMETERS);
        assert!(!sealed.to_string().contains("MEMBER"));
    }

    #[test]
    fn nothing_to_seal_adds_no_carrier() {
        let mut ical = parse(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u\r\nDTSTART;TZID=UTC:20240101T000000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let original = ical.clone();
        assert!(!seal_tree(&mut ical, &Secret::random(), 1, "u"));
        assert_eq!(ical, original);
        assert!(!tree_has_carriers(&ical));
    }

    #[test]
    fn client_supplied_carriers_round_trip_and_garbage_fails() {
        // Client carriers that are not in the carrier position are ordinary
        // sealed X- properties and come back unchanged at their indices.
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-ZA-KEY:AQE=\r\nBEGIN:VEVENT\r\nUID:u\r\nX-ZA-SEALED:not-ours\r\nDTSTART;TZID=UTC:20240101T000000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let sealed = round_trip(text);
        // Our carrier is the last entry; the client's are inside the bundle.
        for component in &sealed.components {
            assert_eq!(
                component
                    .entries
                    .iter()
                    .filter(|e| is_carrier(e, SEALED_PROP))
                    .count(),
                1
            );
            assert!(is_carrier(component.entries.last().unwrap(), SEALED_PROP));
            assert!(!component.entries.iter().any(|e| is_carrier(e, KEY_PROP)));
        }

        // Client garbage in the carrier position (the component's last
        // entry): an error, never a panic and never partial data.
        for (value, expected) in [
            // Not base64.
            ("not-ours", SealError::Decode),
            // Base64 of "not-ours": wrong format byte.
            ("bm90LW91cnM=", SealError::Format),
            // Right format byte, nothing to authenticate.
            ("AQ==", SealError::Aead),
            // Right format byte, a nonce and a tag that do not authenticate.
            (
                "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                SealError::Aead,
            ),
        ] {
            // Variant 1: the VEVENT's last entry is a client X-ZA-SEALED.
            // Variant 2: additionally, the root's last entry is a client
            // X-ZA-KEY (not a tree carrier; unseal_event consumes it).
            for root_tail in ["", "X-ZA-KEY:garbage\r\n"] {
                let garbage_text = format!(
                    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{root_tail}BEGIN:VEVENT\r\nUID:u\r\nDTSTART;TZID=UTC:20240101T000000\r\nX-ZA-SEALED:{value}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
                );
                let mut garbage = parse(&garbage_text);
                let event = &garbage.components[1];
                assert!(is_carrier(event.entries.last().unwrap(), SEALED_PROP));
                if !root_tail.is_empty() {
                    assert!(is_carrier(
                        garbage.components[0].entries.last().unwrap(),
                        KEY_PROP
                    ));
                }
                assert_eq!(
                    unseal_tree(&mut garbage, &Secret::random(), 1, "u"),
                    Err(expected.clone()),
                    "{garbage_text}"
                );
            }
        }
    }

    #[test]
    fn tampering_is_an_error_not_a_panic() {
        let dek = Secret::random();
        let mut sealed = parse(THUNDERBIRD);
        seal_tree(&mut sealed, &dek, 7, "uid-1");
        // Wrong account, wrong scope, wrong key.
        for (account, scope, key) in [
            (8u32, "uid-1", dek.clone()),
            (7, "uid-2", dek.clone()),
            (7, "uid-1", Secret::random()),
        ] {
            let mut t = sealed.clone();
            assert_eq!(
                unseal_tree(&mut t, &key, account, scope),
                Err(SealError::Aead)
            );
        }
        // The VCALENDAR root has nothing to seal here; move the event's
        // bundle onto the alarm (component index 1 -> 2).
        let mut moved = sealed.clone();
        let event_carrier = moved.components[1].entries.pop().unwrap();
        moved.components[2].entries.pop();
        moved.components[2].entries.push(event_carrier);
        assert!(matches!(
            unseal_tree(&mut moved, &dek, 7, "uid-1"),
            Err(SealError::Aead)
        ));
        // Truncated and corrupted base64.
        let mut truncated = sealed.clone();
        if let Some(ICalendarValue::Text(t)) = truncated.components[1]
            .entries
            .last_mut()
            .unwrap()
            .values
            .first_mut()
        {
            t.truncate(10);
        }
        assert!(matches!(
            unseal_tree(&mut truncated, &dek, 7, "uid-1"),
            Err(SealError::Decode | SealError::Format | SealError::Aead)
        ));
        let mut flipped = sealed.clone();
        if let Some(ICalendarValue::Text(t)) = flipped.components[1]
            .entries
            .last_mut()
            .unwrap()
            .values
            .first_mut()
        {
            let mut bytes = base64::engine::general_purpose::STANDARD
                .decode(t.as_str())
                .unwrap();
            bytes[40] ^= 1;
            *t = base64::engine::general_purpose::STANDARD.encode(bytes);
        }
        assert_eq!(
            unseal_tree(&mut flipped, &dek, 7, "uid-1"),
            Err(SealError::Aead)
        );
    }
}
