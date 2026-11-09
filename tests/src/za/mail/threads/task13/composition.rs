/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::utils::za::{HookParked, read_manifest, search_config_guard};
use common::za::{
    ZA_MAIL_SEARCH_UNAVAILABLE,
    test_hooks::{set_search_swap_pause, set_search_view_pause},
};
use email::cache::MessageCacheFetch;
use std::time::Duration;
use store::za_search::manifest::{Manifest, ManifestValue};

const DEADLINE: Duration = Duration::from_secs(5);

pub(super) async fn test(test: &mut TestServer) {
    for (name, pinned, churn) in [
        ("keymail-task13-query-fresh@example.com", false, false),
        ("keymail-task13-query-pinned@example.com", true, false),
        ("keymail-task13-query-current-row@example.com", true, true),
    ] {
        let key = key_mail_account(test, name, STRONG).await;
        case_query_composition(test, &key, pinned, churn).await;
        test.destroy_all_mailboxes(&key).await;
    }
}

pub(super) async fn manifest(test: &TestServer, key: &Account) -> Manifest {
    match read_manifest(test, key.id().document_id()).await {
        Some(ManifestValue::Index(manifest)) => manifest,
        other => panic!("expected an admitted index, got {other:?}"),
    }
}

async fn case_query_composition(test: &TestServer, key: &Account, pinned: bool, churn: bool) {
    println!("query composition pinned={pinned} churn={churn}");
    let account_id = key.id().document_id();
    let root = message_with("projected conversation", "A");
    let separate = message_with("projected conversation", "D");
    let a = import(key, root.clone(), "2026-01-02T00:00:00Z").await;
    let d = import(key, separate.clone(), "2026-01-05T00:00:00Z").await;
    let u = import(
        key,
        message_with("query paging witness", "U"),
        "2026-01-06T00:00:00Z",
    )
    .await;
    let h = if churn {
        let msg = with_references(
            message_with("Re: projected conversation", "deleted H"),
            &[message_id(&root)],
        );
        Some(import(key, msg, "2026-01-07T00:00:00Z").await)
    } else {
        None
    };
    test.wait_for_tasks().await;

    let (s, before) = query(key, true).await;
    assert_eq!(before, h.map_or([u, d, a], |h| [h, u, d]));
    assert_ne!(a.prefix_id(), d.prefix_id());
    let u_change = if pinned {
        flag_seen(key, u).await;
        let records = range(test, key, SyncCollection::Email, exact(&s), u64::MAX).await;
        let [(id, changes)] = records.records.as_slice() else {
            panic!("expected only the raw flag update: {records:?}");
        };
        assert!(changes.contains(&Change::UpdateItem(u.id())));
        Some(*id)
    } else {
        None
    };

    let member = with_references(
        message_with("Re: projected conversation", "C"),
        &[message_id(&root)],
    );
    let c = import(key, member, "2026-01-04T00:00:00Z").await;
    test.wait_for_tasks().await;
    let (e, at_e) = query(key, true).await;
    assert_eq!(c.prefix_id(), a.prefix_id());
    assert_eq!(at_e, h.map_or([u, d, c], |h| [h, u, d]));
    let e_id = exact(&e);
    assert_eq!(manifest(test, key).await.newest_row.change_id, e_id);
    let raw = range(test, key, SyncCollection::Email, exact(&s), e_id).await;
    let raw_c = Id::from_parts(c.document_id(), c.document_id()).id();
    assert!(contains(&raw, Change::InsertItem(raw_c)));
    assert!(contains(&raw, Change::UpdateItem(raw_c)));
    assert!(raw.records.iter().all(|(_, entries)| {
        entries.iter().all(|entry| {
            entry
                .item_id()
                .is_none_or(|id| id as u32 != d.document_id())
        })
    }));

    let mut old = s.clone();
    let bridge = with_references(
        message_with("Re: projected conversation", "B"),
        &[message_id(&root), message_id(&separate)],
    );
    let result = if let Some(u_change) = u_change {
        assert!(u_change < e_id);
        let first = query_diff_with(key, &s, true, json!({"maxChanges": 1})).await;
        let first = response(&first, "Email/queryChanges");
        assert_eq!(state(first, "oldQueryState"), s);
        old = state(first, "newQueryState");
        assert_eq!(old, State::new_intermediate(exact(&s), e_id, 1));
        assert_eq!(ids(first, "removed"), [u]);
        // With H present it ranks ahead of U at E. The update is still the
        // only item on the page.
        assert_eq!(added(first), [(u, usize::from(churn))]);

        let initial_pages = if !churn {
            let mut tokens = Vec::new();
            for method in ["Email/changes", "Thread/changes"] {
                let first = changes(key, method, &State::Initial, Some(1)).await;
                let (token, end, _) = first_page(response(&first, method));
                assert_eq!(end, e_id);
                tokens.push((method, token));
            }
            tokens
        } else {
            Vec::new()
        };
        if let Some(h) = h {
            let new = with_references(
                message_with("Re: projected conversation", "new highest ranked member"),
                &[message_id(&root)],
            );
            let n = import(key, new, "2026-01-09T00:00:00Z").await;
            destroy(key, h).await;
            flag_seen(key, c).await;
            let suffix = range(test, key, SyncCollection::Email, e_id, u64::MAX).await;
            assert!(contains(
                &suffix,
                Change::InsertItem(Id::from_parts(n.document_id(), n.document_id()).id())
            ));
            assert!(
                suffix
                    .records
                    .iter()
                    .any(|(_, entries)| entries.iter().any(|entry| {
                        matches!(entry, Change::DeleteItem(raw) if *raw as u32 == h.document_id())
                    }))
            );
        }
        import(key, bridge, "2026-01-01T00:00:00Z").await;
        test.wait_for_tasks().await;
        let f = manifest(test, key).await.newest_row.change_id;
        let cache = test.server.get_cached_messages(account_id).await.unwrap();
        assert!(e_id < f && f <= cache.last_change_id);
        for (method, token) in initial_pages {
            let reply = changes(key, method, &token, None).await;
            let page = response(&reply, method);
            assert_initial_page(page, &token);
            assert_eq!(state(page, "newState"), e);
            let expected = if method == "Email/changes" {
                vec![d, u, c]
            } else {
                vec![Id::from(d.prefix_id()), Id::from(u.prefix_id())]
            };
            assert_eq!(ids(page, "created"), expected);
        }
        if let Some(h) = h {
            // $seen changed on C after E. Use its current keyword, group only
            // eligible current rows at E, and re-emit H's historical ID.
            let result = query_diff_with(
                key,
                &old,
                true,
                json!({
                    "filter": {"someInThreadHaveKeyword": "$seen"},
                    "calculateTotal": true,
                }),
            )
            .await;
            let diff = response(&result, "Email/queryChanges");
            assert_eq!(ids(diff, "removed"), [a, h]);
            assert_eq!(added(diff), [(c, 1)]);
            assert_eq!(diff["total"], 2); // U and T; D is still untagged at E.
            assert_eq!(state(diff, "newQueryState"), e);

            let content = query_diff_with(
                key,
                &old,
                true,
                json!({
                    "filter": {"operator": "AND", "conditions": [
                        {"subject": "projected conversation"},
                        {"someInThreadHaveKeyword": "$seen"},
                    ]},
                    "calculateTotal": true,
                }),
            )
            .await;
            let content = response(&content, "Email/queryChanges");
            assert_eq!(added(content), [(c, 0)]);
            assert_eq!(ids(content, "removed"), [a, h]);
            assert_eq!(content["total"], 1);
            assert_eq!(state(content, "newQueryState"), e);

            // The current merged ID of D must not be used for the cutoff.
            let result = query_diff_with(
                key,
                &old,
                false,
                json!({"upToId": d, "calculateTotal": true}),
            )
            .await;
            let cutoff = response(&result, "Email/queryChanges");
            assert!(added(cutoff).is_empty(), "{cutoff}");
            assert!(ids(cutoff, "removed").is_empty());
            assert_eq!(cutoff["total"], 4); // A, D, C and U; H has no row.
            assert_eq!(state(cutoff, "newQueryState"), e);
            let future_d = Id::from_parts(a.prefix_id(), d.document_id());
            assert_ne!(future_d, d);
            let reply = query_diff_with(
                key,
                &old,
                false,
                json!({"upToId": future_d, "calculateTotal": true}),
            )
            .await;
            let future_cutoff = response(&reply, "Email/queryChanges");
            assert_eq!(added(future_cutoff), [(c, 2)]);
            assert_eq!(future_cutoff["total"], 4);
            assert_eq!(state(future_cutoff, "newQueryState"), e);
        }
        query_diff(key, &old, true).await
    } else {
        let reader = key.clone();
        let since = old.clone();
        let parked = HookParked::start(account_id, set_search_view_pause, async move {
            query_diff(&reader, &since, true).await
        })
        .await;
        let thread_reader = key.clone();
        let thread_read = HookParked::start(account_id, set_search_view_pause, async move {
            thread_reader
                .jmap_method_call(
                    "Thread/get",
                    json!({"accountId": thread_reader.id_string()}),
                )
                .await
        })
        .await;
        let writer = key.clone();
        let publishing = HookParked::start(account_id, set_search_swap_pause, async move {
            import(&writer, bridge, "2026-01-01T00:00:00Z").await
        })
        .await;
        assert!(manifest(test, key).await.newest_row.change_id > e_id);
        let result = tokio::time::timeout(DEADLINE, parked.finish())
            .await
            .expect("queryChanges acquired another view behind the publication");
        let thread = thread_read.finish().await;
        let thread = response(&thread, "Thread/get");
        assert_eq!(state(thread, "state"), e);
        let list = thread["list"].as_array().unwrap();
        assert!(list.iter().any(|thread| ids(thread, "emailIds") == [d]));
        assert!(
            list.iter()
                .any(|thread| unordered(ids(thread, "emailIds")) == unordered(vec![a, c]))
        );
        tokio::time::timeout(DEADLINE, publishing.finish())
            .await
            .unwrap();
        let changes = changes(key, "Thread/changes", &e, None).await;
        let changes = response(&changes, "Thread/changes");
        assert_eq!(ids(changes, "destroyed"), [Id::from(d.prefix_id())]);
        assert_eq!(ids(changes, "updated"), [Id::from(a.prefix_id())]);
        result
    };
    let diff = response(&result, "Email/queryChanges");
    assert_eq!(state(diff, "oldQueryState"), old);
    assert_eq!(state(diff, "newQueryState"), e);
    if let Some(h) = h {
        assert_eq!(ids(diff, "removed"), [a, h]);
    } else {
        assert_eq!(ids(diff, "removed"), [a]);
        assert_diff_applies(before, diff, &at_e);
    }
    assert_eq!(added(diff), [(c, 2)]);

    let (f, now) = query(key, true).await;
    assert!(exact(&f) > e_id);
    let (_, all) = query(key, false).await;
    let moved_d = by_document(&all, d);
    assert_ne!(moved_d, d);
    assert_eq!(moved_d.prefix_id(), a.prefix_id());
    assert!(!added(diff).iter().any(|(id, _)| *id == moved_d));
    if !churn {
        let reply = query_diff(key, &e, true).await;
        let diff = response(&reply, "Email/queryChanges");
        assert_eq!(added(diff), [(moved_d, 1)]);
        assert!(ids(diff, "removed").contains(&d));
        assert_eq!(state(diff, "newQueryState"), f);
        assert_diff_applies(at_e, diff, &now);
    }

    let reply = key
        .jmap_method_call(
            "Email/query",
            json!({"accountId": key.id_string(), "filter": {"subject": "projected conversation"}}),
        )
        .await;
    let latest = state(response(&reply, "Email/query"), "queryState");
    let _deadline = search_config_guard!(test, wait_secs: 0);
    let reply = query_diff_with(
        key,
        &latest,
        false,
        json!({"filter": {"subject": "projected conversation"}}),
    )
    .await;
    let error = response(&reply, "error");
    assert_eq!(error["type"], "unsupportedFilter");
    assert_eq!(
        error["description"],
        format!("subject: {ZA_MAIL_SEARCH_UNAVAILABLE}")
    );
}
