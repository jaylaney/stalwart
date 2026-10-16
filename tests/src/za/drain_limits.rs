/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.2 ("The cap", "Skipped items") and 10: the drain's per-session limits.

use super::{
    STRONG,
    drain::{DrainOn, request_tree},
    fanout::{CONTENT_TYPE, KEY1_CAL, NO_REPLY, Party, START_A, invite, uid_copies},
};
use crate::utils::{
    server::TestServer,
    za::{
        PlantedItem, copy_href, inbox_ids, pending_ids, plant_inbox_item, trigger_drain,
        za_app_password, za_post,
    },
};
use common::{auth::credential::AppPassword, za::test_hooks};
use hyper::StatusCode;
use serde_json::json;
use std::time::Duration;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access drain limit tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start2 = inbox_ids(test, key2.id).await;

    // 1. A private key that does not open suspends the session's drain
    // (spec 10): a fresh session whose first unwrap fails drains nothing on
    // any later request, backoff or not; another session drains the item.
    let uid1 = "za-limit-1";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-1.ics"),
            [CONTENT_TYPE],
            invite(uid1, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    // A fresh session: the hook fires on a session's first unwrap only, and
    // key2's password session unwrapped its key in an earlier module.
    let app_fail = za_app_password(key2.name, STRONG, "drain limits, failing").await;
    test_hooks::set_private_key_fail(Some(key2.id));
    assert_eq!(
        trigger_drain(key2.name, &app_fail).await,
        207,
        "a private-key failure failed the request"
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "a session without its private key drained"
    );
    test_hooks::set_drain_retry_override(Some(0));
    assert_eq!(trigger_drain(key2.name, &app_fail).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the backoff lifted a permanent deferral"
    );
    test_hooks::set_drain_retry_override(None);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "another session of the account did not drain"
    );
    assert_eq!(uid_copies(test, key2.id, uid1).await, 1);

    // 2. A store error before the match defers the session like one after
    // it (spec 10): with the enumeration failing once, the next request
    // inside the backoff does not drain; after it, the item drains.
    let uid2 = "za-limit-2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    // The deferral is an absolute deadline taken at the failing pass, so
    // the short backoff is set before the failure (as `test_cursor` does).
    test_hooks::set_drain_retry_override(Some(2));
    test_hooks::set_drain_enumerate_fail(Some(key2.id));
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "an enumeration error set no deferral"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);
    test_hooks::set_drain_retry_override(None);

    // 3. An item that fails three times is skipped for the session (spec
    // 6.2, "Skipped items"): a later CANCEL for its UID waits with it, an
    // unrelated item drains, and the skipped item is not retried in this
    // session even once it would succeed; a fresh session applies the
    // REQUEST and then the CANCEL, in order.
    let uid3 = "za-limit-3";
    let organizer3 = format!("{KEY1_CAL}limit-3.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer3,
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let request_item = pending_ids(test, key2.id).await;
    assert_eq!(request_item.len(), 1);
    let request_item = request_item[0];
    // A CANCEL for the same UID (no `Schedule-Reply: F`), then an unrelated
    // invitation.
    key1.client
        .request("DELETE", &organizer3, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let uid4 = "za-limit-4";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-4.ics"),
            [CONTENT_TYPE],
            invite(uid4, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 3);
    test_hooks::set_drain_fail_item(Some((key2.id, request_item)));
    test_hooks::set_drain_retry_override(Some(0));
    for attempt in 1..=2 {
        assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
        assert_eq!(
            pending_ids(test, key2.id).await.len(),
            3,
            "failure {attempt} did not abort the pass"
        );
    }
    // The third failure skips the item; the pass goes on.
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let left = pending_ids(test, key2.id).await;
    assert_eq!(
        left.len(),
        2,
        "the skipped item did not let the pass continue"
    );
    assert!(left.contains(&request_item), "the skipped item was applied");
    assert_eq!(
        uid_copies(test, key2.id, uid3).await,
        0,
        "the CANCEL overtook its REQUEST"
    );
    assert_eq!(
        uid_copies(test, key2.id, uid4).await,
        1,
        "the unrelated item did not drain"
    );
    // Not retried in this session once it would succeed.
    test_hooks::set_drain_fail_item(None);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid3).await,
        0,
        "a skipped item was retried"
    );
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    test_hooks::set_drain_retry_override(None);
    // A fresh session retries: the REQUEST imports, then the CANCEL applies.
    let app_fresh = za_app_password(key2.name, STRONG, "drain limits, fresh").await;
    assert_eq!(trigger_drain(key2.name, &app_fresh).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1);
    let copy3 = copy_href(test, key2.id, uid3).await;
    let cancelled = super::drain::body(&key2.client, &copy3).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");

    // 4. The cap (spec 6.2, "The cap"): 33 pending items drain 32 in one
    // pass; the next request continues at once, with the default backoff
    // in force, and takes an item that arrived in between as well.
    let uids: Vec<String> = (0..33).map(|i| format!("za-limit-c{i}")).collect();
    for (i, uid) in uids.iter().enumerate() {
        plant_inbox_item(
            test,
            key2.id,
            &request_tree(uid, "key1@example.com", "key2@example.com"),
            PlantedItem::Pending {
                pair: Some(("key2@example.com", [i as u8 + 1; 16])),
                reply_token: None,
            },
            key1.id,
            None,
        )
        .await;
    }
    assert_eq!(pending_ids(test, key2.id).await.len(), 33);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the pass ignored the cap"
    );
    let late = "za-limit-c33";
    plant_inbox_item(
        test,
        key2.id,
        &request_tree(late, "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x77; 16])),
            reply_token: None,
        },
        key1.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the continuation waited for the backoff"
    );
    let mut copies = 0;
    for uid in uids.iter().map(String::as_str).chain([late]) {
        copies += uid_copies(test, key2.id, uid).await;
    }
    assert_eq!(copies, 34);

    // Cleanup.
    for name in ["limit-1", "limit-2", "limit-4"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for uid in [uid1, uid2, uid3, uid4]
        .into_iter()
        .chain(uids.iter().map(String::as_str))
        .chain([late])
    {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    for app in [&app_fail, &app_fresh] {
        let credential_id = AppPassword::parse(app).unwrap().credential_id;
        za_post(
            "app-password/revoke",
            &json!({ "username": key2.name, "password": STRONG, "credential_id": credential_id }),
        )
        .await
        .expect(200);
    }
}
