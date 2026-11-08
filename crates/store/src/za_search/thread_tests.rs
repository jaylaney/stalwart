/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::xxhash_rust::xxh3::xxh3_128;
use rand::{RngExt, SeedableRng, rngs::SmallRng};

fn msg(_document_id: u32, subject: u128, hashes: &[u128]) -> ThreadFieldsK {
    ThreadFieldsK {
        hashes: hashes.to_vec(),
        subject_hash: subject,
        sent_at: None,
    }
}

fn indexed(items: &[(u32, u128, &[u128])]) -> ThreadPart {
    let mut part = ThreadPart::default();
    for &(id, subject, hashes) in items {
        part.assign(&msg(id, subject, hashes), id, 1, u64::from(id) * 10);
    }
    part
}

fn row(revision: u32, change_id: u64) -> RevisionRow {
    RevisionRow {
        revision,
        change_id,
    }
}

fn alias(source: u32, target: u32, revision: u32) -> Alias {
    Alias {
        source,
        target,
        revision,
    }
}

fn record(document_id: u32, thread_id: u32, revision: u32, created: u64) -> ThreadRecord {
    ThreadRecord {
        document_id,
        thread_id,
        revision,
        created,
        deleted: None,
        subject_hash: 1,
        hashes: vec![u128::from(document_id)],
    }
}

#[test]
fn s61_no_candidate_is_a_new_thread_of_its_own_id() {
    let mut part = ThreadPart::default();
    let assignment = part.assign(&msg(5, 1, &[0x55]), 5, 1, 50);
    assert_eq!(assignment.thread_id, 5);
    assert!(assignment.aliases.is_empty());
    assert_eq!(part.present_members(5), [5]);
}

#[test]
fn s61_one_candidate_thread_joins_it() {
    let mut part = indexed(&[(1, 1, &[0x11])]);
    let assignment = part.assign(&msg(2, 1, &[0x22, 0x11]), 2, 1, 20);
    assert_eq!(assignment.thread_id, 1);
    assert!(assignment.aliases.is_empty());
    assert_eq!(part.present_members(1), [1, 2]);
}

#[test]
fn s61_subject_bucket_must_match() {
    let mut part = indexed(&[(1, 7, &[0x11])]);
    assert_eq!(
        part.assign(&msg(2, 8, &[0x22, 0x11]), 2, 1, 20).thread_id,
        2
    );
}

#[test]
fn s61_a_tombstoned_record_is_not_a_candidate() {
    let mut part = indexed(&[(1, 1, &[0x11])]);
    assert!(part.tombstone(1, 15));
    assert_eq!(
        part.assign(&msg(2, 1, &[0x22, 0x11]), 2, 1, 20).thread_id,
        2
    );
    let tombstone = part.record(1).unwrap();
    assert_eq!(tombstone.deleted, Some(15));
    assert!(tombstone.hashes.is_empty());
    assert_eq!(tombstone.thread_id, 1);
}

#[test]
fn h10_survivor_is_the_thread_with_most_candidates_ties_lowest() {
    let mut part = indexed(&[
        (3, 1, &[0x33]),
        (5, 1, &[0x55]),
        (6, 1, &[0x66, 0x55]),
        (9, 1, &[0x99]),
        (4, 1, &[0x44, 0x33]),
    ]);
    let mut assignment = part.assign(
        &msg(10, 1, &[0xa0, 0x33, 0x44, 0x55, 0x66, 0x99]),
        10,
        2,
        100,
    );
    assignment
        .aliases
        .sort_unstable_by_key(|alias| alias.source);
    assert_eq!(assignment.thread_id, 3);
    assert_eq!(assignment.aliases, [alias(5, 3, 2), alias(9, 3, 2)]);
    assert_eq!(part.present_members(3), [3, 4, 5, 6, 9, 10]);
    assert!(part.present_members(5).is_empty());
    assert!(part.present_members(9).is_empty());
    assert_eq!(part.current_thread(6), 3);
    assert_eq!(part.record(6).unwrap().thread_id, 5);

    // Thread 1 has more total members. Its root alone carries the queried 0x11.
    let mut part = indexed(&[
        (1, 1, &[0x11, 0x12]),
        (2, 1, &[0x22, 0x12]),
        (3, 1, &[0x33, 0x22]),
        (4, 1, &[0x44, 0x33]),
        (5, 1, &[0x55]),
        (6, 1, &[0x66, 0x55]),
    ]);
    let assignment = part.assign(&msg(7, 1, &[0x11, 0x55, 0x66]), 7, 2, 70);
    assert_eq!(assignment.thread_id, 5);
    assert_eq!(assignment.aliases, [alias(1, 5, 2)]);
}

#[test]
fn s41_duplicate_message_ids_share_the_root() {
    let mut part = indexed(&[(1, 1, &[0x11])]);
    assert_eq!(part.assign(&msg(2, 1, &[0x11]), 2, 1, 20).thread_id, 1);
    let assignment = part.assign(&msg(3, 1, &[0x33, 0x11]), 3, 1, 30);
    assert_eq!(assignment.thread_id, 1);
    assert!(assignment.aliases.is_empty());
}

#[test]
fn s61_empty_threading_fields_are_a_singleton_and_never_a_candidate() {
    // find_thread_id returns early for an empty message_ids list (ingest.rs:1113-1115).
    let empty_subject = xxh3_128(b"!");
    let mut part = ThreadPart::default();
    for (id, hashes) in [(1, &[][..]), (2, &[][..]), (3, &[0x33][..])] {
        let assignment = part.assign(&msg(id, empty_subject, hashes), id, 1, u64::from(id) * 10);
        assert_eq!(assignment.thread_id, id);
        assert!(assignment.aliases.is_empty());
        assert_eq!(part.present_members(id), [id]);
    }
}

#[test]
fn r20_k_references_in_k_distinct_threads_write_exactly_k_minus_one_aliases() {
    let ids = 1..=K as u32;
    let hashes = ids.clone().map(u128::from).collect::<Vec<_>>();
    let mut part = ThreadPart::default();
    for id in ids {
        part.assign(&msg(id, 1, &[u128::from(id)]), id, 1, u64::from(id) * 10);
    }
    let assignment = part.assign(&msg(17, 1, &hashes), 17, 2, 170);
    assert_eq!(assignment.thread_id, 1);
    assert_eq!(assignment.aliases.len(), K - 1);
}

#[test]
fn r20_no_assignment_writes_more_than_k_minus_one_aliases() {
    let mut rng = SmallRng::seed_from_u64(0x7a_5ea2c4);
    for _ in 0..200 {
        let mut part = ThreadPart::default();
        for id in 1..=60 {
            let hashes = (0..rng.random_range(1..=K))
                .map(|_| rng.random_range(1..=40))
                .collect::<Vec<_>>();
            let assignment = part.assign(&msg(id, 1, &hashes), id, 1, u64::from(id) * 10);
            assert!(assignment.aliases.len() <= K - 1);
            assert_eq!(part.records().count(), id as usize);

            let mut threads_by_hash = [None; 41];
            for record in part.records().filter(|record| record.deleted.is_none()) {
                let thread = part.current_thread(record.document_id);
                for &hash in &record.hashes {
                    assert_eq!(
                        *threads_by_hash[hash as usize].get_or_insert(thread),
                        thread,
                        "hash {hash} resolves to several threads after assignment {id}",
                    );
                }
            }
        }
    }
}

#[test]
fn r16_insert_then_delete_in_order_threads_as_upstream_did() {
    let mut chronological = indexed(&[(1, 1, &[0xa]), (2, 1, &[0xd, 0xa])]);
    let mut deletion_first = chronological.clone();
    let incoming = msg(3, 1, &[0xee, 0xd]);
    chronological.assign(&incoming, 3, 2, 30);
    chronological.tombstone(2, 31);
    assert_eq!(chronological.current_thread(3), 1);

    deletion_first.tombstone(2, 31);
    deletion_first.assign(&incoming, 3, 2, 30);
    assert_eq!(deletion_first.current_thread(3), 3);
}

#[test]
fn r16_r20_cursor_chronological_order_threads_as_the_control() {
    let mut part = ThreadPart::default();
    let items: [(u32, &[u128]); 4] = [
        (1, &[0xa]),
        (2, &[0xd, 0xa]),
        (3, &[0xe, 0xd]),
        (4, &[0xf, 0xd]),
    ];
    for (id, hashes) in items {
        let assignment = part.assign(&msg(id, 1, hashes), id, 1, u64::from(id) * 10);
        assert!(assignment.aliases.is_empty());
        for &hash in hashes {
            let query = msg(0, 1, &[hash]);
            let mut candidates = part.matching_documents(&query);
            let first = candidates
                .next()
                .expect("the newly assigned message matches");
            assert!(
                candidates
                    .all(|candidate| part.current_thread(candidate) == part.current_thread(first))
            );
        }
    }
    assert_eq!(part.present_members(1), [1, 2, 3, 4]);
    assert_eq!(part.aliases().count(), 0);
}

#[test]
fn pl2_dependent_inserts_in_one_pass_follow_the_staged_overlay() {
    let mut root = record(1, 1, 1, 10);
    root.hashes = vec![0xa];
    let mut part = ThreadPart::from_parts(vec![root], vec![], vec![row(1, 100)]);
    assert_eq!(part.assign(&msg(2, 1, &[0xd, 0xa]), 2, 2, 20).thread_id, 1);
    let assignment = part.assign(&msg(3, 1, &[0xe, 0xd]), 3, 2, 30);
    assert_eq!(assignment.thread_id, 1);
    assert!(assignment.aliases.is_empty());
    assert_eq!(part.current_thread(3), 1);
    assert_eq!(part.present_members(1), [1, 2, 3]);
    assert_eq!(part.thread_of(3, 100), 3);

    let mut part = indexed(&[(10, 5, &[0x10]), (20, 5, &[0x20])]);
    part.extend([], [], [row(1, 100)], []);
    let assignment = part.assign(&msg(30, 5, &[0x30, 0x10, 0x20]), 30, 2, 300);
    assert_eq!(assignment.thread_id, 10);
    assert_eq!(assignment.aliases, [alias(20, 10, 2)]);
    let assignment = part.assign(&msg(40, 5, &[0x40, 0x20]), 40, 2, 400);
    assert_eq!(assignment.thread_id, 10);
    assert!(assignment.aliases.is_empty());
    assert_eq!(part.current_thread(20), 10);
    assert_eq!(part.thread_of(20, u64::MAX), 20);
}

fn r5_fixture() -> ThreadPart {
    ThreadPart::from_parts(
        vec![record(7, 7, 1, 50)],
        vec![alias(7, 5, 2), alias(5, 3, 3)],
        vec![row(1, 100), row(2, 200), row(3, 300)],
    )
}

#[test]
fn r5_thread_of_d_as_of_x_before_at_between_and_after() {
    let part = r5_fixture();
    for (state, expected) in [
        (99, 7),
        (100, 7),
        (150, 7),
        (200, 5),
        (250, 5),
        (300, 3),
        (u64::MAX, 3),
    ] {
        assert_eq!(part.thread_of(7, state), expected, "state {state}");
    }
    assert_eq!(part.revision_at(99), 0);
    assert_eq!(part.revision_at(250), 2);
    assert_eq!(part.thread_of(8, 1000), 8);
}

fn r25_fixture() -> ThreadPart {
    let mut deleted = record(3, 1, 3, 250);
    deleted.deleted = Some(350);
    deleted.hashes.clear();
    deleted.subject_hash = 0;
    ThreadPart::from_parts(
        vec![record(1, 1, 1, 10), record(2, 2, 1, 20), deleted],
        vec![alias(2, 1, 2)],
        vec![row(1, 100), row(2, 200), row(3, 300)],
    )
}

#[test]
fn r25_members_of_t_as_of_x_across_two_merges_and_a_later_deletion() {
    let part = r25_fixture();
    assert_eq!(part.members_of(1, 150), [1]);
    assert_eq!(part.members_of(2, 150), [2]);
    assert_eq!(part.members_of(1, 250), [1, 2]);
    assert_eq!(part.members_of(1, 300), [1, 2, 3]);
    assert_eq!(part.members_of(1, 400), [1, 2]);
    assert!(part.members_of(2, 400).is_empty());
    assert_eq!(part.present_members(1), [1, 2]);
    assert_eq!(part.members(1), [1, 2, 3]);
}

#[test]
fn s63_exists_at_follows_creation_and_deletion_change_ids() {
    let part = r25_fixture();
    assert_eq!(part.exists_at(3, 249), Some(false));
    assert_eq!(part.exists_at(3, 250), Some(true));
    assert_eq!(part.exists_at(3, 349), Some(true));
    assert_eq!(part.exists_at(3, 350), Some(false));
    assert_eq!(part.exists_at(99, 1), None);
}

#[test]
fn s63_a_deleted_record_resolves_through_its_retained_record() {
    let part = r25_fixture();
    assert_eq!(part.thread_of(3, 300), 1);
    assert_eq!(part.thread_of(3, 400), 1);
}

#[test]
fn s55_fold_alias_moves_one_hop_and_keeps_every_servable_resolution() {
    let mut part = r5_fixture();
    let states = [200, 250, 300, 1000];
    let before = states.map(|state| part.thread_of(7, state));
    assert_eq!(before, [5, 5, 3, 3]);
    assert_eq!(part.fold_sub_bound(|a| a.revision <= 2), 1);
    let record = part.record(7).unwrap();
    assert_eq!(record.thread_id, 5);
    assert_eq!(record.revision, 2);
    assert_eq!(
        part.aliases().copied().collect::<Vec<_>>(),
        [alias(5, 3, 3)]
    );
    assert_eq!(states.map(|state| part.thread_of(7, state)), before);
    assert_eq!(part.present_members(3), [7]);
    assert_eq!(part.members(3), [7]);
}

#[test]
fn pl7_a_same_revision_chain_folds_to_its_end_in_either_order() {
    let mut part = indexed(&[(10, 5, &[10]), (20, 5, &[20]), (30, 5, &[30])]);
    part.extend([], [], [row(1, 100)], []);
    let first = part.assign(&msg(40, 5, &[0x40, 30, 20]), 40, 2, 400);
    assert_eq!(first.thread_id, 20);
    assert_eq!(first.aliases, [alias(30, 20, 2)]);
    let second = part.assign(&msg(41, 5, &[0x41, 0x40, 10]), 41, 2, 410);
    assert_eq!(second.thread_id, 10);
    assert_eq!(second.aliases, [alias(20, 10, 2)]);
    part.extend([], [], [row(2, 200)], []);
    assert_eq!(part.thread_of(30, 200), 10);
    // Tombstones travel through the same alias chain while remaining out of the present table.
    assert!(part.tombstone(30, 500));

    let records: Vec<_> = part.records().cloned().collect();
    let rows = part.rows().to_vec();
    let aliases = [alias(20, 10, 2), alias(30, 20, 2)];
    let forward = ThreadPart::from_parts(records.clone(), aliases.to_vec(), rows.clone());
    let reversed = ThreadPart::from_parts(records, aliases.into_iter().rev().collect(), rows);
    for mut part in [part, forward, reversed] {
        assert_eq!(part.fold_sub_bound(|a| a.revision <= 2), 3);
        assert!(part.records().all(|record| record.thread_id == 10));
        for id in [20, 30, 40] {
            assert_eq!(part.record(id).unwrap().revision, 2);
        }
        assert_eq!(part.aliases().count(), 0);
        assert_eq!(part.thread_of(30, 200), 10);
        assert_eq!(part.present_members(10), [10, 20, 40, 41]);
        assert_eq!(part.members(10), [10, 20, 30, 40, 41]);
        assert!(part.members(20).is_empty());
        assert!(part.members(30).is_empty());
    }
}

#[test]
fn r3_tombstone_keeps_identity_and_drops_fields() {
    let mut part = indexed(&[(1, 1, &[0x11]), (2, 1, &[0x22, 0x11])]);
    let before = part.record(2).unwrap().clone();
    assert!(part.tombstone(2, 350));
    let after = part.record(2).unwrap();
    assert_eq!(after.thread_id, before.thread_id);
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.created, before.created);
    assert_eq!(after.deleted, Some(350));
    assert!(after.hashes.is_empty());
    assert_eq!(after.subject_hash, 0);
    assert!(!part.is_present(2));
    assert_eq!(part.present_members(1), [1]);
    assert_eq!(part.members(1), [1, 2]);
    assert!(!part.tombstone(999, 1));
}

#[test]
fn s43_extend_merges_rows_by_revision_and_ignores_unknown_tombstones() {
    let mut part = ThreadPart::from_parts(vec![record(1, 1, 1, 10)], vec![], vec![row(2, 200)]);
    part.extend(
        [],
        [],
        [row(2, 200), row(1, 100)],
        [Tombstone {
            document_id: 999,
            deleted: 1,
        }],
    );
    assert_eq!(part.rows(), [row(1, 100), row(2, 200)]);
    assert_eq!(part.newest_revision(), 2);
    assert_eq!(part.revision_change_id(1), Some(100));
    assert_eq!(part.revision_change_id(0), None);
    assert_eq!(part.records().count(), 1);
    assert_eq!(part.present_members(1), [1]);
}
