/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The periodic scheduler releases an idle loaded index without another request.

use super::{deliver, key_mail_account, message_with};
use crate::utils::{
    server::TestServer,
    za::{za_index_current, za_unlock},
};
use std::{sync::Arc, time::Duration};

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access periodic search cache eviction test...");
    let account =
        key_mail_account(test, "keymail-cache-idle@example.com", "Keymail-idle-84!").await;
    let account_id = account.id().document_id();
    deliver(
        test,
        "sender@example.org",
        account.name(),
        &message_with(
            "Periodic idle eviction",
            "Loaded content must leave memory when idle.",
        ),
    )
    .await;
    za_index_current(&account).await;
    let keys = za_unlock(test, account_id, account.secret()).await;
    let entry = test
        .server
        .za_search_entry(account_id, &keys)
        .await
        .unwrap();
    test.server
        .za_search_load_content(&entry, &keys)
        .await
        .unwrap();
    let loaded = entry.read_loaded().await.unwrap();
    assert!(loaded.thread_bytes > 0);
    assert!(
        loaded
            .content
            .as_ref()
            .is_some_and(|content| content.bytes > 0)
    );
    let entry_lifetime = Arc::downgrade(&entry);
    let index_lifetime = Arc::downgrade(&entry.index);
    drop(loaded);
    drop(entry);
    drop(keys);

    let cache = &test.server.inner.cache.za_search;
    cache.expire_idle_for_test(account_id);
    // There are no keyed requests or calls to entry()/sweep_idle() from here
    // until the scheduler has removed the idle entry. get() does not touch it.
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let retired = cache.get(account_id).is_none()
                && entry_lifetime.upgrade().is_none()
                && index_lifetime.upgrade().is_none();
            if retired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the 30-second periodic sweep did not evict the idle search index");
    assert!(entry_lifetime.upgrade().is_none());
    assert!(index_lifetime.upgrade().is_none());

    test.destroy_all_mailboxes(&account).await;
    test.wait_for_tasks().await;
}
