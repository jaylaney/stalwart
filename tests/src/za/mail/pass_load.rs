/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Warm pass loading must release the resident guard during manifest I/O,
//! then validate the captured manifest against the resident head acquired later.

use super::key_mail_account;
use crate::utils::{
    server::TestServer,
    za::{HookParked, PauseSlotGuard, manifest_bytes, write_manifest_bytes, za_unlock},
};
use common::za::{
    search::{
        error::{LoadFailure, TransientClass},
        load::ObservedLoadError,
        seal::za_search_key,
    },
    test_hooks::{
        Pause, SEARCH_MANIFEST_READ_PAUSE, SearchPassCounter, SearchPassWork,
        set_search_manifest_fail_once, set_search_manifest_read_pause,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use store::za_search::manifest::ManifestValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadCase {
    Unchanged,
    Moved,
    MovedThenReadFailure,
}

pub async fn test(test: &mut TestServer) {
    println!("PR84 warm manifest read releases and rechecks the resident guard");
    let key = key_mail_account(
        test,
        "keymail-pr84-pass-load@example.com",
        "Pr84-Pass-Load!",
    )
    .await;
    let id = key.id().document_id();
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();

    for case in [
        ReadCase::Unchanged,
        ReadCase::Moved,
        ReadCase::MovedThenReadFailure,
    ] {
        let basis = entry.read_loaded().await.unwrap().manifest.clone();
        let before = manifest_bytes(test, id).await.unwrap();
        let _slot = PauseSlotGuard::new(&SEARCH_MANIFEST_READ_PAUSE);
        let counter = SearchPassCounter::start(id);
        let server = test.server.clone();
        let task_entry = entry.clone();
        let task_keys = keys.clone();
        let parked = HookParked::start(id, set_search_manifest_read_pause, async move {
            let _pass = task_entry.pass_lock.lock().await;
            if case == ReadCase::MovedThenReadFailure {
                // A prior failed read established no authenticated basis. A
                // stale warm snapshot must not settle this unknown episode.
                task_entry.note_pass_failure(None, TransientClass::BlobStore, Instant::now());
            }
            let mut observed = None;
            let result = server
                .za_search_load_for_pass(&task_entry, &task_keys, &mut observed)
                .await;
            (result, observed)
        })
        .await;
        let retry = entry.retry_schedule();
        assert_eq!(counter.counts()[SearchPassWork::Manifest as usize], 1);

        // The pause acknowledges that the raw manifest bytes are already
        // captured. Holding the warm read guard across that pending I/O must
        // fail here, before the pause is released.
        let writer = tokio::time::timeout(Duration::from_secs(2), entry.index.write())
            .await
            .expect("warm manifest I/O must not hold the index read guard");
        assert!(writer.is_some());
        drop(writer);

        let mut actual = basis;
        let actual_bytes = if case == ReadCase::Unchanged {
            before
        } else {
            actual.revision += 1;
            let bytes = ManifestValue::Index(actual.clone()).encode(&za_search_key(keys.swk()));
            write_manifest_bytes(test, id, Some(bytes.clone())).await;
            // Publish through the real serialized reload, not by assigning a
            // test-built resident value. The parked read still contains the
            // previous authenticated manifest.
            assert!(
                tokio::time::timeout(
                    Duration::from_secs(2),
                    test.server.za_search_reload_if_moved(&entry, &keys),
                )
                .await
                .expect("a service reload must publish during warm manifest I/O")
                .unwrap()
            );
            bytes
        };

        if case == ReadCase::MovedThenReadFailure {
            set_search_manifest_fail_once(Some(id));
        }
        let (result, observed) = parked.finish().await;
        set_search_manifest_fail_once(None);
        if case == ReadCase::MovedThenReadFailure {
            assert_eq!(
                result,
                Err(ObservedLoadError::Other(LoadFailure::Transient(
                    TransientClass::Store
                )))
            );
            assert_eq!(
                observed, None,
                "a superseded warm read is not an actual observed target"
            );
            assert_eq!(
                entry.retry_schedule(),
                retry,
                "a superseded warm read must not settle the retry episode"
            );
            let _pass = entry.pass_lock.lock().await;
            entry.clear_open_retry();
        } else {
            let (bytes, manifest) = result.unwrap();
            assert_eq!(bytes, actual_bytes);
            assert_eq!(manifest, actual);
            assert_eq!(observed, Some(actual.clone()));
        }
        assert_eq!(entry.read_loaded().await.unwrap().manifest, actual);
        assert_eq!(manifest_bytes(test, id).await.unwrap(), actual_bytes);
        assert_eq!(
            counter.counts(),
            [if case == ReadCase::Unchanged { 1 } else { 3 }, 0, 0, 0, 0],
            "a moved resident head needs a new serialized manifest read"
        );
    }

    // Cancellation at the same acknowledged I/O point is neutral: no target
    // was validated and no backend failure, retry mutation or write occurred.
    let before = manifest_bytes(test, id).await.unwrap();
    let _slot = PauseSlotGuard::new(&SEARCH_MANIFEST_READ_PAUSE);
    let pause = Arc::new(Pause::new(id));
    set_search_manifest_read_pause(Some(pause.clone()));
    let counter = SearchPassCounter::start(id);
    let server = test.server.clone();
    let task_entry = entry.clone();
    let task_keys = keys.clone();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        let _pass = task_entry.pass_lock.lock().await;
        task_entry.note_pass_failure(None, TransientClass::BlobStore, Instant::now());
        server
            .za_search_load_for_pass(&task_entry, &task_keys, &mut None)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
        .await
        .expect("the cancelled load must reach manifest I/O");
    let retry = entry.retry_schedule();
    tasks.abort_all();
    assert!(tasks.join_next().await.unwrap().unwrap_err().is_cancelled());
    pause.release.notify_one();
    assert_eq!(entry.retry_schedule(), retry);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), before);
    assert_eq!(counter.counts(), [1, 0, 0, 0, 0]);
    let writer = tokio::time::timeout(Duration::from_secs(2), entry.index.write())
        .await
        .expect("cancelling a warm read must release the index guard");
    drop(writer);
    let pass = entry.pass_lock.try_lock().expect("cancelled pass released");
    entry.clear_open_retry();
    drop(pass);
    drop(counter);
    test.destroy_all_mailboxes(&key).await;
}
