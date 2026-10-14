/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 3.3, "Every rewrite carries the pairs": a key account's
//! own PUT and PROPPATCH keep the token pairs of the stored `X-ZA-EXTRA`
//! bundle, which the unsealed view a client edits cannot show.

use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{plant_event, plant_sealed_event, za_unlock},
};
use groupware::calendar::{
    CalendarEvent,
    seal::{TOKEN_LEN, TokenPairs, archived_event_is_sealed, event_token_pairs},
};
use hyper::StatusCode;
use vault::session::SessionKeys;

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const PAIR_A: (&str, [u8; TOKEN_LEN]) = ("pair-a@example.com", *b"pairs-token-a-01");
const PAIR_B: (&str, [u8; TOKEN_LEN]) = ("pair-b@example.com", *b"pairs-token-b-02");

/// A key organizer's event with two attendees.
const EVENT: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
    "UID:za-pairs-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990101T100000Z\r\n",
    "DTEND:20990101T110000Z\r\nSEQUENCE:0\r\nSUMMARY:pairs-canary\r\n",
    "ORGANIZER:mailto:key1@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:pair-a@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:pair-b@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// The pairs in the stored event's bundle, opened as the product opens them.
async fn stored_pairs(
    test: &TestServer,
    account_id: u32,
    keys: &SessionKeys,
    path: &str,
) -> Option<TokenPairs> {
    let (archive, _) = raw_event(test, account_id, path).await;
    event_token_pairs(&archive, keys, account_id).unwrap()
}

fn assert_both(pairs: Option<TokenPairs>, after: &str) {
    let pairs = pairs.unwrap_or_else(|| panic!("no token pairs after {after}"));
    assert_eq!(pairs.len(), 2, "pairs after {after}");
    for (address, token) in [PAIR_A, PAIR_B] {
        assert!(
            pairs.matches(address, &token),
            "{address} lost its token after {after}"
        );
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access token pair carry-forward tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let keys = za_unlock(test, id, STRONG).await;
    let path = "/dav/cal/key1%40example.com/default/pairs.ics";
    let mut pairs = TokenPairs::new();
    for (address, token) in [PAIR_A, PAIR_B] {
        pairs.insert(address, token);
    }
    plant_sealed_event(test, id, "default", "pairs.ics", EVENT, STRONG, &pairs).await;
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "planting",
    );
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        body.contains("pairs-canary") && !body.contains("X-ZA-"),
        "{body}"
    );

    // An alarm-only PUT builds no iTIP message; the bundle is rebuilt from
    // the view's fields and the stored pairs.
    let with_alarm = EVENT.replace(
        "END:VEVENT",
        "BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nDESCRIPTION:pairs-alarm\r\nEND:VALARM\r\nEND:VEVENT",
    );
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], with_alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "an alarm-only PUT",
    );
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        body.contains("pairs-alarm") && !body.contains("X-ZA-"),
        "{body}"
    );

    // A PROPPATCH of the display name rewrites the bundle the same way.
    client
        .proppatch(path, [("D:displayname", "pairs-name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_both(
        stored_pairs(test, id, &keys, "default/pairs.ics").await,
        "a PROPPATCH",
    );
    client
        .propfind(path, ["D:displayname"])
        .await
        .properties(path)
        .get("D:displayname")
        .with_values(["pairs-name"]);

    // Review Focus 5: events without pairs take the same rewrites. A release
    // 1 event (its PROPPATCH writes a format 1 bundle) keeps its display name
    // through a PUT; a legacy plaintext event is sealed by its PUT. Neither
    // gains pairs.
    let r1 = "/dav/cal/key1%40example.com/default/pairs-r1.ics";
    let r1_body = EVENT.replace("za-pairs-1", "za-pairs-r1");
    client
        .request_with_headers("PUT", r1, [CONTENT_TYPE], r1_body.clone())
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(r1, [("D:displayname", "pairs-r1-name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    client
        .request_with_headers(
            "PUT",
            r1,
            [CONTENT_TYPE],
            r1_body.replace("pairs-canary", "pairs-r1-changed"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        stored_pairs(test, id, &keys, "default/pairs-r1.ics")
            .await
            .is_none()
    );
    client
        .propfind(r1, ["D:displayname"])
        .await
        .properties(r1)
        .get("D:displayname")
        .with_values(["pairs-r1-name"]);

    let legacy = "/dav/cal/key1%40example.com/default/pairs-legacy.ics";
    let legacy_body = EVENT.replace("za-pairs-1", "za-pairs-legacy");
    plant_event(test, id, "default", "pairs-legacy.ics", &legacy_body, None).await;
    client
        .request_with_headers(
            "PUT",
            legacy,
            [CONTENT_TYPE],
            legacy_body.replace("pairs-canary", "pairs-legacy-changed"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (archive, _) = raw_event(test, id, "default/pairs-legacy.ics").await;
    assert!(archived_event_is_sealed(
        archive.unarchive::<CalendarEvent>().unwrap()
    ));
    assert!(event_token_pairs(&archive, &keys, id).unwrap().is_none());

    test.wait_for_tasks_skip_not_due().await;
    for path in [path, r1, legacy] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}
