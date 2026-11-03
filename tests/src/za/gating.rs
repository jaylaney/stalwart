/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    dav_seal::{raw_event, raw_event_records},
    drain::DrainOn,
    fanout::{NO_REPLY, uid_copies},
};
use crate::utils::{
    http::HttpRequest,
    jmap::{JmapResponse, JmapUtils},
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        SERVER_URL, clear_received_scheduling, copy_href, delete_inbox_item, inbox_ids,
        inbox_items, mail_count, new_inbox_items, pending_ids, plant_event, trigger_drain,
        wait_for_delivery,
    },
};
use common::auth::oauth::GrantType;
use dav_proto::schema::property::{DavProperty, PrincipalProperty};
use groupware::cache::GroupwareCache;
use groupware::calendar::{
    CalendarEvent, CalendarEventContent, ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
    itip::{ItipIngest, RsvpError, RsvpRequest, RsvpResponse},
};
use hyper::{Method, StatusCode};
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
    // `CalendarEvent/copy` gates both of its accounts. Here the target is
    // plain's account, which key1 reaches through the grant above, so only
    // the source gate (`fromAccountId` is the key account) can refuse it.
    let response = key1
        .jmap_method_call(
            "CalendarEvent/copy",
            json!({
                "accountId": plain.id_string(),
                "fromAccountId": key1.id_string(),
                "create": {}
            }),
        )
        .await;
    assert_eq!(
        method_error(&response),
        Some("accountNotSupportedByMethod"),
        "{:?}",
        response.0
    );

    // Invitations spec 8: scheduling URLs are advertised for key accounts
    // again (release 1 withheld them).
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
        .with_status(StatusCode::OK)
        .is_not_empty();
    props
        .properties(principal)
        .get(&outbox)
        .with_status(StatusCode::OK)
        .is_not_empty();
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

    // DAV OPTIONS (invitations spec 8): `calendar-auto-schedule` is advertised
    // to a key account again; a non-key or anonymous request keeps upstream's
    // header.
    let key_dav = dav_header(&key_client).await;
    assert!(key_dav.contains("calendar-access"), "{key_dav}");
    assert!(key_dav.contains("calendar-auto-schedule"), "{key_dav}");
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
    // capability; a non-key account's does. 1.0 hides other principals
    // unless directory queries are allowed (`Sharing.allowDirectoryQueries`,
    // off by default; 0.16 listed them), so the query runs with them on.
    let set_directory_queries = async |allow: bool| {
        admin
            .registry_update_setting(
                registry::schema::structs::Sharing {
                    allow_directory_queries: allow,
                    ..Default::default()
                },
                &[registry::schema::prelude::Property::AllowDirectoryQueries],
            )
            .await;
        admin.reload_settings().await;
    };
    set_directory_queries(true).await;
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
    // 1.0 fills `accounts` only for a principal the caller can access, so
    // the key account's own objects are read by the key account itself; an
    // account object lists its capabilities under `accountCapabilities`.
    assert!(key_principal["accounts"].is_null(), "{key_principal}");
    let own = key1
        .jmap_method_call(
            "Principal/get",
            json!({
                "accountId": key1.id_string(),
                "ids": [key1.id_string()],
                "properties": ["id", "accounts", "capabilities"],
            }),
        )
        .await;
    let key_principal = own
        .list()
        .iter()
        .find(|principal| principal["id"] == key1.id_string())
        .unwrap_or_else(|| panic!("own principal: {:?}", own.0))
        .clone();
    assert!(
        key_principal["capabilities"].get(calendars).is_none(),
        "{key_principal}"
    );
    assert!(
        key_principal["accounts"][key1.id_string()]["accountCapabilities"]
            .get(calendars)
            .is_none(),
        "{key_principal}"
    );
    assert!(
        key_principal["accounts"][key1.id_string()]["accountCapabilities"]
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
        plain_principal["accounts"][plain.id_string()]["accountCapabilities"]
            .get(calendars)
            .is_some(),
        "{plain_principal}"
    );
    set_directory_queries(false).await;

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
            "event": null,
            // 1.0's busy period carries the account it came from.
            "accountId": null
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
    // recipient's mailbox, so a count shows what the sender sent.
    let plain_mail = mail_count(test, plain_id).await;
    let key_mail = mail_count(test, key1_id).await;
    // key1's inbox items before this test, so it deletes only its own.
    let key_items = inbox_ids(test, key1_id).await;

    // Organizer is a key account (invitations spec 5.1, `SendSealed`): the
    // schedule tag is set as upstream sets it. plain is a non-key account,
    // of iMIP's mail class: mailed after the commit (iMIP spec 5.4, 8.5).
    // The PUT mails plain: the raw client (5 s, PR review T-2).
    let response = HttpRequest::with_credentials(8899, "key1@example.com", STRONG)
        .send_full(
            Method::PUT,
            &format!("{key_cal}invite.ics"),
            Some(KEY_INVITE.as_bytes().to_vec()),
            Some(CONTENT_TYPE.1),
        )
        .await;
    assert_eq!(response.status.as_u16(), 201, "{}", response.body);
    assert!(
        response.header("schedule-tag").is_some(),
        "{:?}",
        response.headers
    );
    wait_for_delivery(test).await;
    // iMIP spec 5.6, 8.5: the invitation reached plain once; its ingest
    // (upstream's) lists one notification and imports one copy. Its lack of
    // RSVP links is read from a plain local attendee's delivered copy in
    // `imip::outbound`'s `mailed_once` (final review C-10).
    assert_eq!(mail_count(test, plain_id).await, plain_mail + 1);
    assert_eq!(members(&plain_client, plain_inbox).await.len(), 1);
    assert_eq!(uid_copies(test, plain_id, "za-invite-1").await, 1);

    // RSVP page: a token for a key account's event is refused as an invalid
    // link, before the event is read.
    let document_id = test
        .server
        .fetch_groupware_resources(key1_id, key1_id, SyncCollection::Calendar)
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

    // The stored event carries the schedule tag a CANCEL on DELETE needs
    // (`delete_all` in groupware's calendar storage).
    // At 1.0 the schedule tag is on the row (`raw_event_records`).
    let (records, _) = raw_event_records(test, key1_id, "default/invite.ics").await;
    assert!(
        records
            .meta
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_some()
    );
    key_client
        .request_with_headers("DELETE", &format!("{key_cal}invite.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // A legacy plaintext event with a schedule tag and a visible attendee:
    // a key account's DELETE builds its CANCEL (invitations spec 5.5), and
    // the fan-out hands plain, a non-key account, to the mailer after the
    // commit (iMIP spec 5.5, 8.5).
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
        plain_mail + 2,
        "the CANCEL did not reach plain"
    );
    // What plain received from the key organizer, set aside from the RSVP
    // reply counted below.
    let plain_received = members(&plain_client, plain_inbox).await;

    // Attendee is a key account: the invitation (and its cancellation) never
    // reaches its calendar or the DAV listing of its scheduling inbox. The
    // email itself lands in its mailbox, as the mail-count asserts below
    // show, and delivery turns it into a mail-borne pending item, which the
    // listing hides (iMIP spec 4.1, 4.5, 8.5).
    let (key_before, key_pending) = (
        inbox_ids(test, key1_id).await,
        pending_ids(test, key1_id).await,
    );
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
    // The invitation email itself was delivered, so the empty listing and
    // calendar below are not timing.
    assert_eq!(mail_count(test, key1_id).await, key_mail + 1);
    // iMIP spec 4.1, 4.5, 8.5: at store level, one more pending item, a
    // mail-borne one (the drain is skipped, so nothing imports it).
    assert_eq!(
        pending_ids(test, key1_id).await.len(),
        key_pending.len() + 1,
        "the plain organizer's invitation left no pending item"
    );
    let invited = new_inbox_items(test, key1_id, &key_before).await;
    assert!(
        invited.len() == 1
            && invited[0].1.changed_by == ChangedBy::CalendarAddress(String::new())
            && invited[0].1.flags & EVENT_NOTIFICATION_ZA_PENDING != 0,
        "the new item is not one mail-borne pending item"
    );
    assert_eq!(members(&key_client, key_inbox).await, Vec::<String>::new());
    assert_eq!(members(&key_client, key_cal).await, Vec::<String>::new());
    // iMIP spec 4.3, 4.5, 8.5: key1's drain imports the item as a copy and
    // keeps the item, drained, as the mail-borne history upstream's ingest
    // writes. The copy and the item go before the RSVP block below plants
    // its own copy of the same UID.
    {
        let _drain = DrainOn::new();
        assert_eq!(trigger_drain("key1@example.com", STRONG).await, 207);
        assert_eq!(
            pending_ids(test, key1_id).await,
            key_pending,
            "the drain left the invitation pending"
        );
        let copy = copy_href(test, key1_id, "za-invite-2").await;
        let drained = inbox_items(test, key1_id)
            .await
            .into_iter()
            .find(|(id, _)| *id == invited[0].0)
            .map(|(_, item)| item)
            .expect("the drained item");
        assert!(
            drained.changed_by == ChangedBy::CalendarAddress(String::new())
                && drained.flags & EVENT_NOTIFICATION_ZA_PENDING == 0,
            "the item is not one drained mail-borne item"
        );
        key_client
            .request_with_headers("DELETE", &copy, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        delete_inbox_item(test, &key_client, key1_id, invited[0].0).await;
    }

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
        .fetch_groupware_resources(plain_id, plain_id, SyncCollection::Calendar)
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
    assert_eq!(
        response.etag(),
        etag,
        "the key attendee's copy was rewritten (ETag changed)"
    );
    let (archive, _) = raw_event(test, key1_id, "default/copy.ics").await;
    let stored = archive
        .unarchive::<CalendarEventContent>()
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
    // through the mail queue: exactly one notification beside what the key
    // organizer mailed plain (iMIP spec 8.5).
    let replies = members(&plain_client, plain_inbox)
        .await
        .into_iter()
        .filter(|member| !plain_received.contains(member))
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 1, "{replies:?}");
    for member in replies {
        plain_client
            .request("DELETE", &member, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    let before_cancel = inbox_ids(test, key1_id).await;
    plain_client
        .request("DELETE", &format!("{plain_cal}invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    // The CANCEL email was delivered too, and makes no item: the drain
    // consumed the invitation's item and its copy is gone, so the UID is
    // unknown at delivery (iMIP spec 4.1 step 5: `EventNotFound`).
    assert_eq!(mail_count(test, key1_id).await, key_mail + 2);
    assert_eq!(
        inbox_ids(test, key1_id).await,
        before_cancel,
        "the CANCEL of an unknown UID made an item"
    );
    assert_eq!(members(&key_client, key_inbox).await, Vec::<String>::new());
    assert_eq!(members(&key_client, key_cal).await, Vec::<String>::new());
    // iMIP spec 8.5: this test's mail-borne items go with it.
    for id in inbox_ids(test, key1_id).await {
        if !key_items.contains(&id) {
            delete_inbox_item(test, &key_client, key1_id, id).await;
        }
    }
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // iMIP spec 8.5: `plain` outlives the suite, so what it received goes
    // for the final emptiness check (its mail, the notifications and the
    // calendar its ingest created).
    clear_received_scheduling(test, &plain).await;
}

/// `test_metadata`'s accounts.
const METADATA_KEY: &str = "keymail33@example.com";
const METADATA_PLAIN: &str = "plainmail33@example.com";

/// The capabilities `test_metadata`'s JMAP requests use: object metadata is
/// a property layer of the mail methods, active only when the request uses
/// its capability.
const METADATA_USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:metadata",
];

/// A shared metadata value under a vendor namespace (the only kind 1.0
/// registers no namespace for: `REGISTERED_NAMESPACES` is empty, and
/// `vendorNamespaces` is on by default).
fn metadata_value() -> Value {
    json!({ "example.com": { "note": "metadata-canary" } })
}

/// The upgrade design's object-metadata refusals (Task 6, ruling 66): a key
/// account gets no JMAP object metadata and no IMAP METADATA, and an
/// ordinary account keeps upstream's (the controls).
///
/// - `Mailbox/set` with metadata on a key account is refused per object
///   (`forbidden`, `ZA_METADATA_REFUSED`), and the same call without it
///   succeeds; an update naming metadata is refused too.
/// - `Mailbox/get` naming the metadata property answers objects without it,
///   and `Mailbox/query` with a metadata filter answers `unsupportedFilter`.
/// - The session advertises no metadata capability for the key account.
/// - IMAP `GETMETADATA`, `SETMETADATA` and `LIST … RETURN (METADATA …)`
///   answer `NO [NOPERM]` in the key account's session, and its `CAPABILITY`
///   lists neither `METADATA` nor `LIST-METADATA`.
///
/// The ordinary account's identical requests succeed, so each refusal is
/// the fork's and not an unsupported feature. `metadata_beyond_mailboxes`
/// adds the review's cases (Task 6 fix round, F6): Email, `Principal/get`,
/// `*/changes`, the caller rule and `ContactCard/copy`.
pub async fn test_metadata(test: &mut TestServer) {
    use crate::utils::imap::{ImapConnection, Type};
    use imap_proto::ResponseType;
    use jmap::api::metadata::ZA_METADATA_REFUSED;

    println!("Running zero-access object metadata refusal tests...");
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(
            METADATA_KEY,
            STRONG,
            "Metadata",
            &[],
            super::user_permissions(),
        )
        .await;
    let plain = admin
        .create_user_account(
            METADATA_PLAIN,
            STRONG,
            "Metadata",
            &[],
            super::user_permissions(),
        )
        .await;

    // JMAP: Mailbox/set create, with and without metadata.
    for account in [&key, &plain] {
        let is_key = account.name() == METADATA_KEY;
        let with = account
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/set", {
                    "accountId": account.id_string(),
                    "create": { "m": { "name": "with-metadata", "metadata": metadata_value() } }
                }, "0"]]),
            )
            .await;
        let response = with.method_response();
        if is_key {
            assert_eq!(
                response["notCreated"]["m"]["type"], "forbidden",
                "a key account's Mailbox/set with metadata: {:?}",
                with.0
            );
            assert_eq!(
                response["notCreated"]["m"]["description"], ZA_METADATA_REFUSED,
                "{:?}",
                with.0
            );
        } else {
            assert!(
                response["created"]["m"]["id"].is_string(),
                "the control's Mailbox/set with metadata: {:?}",
                with.0
            );
        }
        let without = account
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/set", {
                    "accountId": account.id_string(),
                    "create": { "m": { "name": "without-metadata" } }
                }, "0"]]),
            )
            .await;
        let id = without.method_response()["created"]["m"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("Mailbox/set without metadata: {:?}", without.0))
            .to_string();

        // An update naming metadata.
        let update = account
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/set", {
                    "accountId": account.id_string(),
                    "update": { (id.as_str()): { "metadata": metadata_value() } }
                }, "0"]]),
            )
            .await;
        let response = update.method_response();
        if is_key {
            assert_eq!(
                response["notUpdated"][id.as_str()]["type"],
                "forbidden",
                "a key account's Mailbox/set update with metadata: {:?}",
                update.0
            );
        } else {
            assert!(
                response["updated"].get(id.as_str()).is_some(),
                "the control's Mailbox/set update with metadata: {:?}",
                update.0
            );
        }

        // A get naming the metadata property, and a metadata filter.
        let get = account
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/get", {
                    "accountId": account.id_string(),
                    "ids": [&id],
                    "properties": ["id", "name", "metadata"]
                }, "0"]]),
            )
            .await;
        let object = &get.method_response()["list"][0];
        assert_eq!(object["id"], id.as_str(), "Mailbox/get: {:?}", get.0);
        if is_key {
            assert!(
                object.get("metadata").is_none(),
                "a key account's Mailbox/get carries metadata: {:?}",
                get.0
            );
        } else {
            assert_eq!(
                object["metadata"],
                metadata_value(),
                "the control's Mailbox/get: {:?}",
                get.0
            );
        }
        let query = account
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/query", {
                    "accountId": account.id_string(),
                    "filter": { "metadataExists": "example.com/note" }
                }, "0"]]),
            )
            .await;
        if is_key {
            assert_eq!(
                method_error(&query),
                Some("unsupportedFilter"),
                "a key account's metadata filter: {:?}",
                query.0
            );
        } else {
            assert_eq!(
                method_error(&query),
                None,
                "the control's metadata filter: {:?}",
                query.0
            );
        }

        // The session's capabilities.
        let session = account.jmap_session_object().await;
        let advertised = session.0["capabilities"]
            .get("urn:ietf:params:jmap:metadata")
            .is_some();
        assert_eq!(
            advertised,
            !is_key,
            "the session's metadata capability for {}: {:?}",
            account.name(),
            session.0["capabilities"]
        );
    }

    // IMAP METADATA.
    for account in [&key, &plain] {
        let is_key = account.name() == METADATA_KEY;
        let mut imap = ImapConnection::connect(b"_m ").await;
        imap.authenticate(account.name(), account.secret()).await;
        imap.send("CAPABILITY").await;
        let capability = imap
            .assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .join("\n");
        let tokens = capability.split_whitespace().collect::<Vec<_>>();
        for name in ["METADATA", "LIST-METADATA"] {
            assert_eq!(
                tokens.contains(&name),
                !is_key,
                "{name} in the CAPABILITY of {}: {capability}",
                account.name()
            );
        }
        for command in [
            "SETMETADATA INBOX (/shared/comment \"metadata-canary\")",
            "GETMETADATA INBOX /shared/comment",
            "LIST \"\" \"*\" RETURN (METADATA (/shared/comment))",
        ] {
            imap.send(command).await;
            if is_key {
                let refused = imap
                    .assert_read(Type::Tagged, ResponseType::No)
                    .await
                    .join("\n");
                assert!(
                    refused.contains("NOPERM"),
                    "{command} in a key account's session: {refused}"
                );
            } else {
                imap.assert_read(Type::Tagged, ResponseType::Ok).await;
            }
        }
        if !is_key {
            imap.send_ok("SETMETADATA INBOX (/shared/comment NIL)")
                .await;
        }
        imap.send_ok("LOGOUT").await;
    }

    metadata_beyond_mailboxes(&key, &plain).await;

    // Nothing of the key account's metadata was stored: the leak scanner's
    // object-metadata rule flags any mailbox container or private container
    // of a key account, its private containers on the control's objects
    // included. The control's containers go with its mailboxes.
    let found = super::leak::scan(test, key.id().document_id()).await;
    assert!(
        found.violations.is_empty(),
        "the key account's records:\n{}",
        found.violations.join("\n")
    );
    for account in [&key, &plain] {
        test.destroy_all_mailboxes(account).await;
    }
    test.wait_for_tasks().await;
    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// The Task 6 review's F6 cases of ruling 66, with `plain` as the control
/// and as the owner of the objects shared with `key`:
///
/// - `Email/set` create and update naming metadata on the key account are
///   refused per object, and the control's succeed. (At 1.0 `Email/import`
///   takes no metadata: its request has no metadata property.)
/// - `Principal/get`'s account objects offer no metadata capability to the
///   key account, for its own principal and for the control's, while the
///   control's own principal offers it.
/// - The caller rule: the key account's private and shared metadata on the
///   control's shared mailbox are refused, though its grant allows them.
/// - `*/changes`: a metadata-only change of the shared mailbox is reported
///   to its owner and not to the key account (`MetadataChanges::Unsupported`),
///   which still sees the mailbox's other changes.
/// - Ruling 66's copies (F3): `ContactCard/copy` of the control's card that
///   carries metadata into the key account is refused per object, rather
///   than created without the card's container; a card without metadata is
///   copied.
async fn metadata_beyond_mailboxes(
    key: &crate::utils::account::Account,
    plain: &crate::utils::account::Account,
) {
    use crate::utils::account::Account;
    use email::mailbox::INBOX_ID;
    use jmap::api::metadata::ZA_METADATA_REFUSED;
    use types::id::Id;

    println!("metadata_beyond_mailboxes");
    let inbox = Id::from(INBOX_ID).to_string();
    let email = |subject: &str| {
        json!({
            "mailboxIds": { (inbox.as_str()): true },
            "from": [{ "email": "sender@example.com" }],
            "subject": subject,
            "textBody": [{ "partId": "t", "type": "text/plain" }],
            "bodyValues": { "t": { "value": "metadata body" } }
        })
    };

    // Email/set create and update.
    for account in [key, plain] {
        let is_key = account.name() == METADATA_KEY;
        let mut with = email("with metadata");
        with["metadata"] = metadata_value();
        let reply = account
            .jmap_request(
                METADATA_USING,
                json!([["Email/set", {
                    "accountId": account.id_string(),
                    "create": { "e": with, "f": email("without metadata") }
                }, "0"]]),
            )
            .await;
        let response = reply.method_response();
        if is_key {
            assert_eq!(
                response["notCreated"]["e"]["type"], "forbidden",
                "a key account's Email/set create with metadata: {:?}",
                reply.0
            );
            assert_eq!(
                response["notCreated"]["e"]["description"], ZA_METADATA_REFUSED,
                "{:?}",
                reply.0
            );
        } else {
            assert!(
                response["created"]["e"]["id"].is_string(),
                "the control's Email/set create with metadata: {:?}",
                reply.0
            );
        }
        let id = response["created"]["f"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("Email/set create without metadata: {:?}", reply.0))
            .to_string();
        let reply = account
            .jmap_request(
                METADATA_USING,
                json!([["Email/set", {
                    "accountId": account.id_string(),
                    "update": { (id.as_str()): { "metadata": metadata_value() } }
                }, "0"]]),
            )
            .await;
        let response = reply.method_response();
        if is_key {
            assert_eq!(
                response["notUpdated"][id.as_str()]["type"],
                "forbidden",
                "a key account's Email/set update with metadata: {:?}",
                reply.0
            );
            assert_eq!(
                response["notUpdated"][id.as_str()]["description"],
                ZA_METADATA_REFUSED,
                "{:?}",
                reply.0
            );
        } else {
            assert!(
                response["updated"].get(id.as_str()).is_some(),
                "the control's Email/set update with metadata: {:?}",
                reply.0
            );
        }
    }

    // The control shares a mailbox with the key account, with the right to
    // set metadata.
    let reply = plain
        .jmap_request(
            METADATA_USING,
            json!([["Mailbox/set", {
                "accountId": plain.id_string(),
                "create": { "s": {
                    "name": "shared-with-key",
                    "shareWith": { (key.id_string()): {
                        "mayReadItems": true,
                        "maySetMetadata": true
                    } }
                } }
            }, "0"]]),
        )
        .await;
    let shared = reply.method_response()["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the shared mailbox: {:?}", reply.0))
        .to_string();

    // Principal/get: the key account's own principal and the control's, as
    // the key account reads them, and the control's own (offered).
    const PRINCIPAL_USING: &[&str] = &[
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:principals",
        "urn:ietf:params:jmap:metadata",
    ];
    for (caller, ids, offered) in [
        (key, vec![key.id_string(), plain.id_string()], false),
        (plain, vec![plain.id_string()], true),
    ] {
        let reply = caller
            .jmap_request(
                PRINCIPAL_USING,
                json!([["Principal/get", {
                    "accountId": caller.id_string(),
                    "ids": &ids,
                    "properties": ["id", "accounts"]
                }, "0"]]),
            )
            .await;
        let list = reply.method_response()["list"]
            .as_array()
            .unwrap_or_else(|| panic!("Principal/get: {:?}", reply.0));
        assert_eq!(list.len(), ids.len(), "Principal/get: {:?}", reply.0);
        let own = list
            .iter()
            .find(|principal| principal["id"] == caller.id_string())
            .unwrap_or_else(|| panic!("the caller's own principal: {:?}", reply.0));
        assert!(
            own["accounts"].is_object(),
            "the caller's own principal lists no accounts: {:?}",
            reply.0
        );
        for principal in list {
            assert_eq!(
                principal["accounts"]
                    .to_string()
                    .contains("urn:ietf:params:jmap:metadata"),
                offered && principal["id"] == caller.id_string(),
                "the metadata capability in {}'s Principal/get of {}: {:?}",
                caller.name(),
                principal["id"],
                reply.0
            );
        }
    }

    // The caller rule: the grant allows both, ruling 66 refuses both.
    for property in ["privateMetadata", "metadata"] {
        let reply = key
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/set", {
                    "accountId": plain.id_string(),
                    "update": { (shared.as_str()): { (property): metadata_value() } }
                }, "0"]]),
            )
            .await;
        let response = reply.method_response();
        assert_eq!(
            response["notUpdated"][shared.as_str()]["description"],
            ZA_METADATA_REFUSED,
            "the key account's {property} on the control's shared mailbox: {:?}",
            reply.0
        );
    }
    // The control's own private metadata on the same mailbox is stored:
    // the key account's refusal is the fork's, not the grant's.
    let reply = plain
        .jmap_request(
            METADATA_USING,
            json!([["Mailbox/set", {
                "accountId": plain.id_string(),
                "update": { (shared.as_str()): { "privateMetadata": metadata_value() } }
            }, "0"]]),
        )
        .await;
    assert!(
        reply.method_response()["updated"]
            .get(shared.as_str())
            .is_some(),
        "the control's private metadata: {:?}",
        reply.0
    );

    // `*/changes`: a metadata-only change of the shared mailbox.
    let state = async |caller: &Account| {
        let reply = caller
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/get", {
                    "accountId": plain.id_string(),
                    "ids": [&shared]
                }, "0"]]),
            )
            .await;
        reply.method_response()["state"]
            .as_str()
            .unwrap_or_else(|| panic!("Mailbox/get: {:?}", reply.0))
            .to_string()
    };
    let changes = async |caller: &Account, since: &str| {
        let reply = caller
            .jmap_request(
                METADATA_USING,
                json!([["Mailbox/changes", {
                    "accountId": plain.id_string(),
                    "sinceState": since
                }, "0"]]),
            )
            .await;
        reply.method_response()["updated"]
            .as_array()
            .unwrap_or_else(|| panic!("Mailbox/changes: {:?}", reply.0))
            .clone()
    };
    let key_since = state(key).await;
    let plain_since = state(plain).await;
    let reply = plain
        .jmap_request(
            METADATA_USING,
            json!([["Mailbox/set", {
                "accountId": plain.id_string(),
                "update": { (shared.as_str()): { "metadata": metadata_value() } }
            }, "0"]]),
        )
        .await;
    assert!(
        reply.method_response()["updated"]
            .get(shared.as_str())
            .is_some(),
        "the control's metadata-only change: {:?}",
        reply.0
    );
    assert!(
        changes(plain, &plain_since).await.contains(&json!(shared)),
        "the owner's Mailbox/changes omits its metadata-only change"
    );
    assert!(
        !changes(key, &key_since).await.contains(&json!(shared)),
        "the key account's Mailbox/changes reports a metadata-only change"
    );
    let reply = plain
        .jmap_request(
            METADATA_USING,
            json!([["Mailbox/set", {
                "accountId": plain.id_string(),
                "update": { (shared.as_str()): { "name": "shared-with-key-renamed" } }
            }, "0"]]),
        )
        .await;
    assert!(
        reply.method_response()["updated"]
            .get(shared.as_str())
            .is_some(),
        "the rename: {:?}",
        reply.0
    );
    assert!(
        changes(key, &key_since).await.contains(&json!(shared)),
        "the key account does not see the shared mailbox's changes, so the \
         metadata-only case above proves nothing"
    );

    // ContactCard/copy into the key account (ruling 66, F3).
    const CONTACTS_USING: &[&str] = &[
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:contacts",
        "urn:ietf:params:jmap:metadata",
    ];
    let book = |account: &Account, name: &str, share: Option<&Account>| {
        let mut object = json!({ "name": name });
        if let Some(grantee) = share {
            object["shareWith"] = json!({ (grantee.id_string()): { "mayRead": true } });
        }
        json!([["AddressBook/set", {
            "accountId": account.id_string(),
            "create": { "b": object }
        }, "0"]])
    };
    let created = |reply: &JmapResponse, id: &str| {
        reply.method_response()["created"][id]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: {:?}", reply.0))
            .to_string()
    };
    let reply = plain
        .jmap_request(CONTACTS_USING, book(plain, "copy-source", Some(key)))
        .await;
    let source_book = created(&reply, "b");
    let reply = key
        .jmap_request(CONTACTS_USING, book(key, "copy-destination", None))
        .await;
    let destination_book = created(&reply, "b");
    let reply = plain
        .jmap_request(
            CONTACTS_USING,
            json!([["ContactCard/set", {
                "accountId": plain.id_string(),
                "create": {
                    "m": {
                        "uid": "za-metadata-copy-m",
                        "name": { "full": "Card with metadata" },
                        "addressBookIds": { (source_book.as_str()): true },
                        "metadata": metadata_value()
                    },
                    "n": {
                        "uid": "za-metadata-copy-n",
                        "name": { "full": "Card without metadata" },
                        "addressBookIds": { (source_book.as_str()): true }
                    }
                }
            }, "0"]]),
        )
        .await;
    let with_metadata = created(&reply, "m");
    let without_metadata = created(&reply, "n");
    let reply = key
        .jmap_request(
            CONTACTS_USING,
            json!([["ContactCard/copy", {
                "fromAccountId": plain.id_string(),
                "accountId": key.id_string(),
                // 1.0 reads the source from each entry's `id` (RFC 8620 5.4).
                "create": {
                    (with_metadata.as_str()): {
                        "id": with_metadata,
                        "addressBookIds": { (destination_book.as_str()): true }
                    },
                    (without_metadata.as_str()): {
                        "id": without_metadata,
                        "addressBookIds": { (destination_book.as_str()): true }
                    }
                }
            }, "0"]]),
        )
        .await;
    let response = reply.method_response();
    assert_eq!(
        response["notCreated"][with_metadata.as_str()]["description"],
        ZA_METADATA_REFUSED,
        "ContactCard/copy of a card with metadata into a key account: {:?}",
        reply.0
    );
    assert!(
        response["created"][without_metadata.as_str()]["id"].is_string(),
        "ContactCard/copy of a card without metadata into a key account: {:?}",
        reply.0
    );
    // The account destruction at the end of `test_metadata` removes the
    // address books and cards.
}
