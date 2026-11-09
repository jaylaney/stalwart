/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::{
    utils::za::{pop3_delete, search_config_guard},
    za::mail::keyed_token,
};
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_MAIL_SEARCH_UNAVAILABLE, test_hooks::set_search_cache_hold},
};
use email::cache::{MessageCacheFetch, email::MessageCacheAccess};
use jmap::changes::{get::ChangesLookup, query::QueryChanges};
use jmap_proto::{
    method::changes::{ChangesRequest, ChangesResponse},
    object::NullObject,
    request::{QueryChangesRequestMethod, capability::CapabilityIds, method::MethodObject},
};
use std::sync::Arc;

struct HoldCache;

impl HoldCache {
    fn new(account: u32) -> Self {
        set_search_cache_hold(Some(account));
        Self
    }
}

impl Drop for HoldCache {
    fn drop(&mut self) {
        set_search_cache_hold(None);
    }
}

async fn node_changes(
    server: &Server,
    token: &AccessToken,
    since: State,
) -> trc::Result<ChangesResponse<NullObject>> {
    server
        .changes(
            ChangesRequest {
                account_id: Id::from(token.account_id()),
                since_state: since,
                max_changes: Some(1),
                ignore_metadata_only_changes: None,
            },
            MethodObject::Email,
            token,
            CapabilityIds(0),
        )
        .await
        .map(|page| page.response)
}

async fn node_query_changes(
    server: &Server,
    token: &AccessToken,
    since: &State,
    filter: Value,
) -> trc::Result<Value> {
    let mut args = json!({
        "accountId": Id::from(token.account_id()),
        "sinceQueryState": since.to_string(),
        "calculateTotal": true,
        "sort": [{"property": "receivedAt", "isAscending": false}],
    });
    if !filter.is_null() {
        args["filter"] = filter;
    }
    let args = serde_json::from_str(&args.to_string()).unwrap();
    server
        .query_changes(
            QueryChangesRequestMethod::Email(Box::new(args)),
            token,
            CapabilityIds(0),
        )
        .await
        .map(|response| serde_json::to_value(response).unwrap())
}

pub(super) async fn test(test: &mut TestServer) {
    println!("token minimum and exhausted queryChanges fallback");
    let key = key_mail_account(test, "keymail-task13-minimum@example.com", STRONG).await;
    let account_id = key.id().document_id();
    let live = three_singletons(&key).await;
    flag_seen(&key, live[0]).await;
    let (warm_state, _) = query(&key, false).await;
    let before = composition::manifest(test, &key).await;
    let token = keyed_token(test, account_id, key.secret()).await;
    let node = super::super::races::separate_publisher(test).await;
    let warm = node.get_cached_messages(account_id).await.unwrap();
    assert_eq!(warm.last_change_id, exact(&warm_state));
    assert!(warm.last_change_id > before.newest_row.change_id.max(before.epoch));
    assert!(!Arc::ptr_eq(
        &warm,
        &test.server.get_cached_messages(account_id).await.unwrap()
    ));
    let warmed = node_changes(&node, &token, State::Initial).await.unwrap();
    assert_eq!(warmed.created, [live[0]]);

    pop3_delete(test, &key, live[0].document_id()).await;
    let first = changes(&key, "Email/changes", &State::Initial, Some(1)).await;
    let (initial, end, first) = first_page(response(&first, "Email/changes"));
    assert_eq!(first, live[1]);
    let refreshed = test.server.get_cached_messages(account_id).await.unwrap();
    assert_eq!(refreshed.last_change_id, end);
    assert!(warm.last_change_id < end);
    assert!(!refreshed.has_email_id(&live[0].document_id()));
    let after = composition::manifest(test, &key).await;
    assert_eq!(after.newest_row, before.newest_row);
    assert_eq!(after.epoch, before.epoch);
    assert!(retained_bound(test, &key).await <= end);
    let suffix = range(test, &key, SyncCollection::Email, warm.last_change_id, end).await;
    assert!(!suffix.truncated && contains(&suffix, Change::DeleteItem(live[0].id())));

    let exhausted = State::new_intermediate(0, end, 2);
    let exact_end = State::new_exact(end);
    let hold = HoldCache::new(account_id);
    let held = node.get_cached_messages(account_id).await.unwrap();
    assert_eq!(held.last_change_id, warm.last_change_id);
    for since in [&initial, &exact_end, &exhausted] {
        let unavailable = trc::EventType::Store(trc::StoreEvent::UnexpectedError);
        let error = node_changes(&node, &token, since.clone())
            .await
            .err()
            .expect("changes served a stale cache");
        assert_eq!(error.event_type(), unavailable, "standalone changes floor");
        let error = node_query_changes(&node, &token, since, Value::Null)
            .await
            .expect_err("queryChanges served a stale cache");
        assert_eq!(error.event_type(), unavailable, "queryChanges floor");
    }
    for since in [State::Initial, State::new_exact(0)] {
        let initial = node_changes(&node, &token, since).await.unwrap();
        assert_eq!(initial.created, [live[0]], "fresh state carried a floor");
        assert_eq!(
            initial.new_state,
            State::new_intermediate(0, warm.last_change_id, 1)
        );
    }
    assert!(Arc::ptr_eq(
        &node.get_cached_messages(account_id).await.unwrap(),
        &held
    ));

    // The selected Content mode uses its named-property unavailable adapter.
    let deadline = search_config_guard!(test, wait_secs: 0);
    let error = node_query_changes(&node, &token, &initial, json!({"subject": "paging"}))
        .await
        .unwrap_err();
    assert_eq!(
        error.event_type(),
        trc::EventType::Jmap(trc::JmapEvent::UnsupportedFilter)
    );
    assert_eq!(
        error.value_as_str(trc::Key::Details),
        Some(format!("subject: {ZA_MAIL_SEARCH_UNAVAILABLE}").as_str())
    );
    drop(deadline);
    drop(hold);

    let next = node_changes(&node, &token, initial.clone()).await.unwrap();
    assert_eq!(
        node.get_cached_messages(account_id)
            .await
            .unwrap()
            .last_change_id,
        end
    );
    assert_eq!(next.old_state, initial);
    assert_eq!(next.new_state, exact_end);
    assert_eq!(next.created, [live[2]]);
    assert!(!next.has_more_changes && next.updated.is_empty() && next.destroyed.is_empty());
    let empty = node_changes(&node, &token, exact_end.clone())
        .await
        .unwrap();
    assert_eq!(empty.new_state, exact_end);
    assert!(empty.created.is_empty() && empty.updated.is_empty() && empty.destroyed.is_empty());

    let newer = import(
        &key,
        message_with("new after exhausted token", "new"),
        "2025-01-01T00:00:00Z",
    )
    .await;
    let (current, _) = query(&key, false).await;
    assert!(exact(&current) > end);
    let delta = node_query_changes(&node, &token, &exhausted, Value::Null)
        .await
        .unwrap();
    assert_eq!(state(&delta, "oldQueryState"), exhausted);
    assert_eq!(state(&delta, "newQueryState"), current);
    assert_eq!(delta["total"], 3);
    assert_eq!(added(&delta), [(newer, 2)]);
    assert!(ids(&delta, "removed").is_empty());
    test.destroy_all_mailboxes(&key).await;
}
