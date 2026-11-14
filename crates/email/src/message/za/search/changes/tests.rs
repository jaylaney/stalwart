/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use Change::*;
use StreamKind::*;
use store::za_search::thread::{Alias, RevisionRow, ThreadRecord};

fn record(document_id: u32, thread_id: u32, revision: u32, created: u64) -> ThreadRecord {
    ThreadRecord {
        document_id,
        thread_id,
        revision,
        created,
        deleted: None,
        subject_hash: 0,
        hashes: vec![],
    }
}

fn history(records: Vec<ThreadRecord>, aliases: &[(u32, u32, u32)], rows: &[u64]) -> ThreadPart {
    ThreadPart::from_parts(
        records,
        aliases
            .iter()
            .map(|&(source, target, revision)| Alias {
                source,
                target,
                revision,
            })
            .collect(),
        rows.iter()
            .enumerate()
            .map(|(index, &change_id)| RevisionRow {
                revision: index as u32 + 1,
                change_id,
            })
            .collect(),
    )
}

fn records_of(records: &[(u64, Change)]) -> ZaChangeRecords {
    ZaChangeRecords {
        records: records
            .iter()
            .map(|&(id, entry)| (id, vec![entry]))
            .collect(),
        ..Default::default()
    }
}

fn email_items(
    thread: &ThreadPart,
    since: u64,
    end: u64,
    records: &[(u64, Change)],
    later: &[(u64, Change)],
    present: &[u32],
) -> Vec<StreamItem> {
    za_email_stream(
        thread,
        &RangeView {
            records: &records_of(records),
            later: &records_of(later),
            present: &|id| present.contains(&id),
            end_is_current: later.is_empty(),
        },
        since,
        end,
    )
}

fn kinds(items: &[StreamItem]) -> Vec<(StreamKind, Id)> {
    items
        .iter()
        .map(|item| (item.kind, item.old.or(item.new).unwrap()))
        .collect()
}

fn id(thread: u32, document: u32) -> Id {
    Id::from_parts(thread, document)
}

#[test]
fn first_assignment_renders_the_singleton_before_its_mapped_id() {
    let thread = history(vec![record(7, 3, 1, 10)], &[], &[12]);
    let witness = (12, UpdateItem(42));
    let items = email_items(&thread, 11, 12, &[witness], &[], &[3, 7]);
    assert_eq!(kinds(&items), [(Destroyed, id(7, 7)), (Created, id(3, 7))]);
    assert!(email_items(&thread, 12, 13, &[], &[], &[3, 7]).is_empty());
    let items = email_items(
        &thread,
        9,
        12,
        &[(10, InsertItem(7)), witness],
        &[],
        &[3, 7],
    );
    assert_eq!(kinds(&items), [(Created, id(3, 7))]);
}

#[test]
fn two_merges_render_at_each_inclusive_boundary() {
    let thread = history(
        vec![record(7, 5, 1, 5)],
        &[(5, 3, 2), (3, 1, 3)],
        &[5, 12, 20],
    );
    for (since, end, old, new) in [(11, 12, 5, 3), (15, 20, 3, 1), (11, 21, 5, 1)] {
        assert_eq!(
            kinds(&email_items(&thread, since, end, &[], &[], &[7])),
            [(Destroyed, id(old, 7)), (Created, id(new, 7))]
        );
    }
    for (since, end) in [(12, 15), (20, 21)] {
        assert!(email_items(&thread, since, end, &[], &[], &[7]).is_empty());
    }
}

#[test]
fn raw_deletion_destroys_the_old_mapped_id_even_if_the_record_lags() {
    let present = record(7, 3, 1, 10);
    let mut deleted = present.clone();
    deleted.deleted = Some(30);
    for record in [present, deleted] {
        let thread = history(vec![record], &[], &[12]);
        assert_eq!(
            kinds(&email_items(
                &thread,
                25,
                30,
                &[(30, DeleteItem(7))],
                &[],
                &[]
            )),
            [(Destroyed, id(3, 7))]
        );
        assert!(email_items(&thread, 30, 31, &[], &[], &[]).is_empty());
    }
}

#[test]
fn inserted_then_deleted_in_the_range_is_absent_at_both_ends() {
    let records = [(13, InsertItem(9)), (14, DeleteItem(9))];
    for thread in [
        ThreadPart::default(),
        history(vec![record(9, 3, 1, 13)], &[], &[13]),
    ] {
        assert!(email_items(&thread, 12, 15, &records, &[], &[]).is_empty());
    }
}

#[test]
fn lagging_index_does_not_override_presence_at_either_endpoint() {
    let thread = history(vec![record(9, 3, 1, 13)], &[], &[13]);
    let delete = (16, DeleteItem(9));
    assert_eq!(
        kinds(&email_items(
            &thread,
            12,
            15,
            &[(13, InsertItem(9))],
            &[delete],
            &[]
        )),
        [(Created, id(3, 9))]
    );
    assert_eq!(
        kinds(&email_items(&thread, 15, 16, &[delete], &[], &[])),
        [(Destroyed, id(3, 9))]
    );
    assert!(email_items(&thread, 16, 18, &[(18, UpdateItem(9))], &[], &[]).is_empty());
}

#[test]
fn earliest_raw_or_sealed_change_orders_candidates_then_document_id() {
    let thread = history(
        vec![record(2, 7, 1, 5), record(4, 7, 1, 5)],
        &[(7, 3, 2), (3, 1, 3)],
        &[5, 15, 20],
    );
    let items = email_items(
        &thread,
        11,
        20,
        &[(13, UpdateItem(9)), (19, UpdateItem(4))],
        &[],
        &[2, 4, 9],
    );
    assert_eq!(
        kinds(&items),
        [
            (Updated, id(9, 9)),
            (Destroyed, id(7, 2)),
            (Created, id(1, 2)),
            (Destroyed, id(7, 4)),
            (Created, id(1, 4)),
        ]
    );
    assert_eq!(
        items.iter().map(|item| item.order).collect::<Vec<_>>(),
        [(13, 9), (15, 2), (15, 2), (15, 4), (15, 4)]
    );
}

#[test]
fn the_epoch_refuses_only_the_methods_kind_in_the_closed_end_of_the_range() {
    let item = records_of(&[(9, UpdateItem(1))]);
    let container = records_of(&[(9, UpdateContainerPartial(1, Default::default()))]);
    assert!(za_epoch_refuses(&item, false, 5, 9));
    assert!(za_epoch_refuses(&container, true, 5, 9));
    assert!(!za_epoch_refuses(&item, true, 5, 9));
    assert!(!za_epoch_refuses(&container, false, 5, 9));
    assert!(!za_epoch_refuses(&item, false, 0, 9));
    assert!(!za_epoch_refuses(&item, false, 9, 9));
    assert!(!za_epoch_refuses(&item, false, 5, 8));
}

#[test]
fn initial_end_restores_later_deletes_excludes_later_inserts_and_maps_at_e() {
    let thread = history(vec![record(5, 2, 1, 10)], &[(2, 1, 2)], &[10, 110]);
    let later = records_of(&[
        (110, InsertItem(9)),
        (120, DeleteItem(2)),
        (130, InsertItem(11)),
        (140, DeleteItem(11)),
    ]);
    let end = za_present_at_end([5, 9].into_iter(), &later);
    assert_eq!(end.iter().collect::<Vec<_>>(), [2, 5]);
    assert_eq!(
        kinds(&za_initial_stream(&thread, end.iter(), 100)),
        [(Created, id(2, 2)), (Created, id(2, 5))]
    );
    assert_eq!(
        kinds(&za_initial_stream(&thread, [9, 5, 2].into_iter(), 140)),
        [
            (Created, id(2, 2)),
            (Created, id(1, 5)),
            (Created, id(9, 9))
        ]
    );
}

#[test]
fn a_thread_counts_the_member_deleted_after_the_end_and_ignores_flag_only_updates() {
    for (created, change, expected) in [
        (20, InsertItem(7), vec![(3, Updated)]),
        (18, UpdateItem(7), vec![]),
    ] {
        let thread = history(
            vec![record(3, 3, 1, 5), record(7, 3, 2, created)],
            &[],
            &[5, 18],
        );
        let items = za_thread_stream(
            &thread,
            &RangeView {
                records: &records_of(&[(20, change)]),
                later: &records_of(&[(25, DeleteItem(7))]),
                present: &|id| id == 3,
                end_is_current: false,
            },
            19,
            20,
        );
        assert_eq!(
            items
                .into_iter()
                .map(|item| (item.thread_id, item.kind))
                .collect::<Vec<_>>(),
            expected
        );
    }
}
