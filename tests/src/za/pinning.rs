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
    drain::{DrainOn, body, partstat_of},
    fanout::{
        CONTENT_TYPE, MKCALENDAR, NO_REPLY, Party, START_A, START_B, attendee_statuses, invite,
        uid_copies,
    },
};
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        HookParked, VaultReply, copy_href, inbox_ids, item_method, item_partstat, new_inbox_items,
        open_pending_item_as, pending_ids, pin_record, raw_pins, restore_wrap, rewrap_legacy,
        saved_wrap, set_raw_pins, substitute_public_key, trigger_drain, vault_revision, wrap_mark,
        write_pin_record, writer_key, za_post, za_unpin,
    },
};
use ::registry::schema::{prelude::Property, structs::CalendarScheduling};
use calcard::{
    common::IanaString,
    icalendar::{
        ICalendar, ICalendarComponent, ICalendarComponentType, ICalendarMethod,
        ICalendarParameterName, ICalendarParameterValue, ICalendarProperty, ICalendarStatus,
        ICalendarValueType,
    },
};
use common::{auth::vault::ZaMark, za::test_hooks};
use groupware::cache::GroupwareCache;
use hyper::{Method, StatusCode};
use serde_json::{Value, json};
use std::future::Future;
use store::write::now;
use types::collection::SyncCollection;
use vault::{
    keys::display_fingerprint,
    pins::{PinSource, ZaPinRecord, ZaWithheld},
};

pub const ALICE: &str = "key1@example.com";
pub const BOB: &str = "key2@example.com";
pub const CAROL: &str = "key3@example.com";
/// Override X of `recurring_invite`, the second occurrence.
const X: &str = START_B;
/// carol's password in case 10, between its two password changes.
pub const NEW: &str = "another horse battery staple 2";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access key pinning tests...");
    let _drain = DrainOn::new();
    case_01_first_use_under_a_legacy_wrap(test).await;
    case_02_refusal(test).await;
    case_03_repair(test).await;
    case_04_confirm_and_redeliver(test).await;
    case_05_forget_keeps_entries(test).await;
    case_06_fail_closed(test).await;
    case_07_repair_keeps_fail_closed(test).await;
    case_08_stale_first_use(test).await;
    case_09_healing(test).await;
    case_10_replay(test).await;
    case_11_ordinary_delivery(test).await;
    case_12_partial_changes(test).await;
    case_13_stated_losses(test).await;
    case_14_delegates(test).await;
    case_15_pre_pin(test).await;
    case_16_pin_before_deliver(test).await;
    case_17_cleanup_races(test).await;
    case_18_event_identity(test).await;
    case_19_participation_filter(test).await;
    case_20_drain_before_build(test).await;
    case_21_recorded_instances(test).await;
    rf_1_address_forms(test).await;
    rf_2_mixed_recipients(test).await;
    rf_3_concurrent_first_use(test).await;
    rf_4_moved_source(test).await;
    rf_5_never_pinned(test).await;
    fr_1_waiting_entry_spends_no_budget(test).await;
    fr_2_failed_cleanup_counts_remaining(test).await;
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

/// Spec 9 case 2: alice's edit after bob's key changed is refused: no item,
/// `3.8` and one withheld entry, listed with the served fingerprint beside
/// the pinned one; a second refusal re-arms the entry with a new refusal id.
/// The fan-out reads the served key fresh from the vault record, never the
/// account cache's `za_public_key` (M48).
async fn case_02_refusal(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    // alice pins bob at first use.
    za_unpin(test, alice.id, STRONG).await;
    let (path, original, writer) = refuse_once(test, &alice, &bob, "pin-02").await;
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
    assert_eq!(
        new_inbox_items(test, bob.id, &bob_start).await.len(),
        1,
        "the invitation only: no item for the refused edit"
    );
    let listing = contacts(ALICE).await;
    let entry = listed(&listing, BOB);
    assert_eq!(
        (entry["withheld"].clone(), entry["source"].clone()),
        (json!(1), json!("first-use"))
    );
    assert_ne!(entry["served_fingerprint"], entry["fingerprint"]);
    let first = withheld(test, alice.id, BOB).await[0].refusal_id;
    // A second edit: one entry, a new refusal id.
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-02 v2", "fo-title pin-02 v3"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
    let again = withheld(test, alice.id, BOB).await;
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].refusal_id, first);
    // The M48 check: bob's key restored with invalidation; his account
    // cache warmed with the real key (a cold cache loads `public_key` from
    // the vault record); the key substituted without invalidation. A
    // fan-out reading the cached key would deliver.
    substitute_public_key(test, bob.id, &original, true).await;
    lookup(ALICE, BOB).await.expect(200);
    substitute_public_key(test, bob.id, &writer, false).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-02 v3", "fo-title pin-02 v4"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path, BOB).await,
        ["3.8"],
        "the served key is read fresh"
    );

    // Cleanup.
    substitute_public_key(test, bob.id, &original, true).await;
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-02").await;
    drop_entries(ALICE, BOB).await;
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

/// Spec 9 case 4: `contacts/pin` refuses a fingerprint other than the
/// served key's, pins the served key as `Confirmed` when it matches (any
/// case and spacing) and redelivers the withheld change as the full current
/// state; bob's drain opens it. At most `REDELIVER_BATCH` (32) entries go
/// per call, the rest are `remaining` for the next.
async fn case_04_confirm_and_redeliver(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let (path, original, writer) = refuse_once(test, &alice, &bob, "pin-04").await;
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(
        pin(ALICE, BOB, &display_fingerprint(&writer))
            .await
            .expect(409)["error"],
        "fingerprint mismatch"
    );
    let fingerprint = lookup(ALICE, BOB).await.str(200, "fingerprint");
    assert_eq!(
        fingerprint,
        display_fingerprint(&served(test, bob.id).await)
    );
    let reply = pin(ALICE, BOB, &fingerprint.to_uppercase().replace(' ', ""))
        .await
        .expect(200);
    assert_eq!(
        reply,
        json!({"ok": true, "delivered": 1, "failed": 0, "dropped": 0, "remaining": 0})
    );
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert!(
        pending_ids(test, bob.id).await.is_empty(),
        "bob's drain opens the redelivered item"
    );
    assert!(
        body(&bob.client, &copy_href(test, bob.id, "pin-04").await)
            .await
            .contains("v2")
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
    assert_eq!(
        pin_record(test, alice.id, STRONG)
            .await
            .unwrap()
            .pin(BOB)
            .unwrap()
            .source,
        PinSource::Confirmed
    );
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // The batch limit: 33 events created while bob's key is substituted,
    // each refused.
    substitute_public_key(test, bob.id, &writer, true).await;
    let uids: Vec<String> = (1..=33).map(|n| format!("pin-04-{n}")).collect();
    for uid in &uids {
        alice
            .client
            .request_with_headers(
                "PUT",
                &event_path(&alice, uid),
                [CONTENT_TYPE],
                invite(uid, START_A, &[BOB]),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 33);
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200),
        json!({"ok": true, "delivered": 32, "failed": 0, "dropped": 0, "remaining": 1})
    );
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200)["delivered"],
        1
    );
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    for uid in &uids {
        delete_event(&alice, &event_path(&alice, uid)).await;
    }
    clear_recipient(test, &bob, &bob_start, "pin-04").await;
    for uid in &uids {
        clear_recipient(test, &bob, &bob_start, uid).await;
    }
}

/// Spec 9 case 5: `contacts/forget` for bob removes his pin and keeps his
/// entry (spec 6.5); alice's next edit pins bob as first use and drains the
/// entry under the two-item rule (spec 5.1). The edit is a master REQUEST,
/// so no separate full state goes ahead of it.
async fn case_05_forget_keeps_entries(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let (path, original, _) = refuse_once(test, &alice, &bob, "pin-05").await;
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(
        forget(ALICE, Some(BOB)).await.expect(200),
        json!({"ok": true})
    );
    assert!(
        pin_record(test, alice.id, STRONG)
            .await
            .unwrap()
            .pin(BOB)
            .is_none()
    );
    assert_eq!(
        withheld(test, alice.id, BOB).await.len(),
        1,
        "forget keeps the entry"
    );
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-05 v2", "fo-title pin-05 v3"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pin_record(test, alice.id, STRONG)
            .await
            .unwrap()
            .pin(BOB)
            .unwrap()
            .source,
        PinSource::FirstUse
    );
    assert_eq!(
        opened(test, bob.id, &before).await.len(),
        1,
        "a master REQUEST: no separate full state"
    );
    assert!(
        withheld(test, alice.id, BOB).await.is_empty(),
        "drained under the two-item rule"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-05").await;
}

/// Spec 9 case 6: under the pinned wrap with the pin record deleted, the
/// fan-out fails closed for every key-account recipient and records
/// nothing (spec 4.4); the listing reports `record_missing`; `contacts/pin`
/// writes a fresh record and ends the state, and `contacts/forget` with
/// null does too, returning every contact to first use.
async fn case_06_fail_closed(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    // alice holds a record pinning bob and carol, under the pinned wrap.
    za_unpin(test, alice.id, STRONG).await;
    for contact in [BOB, CAROL] {
        let fingerprint = lookup(ALICE, contact).await.str(200, "fingerprint");
        pin(ALICE, contact, &fingerprint).await.expect(200);
    }
    assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned);
    set_raw_pins(test, alice.id, None).await;
    let (b, c) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let path = event_path(&alice, "pin-06");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-06", START_A, &[BOB, CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
    assert_eq!(
        attendee_statuses(&alice.client, &path, CAROL).await,
        ["3.8"]
    );
    assert!(
        new_inbox_items(test, bob.id, &b).await.is_empty()
            && new_inbox_items(test, carol.id, &c).await.is_empty()
    );
    assert_eq!(
        raw_pins(test, alice.id).await,
        None,
        "fail closed writes no record"
    );
    let listing = contacts(ALICE).await;
    assert_eq!(
        (listing["record_missing"].clone(), listing["pins"].clone()),
        (json!(true), json!([]))
    );
    let reply = pin(
        ALICE,
        BOB,
        &lookup(ALICE, BOB).await.str(200, "fingerprint"),
    )
    .await
    .expect(200);
    assert_eq!(
        reply["delivered"], 0,
        "refusals while failing closed record nothing"
    );
    assert_eq!(contacts(ALICE).await["record_missing"], false);
    set_raw_pins(test, alice.id, None).await;
    assert_eq!(forget(ALICE, None).await.expect(200), json!({"ok": true}));
    let listing = contacts(ALICE).await;
    assert_eq!(
        (listing["record_missing"].clone(), listing["pins"].clone()),
        (json!(false), json!([]))
    );
    // alice's next edit sends to both, first use.
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-06", "fo-title pin-06 v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["1.2"]);
    assert_eq!(
        attendee_statuses(&alice.client, &path, CAROL).await,
        ["1.2"]
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-06").await;
    clear_recipient(test, &carol, &carol_start, "pin-06").await;
}

/// Spec 9 case 7: a self-check repair under the pinned wrap with no record
/// rewrites the stored key and writes nothing to field 151, so the
/// fail-closed state survives it (invariant 22): the drain's repair, then
/// the listing's (`stored_key_matches` false, the fingerprint derived from
/// the private key), then a refused invitation.
async fn case_07_repair_keeps_fail_closed(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    // alice pinned.
    let fingerprint = lookup(ALICE, BOB).await.str(200, "fingerprint");
    pin(ALICE, BOB, &fingerprint).await.expect(200);
    assert_eq!(wrap_mark(test, alice.id, STRONG).await, ZaMark::Pinned);
    set_raw_pins(test, alice.id, None).await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        stored_key(test, alice.id).await,
        original,
        "the drain's self-check repairs"
    );
    assert_eq!(
        raw_pins(test, alice.id).await,
        None,
        "the repair creates no record"
    );
    substitute_public_key(test, alice.id, &writer_key(), true).await;
    let listing = contacts(ALICE).await;
    assert_eq!(
        (
            listing["stored_key_matches"].clone(),
            listing["record_missing"].clone()
        ),
        (json!(false), json!(true))
    );
    assert_eq!(
        listing["fingerprint"],
        display_fingerprint(&original.clone().try_into().unwrap())
    );
    assert_eq!(
        stored_key(test, alice.id).await,
        original,
        "the listing's self-check repairs"
    );
    assert_eq!(raw_pins(test, alice.id).await, None);
    // alice invites bob.
    let before = inbox_ids(test, bob.id).await;
    let path = event_path(&alice, "pin-07");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-07", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["3.8"]);
    assert!(new_inbox_items(test, bob.id, &before).await.is_empty());
    assert_eq!(raw_pins(test, alice.id).await, None);

    // Cleanup.
    assert_eq!(forget(ALICE, None).await.expect(200), json!({"ok": true}));
    delete_event(&alice, &path).await;
}

/// Spec 9 case 8: a fan-out that read "legacy, no record" before another
/// session's first pin (the record and the rewrap) and the record's
/// deletion loses its batch's vault assertion, re-reads the pinned wrap
/// and fails closed: no first-use record is published (spec 4.4).
async fn case_08_stale_first_use(test: &TestServer) {
    let (alice, _, carol) = parties(test);
    let alice_start = inbox_ids(test, alice.id).await;
    za_unpin(test, carol.id, STRONG).await;
    let path = event_path(&carol, "pin-08");
    let parked = HookParked::start(
        carol.id,
        test_hooks::set_pin_decide_pause,
        put_raw(
            CAROL,
            &path,
            invite_from(CAROL, "pin-08", START_A, &[ALICE]),
        ),
    )
    .await;
    let fingerprint = lookup(CAROL, ALICE).await.str(200, "fingerprint");
    pin(CAROL, ALICE, &fingerprint).await.expect(200);
    assert_eq!(
        wrap_mark(test, carol.id, STRONG).await,
        ZaMark::Pinned,
        "the record and the rewrap"
    );
    set_raw_pins(test, carol.id, None).await;
    assert_eq!(parked.finish().await, 201);
    assert_eq!(
        attendee_statuses(&carol.client, &path, ALICE).await,
        ["3.8"]
    );
    assert!(
        new_inbox_items(test, alice.id, &alice_start)
            .await
            .is_empty()
    );
    assert_eq!(
        raw_pins(test, carol.id).await,
        None,
        "the stale fan-out published no record"
    );

    // Cleanup.
    delete_event(&carol, &path).await;
    za_unpin(test, carol.id, STRONG).await;
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

/// Spec 9 case 10, spec 2 limit 2 (asserted as a limit, not a
/// protection): a store writer holding a pre-pin wrap and an earlier copy
/// of the pin record can replay both. The saved legacy wrap with the record
/// deleted reopens first use; the saved record reverts a confirmed pin to
/// first use, with no generation bump and nothing for the owner to notice.
async fn case_10_replay(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    za_unpin(test, carol.id, STRONG).await;
    let legacy = saved_wrap(test, carol.id).await;
    let path_a = event_path(&carol, "pin-10a");
    carol
        .client
        .request_with_headers(
            "PUT",
            &path_a,
            [CONTENT_TYPE],
            invite_from(CAROL, "pin-10a", START_A, &[ALICE]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Pinned);
    za_post(
        "password",
        &json!({"username": CAROL, "password": STRONG, "new_password": NEW}),
    )
    .await
    .expect(200);
    restore_wrap(test, carol.id, &legacy).await;
    set_raw_pins(test, carol.id, None).await;
    assert_eq!(wrap_mark(test, carol.id, NEW).await, ZaMark::Legacy);

    // carol, logging in with NEW, invites bob: first use again.
    let carol_new = DummyWebDavClient::new(carol.id, CAROL, NEW, CAROL);
    let before = inbox_ids(test, bob.id).await;
    let path_b = event_path(&carol, "pin-10b");
    carol_new
        .request_with_headers(
            "PUT",
            &path_b,
            [CONTENT_TYPE],
            invite_from(CAROL, "pin-10b", START_B, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        new_inbox_items(test, bob.id, &before).await.len(),
        1,
        "first use reopened"
    );
    assert!(
        pin_record(test, carol.id, NEW)
            .await
            .unwrap()
            .pin(BOB)
            .is_some()
    );

    // The saved record replayed over a confirmed pin.
    let saved = raw_pins(test, carol.id)
        .await
        .expect("the first-use record");
    za_post(
        "contacts/pin",
        &json!({
            "username": CAROL,
            "password": NEW,
            "address": BOB,
            "fingerprint": display_fingerprint(&served(test, bob.id).await),
        }),
    )
    .await
    .expect(200);
    assert_eq!(
        pin_record(test, carol.id, NEW)
            .await
            .unwrap()
            .pin(BOB)
            .unwrap()
            .source,
        PinSource::Confirmed
    );
    set_raw_pins(test, carol.id, Some(&saved)).await;
    let revision = vault_revision(test, carol.id).await;
    assert_eq!(trigger_drain(CAROL, NEW).await, 207);
    assert_eq!(
        pin_record(test, carol.id, NEW)
            .await
            .unwrap()
            .pin(BOB)
            .unwrap()
            .source,
        PinSource::FirstUse
    );
    assert_eq!(
        vault_revision(test, carol.id).await,
        revision,
        "no generation bump, no signal"
    );

    // Cleanup.
    za_post(
        "password",
        &json!({"username": CAROL, "password": NEW, "new_password": STRONG}),
    )
    .await
    .expect(200);
    za_unpin(test, carol.id, STRONG).await;
    delete_event(&carol, &path_a).await;
    delete_event(&carol, &path_b).await;
    clear_recipient(test, &alice, &alice_start, "pin-10a").await;
    clear_recipient(test, &bob, &bob_start, "pin-10b").await;
}

/// Spec 9 case 11 (spec 5.1, the two-item rule): ordinary delivery to an
/// address with a withheld entry writes the full current state, then
/// upstream's messages for it, and retires the entry after both; the full
/// state is skipped when upstream's messages carry the master (a REQUEST or
/// a CANCEL), when the relationship ended, or when it yields nothing. Each
/// sub-case that counts items starts from a fresh refusal.
async fn case_11_ordinary_delivery(test: &TestServer) {
    case_11_reply(test).await;
    case_11_organizer(test).await;
    case_11_removal(test).await;
    case_11_failed_full_state(test).await;
    case_11_mixed(test).await;
    case_11_master_removal(test).await;
    case_11_delete(test).await;
    case_11_withheld_then_declined(test).await;
}

/// Case 11, Reply (`pin-11r`): bob's DECLINED refused; his ACCEPTED then
/// delivers his refused instances' current participation, then the REPLY.
async fn case_11_reply(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let fp = display_fingerprint(&served(test, alice.id).await);
    let path = event_path(&alice, "pin-11r");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-11r", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-11r").await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| with_bob_partstat(
            &body,
            Scope::Master,
            "DECLINED"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    let before = inbox_ids(test, alice.id).await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| with_bob_partstat(
            &body,
            Scope::Master,
            "ACCEPTED"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    let items = opened(test, alice.id, &before).await;
    assert_eq!(items.len(), 2, "his current participation, then the REPLY");
    assert!(
        items
            .iter()
            .all(|item| item_partstat(item, BOB).as_deref() == Some("ACCEPTED"))
    );
    assert_eq!(
        pin(BOB, ALICE, &fp).await.expect(200),
        json!({"ok": true, "delivered": 0, "failed": 0, "dropped": 0, "remaining": 0})
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path, BOB).await.as_deref(),
        Some("ACCEPTED")
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    alice.delete_items_since(test, &alice_start).await;
    clear_recipient(test, &bob, &bob_start, "pin-11r").await;
}

/// Case 11, Organizer (`pin-11o`): a daily event with override A naming
/// bob; the move of A refused; the new STATUS:CANCELLED override B naming
/// bob at NEEDS-ACTION (new overrides message an attendee only at
/// NEEDS-ACTION) gives bob the full REQUEST, then B's CANCEL. Then a fresh
/// refusal and a master change: one item.
async fn case_11_organizer(test: &TestServer) {
    const A_MOVED: &str = "DTSTART:20990302T120000Z";
    const B: &str = "20990303T100000Z";
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let path = event_path(&alice, "pin-11o");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-11o", &[BOB], &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace(&format!("DTSTART:{START_B}"), A_MOVED))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| with_override(
            &body,
            "pin-11o",
            B,
            "STATUS:CANCELLED\r\n",
            &[BOB]
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        methods(&opened(test, bob.id, &before).await),
        [
            Some(ICalendarMethod::Request),
            Some(ICalendarMethod::Cancel)
        ]
    );
    assert!(
        withheld(test, alice.id, BOB).await.is_empty(),
        "retired after both"
    );
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = body(&bob.client, &copy_href(test, bob.id, "pin-11o").await).await;
    assert!(
        copy.contains(A_MOVED) && copy.contains("STATUS:CANCELLED"),
        "A updated, B cancelled: {copy}"
    );

    // A fresh refusal, then a master change.
    let master_title = |from: &'static str, to: &'static str| {
        move |body: String| body.replacen(&format!("{from}\r\n"), &format!("{to}\r\n"), 1)
    };
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(
            &alice.client,
            &path,
            master_title("SUMMARY:fo-title pin-11o", "SUMMARY:fo-title pin-11o v2")
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let before_master = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(
            &alice.client,
            &path,
            master_title("SUMMARY:fo-title pin-11o v2", "SUMMARY:fo-title pin-11o v3")
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        opened(test, bob.id, &before_master).await.len(),
        1,
        "a master change gives one item"
    );
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-11o").await;
}

/// Case 11, Removal (`pin-11x`, and `pin-11x2` with the client bumping X's
/// SEQUENCE in the removal PUT): bob on the master and on override X with
/// carol; an older update (DESCRIPTION) refused; alice removes bob from X:
/// the full REQUEST, then X's CANCEL. bob keeps the update and X ends
/// cancelled.
async fn case_11_removal(test: &TestServer) {
    const DESCRIPTION_V2: &str = "DESCRIPTION:fo-description v2";
    let (alice, bob, carol) = parties(test);
    for uid in ["pin-11x", "pin-11x2"] {
        let (bob_start, carol_start) = (
            inbox_ids(test, bob.id).await,
            inbox_ids(test, carol.id).await,
        );
        let path = event_path(&alice, uid);
        alice
            .client
            .request_with_headers(
                "PUT",
                &path,
                [CONTENT_TYPE],
                recurring_invite(uid, &[BOB], &[BOB, CAROL]),
            )
            .await
            .with_status(StatusCode::CREATED);
        assert_eq!(trigger_drain(BOB, STRONG).await, 207);
        assert_eq!(trigger_drain(CAROL, STRONG).await, 207);
        let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
        let title = format!("SUMMARY:fo-title {uid}\r\n");
        assert_eq!(
            rewrite(&alice.client, &path, |body| body.replacen(
                &title,
                &format!("{title}{DESCRIPTION_V2}\r\n"),
                1
            ))
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(withheld(test, alice.id, BOB).await.len(), 1, "{uid}");
        substitute_public_key(test, bob.id, &original, true).await;
        let before = inbox_ids(test, bob.id).await;
        assert_eq!(
            rewrite(&alice.client, &path, |body| {
                let body = edit_attendee(&body, BOB, Scope::Overrides, |_| None);
                if uid == "pin-11x2" {
                    bump_sequence(&body, X)
                } else {
                    body
                }
            })
            .await,
            StatusCode::NO_CONTENT
        );
        let items = opened(test, bob.id, &before).await;
        assert_eq!(
            methods(&items),
            [
                Some(ICalendarMethod::Request),
                Some(ICalendarMethod::Cancel)
            ],
            "{uid}"
        );
        assert_eq!(trigger_drain(BOB, STRONG).await, 207);
        let copy = copy_href(test, bob.id, uid).await;
        let copy_body = body(&bob.client, &copy).await;
        assert!(
            copy_body.contains(DESCRIPTION_V2),
            "{uid}: bob keeps the update"
        );
        assert_eq!(
            status_in(&bob.client, &copy, Some(X)).await,
            Some(ICalendarStatus::Cancelled),
            "{uid}: X cancelled for bob: {copy_body}"
        );

        // Cleanup.
        delete_event(&alice, &path).await;
        clear_recipient(test, &bob, &bob_start, uid).await;
        clear_recipient(test, &carol, &carol_start, uid).await;
    }
}

/// Case 11, a failed full state (`pin-11f`, spec 5.1 and 5.2 phase 4): as
/// Removal, the refused older update a long DESCRIPTION and the inbound
/// size limit set between the full REQUEST's size and the CANCEL's for the
/// removal. The full state fails (`5.1`), X's CANCEL is still written, and
/// the entry stays: it is retired only when every item was written.
async fn case_11_failed_full_state(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let admin = test.account("admin@example.com").clone();
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let path = event_path(&alice, "pin-11f");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-11f", &[BOB], &[BOB, CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert_eq!(trigger_drain(CAROL, STRONG).await, 207);
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    let description = format!("DESCRIPTION:{}\r\n", "pin-11f ".repeat(300));
    assert_eq!(
        rewrite(&alice.client, &path, |body| body.replacen(
            "SUMMARY:fo-title pin-11f\r\n",
            &format!("SUMMARY:fo-title pin-11f\r\n{description}"),
            1
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let before = inbox_ids(test, bob.id).await;
    // The full REQUEST carries the 2400-byte DESCRIPTION; X's CANCEL does
    // not.
    admin
        .registry_update_setting(
            CalendarScheduling {
                itip_max_size: 1500,
                ..Default::default()
            },
            &[Property::ItipMaxSize],
        )
        .await;
    admin.reload_settings().await;
    let status = rewrite(&alice.client, &path, |body| {
        edit_attendee(&body, BOB, Scope::Overrides, |_| None)
    })
    .await;
    admin
        .registry_update_setting(CalendarScheduling::default(), &[Property::ItipMaxSize])
        .await;
    admin.reload_settings().await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        methods(&opened(test, bob.id, &before).await),
        [Some(ICalendarMethod::Cancel)],
        "the full state failed, the CANCEL was written"
    );
    assert_eq!(attendee_statuses(&alice.client, &path, BOB).await, ["5.1"]);
    assert_eq!(
        withheld(test, alice.id, BOB).await.len(),
        1,
        "not retired: the full state was not written"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    drop_entries(ALICE, BOB).await;
    clear_recipient(test, &bob, &bob_start, "pin-11f").await;
    clear_recipient(test, &carol, &carol_start, "pin-11f").await;
}

/// Case 11, Mixed (`pin-11m`): bob on overrides X and Z but not the master,
/// carol on the master and X, bob holding an entry; one PUT adds dave to
/// the master and removes bob from X, the client bumping X. bob gets the
/// full REQUEST (he is on Z), then his CANCEL for X, and no extra full
/// state; the CANCEL, built from the old tree one below, carries the full
/// REQUEST's SEQUENCE for X.
async fn case_11_mixed(test: &TestServer) {
    const Z: &str = "20990304T100000Z";
    const DAVE: &str = "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:dave@external.test\r\n";
    let (alice, bob, carol) = parties(test);
    let (bob_start, carol_start) = (
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let path = event_path(&alice, "pin-11m");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            with_override(
                &recurring_invite("pin-11m", &[CAROL], &[BOB, CAROL]),
                "pin-11m",
                Z,
                "",
                &[BOB],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert_eq!(trigger_drain(CAROL, STRONG).await, 207);
    // bob's entry: a refused change to Z.
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| edit_override(
            &body,
            Z,
            |line| {
                Some(line.replace("SUMMARY:fo-title pin-11m", "SUMMARY:fo-title pin-11m Z v2"))
            }
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| {
            let body = edit_override(&body, X, |line| {
                if line.starts_with("ATTENDEE")
                    && line
                        .to_ascii_lowercase()
                        .ends_with(&format!("mailto:{BOB}"))
                {
                    None
                } else {
                    Some(bumped(line))
                }
            });
            body.replacen("END:VEVENT", &format!("{DAVE}END:VEVENT"), 1)
        })
        .await,
        StatusCode::NO_CONTENT
    );
    let items = opened(test, bob.id, &before).await;
    assert_eq!(
        methods(&items),
        [
            Some(ICalendarMethod::Request),
            Some(ICalendarMethod::Cancel)
        ],
        "no extra full state"
    );
    assert!(
        sequence_of(&items[0], X).is_some(),
        "the full REQUEST holds X"
    );
    assert_eq!(
        sequence_of(&items[1], X),
        sequence_of(&items[0], X),
        "the CANCEL raised to the full REQUEST's value"
    );
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-11m").await;
    clear_recipient(test, &carol, &carol_start, "pin-11m").await;
}

/// Case 11, Master removal (`pin-11y`): bob the master's only attendee and
/// also on override Y; after a fresh refusal, alice removes bob from the
/// master: one item, the master CANCEL (another update-eligible master
/// attendee would trigger the full update, reaching bob through Y).
async fn case_11_master_removal(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let path = event_path(&alice, "pin-11y");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-11y", &[BOB], &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body.replacen(
            "SUMMARY:fo-title pin-11y\r\n",
            "SUMMARY:fo-title pin-11y v2\r\n",
            1
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| edit_attendee(
            &body,
            BOB,
            Scope::Master,
            |_| None
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    let items = opened(test, bob.id, &before).await;
    assert_eq!(items.len(), 1);
    assert_eq!(methods(&items), [Some(ICalendarMethod::Cancel)]);
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-11y").await;
}

/// Case 11, DELETE (`pin-11d`): bob's ACCEPTED refused; bob deletes his
/// copy: alice gets DECLINED and no full state (the source is gone), and
/// the entry is retired after the decline. Already DECLINED (`pin-11d2`):
/// `itip_cancel` builds nothing for a DECLINED attendee, yet the fork's
/// variant of `attendee_decline` sends DECLINED.
async fn case_11_delete(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    for (uid, refused) in [("pin-11d", "ACCEPTED"), ("pin-11d2", "DECLINED")] {
        let (alice_start, bob_start) = (
            inbox_ids(test, alice.id).await,
            inbox_ids(test, bob.id).await,
        );
        // bob pins alice first.
        write_pin_record(
            test,
            bob.id,
            STRONG,
            &record_pinning(ALICE, served(test, alice.id).await),
        )
        .await;
        let path = event_path(&alice, uid);
        alice
            .client
            .request_with_headers("PUT", &path, [CONTENT_TYPE], invite(uid, START_A, &[BOB]))
            .await
            .with_status(StatusCode::CREATED);
        assert_eq!(trigger_drain(BOB, STRONG).await, 207);
        let copy = copy_href(test, bob.id, uid).await;
        let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
        assert_eq!(
            rewrite(&bob.client, &copy, |body| with_bob_partstat(
                &body,
                Scope::Master,
                refused
            ))
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1, "{uid}");
        substitute_public_key(test, alice.id, &original, true).await;
        let before = inbox_ids(test, alice.id).await;
        bob.client
            .request("DELETE", &copy, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        let items = opened(test, alice.id, &before).await;
        assert_eq!(
            (
                items.len(),
                items
                    .first()
                    .and_then(|item| item_partstat(item, BOB))
                    .as_deref()
            ),
            (1, Some("DECLINED")),
            "{uid}: no full state: the source is gone"
        );
        assert!(
            withheld(test, bob.id, ALICE).await.is_empty(),
            "{uid}: retired after the decline"
        );

        // Cleanup.
        delete_event(&alice, &path).await;
        alice.delete_items_since(test, &alice_start).await;
        clear_recipient(test, &bob, &bob_start, uid).await;
    }
}

/// Case 11, Withheld, then DECLINED (`pin-11w`): alice's SUMMARY edit
/// refused (no SEQUENCE bump), bob then DECLINED; alice's next edit sends
/// bob nothing (upstream's: a DECLINED attendee gets no update, and no fork
/// mutant reaches it), and `contacts/pin` drops the entry (the fork's
/// check: the full REQUEST yields nothing for a DECLINED attendee).
async fn case_11_withheld_then_declined(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    let path = event_path(&alice, "pin-11w");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-11w", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-11w", "fo-title pin-11w v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    let copy = copy_href(test, bob.id, "pin-11w").await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| with_bob_partstat(
            &body,
            Scope::Master,
            "DECLINED"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path, BOB).await.as_deref(),
        Some("DECLINED")
    );
    let before_edit = inbox_ids(test, bob.id).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-11w v2", "fo-title pin-11w v3"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert!(
        opened(test, bob.id, &before_edit).await.is_empty(),
        "upstream sends a DECLINED attendee nothing"
    );
    let fp = display_fingerprint(&served(test, bob.id).await);
    assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["dropped"], 1);

    // Cleanup.
    delete_event(&alice, &path).await;
    alice.delete_items_since(test, &alice_start).await;
    clear_recipient(test, &bob, &bob_start, "pin-11w").await;
}

/// Spec 9 case 12 (spec 5.1): partial changes to the existing overrides A
/// (day 2) and B (day 3) of a daily event naming bob and carol, bob
/// drained, each variant its own event. `pin-12a`: both moves refused;
/// after `contacts/pin` bob's copy holds both. `pin-12b`: the move of A
/// reaches bob, then the move of B is refused; the redelivered full state
/// carries A at the partial's SEQUENCE (the reconciliation), so bob's copy
/// does not reject it as `OutOfSequence`. `pin-12c`: as `pin-12b`, the
/// second PUT also writing A's SEQUENCE one below the stored value (the
/// floor).
async fn case_12_partial_changes(test: &TestServer) {
    const A_AT: &str = "DTSTART:20990302T100000Z";
    const A_MOVED: &str = "DTSTART:20990302T120000Z";
    const B_AT: &str = "DTSTART:20990303T100000Z";
    const B_MOVED: &str = "DTSTART:20990303T120000Z";
    let (alice, bob, carol) = parties(test);
    for uid in ["pin-12a", "pin-12b", "pin-12c"] {
        let (bob_start, carol_start) = (
            inbox_ids(test, bob.id).await,
            inbox_ids(test, carol.id).await,
        );
        let path = event_path(&alice, uid);
        alice
            .client
            .request_with_headers("PUT", &path, [CONTENT_TYPE], partial_invite(uid))
            .await
            .with_status(StatusCode::CREATED);
        assert_eq!(trigger_drain(BOB, STRONG).await, 207);
        let move_a = |body: String| body.replace(A_AT, A_MOVED);
        let original = if uid == "pin-12a" {
            let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
            assert_eq!(
                rewrite(&alice.client, &path, move_a).await,
                StatusCode::NO_CONTENT
            );
            original
        } else {
            // The delivered partial.
            assert_eq!(
                rewrite(&alice.client, &path, move_a).await,
                StatusCode::NO_CONTENT
            );
            assert_eq!(trigger_drain(BOB, STRONG).await, 207);
            let copy = body(&bob.client, &copy_href(test, bob.id, uid).await).await;
            assert!(copy.contains(A_MOVED), "{uid}: {copy}");
            substitute_public_key(test, bob.id, &writer_key(), true).await
        };
        assert_eq!(
            rewrite(&alice.client, &path, |body| {
                let body = body.replace(B_AT, B_MOVED);
                if uid == "pin-12c" {
                    lower_sequence(&body, START_B)
                } else {
                    body
                }
            })
            .await,
            StatusCode::NO_CONTENT
        );
        substitute_public_key(test, bob.id, &original, true).await;
        let fp = display_fingerprint(&served(test, bob.id).await);
        assert_eq!(pin(ALICE, BOB, &fp).await.expect(200)["delivered"], 1);
        assert_eq!(trigger_drain(BOB, STRONG).await, 207);
        assert!(pending_ids(test, bob.id).await.is_empty());
        let copy = body(&bob.client, &copy_href(test, bob.id, uid).await).await;
        if uid == "pin-12a" {
            assert!(
                copy.contains(A_MOVED) && copy.contains(B_MOVED),
                "{uid}: {copy}"
            );
        } else {
            assert!(copy.contains(B_MOVED), "{uid}: no OutOfSequence: {copy}");
        }

        // Cleanup.
        delete_event(&alice, &path).await;
        clear_recipient(test, &bob, &bob_start, uid).await;
        clear_recipient(test, &carol, &carol_start, uid).await;
    }
}

/// Spec 9 case 13 (spec 2, limit 6): the stated losses of redelivering
/// current state, one sub-test each. (a) An organizer DELETE retires the
/// document's entries and bob keeps his copy; (b) a refused removal CANCEL
/// is dropped and bob keeps his copy; (c) an occurrence bob declined by
/// deleting it is not reconstructed; (d) a refused delegation of the
/// master, then an organizer master change that reaches bob first: alice
/// gains no delegate.
async fn case_13_stated_losses(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start, carol_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let bob_fingerprint = display_fingerprint(&served(test, bob.id).await);
    let alice_fingerprint = display_fingerprint(&served(test, alice.id).await);

    // (a) A refused edit, then alice deletes the event with scheduling (its
    // CANCEL refused too).
    let path = event_path(&alice, "pin-13a");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-13a", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-13a").await;
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-13a", "fo-title pin-13a v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    alice
        .client
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    substitute_public_key(test, bob.id, &original, true).await;
    assert!(
        withheld(test, alice.id, BOB).await.is_empty(),
        "the deletion retires the document's entries"
    );
    assert!(
        !body(&bob.client, &copy).await.contains("STATUS:CANCELLED"),
        "bob keeps his copy"
    );

    // (b) alice removes bob from the event: the CANCEL refused, an entry.
    let path = event_path(&alice, "pin-13b");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-13b", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-13b").await;
    let original = substitute_public_key(test, bob.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| edit_attendee(
            &body,
            BOB,
            Scope::Master,
            |_| None
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(
        pin(ALICE, BOB, &bob_fingerprint).await.expect(200)["dropped"],
        1
    );
    assert!(!body(&bob.client, &copy).await.contains("STATUS:CANCELLED"));
    delete_event(&alice, &path).await;

    // (c) bob deletes occurrence X of his copy: a refused decline recording
    // X.
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path = event_path(&alice, "pin-13c");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-13c", &[BOB], &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-13c").await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| without_override(&body, X)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    assert_eq!(
        pin(BOB, ALICE, &alice_fingerprint).await.expect(200)["dropped"],
        1,
        "X no longer exists in bob's copy"
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert!(
        !body(&alice.client, &path)
            .await
            .contains("PARTSTAT=DECLINED"),
        "the decline is not reconstructed"
    );
    delete_event(&alice, &path).await;

    // (d) bob's delegation of the master to carol refused; alice's master
    // change reaches bob first.
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path = event_path(&alice, "pin-13d");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-13d", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-13d").await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy, delegate_to_carol).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-13d", "fo-title pin-13d v2"))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert_eq!(
        pin(BOB, ALICE, &alice_fingerprint).await.expect(200)["delivered"],
        1,
        "the reset participation is redelivered"
    );
    assert!(
        withheld(test, bob.id, ALICE).await.is_empty(),
        "and the entry retired"
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert!(
        !body(&alice.client, &path).await.contains(CAROL),
        "alice gains no delegate"
    );
    delete_event(&alice, &path).await;

    // Cleanup.
    alice.delete_items_since(test, &alice_start).await;
    for uid in ["pin-13a", "pin-13b", "pin-13c", "pin-13d"] {
        clear_recipient(test, &bob, &bob_start, uid).await;
    }
    clear_recipient(test, &carol, &carol_start, "pin-13d").await;
}

/// Spec 9 case 14 (spec 5.6): (organizer side) the drain's delegating
/// merge fans out under the organizer's pins, so a refused delegate gets
/// `3.8` in the organizer's event and an entry for that event's document,
/// which `contacts/pin` redelivers; (attendee side) a refused delegating
/// REPLY records the master, and `contacts/pin` sends the delegator's
/// participation with the delegate's line.
async fn case_14_delegates(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start, carol_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let alice_fingerprint = display_fingerprint(&served(test, alice.id).await);
    // bob pins alice first, so his replies do not depend on earlier cases.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;

    // Organizer side.
    let path = event_path(&alice, "pin-14");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-14", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let event_id = document_id_of(test, alice.id, &path).await;
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let fingerprint = lookup(ALICE, CAROL).await.str(200, "fingerprint");
    pin(ALICE, CAROL, &fingerprint).await.expect(200);
    let original = substitute_public_key(test, carol.id, &writer_key(), true).await;
    let copy = copy_href(test, bob.id, "pin-14").await;
    assert_eq!(
        rewrite(&bob.client, &copy, delegate_to_carol).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        attendee_statuses(&alice.client, &path, CAROL).await,
        ["3.8"]
    );
    let entries = withheld(test, alice.id, CAROL).await;
    assert_eq!(
        (entries.len(), entries[0].document_id),
        (1, event_id),
        "the organizer event's document"
    );
    substitute_public_key(test, carol.id, &original, true).await;
    assert_eq!(
        pin(ALICE, CAROL, &fingerprint).await.expect(200)["delivered"],
        1
    );

    // Attendee side.
    let path_b = event_path(&alice, "pin-14b");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_b,
            [CONTENT_TYPE],
            invite("pin-14b", START_B, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    let copy_b = copy_href(test, bob.id, "pin-14b").await;
    assert_eq!(
        rewrite(&bob.client, &copy_b, delegate_to_carol).await,
        StatusCode::NO_CONTENT
    );
    substitute_public_key(test, alice.id, &original, true).await;
    let entry = &withheld(test, bob.id, ALICE).await[0];
    assert!(
        entry.instances.master,
        "the refused REPLY records the master"
    );
    assert_eq!(
        pin(BOB, ALICE, &alice_fingerprint).await.expect(200)["delivered"],
        1
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path_b, BOB).await.as_deref(),
        Some("DELEGATED")
    );
    assert!(
        body(&alice.client, &path_b)
            .await
            .contains("DELEGATED-FROM"),
        "the redelivery carries bob's delegate line"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    delete_event(&alice, &path_b).await;
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    alice.delete_items_since(test, &alice_start).await;
    for uid in ["pin-14", "pin-14b"] {
        clear_recipient(test, &bob, &bob_start, uid).await;
        clear_recipient(test, &carol, &carol_start, uid).await;
    }
}

/// Spec 9 case 15: a contact pinned on the page before any invitation (the
/// endpoint's first pin is the batch with the rewrap, spec 4.4); a changed
/// key is then refused at the first invitation.
async fn case_15_pre_pin(test: &TestServer) {
    let (alice, _, carol) = parties(test);
    za_unpin(test, alice.id, STRONG).await;
    let fingerprint = lookup(ALICE, CAROL).await.str(200, "fingerprint");
    pin(ALICE, CAROL, &fingerprint).await.expect(200);
    assert_eq!(
        wrap_mark(test, alice.id, STRONG).await,
        ZaMark::Pinned,
        "the endpoint's first pin is the batch"
    );
    let original = substitute_public_key(test, carol.id, &writer_key(), true).await;
    let before = inbox_ids(test, carol.id).await;
    let path = event_path(&alice, "pin-15");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-15", START_A, &[CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        attendee_statuses(&alice.client, &path, CAROL).await,
        ["3.8"]
    );
    assert!(new_inbox_items(test, carol.id, &before).await.is_empty());
    assert_eq!(withheld(test, alice.id, CAROL).await.len(), 1);

    // Cleanup.
    substitute_public_key(test, carol.id, &original, true).await;
    delete_event(&alice, &path).await;
    drop_entries(ALICE, CAROL).await;
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

/// Spec 9 case 17: the retirement rule under races (spec 5.2, 6.4 step 5).
/// Each part has its own event and its own sub-address of bob (pinned and
/// withheld separately, spec 4.3) and starts from one refused change with
/// bob's key restored. (a) A newer refusal while `contacts/pin` is parked
/// before its cleanup; (b) a second `contacts/pin` retires the entry and a
/// refusal recreates it while the first is parked; (c) a newer refusal
/// while an ordinary edit is parked before its phase 4; (d) a newer
/// refusal while `contacts/pin`'s inner fan-out is parked before phase 4
/// (its reseal then loses the archive assertion); (e) the same park with an
/// edit that sends nothing, so the retried reseal applies `1.2` and the
/// entry, its refusal id unchanged, stays. A re-armed or recreated entry
/// always stays.
async fn case_17_cleanup_races(test: &TestServer) {
    const SUB_A: &str = "key2+pin17a@example.com";
    const SUB_B: &str = "key2+pin17b@example.com";
    const SUB_C: &str = "key2+pin17c@example.com";
    const SUB_D: &str = "key2+pin17d@example.com";
    const SUB_E: &str = "key2+pin17e@example.com";
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let fingerprint = display_fingerprint(&served(test, bob.id).await);
    for sub in [SUB_A, SUB_B, SUB_C, SUB_D, SUB_E] {
        let looked_up = lookup(ALICE, sub).await.str(200, "fingerprint");
        assert_eq!(looked_up, fingerprint, "{sub} resolves to bob");
        pin(ALICE, sub, &fingerprint).await.expect(200);
    }

    // (a) A newer refusal while contacts/pin is parked before step 5.
    let (path, original, writer) = refuse_once_at(test, &alice, &bob, SUB_A, "pin-17a").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_redeliver_cleanup_pause,
        pin_request(ALICE, SUB_A, &fingerprint),
    )
    .await;
    let newer = newer_refusal(
        test, &alice, &bob, &path, "pin-17a", SUB_A, &original, &writer,
    )
    .await;
    let reply = parked.finish().await.expect(200);
    assert_eq!(
        (reply["delivered"].clone(), reply["remaining"].clone()),
        (json!(0), json!(1))
    );
    let entries = withheld(test, alice.id, SUB_A).await;
    assert_eq!(
        (entries.len(), entries[0].refusal_id),
        (1, newer),
        "the re-armed entry stays"
    );
    delete_event(&alice, &path).await;
    drop_entries(ALICE, SUB_A).await;

    // (b) The first refusal id the entry ever had, consumed by A; B retires
    // the entry and a refusal recreates it (a counter would give it the
    // same id) before A's step 5.
    let (path, original, writer) = refuse_once_at(test, &alice, &bob, SUB_B, "pin-17b").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_redeliver_cleanup_pause,
        pin_request(ALICE, SUB_B, &fingerprint),
    )
    .await;
    let b_reply = pin(ALICE, SUB_B, &fingerprint).await.expect(200);
    assert_eq!(b_reply["delivered"], 1);
    assert!(withheld(test, alice.id, SUB_B).await.is_empty());
    newer_refusal(
        test, &alice, &bob, &path, "pin-17b", SUB_B, &original, &writer,
    )
    .await;
    assert_eq!(parked.finish().await.expect(200)["remaining"], 1);
    assert_eq!(
        withheld(test, alice.id, SUB_B).await.len(),
        1,
        "A's retry kept the recreated entry"
    );
    delete_event(&alice, &path).await;
    drop_entries(ALICE, SUB_B).await;

    // (c) A newer refusal while an ordinary edit is parked before phase 4,
    // after SUB_C's item.
    let (path, original, writer) = refuse_once_at(test, &alice, &bob, SUB_C, "pin-17c").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let edited = body(&alice.client, &path)
        .await
        .replace("fo-title pin-17c v2", "fo-title pin-17c v3");
    assert!(
        pending_ids(test, alice.id).await.is_empty(),
        "nothing pending: the pause parks the edit's fan-out, not a merge's"
    );
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_retire_pause,
        put_raw(ALICE, &path, edited),
    )
    .await;
    substitute_public_key(test, bob.id, &writer, true).await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body
            .replace("fo-title pin-17c v2", "fo-title pin-17c v4"))
        .await,
        StatusCode::NO_CONTENT
    );
    let newer = withheld(test, alice.id, SUB_C).await[0].refusal_id;
    substitute_public_key(test, bob.id, &original, true).await;
    // The parked PUT's commit loses its archive assertion.
    let _ = parked.finish().await;
    let entries = withheld(test, alice.id, SUB_C).await;
    assert_eq!(
        (entries.len(), entries[0].refusal_id),
        (1, newer),
        "the newer refusal survives phase 4"
    );
    delete_event(&alice, &path).await;
    drop_entries(ALICE, SUB_C).await;

    // (d) A newer refusal while contacts/pin's inner fan-out is parked
    // before phase 4.
    let (path, original, writer) = refuse_once_at(test, &alice, &bob, SUB_D, "pin-17d").await;
    substitute_public_key(test, bob.id, &original, true).await;
    assert!(
        pending_ids(test, alice.id).await.is_empty(),
        "nothing pending: the pause parks the redelivery's fan-out, not a merge's"
    );
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_retire_pause,
        pin_request(ALICE, SUB_D, &fingerprint),
    )
    .await;
    let newer = newer_refusal(
        test, &alice, &bob, &path, "pin-17d", SUB_D, &original, &writer,
    )
    .await;
    let reply = parked.finish().await.expect(200);
    assert_eq!(
        (reply["delivered"].clone(), reply["remaining"].clone()),
        (json!(0), json!(1))
    );
    let entries = withheld(test, alice.id, SUB_D).await;
    assert_eq!(
        (entries.len(), entries[0].refusal_id),
        (1, newer),
        "the re-armed entry stays"
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path, SUB_D).await,
        ["3.8"],
        "the newer refusal's status survives"
    );
    delete_event(&alice, &path).await;
    drop_entries(ALICE, SUB_D).await;

    // (e) An edit that sends nothing (CATEGORIES is no scheduling property,
    // so upstream builds no message and nothing fans out) commits while
    // contacts/pin's inner fan-out is parked before phase 4: the reseal
    // loses its archive assertion, the fresh pin record still holds the
    // consumed refusal id, and `1.2` is applied on the fresh archive.
    let (path, original, _) = refuse_once_at(test, &alice, &bob, SUB_E, "pin-17e").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let consumed = withheld(test, alice.id, SUB_E).await[0].refusal_id;
    let before = inbox_ids(test, bob.id).await;
    assert!(
        pending_ids(test, alice.id).await.is_empty(),
        "nothing pending: the pause parks the redelivery's fan-out, not a merge's"
    );
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_retire_pause,
        pin_request(ALICE, SUB_E, &fingerprint),
    )
    .await;
    assert_eq!(
        rewrite(&alice.client, &path, |body| body.replacen(
            "END:VEVENT",
            "CATEGORIES:pin-17e\r\nEND:VEVENT",
            1
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    let reply = parked.finish().await.expect(200);
    assert_eq!(
        (reply["delivered"].clone(), reply["remaining"].clone()),
        (json!(0), json!(1))
    );
    assert_eq!(
        new_inbox_items(test, bob.id, &before).await.len(),
        1,
        "the redelivered item only: the edit sent nothing"
    );
    assert_eq!(
        attendee_statuses(&alice.client, &path, SUB_E).await,
        ["1.2"],
        "applied once more on the fresh archive"
    );
    assert!(
        body(&alice.client, &path)
            .await
            .contains("CATEGORIES:pin-17e"),
        "the concurrent edit survives the retried reseal"
    );
    let entries = withheld(test, alice.id, SUB_E).await;
    assert_eq!(
        (entries.len(), entries[0].refusal_id),
        (1, consumed),
        "the entry stays withheld"
    );
    delete_event(&alice, &path).await;
    drop_entries(ALICE, SUB_E).await;

    // Cleanup.
    for uid in ["pin-17a", "pin-17b", "pin-17c", "pin-17d", "pin-17e"] {
        clear_recipient(test, &bob, &bob_start, uid).await;
    }
}

/// Case 17's newer refusal: bob's key substituted, alice's edit of `path`
/// (the next `v` suffix of `uid`'s title) refused for `sub`, bob's key
/// restored. Returns the entry's new refusal id.
#[allow(clippy::too_many_arguments)]
async fn newer_refusal(
    test: &TestServer,
    alice: &Party,
    bob: &Party,
    path: &str,
    uid: &str,
    sub: &str,
    original: &[u8],
    writer: &[u8; 32],
) -> u64 {
    substitute_public_key(test, bob.id, writer, true).await;
    let current = body(&alice.client, path).await;
    let title = format!("fo-title {uid} v");
    let at = current.find(&title).expect("the title") + title.len();
    let version: u32 = current[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a version");
    let edited = current.replacen(
        &format!("{title}{version}"),
        &format!("{title}{}", version + 1),
        1,
    );
    alice
        .client
        .request_with_headers("PUT", path, [CONTENT_TYPE], edited)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(attendee_statuses(&alice.client, path, sub).await, ["3.8"]);
    let entries = withheld(test, alice.id, sub).await;
    assert_eq!(entries.len(), 1);
    substitute_public_key(test, bob.id, original, true).await;
    entries[0].refusal_id
}

/// Spec 9 case 18: an entry is bound to its source document (invariant
/// 23): two events with one UID in two calendars are separate entries; a
/// redelivery sends each document's own state; an entry whose document was
/// deleted is dropped, even when a new document took the UID.
async fn case_18_event_identity(test: &TestServer) {
    const SECOND: &str = "/dav/cal/key1%40example.com/pin18b/";
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    alice
        .client
        .request("MKCALENDAR", SECOND, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let event = |summary: &str| {
        invite("pin-18", START_A, &[BOB])
            .replace("SUMMARY:fo-title pin-18", &format!("SUMMARY:{summary}"))
    };
    let path_a = event_path(&alice, "pin-18");
    let path_b = format!("{SECOND}pin-18.ics");
    for (path, summary) in [(&path_a, "A"), (&path_b, "B")] {
        alice
            .client
            .request_with_headers("PUT", path, [CONTENT_TYPE], event(summary))
            .await
            .with_status(StatusCode::CREATED);
    }
    let b = document_id_of(test, alice.id, &path_b).await;
    assert_ne!(document_id_of(test, alice.id, &path_a).await, b);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let fingerprint = display_fingerprint(&served(test, bob.id).await);
    let edit = |path: &str, from: &str, to: &str| {
        let (from, to) = (format!("SUMMARY:{from}\r\n"), format!("SUMMARY:{to}\r\n"));
        let client = &alice.client;
        let path = path.to_string();
        async move {
            assert_eq!(
                rewrite(client, &path, |body| body.replace(&from, &to)).await,
                StatusCode::NO_CONTENT
            );
        }
    };

    // (1) Only b edited while bob's key is substituted.
    let writer = writer_key();
    let original = substitute_public_key(test, bob.id, &writer, true).await;
    edit(&path_b, "B", "B v2").await;
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(
        withheld(test, alice.id, BOB)
            .await
            .iter()
            .map(|entry| entry.document_id)
            .collect::<Vec<_>>(),
        [b]
    );
    let before = inbox_ids(test, bob.id).await;
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200)["delivered"],
        1
    );
    assert_eq!(opened_summaries(test, bob.id, &before).await, ["B v2"]);

    // (2) Both edited.
    substitute_public_key(test, bob.id, &writer, true).await;
    edit(&path_a, "A", "A v3").await;
    edit(&path_b, "B v2", "B v3").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let before2 = inbox_ids(test, bob.id).await;
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200)["delivered"],
        2
    );
    assert_eq!(
        opened_summaries(test, bob.id, &before2).await,
        ["A v3", "B v3"]
    );

    // (3) b refused, deleted, and recreated under the same UID after the
    // restore (so the create's fan-out does not refuse bob for the new
    // document).
    substitute_public_key(test, bob.id, &writer, true).await;
    edit(&path_b, "B v3", "B v4").await;
    delete_event(&alice, &path_b).await;
    substitute_public_key(test, bob.id, &original, true).await;
    alice
        .client
        .request_with_headers("PUT", &path_b, [CONTENT_TYPE], event("B"))
        .await
        .with_status(StatusCode::CREATED);
    let reply = pin(ALICE, BOB, &fingerprint).await.expect(200);
    assert_eq!(
        (reply["delivered"].clone(), reply["dropped"].clone()),
        (json!(0), json!(1))
    );

    // Cleanup.
    delete_event(&alice, &path_a).await;
    delete_event(&alice, &path_b).await;
    alice
        .client
        .request("DELETE", SECOND, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    clear_recipient(test, &bob, &bob_start, "pin-18").await;
}

/// Spec 9 case 19 (spec 6.4 step 2): the redelivered REPLY carries only
/// the refused instances. bob's copy keeps an override X cancelled and
/// naming him after alice removed him from it; a whole-copy REPLY would be
/// rejected or would add X to alice's event.
async fn case_19_participation_filter(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start, carol_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path = event_path(&alice, "pin-19");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-19", &[BOB, CAROL], &[BOB, CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    // alice removes bob from X: a removal CANCEL to bob.
    assert_eq!(
        rewrite(&alice.client, &path, |body| edit_attendee(
            &body,
            BOB,
            Scope::Overrides,
            |_| None
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-19").await;
    let copy_body = body(&bob.client, &copy).await;
    assert!(
        copy_body.contains("STATUS:CANCELLED"),
        "bob's copy keeps X cancelled: {copy_body}"
    );
    // bob accepts the master while alice's key is substituted: refused.
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| edit_attendee(
            &body,
            BOB,
            Scope::Master,
            |line| Some(with_partstat(line, "ACCEPTED"))
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    let vevents_before = vevent_count(&alice.client, &path).await;
    let fingerprint = display_fingerprint(&served(test, alice.id).await);
    assert_eq!(
        pin(BOB, ALICE, &fingerprint).await.expect(200)["delivered"],
        1
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path, BOB).await.as_deref(),
        Some("ACCEPTED"),
        "a whole-copy REPLY would be rejected: OutOfSequence (the reconciled X is ahead of bob's), or SenderIsNotParticipant without the reconciliation"
    );
    assert_eq!(
        vevent_count(&alice.client, &path).await,
        vevents_before,
        "alice gains no override"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    alice.delete_items_since(test, &alice_start).await;
    clear_recipient(test, &bob, &bob_start, "pin-19").await;
    clear_recipient(test, &carol, &carol_start, "pin-19").await;
}

/// Spec 9 case 20 (spec 6.4): `contacts/pin` drains the account before it
/// builds a REPLY, so a partial REQUEST pending in the inbox reaches the
/// copy first.
async fn case_20_drain_before_build(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path = event_path(&alice, "pin-20");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            recurring_invite("pin-20", &[BOB], &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    let copy = copy_href(test, bob.id, "pin-20").await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| edit_attendee(
            &body,
            BOB,
            Scope::Master,
            |line| Some(with_partstat(line, "ACCEPTED"))
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    // alice moves X: a partial REQUEST, pending in bob's inbox.
    assert_eq!(
        rewrite(&alice.client, &path, |body| body.replace(
            &format!("DTSTART:{START_B}"),
            "DTSTART:20990302T120000Z"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(pending_ids(test, bob.id).await.len(), 1);
    let fingerprint = display_fingerprint(&served(test, alice.id).await);
    assert_eq!(
        pin(BOB, ALICE, &fingerprint).await.expect(200)["delivered"],
        1
    );
    assert!(
        pending_ids(test, bob.id).await.is_empty(),
        "contacts/pin drained before building"
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path, BOB).await.as_deref(),
        Some("ACCEPTED")
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    alice.delete_items_since(test, &alice_start).await;
    clear_recipient(test, &bob, &bob_start, "pin-20").await;
}

/// Spec 9 case 21 (spec 4.1, 6.4 step 2): a redelivered or ordinary REPLY
/// exports only the recorded instances. bob delegated the master to carol,
/// who accepted (bob does not see it); bob's ACCEPTED on override X is
/// refused. Through `contacts/pin` (run 1) or bob's next ordinary change to
/// X (run 2), alice has bob's answer on X and carol stays ACCEPTED on the
/// master, which is not exported. A genuine delegation of the master,
/// refused and redelivered, gives alice carol, and her invitation.
async fn case_21_recorded_instances(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start, carol_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let fp = display_fingerprint(&served(test, alice.id).await);

    // Run 1: contacts/pin.
    let (path, _) = recorded_instance_setup(test, &alice, &bob, &carol, "pin-21").await;
    assert_eq!(pin(BOB, ALICE, &fp).await.expect(200)["delivered"], 1);
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_in(&alice.client, &path, Some(X), BOB)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );
    assert_eq!(
        partstat_in(&alice.client, &path, None, CAROL)
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the master is not exported"
    );

    // Run 2: bob sets X TENTATIVE (ordinary recovery).
    let (path_b, copy_b) = recorded_instance_setup(test, &alice, &bob, &carol, "pin-21b").await;
    assert_eq!(
        rewrite(&bob.client, &copy_b, |body| with_bob_partstat(
            &body,
            Scope::Overrides,
            "TENTATIVE"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_in(&alice.client, &path_b, Some(X), BOB)
            .await
            .as_deref(),
        Some("TENTATIVE")
    );
    assert_eq!(
        partstat_in(&alice.client, &path_b, None, CAROL)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );

    // Genuine delegation (pin-21c).
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path_c = event_path(&alice, "pin-21c");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_c,
            [CONTENT_TYPE],
            daily_invite("pin-21c", &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy_c = copy_href(test, bob.id, "pin-21c").await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy_c, delegate_to_carol).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1);
    substitute_public_key(test, alice.id, &original, true).await;
    let carol_before = inbox_ids(test, carol.id).await;
    assert_eq!(pin(BOB, ALICE, &fp).await.expect(200)["delivered"], 1);
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_in(&alice.client, &path_c, None, CAROL)
            .await
            .as_deref(),
        Some("NEEDS-ACTION"),
        "alice gains carol"
    );
    assert_eq!(
        opened(test, carol.id, &carol_before).await.len(),
        1,
        "and invites her"
    );

    // Cleanup.
    for path in [&path, &path_b, &path_c] {
        delete_event(&alice, path).await;
    }
    alice.delete_items_since(test, &alice_start).await;
    for uid in ["pin-21", "pin-21b", "pin-21c"] {
        clear_recipient(test, &bob, &bob_start, uid).await;
        clear_recipient(test, &carol, &carol_start, uid).await;
    }
}

/// Case 21's setup: bob pins alice first; alice's daily event `uid` naming
/// bob; bob delegates the master to carol, carol accepts; alice adds
/// override X naming only bob at NEEDS-ACTION and bob drains it; bob's
/// ACCEPTED on X is refused; alice's key restored. Returns alice's href and
/// bob's copy's.
async fn recorded_instance_setup(
    test: &TestServer,
    alice: &Party,
    bob: &Party,
    carol: &Party,
    uid: &str,
) -> (String, String) {
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path = event_path(alice, uid);
    alice
        .client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], daily_invite(uid, &[BOB]))
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, uid).await;
    assert_eq!(
        rewrite(&bob.client, &copy, delegate_to_carol).await,
        StatusCode::NO_CONTENT
    );
    // alice's delegating merge invites carol.
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(trigger_drain(CAROL, STRONG).await, 207);
    let carol_copy = copy_href(test, carol.id, uid).await;
    assert_eq!(
        rewrite(&carol.client, &carol_copy, |body| edit_attendee(
            &body,
            CAROL,
            Scope::Master,
            |line| Some(with_partstat(line, "ACCEPTED"))
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_in(&alice.client, &path, None, CAROL)
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "{uid}: carol accepted"
    );
    assert_eq!(
        rewrite(&alice.client, &path, |body| with_override(
            &body,
            uid,
            X,
            "",
            &[BOB]
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    assert_eq!(
        partstat_in(&bob.client, &copy, Some(X), BOB)
            .await
            .as_deref(),
        Some("NEEDS-ACTION"),
        "{uid}: bob holds X"
    );
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    assert_eq!(
        rewrite(&bob.client, &copy, |body| with_bob_partstat(
            &body,
            Scope::Overrides,
            "ACCEPTED"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(withheld(test, bob.id, ALICE).await.len(), 1, "{uid}");
    substitute_public_key(test, alice.id, &original, true).await;
    (path, copy)
}

/// Review Focus 1: the endpoints take an address as given and normalize it
/// as the fan-out does (spec 4.3): `mailto:` and display-name forms are
/// 400, case and whitespace are normalized, and pins are keyed by the
/// normalized address only.
async fn rf_1_address_forms(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let path = event_path(&alice, "pin-rf1");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite("pin-rf1", START_A, &["Key2@Example.COM"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    for bad in ["mailto:key2@example.com", "Key2 <key2@example.com>"] {
        assert_eq!(lookup(ALICE, bad).await.status, 400, "{bad}");
    }
    assert_eq!(
        lookup(ALICE, " Key2@Example.COM ").await.expect(200)["address"],
        BOB
    );
    for other in ["plain@example.com", "nobody@example.com"] {
        assert_eq!(
            lookup(ALICE, other).await.expect(409)["error"],
            "not a key account"
        );
    }
    assert_eq!(
        lookup(ALICE, ALICE).await.expect(200)["fingerprint"],
        display_fingerprint(&served(test, alice.id).await)
    );
    let listing = contacts(ALICE).await;
    let bobs: Vec<&str> = listing["pins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pin| pin["address"].as_str().unwrap())
        .filter(|address| address.eq_ignore_ascii_case(BOB))
        .collect();
    assert_eq!(
        bobs,
        ["key2@example.com"],
        "keyed exactly by the normalized address"
    );
    let fingerprint = lookup(ALICE, BOB).await.str(200, "fingerprint");
    pin(ALICE, " Key2@Example.COM ", &fingerprint)
        .await
        .expect(200);
    let record = pin_record(test, alice.id, STRONG).await.unwrap();
    assert_eq!(
        record
            .pins
            .iter()
            .filter(|pin| pin.address.eq_ignore_ascii_case(BOB))
            .count(),
        1,
        "one pin after the page's form too"
    );

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-rf1").await;
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

/// Review Focus 4: an entry names its source by document id, which a MOVE
/// into another calendar keeps, so the moved event is redelivered.
async fn rf_4_moved_source(test: &TestServer) {
    const MOVED: &str = "/dav/cal/key1%40example.com/pinrf4/";
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let (path, original, _) = refuse_once(test, &alice, &bob, "pin-rf4").await;
    let document_id = document_id_of(test, alice.id, &path).await;
    alice
        .client
        .request("MKCALENDAR", MOVED, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let moved = format!("{MOVED}pin-rf4.ics");
    alice
        .client
        .request_with_headers("MOVE", &path, [("destination", moved.as_str())], "")
        .await
        .with_status(StatusCode::CREATED);
    substitute_public_key(test, bob.id, &original, true).await;
    assert_eq!(document_id_of(test, alice.id, &moved).await, document_id);
    assert_eq!(
        withheld(test, alice.id, BOB).await[0].document_id,
        document_id,
        "a MOVE keeps the id"
    );
    let fingerprint = display_fingerprint(&served(test, bob.id).await);
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200)["delivered"],
        1
    );

    // Cleanup.
    delete_event(&alice, &moved).await;
    alice
        .client
        .request("DELETE", MOVED, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    clear_recipient(test, &bob, &bob_start, "pin-rf4").await;
}

/// Review Focus 5: an account that never pinned lists no pins and is not
/// fail-closed; neither form of `contacts/forget` creates a record or
/// rewraps (spec 4.4, invariant 22).
async fn rf_5_never_pinned(test: &TestServer) {
    let (_, _, carol) = parties(test);
    za_unpin(test, carol.id, STRONG).await;
    assert_eq!(
        contacts(CAROL).await,
        json!({
            "fingerprint": display_fingerprint(&served(test, carol.id).await),
            "stored_key_matches": true,
            "key_repaired_at": null,
            "record_missing": false,
            "pins": []
        })
    );
    assert_eq!(
        forget(CAROL, Some(BOB)).await.expect(200),
        json!({"ok": true})
    );
    assert_eq!(forget(CAROL, None).await.expect(200), json!({"ok": true}));
    assert_eq!(
        raw_pins(test, carol.id).await,
        None,
        "nothing here creates a record"
    );
    assert_eq!(wrap_mark(test, carol.id, STRONG).await, ZaMark::Legacy);
}

/// Final review F3 (spec 6.4): an attendee-side entry after an incomplete
/// drain is `remaining` without spending the call's budget, so an
/// organizer-side entry behind it in the record is still redelivered. bob
/// holds both for alice, his refused answer to her invitation, then his
/// refused invitation to her; carol's invitation, whose drain commit fails,
/// keeps the drain incomplete; the batch is one entry. With the drain
/// complete, the next call redelivers the answer.
async fn fr_1_waiting_entry_spends_no_budget(test: &TestServer) {
    let (alice, bob, carol) = parties(test);
    let (alice_start, bob_start) = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
    );
    // bob pins alice first.
    write_pin_record(
        test,
        bob.id,
        STRONG,
        &record_pinning(ALICE, served(test, alice.id).await),
    )
    .await;
    let path_a = event_path(&alice, "pin-fr1a");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path_a,
            [CONTENT_TYPE],
            invite("pin-fr1a", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(BOB, STRONG).await, 207);
    let copy = copy_href(test, bob.id, "pin-fr1a").await;
    let original = substitute_public_key(test, alice.id, &writer_key(), true).await;
    // The attendee-side entry first: bob's answer, refused.
    assert_eq!(
        rewrite(&bob.client, &copy, |body| with_bob_partstat(
            &body,
            Scope::Master,
            "ACCEPTED"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    // The organizer-side entry after it: bob's invitation, refused.
    let path_b = event_path(&bob, "pin-fr1b");
    bob.client
        .request_with_headers(
            "PUT",
            &path_b,
            [CONTENT_TYPE],
            invite_from(BOB, "pin-fr1b", START_B, &[ALICE]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        attendee_statuses(&bob.client, &path_b, ALICE).await,
        ["3.8"]
    );
    let (copy_id, event_id) = (
        document_id_of(test, bob.id, &copy).await,
        document_id_of(test, bob.id, &path_b).await,
    );
    let documents = |entries: Vec<ZaWithheld>| -> Vec<u32> {
        entries.iter().map(|entry| entry.document_id).collect()
    };
    assert_eq!(
        documents(withheld(test, bob.id, ALICE).await),
        [copy_id, event_id],
        "the attendee-side entry ahead of the organizer-side one"
    );
    substitute_public_key(test, alice.id, &original, true).await;
    // carol's invitation, pending in bob's inbox, fails to drain.
    let path_c = event_path(&carol, "pin-fr1c");
    carol
        .client
        .request_with_headers(
            "PUT",
            &path_c,
            [CONTENT_TYPE],
            invite_from(CAROL, "pin-fr1c", START_A, &[BOB]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let pending = pending_ids(test, bob.id).await;
    assert_eq!(pending.len(), 1, "carol's invitation is pending");
    let fingerprint = display_fingerprint(&served(test, alice.id).await);
    test_hooks::set_drain_fail_item(Some((bob.id, pending[0])));
    test_hooks::set_redeliver_batch_override(Some(1));
    let reply = pin(BOB, ALICE, &fingerprint).await;
    test_hooks::set_redeliver_batch_override(None);
    test_hooks::set_drain_fail_item(None);
    assert_eq!(
        reply.expect(200),
        json!({"ok": true, "delivered": 1, "failed": 0, "dropped": 0, "remaining": 1}),
        "the waiting answer spends no budget: the invitation is delivered"
    );
    assert_eq!(
        documents(withheld(test, bob.id, ALICE).await),
        [copy_id],
        "the answer stays withheld"
    );
    assert_eq!(
        attendee_statuses(&bob.client, &path_b, ALICE).await,
        ["1.2"]
    );
    assert_eq!(
        pin(BOB, ALICE, &fingerprint).await.expect(200),
        json!({"ok": true, "delivered": 1, "failed": 0, "dropped": 0, "remaining": 0}),
        "the drain complete, the answer is redelivered"
    );
    assert!(withheld(test, bob.id, ALICE).await.is_empty());
    assert_eq!(trigger_drain(ALICE, STRONG).await, 207);
    assert_eq!(
        partstat_of(&alice.client, &path_a, BOB).await.as_deref(),
        Some("ACCEPTED")
    );

    // Cleanup.
    delete_event(&alice, &path_a).await;
    delete_event(&bob, &path_b).await;
    delete_event(&carol, &path_c).await;
    clear_recipient(test, &alice, &alice_start, "pin-fr1b").await;
    clear_recipient(test, &bob, &bob_start, "pin-fr1a").await;
    clear_recipient(test, &bob, &bob_start, "pin-fr1c").await;
}

/// Final review F5 (spec 5.2, 6.4 step 5): a failed cleanup leaves each
/// consumed entry withheld and counts it `remaining`, not `delivered`; the
/// delivery stands, and the next call delivers again and retires the
/// entry. `PIN_WRITE_FAIL_ONCE` is set while `contacts/pin` is parked at
/// `REDELIVER_CLEANUP_PAUSE`, after its inner fan-out (whose phase 2 would
/// take it first), so the step 5 write fails.
async fn fr_2_failed_cleanup_counts_remaining(test: &TestServer) {
    let (alice, bob, _) = parties(test);
    let bob_start = inbox_ids(test, bob.id).await;
    let (path, original, _) = refuse_once(test, &alice, &bob, "pin-fr2").await;
    substitute_public_key(test, bob.id, &original, true).await;
    let fingerprint = display_fingerprint(&served(test, bob.id).await);
    let before = inbox_ids(test, bob.id).await;
    let parked = HookParked::start(
        alice.id,
        test_hooks::set_redeliver_cleanup_pause,
        pin_request(ALICE, BOB, &fingerprint),
    )
    .await;
    test_hooks::set_pin_write_fail(Some(alice.id));
    let reply = parked.finish().await;
    test_hooks::set_pin_write_fail(None);
    assert_eq!(
        reply.expect(200),
        json!({"ok": true, "delivered": 0, "failed": 0, "dropped": 0, "remaining": 1}),
        "a failed cleanup counts its entry remaining"
    );
    assert_eq!(
        new_inbox_items(test, bob.id, &before).await.len(),
        1,
        "the delivery stands"
    );
    assert_eq!(withheld(test, alice.id, BOB).await.len(), 1);
    assert_eq!(
        listed(&contacts(ALICE).await, BOB)["withheld"],
        1,
        "the listing agrees with the counts"
    );
    assert_eq!(
        pin(ALICE, BOB, &fingerprint).await.expect(200),
        json!({"ok": true, "delivered": 1, "failed": 0, "dropped": 0, "remaining": 0})
    );
    assert!(withheld(test, alice.id, BOB).await.is_empty());

    // Cleanup.
    delete_event(&alice, &path).await;
    clear_recipient(test, &bob, &bob_start, "pin-fr2").await;
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
/// record is absent. A record that is stored but does not open panics, so
/// an emptiness assertion cannot pass on an unusable record.
pub async fn withheld(test: &TestServer, account_id: u32, address: &str) -> Vec<ZaWithheld> {
    let Some(record) = pin_record(test, account_id, STRONG).await else {
        assert!(
            raw_pins(test, account_id).await.is_none(),
            "the pin record of account {account_id} is stored but unusable"
        );
        return Vec::new();
    };
    record
        .withheld
        .into_iter()
        .filter(|entry| entry.address == address)
        .collect()
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

/// `POST /api/vault/contacts` as `name` (password `STRONG`); 200 expected.
pub async fn contacts(name: &str) -> Value {
    za_post("contacts", &json!({"username": name, "password": STRONG}))
        .await
        .expect(200)
}

/// `POST /api/vault/contacts/lookup` as `name` (password `STRONG`).
pub async fn lookup(name: &str, address: &str) -> VaultReply {
    za_post(
        "contacts/lookup",
        &json!({"username": name, "password": STRONG, "address": address}),
    )
    .await
}

/// `POST /api/vault/contacts/pin` as `name` (password `STRONG`).
pub async fn pin(name: &str, address: &str, fingerprint: &str) -> VaultReply {
    pin_request(name, address, fingerprint).await
}

/// `pin` as an owned future, for a request parked at a pause point.
pub fn pin_request(
    name: &str,
    address: &str,
    fingerprint: &str,
) -> impl Future<Output = VaultReply> + Send + 'static + use<> {
    let body = json!({
        "username": name,
        "password": STRONG,
        "address": address,
        "fingerprint": fingerprint,
    });
    async move { za_post("contacts/pin", &body).await }
}

/// `POST /api/vault/contacts/forget` as `name` (password `STRONG`);
/// `{"address": null}` for `None`.
pub async fn forget(name: &str, address: Option<&str>) -> VaultReply {
    za_post(
        "contacts/forget",
        &json!({"username": name, "password": STRONG, "address": address}),
    )
    .await
}

/// The listing's object for `address`; it must be listed.
pub fn listed<'a>(listing: &'a Value, address: &str) -> &'a Value {
    listing["pins"]
        .as_array()
        .expect("a pins array")
        .iter()
        .find(|pin| pin["address"] == address)
        .unwrap_or_else(|| panic!("{address} is not listed: {listing}"))
}

/// One refused change: the organizer invites `address` (`uid`, in its
/// default calendar), the attendee drains, the attendee's key is
/// substituted with invalidation and the organizer edits the title to
/// `v2`. Returns the event's href, the attendee's original key and the
/// writer's key; the key stays substituted.
pub async fn refuse_once_at(
    test: &TestServer,
    organizer: &Party,
    attendee: &Party,
    address: &str,
    uid: &str,
) -> (String, Vec<u8>, [u8; 32]) {
    let path = event_path(organizer, uid);
    organizer
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            invite_from(organizer.name, uid, START_A, &[address]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(attendee.name, STRONG).await, 207);
    let writer = writer_key();
    let original = substitute_public_key(test, attendee.id, &writer, true).await;
    assert_eq!(
        rewrite(&organizer.client, &path, |body| body.replace(
            &format!("fo-title {uid}"),
            &format!("fo-title {uid} v2")
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    (path, original, writer)
}

/// `refuse_once_at` for the attendee's own address.
pub async fn refuse_once(
    test: &TestServer,
    organizer: &Party,
    attendee: &Party,
    uid: &str,
) -> (String, Vec<u8>, [u8; 32]) {
    refuse_once_at(test, organizer, attendee, attendee.name, uid).await
}

/// Cleanup after a case deleted its events: `lookup`, then `pin`, which
/// drops the entries of gone documents.
pub async fn drop_entries(name: &str, address: &str) {
    let fingerprint = lookup(name, address).await.str(200, "fingerprint");
    let reply = pin(name, address, &fingerprint).await.expect(200);
    assert_eq!(
        (reply["failed"].clone(), reply["remaining"].clone()),
        (json!(0), json!(0)),
        "{reply}"
    );
}

/// The VEVENTs of the event at `href`, as its owner's client reads it.
pub async fn vevent_count(client: &DummyWebDavClient, href: &str) -> usize {
    body(client, href).await.matches("BEGIN:VEVENT").count()
}

/// The `public_key` of the account's vault record.
pub async fn stored_key(test: &TestServer, account_id: u32) -> Vec<u8> {
    test.server
        .za_vault_record(account_id)
        .await
        .unwrap()
        .expect("a vault record")
        .record
        .public_key
}

/// The SUMMARY of each item written since `before`, each opened as the
/// drain would (`open_pending_item_as`), sorted.
pub async fn opened_summaries(test: &TestServer, account_id: u32, before: &[u32]) -> Vec<String> {
    let mut summaries = Vec::new();
    for (document_id, _) in new_inbox_items(test, account_id, before).await {
        let (tree, _) = open_pending_item_as(test, account_id, STRONG, document_id).await;
        summaries.extend(
            tree.components
                .iter()
                .filter(|comp| comp.component_type == ICalendarComponentType::VEvent)
                .flat_map(|comp| comp.entries.iter())
                .find(|entry| entry.name == ICalendarProperty::Summary)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_text())
                .map(str::to_string),
        );
    }
    summaries.sort();
    summaries
}

/// alice's daily event (`RRULE:FREQ=DAILY;COUNT=5` from `START_A`) for
/// `attendees`, with an override X of the second occurrence (`START_B`)
/// naming `override_attendees`.
pub fn recurring_invite(uid: &str, attendees: &[&str], override_attendees: &[&str]) -> String {
    let mut override_x = format!(
        concat!(
            "END:VEVENT\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n",
            "DTSTAMP:20240101T000000Z\r\nRECURRENCE-ID:{start}\r\n",
            "DTSTART:{start}\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
            "SUMMARY:fo-title {uid}\r\nORGANIZER:mailto:key1@example.com\r\n"
        ),
        uid = uid,
        start = START_B
    );
    for attendee in override_attendees {
        override_x.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    override_x.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    invite(uid, START_A, attendees)
        .replace(
            "DURATION:PT1H\r\n",
            "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
        )
        .replace("END:VEVENT\r\nEND:VCALENDAR\r\n", &override_x)
}

/// `recurring_invite` naming bob and carol on the master and on override A
/// (day 2, `START_B`), with a copy of A as override B (day 3).
fn partial_invite(uid: &str) -> String {
    let one = recurring_invite(uid, &[BOB, CAROL], &[BOB, CAROL]);
    let start = one.rfind("BEGIN:VEVENT").unwrap();
    let end = one.rfind("END:VCALENDAR").unwrap();
    let b = one[start..end].replace(START_B, "20990303T100000Z");
    format!("{}{b}{}", &one[..end], &one[end..])
}

/// `body` with the SEQUENCE of the override whose RECURRENCE-ID is
/// `recurrence_id` (UTC) one below its stored value.
fn lower_sequence(body: &str, recurrence_id: &str) -> String {
    let target = format!("RECURRENCE-ID:{recurrence_id}");
    let lowered = body
        .split("BEGIN:VEVENT")
        .map(|part| {
            if !part.contains(&target) {
                return part.to_string();
            }
            part.split("\r\n")
                .map(|line| match line.strip_prefix("SEQUENCE:") {
                    Some(value) => format!("SEQUENCE:{}", value.parse::<i64>().unwrap() - 1),
                    None => line.to_string(),
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        })
        .collect::<Vec<_>>()
        .join("BEGIN:VEVENT");
    assert_ne!(lowered, body, "the override has a SEQUENCE: {body}");
    lowered
}

/// Which components `edit_attendee` edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The component without RECURRENCE-ID.
    Master,
    /// The components with one.
    Overrides,
}

/// `body` unfolded, with each ATTENDEE line of `address` in the `scope`
/// components replaced by what `edit` returns (`None` removes it).
pub fn edit_attendee(
    body: &str,
    address: &str,
    scope: Scope,
    edit: impl Fn(&str) -> Option<String>,
) -> String {
    let target = format!("mailto:{address}");
    body.replace("\r\n ", "")
        .split("BEGIN:VEVENT")
        .enumerate()
        .map(|(at, part)| {
            let is_override = part.contains("RECURRENCE-ID");
            if at == 0 || is_override != (scope == Scope::Overrides) {
                return part.to_string();
            }
            part.split("\r\n")
                .filter_map(|line| {
                    if line.starts_with("ATTENDEE") && line.to_ascii_lowercase().ends_with(&target)
                    {
                        edit(line)
                    } else {
                        Some(line.to_string())
                    }
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        })
        .collect::<Vec<_>>()
        .join("BEGIN:VEVENT")
}

/// An ATTENDEE line with its PARTSTAT set to `partstat` (which may carry
/// further parameters after it).
pub fn with_partstat(line: &str, partstat: &str) -> String {
    match line.find("PARTSTAT=") {
        Some(at) => {
            let end = line[at..].find([';', ':']).map_or(line.len(), |e| at + e);
            format!("{}PARTSTAT={partstat}{}", &line[..at], &line[end..])
        }
        None => line.replacen("ATTENDEE", &format!("ATTENDEE;PARTSTAT={partstat}"), 1),
    }
}

/// bob's copy of a single-instance event with bob DELEGATED to carol and
/// carol's line, DELEGATED-FROM bob, after his.
pub fn delegate_to_carol(body: String) -> String {
    edit_attendee(&body, BOB, Scope::Master, |line| {
        Some(format!(
            "{}\r\nATTENDEE;DELEGATED-FROM=\"mailto:{BOB}\";PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{CAROL}",
            with_partstat(line, &format!("DELEGATED;DELEGATED-TO=\"mailto:{CAROL}\""))
        ))
    })
}

/// The items written to the account's inbox since `before`, each opened as
/// the drain would (`open_pending_item_as`), in document-id order.
pub async fn opened(test: &TestServer, account_id: u32, before: &[u32]) -> Vec<ICalendar> {
    let mut ids: Vec<u32> = new_inbox_items(test, account_id, before)
        .await
        .into_iter()
        .map(|(document_id, _)| document_id)
        .collect();
    ids.sort_unstable();
    let mut items = Vec::with_capacity(ids.len());
    for document_id in ids {
        items.push(
            open_pending_item_as(test, account_id, STRONG, document_id)
                .await
                .0,
        );
    }
    items
}

/// The METHOD of each item.
fn methods(items: &[ICalendar]) -> Vec<Option<ICalendarMethod>> {
    items.iter().map(item_method).collect()
}

/// The SEQUENCE of the component whose RECURRENCE-ID text is
/// `recurrence_id` (UTC).
fn sequence_of(ical: &ICalendar, recurrence_id: &str) -> Option<i64> {
    instance_of(ical, Some(recurrence_id))?
        .entries
        .iter()
        .find(|entry| entry.name == ICalendarProperty::Sequence)
        .and_then(|entry| entry.values.first())
        .and_then(|value| value.as_integer())
}

/// The PARTSTAT of `address` in the event at `href` as its owner reads it,
/// in the component whose RECURRENCE-ID text is `recurrence_id` (UTC), or
/// in the master for `None`.
pub async fn partstat_in(
    client: &DummyWebDavClient,
    href: &str,
    recurrence_id: Option<&str>,
    address: &str,
) -> Option<String> {
    let ical = parse(&body(client, href).await);
    instance_of(&ical, recurrence_id)?
        .entries
        .iter()
        .filter(|entry| {
            entry.name == ICalendarProperty::Attendee
                && entry
                    .values
                    .first()
                    .and_then(|value| value.as_text())
                    .is_some_and(|value| {
                        value
                            .trim_start_matches("mailto:")
                            .eq_ignore_ascii_case(address)
                    })
        })
        .flat_map(|entry| entry.params.iter())
        .find_map(|param| match (&param.name, &param.value) {
            (ICalendarParameterName::Partstat, ICalendarParameterValue::Partstat(status)) => {
                Some(status.as_str().to_string())
            }
            _ => None,
        })
}

/// The STATUS of the component `partstat_in` reads.
async fn status_in(
    client: &DummyWebDavClient,
    href: &str,
    recurrence_id: Option<&str>,
) -> Option<ICalendarStatus> {
    let ical = parse(&body(client, href).await);
    instance_of(&ical, recurrence_id)?.status().cloned()
}

/// The VEVENT whose RECURRENCE-ID text is `recurrence_id` (UTC), or the one
/// without RECURRENCE-ID for `None`.
fn instance_of<'x>(
    ical: &'x ICalendar,
    recurrence_id: Option<&str>,
) -> Option<&'x ICalendarComponent> {
    ical.components
        .iter()
        .filter(|comp| comp.component_type == ICalendarComponentType::VEvent)
        .find(|comp| {
            let text = comp
                .entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::RecurrenceId)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_partial_date_time())
                .map(|date| {
                    let mut text = String::new();
                    date.format_as_ical(&mut text, &ICalendarValueType::DateTime)
                        .unwrap();
                    text
                });
            text.as_deref() == recurrence_id
        })
}

fn parse(text: &str) -> ICalendar {
    match calcard::Parser::new(text).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

/// `fanout::invite` with `RRULE:FREQ=DAILY;COUNT=5` and no override.
fn daily_invite(uid: &str, attendees: &[&str]) -> String {
    invite(uid, START_A, attendees).replace(
        "DURATION:PT1H\r\n",
        "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
    )
}

/// `body` with an override of `recurrence_id` (UTC) of alice's event `uid`
/// appended: `extra` (whole lines), then an ATTENDEE at NEEDS-ACTION for
/// each of `attendees`.
fn with_override(
    body: &str,
    uid: &str,
    recurrence_id: &str,
    extra: &str,
    attendees: &[&str],
) -> String {
    let mut component = format!(
        concat!(
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
            "RECURRENCE-ID:{start}\r\nDTSTART:{start}\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
            "{extra}SUMMARY:fo-title {uid}\r\nORGANIZER:mailto:key1@example.com\r\n"
        ),
        uid = uid,
        start = recurrence_id,
        extra = extra
    );
    for attendee in attendees {
        component.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    component.push_str("END:VEVENT\r\n");
    let end = body.rfind("END:VCALENDAR").expect("a calendar");
    format!("{}{component}{}", &body[..end], &body[end..])
}

/// `body` unfolded, with the lines of the override whose RECURRENCE-ID is
/// `recurrence_id` (UTC) replaced by what `edit` returns (`None` removes
/// one).
fn edit_override(body: &str, recurrence_id: &str, edit: impl Fn(&str) -> Option<String>) -> String {
    let target = format!("RECURRENCE-ID:{recurrence_id}");
    assert!(
        body.contains(&target),
        "no override {recurrence_id}: {body}"
    );
    body.replace("\r\n ", "")
        .split("BEGIN:VEVENT")
        .map(|part| {
            if !part.contains(&target) {
                return part.to_string();
            }
            part.split("\r\n")
                .filter_map(&edit)
                .collect::<Vec<_>>()
                .join("\r\n")
        })
        .collect::<Vec<_>>()
        .join("BEGIN:VEVENT")
}

/// A SEQUENCE line one higher; any other line unchanged.
fn bumped(line: &str) -> String {
    match line.strip_prefix("SEQUENCE:") {
        Some(value) => format!("SEQUENCE:{}", value.parse::<i64>().unwrap() + 1),
        None => line.to_string(),
    }
}

/// `body` with the SEQUENCE of the override whose RECURRENCE-ID is
/// `recurrence_id` (UTC) one above its stored value, as a client bumps it.
fn bump_sequence(body: &str, recurrence_id: &str) -> String {
    let bumped_body = edit_override(body, recurrence_id, |line| Some(bumped(line)));
    assert_ne!(
        bumped_body,
        body.replace("\r\n ", ""),
        "the override has a SEQUENCE: {body}"
    );
    bumped_body
}

/// `body` without the override whose RECURRENCE-ID is `recurrence_id`
/// (UTC), as a client deletes one occurrence.
fn without_override(body: &str, recurrence_id: &str) -> String {
    const END: &str = "END:VEVENT\r\n";
    let at = body
        .find(&format!("RECURRENCE-ID:{recurrence_id}"))
        .unwrap_or_else(|| panic!("no override {recurrence_id}: {body}"));
    let start = body[..at].rfind("BEGIN:VEVENT").unwrap();
    let end = at + body[at..].find(END).unwrap() + END.len();
    format!("{}{}", &body[..start], &body[end..])
}

/// `body` with bob's PARTSTAT set to `partstat` in the `scope` components.
fn with_bob_partstat(body: &str, scope: Scope, partstat: &str) -> String {
    edit_attendee(body, BOB, scope, |line| Some(with_partstat(line, partstat)))
}
