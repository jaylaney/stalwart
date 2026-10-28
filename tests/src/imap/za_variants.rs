/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account variants of the IMAP sub-tests (mail spec 11), run with
//! `ZA_KEY_ACCOUNTS=1`: every user is a key account, so the steps that search
//! or sort on sealed content (spec 8), share a key account's mailbox or copy
//! across accounts (6.3) or train the spam filter (9) assert the refusal or
//! the designed outcome, and every other step runs as upstream runs it. The
//! group `support@example.com` and the admin stay plain.

use super::{
    AssertResult, ImapConnection, Type,
    antispam::spam_classifier_model,
    append::{assert_append_message, build_messages},
    expand_uid_list,
};
use crate::{
    system::antispam::{HAM, SPAM, TEST},
    utils::{server::TestServer, smtp::SmtpConnection},
};
use common::za::{ZA_MAIL_KEYLESS, ZA_MAIL_SEARCH_REFUSED};
use imap_proto::ResponseType;
use registry::schema::{
    enums::TaskSpamFilterMaintenanceType,
    prelude::ObjectType,
    structs::{Task, TaskSpamFilterMaintenance, TaskStatus},
};

/// Mail spec 8: `command` answers `NO [CANNOT]` with the fixed text and no
/// result line.
async fn assert_search_refused(imap: &mut ImapConnection, command: &str) {
    imap.send(command).await;
    let lines = imap
        .assert_read(Type::Tagged, ResponseType::No)
        .await
        .assert_response_code("CANNOT")
        .assert_contains(ZA_MAIL_SEARCH_REFUSED);
    assert!(
        !lines
            .iter()
            .any(|line| ["* SEARCH", "* SORT", "* THREAD", "* ESEARCH"]
                .iter()
                .any(|prefix| line.starts_with(prefix))),
        "{command}: {lines:?}"
    );
}

/// Mail spec 6.3: `command` answers `NO [NOPERM]` with the keyless text.
async fn assert_noperm(imap: &mut ImapConnection, command: &str) {
    imap.send(command).await;
    imap.assert_read(Type::Tagged, ResponseType::No)
        .await
        .assert_response_code("NOPERM")
        .assert_contains(ZA_MAIL_KEYLESS);
}

/// The `RFC822.SIZE` of every message of the selected mailbox, by UID.
async fn sizes_by_uid(imap: &mut ImapConnection) -> Vec<(u32, u64)> {
    imap.send("UID FETCH 1:* (UID RFC822.SIZE)").await;
    let mut sizes = imap
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .iter()
        .filter(|line| line.starts_with("* "))
        .map(|line| {
            let field = |name: &str| {
                line.split_once(name)
                    .and_then(|(_, rest)| rest.split([' ', ')']).next())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or_else(|| panic!("no {name} in {line}"))
            };
            (field("UID ") as u32, field("RFC822.SIZE "))
        })
        .collect::<Vec<_>>();
    sizes.sort_unstable();
    sizes
}

/// `search::test` in key mode. INBOX holds `append::test`'s ten messages,
/// sealed; UID `n` carries the keyword `Flag_00<n - 1>`. Flag, UID, size and
/// result-option searches are served with upstream's results; the FROM,
/// SUBJECT, TEXT, SENTON and SORT-by-header steps are refused, and the saved
/// search and the sorts are rebuilt from served keys.
pub async fn search(imap: &mut ImapConnection, imap_check: &mut ImapConnection) {
    println!("Running key-account SEARCH tests...");

    // Searches without selecting a mailbox should fail.
    imap.send("SEARCH RETURN (MIN MAX COUNT ALL) ALL").await;
    imap.assert_read(Type::Tagged, ResponseType::Bad).await;

    // Select INBOX
    imap.send("SELECT INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("10 EXISTS")
        .assert_contains("[UIDNEXT 11]");
    imap_check.send("SELECT INBOX").await;
    imap_check.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Min, Max and Count
    imap.send("SEARCH RETURN (MIN MAX COUNT ALL) ALL").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("COUNT 10 MIN 1 MAX 10 ALL 1:10");
    imap_check.send("UID SEARCH ALL").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 1 2 3 4 5 6 7 8 9 10");

    // Filters on content are refused; flag, UID and size filters are served.
    assert_search_refused(imap_check, "UID SEARCH OR FROM nathaniel SUBJECT argentina").await;
    imap_check
        .send("UID SEARCH UNSEEN OR KEYWORD Flag_007 KEYWORD Flag_004")
        .await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 5 8");
    assert_search_refused(
        imap_check,
        "UID SEARCH TEXT coffee FROM vandelay SUBJECT exporting SENTON 20-Nov-2021",
    )
    .await;
    assert_search_refused(
        imap_check,
        concat!(
            "UID SEARCH CHARSET UTF-8 TEXT {75+}\r\n",
            "ℌ𝔢𝔩𝔭 𝔪𝔢 𝔢𝔵𝔭𝔬𝔯𝔱 𝔪𝔶 𝔟𝔬𝔬𝔨"
        ),
    )
    .await;
    // A served key mixed with a refused one is refused as a whole.
    assert_search_refused(imap_check, "UID SEARCH NOT (FROM nathaniel ANSWERED)").await;
    imap_check
        .send("UID SEARCH UID 0:6 LARGER 1000 SMALLER 2000")
        .await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 1 2");

    // Saved search: upstream's FROM/SUBJECT search is refused; the same
    // operations run on a saved flag search (UIDs 1, 3, 4 and 6).
    assert_search_refused(
        imap_check,
        "UID SEARCH RETURN (SAVE ALL) OR OR FROM nathaniel FROM vandelay OR SUBJECT rfc FROM gore",
    )
    .await;
    imap_check
        .send(concat!(
            "UID SEARCH RETURN (SAVE ALL) OR OR KEYWORD Flag_000 KEYWORD Flag_002 ",
            "OR KEYWORD Flag_003 KEYWORD Flag_005"
        ))
        .await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("1,3:4,6");
    imap_check.send("UID SEARCH NOT $").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 2 5 7 8 9 10");
    // Of UIDs 1 to 6 only 1 and 2 are between 1000 and 2000 bytes.
    imap_check
        .send("UID SEARCH $ LARGER 1000 SMALLER 2000")
        .await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 1");
    assert_search_refused(imap_check, "UID SEARCH $ SMALLER 1000 SUBJECT section").await;
    imap_check.send("UID SEARCH RETURN (MIN MAX) NOT $").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("MIN 2 MAX 10");

    // ALL retains every saved flag match even when MIN and MAX are requested.
    imap_check
        .send(concat!(
            "UID SEARCH RETURN (SAVE MIN MAX ALL) OR OR KEYWORD Flag_000 KEYWORD Flag_002 ",
            "OR KEYWORD Flag_003 KEYWORD Flag_005"
        ))
        .await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("MIN 1 MAX 6 ALL 1,3:4,6");
    imap_check.send("UID SEARCH $").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* SEARCH 1 3 4 6");

    // Sort: by subject, date or sender is refused; by size and arrival is
    // served, the size order matching the messages' sizes.
    assert_search_refused(
        imap_check,
        "UID SORT (REVERSE SUBJECT REVERSE DATE) UTF-8 FROM Nathaniel",
    )
    .await;
    assert_search_refused(imap, "UID SORT RETURN (COUNT ALL) (DATE SUBJECT) UTF-8 ALL").await;
    let mut by_size = sizes_by_uid(imap_check).await;
    by_size.sort_by_key(|(uid, size)| (*size, *uid));
    let expected = by_size
        .iter()
        .map(|(uid, _)| uid.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    imap_check.send("UID SORT (SIZE) UTF-8 ALL").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals(&format!("* SORT {expected}"));
    imap.send("UID SORT RETURN (COUNT ALL) (ARRIVAL) UTF-8 ALL")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("COUNT 10 ALL");
}

/// `fetch::test` in key mode: the session holds the account's keys, so every
/// FETCH runs as upstream; only the saved search is rebuilt on the UIDs
/// upstream's refused `FROM` search finds (1, 4 and 6).
pub async fn fetch(imap: &mut ImapConnection, imap_check: &mut ImapConnection) {
    println!("Running key-account FETCH tests...");

    // Examine INBOX
    imap.send("EXAMINE INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("10 EXISTS")
        .assert_contains("[UIDNEXT 11]");

    // Fetch all properties available from JMAP
    imap.send(concat!(
        "FETCH 10 (FLAGS INTERNALDATE PREVIEW OBJECTID ",
        "RFC822.SIZE UID ENVELOPE BODYSTRUCTURE)"
    ))
    .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("FLAGS (Flag_009)")
        .assert_contains("RFC822.SIZE 1457")
        .assert_contains("UID 10")
        .assert_contains("INTERNALDATE")
        .assert_contains("OBJECTID (")
        .assert_contains("EMAILID ")
        .assert_contains("THREADID ")
        .assert_contains("but then I thought, why not do both?")
        .assert_contains(concat!(
            "ENVELOPE (\"Sat, 20 Nov 2021 14:22:01 -0800\" ",
            "\"Why not both importing AND exporting? ☺\" ",
            "((\"Art Vandelay (Vandelay Industries)\" NIL \"art\" \"vandelay.com\")) ",
            "((\"Art Vandelay (Vandelay Industries)\" NIL \"art\" \"vandelay.com\")) ",
            "((\"Art Vandelay (Vandelay Industries)\" NIL \"art\" \"vandelay.com\")) ",
            "((NIL NIL \"Colleagues\" NIL)",
            "(\"James Smythe\" NIL \"james\" \"vandelay.com\")",
            "(NIL NIL NIL NIL)(NIL NIL \"Friends\" NIL)",
            "(NIL NIL \"jane\" \"example.com\")",
            "(\"John Smîth\" NIL \"john\" \"example.com\")",
            "(NIL NIL NIL NIL)) NIL NIL NIL NIL)"
        ))
        .assert_contains(concat!(
            "BODYSTRUCTURE ((\"text\" \"html\" (\"charset\" \"us-ascii\") NIL NIL ",
            "\"base64\" 239 3 \"07aab44e51c5f1833a5d19f2e1804c4b\" NIL NIL NIL)",
            "(\"message\" \"rfc822\" NIL NIL NIL NIL 723 ",
            "(NIL \"Exporting my book about coffee tables\" ",
            "((\"Cosmo Kramer\" NIL \"kramer\" \"kramerica.com\")) ",
            "((\"Cosmo Kramer\" NIL \"kramer\" \"kramerica.com\")) ",
            "((\"Cosmo Kramer\" NIL \"kramer\" \"kramerica.com\")) ",
            "NIL NIL NIL NIL NIL) ",
            "((\"text\" \"plain\" (\"charset\" \"utf-16\") NIL NIL ",
            "\"quoted-printable\" 228 3 \"3a942a99cdd8a099ae107d3867ec20fb\" NIL NIL NIL)",
            "(\"image\" \"gif\" (\"name\" \"Book about ☕ tables.gif\") ",
            "NIL NIL \"Base64\" 56 \"d40fa7f401e9dc2df56cbb740d65ff52\" ",
            "(\"attachment\" NIL) NIL NIL) \"mixed\" (\"boundary\" \"giddyup\") NIL NIL NIL)",
            " 0 \"cdb0382a03a15601fb1b3c7422521620\" NIL NIL NIL) ",
            "\"mixed\" (\"boundary\" \"festivus\") NIL NIL NIL)"
        ));

    imap_check.send("EXAMINE INBOX").await;
    imap_check.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_check.send("FETCH 10 (ENVELOPE BODYSTRUCTURE)").await;
    imap_check
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(
            "\"=?utf-8?B?V2h5IG5vdCBib3RoIGltcG9ydGluZyBBTkQgZXhwb3J0aW5nPyDimLo=?=\" ",
        )
        .assert_contains("(\"=?utf-8?B?Sm9obiBTbcOudGg=?=\" NIL \"john\" \"example.com\")")
        .assert_contains("(\"name\" \"=?utf-8?B?Qm9vayBhYm91dCDimJUgdGFibGVzLmdpZg==?=\")");

    // Fetch bodyparts
    imap.send(concat!(
        "UID FETCH 10 (BINARY[1] BINARY.SIZE[1] BODY[1.TEXT] BODY[2.1.HEADER] ",
        "BINARY[2.1] BODY[MIME] BODY[HEADER.FIELDS (From)]<10.8>)"
    ))
    .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("BINARY[1] ~{175}")
        .assert_contains("BINARY.SIZE[1] 175")
        .assert_contains("BODY[1.TEXT] {239}")
        .assert_contains("BODY[2.1.HEADER] {88}")
        .assert_contains("BINARY[2.1] ~{108}")
        .assert_contains("BODY[MIME] {54}")
        .assert_contains("BODY[HEADER.FIELDS (FROM)]<10> {8}")
        .assert_contains("&ldquo;exporting&rdquo;")
        .assert_contains("PGh0bWw+PHA+")
        .assert_contains("Content-Transfer-Encoding: quoted-printable")
        .assert_contains("Vandelay");
    let fraktur_utf16_le: Vec<u8> = "ℌ𝔢𝔩𝔭 𝔪𝔢 𝔢𝔵𝔭𝔬𝔯𝔱 𝔪𝔶 𝔟𝔬𝔬𝔨"
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    imap.assert_last_contains_bytes(&fraktur_utf16_le);

    // We are in EXAMINE mode, fetching body should not set \Seen
    imap.send("UID FETCH 10 (FLAGS)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("FLAGS (Flag_009)");

    // Switch to SELECT mode
    imap.send("SELECT INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Peek bodyparts
    imap.send("UID FETCH 10 (BINARY.PEEK[1] BINARY.SIZE[1] BODY.PEEK[1.TEXT])")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("BINARY[1] ~{175}")
        .assert_contains("BINARY.SIZE[1] 175")
        .assert_contains("BODY[1.TEXT] {239}");

    // PEEK was used, \Seen should not be set
    imap.send("UID FETCH 10 (FLAGS)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("FLAGS (Flag_009)");

    // Fetching a body section should set the \Seen flag
    imap.send("UID FETCH 10 (BODY[1.TEXT])").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("FLAGS")
        .assert_contains("\\Seen");

    // Fetch a sequence
    imap.send("FETCH 1:5,7:10 (UID FLAGS)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("* 1 FETCH (UID 1 ")
        .assert_contains("* 2 FETCH (UID 2 ")
        .assert_contains("* 3 FETCH (UID 3 ")
        .assert_contains("* 4 FETCH (UID 4 ")
        .assert_contains("* 5 FETCH (UID 5 ")
        .assert_contains("* 7 FETCH (UID 7 ")
        .assert_contains("* 8 FETCH (UID 8 ")
        .assert_contains("* 9 FETCH (UID 9 ")
        .assert_contains("* 10 FETCH (UID 10 ")
        .assert_count("\\Recent", 0);

    imap.send("FETCH 7:* (UID FLAGS)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("* 7 FETCH (UID 7 ")
        .assert_contains("* 8 FETCH (UID 8 ")
        .assert_contains("* 9 FETCH (UID 9 ")
        .assert_contains("* 10 FETCH (UID 10 ");

    // Fetch using a saved search: upstream's `FROM` search is refused (spec
    // 8); a saved search on the UIDs it finds (1, 4 and 6) fetches upstream's
    // three previews.
    assert_search_refused(imap, "UID SEARCH RETURN (SAVE) FROM \"nathaniel\"").await;
    imap.send("UID SEARCH RETURN (SAVE) UID 1,4,6").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("FETCH $ (UID PREVIEW)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("* 1 FETCH (UID 1 ")
        .assert_contains("* 4 FETCH (UID 4 ")
        .assert_contains("* 6 FETCH (UID 6 ")
        .assert_count("FETCH (UID", 3)
        .assert_contains("Some text appears here")
        .assert_contains("plain text version of message goes here")
        .assert_contains("This is implicitly typed plain US-ASCII text.");

    // A failing command in a pipelined batch does not swallow the tagged completion of the commands queued behind it
    imap.send_raw(concat!(
        "_p UID FETCH 1:* (UID) (CHANGEDSINCE 1 VANISHED)\r\n",
        "_x FETCH 1 (UID)\r\n"
    ))
    .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("_p BAD")
        .assert_contains("* 1 FETCH (UID 1");
}

/// `store::test` in key mode: flags are keyless data and every STORE runs as
/// upstream; the saved search is rebuilt on `SEEN` over UIDs 1, 4 and 6 (the
/// three messages upstream's `FROM nathaniel` finds) instead of `FROM`.
pub async fn store(imap: &mut ImapConnection, test: &TestServer) {
    println!("Running key-account STORE tests...");

    // Select INBOX
    imap.send("SELECT INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("10 EXISTS")
        .assert_contains("[UIDNEXT 11]");

    // Set all messages to flag "Seen"
    imap.send("UID STORE 1:10 +FLAGS.SILENT (\\Seen)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("FLAGS", 0);

    // Check that the flags were set
    imap.send("UID FETCH 1:* (Flags)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("\\Seen", 10);

    // Check status
    imap.send("STATUS INBOX (UIDNEXT MESSAGES UNSEEN)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("MESSAGES 10")
        .assert_contains("UNSEEN 0")
        .assert_contains("UIDNEXT 11");

    // Remove Seen flag from all messages
    imap.send("UID STORE 1:10 -FLAGS (\\Seen)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("FLAGS", 10)
        .assert_count("Seen", 0);

    // Check that the flags were removed
    imap.send("UID FETCH 1:* (Flags)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("\\Seen", 0);
    imap.send("STATUS INBOX (UIDNEXT MESSAGES UNSEEN)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("MESSAGES 10")
        .assert_contains("UNSEEN 10")
        .assert_contains("UIDNEXT 11");

    // Store using saved searches: `FROM` is refused (spec 8); the saved
    // search is `SEEN`, over the three messages marked seen for it.
    test.wait_for_tasks().await;
    assert_search_refused(imap, "SEARCH RETURN (SAVE) FROM nathaniel").await;
    imap.send("UID STORE 1,4,6 +FLAGS.SILENT (\\Seen)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("SEARCH RETURN (SAVE) SEEN").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("UID STORE $ +FLAGS (\\Answered)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("FLAGS", 3);

    // Remove Answered flag
    imap.send("UID STORE 1:* -FLAGS (\\Answered)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("FLAGS", 3)
        .assert_count("Answered", 0);

    // The variant's own Seen flags go, leaving upstream's end state.
    imap.send("UID STORE 1,4,6 -FLAGS.SILENT (\\Seen)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
}

/// `thread::test` in key mode: a key account's messages are never threaded
/// (mail spec 5.4), so THREAD answers twelve singletons where upstream finds
/// three threads of four, a THREADID search finds the one message, and a
/// SUBJECT filter is refused (spec 8).
pub async fn thread(imap: &mut ImapConnection, test: &TestServer) {
    println!("Running key-account THREAD tests...");

    // Create test messages
    let messages = build_messages();

    // Insert messages using Multiappend
    imap.send("CREATE Manchego").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    for (pos, message) in messages.iter().enumerate() {
        if pos == 0 {
            imap.send(&format!("APPEND Manchego {{{}}}", message.len()))
                .await;
        } else {
            imap.send_untagged(&format!(" {{{}}}", message.len())).await;
        }
        imap.assert_read(Type::Continuation, ResponseType::Ok).await;
        if pos < messages.len() - 1 {
            imap.send_raw(message).await;
        } else {
            imap.send_untagged(message).await;
            assert_eq!(
                expand_uid_list(
                    &imap
                        .assert_read(Type::Tagged, ResponseType::Ok)
                        .await
                        .into_append_uid()
                )
                .len(),
                messages.len(),
            );
        }
    }

    // Obtain ThreadId and MessageId of the first message
    test.wait_for_tasks().await;
    imap.send("SELECT Manchego").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;

    let mut email_id = None;
    let mut thread_id = None;
    imap.send("UID FETCH 1 (OBJECTID)").await;
    for line in imap.assert_read(Type::Tagged, ResponseType::Ok).await {
        if let Some((_, value)) = line.split_once("EMAILID ") {
            email_id = value
                .split([' ', ')'])
                .next()
                .expect("Missing delimiter")
                .to_string()
                .into();
        }
        if let Some((_, value)) = line.split_once("THREADID ") {
            thread_id = value
                .split([' ', ')'])
                .next()
                .expect("Missing delimiter")
                .to_string()
                .into();
        }
    }
    let email_id = email_id.expect("Missing EMAILID");
    let thread_id = thread_id.expect("Missing THREADID");

    // Twelve singleton threads
    imap.send("THREAD REFERENCES UTF-8 1:*").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* THREAD (1)(2)(3)(4)(5)(6)(7)(8)(9)(10)(11)(12)");

    // Filtering by subject is refused
    assert_search_refused(imap, "THREAD REFERENCES UTF-8 SUBJECT T1").await;

    // Filter by threadId and messageId: both find the first message alone
    imap.send(&format!(
        "UID THREAD REFERENCES UTF-8 THREADID {}",
        thread_id
    ))
    .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("(1)")
        .assert_count("(", 1);

    imap.send(&format!("UID THREAD REFERENCES UTF-8 EMAILID {}", email_id))
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("(1)")
        .assert_count("(", 1);

    // Delete all messages
    imap.send("STORE 1:* +FLAGS.SILENT (\\Deleted)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("EXPUNGE").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("EXPUNGE", 13);
}

/// `acl::test` in key mode. John, Jane and Bill are key accounts and the
/// Support group is plain: Jane reads and writes the group's shared folders
/// as upstream; every grant on her own INBOX is refused (mail spec 6.3), so
/// John and Bill never see it; a copy or move between her mailboxes and
/// the group's, in either direction, is refused while one inside the group
/// works; and upstream's MYRIGHTS and i/t/e rights steps run on the admin's
/// INBOX shared with John and Bill, as on a plain account's share.
pub async fn acl(imap_john: &mut ImapConnection, test: &TestServer) {
    println!("Running key-account ACL tests...");
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest(
        "bill@example.com",
        &["support@example.com"],
        concat!(
            "From: bill@example.com\r\n",
            "To: support@example.com\r\n",
            "Subject: TPS Report\r\n",
            "\r\n",
            "I'm going to need those TPS reports ASAP. ",
            "So, if you could do that, that'd be great."
        ),
    )
    .await;
    let support = "Shared Folders/support@example.com/INBOX";

    // Connect to all test accounts
    let mut imap_jane = test.account("jane.smith@example.com").imap_client().await;
    let mut imap_bill = test.account("foobar@example.com").imap_client().await;

    // Jane should see the Support account, a plain group's share
    imap_jane.send("LIST \"\" \"*\"").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(support);
    imap_jane.send(&format!("SELECT \"{support}\"")).await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_jane.send("FETCH 1 (PREVIEW)").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("TPS reports ASAP");
    imap_jane.send("UNSELECT").await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Jane should be able to create folders under the Support account
    imap_jane
        .send("CREATE \"Shared Folders/support@example.com/inbox/Jane's Folder\"")
        .await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_jane.send("LIST \"\" \"*\"").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals(
            "* LIST () \"/\" \"Shared Folders/support@example.com/INBOX/Jane's Folder\"",
        );

    // John should have no shared folders
    imap_john.send("LIST \"\" \"*\"").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("Shared Folders", 0);
    imap_john.send("NAMESPACE").await;
    imap_john.assert_read(Type::Tagged, ResponseType::Ok).await;

    // List rights
    imap_jane.send("LISTRIGHTS INBOX jdoe@example.com").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* LISTRIGHTS \"INBOX\" \"jdoe@example.com\" r l ws i et k x p a");

    // Jane's grants on her own INBOX are refused, so nothing is shared
    assert_noperm(&mut imap_jane, "SETACL INBOX jdoe@example.com lr").await;
    assert_noperm(&mut imap_jane, "SETACL INBOX foobar@example.com lrxtws").await;
    assert_noperm(&mut imap_jane, "SETACL INBOX jdoe@example.com +i").await;
    imap_jane.send("GETACL INBOX").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_not_contains("jdoe@example.com")
        .assert_not_contains("foobar@example.com");
    for imap in [&mut *imap_john, &mut imap_bill] {
        imap.send("LIST \"\" \"*\"").await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_count("Shared Folders", 0);
        imap.send("SELECT \"Shared Folders/jane.smith@example.com/INBOX\"")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::No).await;
    }
    imap_john.send("NAMESPACE").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* NAMESPACE ((\"\" \"/\")) NIL NIL");
    assert_append_message(
        imap_john,
        "Shared Folders/jane.smith@example.com/INBOX",
        "From: john\n\ncontents",
        ResponseType::No,
    )
    .await;

    // Copying and moving between Jane's mailboxes and the group's is
    // refused in both directions; inside the group it works.
    let uid = assert_append_message(
        &mut imap_jane,
        "INBOX",
        "From: jane\n\ncopy test",
        ResponseType::Ok,
    )
    .await
    .into_append_uid();
    imap_jane.send("SELECT INBOX").await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert_noperm(&mut imap_jane, &format!("UID COPY {uid} \"{support}\"")).await;
    assert_noperm(&mut imap_jane, &format!("UID MOVE {uid} \"{support}\"")).await;
    imap_jane.send(&format!("SELECT \"{support}\"")).await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert_noperm(&mut imap_jane, "COPY 1 INBOX").await;
    assert_noperm(&mut imap_jane, "MOVE 1 INBOX").await;
    imap_jane
        .send(&format!("COPY 1 \"{support}/Jane's Folder\""))
        .await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("[COPYUID ");
    imap_jane.send("STATUS INBOX (MESSAGES)").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("(MESSAGES 1)");
    imap_jane
        .send(&format!("STATUS \"{support}/Jane's Folder\" (MESSAGES)"))
        .await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("(MESSAGES 1)");
    imap_jane.send("UNSELECT").await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_jane
        .send(&format!("DELETE \"{support}/Jane's Folder\""))
        .await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Removing a grant stays possible (none exists here)
    imap_jane.send("DELETEACL INBOX foobar@example.com").await;
    imap_jane.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_jane.send("GETACL INBOX").await;
    imap_jane
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("foobar@example.com", 0);

    // Bill and John still see no shared folder
    for imap in [&mut imap_bill, &mut *imap_john] {
        imap.send("LIST \"\" \"*\"").await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_count("Shared Folders", 0);
    }

    // A plain account's share: upstream's grant, MYRIGHTS and i/t/e steps run
    // on the admin's INBOX shared with John and Bill, key sessions in a plain
    // account, as upstream runs them on Jane's.
    let admin = test.account("admin@example.com");
    let shared = format!("Shared Folders/{}/INBOX", admin.name());
    let mut imap_admin = admin.imap_client().await;
    imap_admin.send("SETACL INBOX jdoe@example.com lr").await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_john.send("LIST \"\" \"*\"").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* LIST (\\NoSelect) \"/\" \"Shared Folders\"")
        .assert_equals(&format!(
            "* LIST (\\NoSelect) \"/\" \"Shared Folders/{}\"",
            admin.name()
        ))
        .assert_equals(&format!("* LIST () \"/\" \"{shared}\""));

    // Grant access to Bill and check ACLs
    imap_admin.send("GETACL INBOX").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("\"jdoe@example.com\" rl");
    imap_admin
        .send("SETACL INBOX foobar@example.com lrxtws")
        .await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_admin.send("GETACL INBOX").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("\"jdoe@example.com\" rl")
        .assert_contains("\"foobar@example.com\" tewsrxl");
    imap_bill.send("LIST \"\" \"*\"").await;
    imap_bill
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(&shared);

    // Namespace should now return the Shared Folders namespace
    imap_john.send("NAMESPACE").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals("* NAMESPACE ((\"\" \"/\")) ((\"Shared Folders\" \"/\")) NIL");

    // List John's right on the admin's Inbox
    imap_john.send(&format!("MYRIGHTS \"{shared}\"")).await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals(&format!("* MYRIGHTS \"{shared}\" rl"));

    // John should not be able to append messages
    assert_append_message(
        imap_john,
        &shared,
        "From: john\n\ncontents",
        ResponseType::No,
    )
    .await;

    // Grant insert access to John on the admin's Inbox, and try inserting
    // the message again.
    imap_admin.send("SETACL INBOX jdoe@example.com +i").await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_john.send(&format!("MYRIGHTS \"{shared}\"")).await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_equals(&format!("* MYRIGHTS \"{shared}\" rli"));
    let uid = assert_append_message(
        imap_john,
        &shared,
        "From: john\n\ncontents",
        ResponseType::Ok,
    )
    .await
    .into_append_uid();

    // Only Bill should be allowed to delete messages on the admin's Inbox
    for imap in [&mut *imap_john, &mut imap_bill] {
        imap.send(&format!("SELECT \"{shared}\"")).await;
        imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    }
    imap_john
        .send(&format!("UID STORE {uid} +FLAGS (\\Deleted)"))
        .await;
    imap_john.assert_read(Type::Tagged, ResponseType::No).await;

    imap_bill
        .send(&format!("UID STORE {uid} +FLAGS (\\Deleted)"))
        .await;
    imap_bill.assert_read(Type::Tagged, ResponseType::Ok).await;

    imap_john.send("UID EXPUNGE").await;
    imap_john.assert_read(Type::Tagged, ResponseType::No).await;

    imap_john.send(&format!("UID FETCH {uid} (PREVIEW)")).await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("contents");

    imap_bill.send("UID EXPUNGE").await;
    imap_bill.assert_read(Type::Tagged, ResponseType::Ok).await;

    imap_bill.send(&format!("UID FETCH {uid} (PREVIEW)")).await;
    imap_bill
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("contents", 0);

    imap_bill
        .send(&format!("STATUS \"{shared}\" (MESSAGES)"))
        .await;
    imap_bill
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("(MESSAGES 0)");
    imap_john.send("UNSELECT").await;
    imap_john.assert_read(Type::Tagged, ResponseType::Ok).await;

    // The admin stops sharing with Bill, and removes Insert access to John
    imap_admin.send("DELETEACL INBOX foobar@example.com").await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_admin.send("SETACL INBOX jdoe@example.com -i").await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_admin.send("GETACL INBOX").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("\"jdoe@example.com\" rl")
        .assert_count("foobar@example.com", 0);

    // Bill should not have access to the admin's Inbox anymore
    imap_bill.send("LIST \"\" \"*\"").await;
    imap_bill
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("Shared Folders", 0);

    // And John should still have access
    imap_john.send("LIST \"\" \"*\"").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("Shared Folders", 3);

    // The share is removed again: the later sub-modules ran without one.
    imap_admin.send("DELETEACL INBOX jdoe@example.com").await;
    imap_admin.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap_john.send("LIST \"\" \"*\"").await;
    imap_john
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_count("Shared Folders", 0);
}

/// `antispam::test` in key mode. Spam training never happens for a key
/// account (mail spec 9): the account's APPENDs, STORE and MOVEs add no
/// sample, and neither does a spam trap that is a key account. The
/// classifier is trained instead with upstream's steps by the admin, a plain
/// account; it weighs the recipient's own samples, so upstream's
/// classification results are asserted on deliveries to the admin, and the
/// same deliveries to the key account are asserted classified (their spam
/// headers read back with the session's keys). The Support group's own move
/// into its Junk folder trains as upstream, while a move between it and
/// Jane's mailboxes is a refused cross-account move (6.3).
pub async fn antispam(test: &TestServer) {
    println!("Running key-account Spam classifier tests...");
    let admin = test.account("admin@example.com");
    let account = test.account("sgd@example.com");
    let mut imap = account.imap_client().await;
    let mut imap_admin = admin.imap_client().await;
    let admin_id = admin.id();

    // Make sure there are no training samples
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    assert_eq!(admin.spam_training_samples().await, vec![]);

    // The key account's APPENDs, STORE and MOVE add no sample
    imap.append("INBOX", HAM[0]).await;
    imap.append("Junk Mail", SPAM[0]).await;
    imap.append("Drafts", SPAM[1]).await;
    imap.append("Drafts", SPAM[2]).await;
    imap.send_ok("SELECT Drafts").await;
    imap.send_ok("STORE 1 +FLAGS ($Junk)").await;
    imap.send_ok("MOVE 2 \"Junk Mail\"").await;
    for message in HAM.iter().skip(1) {
        imap.append("INBOX", message).await;
    }
    for message in SPAM.iter().skip(3) {
        imap.append("Junk Mail", message).await;
    }
    assert_eq!(admin.spam_training_samples().await, vec![]);
    assert_eq!(account.spam_training_samples().await, vec![]);

    // The admin trains the classifier with upstream's steps
    imap_admin.append("INBOX", HAM[0]).await;
    imap_admin.append("Junk Mail", SPAM[0]).await;
    let samples = admin.spam_training_samples().await;
    assert_eq!(samples.iter().filter(|x| !x.1.is_spam).count(), 1);
    assert_eq!(samples.iter().filter(|x| x.1.is_spam).count(), 1);
    imap_admin.append("Drafts", SPAM[1]).await;
    imap_admin.append("Drafts", SPAM[2]).await;
    imap_admin.send_ok("SELECT Drafts").await;
    imap_admin.send_ok("STORE 1 +FLAGS ($Junk)").await;
    imap_admin.send_ok("MOVE 2 \"Junk Mail\"").await;
    let samples = admin.spam_training_samples().await;
    assert_eq!(samples.iter().filter(|x| !x.1.is_spam).count(), 1);
    assert_eq!(samples.iter().filter(|x| x.1.is_spam).count(), 3);
    for message in HAM.iter().skip(1) {
        imap_admin.append("INBOX", message).await;
    }
    for message in SPAM.iter().skip(3) {
        imap_admin.append("Junk Mail", message).await;
    }
    let samples = admin.spam_training_samples().await;
    assert_eq!(samples.iter().filter(|x| !x.1.is_spam).count(), 10);
    assert_eq!(samples.iter().filter(|x| x.1.is_spam).count(), 10);
    assert_eq!(samples.len(), 20);
    // Upstream reads the samples as their owner, whose blob ids name the
    // account; the admin's registry view does not, so the owner is read
    // from the sample.
    assert!(
        samples
            .iter()
            .all(|(_, s)| s.account_id == Some(admin_id) && !s.delete_after_use),
        "{samples:?}"
    );

    // Train the classifier
    admin
        .registry_create_object(Task::SpamFilterMaintenance(TaskSpamFilterMaintenance {
            maintenance_type: TaskSpamFilterMaintenanceType::Train,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    let model = spam_classifier_model(&test.server).await;
    assert_eq!(model.reservoir.ham.total_seen, 10);
    assert_eq!(model.reservoir.spam.total_seen, 10);
    assert_eq!(
        model.last_id,
        samples.iter().map(|(id, _)| id.id()).max().unwrap()
    );
    assert!(test.server.inner.data.spam_classifier.load().is_active());

    // Upstream's deliveries, to the account whose samples trained the
    // classifier (it weighs the recipient's own samples), are classified as
    // upstream classifies them.
    for message in TEST {
        let mut lmtp = SmtpConnection::connect().await;
        lmtp.ingest("bill@example.com", &["admin@example.com"], message)
            .await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    imap_admin.send_ok("SELECT INBOX").await;
    imap_admin.send("FETCH 11 (FLAGS RFC822.TEXT)").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_not_contains("FLAGS ($Junk")
        .assert_contains("Subject: can someone explain")
        .assert_contains("X-Spam-Status: No")
        .assert_contains("PROB_HAM_HIGH");
    imap_admin.send("FETCH 12 (FLAGS RFC822.TEXT)").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_not_contains("FLAGS ($Junk")
        .assert_contains("Subject: classifier test")
        .assert_contains("X-Spam-Status: No")
        .assert_contains_any(&["PROB_SPAM_UNCERTAIN", "PROB_HAM_LOW"]);
    imap_admin.send_ok("SELECT \"Junk Mail\"").await;
    imap_admin.send("FETCH 10 (FLAGS RFC822.TEXT)").await;
    imap_admin
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("FLAGS ($Junk")
        .assert_contains("Subject: save up to")
        .assert_contains("X-Spam-Status: Yes")
        .assert_contains("PROB_SPAM_HIGH");
    imap_admin.send_ok("MOVE 10 INBOX").await;
    let samples = admin.spam_training_samples().await;
    assert_eq!(samples.iter().filter(|x| !x.1.is_spam).count(), 11);
    assert_eq!(samples.iter().filter(|x| x.1.is_spam).count(), 10);

    // The same deliveries to the key account are classified too: each of
    // the three reads, with the session's keys, with its spam headers.
    for message in TEST {
        let mut lmtp = SmtpConnection::connect().await;
        lmtp.ingest("bill@example.com", &["sgd@example.com"], message)
            .await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut delivered = 0;
    for (mailbox, appended) in [("INBOX", 10), ("Junk Mail", 9)] {
        imap.send_ok(&format!("SELECT \"{mailbox}\"")).await;
        imap.send(&format!(
            "FETCH {}:* (BODY.PEEK[HEADER.FIELDS (X-Spam-Status Subject)])",
            appended + 1
        ))
        .await;
        let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
        let fetched = lines
            .iter()
            .filter(|line| line.starts_with("* ") && line.contains(" FETCH "))
            .count();
        // `n:*` with no message past `n` answers the last one.
        if lines.join("\n").contains(&format!("* {appended} FETCH")) {
            continue;
        }
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("X-Spam-Status: "))
                .count(),
            fetched,
            "{mailbox}: {lines:?}"
        );
        delivered += fetched;
    }
    assert_eq!(delivered, 3, "the three deliveries to the key account");
    // The user's move out of Junk adds no ham sample
    imap.send_ok("SELECT \"Junk Mail\"").await;
    imap.send_ok("MOVE 1 INBOX").await;
    assert_eq!(account.spam_training_samples().await, vec![]);
    assert_eq!(admin.spam_training_samples().await.len(), 21);

    // A spam trap that is a key account adds no sample either
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest("bill@example.com", &["spamtrap@example.com"], SPAM[4])
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let samples = admin.spam_training_samples().await;
    assert_eq!(samples.iter().filter(|x| !x.1.is_spam).count(), 11);
    assert_eq!(samples.iter().filter(|x| x.1.is_spam).count(), 10);
    let last_id = samples.iter().map(|(id, _)| id.id()).max().unwrap();
    for _ in 0..2 {
        admin
            .registry_create_object(Task::SpamFilterMaintenance(TaskSpamFilterMaintenance {
                maintenance_type: TaskSpamFilterMaintenanceType::Train,
                status: TaskStatus::now(),
            }))
            .await;
        test.wait_for_tasks().await;
        let model = spam_classifier_model(&test.server).await;
        assert_eq!(model.reservoir.ham.total_seen, 11);
        assert_eq!(model.reservoir.spam.total_seen, 10);
        assert_eq!(model.last_id, last_id);
    }
    assert_eq!(account.spam_training_samples().await, vec![]);

    let support_id = test.account("support@example.com").id();
    let jane = test.account("jane.smith@example.com");
    let jane_id = jane.id();
    let mut imap_jane = jane.imap_client().await;
    let samples_for = |account_id, is_spam: bool| {
        let admin = &admin;
        async move {
            admin
                .spam_training_samples()
                .await
                .into_iter()
                .filter(|(_, sample)| {
                    sample.account_id == Some(account_id) && sample.is_spam == is_spam
                })
                .count()
        }
    };

    imap_jane
        .append("Shared Folders/support@example.com/Drafts", SPAM[1])
        .await;
    assert_eq!(samples_for(support_id, true).await, 0);

    imap_jane
        .send_ok("SELECT \"Shared Folders/support@example.com/Drafts\"")
        .await;
    // Into Jane's own Junk: a cross-account move, refused, trains nothing
    assert_noperm(&mut imap_jane, "MOVE * \"Junk Mail\"").await;
    assert_eq!(samples_for(support_id, true).await, 0);
    // Into the group's own Junk: trains the group as upstream does
    imap_jane
        .send_ok("MOVE * \"Shared Folders/support@example.com/Junk Mail\"")
        .await;
    assert_eq!(samples_for(support_id, true).await, 1);

    imap_jane
        .send_ok("SELECT \"Shared Folders/support@example.com/Junk Mail\"")
        .await;
    assert_noperm(&mut imap_jane, "MOVE * INBOX").await;
    imap_jane
        .send_ok("MOVE * \"Shared Folders/support@example.com/Drafts\"")
        .await;
    assert_eq!(samples_for(support_id, false).await, 1);
    assert_eq!(samples_for(jane_id, false).await, 0);
    assert_eq!(samples_for(jane_id, true).await, 0);
}
