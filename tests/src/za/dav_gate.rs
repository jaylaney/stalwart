/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::{
    utils::{server::TestServer, webdav::DummyWebDavClient},
    webdav::{TEST_ICAL_1, TEST_ICAL_2},
};
use common::auth::oauth::GrantType;
use hyper::StatusCode;

const CALENDAR_QUERY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop><D:getetag/><C:calendar-data/></D:prop>
  <C:filter><C:comp-filter name="VCALENDAR"/></C:filter>
</C:calendar-query>"#;

const FREE_BUSY_QUERY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<C:free-busy-query xmlns:C="urn:ietf:params:xml:ns:caldav">
  <C:time-range start="19000101T000000Z" end="21000101T000000Z"/>
</C:free-busy-query>"#;

fn calendar_multiget(hrefs: &[&str]) -> String {
    let hrefs = hrefs
        .iter()
        .map(|href| format!("<D:href>{href}</D:href>"))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<C:calendar-multiget xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop><D:getetag/><C:calendar-data/></D:prop>
  {hrefs}
</C:calendar-multiget>"#
    )
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access DAV gate tests...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").clone();
    let key1_id = key1.id().document_id();
    let plain = test.account("plain@example.com").clone();
    // `plain` has no email address, so `webdav_client()` cannot be used.
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let key_client =
        DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let master = Box::leak(format!("key1@example.com%{}", admin.name()).into_boxed_str());
    let master_client = DummyWebDavClient::new(key1_id, master, admin.secret(), "key1@example.com");
    let admin_client = DummyWebDavClient::new(key1_id, admin.name(), admin.secret(), admin.name());

    // Owner with keys: allowed.
    key_client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);

    // Master-user login (admin impersonating key1): token has no keys -> 403.
    master_client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Admin with Impersonate permission addressing the account directly: 403.
    admin_client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    admin_client
        .request("PROPFIND", "/dav/itip/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Controls: the same admin requests on a non-key account pass upstream's
    // access check, so the 403s above are the gate's.
    admin_client
        .request("PROPFIND", "/dav/cal/plain@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    admin_client
        .request("PROPFIND", "/dav/itip/plain@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Address book and principal paths are not gated.
    admin_client
        .request("PROPFIND", "/dav/card/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    admin_client
        .request("PROPFIND", "/dav/pal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // OAuth Bearer for the key account itself: the token is valid (the
    // address book answers) but carries no keys, so calendar paths refuse
    // it (spec 4.2). Minted in-process: it is the token the server issues at
    // the end of an OAuth flow, and the tests crate has no OAuth client.
    let token = test
        .server
        .encode_access_token(
            GrantType::AccessToken,
            key1_id,
            "key1@example.com",
            3600,
            None,
            None,
        )
        .await
        .unwrap();
    let mut bearer_client =
        DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    bearer_client.credentials = format!("Bearer {token}");
    bearer_client
        .request("PROPFIND", "/dav/card/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    for path in [
        "/dav/cal/key1@example.com/",
        "/dav/cal/key1@example.com/default/",
        "/dav/itip/key1@example.com/",
    ] {
        bearer_client
            .request("PROPFIND", path, "")
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    bearer_client
        .request_with_headers(
            "REPORT",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            CALENDAR_QUERY,
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Control: a Bearer token of the non-key account reads its own calendars.
    let plain_id = plain.id().document_id();
    let token = test
        .server
        .encode_access_token(
            GrantType::AccessToken,
            plain_id,
            plain.name(),
            3600,
            None,
            None,
        )
        .await
        .unwrap();
    let mut plain_bearer =
        DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    plain_bearer.credentials = format!("Bearer {token}");
    plain_bearer
        .request("PROPFIND", "/dav/cal/plain@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Another user without any grant: 403 (as upstream), unchanged.
    plain_client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::FORBIDDEN);

    // One event in each account's default calendar.
    plain_client
        .request_with_headers(
            "PUT",
            "/dav/cal/plain@example.com/default/x.ics",
            [("content-type", "text/calendar")],
            TEST_ICAL_1,
        )
        .await
        .with_status(StatusCode::CREATED);
    key_client
        .request_with_headers(
            "PUT",
            "/dav/cal/key1@example.com/default/y.ics",
            [("content-type", "text/calendar")],
            TEST_ICAL_2,
        )
        .await
        .with_status(StatusCode::CREATED);

    // Calendar REPORTs are served only under the calendar and scheduling
    // prefixes: elsewhere the URI's account would reach the calendar store
    // past the gate.
    for client in [&admin_client, &master_client] {
        for prefix in ["pal", "card", "file"] {
            let path = format!("/dav/{prefix}/key1@example.com/default/");
            for body in [CALENDAR_QUERY, FREE_BUSY_QUERY] {
                client
                    .request("REPORT", &path, body)
                    .await
                    .with_status(StatusCode::METHOD_NOT_ALLOWED);
            }
            client
                .request(
                    "REPORT",
                    &path,
                    calendar_multiget(&["/dav/cal/key1%40example.com/default/y.ics"]),
                )
                .await
                .with_status(StatusCode::METHOD_NOT_ALLOWED);
        }
        // Under the calendar and scheduling prefixes, the gate refuses.
        for prefix in ["cal", "itip"] {
            let path = format!("/dav/{prefix}/key1@example.com/default/");
            for body in [CALENDAR_QUERY, FREE_BUSY_QUERY] {
                client
                    .request("REPORT", &path, body)
                    .await
                    .with_status(StatusCode::FORBIDDEN);
            }
        }
        // A multiget href outside the calendar prefix is not read; one
        // inside it is refused by the gate.
        let response = client
            .multiget_calendar(
                "/dav/cal/plain@example.com/default/",
                &[
                    "/dav/pal/key1%40example.com/default/y.ics",
                    "/dav/card/key1%40example.com/default/y.ics",
                    "/dav/file/key1%40example.com/default/y.ics",
                    "/dav/cal/key1%40example.com/default/y.ics",
                ],
            )
            .await;
        for prefix in ["pal", "card", "file"] {
            response
                .properties(&format!("/dav/{prefix}/key1%40example.com/default/y.ics"))
                .with_status(StatusCode::NOT_FOUND);
        }
        response
            .properties("/dav/cal/key1%40example.com/default/y.ics")
            .with_status(StatusCode::FORBIDDEN);
    }
    // Non-key accounts are unaffected under the calendar prefix.
    plain_client
        .request(
            "REPORT",
            "/dav/cal/plain@example.com/default/",
            CALENDAR_QUERY,
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .with_hrefs(["/dav/cal/plain%40example.com/default/x.ics"]);
    plain_client
        .request(
            "REPORT",
            "/dav/cal/plain@example.com/default/",
            FREE_BUSY_QUERY,
        )
        .await
        .with_status(StatusCode::OK);
    plain_client
        .multiget_calendar(
            "/dav/cal/plain@example.com/default/",
            &["/dav/cal/plain%40example.com/default/x.ics"],
        )
        .await
        .properties("/dav/cal/plain%40example.com/default/x.ics")
        .with_status(StatusCode::OK);

    // Cross-account COPY/MOVE where either side is a key account: 403 even
    // when the ACL allows it. `plain` grants key1 read and write on its
    // calendar; key1 can then read and write there directly.
    plain_client
        .acl(
            "/dav/cal/plain@example.com/default/",
            "/dav/pal/key1%40example.com/",
            ["read", "write"],
        )
        .await
        .with_status(StatusCode::OK);
    key_client
        .request("GET", "/dav/cal/plain@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::OK);
    key_client
        .request_with_headers(
            "PUT",
            "/dav/cal/plain@example.com/default/z.ics",
            [("content-type", "text/calendar")],
            TEST_ICAL_2,
        )
        .await
        .with_status(StatusCode::CREATED);
    // Let the search index task see z.ics before it is deleted; an index
    // task that runs after the deletion leaves orphaned index entries.
    test.wait_for_tasks().await;
    key_client
        .request("DELETE", "/dav/cal/plain@example.com/default/z.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // Key account to non-key account, and back.
    key_client
        .request_with_headers(
            "COPY",
            "/dav/cal/key1@example.com/default/y.ics",
            [("destination", "/dav/cal/plain@example.com/default/y.ics")],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    key_client
        .request_with_headers(
            "MOVE",
            "/dav/cal/plain@example.com/default/x.ics",
            [("destination", "/dav/cal/key1@example.com/default/x.ics")],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Nothing moved.
    plain_client
        .request("GET", "/dav/cal/plain@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::OK);
    plain_client
        .request("GET", "/dav/cal/plain@example.com/default/y.ics", "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    key_client
        .request("GET", "/dav/cal/key1@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    plain_client
        .acl(
            "/dav/cal/plain@example.com/default/",
            "/dav/pal/key1%40example.com/",
            [],
        )
        .await
        .with_status(StatusCode::OK);

    key_client
        .request("DELETE", "/dav/cal/key1@example.com/default/y.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", "/dav/cal/plain@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // `plain` outlives this module: drop the calendar its PUT created.
    plain_client
        .request("DELETE", "/dav/cal/plain@example.com/default", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
