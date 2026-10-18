/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key pinning spec 9: the pinning cases, with alice, bob and carol as key
//! accounts and the drain on. Each case puts the accounts into the state it
//! names and leaves no substituted key, fail-closed account or withheld
//! entry behind; pin records and pinned wraps may remain.

use super::{
    STRONG,
    drain::{DrainOn, body},
    fanout::{
        CONTENT_TYPE, NO_REPLY, Party, START_A, START_B, attendee_statuses, invite, uid_copies,
    },
};
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        HookParked, copy_href, inbox_ids, new_inbox_items, pending_ids, pin_record, raw_pins,
        rewrap_legacy, set_raw_pins, substitute_public_key, trigger_drain, vault_revision,
        wrap_mark, write_pin_record, writer_key, za_unpin,
    },
};
use common::{auth::vault::ZaMark, za::test_hooks};
use groupware::cache::GroupwareCache;
use hyper::{Method, StatusCode};
use std::future::Future;
use store::write::now;
use types::collection::SyncCollection;
use vault::pins::{PinSource, ZaPinRecord, ZaWithheld};

pub const ALICE: &str = "key1@example.com";
pub const BOB: &str = "key2@example.com";
pub const CAROL: &str = "key3@example.com";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access key pinning tests...");
    let _drain = DrainOn::new();
    case_01_first_use_under_a_legacy_wrap(test).await;
    case_03_repair(test).await;
    case_09_healing(test).await;
    case_16_pin_before_deliver(test).await;
    rf_2_mixed_recipients(test).await;
    rf_3_concurrent_first_use(test).await;
}

/// Spec 9 case 1: alice, who never pinned, invites bob. The fan-out pins
/// bob's served key as first use in a fresh record, rewraps alice's
/// private key in the same batch (the one generation bump pinning costs)
/// and delivers; a later first use pins without touching the vault (spec
/// 4.4, 5.1).
async fn case_01_first_use_under_a_legacy_wrap(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    za_unpin(test, alice.id, STRONG).await;
    let revision = vault_revision(test, alice.id).await;
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let path = event_path(&alice, "pin-01");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-01", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(new_inbox_items(test, bob.id, &bob_start).await.len(), 1);
    let record = pin_record(test, alice.id, STRONG)
        .await
        .expect("the first pin creates the record");
    let pin = record.pin(BOB).expect("bob is pinned");
    assert_eq!(
        (pin.source, pin.public_key),
        (PinSource::FirstUse, served(test, bob.id).await)
    );
    assert!(record.withheld.is_empty());
    assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned);
    assert_eq!(
        vault_revision(test, alice.id).await,
        revision + 1,
        "one batch, one bump"
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
    // alice invites carol: carol is pinned and the vault is not written.
    let path_b = event_path(&alice, "pin-01b");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_b,
            [CONTENT_TYPE],
            invite("pin-01b", START_B, &[CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(
        pin_record(test, alice.id, STRONG)
            .await
            .unwrap()
            .pin(CAROL)
            .is_some()
    );
    assert_eq!(
        vault_revision(test, alice.id).await,
        revision + 1,
        "later pin writes leave the vault alone"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    delete_event(&alice, &path_b).await;
    clear_recipient(test, &bob, &bob_start, "pin-01").await;
    clear_recipient(test, &carol, &carol_start, "pin-01b").await;
}

/// Spec 9 case 3: bob's next DAV request, with no item pending, repairs his
/// substituted stored key in one batch (spec 3.3); his record exists and
/// opens, so `key_repaired_at` is set, and the legacy wrap under it is
/// healed in the same batch (spec 4.4).
async fn case_03_repair(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    // bob pins alice first.
    za_unpin(test, bob.id, STRONG).await;
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    let revision = vault_revision(test, bob.id).await;
    assert!(pending_ids(test, bob.id).await.is_empty());
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let stored = test
        .server
        .za_vault_record(bob.id)
        .await
        .unwrap()
        .unwrap()
        .record
        .public_key;
    assert_eq!(
        stored, original,
        "the stored key is rewritten from the private key"
    );
    assert_eq!(
        vault_revision(test, bob.id).await,
        revision + 1,
        "one batch: the repair and the heal"
    );
    assert_eq!(wrap_mark(test, bob.id, STRONG).await, ZaMark::Pinned);
    let record = pin_record(test, bob.id, STRONG)
        .await
        .expect("the record survives");
    assert!(record.key_repaired_at.is_some());
    assert!(record.pin(ALICE).is_some());
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert_eq!(
        vault_revision(test, bob.id).await,
        revision + 1,
        "a matching key writes nothing"
    );
}

/// Spec 9 case 9: a legacy wrap under a usable record (a replayed wrap,
/// spec 2 limit 2) is healed by the next writer, in one batch with what it
/// writes: (a) the fan-out's record write, (b) the session's first drain
/// pass. Then, under the pinned wrap with the record deleted, the fan-out
/// fails closed and writes no record (spec 4.4).
async fn case_09_healing(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    // carol holds a record pinning alice.
    za_unpin(test, carol.id, STRONG).await;
    let path_alice = event_path(&carol, "pin-09a");
    carol
        .client
        .request_with_headers(
            "PUT",
            &path_alice,
            [CONTENT_TYPE],
            invite_from(CAROL, "pin-09a", START_A, &[ALICE]),
        )
        .await
        .with_status(StatusCode::CREATED);

    // (a) The fan-out heals; carol's drain is skipped, so no self-check
    // heals first.
    rewrap_legacy(test, carol.id, STRONG).await;
    let revision_a = vault_revision(test, carol.id).await;
    test_hooks::set_drain_skip(carol.id);
    let path = event_path(&carol, "pin-09");
    carol
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite_from(CAROL, "pin-09", START_B, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    test_hooks::clear_drain_skip(carol.id);
    assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Pinned);
    assert_eq!(
        vault_revision(test, carol.id).await,
        revision_a + 1,
        "one batch: the record and the rewrap"
    );
    let record = pin_record(test, carol.id, STRONG).await.unwrap();
    assert!(record.pin(ALICE).is_some() && record.pin(BOB).is_some());

    // (b) The drain heals.
    rewrap_legacy(test, carol.id, STRONG).await;
    let revision_b = vault_revision(test, carol.id).await;
    assert_eq!(trigger_drain(CAROL, STRONG).await, 207);
    assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Pinned);
    assert_eq!(vault_revision(test, carol.id).await, revision_b + 1);
    assert_eq!(
        pin_record(test, carol.id, STRONG).await.unwrap().pins,
        record.pins,
        "a heal changes no pin"
    );

    // Fail closed: the pinned wrap with the record deleted.
    set_raw_pins(test, carol.id, None).await;
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&carol.client, &path, |body| body
            .replace("fo-title pin-09", "fo-title pin-09 v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(attendee_statuses(&carol.client, &path, BOB).await, ["3.8"]);
    assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
    assert_eq!(
        raw_pins(test, carol.id).await,
        None,
        "failing closed writes no record"
    );

    // Cleanup.
    delete_event(&carol, &path_alice).await;
    delete_event(&carol, &path).await;
    clear_recipient(test, &alice, &alice_start, "pin-09a").await;
    clear_recipient(test, &bob, &bob_start, "pin-09").await;
    za_unpin(test, carol.id, STRONG).await;
}

/// Spec 9 case 16: pins are written before items (spec 5.2). When the
/// phase 2 write fails, a new contact gets `5.1`, no item and no pin;
/// alice's next edit pins and delivers.
async fn case_16_pin_before_deliver(test: &TestServer) {
    const NEW_CONTACT: &str = "key3+pin16@example.com";
    let (alice, bob, carol) = parties(test);
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    // alice holds a record.
    za_unpin(test, alice.id, STRONG).await;
    let path_bob = event_path(&alice, "pin-16");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_bob,
            [CONTENT_TYPE],
            invite("pin-16", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(pin_record(test, alice.id, STRONG).await.is_some());

    let before = inbox_ids(test, carol.id).await;
    let path = event_path(&alice, "pin-16b");
    test_hooks::set_pin_write_fail(Some(alice.id));
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-16b", START_B, &[NEW_CONTACT]),
        )
        .await
        .with_status(StatusCode::CREATED);
    test_hooks::set_pin_write_fail(None);
    assert_eq!(
        attendee_statuses(&alice.client, &path, NEW_CONTACT).await,
        ["5.1"]
    );
    assert!(
        new_inbox_items(test, carol.id, &before).await.is_empty(),
        "no item without its pin"
    );
    assert!(
        pin_record(test, alice.id, STRONG)
            .await
            .unwrap()
            .pin(NEW_CONTACT)
            .is_none()
    );

    // alice's next edit pins and delivers.
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-16b", "fo-title pin-16b v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path, NEW_CONTACT).await,
        ["1.2"]
    );
    assert_eq!(new_inbox_items(test, carol.id, &before).await.len(), 1);

    // Cleanup.
    delete_event(&alice, &path_bob).await;
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-16").await;
    clear_recipient(test, &carol, &carol_start, "pin-16b").await;
}

/// Review Focus 2: a refusal is per recipient, never the whole fan-out.
/// With bob's key substituted and carol's not, one edit refuses only bob
/// (`3.8`, one entry, for the edited document) and delivers to carol; once
/// bob's key is restored, the next edit delivers to bob and retires the
/// entry.
async fn rf_2_mixed_recipients(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    za_unpin(test, alice.id, STRONG).await;
    let path = event_path(&alice, "pin-rf2");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-rf2", START_A, &[BOB, CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);

    let (bob_before, carol_before) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-rf2", "fo-title pin-rf2 v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
    assert_eq!(
        attendee_statuses(&alice.client, &path, CAROL).await,
        ["1.2"]
    );
    assert!(new_inbox_items(test, bob.id, &bob_before).await.is_empty());
    assert_eq!(
        new_inbox_items(test, carol.id, &carol_before).await.len(),
        1
    );
    assert_eq!(
        (
            withheld(test, alice.id, BOB).await.len(),
            withheld(test, alice.id, CAROL).await.len()
        ),
        (1, 0)
    );
    let entry = &withheld(test, alice.id, BOB).await[0];
    assert_eq!(
        (entry.document_id, entry.uid.as_str()),
        (document_id_of(test, alice.id, &path).await, "pin-rf2"),
        "the entry names the source event (invariant 23)"
    );

    // bob's key restored, alice edits again: delivered and the entry
    // retired.
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-rf2 v2", "fo-title pin-rf2 v3"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-rf2").await;
    clear_recipient(test, &carol, &carol_start, "pin-rf2").await;
}

/// Review Focus 3: two sessions of one sender first-use different contacts
/// at once. The fan-out parked after its reads loses the phase 2 write,
/// re-reads and re-decides: both pins exist, both items are written and
/// nobody gets `5.1` (spec 4.2).
async fn rf_3_concurrent_first_use(test: &TestServer) {
    const NEW_CONTACT: &str = "key2+rf3@example.com";
    let (alice, bob, carol) = parties(test);
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    // alice holds a record pinning bob only, under the pinned wrap: her
    // next session's self-check heals the legacy wrap za_unpin left.
    za_unpin(test, alice.id, STRONG).await;
    write_pin_record(
        test,
        alice.id,
        STRONG,
        &record_pinning(BOB, served(test, bob.id).await),
    )
    .await;
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned);

    let path_a = event_path(&alice, "pin-rf3a");
    let path_b = event_path(&alice, "pin-rf3b");
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_pin_decide_pause,
        put_raw(ALICE, &path_a, invite("pin-rf3a", START_A, &[CAROL])),
    )
    .await;
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_b,
            [CONTENT_TYPE],
            invite("pin-rf3b", START_B, &[NEW_CONTACT]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(parked.finish().await, 201);
    let record = pin_record(test, alice.id, STRONG).await.unwrap();
    assert!(
        record.pin(CAROL).is_some() && record.pin(NEW_CONTACT).is_some(),
        "the loser re-read and re-decided"
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path_a, CAROL).await,
        ["1.2"]
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path_b, NEW_CONTACT).await,
        ["1.2"]
    );

    // Cleanup.
    delete_event(&alice, &path_a).await;
    delete_event(&alice, &path_b).await;
    clear_recipient(test, &bob, &bob_start, "pin-rf3b").await;
    clear_recipient(test, &carol, &carol_start, "pin-rf3a").await;
}

/// alice, bob and carol, in that order.
pub fn parties(test: &TestServer) -> (Party, Party, Party) {
    (
        Party::new(test, ALICE),
        Party::new(test, BOB),
        Party::new(test, CAROL),
    )
}

/// GETs `href` as `client`, applies `edit` to the body and PUTs the result
/// back, as `set_partstat` does. Returns the PUT's status.
pub async fn rewrite(
    client: &DummyWebDavClient,
    href: &str,
    edit: impl FnOnce(String) -> String,
) -> StatusCode {
    let current = body(client, href).await;
    client
        .request_with_headers("PUT", href, [CONTENT_TYPE], edit(current))
        .await
        .status
}

/// A pin record holding one `FirstUse` pin of `key` for `address`.
pub fn record_pinning(address: &str, key: [u8; 32]) -> ZaPinRecord {
    let mut record = ZaPinRecord::new();
    record.set_pin(address, key, PinSource::FirstUse, now() as i64);
    record
}

/// The withheld entries of the account's pin record for `address` (as
/// stored, normalized), read under the MK `STRONG` unlocks; none when the
/// record is absent or unusable.
pub async fn withheld(test: &TestServer, account_id: u32, address: &str) -> Vec<ZaWithheld> {
    pin_record(test, account_id, STRONG)
        .await
        .map(|record| {
            record
                .withheld
                .into_iter()
                .filter(|entry| entry.address == address)
                .collect()
        })
        .unwrap_or_default()
}

/// The account's served public key (key pinning spec 5.1); it must have one.
pub async fn served(test: &TestServer, account_id: u32) -> [u8; 32] {
    test.server
        .za_served_public_key(account_id)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("account {account_id} serves no public key"))
}

/// The href of the event `name` in the party's default calendar.
pub fn event_path(party: &Party, name: &str) -> String {
    format!(
        "/dav/cal/{}/default/{name}.ics",
        party.name.replace('@', "%40")
    )
}

/// `fanout::invite` with `organizer` as its ORGANIZER.
pub fn invite_from(organizer: &str, uid: &str, start: &str, attendees: &[&str]) -> String {
    invite(uid, start, attendees).replace(
        "ORGANIZER:mailto:key1@example.com",
        &format!("ORGANIZER:mailto:{organizer}"),
    )
}

/// A PUT of `body` to `path` as `name` (password `STRONG`), through the
/// raw client (5 s) as `trigger_drain` sends, so a request parked at a
/// pause point is not cut off by `DummyWebDavClient`'s 500 ms timeout.
/// Resolves to the status.
pub fn put_raw(
    name: &str,
    path: &str,
    body: String,
) -> impl Future<Output = u16> + Send + 'static + use<> {
    let request = HttpRequest::with_credentials(8899, name, STRONG);
    let path = path.to_string();
    async move {
        request
            .send_full(
                Method::PUT,
                &path,
                Some(body.into_bytes()),
                Some(CONTENT_TYPE.1),
            )
            .await
            .status
            .as_u16()
    }
}

/// The document id of the account's event at `href`, from the resource
/// cache as `copy_href` reads it.
pub async fn document_id_of(test: &TestServer, account_id: u32, href: &str) -> u32 {
    let resources = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    // A container's href ends with '/', an event's never, so a calendar
    // sharing the event's id does not match.
    resources
        .resources
        .iter()
        .map(|resource| resource.document_id)
        .find(|id| {
            resources
                .format_resource_paths_by_id(*id)
                .any(|path| path == href)
        })
        .unwrap_or_else(|| panic!("{href} is not in the resource cache"))
}

/// Cleanup: the organizer deletes its event at `href` with
/// `Schedule-Reply: F` (no CANCEL).
async fn delete_event(organizer: &Party, href: &str) {
    organizer
        .client
        .request_with_headers("DELETE", href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Cleanup: the recipient drains, deletes its copy of `uid` with
/// `Schedule-Reply: F` and every inbox item written since `before`.
async fn clear_recipient(test: &TestServer, party: &Party, before: &[u32], uid: &str) {
    assert_eq!(trigger_drain(party.name, STRONG).await, 207);
    if uid_copies(test, party.id, uid).await > 0 {
        let href = copy_href(test, party.id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    party.delete_items_since(test, before).await;
}
