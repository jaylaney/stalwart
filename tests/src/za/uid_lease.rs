/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.3 and 5.1: the UID creation lease in a key account's
//! PUT create, its archive-based uniqueness re-check, its fenced commit and
//! its expiry; a non-key account takes no lease.

use super::{
    STRONG,
    fanout::{KEY1_CAL, MKCALENDAR, NO_REPLY, Party, START_A, invite, uid_copies},
};
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    za::{HookParked, inbox_ids, new_inbox_items, plant_sealed_event, za_uid_leases},
};
use common::za::{lease::ZaUidLease, test_hooks};
use groupware::calendar::seal::TokenPairs;
use hyper::Method;
use std::{future::Future, time::Duration};

/// A PUT with its own client and a 5-second timeout; resolves to the status.
pub fn put(
    name: &str,
    secret: &str,
    path: String,
    body: String,
) -> impl Future<Output = u16> + Send + 'static + use<> {
    let request = HttpRequest::with_credentials(8899, name, secret);
    async move {
        request
            .send_full(
                Method::PUT,
                &path,
                Some(body.into_bytes()),
                Some("text/calendar; charset=utf-8"),
            )
            .await
            .status
            .as_u16()
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access creation lease tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start2 = inbox_ids(test, key2.id).await;
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "a lease outlived an earlier module"
    );

    // (a) Stale snapshot.
    let uid = "za-lease-stale";
    let parked = HookParked::start(
        key1.id,
        test_hooks::set_put_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-stale.ics"),
            invite(uid, START_A, &[]),
        ),
    )
    .await;
    plant_sealed_event(
        test,
        key1.id,
        "default",
        "fo-stale-drained.ics",
        &invite(uid, START_A, &[]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    assert_eq!(
        parked.finish().await,
        412,
        "the stale snapshot let a second copy in"
    );
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the refused PUT kept its lease"
    );

    // (b) Expiry: the held PUT's commit is fenced out.
    let uid = "za-lease-expiry";
    test_hooks::set_lease_secs_override(Some(1));
    let parked = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-expiry.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let lease = ZaUidLease::acquire(test.server.store(), key1.id, uid)
        .await
        .unwrap()
        .expect("an expired lease is taken over");
    plant_sealed_event(
        test,
        key1.id,
        "default",
        "fo-expiry-other.ics",
        &invite(uid, START_A, &[]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    assert!(
        lease.release(test.server.store()).await,
        "the test's lease releases"
    );
    test_hooks::set_lease_secs_override(None);
    assert_eq!(parked.finish().await, 412, "an expired holder committed");
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "the held PUT's item was written before its commit (spec 5.4)"
    );

    // (b') Expiry with no copy: the test takes the expired lease over and
    // releases it without planting. The held PUT's fenced commit fails, its
    // retake finds no stored copy, and it answers 503; its item stays.
    let uid = "za-lease-expiry-none";
    let before2 = inbox_ids(test, key2.id).await;
    test_hooks::set_lease_secs_override(Some(1));
    let parked = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-expiry-none.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let lease = ZaUidLease::acquire(test.server.store(), key1.id, uid)
        .await
        .unwrap()
        .expect("an expired lease is taken over");
    assert!(
        lease.release(test.server.store()).await,
        "the test's lease releases"
    );
    test_hooks::set_lease_secs_override(None);
    assert_eq!(
        parked.finish().await,
        503,
        "a lost fence with no copy did not answer 503"
    );
    assert_eq!(uid_copies(test, key1.id, uid).await, 0);
    assert_eq!(
        new_inbox_items(test, key2.id, &before2).await.len(),
        1,
        "the held PUT's item was written before its commit (spec 5.4)"
    );
    assert_eq!(za_uid_leases(test).await, 0, "the 503 left a lease behind");

    // (c) PUT versus PUT, one session holding the lease in its fan-out.
    let uid = "za-lease-race";
    let parked = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-race-a.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    let second = tokio::spawn(put(
        key1.name,
        STRONG,
        format!("{KEY1_CAL}fo-race-b.ics"),
        invite(uid, START_A, &["key2@example.com"]),
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(parked.finish().await, 201);
    assert_eq!(
        second.await.unwrap(),
        412,
        "two creators of one UID both created"
    );
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);

    // The busy lease: held for the whole retry window, the PUT answers 503.
    let uid = "za-lease-busy";
    let busy = format!("{KEY1_CAL}fo-busy.ics");
    let lease = ZaUidLease::acquire(test.server.store(), key1.id, uid)
        .await
        .unwrap()
        .expect("a free lease");
    assert_eq!(
        put(key1.name, STRONG, busy.clone(), invite(uid, START_A, &[])).await,
        503
    );
    assert!(lease.release(test.server.store()).await);
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the busy PUT left a lease behind"
    );
    assert_eq!(
        put(key1.name, STRONG, busy, invite(uid, START_A, &[])).await,
        201
    );

    // (d) A non-key account takes no lease.
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_cal = "/dav/cal/plain%40example.com/fo-lease/";
    let plain_client = crate::utils::webdav::DummyWebDavClient::new(
        plain_id,
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    plain_client
        .request("MKCALENDAR", plain_cal, MKCALENDAR)
        .await
        .with_status(hyper::StatusCode::CREATED);
    let uid = "za-lease-plain";
    let lease = ZaUidLease::acquire(test.server.store(), plain_id, uid)
        .await
        .unwrap()
        .expect("a free lease");
    let plain_event = invite(uid, START_A, &[]).replace("key1@example.com", "plain@example.com");
    assert_eq!(
        put(
            plain.name(),
            plain.secret(),
            format!("{plain_cal}fo-plain.ics"),
            plain_event
        )
        .await,
        201,
        "the plain PUT waited on a lease"
    );
    assert!(
        lease.release(test.server.store()).await,
        "the plain PUT left the lease as it found it"
    );
    // The plain account's event is indexed by the background task: wait for
    // it before deleting the calendar, or the index keys outlive the delete.
    test.wait_for_tasks().await;
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(hyper::StatusCode::NO_CONTENT);

    // (e) Cross-calendar UID (spec 6.3 step 6): the UID of key1's `fo-busy`
    // (default calendar) into a second calendar of key1 creates its copy, as
    // upstream allows. Unlike the cases above this expects a 201: it pins the
    // per-calendar filter in `za_assert_is_unique_uid`, since dropping that
    // filter would turn this 201 into a 412.
    let second_cal = "/dav/cal/key1%40example.com/fo-lease-second/";
    key1.client
        .request("MKCALENDAR", second_cal, MKCALENDAR)
        .await
        .with_status(hyper::StatusCode::CREATED);
    let before1 = inbox_ids(test, key1.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    assert_eq!(
        put(
            key1.name,
            STRONG,
            format!("{second_cal}fo-second.ics"),
            invite("za-lease-busy", START_A, &["key2@example.com"]),
        )
        .await,
        201,
        "the per-calendar UID rule refused another calendar"
    );
    assert_eq!(uid_copies(test, key1.id, "za-lease-busy").await, 2);
    assert_eq!(new_inbox_items(test, key2.id, &before2).await.len(), 1);
    assert!(new_inbox_items(test, key1.id, &before1).await.is_empty());
    assert_eq!(za_uid_leases(test).await, 0);
    key1.client
        .request_with_headers("DELETE", second_cal, [NO_REPLY], "")
        .await
        .with_status(hyper::StatusCode::NO_CONTENT);

    // Cleanup.
    for name in [
        "fo-stale-drained",
        "fo-expiry-other",
        "fo-race-a",
        "fo-busy",
    ] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(hyper::StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0, "every lease was released");
}
