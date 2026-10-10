/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account variants of copy_move, acl, cal_alarm and cal_scheduling (spec 11).

use super::{TEST_ICAL_1, TEST_ICAL_2};
use crate::utils::{
    server::TestServer,
    za::{mail_count, plant_event, queued_recipients, wait_for_delivery},
};
use calcard::common::timezone::Tz;
use email::cache::MessageCacheFetch;
use groupware::{
    calendar::CalendarEvent,
    scheduling::{
        ItipTime, ItipValue,
        format::{DateStyle, TextFormatter},
    },
};
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
use std::time::{Duration, Instant};
use store::write::now;

pub async fn copy_move(test: &TestServer) {
    println!("Running key-account copy/move tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();

    // In-account copy and move succeed.
    john.request_with_headers(
        "PUT",
        "/dav/cal/john@example.com/default/a.ics",
        [("content-type", "text/calendar")],
        TEST_ICAL_1,
    )
    .await
    .with_status(StatusCode::CREATED);
    john.mkcol(
        "MKCALENDAR",
        "/dav/cal/john@example.com/other/",
        [],
        [("D:displayname", "Other")],
    )
    .await
    .with_status(StatusCode::CREATED);
    john.request_with_headers(
        "COPY",
        "/dav/cal/john@example.com/default/a.ics",
        [("destination", "/dav/cal/john@example.com/other/a.ics")],
        "",
    )
    .await
    .with_status(StatusCode::CREATED);
    john.request_with_headers(
        "MOVE",
        "/dav/cal/john@example.com/other/a.ics",
        [("destination", "/dav/cal/john@example.com/other/b.ics")],
        "",
    )
    .await
    .with_status(StatusCode::CREATED);
    let body = john
        .request("GET", "/dav/cal/john@example.com/other/b.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        body.contains("What a nice present") && !body.contains("X-ZA-"),
        "{body}"
    );
    john.request_with_headers(
        "COPY",
        "/dav/cal/john@example.com/other/",
        [
            ("destination", "/dav/cal/john@example.com/other-copy/"),
            ("depth", "infinity"),
        ],
        "",
    )
    .await
    .with_status(StatusCode::CREATED);
    john.request("GET", "/dav/cal/john@example.com/other-copy/b.ics", "")
        .await
        .with_status(StatusCode::OK);

    // Across accounts: refused in both directions, including to a group the
    // caller belongs to (Jane is a member of support).
    john.request_with_headers(
        "COPY",
        "/dav/cal/john@example.com/default/a.ics",
        [("destination", "/dav/cal/jane@example.com/default/a.ics")],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    john.request_with_headers(
        "MOVE",
        "/dav/cal/john@example.com/default/a.ics",
        [("destination", "/dav/cal/support@example.com/default/a.ics")],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    jane.request_with_headers(
        "PUT",
        "/dav/cal/jane@example.com/default/j.ics",
        [("content-type", "text/calendar")],
        TEST_ICAL_2,
    )
    .await
    .with_status(StatusCode::CREATED);
    jane.request_with_headers(
        "COPY",
        "/dav/cal/jane@example.com/default/j.ics",
        [("destination", "/dav/cal/support@example.com/default/j.ics")],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    jane.request_with_headers(
        "COPY",
        "/dav/cal/jane@example.com/default/",
        [
            ("destination", "/dav/cal/support@example.com/jane/"),
            ("depth", "infinity"),
        ],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    // Jane (key account, member of support) moves from her own calendar into
    // the group: refused by the gate, not by permissions.
    jane.request_with_headers(
        "MOVE",
        "/dav/cal/jane@example.com/default/j.ics",
        [("destination", "/dav/cal/support@example.com/default/j2.ics")],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    // From the group's calendar into Jane's account: refused too.
    jane.request_with_headers(
        "PUT",
        "/dav/cal/support@example.com/default/g.ics",
        [("content-type", "text/calendar")],
        TEST_ICAL_2,
    )
    .await
    .with_status(StatusCode::CREATED);
    jane.request_with_headers(
        "COPY",
        "/dav/cal/support@example.com/default/g.ics",
        [("destination", "/dav/cal/jane@example.com/default/g.ics")],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    jane.request("DELETE", "/dav/cal/support@example.com/default/g.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    for (client, path) in [
        (&john, "/dav/cal/john@example.com/other-copy/"),
        (&john, "/dav/cal/john@example.com/other/"),
        (&john, "/dav/cal/john@example.com/default/a.ics"),
        (&jane, "/dav/cal/jane@example.com/default/j.ics"),
    ] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    jane.delete_default_containers_by_account("support@example.com")
        .await;
    test.assert_is_empty().await;
}

pub async fn acl(test: &TestServer) {
    println!("Running key-account ACL tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();
    let grant = "<?xml version=\"1.0\"?><D:acl xmlns:D=\"DAV:\"><D:ace><D:principal><D:href>/dav/pal/jane@example.com/</D:href></D:principal><D:grant><D:privilege><D:read/></D:privilege></D:grant></D:ace></D:acl>";
    john.request("ACL", "/dav/cal/john@example.com/default/", grant)
        .await
        .with_status(StatusCode::FORBIDDEN);
    jane.request("PROPFIND", "/dav/cal/john@example.com/default/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // A key account that is a group member reads the group's calendar (Review Focus 1).
    jane.request_with_headers(
        "PUT",
        "/dav/cal/support@example.com/default/member.ics",
        [("content-type", "text/calendar")],
        TEST_ICAL_2,
    )
    .await
    .with_status(StatusCode::CREATED);
    let body = jane
        .request("GET", "/dav/cal/support@example.com/default/member.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("BEGIN:VEVENT"), "{body}");
    let listing = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/cal/support@example.com/default/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        listing.hrefs().iter().any(|h| h.ends_with("member.ics")),
        "{:?}",
        listing.hrefs()
    );
    jane.request("PROPFIND", "/dav/cal/support@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    john.request("PROPFIND", "/dav/cal/support@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    jane.request(
        "DELETE",
        "/dav/cal/support@example.com/default/member.ics",
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    jane.delete_default_containers_by_account("support@example.com")
        .await;
    test.assert_is_empty().await;
}

pub async fn alarm(test: &TestServer) {
    println!("Running key-account alarm tests...");
    let account = test.account("john@example.com");
    let client = account.webdav_client();
    let start = now() as i64 + 5;
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/john%40example.com/default/its-alarming-how-charming-i-feel.ics",
            [("content-type", "text/calendar; charset=utf-8")],
            super::cal_alarm::TEST_ALARM_1.replace(
                "$START",
                &DateTime::from_timestamp(start)
                    .to_rfc3339()
                    .replace(['-', ':'], ""),
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    let messages = test
        .server
        .get_cached_messages(client.account_id)
        .await
        .unwrap();
    assert_eq!(messages.emails.items.len(), 2);
    // Spec 11: the generic email still carries the event's start time.
    let start = TextFormatter::new("en").unwrap().field_to_string(
        &ItipValue::Time(ItipTime {
            start,
            tz_id: Tz::UTC.as_id(),
        }),
        DateStyle::Short,
    );
    for message in messages.emails.items.iter() {
        let contents = test
            .fetch_email(client.account_id, message.document_id)
            .await;
        let message = MessageParser::new().parse(&contents).unwrap();
        let to = message
            .to()
            .and_then(|t| t.first())
            .and_then(|a| a.address())
            .unwrap_or_default()
            .to_string();
        // External alarm recipients are allowed on this server (key mode),
        // so only the key-account override keeps the VALARM ATTENDEE out.
        assert_eq!(
            to, "john@example.com",
            "recipient is the account address, not the alarm attendee"
        );
        let html =
            String::from_utf8(message.html_bodies().next().unwrap().contents().to_vec()).unwrap();
        let text = message
            .html_bodies()
            .next()
            .unwrap()
            .text_contents()
            .unwrap()
            .to_string();
        assert!(
            html.contains(
                "/dav/cal/john%40example.com/default/its-alarming-how-charming-i-feel.ics"
            ),
            "{html}"
        );
        assert!(text.contains(&start), "{start} missing from: {text}");
        // Spec 9: no organizer row. The account address appears only in the
        // headers; the link carries it percent-encoded, which is why the
        // plain form is a usable canary for the organizer row.
        assert!(
            !html.contains("john@example.com") && !text.contains("john@example.com"),
            "organizer row present in the generic alarm email: {text}"
        );
        for canary in [
            "See the pretty girl",
            "What mirror where",
            "I feel pretty",
            "alarming how charming",
            "meet.example.com/west-side",
            "West Side",
            "john_doe@unknown.com",
        ] {
            assert!(
                !html.contains(canary) && !text.contains(canary),
                "{canary} leaked into the alarm email: {html}"
            );
        }
        let subject = message.subject().unwrap_or_default();
        assert!(
            !subject.contains("pretty")
                && !subject.contains("mirror")
                && !subject.contains("No Subject"),
            "{subject}"
        );
    }
    client.delete_default_containers().await;
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await
}

pub async fn scheduling(test: &TestServer) {
    println!("Running key-account scheduling tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();
    let jane_mail = mail_count(test, jane.account_id).await;
    let invite = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-sched-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:sched-canary\r\nORGANIZER:mailto:john@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:jane@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let response = john
        .request_with_headers(
            "PUT",
            "/dav/cal/john@example.com/default/s.ics",
            [("content-type", "text/calendar; charset=utf-8")],
            invite,
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(response.headers.get("schedule-tag").is_none());
    wait_for_delivery(test).await;
    // Sender side: no iMIP email reached jane's mailbox. Her scheduling inbox
    // would stay empty even if john's send gate failed, because jane is a key
    // account whose own ingest gate drops invitations; the mailbox is what
    // shows john sent nothing.
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // hrefs() includes the collection itself, so one href is an empty inbox
    // (jane's own ingest gate).
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    // The event is readable by its owner with attendees intact.
    let body = john
        .request("GET", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("mailto:jane@example.com") && !body.contains("X-ZA-"));
    // A CANCEL on DELETE needs a stored schedule tag (`delete_all`); key
    // accounts never get one. The DELETE gate itself is tested with a
    // planted event in `za::gating::test_scheduling`.
    let (archive, _) = crate::za::dav_seal::raw_event(test, john.account_id, "default/s.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_none()
    );
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    // Counting jane's mail created her default mailboxes.
    test.destroy_all_mailboxes(test.account("jane@example.com"))
        .await;
    test.assert_is_empty().await;
}

/// A legacy plaintext event whose VALARM names an external recipient.
const R15_ALARM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-r15\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:$START\r\nDURATION:PT1H\r\nSUMMARY:r15-canary\r\nBEGIN:VALARM\r\nTRIGGER:-P2S\r\nACTION:EMAIL\r\nATTENDEE:mailto:r15-external@unknown.com\r\nSUMMARY:r15-canary\r\nDESCRIPTION:r15-canary\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// R15: a key account's alarm email goes to the account's own address even
/// when the VALARM names an external ATTENDEE and the server allows external
/// alarm recipients (key mode sets `allow_external_rcpts`). Sealing hides a
/// VALARM's ATTENDEE in every event written with keys, so only a legacy
/// plaintext event, planted here, reaches the override in the alarm task.
pub async fn alarm_override(test: &TestServer) {
    println!("Running key-account alarm recipient override test...");
    let account = test.account("john@example.com");
    let client = account.webdav_client();
    let id = client.account_id;
    let cal = "/dav/cal/john%40example.com/r15/";
    client
        .request(
            "MKCALENDAR",
            cal,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>",
        )
        .await
        .with_status(StatusCode::CREATED);
    let start = DateTime::from_timestamp(now() as i64 + 5)
        .to_rfc3339()
        .replace(['-', ':'], "");
    plant_event(
        test,
        id,
        "r15",
        "r15.ics",
        &R15_ALARM.replace("$START", &start),
        None,
    )
    .await;

    // The alarm fires two seconds before the start.
    let deadline = Instant::now() + Duration::from_secs(15);
    while mail_count(test, id).await == 0 {
        assert!(
            Instant::now() < deadline,
            "no alarm email reached the account; queued for {:?}",
            queued_recipients(test).await
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    wait_for_delivery(test).await;
    let messages = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(messages.emails.items.len(), 1);
    let contents = test
        .fetch_email(id, messages.emails.items[0].document_id)
        .await;
    let message = MessageParser::new().parse(&contents).unwrap();
    let to = message
        .to()
        .and_then(|t| t.first())
        .and_then(|a| a.address())
        .unwrap_or_default();
    assert_eq!(to, "john@example.com", "recipient is the account address");
    assert!(
        !String::from_utf8_lossy(&contents).contains("r15-external"),
        "the external alarm attendee appears in the email"
    );

    test.wait_for_tasks().await;
    client
        .request("DELETE", &format!("{cal}r15.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // The MKCALENDAR above recreated the default containers that `alarm`
    // deleted.
    client.delete_default_containers().await;
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await
}
