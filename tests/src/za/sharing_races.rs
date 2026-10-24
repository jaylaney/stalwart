/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Staged sharing transitions exercise the real event commit boundaries.
use super::{STRONG, uid_lease::put, user_permissions};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{HookParked, za_unlock},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::za::test_hooks;
use groupware::{
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent,
        seal::{
            calendar_view_in_domain, rewrap_event, seal_event_in_domain, unseal_event_in_domain,
        },
        sharing::{
            format::{
                GrantRingHeader, OwnerRingHeader, ZaGrant, ZaOwnerRing, seal_grant_ring,
                seal_owner_ring,
            },
            resolve::{
                ZaCalendarError, calendars_changed, za_calendar_context, za_owned_calendar_context,
            },
            test::prepare_shared_calendar,
        },
    },
};
use hyper::StatusCode;
use std::collections::BTreeMap;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use types::collection::{Collection, SyncCollection};

const OWNER: &str = "race-owner@example.com";
const CAL: &str = "/dav/cal/race-owner%40example.com/race/";
const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>Race</D:displayname></D:prop></D:set></C:mkcalendar>";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//race//EN\r\nBEGIN:VEVENT\r\nUID:race-create\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:race-private\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT10M\r\nATTENDEE:mailto:race-owner@example.com\r\nSUMMARY:race-alarm\r\nDESCRIPTION:race-alarm-private\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

async fn event_archive(test: &TestServer, owner: u32, id: u32) -> Archive<AlignedBytes> {
    test.server
        .store()
        .get_value(ValueKey::archive(owner, Collection::CalendarEvent, id))
        .await
        .unwrap()
        .unwrap()
}

async fn stage(test: &TestServer, owner: u32, calendar_id: u32, generation: u32) {
    stage_grant(test, owner, calendar_id, generation, None).await;
}

async fn stage_grant(
    test: &TestServer,
    owner: u32,
    calendar_id: u32,
    generation: u32,
    reader: Option<u32>,
) {
    let keys = za_unlock(test, owner, STRONG).await;
    let old = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();
    let mut calendar = calendar_view_in_domain(&old.stored, &old.domain, owner)
        .unwrap()
        .deserialize::<Calendar>()
        .unwrap();
    calendar
        .preferences
        .retain(|entry| entry.account_id == owner);
    calendar.acls.clear();
    let mut grants = Vec::new();
    let mut grant_rings = BTreeMap::new();
    if let Some(reader) = reader {
        let reader_keys = za_unlock(test, reader, STRONG).await;
        let public =
            vault::keys::derive_public_key(test.server.za_private_key(&reader_keys).await.unwrap());
        let mut preference = calendar.preferences[0].clone();
        preference.account_id = reader;
        calendar.preferences.push(preference);
        calendar.acls.push(types::acl::AclGrant {
            account_id: reader,
            grants: utils::map::bitmap::Bitmap::new()
                .with_item(types::acl::Acl::Read)
                .with_item(types::acl::Acl::ReadItems)
                .with_item(types::acl::Acl::Modify)
                .with_item(types::acl::Acl::AddItems)
                .with_item(types::acl::Acl::ModifyItems)
                .with_item(types::acl::Acl::RemoveItems),
        });
        grants.push(ZaGrant {
            account_id: reader,
            address: "race-reader@example.com".into(),
            public_key: public,
            level: 2,
            granted_at: 1,
            change_id: 1,
        });
        grant_rings.insert(
            reader,
            seal_grant_ring(
                &GrantRingHeader {
                    calendar_id,
                    domain_id: [0x31; 16],
                    grantee_id: reader,
                    grant_change_id: 1,
                    rename_change_id: 0,
                    boundary_pending: false,
                    generations: (1..=generation).collect(),
                },
                &(1..=generation)
                    .map(|g| (g, vault::keys::Secret::from_bytes([g as u8; 32])))
                    .collect(),
                owner,
                &public,
            )
            .unwrap(),
        );
    }
    let header = OwnerRingHeader {
        calendar_id,
        domain_id: [0x31; 16],
        rename_change_id: 0,
        boundary_pending: false,
        generations: (1..=generation).collect(),
    };
    let ring = ZaOwnerRing {
        version: 1,
        keys: (1..=generation).map(|g| [g as u8; 32]).collect(),
        converted: false,
        reverting: false,
        grants,
    };
    let encoded = seal_owner_ring(&header, &ring, owner, keys.ewk()).unwrap();
    let calendar =
        prepare_shared_calendar(&keys, calendar_id, calendar, &encoded, &grant_rings).unwrap();
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            old.stored.to_unarchived::<Calendar>().unwrap(),
            owner,
            calendar_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
}

async fn save_event(
    test: &TestServer,
    owner: u32,
    id: u32,
    stored: &Archive<AlignedBytes>,
    event: CalendarEvent,
) {
    let mut batch = BatchBuilder::new();
    event
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            owner,
            id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
}

fn put_response(
    path: String,
    body: String,
) -> impl std::future::Future<Output = crate::utils::http::HttpResponseFull> + Send + 'static {
    let request = crate::utils::http::HttpRequest::with_credentials(8899, OWNER, STRONG);
    async move {
        request
            .send_full(
                hyper::Method::PUT,
                &path,
                Some(body.into_bytes()),
                Some("text/calendar"),
            )
            .await
    }
}

fn delete(path: String) -> impl std::future::Future<Output = u16> + Send + 'static {
    let request = crate::utils::http::HttpRequest::with_credentials(8899, OWNER, STRONG);
    async move {
        request
            .send_full(hyper::Method::DELETE, &path, None, None)
            .await
            .status
            .as_u16()
    }
}

async fn alarm_task(
    test: &TestServer,
    owner: u32,
    id: u32,
) -> Option<registry::schema::structs::Task> {
    test.server
        .store()
        .get_value(ValueKey::from(store::write::ValueClass::TaskQueue(
            store::write::TaskQueueClass::Task {
                id: types::id::Id::from_parts(owner, id).id(),
            },
        )))
        .await
        .unwrap()
}

fn generation(event: &CalendarEvent) -> u32 {
    let text = event.data.event.to_string().replace("\r\n ", "");
    let encoded = text
        .lines()
        .find_map(|line| line.strip_prefix("X-ZA-KEY:"))
        .unwrap();
    let envelope = STANDARD.decode(encoded).unwrap();
    assert_eq!(
        envelope[1], 3,
        "event committed under the pre-conversion EWK"
    );
    u32::from_be_bytes(envelope[6..10].try_into().unwrap())
}

pub async fn test(test: &mut TestServer) {
    grantee_http_write_races(test).await;
    sharing_http_races(test).await;
    sharing_destruction_contracts(test).await;
    task5_boundary_and_copy_races(test).await;
    println!("Running calendar write-fence races...");
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(OWNER, STRONG, "Race owner", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let client = DummyWebDavClient::new(owner_id, OWNER, STRONG, OWNER);
    client
        .request("MKCALENDAR", CAL, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let calendar_id = resources.by_path("race").unwrap().document_id();
    let parked = HookParked::start(
        owner_id,
        test_hooks::set_commit_pause,
        put_response(format!("{CAL}race-create.ics"), EVENT.into()),
    )
    .await;
    let held_at = store::write::now() as i64;
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    stage(test, owner_id, calendar_id, 1).await;
    let response = parked.finish().await;
    assert_eq!(response.status.as_u16(), 201);
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let id = resources
        .by_path("race/race-create.ics")
        .unwrap()
        .document_id();
    let committed = event_archive(test, owner_id, id).await;
    let metadata = committed.deserialize::<CalendarEvent>().unwrap();
    assert!(
        metadata.created <= held_at && metadata.modified <= held_at,
        "retry advanced the original creation/modified timestamp"
    );
    assert_eq!(metadata.created, metadata.modified);
    let alarm = committed
        .unarchive::<CalendarEvent>()
        .unwrap()
        .data
        .next_alarm(
            store::write::now() as i64,
            calcard::common::timezone::Tz::Floating,
        )
        .unwrap();
    match alarm_task(test, owner_id, id).await.unwrap() {
        registry::schema::structs::Task::CalendarAlarmEmail(task) => {
            assert_eq!(task.account_id.document_id(), owner_id);
            assert_eq!(task.document_id.document_id(), id);
            match task.status {
                registry::schema::structs::TaskStatus::Pending(status) => {
                    assert_eq!(status.due.timestamp(), alarm.alarm_time)
                }
                _ => panic!("future alarm task lost its pending state"),
            }
        }
        _ => panic!("retry did not retain its email alarm task"),
    }
    assert_eq!(
        generation(&committed.deserialize::<CalendarEvent>().unwrap()),
        1
    );
    assert_eq!(
        response.header("etag"),
        Some(format!("\"{}\"", committed.version.hash().unwrap()).as_str()),
        "create returned the discarded first-attempt ETag"
    );
    // Case 21's update variant: an already opened EWK event crosses conversion.
    client
        .request("MKCALENDAR", &CAL.replace("/race/", "/race-update/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let update_cal = format!("{}", CAL.replace("/race/", "/race-update/"));
    let update_path = format!("{update_cal}race-update.ics");
    let update_body = EVENT.replace("race-create", "race-update");
    assert_eq!(
        put(OWNER, STRONG, update_path.clone(), update_body.clone()).await,
        201
    );
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let update_calendar = resources.by_path("race-update").unwrap().document_id();
    let update_id = resources
        .by_path("race-update/race-update.ics")
        .unwrap()
        .document_id();
    let parked = HookParked::start(
        owner_id,
        test_hooks::set_update_commit_pause,
        put(
            OWNER,
            STRONG,
            update_path.clone(),
            update_body.replace("race-private", "race-updated"),
        ),
    )
    .await;
    stage(test, owner_id, update_calendar, 1).await;
    assert_eq!(parked.finish().await, 204);
    assert_eq!(
        generation(
            &event_archive(test, owner_id, update_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        1
    );

    // Case 22's independent owner-update hold crosses rotation.
    let parked = HookParked::start(
        owner_id,
        test_hooks::set_update_commit_pause,
        put(
            OWNER,
            STRONG,
            update_path.clone(),
            update_body.replace("race-private", "race-rotated"),
        ),
    )
    .await;
    stage(test, owner_id, update_calendar, 2).await;
    assert_eq!(parked.finish().await, 204);
    assert_eq!(
        generation(
            &event_archive(test, owner_id, update_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        2
    );

    calendar_retry_keeps_event_and_uid_assertions(test, owner_id, calendar_id, id, &client).await;
    second_calendar_loss(test, owner_id, calendar_id, id).await;
    second_create_loss(test, owner_id, calendar_id).await;
    let linked_grantee_status = grantee_commit_seams(test, owner_id, calendar_id, id).await;
    let alarm_cleanup = delete_retry_keeps_due_alarm(test, owner_id, calendar_id).await;
    assert!(
        linked_grantee_status == 409 && alarm_cleanup,
        "review regressions: C1 linked grantee status={linked_grantee_status} (want409); A1 original alarm cleanup={alarm_cleanup} (wanttrue)"
    );
    fanout_is_not_repeated(test, owner_id, calendar_id).await;
    drain_and_contacts_use_current_domain(test, owner_id, calendar_id, &client).await;
    non_put_writers_use_current_domain(
        test,
        owner_id,
        update_calendar,
        update_id,
        &client,
        &update_path,
    )
    .await;
    client
        .request_with_headers("DELETE", CAL, [super::fanout::NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request_with_headers("DELETE", &update_cal, [super::fanout::NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    for name in [
        OWNER,
        "race-reader@example.com",
        "race-fanout@example.com",
        "race-drain@example.com",
    ] {
        let account = test.account(name).id().document_id();
        super::fanout::Party::new(test, name)
            .delete_items(crate::utils::za::inbox_ids(test, account).await)
            .await;
        crate::utils::za::za_unpin(test, account, STRONG).await;
    }
    assert!(
        alarm_task(test, owner_id, id).await.is_none(),
        "fixture cleanup retained an alarm task"
    );
}

async fn calendar_retry_keeps_event_and_uid_assertions(
    test: &TestServer,
    owner: u32,
    calendar: u32,
    id: u32,
    _client: &DummyWebDavClient,
) {
    let parked = HookParked::start(
        owner,
        test_hooks::set_update_commit_pause,
        put(
            OWNER,
            STRONG,
            format!("{CAL}race-create.ics"),
            EVENT.replace("race-private", "race-loser"),
        ),
    )
    .await;
    let stored = event_archive(test, owner, id).await;
    let mut concurrent = stored.deserialize::<CalendarEvent>().unwrap();
    concurrent.flags ^= 1;
    save_event(test, owner, id, &stored, concurrent.clone()).await;
    stage(test, owner, calendar, 2).await;
    assert_eq!(
        parked.finish().await,
        409,
        "calendar retry overwrote a concurrent event"
    );
    assert_eq!(
        event_archive(test, owner, id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap()
            .flags,
        concurrent.flags
    );

    test_hooks::set_lease_secs_override(Some(1));
    let parked = HookParked::start(
        owner,
        test_hooks::set_commit_pause,
        put(
            OWNER,
            STRONG,
            format!("{CAL}race-uid.ics"),
            EVENT.replace("race-create", "race-uid"),
        ),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let replacement =
        common::za::lease::ZaUidLease::acquire(test.server.store(), owner, "race-uid")
            .await
            .unwrap()
            .unwrap();
    assert!(replacement.release(test.server.store()).await);
    test_hooks::set_lease_secs_override(None);
    stage(test, owner, calendar, 3).await;
    assert_eq!(
        parked.finish().await,
        503,
        "calendar retry lost the original UID fence"
    );
}

async fn non_put_writers_use_current_domain(
    test: &TestServer,
    owner: u32,
    calendar: u32,
    id: u32,
    client: &DummyWebDavClient,
    path: &str,
) {
    let properties = "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:X=\"urn:race\"><D:set><D:prop><X:private>race-property</X:private></D:prop></D:set></D:propertyupdate>";
    client
        .request("PROPPATCH", path, properties)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let stored = event_archive(test, owner, id).await;
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    assert_eq!(generation(&event), 2);
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, calendar)
        .await
        .unwrap();
    unseal_event_in_domain(&mut event, &context.domain).unwrap();
    assert!(!event.dead_properties.0.is_empty());

    // Linked-write refusal holds through both paths, while unlink-only recovery
    // remains possible without opening or resealing the source.
    let other = format!("{}", CAL.replace("/race/", "/race-other/"));
    client
        .request("MKCALENDAR", &other, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let other_id = resources.by_path("race-other").unwrap().document_id();
    let mut linked = stored.deserialize::<CalendarEvent>().unwrap();
    let other_context = za_owned_calendar_context(&test.server, &keys, other_id)
        .await
        .unwrap();
    rewrap_event(&mut linked, &context.domain, &other_context.domain).unwrap();
    linked.names.push(common::DavName {
        name: "linked.ics".into(),
        parent_id: other_id,
    });
    save_event(test, owner, id, &stored, linked).await;
    for linked_path in [path.to_string(), format!("{other}linked.ics")] {
        client
            .request("GET", &linked_path, "")
            .await
            .with_status(StatusCode::OK);
        assert_eq!(
            put(
                OWNER,
                STRONG,
                linked_path.clone(),
                EVENT.replace("race-create", "race-update")
            )
            .await,
            409
        );
        client
            .request("PROPPATCH", &linked_path, properties)
            .await
            .with_status(StatusCode::CONFLICT);
    }
    client
        .request("DELETE", &format!("{other}linked.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        event_archive(test, owner, id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap()
            .names
            .len()
            == 1
    );
    client
        .request("DELETE", &other, "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    let parked = HookParked::start(
        owner,
        test_hooks::set_delete_commit_pause,
        delete(path.to_string()),
    )
    .await;
    stage(test, owner, calendar, 3).await;
    assert_eq!(
        parked.finish().await,
        204,
        "authorized DELETE did not retry calendar change"
    );
    assert!(
        test.server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                owner,
                Collection::CalendarEvent,
                id
            ))
            .await
            .unwrap()
            .is_none()
    );
}

async fn second_calendar_loss(test: &TestServer, owner: u32, calendar: u32, id: u32) {
    println!("second_calendar_loss...");
    use std::{sync::Arc, time::Duration};
    let original = event_archive(test, owner, id).await;
    let first = Arc::new(test_hooks::Pause::new(owner));
    test_hooks::set_update_commit_pause(Some(first.clone()));
    let request = tokio::spawn(put(
        OWNER,
        STRONG,
        format!("{CAL}race-create.ics"),
        EVENT.replace("race-private", "race-second-loss"),
    ));
    tokio::time::timeout(Duration::from_secs(5), first.arrived.notified())
        .await
        .unwrap();
    stage(test, owner, calendar, 4).await;
    let second = Arc::new(test_hooks::Pause::new(owner));
    test_hooks::set_update_commit_pause(Some(second.clone()));
    first.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), second.arrived.notified())
        .await
        .unwrap();
    stage(test, owner, calendar, 5).await;
    let reads = groupware::calendar::sharing::test::calendar_reads(owner, calendar);
    second.release.notify_one();
    assert_eq!(
        request.await.unwrap(),
        409,
        "second calendar assertion loss must terminate"
    );
    assert_eq!(
        groupware::calendar::sharing::test::calendar_reads(owner, calendar),
        reads,
        "second loss reread calendar state"
    );
    assert_eq!(
        event_archive(test, owner, id).await.as_bytes(),
        original.as_bytes(),
        "terminal update loss mutated the source"
    );
    test_hooks::set_update_commit_pause(None);
}

async fn second_create_loss(test: &TestServer, owner: u32, calendar: u32) {
    use std::{sync::Arc, time::Duration};
    println!("second_create_loss...");
    for successor in [false, true] {
        let uid = if successor {
            "race-second-create-successor"
        } else {
            "race-second-create-owned"
        };
        if successor {
            test_hooks::set_lease_secs_override(Some(1));
        }
        let first = Arc::new(test_hooks::Pause::new(owner));
        test_hooks::set_commit_pause(Some(first.clone()));
        let request = tokio::spawn(put(
            OWNER,
            STRONG,
            format!("{CAL}{uid}.ics"),
            EVENT.replace("race-create", uid),
        ));
        tokio::time::timeout(Duration::from_secs(5), first.arrived.notified())
            .await
            .unwrap();
        stage(test, owner, calendar, 6).await;
        let second = Arc::new(test_hooks::Pause::new(owner));
        test_hooks::set_commit_pause(Some(second.clone()));
        first.release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), second.arrived.notified())
            .await
            .unwrap();
        let replacement = if successor {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            // Keep the successor live throughout the bounded UID retake.
            test_hooks::set_lease_secs_override(None);
            Some(
                common::za::lease::ZaUidLease::acquire(test.server.store(), owner, uid)
                    .await
                    .unwrap()
                    .unwrap(),
            )
        } else {
            None
        };
        stage(test, owner, calendar, 7).await;
        let reads = groupware::calendar::sharing::test::calendar_reads(owner, calendar);
        second.release.notify_one();
        assert_eq!(request.await.unwrap(), 503);
        assert_eq!(
            groupware::calendar::sharing::test::calendar_reads(owner, calendar),
            reads,
            "second create loss read calendars a third time"
        );
        assert!(
            test.server
                .document_ids_matching(
                    owner,
                    Collection::CalendarEvent,
                    types::field::CalendarEventField::Uid,
                    uid.as_bytes()
                )
                .await
                .unwrap()
                .is_empty(),
            "terminal create loss stored an event"
        );
        if let Some(replacement) = replacement {
            let mut fenced = BatchBuilder::new();
            replacement.assert_in(&mut fenced);
            test.server
                .store()
                .write(fenced.build_all())
                .await
                .expect("opaque cleanup removed or changed its successor fence");
            assert!(
                replacement.release(test.server.store()).await,
                "opaque cleanup cleared its successor"
            );
        } else {
            let next = common::za::lease::ZaUidLease::acquire(test.server.store(), owner, uid)
                .await
                .unwrap();
            assert!(
                next.is_some(),
                "opaque second calendar loss abandoned an owned UID lease"
            );
            assert!(next.unwrap().release(test.server.store()).await);
        }
        test_hooks::set_commit_pause(None);
        test_hooks::set_lease_secs_override(None);
    }
}

// Until foreign DAV routing opens in Task 10, a real grantee token and the
// same resolver/assertion/rewrap writer seams provide the race oracle.
async fn grantee_commit_seams(test: &mut TestServer, owner: u32, calendar: u32, id: u32) -> u16 {
    println!("grantee_commit_seams...");
    use types::acl::Acl;
    let reader = test
        .account("admin@example.com")
        .create_key_user_account(
            "race-reader@example.com",
            STRONG,
            "Race reader",
            &[],
            user_permissions(),
        )
        .await;
    let reader_id = reader.id().document_id();
    test.insert_account(reader);
    stage_grant(test, owner, calendar, 5, Some(reader_id)).await;
    let keys = za_unlock(test, reader_id, STRONG).await;
    let token = common::auth::AccessToken::new_maybe_invalid(
        test.server.access_token(reader_id).await.unwrap(),
    )
    .with_session_keys(keys);
    let required = utils::map::bitmap::Bitmap::new().with_item(Acl::ModifyItems);
    let before = za_calendar_context(&test.server, &token, owner, calendar, required)
        .await
        .unwrap()
        .unwrap();
    let stored = event_archive(test, owner, id).await;
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    let mut pairs = unseal_event_in_domain(&mut event, &before.domain)
        .unwrap()
        .unwrap_or_default();
    pairs.insert("race-reader@example.com", [0x27; 16]);
    seal_event_in_domain(&mut event, &before.domain, Some(&pairs)).unwrap();
    let sealed_before = event.clone();
    let alarm_before = alarm_task(test, owner, id).await.unwrap();
    let timestamp = 123456789;
    let mut batch = BatchBuilder::new();
    before.assert_in(&mut batch);
    event
        .clone()
        .update_at(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            owner,
            id,
            timestamp,
            &mut batch,
        )
        .unwrap();
    stage_grant(test, owner, calendar, 6, Some(reader_id)).await;
    assert!(
        test.server
            .commit_batch(batch)
            .await
            .unwrap_err()
            .is_assertion_failure()
    );
    let after = za_calendar_context(&test.server, &token, owner, calendar, required)
        .await
        .unwrap()
        .unwrap();
    assert!(calendars_changed(
        std::slice::from_ref(&before),
        std::slice::from_ref(&after)
    ));
    rewrap_event(&mut event, &before.domain, &after.domain).unwrap();
    assert_eq!(generation(&event), 6);
    let mut payload_before = sealed_before.clone();
    let mut payload_after = event.clone();
    payload_before.data.event.components[0].entries.pop();
    payload_after.data.event.components[0].entries.pop();
    assert_eq!(
        payload_after, payload_before,
        "retry changed sealed payload, pairs, metadata or alarms"
    );
    let mut batch = BatchBuilder::new();
    after.assert_in(&mut batch);
    event
        .clone()
        .update_at(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            owner,
            id,
            timestamp,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    let committed = event_archive(test, owner, id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    assert_eq!(committed.modified, timestamp);
    assert_eq!(
        alarm_task(test, owner, id).await,
        Some(alarm_before),
        "retry changed alarm queue values"
    );
    assert_eq!(committed.created, event.created);
    assert_eq!(committed.data, event.data);
    assert!(
        !committed.data.alarms.is_empty(),
        "the preservation fixture needs an alarm"
    );
    assert_eq!(committed.data.alarms, sealed_before.data.alarms);
    let mut opened = committed.clone();
    let restored = unseal_event_in_domain(&mut opened, &after.domain)
        .unwrap()
        .unwrap();
    assert_eq!(restored.get("race-reader@example.com"), Some(&[0x27; 16]));
    let hits = test
        .server
        .document_ids_matching(
            owner,
            Collection::CalendarEvent,
            types::field::CalendarEventField::Uid,
            b"race-create",
        )
        .await
        .unwrap();
    assert!(hits.contains(id), "retry lost its UID index");

    // C1: requested shared path is authorized; the other parent is private.
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let private_parent = resources.by_path("default").unwrap().document_id();
    let mut linked = committed.clone();
    linked.names.push(common::DavName {
        name: "race-grantee-linked.ics".into(),
        parent_id: private_parent,
    });
    let linked_status = match groupware::calendar::sharing::resolve::za_event_contexts(
        &test.server,
        &token,
        owner,
        &linked,
        calendar,
        utils::map::bitmap::Bitmap::new().with_item(Acl::ModifyItems),
    )
    .await
    {
        Err(ZaCalendarError::LinkedEvent) => 409,
        Err(ZaCalendarError::Forbidden) => 403,
        _ => panic!("linked shared write was not refused"),
    };
    println!("C1 authorized shared-path linked status: {linked_status} (expected409)");

    // Revoked UPDATE and DELETE each lose their old calendar assertion and
    // fresh authorization refuses them; the original event stays byte-for-byte.
    for remove in [false, true] {
        stage_grant(test, owner, calendar, 6, Some(reader_id)).await;
        let required = utils::map::bitmap::Bitmap::new().with_item(if remove {
            Acl::RemoveItems
        } else {
            Acl::ModifyItems
        });
        let context = za_calendar_context(&test.server, &token, owner, calendar, required)
            .await
            .unwrap()
            .unwrap();
        let stored = event_archive(test, owner, id).await;
        let mut batch = BatchBuilder::new();
        context.assert_in(&mut batch);
        if remove {
            groupware::DestroyArchive(stored.to_unarchived::<CalendarEvent>().unwrap())
                .delete_za(
                    &test.server.account_info(owner).await.unwrap(),
                    owner,
                    id,
                    calendar,
                    None,
                    false,
                    token.session_keys().map(|keys| keys.as_ref()),
                    Some(&context.domain),
                    &mut batch,
                )
                .unwrap();
        } else {
            stored
                .deserialize::<CalendarEvent>()
                .unwrap()
                .update_at(
                    test.server
                        .account_info(owner)
                        .await
                        .unwrap()
                        .account_tenant_ids(),
                    stored.to_unarchived::<CalendarEvent>().unwrap(),
                    owner,
                    id,
                    timestamp,
                    &mut batch,
                )
                .unwrap();
        }
        stage(test, owner, calendar, 7).await;
        assert!(
            test.server
                .commit_batch(batch)
                .await
                .unwrap_err()
                .is_assertion_failure()
        );
        let status =
            match za_calendar_context(&test.server, &token, owner, calendar, required).await {
                Err(ZaCalendarError::Forbidden) => 403,
                _ => panic!("revoked writer still authorized"),
            };
        assert_eq!(status, 403);
        assert!(
            matches!(
                groupware::calendar::sharing::resolve::za_event_contexts(
                    &test.server,
                    &token,
                    owner,
                    &linked,
                    calendar,
                    required
                )
                .await,
                Err(ZaCalendarError::Forbidden)
            ),
            "unauthorized requested path must remain403"
        );
        assert_eq!(
            event_archive(test, owner, id).await.as_bytes(),
            stored.as_bytes(),
            "refused write mutated or deleted the source"
        );
    }
    linked_status
}

async fn fanout_is_not_repeated(test: &mut TestServer, owner: u32, calendar: u32) {
    println!("fanout_is_not_repeated...");
    const RECIPIENT: &str = "race-fanout@example.com";
    let account = test
        .account("admin@example.com")
        .create_key_user_account(RECIPIENT, STRONG, "Race fanout", &[], user_permissions())
        .await;
    let recipient = account.id().document_id();
    test.insert_account(account);
    let before = crate::utils::za::inbox_ids(test, recipient).await;
    let body = super::fanout::invite("race-fanout", super::fanout::START_A, &[RECIPIENT])
        .replace("key1@example.com", OWNER);
    assert_eq!(
        put(OWNER, STRONG, format!("{CAL}race-fanout.ics"), body.clone()).await,
        201
    );
    let parked = HookParked::start(
        owner,
        test_hooks::set_update_commit_pause,
        put(
            OWNER,
            STRONG,
            format!("{CAL}race-fanout.ics"),
            body.replace("fo-title", "race-fanout-changed"),
        ),
    )
    .await;
    let items = crate::utils::za::inbox_ids(test, recipient).await;
    stage(test, owner, calendar, 8).await;
    assert_eq!(parked.finish().await, 204);
    assert_eq!(
        crate::utils::za::inbox_ids(test, recipient).await,
        items,
        "calendar retry repeated fan-out"
    );
    super::fanout::Party::new(test, RECIPIENT)
        .delete_items_since(test, &before)
        .await;
}

async fn drain_and_contacts_use_current_domain(
    test: &mut TestServer,
    owner: u32,
    calendar: u32,
    client: &DummyWebDavClient,
) {
    println!("drain_and_contacts_use_current_domain...");
    use crate::utils::za::{substitute_public_key, trigger_drain, writer_key};
    const RECEIVER: &str = "race-drain@example.com";
    const DEFAULT: &str = "/dav/cal/race-drain%40example.com/default/";
    let account = test
        .account("admin@example.com")
        .create_key_user_account(RECEIVER, STRONG, "Race drain", &[], user_permissions())
        .await;
    let receiver = account.id().document_id();
    test.insert_account(account);
    let recipient = DummyWebDavClient::new(receiver, RECEIVER, STRONG, RECEIVER);
    let resources = test
        .server
        .fetch_dav_resources(receiver, receiver, SyncCollection::Calendar)
        .await
        .unwrap();
    let target = resources.by_path("default").unwrap().document_id();
    stage(test, receiver, target, 1).await;
    let _drain = super::drain::DrainOn::new();
    let path = format!("{CAL}race-drain.ics");
    let body =
        super::pinning::invite_from(OWNER, "race-drain", super::fanout::START_A, &[RECEIVER]);
    assert_eq!(put(OWNER, STRONG, path.clone(), body.clone()).await, 201);
    test_hooks::set_drain_retry_override(Some(0));
    let parked = HookParked::start(
        receiver,
        test_hooks::set_drain_lease_pause,
        trigger_drain(RECEIVER, STRONG),
    )
    .await;
    stage(test, receiver, target, 2).await;
    assert_eq!(parked.finish().await, 207);
    assert!(
        !crate::utils::za::pending_ids(test, receiver)
            .await
            .is_empty(),
        "drain calendar loss did not stay transient/pending"
    );
    assert!(
        test.server
            .document_ids_matching(
                receiver,
                Collection::CalendarEvent,
                types::field::CalendarEventField::Uid,
                b"race-drain"
            )
            .await
            .unwrap()
            .is_empty(),
        "drain retried inside the failed item boundary"
    );
    assert_eq!(trigger_drain(RECEIVER, STRONG).await, 207);
    let hits = test
        .server
        .document_ids_matching(
            receiver,
            Collection::CalendarEvent,
            types::field::CalendarEventField::Uid,
            b"race-drain",
        )
        .await
        .unwrap();
    let event_id = hits.iter().next().unwrap();
    assert_eq!(
        generation(
            &event_archive(test, receiver, event_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        2,
        "drain import used the account EWK"
    );
    stage(test, receiver, target, 3).await;
    assert_eq!(
        put(
            OWNER,
            STRONG,
            path.clone(),
            body.replace("fo-title", "race-merge")
        )
        .await,
        204
    );
    assert_eq!(trigger_drain(RECEIVER, STRONG).await, 207);
    assert_eq!(
        generation(
            &event_archive(test, receiver, event_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        3,
        "drain merge used an old key domain"
    );

    let source_resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let source_id = source_resources
        .by_path("race/race-drain.ics")
        .unwrap()
        .document_id();
    // First reseal path: redelivery of a withheld edit in a shared source.
    let original = substitute_public_key(test, receiver, &writer_key(), true).await;
    let changed = body.replace("fo-title", "race-refused-one");
    assert_eq!(put(OWNER, STRONG, path.clone(), changed).await, 204);
    substitute_public_key(test, receiver, &original, true).await;
    let fingerprint =
        vault::keys::display_fingerprint(&super::pinning::served(test, receiver).await);
    let reply = super::pinning::pin(OWNER, RECEIVER, &fingerprint)
        .await
        .expect(200);
    assert_eq!(reply["delivered"], 1);
    assert_eq!(reply["failed"], 0);
    assert_eq!(
        generation(
            &event_archive(test, owner, source_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        8
    );

    // Fresh-archive reseal: a concurrent non-scheduling edit keeps the refusal
    // id but invalidates the first event assertion while redelivery is parked.
    substitute_public_key(test, receiver, &writer_key(), true).await;
    let changed = body.replace("fo-title", "race-refused-two");
    assert_eq!(put(OWNER, STRONG, path.clone(), changed.clone()).await, 204);
    substitute_public_key(test, receiver, &original, true).await;
    let parked = HookParked::start(
        owner,
        test_hooks::set_retire_pause,
        super::pinning::pin_request(OWNER, RECEIVER, &fingerprint),
    )
    .await;
    let text = client
        .request("GET", &path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(
        put(
            OWNER,
            STRONG,
            path.clone(),
            text.replacen("END:VEVENT", "CATEGORIES:race-cas\r\nEND:VEVENT", 1)
        )
        .await,
        204
    );
    let reply = parked.finish().await.expect(200);
    assert_eq!(reply["failed"], 0);
    assert_eq!(reply["remaining"], 1);
    assert_eq!(
        generation(
            &event_archive(test, owner, source_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        8
    );
    assert!(
        client
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap()
            .contains("CATEGORIES:race-cas")
    );

    // Unsafe linked redelivery counts as failed, preserving the withheld id.
    let other = CAL.replace("/race/", "/race-contact-link/");
    client
        .request("MKCALENDAR", &other, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let other_id = resources
        .by_path("race-contact-link")
        .unwrap()
        .document_id();
    let stored = event_archive(test, owner, source_id).await;
    let mut linked = stored.deserialize::<CalendarEvent>().unwrap();
    linked.names.push(common::DavName {
        name: "linked.ics".into(),
        parent_id: other_id,
    });
    save_event(test, owner, source_id, &stored, linked).await;
    let reply = super::pinning::pin(OWNER, RECEIVER, &fingerprint)
        .await
        .expect(200);
    assert_eq!(reply["failed"], 1);
    client
        .request("DELETE", &format!("{other}linked.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", &other, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        super::pinning::pin(OWNER, RECEIVER, &fingerprint)
            .await
            .expect(200)["failed"],
        0
    );
    assert_eq!(trigger_drain(RECEIVER, STRONG).await, 207);
    recipient
        .request_with_headers("DELETE", DEFAULT, [super::fanout::NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    super::fanout::Party::new(test, RECEIVER)
        .delete_items(crate::utils::za::inbox_ids(test, receiver).await)
        .await;
    test_hooks::set_drain_retry_override(None);
    // The owner source remains for the enclosing calendar cleanup.
    let keys = za_unlock(test, owner, STRONG).await;
    assert!(
        za_owned_calendar_context(&test.server, &keys, calendar)
            .await
            .is_ok()
    );
}

async fn delete_retry_keeps_due_alarm(test: &TestServer, owner: u32, calendar: u32) -> bool {
    use crate::utils::za::hold_task_consumer;
    use common::auth::vault::RawValue;
    use store::write::{TaskQueueClass, ValueClass};
    println!("delete_retry_keeps_due_alarm...");
    let _consumer = hold_task_consumer(test).await;
    let due = store::write::now() as i64 + 3;
    let stamp = |time| {
        mail_parser::DateTime::from_timestamp(time)
            .to_rfc3339()
            .replace(['-', ':'], "")
    };
    let path = format!("{CAL}race-delete-due.ics");
    let body = EVENT
        .replace("race-create", "race-delete-due")
        .replace("20990102T090000Z", &stamp(due))
        .replace("20990102T100000Z", &stamp(due + 3600))
        .replace("TRIGGER:-PT10M", "TRIGGER:PT0S");
    assert_eq!(put(OWNER, STRONG, path.clone(), body).await, 201);
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let event_id = resources
        .by_path("race/race-delete-due.ics")
        .unwrap()
        .document_id();
    let task_id = types::id::Id::from_parts(owner, event_id).id();
    let due_key = || {
        ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Due {
            id: task_id,
            due: due as u64,
        }))
    };
    assert!(
        alarm_task(test, owner, event_id).await.is_some(),
        "initial Task missing"
    );
    assert!(
        test.server
            .store()
            .get_value::<RawValue>(due_key())
            .await
            .unwrap()
            .is_some(),
        "initial original Due(T) missing"
    );
    assert!(
        (store::write::now() as i64) < due,
        "fixture must start beforeT"
    );
    let parked = HookParked::start(owner, test_hooks::set_delete_commit_pause, delete(path)).await;
    assert!(
        (store::write::now() as i64) < due,
        "DELETE initial cutoff must precedeT"
    );
    stage(test, owner, calendar, 8).await;
    while (store::write::now() as i64) < due {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!((store::write::now() as i64) >= due);
    assert_eq!(parked.finish().await, 204);
    assert!(
        test.server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                owner,
                Collection::CalendarEvent,
                event_id
            ))
            .await
            .unwrap()
            .is_none()
    );
    let task_present = alarm_task(test, owner, event_id).await.is_some();
    let due_present = test
        .server
        .store()
        .get_value::<RawValue>(due_key())
        .await
        .unwrap()
        .is_some();
    println!(
        "A1 after retry while consumer held: originalTask={task_present}, originalDue(T)={due_present} (both expectedfalse)"
    );
    !task_present && !due_present
}

/// Task 5 uses dedicated calendars rather than grant endpoints, which arrive later.
async fn task5_boundary_and_copy_races(test: &mut TestServer) {
    use groupware::calendar::sharing::boundary::{
        ZaBoundaryCommit, mark_boundary_pending, za_complete_boundary, za_repair_owned_context,
    };
    use std::sync::Arc;
    const USER: &str = "share-boundary@example.com";
    const HOME: &str = "/dav/cal/share-boundary%40example.com/";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(USER, STRONG, "Boundary owner", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let client = DummyWebDavClient::new(owner_id, USER, STRONG, USER);
    let keys = za_unlock(test, owner_id, STRONG).await;
    for slug in ["a", "b", "shared"] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let a = resources.by_path("a").unwrap().document_id();
    let b = resources.by_path("b").unwrap().document_id();
    let shared = resources.by_path("shared").unwrap().document_id();
    super::sharing::stage_shared_calendar(test, owner_id, shared, None).await;
    async fn pending(test: &TestServer, keys: &vault::session::SessionKeys, id: u32) -> u64 {
        let context = za_owned_calendar_context(&test.server, keys, id)
            .await
            .unwrap();
        let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
        mark_boundary_pending(&mut calendar, &context.domain).unwrap();
        let mut batch = BatchBuilder::new();
        calendar
            .update(
                test.server
                    .account_info(keys.account_id)
                    .await
                    .unwrap()
                    .account_tenant_ids(),
                context.stored.to_unarchived::<Calendar>().unwrap(),
                keys.account_id,
                id,
                &mut batch,
            )
            .unwrap();
        test.server
            .commit_batch(batch)
            .await
            .unwrap()
            .last_change_id(keys.account_id)
            .unwrap()
    }
    // An interrupted second write stays pending until an explicit owner read adapter repairs it.
    let committed = pending(test, &keys, shared).await;
    assert!(
        za_owned_calendar_context(&test.server, &keys, shared)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .pending,
        "resolver must remain read-only"
    );
    let response = client
        .request_with_headers(
            "PROPFIND",
            &format!("{HOME}shared/"),
            [("depth", "0")],
            "<D:propfind xmlns:D=\"DAV:\"><D:prop><D:displayname/><D:getetag/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let repaired = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    let etag = format!("\"{}\"", repaired.stored.version.hash().unwrap());
    assert!(
        response
            .xml
            .iter()
            .any(|(path, value)| path.ends_with(":getetag") && value == &etag),
        "PROPFIND must emit repaired archive ETag: {:?}",
        response.xml
    );
    assert!(!repaired.boundary.unwrap().pending);
    assert!(repaired.boundary.unwrap().rename_change_id >= committed);
    let committed = pending(test, &keys, shared).await;
    let context = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    let repaired = za_repair_owned_context(&test.server, &keys, context)
        .await
        .unwrap();
    assert_eq!(
        repaired.stored.version,
        za_owned_calendar_context(&test.server, &keys, shared)
            .await
            .unwrap()
            .stored
            .version
    );
    assert!(repaired.boundary.unwrap().rename_change_id >= committed);
    // Losing own-commit assertion must reread archive then log; X cannot clear a newer flag.
    let first = pending(test, &keys, shared).await;
    let server = test.server.clone();
    let request_keys = keys.clone();
    let parked = HookParked::start(owner_id, test_hooks::set_share_boundary_pause, async move {
        za_complete_boundary(
            &server,
            &request_keys,
            shared,
            Some(ZaBoundaryCommit { change_id: first }),
        )
        .await
        .unwrap()
    })
    .await;
    let newer = pending(test, &keys, shared).await;
    let returned = parked.finish().await;
    let current = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    assert_eq!(returned.version, current.stored.version);
    assert!(!current.boundary.unwrap().pending);
    assert!(current.boundary.unwrap().rename_change_id >= newer);
    // Both losses are bounded. In particular the old own commit cannot clear the third flag.
    let first = pending(test, &keys, shared).await;
    let server = test.server.clone();
    let request_keys = keys.clone();
    let parked = HookParked::start(owner_id, test_hooks::set_share_boundary_pause, async move {
        za_complete_boundary(
            &server,
            &request_keys,
            shared,
            Some(ZaBoundaryCommit { change_id: first }),
        )
        .await
        .unwrap()
    })
    .await;
    pending(test, &keys, shared).await;
    let fallback = Arc::new(test_hooks::Pause::new(owner_id));
    test_hooks::set_share_boundary_pause(Some(fallback.clone()));
    let finishing = tokio::spawn(parked.finish());
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fallback.arrived.notified(),
    )
    .await
    .unwrap();
    let newest = pending(test, &keys, shared).await;
    fallback.release.notify_one();
    finishing.await.unwrap();
    let current = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    assert!(current.boundary.unwrap().pending);
    za_complete_boundary(&test.server, &keys, shared, None)
        .await
        .unwrap();
    assert!(
        za_owned_calendar_context(&test.server, &keys, shared)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .rename_change_id
            >= newest
    );
    // Opportunistic repair gets only one attempt and preserves pending when its CAS loses.
    pending(test, &keys, shared).await;
    let server = test.server.clone();
    let request_keys = keys.clone();
    let parked = HookParked::start(owner_id, test_hooks::set_share_boundary_pause, async move {
        za_complete_boundary(&server, &request_keys, shared, None)
            .await
            .unwrap()
    })
    .await;
    pending(test, &keys, shared).await;
    let returned = parked.finish().await;
    let text = returned.deserialize::<Calendar>().unwrap().preferences[0]
        .name
        .clone();
    let header = groupware::calendar::sharing::format::parse_owner_header(
        &STANDARD
            .decode(
                text.strip_prefix("$za$")
                    .unwrap()
                    .split('|')
                    .next()
                    .unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(header.boundary_pending);
    assert!(
        za_owned_calendar_context(&test.server, &keys, shared)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .pending
    );
    za_complete_boundary(&test.server, &keys, shared, None)
        .await
        .unwrap();
    // Independent rename and boundary slots: the first commits nothing while held;
    // after release the second observes a new slug and durable pending header.
    let moving = async move {
        DummyWebDavClient::new(owner_id, USER, STRONG, USER)
            .request_with_headers(
                "MOVE",
                &format!("{HOME}shared/"),
                [("destination", format!("{HOME}renamed/").as_str())],
                "",
            )
            .await
            .status
    };
    let parked = HookParked::start(owner_id, test_hooks::set_share_rename_pause, moving).await;
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, shared)
            .await
            .unwrap()
            .stored
            .deserialize::<Calendar>()
            .unwrap()
            .name,
        "shared"
    );
    let completion = Arc::new(test_hooks::Pause::new(owner_id));
    test_hooks::set_share_boundary_pause(Some(completion.clone()));
    let finishing = tokio::spawn(parked.finish());
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        completion.arrived.notified(),
    )
    .await
    .unwrap();
    let during = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    assert_eq!(
        during.stored.deserialize::<Calendar>().unwrap().name,
        "renamed"
    );
    assert!(during.boundary.unwrap().pending);
    let committed_rename_change_id = during.stored.version.change_id().unwrap();
    completion.release.notify_one();
    assert_eq!(finishing.await.unwrap(), StatusCode::CREATED);
    let completed = za_owned_calendar_context(&test.server, &keys, shared)
        .await
        .unwrap();
    assert!(!completed.boundary.unwrap().pending);
    assert!(completed.boundary.unwrap().rename_change_id >= committed_rename_change_id);
    // COPY parked after its precheck cannot link into a source that converted meanwhile.
    client
        .request_with_headers(
            "PUT",
            &format!("{HOME}a/race.ics"),
            [("content-type", "text/calendar")],
            EVENT.replace("race-create", "copy-conversion"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let event_id = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("a/race.ics")
        .unwrap()
        .document_id();
    let copying = async move {
        DummyWebDavClient::new(owner_id, USER, STRONG, USER)
            .request_with_headers(
                "COPY",
                &format!("{HOME}a/race.ics"),
                [("destination", format!("{HOME}b/copied.ics").as_str())],
                "",
            )
            .await
            .status
    };
    let parked = HookParked::start(owner_id, test_hooks::set_copy_pause, copying).await;
    super::sharing::stage_converting_calendar(test, owner_id, a).await;
    assert_eq!(parked.finish().await, StatusCode::FORBIDDEN);
    assert_eq!(
        event_archive(test, owner_id, event_id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap()
            .names
            .len(),
        1
    );
    // Another stored parent is shared: COPY through the unshared path and collection COPY both refuse.
    let stored = event_archive(test, owner_id, event_id).await;
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    event.names.push(common::DavName {
        name: "linked.ics".into(),
        parent_id: b,
    });
    save_event(test, owner_id, event_id, &stored, event).await;
    for (from, to) in [("b/linked.ics", "b/forbidden.ics"), ("b/", "copied/")] {
        client
            .request_with_headers(
                "COPY",
                &format!("{HOME}{from}"),
                [("destination", format!("{HOME}{to}").as_str())],
                "",
            )
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    // The decisive pause is the first child's assembled batch, after destination
    // read-back. A conversion must lose its destination assertion before any link.
    // Checking every original version catches an unsafe first commit followed by403.
    client
        .request("MKCALENDAR", &format!("{HOME}split-source/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    for n in 0..48 {
        client
            .request_with_headers(
                "PUT",
                &format!("{HOME}split-source/{n}.ics"),
                [("content-type", "text/calendar")],
                EVENT.replace("race-create", &format!("split-{n}")),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    let source_resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let mut original_children = Vec::new();
    for resource in source_resources
        .subtree("split-source")
        .filter(|r| !r.is_container())
    {
        let stored = event_archive(test, owner_id, resource.document_id()).await;
        original_children.push((
            resource.document_id(),
            stored.version,
            stored.deserialize::<CalendarEvent>().unwrap().names,
        ));
    }
    let copying = async move {
        DummyWebDavClient::new(owner_id, USER, STRONG, USER)
            .request_with_headers(
                "COPY",
                &format!("{HOME}split-source/"),
                [("destination", format!("{HOME}split-target/").as_str())],
                "",
            )
            .await
            .status
    };
    let parked = HookParked::start(owner_id, test_hooks::set_copy_pause, copying).await;
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let target = resources.by_path("split-target").unwrap().document_id();
    assert_eq!(
        resources.children(target).count(),
        0,
        "calendar phase must precede child links"
    );
    super::sharing::stage_shared_calendar(test, owner_id, target, None).await;
    assert_eq!(parked.finish().await, StatusCode::FORBIDDEN);
    for (id, version, names) in original_children {
        let stored = event_archive(test, owner_id, id).await;
        assert_eq!(
            stored.version, version,
            "COPY committed a child despite destination conversion"
        );
        assert_eq!(
            stored.deserialize::<CalendarEvent>().unwrap().names,
            names,
            "COPY left a forbidden destination link"
        );
    }
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    assert_eq!(
        resources.by_path("split-target").unwrap().document_id(),
        target,
        "creation must not be replayed"
    );
    assert_eq!(resources.children(target).count(), 0);
    assert_eq!(
        resources
            .subtree("split-source")
            .filter(|r| !r.is_container())
            .count(),
        48
    );
    // Successful collection COPY and MOVE overwrite preserve names and remove
    // their source only after every child move completes.
    client
        .request("MKCALENDAR", &format!("{HOME}collection-source/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers(
            "PUT",
            &format!("{HOME}collection-source/one.ics"),
            [("content-type", "text/calendar")],
            EVENT.replace("race-create", "collection-one"),
        )
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers(
            "COPY",
            &format!("{HOME}collection-source/"),
            [("destination", format!("{HOME}collection-target/").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    client
        .request("DELETE", &format!("{HOME}collection-target/one.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}collection-source/"),
            [("destination", format!("{HOME}collection-target/").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("GET", &format!("{HOME}collection-target/one.ics"), "")
        .await
        .with_status(StatusCode::OK);
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    assert!(resources.by_path("collection-source").is_none());
    for slug in ["split-source", "split-target", "collection-target"] {
        client
            .request_with_headers(
                "DELETE",
                &format!("{HOME}{slug}/"),
                [super::fanout::NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    overwrite_grantee_unlink_authority(test, owner_id, &client).await;
    overwrite_other_parent_fences(test, owner_id, &client).await;
    collection_move_conversion_policy(test, owner_id, &client).await;
    for slug in ["a", "b", "renamed"] {
        client
            .request_with_headers(
                "DELETE",
                &format!("{HOME}{slug}/"),
                [super::fanout::NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    test_hooks::set_share_rename_pause(None);
    test_hooks::set_share_boundary_pause(None);
    test_hooks::set_copy_pause(None);
}

async fn overwrite_other_parent_fences(test: &TestServer, owner: u32, client: &DummyWebDavClient) {
    use std::sync::Arc;
    const USER: &str = "share-boundary@example.com";
    const HOME: &str = "/dav/cal/share-boundary%40example.com/";
    for slug in [
        "ow-source",
        "ow-target",
        "ow-other",
        "oc-source",
        "oc-target",
        "oc-other",
    ] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    for (slug, uid) in [
        ("ow-source", "ow-source"),
        ("ow-target", "ow-target"),
        ("oc-source", "oc-source"),
        ("oc-target", "oc-target"),
    ] {
        client
            .request_with_headers(
                "PUT",
                &format!("{HOME}{slug}/event.ics"),
                [("content-type", "text/calendar")],
                EVENT.replace("race-create", uid),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    for (from, to) in [("ow-target", "ow-other"), ("oc-target", "oc-other")] {
        client
            .request_with_headers(
                "COPY",
                &format!("{HOME}{from}/event.ics"),
                [("destination", format!("{HOME}{to}/event.ics").as_str())],
                "",
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let ow_other = resources.by_path("ow-other").unwrap().document_id();
    let ow_target = resources
        .by_path("ow-target/event.ics")
        .unwrap()
        .document_id();
    let ow_original = event_archive(test, owner, ow_target).await;
    let oc_other = resources.by_path("oc-other").unwrap().document_id();
    let oc_target = resources
        .by_path("oc-target/event.ics")
        .unwrap()
        .document_id();
    let oc_original = event_archive(test, owner, oc_target).await;
    let moving = async move {
        DummyWebDavClient::new(owner, USER, STRONG, USER)
            .request_with_headers(
                "MOVE",
                &format!("{HOME}ow-source/event.ics"),
                [("destination", format!("{HOME}ow-target/event.ics").as_str())],
                "",
            )
            .await
            .status
    };
    let parked = HookParked::start(owner, test_hooks::set_copy_pause, moving).await;
    super::sharing::stage_converting_calendar(test, owner, ow_other).await;
    let second = Arc::new(test_hooks::Pause::new(owner));
    test_hooks::set_copy_pause(Some(second.clone()));
    let finishing = tokio::spawn(parked.finish());
    let retried =
        tokio::time::timeout(std::time::Duration::from_secs(5), second.arrived.notified())
            .await
            .is_ok();
    if retried {
        assert_eq!(
            event_archive(test, owner, ow_target).await.version,
            ow_original.version,
            "first lost attempt must not unlink overwrite target"
        );
        second.release.notify_one();
    }
    let moved = finishing.await.unwrap();
    test_hooks::set_copy_pause(None);
    assert_eq!(
        test_hooks::take_move_overwrite_message(owner, ow_target),
        Some(false)
    );
    assert_eq!(moved, StatusCode::NO_CONTENT);
    let survivor = event_archive(test, owner, ow_target)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    assert_eq!(survivor.names.len(), 1);
    assert_eq!(survivor.names[0].parent_id, ow_other);
    // Collection overwrite has no replay of creation; its pre-creation unlink
    // chunk must lose the other parent's assertion before changing any name.
    let copying = async move {
        DummyWebDavClient::new(owner, USER, STRONG, USER)
            .request_with_headers(
                "COPY",
                &format!("{HOME}oc-source/"),
                [("destination", format!("{HOME}oc-target/").as_str())],
                "",
            )
            .await
            .status
    };
    let parked = HookParked::start(owner, test_hooks::set_delete_commit_pause, copying).await;
    super::sharing::stage_converting_calendar(test, owner, oc_other).await;
    let copied = parked.finish().await;
    let unchanged = event_archive(test, owner, oc_target).await.version == oc_original.version;
    println!(
        "Task5 overwrite other-parent fences: event_retried={retried}, collection_status={copied}, original_unchanged={unchanged}"
    );
    assert!(
        retried && copied == StatusCode::CONFLICT && unchanged,
        "overwrite escaped other-parent calendar fence"
    );
    for slug in [
        "ow-source",
        "ow-target",
        "ow-other",
        "oc-source",
        "oc-target",
        "oc-other",
    ] {
        client
            .request_with_headers(
                "DELETE",
                &format!("{HOME}{slug}/"),
                [super::fanout::NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    test_hooks::set_delete_commit_pause(None);
}

async fn collection_move_conversion_policy(
    test: &TestServer,
    owner: u32,
    client: &DummyWebDavClient,
) {
    const USER: &str = "share-boundary@example.com";
    const HOME: &str = "/dav/cal/share-boundary%40example.com/";
    let mut results = Vec::new();
    for convert_source in [false, true] {
        let label = if convert_source {
            "cm-source"
        } else {
            "cm-destination"
        };
        let source_slug = format!("{label}-from");
        let target_slug = format!("{label}-to");
        for slug in [&source_slug, &target_slug] {
            client
                .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
                .await
                .with_status(StatusCode::CREATED);
        }
        for n in 0..3 {
            client
                .request_with_headers(
                    "PUT",
                    &format!("{HOME}{source_slug}/{n}.ics"),
                    [("content-type", "text/calendar")],
                    EVENT.replace("race-create", &format!("{label}-{n}")),
                )
                .await
                .with_status(StatusCode::CREATED);
        }
        let resources = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap();
        let source_id = resources.by_path(&source_slug).unwrap().document_id();
        let target_id = resources.by_path(&target_slug).unwrap().document_id();
        let mut originals = Vec::new();
        for event in resources.children(source_id) {
            let stored = event_archive(test, owner, event.document_id()).await;
            originals.push((
                event.document_id(),
                stored.version,
                stored.deserialize::<CalendarEvent>().unwrap().names,
            ));
        }
        let from = format!("{HOME}{source_slug}/");
        let to = format!("{HOME}{target_slug}/");
        let moving = async move {
            DummyWebDavClient::new(owner, USER, STRONG, USER)
                .request_with_headers("MOVE", &from, [("destination", to.as_str())], "")
                .await
                .status
        };
        let parked = HookParked::start(owner, test_hooks::set_copy_pause, moving).await;
        if convert_source {
            super::sharing::stage_converting_calendar(test, owner, source_id).await;
        } else {
            super::sharing::stage_shared_calendar(test, owner, target_id, None).await;
        }
        let status = parked.finish().await;
        let mut unchanged = true;
        for (id, version, names) in originals {
            let stored = event_archive(test, owner, id).await;
            unchanged &= stored.version == version
                && stored.deserialize::<CalendarEvent>().unwrap().names == names;
        }
        let resources = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap();
        let source_retained = resources
            .by_path(&source_slug)
            .is_some_and(|r| r.document_id() == source_id);
        println!(
            "Task5 collection MOVE {label} conversion: status={status}, all_original_versions_names_unchanged={unchanged}, source_retained={source_retained}"
        );
        results.push(status == StatusCode::FORBIDDEN && unchanged && source_retained);
        for slug in [&source_slug, &target_slug] {
            client
                .request_with_headers(
                    "DELETE",
                    &format!("{HOME}{slug}/"),
                    [super::fanout::NO_REPLY],
                    "",
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
    assert!(
        results.iter().all(|ok| *ok),
        "collection MOVE used event-MOVE policy after a calendar conversion"
    );
}

/// The old destination is an admitted legacy/interrupted linked recovery state:
/// B later became shared while its old B+C event and private C link survived.
/// No healthy shared COPY is claimed to have produced it. Foreign HTTP admission
/// remains Task10; this invokes the actual private MOVE with a real writer token.
async fn overwrite_grantee_unlink_authority(
    test: &mut TestServer,
    owner: u32,
    client: &DummyWebDavClient,
) {
    use std::sync::Arc;
    use types::acl::Acl;
    const HOME: &str = "/dav/cal/share-boundary%40example.com/";
    let writer = test
        .account("admin@example.com")
        .create_key_user_account(
            "share-overwrite-writer@example.com",
            STRONG,
            "Overwrite writer",
            &[],
            user_permissions(),
        )
        .await;
    let writer_id = writer.id().document_id();
    test.insert_account(writer);
    let writer_keys = za_unlock(test, writer_id, STRONG).await;
    let owner_keys = za_unlock(test, owner, STRONG).await;
    for retry in [false, true] {
        let label = if retry {
            "grantee-retry"
        } else {
            "grantee-control"
        };
        let slugs = [
            format!("{label}-a"),
            format!("{label}-b"),
            format!("{label}-c"),
        ];
        for slug in &slugs {
            client
                .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
                .await
                .with_status(StatusCode::CREATED);
        }
        for slug in &slugs[..2] {
            client
                .request_with_headers(
                    "PUT",
                    &format!("{HOME}{slug}/event.ics"),
                    [("content-type", "text/calendar")],
                    EVENT.replace("race-create", slug),
                )
                .await
                .with_status(StatusCode::CREATED);
        }
        client
            .request_with_headers(
                "COPY",
                &format!("{HOME}{}/event.ics", slugs[1]),
                [(
                    "destination",
                    format!("{HOME}{}/event.ics", slugs[2]).as_str(),
                )],
                "",
            )
            .await
            .with_status(StatusCode::CREATED);
        let resources = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap();
        let a = resources.by_path(&slugs[0]).unwrap().document_id();
        let b = resources.by_path(&slugs[1]).unwrap().document_id();
        let c = resources.by_path(&slugs[2]).unwrap().document_id();
        let source = resources
            .by_path(&format!("{}/event.ics", slugs[0]))
            .unwrap()
            .document_id();
        let target = resources
            .by_path(&format!("{}/event.ics", slugs[1]))
            .unwrap()
            .document_id();
        let old_a = za_owned_calendar_context(&test.server, &owner_keys, a)
            .await
            .unwrap();
        super::sharing::stage_shared_calendar(test, owner, a, Some(writer_id)).await;
        super::sharing::stage_shared_calendar(test, owner, b, Some(writer_id)).await;
        let new_a = za_owned_calendar_context(&test.server, &owner_keys, a)
            .await
            .unwrap();
        // Direct staged grants precede a fresh legitimate token, just as a new
        // authenticated request must observe collection access after granting.
        test.server.inner.cache.access_tokens.remove(&writer_id);
        let token = common::auth::AccessToken::new_maybe_invalid(
            test.server.access_token(writer_id).await.unwrap(),
        )
        .with_session_keys(writer_keys.clone());
        assert!(token.has_shared_access(owner, Collection::Calendar));
        for (calendar, required) in [
            (
                a,
                utils::map::bitmap::Bitmap::new().with_item(Acl::RemoveItems),
            ),
            (
                b,
                utils::map::bitmap::Bitmap::new()
                    .with_item(Acl::AddItems)
                    .with_item(Acl::RemoveItems),
            ),
        ] {
            assert!(
                za_calendar_context(&test.server, &token, owner, calendar, required)
                    .await
                    .is_ok_and(|context| context.is_some()),
                "addressed A/B authority must pass"
            );
        }

        let source_stored = event_archive(test, owner, source).await;
        let mut source_event = source_stored.deserialize::<CalendarEvent>().unwrap();
        rewrap_event(&mut source_event, &old_a.domain, &new_a.domain).unwrap();
        save_event(test, owner, source, &source_stored, source_event).await;
        assert!(
            matches!(
                za_calendar_context(
                    &test.server,
                    &token,
                    owner,
                    c,
                    utils::map::bitmap::Bitmap::new().with_item(Acl::ReadItems)
                )
                .await,
                Err(ZaCalendarError::Forbidden)
            ),
            "writer must have no normal access to surviving private C"
        );
        let original_source = event_archive(test, owner, source).await;
        let original_target = event_archive(test, owner, target).await;
        let old_target = original_target.deserialize::<CalendarEvent>().unwrap();
        let moving_slugs = slugs.clone();
        let moving = async move {
            let response = crate::utils::za::sharing_dav_request(
                "share-overwrite-writer@example.com",
                "MOVE",
                &format!("{HOME}{}/event.ics", moving_slugs[0]),
                &[(
                    "destination",
                    &format!("{HOME}{}/event.ics", moving_slugs[1]),
                )],
                "",
            )
            .await;
            if response.status == StatusCode::NO_CONTENT {
                Ok(response.status)
            } else {
                Err(response.status)
            }
        };
        let response = if retry {
            let parked = HookParked::start(owner, test_hooks::set_copy_pause, moving).await;
            let private: Archive<AlignedBytes> = test
                .server
                .store()
                .get_value(ValueKey::archive(owner, Collection::Calendar, c))
                .await
                .unwrap()
                .unwrap();
            let mut changed = private.deserialize::<Calendar>().unwrap();
            // Calendar::update sets modified itself, so change a preserved
            // public field to force an actual raw archive assertion loss.
            changed.preferences[0].sort_order += 1;
            let mut batch = BatchBuilder::new();
            changed
                .update(
                    test.server
                        .account_info(owner)
                        .await
                        .unwrap()
                        .account_tenant_ids(),
                    private.to_unarchived::<Calendar>().unwrap(),
                    owner,
                    c,
                    &mut batch,
                )
                .unwrap();
            test.server.commit_batch(batch).await.unwrap();
            let fresh_private: Archive<AlignedBytes> = test
                .server
                .store()
                .get_value(ValueKey::archive(owner, Collection::Calendar, c))
                .await
                .unwrap()
                .unwrap();
            assert_ne!(
                private.version.hash(),
                fresh_private.version.hash(),
                "retry fixture must change stored calendar content before releasing MOVE"
            );
            let second = Arc::new(test_hooks::Pause::new(owner));
            test_hooks::set_copy_pause(Some(second.clone()));
            let finishing = tokio::spawn(parked.finish());
            tokio::time::timeout(std::time::Duration::from_secs(5), second.arrived.notified())
                .await
                .expect("private surviving parent change must cause a calendar retry");
            assert_eq!(
                event_archive(test, owner, source).await.version,
                original_source.version
            );
            assert_eq!(
                event_archive(test, owner, target).await.version,
                original_target.version
            );
            second.release.notify_one();
            finishing.await.unwrap()
        } else {
            moving.await
        };
        test_hooks::set_copy_pause(None);
        let response = response
            .expect("authorized grantee overwrite may unlink B without private C authority");
        assert_eq!(response, StatusCode::NO_CONTENT);
        assert_eq!(
            test_hooks::take_move_overwrite_message(owner, target),
            Some(false),
            "Bob HTTP overwrite must preserve the outer no-CANCEL Option"
        );
        let surviving = event_archive(test, owner, target)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        assert_eq!(surviving.names.len(), 1);
        assert_eq!(surviving.names[0].parent_id, c);
        assert_eq!(
            surviving.data.event.to_string(),
            old_target.data.event.to_string()
        );
        assert_eq!(surviving.created, old_target.created);
        assert_eq!(surviving.modified, old_target.modified);
        let moved = event_archive(test, owner, source)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        assert_eq!(moved.names.len(), 1);
        assert_eq!(moved.names[0].parent_id, b);
        assert_eq!(
            moved.created,
            original_source
                .deserialize::<CalendarEvent>()
                .unwrap()
                .created
        );
        println!(
            "Task5 grantee overwrite private parent: retry={retry}, status204, private access denied, payload preserved"
        );
        for slug in &slugs {
            client
                .request_with_headers(
                    "DELETE",
                    &format!("{HOME}{slug}/"),
                    [super::fanout::NO_REPLY],
                    "",
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
}

/// A long-lived local test client is needed only for the 3,000-row request.
async fn share_post(path: &'static str, body: serde_json::Value) -> crate::utils::za::VaultReply {
    let response = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap()
        .post(format!("{}/api/vault/{path}", crate::utils::za::SERVER_URL))
        .json(&body)
        .send()
        .await
        .unwrap();
    crate::utils::za::VaultReply {
        status: response.status().as_u16(),
        body: response.json().await.unwrap(),
    }
}

struct HttpSharingHooks;
impl Drop for HttpSharingHooks {
    fn drop(&mut self) {
        use common::za::test_hooks::*;
        set_share_pin_mark_pause(None);
        set_share_conflict_pause(None);
        set_share_commit_pause(None);
        set_share_precheck_pause(None);
        set_share_rewrap_pause(None);
        set_share_init_assert_fail(None);
        set_share_rewrap_assert_fail(None);
        set_share_revert_assert_fail(None);
        set_share_rollback_assert_fail(None);
        set_share_restore_assert_fail(None);
        set_share_rewrap_fail_once(None);
    }
}

async fn sharing_http_races(test: &mut TestServer) {
    use crate::utils::za::{plant_event, plant_sealed_event, za_post, za_post_concurrent};
    use common::za::test_hooks::*;
    use serde_json::json;
    use vault::keys::display_fingerprint;
    let _reset = HttpSharingHooks;
    println!("Running HTTP sharing fences, conversion budgets and resume tests...");
    const USER: &str = "share-http-races@example.com";
    const HOME: &str = "/dav/cal/share-http-races%40example.com/";
    const READER: &str = "shares-http-reader@example.com";
    const SECOND: &str = "shares-http-second@example.com";
    let account = test
        .account("admin@example.com")
        .create_key_user_account(USER, STRONG, "Share races", &[], user_permissions())
        .await;
    let owner = account.id().document_id();
    test.insert_account(account);
    let reader = test.account(READER).id().document_id();
    let second = test.account(SECOND).id().document_id();
    for (address, id) in [(READER, reader), (SECOND, second)] {
        super::pinning::pin(
            USER,
            address,
            &display_fingerprint(&test.server.za_served_public_key(id).await.unwrap().unwrap()),
        )
        .await
        .expect(200);
    }
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let request = |slug: &str, address: &str| json!({"username":USER,"password":STRONG,"calendar":slug,"address":address,"level":"view"});
    let mut slugs = Vec::new();
    for slug in [
        "race", "init", "rewrap", "revert", "rollback", "restore", "cut", "large", "other",
    ] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        slugs.push(slug);
    }
    let calendar = |slug: &str| {
        let server = test.server.clone();
        let slug = slug.to_owned();
        async move {
            server
                .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
                .await
                .unwrap()
                .by_path(&slug)
                .unwrap()
                .document_id()
        }
    };
    let race = calendar("race").await;
    // Real calendar-archive assertion loss: two grants must both survive, once each.
    let answers = za_post_concurrent(
        "shares/grant",
        &[request("race", READER), request("race", SECOND)],
    )
    .await;
    for reply in answers {
        reply.expect(200);
    }
    let keys = za_unlock(test, owner, STRONG).await;
    let state = za_owned_calendar_context(&test.server, &keys, race)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    assert_eq!(state.acls.len(), 2);
    assert_eq!(state.preferences.len(), 3);
    // Cache repopulation while a revoke is parked must be followed by explicit eviction.
    let revoke = json!({"username":USER,"password":STRONG,"calendar":"race","grant":reader});
    let parked = HookParked::start(
        owner,
        set_share_commit_pause,
        share_post("shares/revoke", revoke),
    )
    .await;
    let reader_client = DummyWebDavClient::new(reader, READER, STRONG, READER);
    reader_client
        .request("PROPFIND", "/dav/cal/shares-http-reader%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    reader_client
        .request("PROPFIND", &format!("{HOME}race/"), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(test.server.inner.cache.za_keys.contains_account(reader));
    let mut level_change = request("race", SECOND);
    level_change["level"] = json!("edit");
    za_post("shares/grant", &level_change).await.expect(200);
    let second_client = DummyWebDavClient::new(second, SECOND, STRONG, SECOND);
    second_client
        .request("PROPFIND", "/dav/cal/shares-http-second%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(test.server.inner.cache.za_keys.contains_account(second));
    let second_live_before = vault::session::live::session_keys(second);
    let live_before = vault::session::live::session_keys(reader);
    parked.finish().await.expect(200);
    assert!(!test.server.inner.cache.za_keys.contains_account(reader));
    assert!(
        vault::session::live::session_keys(reader) < live_before,
        "resident session keys must be dropped"
    );
    assert!(
        !test.server.inner.cache.za_keys.contains_account(second),
        "rotation also evicts remaining grantees"
    );
    assert!(vault::session::live::session_keys(second) < second_live_before);
    reader_client
        .request("PROPFIND", &format!("{HOME}race/"), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    second_client
        .request("PROPFIND", &format!("{HOME}race/"), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let survived = za_owned_calendar_context(&test.server, &keys, race)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    assert_eq!(survived.acls.len(), 1);
    assert_eq!(survived.acls[0].account_id, second);
    assert_eq!(survived.acls[0].grants.bitmap, 123);
    za_post("shares/grant", &request("race", SECOND))
        .await
        .expect(200);
    // A pin change during the authoritative decision forces fresh resolution, no grant.
    let parked = HookParked::start(
        owner,
        set_share_commit_pause,
        share_post("shares/grant", request("race", READER)),
    )
    .await;
    super::pinning::forget(USER, Some(READER)).await.expect(200);
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "contact not pinned"
    );
    super::pinning::pin(
        USER,
        READER,
        &display_fingerprint(
            &test
                .server
                .za_served_public_key(reader)
                .await
                .unwrap()
                .unwrap(),
        ),
    )
    .await
    .expect(200);
    let saved_pins = crate::utils::za::raw_pins(test, owner).await.unwrap();
    let parked = HookParked::start(
        owner,
        set_share_commit_pause,
        share_post("shares/grant", request("race", READER)),
    )
    .await;
    crate::utils::za::set_raw_pins(test, owner, None).await;
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "pin record missing"
    );
    crate::utils::za::set_raw_pins(test, owner, Some(&saved_pins)).await;
    // A real password change fences the parked grant's request-scoped keys.
    let parked = HookParked::start(
        owner,
        set_share_commit_pause,
        share_post("shares/grant", request("race", READER)),
    )
    .await;
    let version_before_password = za_owned_calendar_context(&test.server, &keys, race)
        .await
        .unwrap()
        .stored
        .version;
    const NEW_PASSWORD: &str = "sharing race replacement passphrase with 884 numbers";
    za_post(
        "password",
        &json!({"username":USER,"password":STRONG,"new_password":NEW_PASSWORD}),
    )
    .await
    .expect(200);
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "vault changed since verification"
    );
    let changed_keys = za_unlock(test, owner, NEW_PASSWORD).await;
    assert_eq!(
        za_owned_calendar_context(&test.server, &changed_keys, race)
            .await
            .unwrap()
            .stored
            .version,
        version_before_password
    );
    za_post(
        "password",
        &json!({"username":USER,"password":NEW_PASSWORD,"new_password":STRONG}),
    )
    .await
    .expect(200);
    let parked = HookParked::start(
        owner,
        set_share_commit_pause,
        share_post("shares/grant", request("race", READER)),
    )
    .await;
    reader_client
        .request("PROPFIND", "/dav/cal/shares-http-reader%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(test.server.inner.cache.za_keys.contains_account(reader));
    let live_before = vault::session::live::session_keys(reader);
    parked.finish().await.expect(200);
    assert!(!test.server.inner.cache.za_keys.contains_account(reader));
    assert!(vault::session::live::session_keys(reader) < live_before);

    let answers = za_post_concurrent(
        "shares/grant",
        &[request("race", READER), request("race", SECOND)],
    )
    .await;
    for reply in answers {
        assert_eq!(reply.expect(200)["changed"], false);
    }
    sharing_changed_attribution_contract(test, owner, &client).await;
    sharing_dav_copy_after_precheck_contract(test, owner, &client).await;
    sharing_boundary_error_contract(test, owner, reader, &client).await;
    sharing_terminal_contracts(test, owner, &client).await;
    sharing_event_terminal_contract(test, owner, &client).await;
    // Every typed conversion write class exhausts at exactly eight, then resumes.
    let other = calendar("other").await;
    for class in ["init", "rewrap", "revert", "rollback", "restore"] {
        let id = calendar(class).await;
        let event = plant_sealed_event(
            test,
            owner,
            class,
            "old.ics",
            &super::sharing::sharing_event(&format!("http-{class}")),
            STRONG,
            &Default::default(),
        )
        .await;
        let failure = Some(ShareAssertFailure {
            owner_id: owner,
            calendar_id: id,
            remaining: 8,
        });
        if matches!(class, "init" | "rewrap") {
            if class == "init" {
                set_share_init_assert_fail(failure);
            } else {
                set_share_rewrap_assert_fail(failure);
            }
            assert_eq!(
                za_post("shares/grant", &request(class, READER))
                    .await
                    .expect(409)["error"],
                "conflict"
            );
        } else {
            // Add a real link inside the unfenced precheck window; after phase1
            // create a CK event which rollback must rewrite before dropping its ring.
            let before = std::sync::Arc::new(Pause::new(owner));
            let after = std::sync::Arc::new(Pause::new(owner));
            set_share_precheck_pause(Some(before.clone()));
            set_share_rewrap_pause(Some(after.clone()));
            let pending = tokio::spawn(share_post("shares/grant", request(class, READER)));
            tokio::time::timeout(std::time::Duration::from_secs(5), before.arrived.notified())
                .await
                .unwrap();
            let stored = event_archive(test, owner, event).await;
            let mut linked = stored.deserialize::<CalendarEvent>().unwrap();
            linked.names.push(common::DavName {
                name: format!("{class}-copy.ics"),
                parent_id: other,
            });
            save_event(test, owner, event, &stored, linked).await;
            before.release.notify_one();
            tokio::time::timeout(std::time::Duration::from_secs(5), after.arrived.notified())
                .await
                .unwrap();
            client
                .request_with_headers(
                    "PUT",
                    &format!("{HOME}{class}/new.ics"),
                    [("content-type", "text/calendar")],
                    super::sharing::sharing_event(&format!("http-{class}-new")),
                )
                .await
                .with_status(StatusCode::CREATED);
            match class {
                "revert" => set_share_revert_assert_fail(failure),
                "rollback" => set_share_rollback_assert_fail(failure),
                _ => set_share_restore_assert_fail(failure),
            }
            after.release.notify_one();
            assert_eq!(pending.await.unwrap().expect(409)["error"], "conflict");
        }
        let current =
            za_owned_calendar_context(&test.server, &*za_unlock(test, owner, STRONG).await, id)
                .await
                .unwrap();
        let stored = current.stored.deserialize::<Calendar>().unwrap();
        assert!(stored.acls.is_empty());
        assert_eq!(stored.preferences.len(), 1);
        let version = current.stored.version;
        assert_eq!(
            za_post(
                "shares/revoke",
                &json!({"username":USER,"password":STRONG,"calendar":class,"grant":reader})
            )
            .await
            .expect(200)["changed"],
            false,
            "an unpublished journal has no grant to revoke"
        );
        assert_eq!(
            za_owned_calendar_context(&test.server, &*za_unlock(test, owner, STRONG).await, id)
                .await
                .unwrap()
                .stored
                .version,
            version
        );
        if class == "revert" {
            for path in [
                format!("{HOME}{class}/old.ics"),
                format!("{HOME}other/{class}-copy.ics"),
            ] {
                client
                    .request("GET", &path, "")
                    .await
                    .with_status(StatusCode::OK);
                client
                    .request_with_headers(
                        "PUT",
                        &path,
                        [("content-type", "text/calendar")],
                        super::sharing::sharing_event(&format!("http-{class}")),
                    )
                    .await
                    .with_status(StatusCode::CONFLICT);
            }
        }
        // Explicitly clear the exhausted slot before proving next-request completion.
        set_share_init_assert_fail(None);
        set_share_rewrap_assert_fail(None);
        set_share_revert_assert_fail(None);
        set_share_rollback_assert_fail(None);
        set_share_restore_assert_fail(None);
        if matches!(class, "revert" | "rollback" | "restore") {
            assert_eq!(
                za_post("shares/grant", &request(class, READER))
                    .await
                    .expect(409)["error"],
                "calendar has linked events"
            );
            client
                .request_with_headers(
                    "PUT",
                    &format!("{HOME}{class}/old.ics"),
                    [("content-type", "text/calendar")],
                    super::sharing::sharing_event(&format!("http-{class}")),
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
            client
                .request("GET", &format!("{HOME}other/{class}-copy.ics"), "")
                .await
                .with_status(StatusCode::OK);
            client
                .request("DELETE", &format!("{HOME}other/{class}-copy.ics"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
        let result = za_post("shares/grant", &request(class, READER))
            .await
            .expect(200);
        assert_eq!(
            result["rewrapped"],
            if matches!(class, "init" | "rewrap") {
                1
            } else {
                2
            }
        );
        reader_client
            .request("GET", &format!("{HOME}{class}/old.ics"), "")
            .await
            .with_status(StatusCode::OK);
        if matches!(class, "revert" | "rollback" | "restore") {
            let fresh =
                za_owned_calendar_context(&test.server, &*za_unlock(test, owner, STRONG).await, id)
                    .await
                    .unwrap();
            assert_ne!(fresh.domain.domain_id(), current.domain.domain_id());
            client
                .request("GET", &format!("{HOME}{class}/new.ics"), "")
                .await
                .with_status(StatusCode::OK);
            reader_client
                .request("GET", &format!("{HOME}{class}/new.ics"), "")
                .await
                .with_status(StatusCode::OK);
        }
    }
    sharing_count_delta_contract(test, owner, &client).await;
    // Interruptions are store errors, leave a readable journal, then resume.
    plant_event(
        test,
        owner,
        "cut",
        "plain.ics",
        &super::sharing::sharing_event("http-cut"),
        None,
    )
    .await;
    set_share_rewrap_fail_once(Some(owner));
    assert_eq!(
        za_post("shares/grant", &request("cut", READER))
            .await
            .status,
        500
    );
    client
        .request("GET", &format!("{HOME}cut/plain.ics"), "")
        .await
        .with_status(StatusCode::OK);
    assert_eq!(
        za_post("shares/grant", &request("cut", READER))
            .await
            .expect(200)["sealed"],
        1
    );
    reader_client
        .request("GET", &format!("{HOME}cut/plain.ics"), "")
        .await
        .with_status(StatusCode::OK);
    // Thirty successful logical groups consume neither request-wide loss budget.
    for index in 0..3000 {
        plant_event(
            test,
            owner,
            "large",
            &format!("event-{index}.ics"),
            &super::sharing::sharing_event(&format!("http-large-{index}")),
            None,
        )
        .await;
    }
    let large = share_post("shares/grant", request("large", READER))
        .await
        .expect(200);
    assert_eq!(large["sealed"], 3000);
    assert_eq!(large["rewrapped"], 0);
    for slug in slugs {
        if slug == "large" {
            let response = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .unwrap()
                .delete(format!("{}{HOME}{slug}/", crate::utils::za::SERVER_URL))
                .basic_auth(USER, Some(STRONG))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 204);
            continue;
        }
        client
            .request("DELETE", &format!("{HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    let _ = client
        .request("DELETE", &format!("{HOME}default/"), "")
        .await;
    test.wait_for_tasks().await;
}

async fn sharing_terminal_contracts(test: &TestServer, owner: u32, client: &DummyWebDavClient) {
    use crate::utils::za::{plant_event, za_post};
    use common::za::test_hooks::*;
    use serde_json::json;
    const USER: &str = "share-http-races@example.com";
    const HOME: &str = "/dav/cal/share-http-races%40example.com/";
    let request = |slug: &str| json!({"username":USER,"password":STRONG,"calendar":slug,"address":"shares-http-reader@example.com","level":"view"});
    // The eighth opaque loss must freshly choose terminal Account/vault errors
    // before returning the exhausted-budget conflict. Pause before either read.
    for terminal in ["vault", "account"] {
        let slug = format!("terminal-{terminal}");
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        let id = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(&slug)
            .unwrap()
            .document_id();
        set_share_init_assert_fail(Some(ShareAssertFailure {
            owner_id: owner,
            calendar_id: id,
            remaining: 8,
        }));
        let mut pause = std::sync::Arc::new(Pause::new(owner));
        set_share_conflict_pause(Some(pause.clone()));
        let pending = tokio::spawn(share_post("shares/grant", request(&slug)));
        let mut account = None;
        for index in 0..8 {
            tokio::time::timeout(std::time::Duration::from_secs(5), pause.arrived.notified())
                .await
                .unwrap();
            assert_eq!(
                SHARE_INIT_ASSERT_FAIL
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .remaining,
                7 - index
            );
            if index == 7 {
                let old = test.server.za_vault_record(owner).await.unwrap().unwrap();
                let mut new = old.record.clone();
                new.revision += 1;
                test.server
                    .za_vault_write(owner, &new, Some(&old))
                    .await
                    .unwrap();
                if terminal == "account" {
                    account = Some(super::sharing::conversion_remove_account(test, owner).await);
                }
                pause.release.notify_one();
            } else {
                let next = std::sync::Arc::new(Pause::new(owner));
                set_share_conflict_pause(Some(next.clone()));
                pause.release.notify_one();
                pause = next;
            }
        }
        assert_eq!(
            pending.await.unwrap().expect(409)["error"],
            if terminal == "account" {
                "account deleted"
            } else {
                "vault changed since verification"
            }
        );
        if let Some((class, bytes)) = account {
            let mut batch = BatchBuilder::new();
            batch.set(class, bytes);
            test.server.store().write(batch.build_all()).await.unwrap();
        }
        set_share_init_assert_fail(None);
        za_post("shares/grant", &request(&slug)).await.expect(200);
        client
            .request("DELETE", &format!("{HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    // A real changed calendar after 100 acknowledged event writes plus a changed
    // vault yields progress_changed=true. Its committed delta is not permission to resume.
    let slug = "terminal-progress";
    client
        .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let id = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(slug)
        .unwrap()
        .document_id();
    for index in 0..101 {
        plant_event(
            test,
            owner,
            slug,
            &format!("{index}.ics"),
            &super::sharing::sharing_event(&format!("terminal-progress-{index}")),
            None,
        )
        .await;
    }
    super::sharing::stage_converting_calendar(test, owner, id).await;
    let parked = HookParked::start(
        owner,
        set_share_rewrap_pause,
        share_post("shares/grant", request(slug)),
    )
    .await;
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
    calendar.modified += 1;
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            context.stored.to_unarchived::<Calendar>().unwrap(),
            owner,
            id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    let old = test.server.za_vault_record(owner).await.unwrap().unwrap();
    let mut new = old.record.clone();
    new.revision += 1;
    test.server
        .za_vault_write(owner, &new, Some(&old))
        .await
        .unwrap();
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "vault changed since verification"
    );
    let state = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    assert!(state.acls.is_empty());
    assert_eq!(
        za_post("shares/grant", &request(slug)).await.expect(200)["sealed"],
        1
    );
    client
        .request("DELETE", &format!("{HOME}{slug}/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // First-pin ordering: the grant's unusable read precedes an atomic real first
    // pin. Its final pin reread sees it; verified-generation loss still refuses.
    crate::utils::za::rewrap_legacy(test, owner, STRONG).await;
    crate::utils::za::set_raw_pins(test, owner, None).await;
    let parked = HookParked::start(
        owner,
        set_share_pin_mark_pause,
        share_post("shares/grant", request("race")),
    )
    .await;
    let reader = test
        .account("shares-http-reader@example.com")
        .id()
        .document_id();
    super::pinning::pin(
        USER,
        "shares-http-reader@example.com",
        &vault::keys::display_fingerprint(
            &test
                .server
                .za_served_public_key(reader)
                .await
                .unwrap()
                .unwrap(),
        ),
    )
    .await
    .expect(200);
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "vault changed since verification"
    );
    let pins = crate::utils::za::raw_pins(test, owner).await.unwrap();
    let generation = crate::utils::za::vault_revision(test, owner).await;
    crate::utils::za::set_raw_pins(test, owner, Some(&[0x11; 32])).await;
    let parked = HookParked::start(
        owner,
        set_share_pin_mark_pause,
        share_post("shares/grant", request("race")),
    )
    .await;
    crate::utils::za::set_raw_pins(test, owner, Some(&pins)).await;
    assert_eq!(parked.finish().await.expect(200)["changed"], false);
    assert_eq!(
        crate::utils::za::vault_revision(test, owner).await,
        generation
    );
}

async fn sharing_destruction_contracts(test: &mut TestServer) {
    use crate::utils::za::za_post;
    use common::za::test_hooks::set_share_commit_pause;
    use registry::schema::prelude::ObjectType;
    use serde_json::json;
    use store::{Deserialize, IterateParams, LogKey, query::acl::AclQuery};
    const USER: &str = "share-destroy-owner@example.com";
    const GONE: &str = "share-destroy-grantee@example.com";
    const READER: &str = "shares-http-reader@example.com";
    const SECOND: &str = "shares-http-second@example.com";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(USER, STRONG, "Destroy owner", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let gone = test
        .account("admin@example.com")
        .create_key_user_account(GONE, STRONG, "Gone grantee", &[], user_permissions())
        .await;
    let gone_id = gone.id().document_id();
    test.insert_account(gone);
    let reader = test.account(READER).id().document_id();
    let second = test.account(SECOND).id().document_id();
    for (name, id) in [(GONE, gone_id), (READER, reader), (SECOND, second)] {
        super::pinning::pin(
            USER,
            name,
            &vault::keys::display_fingerprint(
                &test.server.za_served_public_key(id).await.unwrap().unwrap(),
            ),
        )
        .await
        .expect(200);
    }
    let client = DummyWebDavClient::new(owner_id, USER, STRONG, USER);
    let path = "/dav/cal/share-destroy-owner%40example.com/destroy/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let request = |name: &str| json!({"username":USER,"password":STRONG,"calendar":"destroy","address":name,"level":"view"});
    for name in [READER, GONE] {
        za_post("shares/grant", &request(name)).await.expect(200);
    }
    let id = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("destroy")
        .unwrap()
        .document_id();
    let keys = za_unlock(test, owner_id, STRONG).await;
    let before = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let old_generations = before.domain.domain_id().copied().unwrap();
    let gone = test.accounts.remove(GONE).unwrap();
    test.account("admin@example.com")
        .destroy_account(gone)
        .await;
    test.wait_for_tasks().await;
    let listed = za_post("shares", &json!({"username":USER,"password":STRONG}))
        .await
        .expect(200);
    let calendar = listed["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["calendar"] == "destroy")
        .unwrap();
    assert_eq!(
        calendar["grants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["grant"] == gone_id)
            .unwrap()["state"],
        "gone"
    );
    test.reload_core();
    let previous_max = test.server.core.groupware.max_shares_per_item;
    test.account("admin@example.com")
        .registry_update_setting(
            registry::schema::structs::Sharing {
                max_shares: 2,
                ..Default::default()
            },
            &[registry::schema::prelude::Property::MaxShares],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    let before_sweep = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .version;
    // Apply is authoritative even if a caller's earlier precheck admitted a
    // different ledger. The current full ledger still includes the gone id.
    assert_eq!(test.server.core.groupware.max_shares_per_item, 2);
    let fresh_keys = za_unlock(test, owner_id, STRONG).await;
    let fresh_context = za_owned_calendar_context(&test.server, &fresh_keys, id)
        .await
        .unwrap();
    let fresh_calendar = fresh_context.stored.deserialize::<Calendar>().unwrap();
    let fresh_owner = fresh_calendar
        .preferences
        .iter()
        .find(|p| p.account_id == owner_id)
        .unwrap();
    let fresh_ring = groupware::calendar::sharing::format::open_owner_ring(
        &STANDARD
            .decode(
                fresh_owner
                    .name
                    .strip_prefix("$za$")
                    .unwrap()
                    .split('|')
                    .next()
                    .unwrap(),
            )
            .unwrap(),
        owner_id,
        id,
        fresh_keys.ewk(),
    )
    .unwrap()
    .1;
    assert_eq!(
        fresh_ring.grants.len(),
        test.server.core.groupware.max_shares_per_item
    );
    assert!(fresh_ring.grants.iter().any(|g| g.account_id == gone_id));
    assert!(!fresh_ring.grants.iter().any(|g| g.account_id == second));
    let vault = test
        .server
        .za_vault_record(owner_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(vault.record.revision, fresh_keys.generation);
    assert_eq!(vault.record.state, vault::record::VaultState::Active);
    assert!(
        groupware::calendar::sharing::mutations::za_share_key_account(&test.server, owner_id)
            .await
            .unwrap()
    );
    assert!(
        groupware::calendar::sharing::mutations::za_share_key_account(&test.server, second)
            .await
            .unwrap()
    );
    let public_key = test
        .server
        .za_served_public_key(second)
        .await
        .unwrap()
        .unwrap();
    let pins = test.server.za_pin_read(&fresh_keys).await.unwrap();
    assert_eq!(
        pins.record
            .as_ref()
            .unwrap()
            .pin(SECOND)
            .unwrap()
            .public_key,
        public_key
    );
    let guards = groupware::calendar::sharing::convert::ZaShareGuards {
        owner_id,
        verified_generation: fresh_keys.generation,
        vault_cas: vault.cas,
        pin_assertion: Some(pins.assertion()),
    };
    let direct = groupware::calendar::sharing::mutations::za_share_apply(
        &test.server,
        &fresh_keys,
        id,
        groupware::calendar::sharing::mutations::ZaShareChange::Grant {
            account_id: second,
            address: SECOND.into(),
            public_key,
            level: groupware::calendar::sharing::mutations::ZaShareLevel::View,
        },
        &guards,
    )
    .await;
    match direct {
        Err(groupware::calendar::sharing::convert::ZaShareError::TooManyShares) => {}
        Err(error) => panic!("direct full-ledger apply expected TooManyShares, got {error:?}"),
        Ok(applied) => panic!(
            "direct full-ledger apply expected TooManyShares, got success changed={}",
            applied.changed
        ),
    }
    assert_eq!(
        za_post("shares/grant", &request(SECOND)).await.expect(409)["error"],
        "too many shares"
    );
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored
            .version,
        before_sweep
    );
    assert_eq!(
        za_post(
            "shares/revoke",
            &json!({"username":USER,"password":STRONG,"calendar":"destroy","grant":u32::MAX})
        )
        .await
        .expect(200)["changed"],
        true,
        "missing selector still sweeps gone grantees"
    );
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let calendar = context.stored.deserialize::<Calendar>().unwrap();
    assert!(!calendar.acls.iter().any(|g| g.account_id == gone_id));
    let owner = calendar
        .preferences
        .iter()
        .find(|p| p.account_id == owner_id)
        .unwrap();
    let bytes = STANDARD
        .decode(
            owner
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    let header = groupware::calendar::sharing::format::parse_owner_header(&bytes).unwrap();
    assert_eq!(header.generations, vec![1, 2]);
    assert_eq!(header.domain_id, old_generations);
    // The ids-only notification under the destroyed principal is documented;
    // verify it and remove precisely that fixture's notification range.
    let from = LogKey {
        account_id: gone_id,
        collection: SyncCollection::ShareNotification.into(),
        change_id: 0,
    };
    let to = LogKey {
        account_id: gone_id,
        collection: SyncCollection::ShareNotification.into(),
        change_id: u64::MAX,
    };
    let mut notifications = 0;
    test.server
        .store()
        .iterate(IterateParams::new(from, to).ascending(), |_, value| {
            let notification =
                common::sharing::notification::ShareNotification::deserialize(value)?;
            assert_eq!(notification.object_account_id, owner_id);
            assert!(notification.name.is_empty());
            notifications += 1;
            Ok(true)
        })
        .await
        .unwrap();
    assert_eq!(notifications, 1);
    test.server.store().delete_range(from, to).await.unwrap();
    za_post("shares/grant", &request(SECOND)).await.expect(200);
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"destroy","grant":second}),
    )
    .await
    .expect(200);
    test.account("admin@example.com")
        .registry_update_setting(
            registry::schema::structs::Sharing {
                max_shares: previous_max as u64,
                ..Default::default()
            },
            &[registry::schema::prelude::Property::MaxShares],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    // Actual registry destruction during the authoritative grant decision.
    let parked = HookParked::start(
        owner_id,
        set_share_commit_pause,
        share_post("shares/grant", request(SECOND)),
    )
    .await;
    let owner = test.accounts.remove(USER).unwrap();
    test.account("admin@example.com")
        .registry_destroy(ObjectType::Account, [owner.id()])
        .await
        .assert_destroyed(&[owner.id()]);
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "account deleted"
    );
    test.wait_for_tasks().await;
    for grantee in [reader, second] {
        assert!(
            !test
                .server
                .store()
                .acl_query(AclQuery::HasAccess {
                    grant_account_id: grantee
                })
                .await
                .unwrap()
                .iter()
                .any(|acl| acl.to_account_id == owner_id)
        );
    }
    let client = DummyWebDavClient::new(reader, READER, STRONG, READER);
    let assisted_before = test.server.core.groupware.assisted_discovery;
    for enabled in [false, true] {
        test.account("admin@example.com")
            .registry_update_setting(
                registry::schema::structs::WebDav {
                    enable_assisted_discovery: enabled,
                    ..Default::default()
                },
                &[registry::schema::prelude::Property::EnableAssistedDiscovery],
            )
            .await;
        test.account("admin@example.com").reload_settings().await;
        test.reload_core();
        for path in [
            "/dav/cal/shares-http-reader%40example.com/",
            "/dav/cal/",
            "/dav/pal/shares-http-reader%40example.com/",
        ] {
            let response = client
                .request_with_headers("PROPFIND", path, [("depth", "1")], "")
                .await;
            assert_eq!(response.status, StatusCode::MULTI_STATUS);
            assert!(!response.body.unwrap().contains("share-destroy-owner"));
        }
    }
    test.account("admin@example.com")
        .registry_update_setting(
            registry::schema::structs::WebDav {
                enable_assisted_discovery: assisted_before,
                ..Default::default()
            },
            &[registry::schema::prelude::Property::EnableAssistedDiscovery],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    client
        .request(
            "PROPFIND",
            "/dav/cal/share-destroy-owner%40example.com/destroy/",
            "",
        )
        .await
        .with_status(StatusCode::NOT_FOUND);
}

async fn sharing_boundary_error_contract(
    test: &TestServer,
    owner: u32,
    reader: u32,
    client: &DummyWebDavClient,
) {
    use common::za::test_hooks::set_share_boundary_pause;
    use serde_json::json;
    const USER: &str = "share-http-races@example.com";
    let path = "/dav/cal/share-http-races%40example.com/boundary-error/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let id = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("boundary-error")
        .unwrap()
        .document_id();
    let reader_client = DummyWebDavClient::new(
        reader,
        "shares-http-reader@example.com",
        STRONG,
        "shares-http-reader@example.com",
    );
    reader_client
        .request("PROPFIND", "/dav/cal/shares-http-reader%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(test.server.inner.cache.za_keys.contains_account(reader));
    let body = json!({"username":USER,"password":STRONG,"calendar":"boundary-error","address":"shares-http-reader@example.com","level":"view"});
    let parked = HookParked::start(
        owner,
        set_share_boundary_pause,
        share_post("shares/grant", body.clone()),
    )
    .await;
    assert!(
        !test.server.inner.cache.za_keys.contains_account(reader),
        "commit invalidation precedes boundary completion"
    );
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let original = context.stored.deserialize::<Calendar>().unwrap();
    let mut corrupted = original.clone();
    corrupted
        .preferences
        .iter_mut()
        .find(|p| p.account_id == owner)
        .unwrap()
        .name = "broken boundary fixture".into();
    let mut batch = BatchBuilder::new();
    corrupted
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            context.stored.to_unarchived::<Calendar>().unwrap(),
            owner,
            id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    assert_eq!(parked.finish().await.status, 500);
    assert!(
        !test.server.inner.cache.za_keys.contains_account(reader),
        "a later boundary error must retain post-commit eviction"
    );
    let current = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(owner, Collection::Calendar, id))
        .await
        .unwrap()
        .unwrap();
    let mut batch = BatchBuilder::new();
    original
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            current.to_unarchived::<Calendar>().unwrap(),
            owner,
            id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    crate::utils::za::za_post("shares/grant", &body)
        .await
        .expect(200);
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn sharing_count_delta_contract(test: &TestServer, owner: u32, client: &DummyWebDavClient) {
    use crate::utils::za::{plant_sealed_event, za_post};
    use common::za::test_hooks::*;
    use serde_json::json;
    const USER: &str = "share-http-races@example.com";
    let path = "/dav/cal/share-http-races%40example.com/count-deltas/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let id = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("count-deltas")
        .unwrap()
        .document_id();
    let mut events = Vec::new();
    for index in 0..2 {
        events.push(
            plant_sealed_event(
                test,
                owner,
                "count-deltas",
                &format!("{index}.ics"),
                &super::sharing::sharing_event(&format!("count-delta-{index}")),
                STRONG,
                &Default::default(),
            )
            .await,
        );
    }
    super::sharing::stage_converting_calendar(test, owner, id).await;
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    for event in &events {
        let stored = event_archive(test, owner, *event).await;
        let mut converted = stored.deserialize::<CalendarEvent>().unwrap();
        rewrap_event(&mut converted, &context.domain, &context.domain).unwrap();
        save_event(test, owner, *event, &stored, converted).await;
    }
    client
        .request("GET", &format!("{path}0.ics"), "")
        .await
        .with_status(StatusCode::OK);
    let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
    let entry = calendar
        .preferences
        .iter_mut()
        .find(|p| p.account_id == owner)
        .unwrap();
    let mut parts = entry
        .name
        .strip_prefix("$za$")
        .unwrap()
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (header, mut ring) = groupware::calendar::sharing::format::open_owner_ring(
        &STANDARD.decode(&parts[0]).unwrap(),
        owner,
        id,
        keys.ewk(),
    )
    .unwrap();
    ring.reverting = true;
    parts[0] = STANDARD.encode(seal_owner_ring(&header, &ring, owner, keys.ewk()).unwrap());
    entry.name = format!("$za${}", parts.join("|"));
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            context.stored.to_unarchived::<Calendar>().unwrap(),
            owner,
            id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    set_share_restore_assert_fail(Some(ShareAssertFailure {
        owner_id: owner,
        calendar_id: id,
        remaining: 1,
    }));
    let result=za_post("shares/grant",&json!({"username":USER,"password":STRONG,"calendar":"count-deltas","address":"shares-http-reader@example.com","level":"view"})).await.expect(200);
    assert_eq!(
        result["rewrapped"], 4,
        "two acknowledged rollback rewrites plus two forward rewrites, each delta once"
    );
    assert_eq!(result["sealed"], 0);
    set_share_restore_assert_fail(None);
    let fresh = za_owned_calendar_context(&test.server, &*za_unlock(test, owner, STRONG).await, id)
        .await
        .unwrap();
    assert_ne!(fresh.domain.domain_id(), context.domain.domain_id());
    for (index, event) in events.iter().enumerate() {
        client
            .request("GET", &format!("{path}{index}.ics"), "")
            .await
            .with_status(StatusCode::OK);
        let mut stored = event_archive(test, owner, *event)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        unseal_event_in_domain(&mut stored, &fresh.domain).unwrap();
        assert!(
            stored
                .data
                .event
                .to_string()
                .contains("share-read-private-summary")
        );
    }
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn sharing_event_terminal_contract(
    test: &TestServer,
    owner: u32,
    client: &DummyWebDavClient,
) {
    use crate::utils::za::{plant_event, za_post};
    use common::za::test_hooks::set_share_rewrap_pause;
    use serde_json::json;
    const USER: &str = "share-http-races@example.com";
    let path = "/dav/cal/share-http-races%40example.com/terminal-event/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let id = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("terminal-event")
        .unwrap()
        .document_id();
    let mut last = 0;
    for index in 0..101 {
        last = plant_event(
            test,
            owner,
            "terminal-event",
            &format!("{index}.ics"),
            &super::sharing::sharing_event(&format!("terminal-event-{index}")),
            None,
        )
        .await;
    }
    super::sharing::stage_converting_calendar(test, owner, id).await;
    let request = json!({"username":USER,"password":STRONG,"calendar":"terminal-event","address":"shares-http-reader@example.com","level":"view"});
    let parked = HookParked::start(
        owner,
        set_share_rewrap_pause,
        share_post("shares/grant", request.clone()),
    )
    .await;
    let stored = event_archive(test, owner, last).await;
    let mut changed = stored.deserialize::<CalendarEvent>().unwrap();
    changed.modified += 1;
    save_event(test, owner, last, &stored, changed).await;
    let old = test.server.za_vault_record(owner).await.unwrap().unwrap();
    let mut revised = old.record.clone();
    revised.revision += 1;
    test.server
        .za_vault_write(owner, &revised, Some(&old))
        .await
        .unwrap();
    let (class, bytes) = super::sharing::conversion_remove_account(test, owner).await;
    assert_eq!(
        parked.finish().await.expect(409)["error"],
        "account deleted"
    );
    let mut batch = BatchBuilder::new();
    batch.set(class, bytes);
    test.server.store().write(batch.build_all()).await.unwrap();
    assert_eq!(
        za_post("shares/grant", &request).await.expect(200)["sealed"],
        1
    );
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn sharing_changed_attribution_contract(
    test: &TestServer,
    owner: u32,
    client: &DummyWebDavClient,
) {
    use crate::utils::za::za_post;
    use common::za::test_hooks::*;
    use serde_json::json;
    for acknowledged in [false, true] {
        let slug = if acknowledged {
            "changed-after-phase1"
        } else {
            "changed-before-phase1"
        };
        let path = format!("/dav/cal/share-http-races%40example.com/{slug}/");
        client
            .request("MKCALENDAR", &path, MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        let id = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(slug)
            .unwrap()
            .document_id();
        let keys = za_unlock(test, owner, STRONG).await;
        let before = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let request = json!({"username":"share-http-races@example.com","password":STRONG,"calendar":slug,"address":"shares-http-reader@example.com","level":"view"});
        let setter: fn(Option<std::sync::Arc<Pause>>) = if acknowledged {
            set_share_rewrap_pause
        } else {
            set_share_precheck_pause
        };
        let parked =
            HookParked::start(owner, setter, share_post("shares/grant", request.clone())).await;
        let paused = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        assert_eq!(paused.domain.calendar_id().is_some(), acknowledged);
        assert_eq!(paused.stored.version != before.stored.version, acknowledged);
        assert!(
            paused
                .stored
                .deserialize::<Calendar>()
                .unwrap()
                .acls
                .is_empty()
        );
        let winner = za_post("shares/grant", &request).await.expect(200);
        assert_eq!(winner["changed"], true);
        assert_eq!(winner["sealed"], 0);
        assert_eq!(winner["rewrapped"], 0);
        let loser = parked.finish().await.expect(200);
        assert_eq!(loser["sealed"], 0);
        assert_eq!(loser["rewrapped"], 0);
        assert_eq!(
            loser["changed"], acknowledged,
            "changed reflects this request's acknowledged phase1 write, not the eventual apply no-op"
        );
        client
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

async fn sharing_dav_copy_after_precheck_contract(
    test: &TestServer,
    owner: u32,
    client: &DummyWebDavClient,
) {
    use crate::utils::za::{plant_sealed_event, za_post};
    use common::za::test_hooks::*;
    use serde_json::json;
    const USER: &str = "share-http-races@example.com";
    const HOME: &str = "/dav/cal/share-http-races%40example.com/";
    for slug in ["copy-after-precheck", "copy-second-calendar"] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    let event = plant_sealed_event(
        test,
        owner,
        "copy-after-precheck",
        "old.ics",
        &super::sharing::sharing_event("copy-after-precheck-old"),
        STRONG,
        &Default::default(),
    )
    .await;
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let source = resources
        .by_path("copy-after-precheck")
        .unwrap()
        .document_id();
    let destination = resources
        .by_path("copy-second-calendar")
        .unwrap()
        .document_id();
    let original = event_archive(test, owner, event).await;
    let before = std::sync::Arc::new(Pause::new(owner));
    let after = std::sync::Arc::new(Pause::new(owner));
    set_share_precheck_pause(Some(before.clone()));
    set_share_rewrap_pause(Some(after.clone()));
    let request = json!({"username":USER,"password":STRONG,"calendar":"copy-after-precheck","address":"shares-http-reader@example.com","level":"view"});
    let pending = tokio::spawn(share_post("shares/grant", request.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(5), before.arrived.notified())
        .await
        .unwrap();
    // A separate authenticated DAV client commits the real COPY before phase1.
    let other_session = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let second_path = format!("{HOME}copy-second-calendar/copied.ics");
    other_session
        .request_with_headers(
            "COPY",
            &format!("{HOME}copy-after-precheck/old.ics"),
            [("destination", second_path.as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    other_session
        .request("GET", &second_path, "")
        .await
        .with_status(StatusCode::OK);
    let committed = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    assert_eq!(
        committed
            .by_path("copy-second-calendar/copied.ics")
            .unwrap()
            .document_id(),
        event
    );
    let linked = event_archive(test, owner, event).await;
    assert_ne!(linked.version, original.version);
    let names = linked.deserialize::<CalendarEvent>().unwrap().names;
    assert_eq!(names.len(), 2);
    assert!(names.iter().any(|n| n.parent_id == source));
    assert!(names.iter().any(|n| n.parent_id == destination));
    before.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), after.arrived.notified())
        .await
        .unwrap();
    let keys = za_unlock(test, owner, STRONG).await;
    let staged = za_owned_calendar_context(&test.server, &keys, source)
        .await
        .unwrap();
    assert!(staged.domain.calendar_id().is_some());
    other_session
        .request_with_headers(
            "PUT",
            &format!("{HOME}copy-after-precheck/new.ics"),
            [("content-type", "text/calendar")],
            super::sharing::sharing_event("copy-after-precheck-new"),
        )
        .await
        .with_status(StatusCode::CREATED);
    after.release.notify_one();
    assert_eq!(
        pending.await.unwrap().expect(409)["error"],
        "calendar has linked events"
    );
    let rolled_back = za_owned_calendar_context(&test.server, &keys, source)
        .await
        .unwrap();
    assert!(rolled_back.domain.calendar_id().is_none());
    let calendar = rolled_back.stored.deserialize::<Calendar>().unwrap();
    assert!(calendar.acls.is_empty());
    assert_eq!(calendar.preferences.len(), 1);
    for path in [
        format!("{HOME}copy-after-precheck/old.ics"),
        second_path.clone(),
        format!("{HOME}copy-after-precheck/new.ics"),
    ] {
        other_session
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK);
    }
    other_session
        .request_with_headers(
            "PUT",
            &format!("{HOME}copy-after-precheck/old.ics"),
            [("content-type", "text/calendar")],
            super::sharing::sharing_event("copy-after-precheck-old"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    other_session
        .request("GET", &second_path, "")
        .await
        .with_status(StatusCode::OK);
    other_session
        .request("DELETE", &second_path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        za_post("shares/grant", &request).await.expect(200),
        json!({"ok":true,"changed":true,"rewrapped":2,"sealed":0})
    );
    let fresh = za_owned_calendar_context(&test.server, &keys, source)
        .await
        .unwrap();
    assert_ne!(fresh.domain.domain_id(), staged.domain.domain_id());
    other_session
        .request("GET", &format!("{HOME}copy-after-precheck/new.ics"), "")
        .await
        .with_status(StatusCode::OK);
    let reader = test
        .account("shares-http-reader@example.com")
        .id()
        .document_id();
    let reader_client = DummyWebDavClient::new(
        reader,
        "shares-http-reader@example.com",
        STRONG,
        "shares-http-reader@example.com",
    );
    for name in ["old.ics", "new.ics"] {
        reader_client
            .request("GET", &format!("{HOME}copy-after-precheck/{name}"), "")
            .await
            .with_status(StatusCode::OK);
    }
    for slug in ["copy-after-precheck", "copy-second-calendar"] {
        client
            .request("DELETE", &format!("{HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

/// Real Bob HTTP writes held only after their foreign calendar gate has admitted them.
/// Keep original event CAS binding while testing calendar-only rotations and revocations.
async fn grantee_http_write_races(test: &mut TestServer) {
    use crate::utils::za::{sharing_dav_request, za_post};
    use serde_json::json;
    use vault::keys::display_fingerprint;
    const USER: &str = "sharing-http-race-owner@example.com";
    const BOB: &str = "sharing-http-race-bob@example.com";
    const CAROL: &str = "sharing-http-race-carol@example.com";
    const HOME: &str = "/dav/cal/sharing-http-race-owner%40example.com/";
    const PATH: &str = "/dav/cal/sharing-http-race-owner%40example.com/shared/";
    let admin = test.account("admin@example.com").clone();
    for name in [USER, BOB, CAROL] {
        let account = admin
            .create_key_user_account(name, STRONG, name, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    let owner = test.account(USER).id().document_id();
    let bob = test.account(BOB).id().document_id();
    let carol = test.account(CAROL).id().document_id();
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    client
        .request("MKCALENDAR", PATH, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    for (name, id) in [(BOB, bob), (CAROL, carol)] {
        let key = test.server.za_served_public_key(id).await.unwrap().unwrap();
        super::pinning::pin(USER, name, &display_fingerprint(&key))
            .await
            .expect(200);
    }
    let grant = |name| json!({"username":USER,"password":STRONG,"calendar":"shared","address":name,"level":"edit"});
    za_post("shares/grant", &grant(BOB)).await.expect(200);
    let keys = za_unlock(test, owner, STRONG).await;
    let calendar = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("shared")
        .unwrap()
        .document_id();
    for revoked in [false, true] {
        for operation in ["CREATE", "UPDATE", "DELETE"] {
            za_post("shares/grant", &grant(BOB)).await.expect(200);
            za_post("shares/grant", &grant(CAROL)).await.expect(200);
            let name = format!(
                "{}-{}.ics",
                if revoked { "revoked" } else { "rotated" },
                operation.to_ascii_lowercase()
            );
            let path = format!("{PATH}{name}");
            let initial = super::sharing::sharing_event(&format!("grantee-{name}"));
            if operation != "CREATE" {
                client
                    .request_with_headers(
                        "PUT",
                        &path,
                        [("content-type", "text/calendar")],
                        &initial,
                    )
                    .await
                    .with_status(StatusCode::CREATED);
            }
            // A fresh foreign GET/PROPFIND proves admission before parking any downstream hook.
            DummyWebDavClient::new(bob, BOB, STRONG, BOB)
                .request("PROPFIND", PATH, "")
                .await
                .with_status(StatusCode::MULTI_STATUS);
            let event_id = if operation != "CREATE" {
                Some(
                    test.server
                        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
                        .await
                        .unwrap()
                        .by_path(&format!("shared/{name}"))
                        .unwrap()
                        .document_id(),
                )
            } else {
                None
            };
            let original = if let Some(id) = event_id {
                Some(event_archive(test, owner, id).await)
            } else {
                None
            };
            let updating_path = path.clone();
            let body = initial.replace("SUMMARY:", "SUMMARY:Bob race ");
            let method = if operation == "DELETE" {
                "DELETE"
            } else {
                "PUT"
            };
            let hook = match operation {
                "CREATE" => test_hooks::set_commit_pause,
                "UPDATE" => test_hooks::set_update_commit_pause,
                _ => test_hooks::set_delete_commit_pause,
            };
            let parked = HookParked::start(owner, hook, async move {
                sharing_dav_request(
                    BOB,
                    method,
                    &updating_path,
                    &[("content-type", "text/calendar")],
                    if method == "PUT" { &body } else { "" },
                )
                .await
            })
            .await;
            let before = za_owned_calendar_context(&test.server, &keys, calendar)
                .await
                .unwrap();
            let generation_before = current_ring_generation(&before);
            za_post("shares/revoke",&json!({"username":USER,"password":STRONG,"calendar":"shared","grant":if revoked {bob} else {carol}})).await.expect(200);
            let after = za_owned_calendar_context(&test.server, &keys, calendar)
                .await
                .unwrap();
            assert!(current_ring_generation(&after) > generation_before);
            let response = parked.finish().await;
            if revoked {
                assert_eq!(
                    response.status,
                    StatusCode::FORBIDDEN,
                    "revoked Bob {operation}: {}",
                    response.body
                );
                if let (Some(id), Some(original)) = (event_id, original.as_ref()) {
                    assert_eq!(
                        event_archive(test, owner, id).await.as_bytes(),
                        original.as_bytes()
                    );
                } else {
                    client
                        .request("GET", &path, "")
                        .await
                        .with_status(StatusCode::NOT_FOUND);
                }
            } else {
                assert_eq!(
                    response.status,
                    if operation == "CREATE" {
                        StatusCode::CREATED
                    } else {
                        StatusCode::NO_CONTENT
                    },
                    "rotated Bob {operation}: {}",
                    response.body
                );
                if operation != "DELETE" {
                    let resources = test
                        .server
                        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
                        .await
                        .unwrap();
                    let id = resources
                        .by_path(&format!("shared/{name}"))
                        .unwrap()
                        .document_id();
                    let stored = event_archive(test, owner, id).await;
                    let mut opened = stored.deserialize::<CalendarEvent>().unwrap();
                    assert_eq!(generation(&opened), current_ring_generation(&after));
                    unseal_event_in_domain(&mut opened, &after.domain).unwrap();
                    DummyWebDavClient::new(bob, BOB, STRONG, BOB)
                        .request("GET", &path, "")
                        .await
                        .with_status(StatusCode::OK);
                }
            }
            if operation != "DELETE" || revoked {
                let _ = client.request("DELETE", &path, "").await;
            }
        }
    }
    // Calendar PROPPATCH keeps an immediate409 instead of the event one-retry path.
    za_post("shares/grant", &grant(BOB)).await.expect(200);
    za_post("shares/grant", &grant(CAROL)).await.expect(200);
    let parked=HookParked::start(owner,test_hooks::set_update_commit_pause,async {
        sharing_dav_request(BOB,"PROPPATCH",PATH,&[],"<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>must-conflict</D:displayname></D:prop></D:set></D:propertyupdate>").await
    }).await;
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"shared","grant":carol}),
    )
    .await
    .expect(200);
    assert_eq!(parked.finish().await.status, StatusCode::CONFLICT);
    let body = DummyWebDavClient::new(bob, BOB, STRONG, BOB)
        .request("PROPFIND", PATH, "")
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(!body.contains("must-conflict"));
    client
        .request("DELETE", PATH, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", &format!("{HOME}default/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for name in [BOB, CAROL] {
        DummyWebDavClient::new(test.account(name).id().document_id(), name, STRONG, name)
            .delete_default_containers()
            .await;
    }
    test.wait_for_tasks().await;
}

fn current_ring_generation(
    context: &groupware::calendar::sharing::resolve::ZaCalendarContext,
) -> u32 {
    let calendar = context.stored.deserialize::<Calendar>().unwrap();
    let entry = calendar
        .preferences
        .iter()
        .find(|entry| entry.account_id == context.owner_id)
        .unwrap();
    let bytes = STANDARD
        .decode(
            entry
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    *groupware::calendar::sharing::format::parse_owner_header(&bytes)
        .unwrap()
        .generations
        .last()
        .unwrap()
}
