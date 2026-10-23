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
