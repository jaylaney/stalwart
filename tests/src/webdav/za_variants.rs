/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account variants of copy_move, acl, cal_alarm and cal_scheduling (spec 11).

use super::{TEST_ICAL_1, TEST_ICAL_2};
use crate::utils::server::TestServer;
use email::cache::MessageCacheFetch;
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
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
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/john%40example.com/default/its-alarming-how-charming-i-feel.ics",
            [("content-type", "text/calendar; charset=utf-8")],
            super::cal_alarm::TEST_ALARM_1.replace(
                "$START",
                &DateTime::from_timestamp(now() as i64 + 5)
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
    test.wait_for_tasks().await;
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // hrefs() includes the collection itself, so 1 means nothing was delivered.
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    // The event is readable by its owner with attendees intact.
    let body = john
        .request("GET", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("mailto:jane@example.com") && !body.contains("X-ZA-"));
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
    // The CANCEL on DELETE must not be delivered either.
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    test.assert_is_empty().await;
}
