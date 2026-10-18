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
    fanout::{CONTENT_TYPE, Party},
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        pending_ids, pin_record, substitute_public_key, trigger_drain, vault_revision, wrap_mark,
        write_pin_record, writer_key, za_unpin,
    },
};
use common::auth::vault::ZaMark;
use hyper::StatusCode;
use store::write::now;
use vault::pins::{PinSource, ZaPinRecord, ZaWithheld};

pub const ALICE: &str = "key1@example.com";
pub const BOB: &str = "key2@example.com";
pub const CAROL: &str = "key3@example.com";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access key pinning tests...");
    let _drain = DrainOn::new();
    case_03_repair(test).await;
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
