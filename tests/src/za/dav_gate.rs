/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use hyper::StatusCode;

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

    // Cross-account COPY where either side is a key account: 403 (both directions).
    // `plain` has no email address, so `webdav_client()` cannot be used.
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    plain_client
        .request_with_headers(
            "PUT",
            "/dav/cal/plain@example.com/default/x.ics",
            [("content-type", "text/calendar")],
            crate::webdav::TEST_ICAL_1,
        )
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .request_with_headers(
            "COPY",
            "/dav/cal/plain@example.com/default/x.ics",
            [("destination", "/dav/cal/key1@example.com/default/x.ics")],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    let key_client =
        DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    key_client
        .request_with_headers(
            "PUT",
            "/dav/cal/key1@example.com/default/y.ics",
            [("content-type", "text/calendar")],
            crate::webdav::TEST_ICAL_1,
        )
        .await
        .with_status(StatusCode::CREATED);
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
