/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    jmap::{JmapResponse, JmapUtils},
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{SERVER_URL, mail_count, plant_event, wait_for_delivery},
};
use common::auth::oauth::GrantType;
use dav_proto::schema::property::{DavProperty, PrincipalProperty};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent,
        itip::{ItipIngest, RsvpError, RsvpRequest, RsvpResponse},
    },
};
use hyper::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;
use types::collection::SyncCollection;

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");

/// One busy hour inside the availability window used below.
const BUSY_EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-gating-busy\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240301T100000Z\r\nDTEND:20240301T110000Z\r\nSUMMARY:busy\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

const OUTBOX_FREEBUSY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VFREEBUSY\r\nUID:fb-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240101T000000Z\r\nDTEND:20240131T000000Z\r\nORGANIZER:mailto:plain@example.com\r\nATTENDEE:mailto:key1@example.com\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";

/// The `type` of a single method-level error, or `None` for a method result.
fn method_error(response: &JmapResponse) -> Option<&str> {
    let call = &response.0["methodResponses"][0];
    (call[0] == "error").then(|| call[1]["type"].as_str().unwrap_or_default())
}

/// The `DAV` header of an OPTIONS request on the calendar root.
async fn dav_header(client: &DummyWebDavClient) -> String {
    client
        .request("OPTIONS", "/dav/cal/", "")
        .await
        .with_status(StatusCode::OK)
        .header("dav")
        .to_string()
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

    // DAV OPTIONS: calendar-auto-schedule is not advertised to a key
    // account (spec 9); a non-key or anonymous request keeps upstream's header.
    let key_dav = dav_header(&key_client).await;
    assert!(key_dav.contains("calendar-access"), "{key_dav}");
    assert!(!key_dav.contains("calendar-auto-schedule"), "{key_dav}");
    let plain_dav = dav_header(&plain_client).await;
    assert!(plain_dav.contains("calendar-auto-schedule"), "{plain_dav}");
    let anonymous = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .request(reqwest::Method::OPTIONS, format!("{SERVER_URL}/dav/cal/"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::OK);
    let anonymous_dav = anonymous.headers()["dav"].to_str().unwrap();
    assert!(
        anonymous_dav.contains("calendar-auto-schedule"),
        "{anonymous_dav}"
    );

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

    // Principal/get: a key account's principal advertises no calendars
    // capability; a non-key account's does.
    let response = plain
        .jmap_method_call(
            "Principal/get",
            json!({
                "accountId": plain.id_string(),
                "ids": [key1.id_string(), plain.id_string()],
                "properties": ["id", "accounts", "capabilities"],
            }),
        )
        .await;
    let principal = |id: &str| {
        response
            .list()
            .iter()
            .find(|principal| principal["id"] == id)
            .unwrap_or_else(|| panic!("{id}: {:?}", response.0))
            .clone()
    };
    let calendars = "urn:ietf:params:jmap:calendars";
    let key_principal = principal(key1.id_string());
    assert!(
        key_principal["capabilities"].get(calendars).is_none(),
        "{key_principal}"
    );
    assert!(
        key_principal["accounts"][key1.id_string()]
            .get(calendars)
            .is_none(),
        "{key_principal}"
    );
    assert!(
        key_principal["accounts"][key1.id_string()]
            .get("urn:ietf:params:jmap:contacts")
            .is_some(),
        "{key_principal}"
    );
    let plain_principal = principal(plain.id_string());
    assert!(
        plain_principal["capabilities"].get(calendars).is_some(),
        "{plain_principal}"
    );
    assert!(
        plain_principal["accounts"][plain.id_string()]
            .get(calendars)
            .is_some(),
        "{plain_principal}"
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

/// Invitation from the key account to the non-key account.
const KEY_INVITE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-invite-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:invite-canary\r\nORGANIZER:mailto:key1@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:plain@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Invitation from the non-key account to the key account.
const PLAIN_INVITE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-invite-2\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990103T090000Z\r\nDTEND:20990103T100000Z\r\nSUMMARY:invite-canary\r\nORGANIZER:mailto:plain@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key1@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Hrefs of a depth-1 PROPFIND other than the collection itself.
async fn members(client: &DummyWebDavClient, href: &str) -> Vec<String> {
    client
        .request_with_headers("PROPFIND", href, [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .filter(|member| *member != href)
        .map(str::to_string)
        .collect()
}

pub async fn test_scheduling(test: &mut TestServer) {
    println!("Running zero-access scheduling gating tests...");
    let key1 = test.account("key1@example.com").clone();
    let key1_id = key1.id().document_id();
    let key_client =
        DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let plain = test.account("plain@example.com").clone();
    // `plain` has no email address, so `webdav_client()` cannot be used.
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let key_cal = "/dav/cal/key1%40example.com/default/";
    let key_inbox = "/dav/itip/key1%40example.com/inbox/";
    let plain_cal = "/dav/cal/plain%40example.com/scheduling/";
    let plain_inbox = "/dav/itip/plain%40example.com/inbox/";
    let plain_id = plain.id().document_id();
    // Mail counts before anything is sent. iMIP email is delivered to the
    // recipient's mailbox (mail is not sealed in this release), so a count
    // that does not move shows the sender sent nothing.
    let plain_mail = mail_count(test, plain_id).await;
    let key_mail = mail_count(test, key1_id).await;

    // Organizer is a key account: stored, nothing sent, no schedule tag.
    let response = key_client
        .request_with_headers(
            "PUT",
            &format!("{key_cal}invite.ics"),
            [CONTENT_TYPE],
            KEY_INVITE,
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(
        response.headers.get("schedule-tag").is_none(),
        "{:?}",
        response.headers
    );
    wait_for_delivery(test).await;
    // Sender side: nothing left the key organizer.
    assert_eq!(mail_count(test, plain_id).await, plain_mail);
    assert_eq!(
        members(&plain_client, plain_inbox).await,
        Vec::<String>::new()
    );

    // RSVP page: a token for a key account's event is refused as an invalid
    // link, before the event is read.
    let document_id = test
        .server
        .fetch_dav_resources(key1_id, key1_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("default/invite.ics")
        .unwrap()
        .document_id();
    for partstat in [None, Some("ACCEPTED".to_string())] {
        let token = test
            .server
            .encode_access_token(
                GrantType::Rsvp,
                key1_id,
                "key1@example.com",
                3600,
                Some(&format!("plain@example.com;{document_id}")),
                None,
            )
            .await
            .unwrap();
        let response = test
            .server
            .http_rsvp_handle(
                RsvpRequest {
                    token,
                    partstat,
                    comment: None,
                },
                "en",
                "127.0.0.1".parse().unwrap(),
            )
            .await
            .unwrap();
        assert!(
            matches!(
                response,
                RsvpResponse::Error {
                    reason: RsvpError::InvalidLink,
                    ..
                }
            ),
            "{response:?}"
        );
    }

    // A CANCEL on DELETE needs a stored schedule tag (`delete_all` in
    // groupware's calendar storage); a key account's events never get one,
    // so deleting this one cannot send anything whatever the DELETE gate does.
    let (archive, _) = raw_event(test, key1_id, "default/invite.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_none()
    );
    key_client
        .request("DELETE", &format!("{key_cal}invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // The DELETE gate itself (`send_itip` off for key accounts): a legacy
    // plaintext event with a schedule tag and a visible attendee is what a
    // CANCEL can be built from, so only the gate stops one here.
    plant_event(
        test,
        key1_id,
        "default",
        "planted-invite.ics",
        &KEY_INVITE.replace("za-invite-1", "za-invite-planted"),
        Some(1),
    )
    .await;
    key_client
        .request("DELETE", &format!("{key_cal}planted-invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, plain_id).await,
        plain_mail,
        "a CANCEL left the key organizer"
    );
    assert_eq!(
        members(&plain_client, plain_inbox).await,
        Vec::<String>::new()
    );

    // Attendee is a key account: the invitation (and its cancellation) never
    // reaches its scheduling inbox or calendar. The email itself may land in
    // its mailbox; mail is not sealed in this release.
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
            &format!("{plain_cal}invite.ics"),
            [CONTENT_TYPE],
            PLAIN_INVITE,
        )
        .await
        .with_status(StatusCode::CREATED);
    wait_for_delivery(test).await;
    // The invitation email itself was delivered, so the empty inbox and
    // calendar below are the ingest gate's doing, not timing.
    assert_eq!(mail_count(test, key1_id).await, key_mail + 1);
    assert_eq!(members(&key_client, key_inbox).await, Vec::<String>::new());
    assert_eq!(members(&key_client, key_cal).await, Vec::<String>::new());

    // RSVP from the key attendee on the non-key organizer's page: the
    // organizer's copy records it, and the key attendee's own copy is never
    // rewritten. A copy written through DAV is sealed and shows no ATTENDEE,
    // so the attendee-copy sync could not match it anyway; a legacy
    // plaintext copy does match, which makes the attendee-copy gate the only
    // thing keeping it unchanged.
    let copy = format!("{key_cal}copy.ics");
    plant_event(test, key1_id, "default", "copy.ics", PLAIN_INVITE, None).await;
    let etag = key_client
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK)
        .etag()
        .to_string();
    let document_id = test
        .server
        .fetch_dav_resources(plain_id, plain_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("scheduling/invite.ics")
        .unwrap()
        .document_id();
    let token = test
        .server
        .encode_access_token(
            GrantType::Rsvp,
            plain_id,
            plain.name(),
            3600,
            Some(&format!("key1@example.com;{document_id}")),
            None,
        )
        .await
        .unwrap();
    let response = test
        .server
        .http_rsvp_handle(
            RsvpRequest {
                token,
                partstat: Some("ACCEPTED".to_string()),
                comment: None,
            },
            "en",
            "127.0.0.1".parse().unwrap(),
        )
        .await
        .unwrap();
    assert!(
        matches!(response, RsvpResponse::Recorded { .. }),
        "{response:?}"
    );
    let organizer_copy = plain_client
        .request("GET", &format!("{plain_cal}invite.ics"), "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        organizer_copy.contains("PARTSTAT=ACCEPTED"),
        "{organizer_copy}"
    );
    let response = key_client
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK);
    assert_eq!(response.etag(), etag);
    let (archive, _) = raw_event(test, key1_id, "default/copy.ics").await;
    let stored = archive
        .unarchive::<CalendarEvent>()
        .unwrap()
        .data
        .event
        .to_string();
    assert!(
        stored.contains("PARTSTAT=NEEDS-ACTION") && !stored.contains("PARTSTAT=ACCEPTED"),
        "the key attendee's copy was rewritten: {stored}"
    );
    key_client
        .request("DELETE", &copy, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // The RSVP reply reaches the organizer's scheduling inbox directly, not
    // through the mail queue: exactly one notification.
    let replies = members(&plain_client, plain_inbox).await;
    assert_eq!(replies.len(), 1, "{replies:?}");
    for member in replies {
        plain_client
            .request("DELETE", &member, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    plain_client
        .request("DELETE", &format!("{plain_cal}invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    // The CANCEL email was delivered too; the ingest gate dropped it.
    assert_eq!(mail_count(test, key1_id).await, key_mail + 2);
    assert_eq!(members(&key_client, key_inbox).await, Vec::<String>::new());
    assert_eq!(members(&key_client, key_cal).await, Vec::<String>::new());
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // Counting `plain`'s mail created its default mailboxes; `plain` outlives
    // the suite, so remove them for the final emptiness check.
    test.destroy_all_mailboxes(&plain).await;
}
