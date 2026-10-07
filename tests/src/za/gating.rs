/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{
    jmap::{JmapResponse, JmapUtils},
    server::TestServer,
    webdav::DummyWebDavClient,
};
use dav_proto::schema::property::{DavProperty, PrincipalProperty};
use hyper::StatusCode;
use serde_json::{Value, json};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");

/// One busy hour inside the availability window used below.
const BUSY_EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-gating-busy\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240301T100000Z\r\nDTEND:20240301T110000Z\r\nSUMMARY:busy\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

const OUTBOX_FREEBUSY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VFREEBUSY\r\nUID:fb-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240101T000000Z\r\nDTEND:20240131T000000Z\r\nORGANIZER:mailto:plain@example.com\r\nATTENDEE:mailto:key1@example.com\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";

/// The `type` of a single method-level error, or `None` for a method result.
fn method_error(response: &JmapResponse) -> Option<&str> {
    let call = &response.0["methodResponses"][0];
    (call[0] == "error").then(|| call[1]["type"].as_str().unwrap_or_default())
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access sharing and JMAP gating tests...");
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
    let key_cal = "/dav/cal/key1%40example.com/default/";
    let plain_cal = "/dav/cal/plain%40example.com/gating/";

    // A busy event in each account, in the same window.
    plain_client
        .request(
            "MKCALENDAR",
            plain_cal,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>",
        )
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .request_with_headers(
            "PUT",
            &format!("{plain_cal}busy.ics"),
            [CONTENT_TYPE],
            BUSY_EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    key_client
        .request_with_headers(
            "PUT",
            &format!("{key_cal}busy.ics"),
            [CONTENT_TYPE],
            BUSY_EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);

    // Sharing: ACL on a key-owned calendar is refused, even for its owner
    // holding keys; on a non-key calendar it works.
    key_client
        .acl(key_cal, "/dav/pal/plain%40example.com/", ["read"])
        .await
        .with_status(StatusCode::FORBIDDEN);
    plain_client
        .acl(plain_cal, "/dav/pal/key1%40example.com/", ["read"])
        .await
        .with_status(StatusCode::OK);
    // A key account may read a calendar shared with it by a non-key account.
    key_client
        .request("PROPFIND", plain_cal, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);

    // Scheduling URLs are not advertised for key accounts.
    let inbox = DavProperty::Principal(PrincipalProperty::ScheduleInboxURL);
    let outbox = DavProperty::Principal(PrincipalProperty::ScheduleOutboxURL);
    let home = DavProperty::Principal(PrincipalProperty::CalendarHomeSet);
    let principal = "/dav/pal/key1%40example.com/";
    let props = key_client
        .propfind(principal, [inbox.clone(), outbox.clone(), home.clone()])
        .await;
    props
        .properties(principal)
        .get(&inbox)
        .with_status(StatusCode::NOT_FOUND);
    props
        .properties(principal)
        .get(&outbox)
        .with_status(StatusCode::NOT_FOUND);
    props
        .properties(principal)
        .get(&home)
        .with_status(StatusCode::OK)
        .is_not_empty();
    let plain_principal = "/dav/pal/plain%40example.com/";
    let props = plain_client
        .propfind(plain_principal, [inbox.clone(), outbox.clone()])
        .await;
    for property in [&inbox, &outbox] {
        props
            .properties(plain_principal)
            .get(property)
            .with_status(StatusCode::OK)
            .is_not_empty();
    }

    // Scheduling outbox: a free-busy request naming a key account answers
    // 3.7 for that recipient.
    let response = plain_client
        .request_with_headers(
            "POST",
            "/dav/itip/plain%40example.com/outbox/",
            [CONTENT_TYPE],
            OUTBOX_FREEBUSY,
        )
        .await
        .with_status(StatusCode::OK);
    let items = response
        .xml
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "A:schedule-response.A:response.A:recipient.D:href"
                    | "A:schedule-response.A:response.A:request-status"
                    | "A:schedule-response.A:response.A:calendar-data"
            )
        })
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        items,
        [
            "mailto:key1@example.com",
            "3.7;Invalid calendar user or insufficient permissions"
        ],
        "{response:?}"
    );

    // JMAP: calendars are not advertised for the key account, and stay
    // advertised for a non-key account (also as key1's secondary account).
    let session = key1.jmap_session_object().await.into_inner();
    let caps = &session["accounts"][key1.id_string()]["accountCapabilities"];
    for capability in [
        "urn:ietf:params:jmap:calendars",
        "urn:ietf:params:jmap:calendars:parse",
    ] {
        assert!(caps.get(capability).is_none(), "{capability}: {caps}");
        assert!(
            session["primaryAccounts"].get(capability).is_none(),
            "{capability}: {session}"
        );
    }
    assert!(
        caps.get("urn:ietf:params:jmap:contacts").is_some(),
        "{caps}"
    );
    assert!(
        session["accounts"][plain.id_string()]["accountCapabilities"]
            .get("urn:ietf:params:jmap:calendars")
            .is_some(),
        "{session}"
    );
    let plain_session = plain.jmap_session_object().await.into_inner();
    assert!(
        plain_session["accounts"][plain.id_string()]["accountCapabilities"]
            .get("urn:ietf:params:jmap:calendars")
            .is_some(),
        "{plain_session}"
    );
    assert!(
        plain_session["primaryAccounts"]
            .get("urn:ietf:params:jmap:calendars")
            .is_some(),
        "{plain_session}"
    );

    // JMAP calendar methods on the key account are refused.
    let key1_account = key1.id_string();
    for (method, args) in [
        ("Calendar/get", json!({ "accountId": key1_account })),
        ("Calendar/query", json!({ "accountId": key1_account })),
        (
            "Calendar/set",
            json!({ "accountId": key1_account, "create": {} }),
        ),
        (
            "Calendar/changes",
            json!({ "accountId": key1_account, "sinceState": "n" }),
        ),
        ("CalendarEvent/get", json!({ "accountId": key1_account })),
        ("CalendarEvent/query", json!({ "accountId": key1_account })),
        (
            "CalendarEvent/set",
            json!({ "accountId": key1_account, "create": {} }),
        ),
        (
            "CalendarEvent/copy",
            json!({ "accountId": key1_account, "fromAccountId": key1_account, "create": {} }),
        ),
        (
            "CalendarEvent/parse",
            json!({ "accountId": key1_account, "blobIds": [] }),
        ),
        (
            "CalendarEvent/changes",
            json!({ "accountId": key1_account, "sinceState": "n" }),
        ),
        (
            "CalendarEvent/queryChanges",
            json!({ "accountId": key1_account, "sinceQueryState": "n" }),
        ),
        (
            "ParticipantIdentity/get",
            json!({ "accountId": key1_account }),
        ),
        (
            "ParticipantIdentity/set",
            json!({ "accountId": key1_account, "create": {} }),
        ),
        (
            "CalendarEventNotification/get",
            json!({ "accountId": key1_account }),
        ),
        (
            "CalendarEventNotification/query",
            json!({ "accountId": key1_account }),
        ),
        (
            "CalendarEventNotification/set",
            json!({ "accountId": key1_account, "destroy": [] }),
        ),
        (
            "CalendarEventNotification/changes",
            json!({ "accountId": key1_account, "sinceState": "n" }),
        ),
        (
            "CalendarEventNotification/queryChanges",
            json!({ "accountId": key1_account, "sinceQueryState": "n" }),
        ),
    ] {
        let response = key1.jmap_method_call(method, args).await;
        assert_eq!(
            method_error(&response),
            Some("accountNotSupportedByMethod"),
            "{method}: {:?}",
            response.0
        );
    }
    // Controls: the same calls succeed on a non-key account.
    let plain_account = plain.id_string();
    for (method, args) in [
        ("Calendar/get", json!({ "accountId": plain_account })),
        ("CalendarEvent/query", json!({ "accountId": plain_account })),
    ] {
        let response = plain.jmap_method_call(method, args).await;
        assert_eq!(method_error(&response), None, "{method}: {:?}", response.0);
    }

    // Availability: an impersonating administrator (a member of every
    // account) sees the non-key account's busy hour and nothing of the key
    // account's, although both hold the same event.
    let availability = |id: &str| {
        json!({
            "accountId": admin.id_string(),
            "id": id,
            "utcStart": "2024-03-01T00:00:00Z",
            "utcEnd": "2024-03-02T00:00:00Z",
        })
    };
    admin
        .jmap_method_call("Principal/getAvailability", availability(plain_account))
        .await
        .list_array()
        .assert_is_equal(json!([{
            "utcStart": "2024-03-01T10:00:00Z",
            "utcEnd": "2024-03-01T11:00:00Z",
            "busyStatus": "confirmed",
            "event": null
        }]));
    admin
        .jmap_method_call("Principal/getAvailability", availability(key1_account))
        .await
        .list_array()
        .assert_is_equal(Value::Array(vec![]));

    // No linked-blob download case: calendars and events are stored inline
    // and never linked as blobs, so no `BlobClass::Linked` id for them
    // passes `blob_has_access`; the gate in `has_access_blob` is defensive.

    // Clean up: drop the grant, the events and plain's calendar.
    plain_client
        .acl(plain_cal, "/dav/pal/key1%40example.com/", [])
        .await
        .with_status(StatusCode::OK);
    test.wait_for_tasks().await;
    key_client
        .request("DELETE", &format!("{key_cal}busy.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
