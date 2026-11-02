/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::parser::parse_sequence_set;
use crate::protocol::{ObjectId, SerializeResponse};
use types::id::Id;

#[test]
fn serialize_error_uses_command_tag() {
    for (error, expected) in [
        (
            trc::AuthEvent::Failed.into_err().id("a1"),
            "a1 NO [AUTHENTICATIONFAILED] ",
        ),
        (
            trc::AuthEvent::Failed
                .into_err()
                .ctx(trc::Key::Id, 7u32)
                .id("a1"),
            "a1 NO [AUTHENTICATIONFAILED] ",
        ),
        (
            trc::AuthEvent::Error
                .into_err()
                .ctx(trc::Key::Id, "12")
                .id("a2"),
            "a2 NO [AUTHENTICATIONFAILED] ",
        ),
        (
            trc::AuthEvent::TooManyAttempts
                .into_err()
                .caused_by(trc::AuthEvent::Failed.into_err().ctx(trc::Key::Id, 7u32))
                .id("a3"),
            "a3 NO [AUTHENTICATIONFAILED] ",
        ),
        (
            trc::AuthEvent::Failed.into_err().ctx(trc::Key::Id, 7u32),
            "* NO [AUTHENTICATIONFAILED] ",
        ),
    ] {
        let response = error.serialize();
        assert!(
            response.starts_with(expected.as_bytes()),
            "{:?} does not start with {expected:?}",
            String::from_utf8_lossy(&response)
        );
    }
}

#[test]
fn serialize_objectid_compound() {
    // Empty compound
    let mut buf = Vec::new();
    ObjectId::default().serialize(&mut buf);
    assert_eq!(String::from_utf8(buf).unwrap(), "OBJECTID ()");

    // Mailbox context: MAILBOXID + ACCOUNTID
    let mut buf = Vec::new();
    ObjectId {
        mailbox_id: Some(Id::from(1u32)),
        account_id: Some(Id::from(2u32)),
        ..Default::default()
    }
    .serialize(&mut buf);
    assert_eq!(
        String::from_utf8(buf).unwrap(),
        format!(
            "OBJECTID (ACCOUNTID {} MAILBOXID {})",
            Id::from(2u32),
            Id::from(1u32)
        )
    );

    // Message context: EMAILID + THREADID only
    let mut buf = Vec::new();
    ObjectId {
        email_id: Some(Id::from_parts(3, 4)),
        thread_id: Some(Id::from(3u32)),
        ..Default::default()
    }
    .serialize(&mut buf);
    assert_eq!(
        String::from_utf8(buf).unwrap(),
        format!(
            "OBJECTID (EMAILID {} THREADID {})",
            Id::from_parts(3, 4),
            Id::from(3u32)
        )
    );
}

#[test]
fn sequence_set_contains() {
    for (sequence, expected_result, max_value) in [
        ("1,5:10", vec![1, 5, 6, 7, 8, 9, 10], 10),
        ("2,4:7,9,12:*", vec![2, 4, 5, 6, 7, 9, 12, 13, 14, 15], 15),
        ("*:4,5:7", vec![4, 5, 6, 7], 7),
        ("2,4,5", vec![2, 4, 5], 5),
    ] {
        let sequence = parse_sequence_set(sequence.as_bytes()).unwrap();

        assert_eq!(
            (1..=15)
                .filter(|num| sequence.contains(*num, max_value))
                .collect::<Vec<_>>(),
            expected_result
        );
    }
}
