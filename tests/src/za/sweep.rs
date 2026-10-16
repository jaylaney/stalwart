/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.3 step 4 and 11: the maintenance task's lease sweep
//! removes an expired UID creation lease and keeps a live one, with the
//! default configuration (the in-memory store is the data store).

use crate::utils::{server::TestServer, za::za_uid_leases};
use ::registry::schema::{
    enums::TaskStoreMaintenanceType,
    structs::{Task, TaskStatus, TaskStoreMaintenance},
};
use common::za::{lease::ZaUidLease, test_hooks};
use std::time::Duration;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access lease sweep test...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").id().document_id();
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "a lease outlived an earlier module"
    );

    // An expired lease (held, never released: a crashed holder) and a live one.
    test_hooks::set_lease_secs_override(Some(1));
    let expired = ZaUidLease::acquire(test.server.store(), key1, "za-sweep-expired")
        .await
        .unwrap()
        .expect("a free lease");
    test_hooks::set_lease_secs_override(None);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let live = ZaUidLease::acquire(test.server.store(), key1, "za-sweep-live")
        .await
        .unwrap()
        .expect("a free lease");
    assert_eq!(za_uid_leases(test).await, 2);

    // The purge, as the scheduler runs it (from code reading: the
    // `PurgeData` arm of `store_maintenance` runs `purge_store`,
    // `purge_in_memory_store` and then the sweep).
    admin
        .registry_create_object(Task::StoreMaintenance(TaskStoreMaintenance {
            maintenance_type: TaskStoreMaintenanceType::PurgeData,
            shard_index: None,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    assert_eq!(
        za_uid_leases(test).await,
        1,
        "the sweep did not remove exactly the expired lease"
    );
    assert!(
        live.release(test.server.store()).await,
        "the live lease releases"
    );
    assert!(
        expired.release(test.server.store()).await,
        "an absent key counts as released"
    );
    assert_eq!(za_uid_leases(test).await, 0);
}
