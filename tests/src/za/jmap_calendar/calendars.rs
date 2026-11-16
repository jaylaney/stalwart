/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{Pair, finish, make_calendar, pair, share};
use crate::{
    utils::{server::TestServer, webdav::DummyWebDavClient, za::za_post},
    za::{
        STRONG,
        dav_seal::{raw_calendar, resource_id},
        fanout::NO_REPLY,
        gating::method_error,
    },
};
use hyper::StatusCode;
use serde_json::json;
use types::id::Id;

const SLUG: &str = "jmapcal-work";
const PATH: &str = "/dav/cal/jmapcal-calendars%40example.com/jmapcal-work/";
const OWNER_NAME: &str = "jmapcal owner name";
const READER_NAME: &str = "jmapcal reader name";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar tests...");
    let pair = pair(test, "calendars").await;
    get_shows_own_entry(test, &pair).await;
    pair.dav
        .request_with_headers("DELETE", PATH, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    finish(test, "calendars").await;
}

async fn set_display_name(dav: &DummyWebDavClient, name: &str) {
    let patch = format!(
        "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>{name}</D:displayname></D:prop></D:set></D:propertyupdate>"
    );
    let reply = dav
        .request("PROPPATCH", PATH, patch)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let body = reply.body.unwrap();
    assert!(body.contains("200 OK"), "{body}");
}

async fn get_shows_own_entry(test: &TestServer, pair: &Pair) {
    let shared_id = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    set_display_name(&pair.dav, OWNER_NAME).await;
    share(test, &pair.key, &pair.key2, pair.key2_id, SLUG, "edit").await;
    set_display_name(&pair.dav2, READER_NAME).await;

    let shared = Id::from(shared_id).to_string();
    let account_id = pair.key.id_string();
    let reply = pair
        .key
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": [shared] }),
        )
        .await;
    assert_eq!(reply.list()[0]["name"], OWNER_NAME, "{reply:?}");

    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": null }),
        )
        .await;
    let [calendar] = reply.list() else {
        panic!("Expected exactly the shared calendar: {reply:?}");
    };
    assert_eq!(calendar["id"], shared);
    assert_eq!(calendar["name"], READER_NAME);
    assert_eq!(calendar["myRights"]["mayReadItems"], true);

    let default = Id::from(resource_id(test, pair.key_id, "default").await).to_string();
    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": [default] }),
        )
        .await;
    assert!(reply.list().is_empty(), "{reply:?}");
    assert_eq!(
        reply.response_at(0)["notFound"],
        json!([default]),
        "{reply:?}"
    );

    let raw = raw_calendar(test, pair.key_id, SLUG).await;
    let raw = String::from_utf8_lossy(raw.as_bytes());
    assert!(!raw.contains(OWNER_NAME) && !raw.contains(READER_NAME));

    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": pair.key.name(), "password": STRONG,
            "calendar": SLUG, "address": pair.key2.name()
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), Some("accountNotFound"), "{reply:?}");
}
