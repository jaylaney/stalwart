/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{ImapResponse, ObjectId, list::ListItem};
use types::id::Id;

use super::HighestModSeq;

#[test]
fn serialize_select() {
    let objectid = ObjectId {
        mailbox_id: Some(Id::from(1u32)),
        account_id: Some(Id::from(2u32)),
        ..Default::default()
    };
    let mut objectid_line = b"* OK [".to_vec();
    objectid.serialize(&mut objectid_line);
    objectid_line.extend_from_slice(b"] Object identifiers\r\n");
    let objectid_line = String::from_utf8(objectid_line).unwrap();

    for (mut response, _tag, expected_v2, expected_v1) in [
        (
            super::Response {
                mailbox: ListItem::new("INBOX"),
                total_messages: 172,
                recent_messages: 5,
                unseen_seq: 3,
                uid_validity: 3857529045,
                uid_next: 4392,
                closed_previous: false,
                is_rev2: true,
                is_utf8: true,
                highest_modseq: HighestModSeq::new(100).into(),
                objectid: Some(objectid.clone()),
                permanent_flags: super::PermanentFlags::All,
            },
            "A142",
            format!(
                concat!(
                    "* 172 EXISTS\r\n",
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n",
                    "* LIST () \"/\" \"INBOX\"\r\n",
                    "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\Answered \\Flagged \\Draft \\*)] All allowed\r\n",
                    "* OK [UIDVALIDITY 3857529045] UIDs valid\r\n",
                    "* OK [UIDNEXT 4392] Next predicted UID\r\n",
                    "* OK [HIGHESTMODSEQ 100] Highest Modseq\r\n",
                    "{}"
                ),
                objectid_line
            ),
            format!(
                concat!(
                    "* 172 EXISTS\r\n",
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft \\Recent)\r\n",
                    "* 5 RECENT\r\n",
                    "* OK [UNSEEN 3] Unseen messages\r\n",
                    "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\Answered \\Flagged \\Draft \\*)] All allowed\r\n",
                    "* OK [UIDVALIDITY 3857529045] UIDs valid\r\n",
                    "* OK [UIDNEXT 4392] Next predicted UID\r\n",
                    "* OK [HIGHESTMODSEQ 100] Highest Modseq\r\n",
                    "{}"
                ),
                objectid_line
            ),
        ),
        (
            super::Response {
                mailbox: ListItem::new("~peter/mail/台北/日本語"),
                total_messages: 172,
                recent_messages: 5,
                unseen_seq: 3,
                uid_validity: 3857529045,
                uid_next: 4392,
                closed_previous: true,
                is_rev2: true,
                is_utf8: true,
                highest_modseq: None,
                objectid: Some(objectid.clone()),
                permanent_flags: super::PermanentFlags::All,
            },
            "A142",
            format!(
                concat!(
                    "* OK [CLOSED] Closed previous mailbox\r\n",
                    "* 172 EXISTS\r\n",
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n",
                    // 1.0 drops the OLDNAME tag for a rev2 client (RFC 9051 5.1).
                    "* LIST () \"/\" \"~peter/mail/台北/日本語\"\r\n",
                    "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\Answered \\Flagged \\Draft \\*)] All allowed\r\n",
                    "* OK [UIDVALIDITY 3857529045] UIDs valid\r\n",
                    "* OK [UIDNEXT 4392] Next predicted UID\r\n",
                    "{}"
                ),
                objectid_line
            ),
            format!(
                concat!(
                    "* OK [CLOSED] Closed previous mailbox\r\n",
                    "* 172 EXISTS\r\n",
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft \\Recent)\r\n",
                    "* 5 RECENT\r\n",
                    "* OK [UNSEEN 3] Unseen messages\r\n",
                    "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\Answered \\Flagged \\Draft \\*)] All allowed\r\n",
                    "* OK [UIDVALIDITY 3857529045] UIDs valid\r\n",
                    "* OK [UIDNEXT 4392] Next predicted UID\r\n",
                    "{}"
                ),
                objectid_line
            ),
        ),
    ] {
        let response_v2 = String::from_utf8(response.clone().serialize()).unwrap();
        response.is_rev2 = false;
        let response_v1 = String::from_utf8(response.serialize()).unwrap();

        assert_eq!(response_v2, expected_v2);
        assert_eq!(response_v1, expected_v1);
    }
}
