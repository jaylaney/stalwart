/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{Pair, event_ics, finish, make_calendar, pair, put_event, session_token, share};
use crate::{
    utils::{
        http::HttpRequest,
        server::TestServer,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{
            HookParked, assert_no_event_preferences, delete_inbox_item, inbox_ids, item_attendees,
            item_method, item_sequence, itip_tasks_for, new_inbox_items, open_pending_item_as,
            plant_sealed_event, shared_container, stored_event, wait_for_queue, za_post,
            za_uid_leases, za_unlock,
        },
    },
    za::{
        STRONG,
        dav_seal::{
            DEAD_CANARIES, assert_dead_container_sealed, assert_dead_round_trip, dead_proppatch,
            resource_id,
        },
        fanout::{CONTENT_TYPE, NO_REPLY, uid_copies},
        mail::Traced,
        uid_lease::put,
    },
};
use calcard::{
    common::timezone::Tz,
    icalendar::{ICalendar, ICalendarMethod},
};
use common::{
    DavName, Server,
    auth::{AccessToken, vault::RawValue},
    storage::metadata::ContainerCleanup,
    za::{
        lease::ZaUidLease,
        test_hooks::{
            ForceCommitSplit, set_commit_pause, set_delete_commit_pause, set_lease_secs_override,
            set_second_chunk_fail_once, set_update_commit_pause,
        },
    },
};
use dav_proto::Depth;
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData,
        alarm::AlarmTarget,
        alerts::{DefaultAlerts, DefaultAlertsResolver},
        pipeline::{
            ZaDeleted, ZaScheduling, ZaWritten,
            admission::{za_roles, za_writer_keys},
            context::{ZaReadMemo, za_read_event_view, za_write_contexts},
            create::{ZaEventCreate, za_create_event},
            delete::{za_calendar_delete_target, za_delete_calendar, za_destroy_event},
            error::{ZaBusy, ZaPipelineError, ZaRefusal},
            placement::{ZaPlacementContexts, ZaPlacementPlan, za_placement_contexts},
            schedule::{ZaScheduled, za_delete_send_itip, za_schedule_create},
            uid::za_take_uid_lease,
            update::{ZaEventUpdate, za_commit_update},
        },
        schedule::{EventAlarmScheduler, EventAlarmUsers},
        seal::{TokenPairs, ZaKeyDomain, container::container_is_ck, unseal_event_in_domain},
        sequence_floor::za_sequence_floor,
        sharing::resolve::za_owned_calendar_context,
    },
};
use hyper::{Method, StatusCode};
use registry::schema::{
    enums::{Permission, TaskType},
    structs::{Task, TaskStatus},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use store::{
    ValueKey,
    query::log::Query,
    write::{Archive, ArchiveBytes, TaskId, TaskQueueClass, ValueClass, now},
};
use trc::{AddContext, EventType, SecurityEvent};
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection, VanishedCollection},
    field::CalendarEventField,
};

const HOME: &str = "/dav/cal/jmapcal-pipeline%40example.com/";
const OUTSIDE: &str = "jmapcal-pipeline-att@remote.test";

/// The module's pair and the ids of `pipe-a`, `pipe-b` and `pipe-s`.
struct Fx {
    pair: Pair,
    a: u32,
    b: u32,
    s: u32,
}

impl Fx {
    async fn new(test: &mut TestServer) -> Self {
        let pair = pair(test, "pipeline").await;
        let a = make_calendar(test, &pair.key, &pair.dav, "pipe-a").await;
        let b = make_calendar(test, &pair.key, &pair.dav, "pipe-b").await;
        let s = make_calendar(test, &pair.key, &pair.dav, "pipe-s").await;
        share(test, &pair.key, &pair.key2, pair.key2_id, "pipe-s", "edit").await;
        Self { pair, a, b, s }
    }

    async fn cleanup(&self, test: &TestServer) {
        for slug in ["pipe-a", "pipe-b", "pipe-s"] {
            self.pair
                .dav
                .request("DELETE", &format!("{HOME}{slug}/"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
        let grantee = self.pair.key2.name();
        let client = DummyWebDavClient::new(self.pair.key2_id, grantee, STRONG, grantee);
        for id in inbox_ids(test, self.pair.key2_id).await {
            delete_inbox_item(test, &client, self.pair.key2_id, id).await;
        }
    }
}

/// The stages a key account's create runs, in CalDAV order, with one name
/// per calendar.
async fn create(
    server: &Server,
    token: &AccessToken,
    owner: u32,
    calendars: &[u32],
    ics: &str,
    schedule: ZaScheduling,
) -> Result<ZaWritten, ZaPipelineError> {
    let resources = server
        .fetch_groupware_resources(token.account_id(), owner, SyncCollection::Calendar)
        .await?;
    let ical = ICalendar::parse(ics).expect("valid pipeline fixture");
    let uid = ical
        .components
        .iter()
        .find_map(|component| component.uid())
        .expect("pipeline fixture UID")
        .to_string();
    let keys = za_writer_keys(server, token, owner)
        .await?
        .expect("pipeline fixture is a key account");
    let roles = za_roles(server, token, owner).await?;
    let lease = za_take_uid_lease(server, &resources, token, owner, calendars, &uid).await?;

    let mut event = CalendarEvent {
        names: calendars
            .iter()
            .map(|&parent_id| DavName {
                name: format!("{uid}.ics"),
                parent_id,
            })
            .collect(),
        ..Default::default()
    };
    let mut content = CalendarEventContent {
        data: CalendarEventData::new_for(
            ical,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &DefaultAlerts::disabled(),
            true,
        ),
        preferences: Vec::new(),
    };

    let prepared = async {
        let contexts =
            za_write_contexts(server, token, owner, &event, calendars[0], Acl::AddItems).await?;
        let next_alarms = server
            .next_event_alarms(
                owner,
                &EventAlarmUsers::new(owner, &content)?,
                &content.data,
                calendars,
                now() as i64,
                &mut DefaultAlertsResolver::with_resources(owner, resources.clone()),
            )
            .await?;
        let scheduled = za_schedule_create(
            server,
            token,
            &roles,
            schedule,
            &mut event,
            &mut content,
            owner,
        )
        .await?;
        let account = server.account(owner).await?;
        Ok((contexts, next_alarms, scheduled, account))
    }
    .await;
    let (contexts, next_alarms, scheduled, account) = match prepared {
        Ok(prepared) => prepared,
        Err(err) => {
            lease.release().await;
            return Err(err);
        }
    };

    za_create_event(
        server,
        token,
        &resources,
        &account,
        &keys,
        &roles,
        ZaEventCreate {
            event,
            content,
            next_alarms,
            scheduled,
            lease,
            uid,
            contexts,
        },
    )
    .await
}

/// The stages a key account's update runs when its calendars or content change, in process.
async fn update(
    server: &Server,
    token: &AccessToken,
    owner: u32,
    document_id: u32,
    calendars: &[u32],
    ics: Option<&str>,
) -> Result<ZaWritten, ZaPipelineError> {
    let resources = server
        .fetch_groupware_resources(token.account_id(), owner, SyncCollection::Calendar)
        .await?;
    let stored_event: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::archive(
            owner,
            Collection::CalendarEvent,
            document_id,
        ))
        .await?
        .ok_or(ZaPipelineError::NotFound)?;
    let stored_content: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::property(
            owner,
            Collection::CalendarEvent,
            document_id,
            CalendarEventField::Content,
        ))
        .await?
        .ok_or(ZaPipelineError::NotFound)?;
    let stored = stored_event
        .unarchive::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut new_event = stored_event
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let source = &new_event.names[0];
    let view = za_read_event_view(
        server,
        token,
        owner,
        document_id,
        source.parent_id,
        stored,
        &stored_content,
        &mut ZaReadMemo::new(),
    )
    .await??
    .expect("pipeline fixture is a key account");
    let mut new_content = view
        .content
        .deserialize::<CalendarEventContent>()
        .caused_by(trc::location!())?;
    let old = &new_content.data.event;
    let mut new_names: Vec<_> = new_event
        .names
        .iter()
        .filter(|name| calendars.contains(&name.parent_id))
        .cloned()
        .collect();
    for &parent_id in calendars {
        if !new_names.iter().any(|name| name.parent_id == parent_id) {
            new_names.push(DavName {
                name: source.name.clone(),
                parent_id,
            });
        }
    }
    let plan = ZaPlacementPlan::jmap(&new_event.names, &new_names, ics.is_some());
    let contexts = if plan.is_change() {
        za_placement_contexts(server, token, owner, &plan).await?
    } else {
        ZaPlacementContexts::unchanged(
            za_write_contexts(
                server,
                token,
                owner,
                &new_event,
                source.parent_id,
                Acl::ModifyItems,
            )
            .await?,
        )
    };
    let keys = za_writer_keys(server, token, owner)
        .await?
        .expect("pipeline fixture is a key account");
    let roles = za_roles(server, token, owner).await?;
    let next_data = ics.map(|ics| {
        let mut new = ICalendar::parse(ics).expect("valid pipeline fixture");
        za_sequence_floor(&mut new, old, roles.sender.addresses());
        CalendarEventData::new_for(
            new,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &DefaultAlerts::disabled(),
            true,
        )
    });
    let timestamp = now() as i64;
    let mut alerts = DefaultAlertsResolver::with_resources(owner, resources.clone());
    let old_parents: Vec<_> = new_event.names.iter().map(|name| name.parent_id).collect();
    let prev_alarms = server
        .next_event_alarms(
            owner,
            &EventAlarmUsers::new(owner, &new_content)?.with_event_flags(new_event.flags),
            &new_content.data,
            &old_parents,
            timestamp,
            &mut alerts,
        )
        .await?;
    if let Some(data) = next_data {
        new_content.data = data;
    }
    let next_alarms = server
        .next_event_alarms(
            owner,
            &EventAlarmUsers::new(owner, &new_content)?.with_event_flags(new_event.flags),
            &new_content.data,
            calendars,
            timestamp,
            &mut alerts,
        )
        .await?;
    let scheduled = ZaScheduled {
        messages: None,
        resolution: None,
    };
    let account = server.account(owner).await?;
    let addressed = new_names[0].parent_id;
    new_event.names = new_names;
    za_commit_update(
        server,
        token,
        &resources,
        &account,
        &keys,
        &roles,
        ZaEventUpdate {
            document_id,
            addressed,
            stored_event: &stored_event,
            stored_content: &stored_content,
            new_event,
            new_content,
            display_name: view.display_name,
            prev_alarms,
            next_alarms,
            scheduled,
            contexts,
            placement: plan.is_change().then_some(plan),
        },
    )
    .await
}

/// The stages a key account's destroy runs, in process.
async fn destroy(
    server: &Server,
    token: &AccessToken,
    owner: u32,
    document_id: u32,
    schedule: ZaScheduling,
) -> Result<ZaDeleted, ZaPipelineError> {
    let resources = server
        .fetch_groupware_resources(token.account_id(), owner, SyncCollection::Calendar)
        .await?;
    let event: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::archive(
            owner,
            Collection::CalendarEvent,
            document_id,
        ))
        .await?
        .ok_or(ZaPipelineError::NotFound)?;
    let keys = za_writer_keys(server, token, owner)
        .await?
        .expect("pipeline fixture is a key account");
    let roles = za_roles(server, token, owner).await?;
    let send_itip = za_delete_send_itip(server, token, &roles.owner, schedule)?;
    za_destroy_event(
        server,
        token,
        &resources,
        &keys,
        &roles,
        owner,
        document_id,
        event,
        send_itip,
    )
    .await
}

/// An event organized by the pipeline owner with each attendee initially needing action.
fn invitation(uid: &str, summary: &str, attendees: &[&str]) -> String {
    let mut properties = String::from("ORGANIZER:mailto:jmapcal-pipeline@example.com\r\n");
    for attendee in attendees {
        properties.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    properties.push_str("END:VEVENT\r\n");
    event_ics(uid, summary).replace("END:VEVENT\r\n", &properties)
}

async fn domains(test: &TestServer, fx: &Fx) -> (ZaKeyDomain, ZaKeyDomain) {
    let keys = za_unlock(test, fx.pair.key_id, STRONG).await;
    let personal = za_owned_calendar_context(&test.server, &keys, fx.a)
        .await
        .unwrap();
    let shared = za_owned_calendar_context(&test.server, &keys, fx.s)
        .await
        .unwrap();
    (personal.domain, shared.domain)
}

async fn vanished_at(test: &TestServer, owner: u32, change_id: u64) -> Vec<String> {
    let mut hrefs: Vec<_> = test
        .server
        .store()
        .vanished_with_change_id::<String>(
            owner,
            VanishedCollection::Calendar.into(),
            Query::RangeInclusive(change_id, change_id),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|(_, href)| href)
        .collect();
    hrefs.sort_unstable();
    hrefs
}

async fn parents_of(test: &TestServer, owner: u32, document_id: u32) -> Vec<u32> {
    let stored = stored_event(test, owner, document_id).await;
    let mut parents: Vec<_> = stored
        .meta
        .unarchive::<CalendarEvent>()
        .unwrap()
        .names
        .iter()
        .map(|name| name.parent_id.to_native())
        .collect();
    parents.sort_unstable();
    parents
}

async fn changes_after(test: &TestServer, owner: u32, tip: u64) -> Vec<u64> {
    let records = test
        .server
        .store()
        .za_changes_records_range(owner, SyncCollection::Calendar, tip, u64::MAX)
        .await
        .unwrap();
    assert!(!records.truncated, "{records:?}");
    records
        .records
        .into_iter()
        .map(|(change_id, _)| change_id)
        .collect()
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar pipeline tests...");
    let fx = Fx::new(test).await;
    create_in_two_calendars(test, &fx).await;
    create_uid_taken_in_second_calendar(test, &fx).await;
    create_naming_a_shared_calendar_with_another(test, &fx).await;
    lost_fence_rechecks_every_calendar(test, &fx).await;
    placement_move_between_unshared(test, &fx).await;
    placement_unshared_to_shared_reseals(test, &fx).await;
    placement_reseals_event_container(test, &fx).await;
    placement_matches_caldav(test, &fx).await;
    placement_uid_taken_in_entered_calendar(test, &fx).await;
    placement_with_content_change_commits_once(test, &fx).await;
    placement_retry_after_grant_change(test, &fx).await;
    placement_retry_after_source_rename(test, &fx).await;
    destroy_linked_event(test, &fx).await;
    destroy_single_name_equals_dav_delete(test, &fx).await;
    delete_send_itip_matches_dav(test, &fx).await;
    destroy_required_denied(test, &fx).await;
    destroy_retry_after_source_rename(test, &fx).await;
    update_without_contexts_is_refused(test, &fx).await;
    calendar_delete_with_a_stale_child_list(test, &fx).await;
    fx.cleanup(test).await;
    finish(test, "pipeline").await;
}

async fn create_in_two_calendars(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let token = session_token(test, owner).await;
    let uid = "jmapcal-pipe-two";
    let written = create(
        &test.server,
        &token,
        owner,
        &[fx.a, fx.b],
        &event_ics(uid, "two calendars"),
        ZaScheduling::On,
    )
    .await
    .expect("create_in_two_calendars: create with two unshared calendars");
    assert_eq!(za_uid_leases(test).await, 0);
    assert_eq!(uid_copies(test, owner, uid).await, 1);
    let stored = stored_event(test, owner, written.document_id).await;
    let mut parents: Vec<_> = stored
        .meta
        .unarchive::<CalendarEvent>()
        .unwrap()
        .names
        .iter()
        .map(|name| name.parent_id.to_native())
        .collect();
    let mut expected_parents = [fx.a, fx.b];
    parents.sort_unstable();
    expected_parents.sort_unstable();
    assert_eq!(parents, expected_parents);
    assert_eq!(stored.etag(), written.etag);
    assert_eq!(
        test.server
            .store()
            .get_last_change_id(owner, SyncCollection::Calendar.into())
            .await
            .unwrap(),
        Some(written.change_id)
    );

    for slug in ["pipe-a", "pipe-b"] {
        let body = fx
            .pair
            .dav
            .request("GET", &format!("{HOME}{slug}/{uid}.ics"), "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(
            body.contains("SUMMARY:two calendars"),
            "{slug} lost the summary"
        );
        assert!(!body.contains("X-ZA-"), "{slug} served a seal marker");
    }
    assert_no_event_preferences(test, owner, written.document_id).await;

    // Deleting the second calendar must invalidate the pending insert.
    let gone = make_calendar(test, &fx.pair.key, &fx.pair.dav, "pipe-gone").await;
    let gone_uid = "jmapcal-pipe-gone";
    let calendars = [fx.a, gone];
    let server = test.server.clone();
    let token = session_token(test, owner).await;
    let parked = HookParked::start(owner, set_commit_pause, async move {
        create(
            &server,
            &token,
            owner,
            &calendars,
            &event_ics(gone_uid, "second calendar deleted"),
            ZaScheduling::On,
        )
        .await
    })
    .await;
    fx.pair
        .dav
        .request("DELETE", &format!("{HOME}pipe-gone/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    match parked.finish().await {
        Err(ZaPipelineError::NotFound) => (),
        Err(err) => panic!("create_in_two_calendars: deleted calendar returned {err:?}"),
        Ok(written) => panic!(
            "create_in_two_calendars: inserted document {} with a deleted parent",
            written.document_id
        ),
    }
    assert_eq!(uid_copies(test, owner, gone_uid).await, 0);
    // The create guard's drop releases this lease on a spawned task.
    tokio::time::timeout(Duration::from_secs(2), async {
        while za_uid_leases(test).await != 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("create_in_two_calendars: the rejected create left a UID lease");

    for slug in ["pipe-a", "pipe-b"] {
        fx.pair
            .dav
            .request_with_headers("DELETE", &format!("{HOME}{slug}/{uid}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

async fn create_uid_taken_in_second_calendar(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let token = session_token(test, owner).await;
    let uid = "jmapcal-pipe-taken";
    let existing = put_event(
        test,
        &fx.pair.key,
        &fx.pair.dav,
        "pipe-b",
        uid,
        "already in pipe-b",
    )
    .await;

    for (order, calendars) in [("[a, b]", [fx.a, fx.b]), ("[b, a]", [fx.b, fx.a])] {
        match create(
            &test.server,
            &token,
            owner,
            &calendars,
            &event_ics(uid, "the second copy"),
            ZaScheduling::On,
        )
        .await
        {
            Err(ZaPipelineError::UidConflict { href: Some(href) }) => {
                assert!(
                    href.ends_with("/pipe-b/jmapcal-pipe-taken.ics"),
                    "{order}: {href}"
                );
            }
            Err(err) => panic!("create_uid_taken_in_second_calendar {order}: {err:?}"),
            Ok(written) => panic!(
                "create_uid_taken_in_second_calendar {order}: inserted conflicting document {}",
                written.document_id
            ),
        }
        assert_eq!(za_uid_leases(test).await, 0, "{order}");
        assert_eq!(uid_copies(test, owner, uid).await, 1, "{order}");
    }
    let stored = stored_event(test, owner, existing).await;
    assert_eq!(
        stored
            .meta
            .unarchive::<CalendarEvent>()
            .unwrap()
            .names
            .len(),
        1
    );
    create(
        &test.server,
        &token,
        owner,
        &[fx.a],
        &event_ics(uid, "same uid, other calendar"),
        ZaScheduling::On,
    )
    .await
    .expect("create_uid_taken_in_second_calendar: UID is free in pipe-a");

    for slug in ["pipe-a", "pipe-b"] {
        fx.pair
            .dav
            .request_with_headers("DELETE", &format!("{HOME}{slug}/{uid}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

async fn create_naming_a_shared_calendar_with_another(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let token = session_token(test, owner).await;
    let uid = "jmapcal-pipe-linked";
    for (order, calendars) in [("[a, s]", [fx.a, fx.s]), ("[s, a]", [fx.s, fx.a])] {
        let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);
        let result = create(
            &test.server,
            &token,
            owner,
            &calendars,
            &event_ics(uid, "linked"),
            ZaScheduling::On,
        )
        .await;
        let events = traced.finish().await;
        match result {
            Err(ZaPipelineError::LinkedEvent) => (),
            Err(err) => panic!("create_naming_a_shared_calendar_with_another {order}: {err:?}"),
            Ok(written) => panic!(
                "create_naming_a_shared_calendar_with_another {order}: inserted linked document {}",
                written.document_id
            ),
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.typ == EventType::Security(SecurityEvent::Unauthorized)
                        && event.account_id == Some(u64::from(owner))
                        && event.details == "zero-access: event linked into a shared calendar"
                })
                .count(),
            1,
            "{order}: expected the owner's linked-event refusal trace"
        );
        assert_eq!(uid_copies(test, owner, uid).await, 0, "{order}");
        assert_eq!(za_uid_leases(test).await, 0, "{order}");
    }

    let written = create(
        &test.server,
        &token,
        owner,
        &[fx.s],
        &event_ics(uid, "shared only"),
        ZaScheduling::On,
    )
    .await
    .expect("create_naming_a_shared_calendar_with_another: a single shared parent is allowed");
    let keys = za_unlock(test, owner, STRONG).await;
    let shared = za_owned_calendar_context(&test.server, &keys, fx.s)
        .await
        .unwrap();
    let personal = za_owned_calendar_context(&test.server, &keys, fx.a)
        .await
        .unwrap();
    assert!(shared.domain.calendar_id().is_some());
    assert!(personal.domain.calendar_id().is_none());
    let stored = stored_event(test, owner, written.document_id).await;
    assert!(unseal_event_in_domain(&mut stored.za_event(), &shared.domain).is_ok());
    assert!(unseal_event_in_domain(&mut stored.za_event(), &personal.domain).is_err());

    fx.pair
        .dav
        .request_with_headers("DELETE", &format!("{HOME}pipe-s/{uid}.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn lost_fence_rechecks_every_calendar(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    for (uid, plant) in [
        ("jmapcal-pipe-lost", true),
        ("jmapcal-pipe-lost-none", false),
    ] {
        let calendars = [fx.a, fx.b];
        let server = test.server.clone();
        let token = session_token(test, owner).await;
        set_lease_secs_override(Some(1));
        let parked = HookParked::start(owner, set_commit_pause, async move {
            create(
                &server,
                &token,
                owner,
                &calendars,
                &event_ics(uid, "lost fence"),
                ZaScheduling::On,
            )
            .await
        })
        .await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let lease = ZaUidLease::acquire(test.server.store(), owner, uid)
            .await
            .unwrap()
            .expect("lost_fence_rechecks_every_calendar: the expired lease can be taken over");
        if plant {
            plant_sealed_event(
                test,
                owner,
                "pipe-b",
                "lost-other.ics",
                &event_ics(uid, "the other creator"),
                STRONG,
                &TokenPairs::new(),
            )
            .await;
        }
        assert!(
            lease.release(test.server.store()).await,
            "{uid}: successor lease did not release"
        );
        set_lease_secs_override(None);

        match (plant, parked.finish().await) {
            (true, Err(ZaPipelineError::UidConflict { href: Some(href) })) => {
                assert!(href.ends_with("/pipe-b/lost-other.ics"), "{uid}: {href}");
            }
            (false, Err(ZaPipelineError::Busy(ZaBusy::CreateLost))) => (),
            (_, Err(err)) => panic!("lost_fence_rechecks_every_calendar {uid}: {err:?}"),
            (_, Ok(written)) => panic!(
                "lost_fence_rechecks_every_calendar {uid}: inserted document {} after lease loss",
                written.document_id
            ),
        }
        assert_eq!(
            uid_copies(test, owner, uid).await,
            u64::from(plant),
            "{uid}"
        );
        assert_eq!(za_uid_leases(test).await, 0, "{uid}");
        if plant {
            fx.pair
                .dav
                .request_with_headers(
                    "DELETE",
                    &format!("{HOME}pipe-b/lost-other.ics"),
                    [NO_REPLY],
                    "",
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
}

async fn placement_move_between_unshared(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-move";
    let id = put_event(
        test,
        &fx.pair.key,
        &fx.pair.dav,
        "pipe-a",
        uid,
        "moves to b",
    )
    .await;
    let token = session_token(test, owner).await;
    let written = update(&test.server, &token, owner, id, &[fx.b], None)
        .await
        .expect("placement_move_between_unshared: move to pipe-b");
    assert_eq!(written.document_id, id);
    assert_eq!(parents_of(test, owner, id).await, [fx.b]);
    let old_path = format!("{HOME}pipe-a/{uid}.ics");
    let new_path = format!("{HOME}pipe-b/{uid}.ics");
    assert_eq!(
        vanished_at(test, owner, written.change_id).await,
        [old_path.clone()]
    );
    fx.pair
        .dav
        .request("GET", &old_path, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    let response = fx
        .pair
        .dav
        .request("GET", &new_path, "")
        .await
        .with_status(StatusCode::OK);
    assert!(response.expect_body().contains("SUMMARY:moves to b"));
    assert!(!response.expect_body().contains("X-ZA-"));

    let (personal, shared) = domains(test, fx).await;
    let stored = stored_event(test, owner, id).await;
    assert!(unseal_event_in_domain(&mut stored.za_event(), &personal).is_ok());
    assert!(unseal_event_in_domain(&mut stored.za_event(), &shared).is_err());
    assert_eq!(stored.etag(), written.etag);
    assert_no_event_preferences(test, owner, id).await;
    fx.pair
        .dav
        .request_with_headers("DELETE", &new_path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn placement_unshared_to_shared_reseals(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-share";
    let summary = "enters the shared calendar";
    let id = put_event(test, &fx.pair.key, &fx.pair.dav, "pipe-a", uid, summary).await;
    let token = session_token(test, owner).await;
    let (personal, shared) = domains(test, fx).await;

    update(&test.server, &token, owner, id, &[fx.s], None)
        .await
        .expect("placement_unshared_to_shared_reseals: enter shared domain");
    let stored = stored_event(test, owner, id).await;
    assert!(unseal_event_in_domain(&mut stored.za_event(), &shared).is_ok());
    assert!(unseal_event_in_domain(&mut stored.za_event(), &personal).is_err());
    for client in [&fx.pair.dav, &fx.pair.dav2] {
        let response = client
            .request("GET", &format!("{HOME}pipe-s/{uid}.ics"), "")
            .await
            .with_status(StatusCode::OK);
        assert!(
            response
                .expect_body()
                .contains(&format!("SUMMARY:{summary}"))
        );
    }

    update(&test.server, &token, owner, id, &[fx.a], None)
        .await
        .expect("placement_unshared_to_shared_reseals: return to personal domain");
    let stored = stored_event(test, owner, id).await;
    assert!(unseal_event_in_domain(&mut stored.za_event(), &personal).is_ok());
    assert!(unseal_event_in_domain(&mut stored.za_event(), &shared).is_err());
    fx.pair
        .dav
        .request_with_headers("DELETE", &format!("{HOME}pipe-a/{uid}.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn placement_reseals_event_container(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-dead";
    let value = "jmapcal-pipe-dead-value";
    let id = put_event(
        test,
        &fx.pair.key,
        &fx.pair.dav,
        "pipe-a",
        uid,
        "annotated by a native client",
    )
    .await;
    let personal_path = format!("{HOME}pipe-a/{uid}.ics");
    fx.pair
        .dav
        .request("PROPPATCH", &personal_path, dead_proppatch(value))
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_dead_round_trip(&fx.pair.dav, &personal_path, value).await;
    let token = session_token(test, owner).await;
    let canaries: Vec<_> = DEAD_CANARIES.iter().copied().chain([value]).collect();

    let result = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let result = update(&test.server, &token, owner, id, &[fx.s], None).await;
        set_second_chunk_fail_once(None);
        result
    };
    result.expect("placement_reseals_event_container: move into shared domain");
    let shared_path = format!("{HOME}pipe-s/{uid}.ics");
    assert_dead_round_trip(&fx.pair.dav, &shared_path, value).await;
    assert_dead_round_trip(&fx.pair.dav2, &shared_path, value).await;
    let container = shared_container(test, owner, Collection::CalendarEvent, id)
        .await
        .unwrap();
    assert_eq!(container_is_ck(&container), Some(true));
    assert_dead_container_sealed(test, owner, id, &canaries).await;

    update(&test.server, &token, owner, id, &[fx.b], None)
        .await
        .expect("placement_reseals_event_container: move out of shared domain");
    let new_path = format!("{HOME}pipe-b/{uid}.ics");
    assert_dead_round_trip(&fx.pair.dav, &new_path, value).await;
    let container = shared_container(test, owner, Collection::CalendarEvent, id)
        .await
        .unwrap();
    assert_eq!(container_is_ck(&container), Some(false));
    assert_dead_container_sealed(test, owner, id, &canaries).await;
    fx.pair
        .dav
        .request_with_headers("DELETE", &new_path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn placement_matches_caldav(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let client = &fx.pair.dav;
    let closed = StatusCode::FORBIDDEN;
    let copy_refusal = Some((
        ZaRefusal::SharedCopy,
        "zero-access: copy into or out of a shared calendar",
    ));
    let linked_refusal = Some((
        ZaRefusal::LinkedMove,
        "zero-access: event linked into a shared calendar",
    ));
    let rows = [
        (
            "move",
            "pipe-a",
            false,
            &[fx.b][..],
            "MOVE",
            "pipe-a",
            "pipe-b",
            StatusCode::CREATED,
            None,
            &[fx.b][..],
            &["pipe-b"][..],
        ),
        (
            "link",
            "pipe-a",
            false,
            &[fx.a, fx.b],
            "COPY",
            "pipe-a",
            "pipe-b",
            StatusCode::CREATED,
            None,
            &[fx.a, fx.b],
            &["pipe-a", "pipe-b"],
        ),
        (
            "unlink",
            "pipe-a",
            true,
            &[fx.a],
            "DELETE",
            "pipe-b",
            "",
            StatusCode::NO_CONTENT,
            None,
            &[fx.a],
            &["pipe-a"],
        ),
        (
            "into-shared",
            "pipe-a",
            false,
            &[fx.s],
            "MOVE",
            "pipe-a",
            "pipe-s",
            StatusCode::CREATED,
            None,
            &[fx.s],
            &["pipe-s"],
        ),
        (
            "out-of-shared",
            "pipe-s",
            false,
            &[fx.a],
            "MOVE",
            "pipe-s",
            "pipe-a",
            StatusCode::CREATED,
            None,
            &[fx.a],
            &["pipe-a"],
        ),
        (
            "link-into-shared",
            "pipe-a",
            false,
            &[fx.a, fx.s],
            "COPY",
            "pipe-a",
            "pipe-s",
            closed,
            copy_refusal,
            &[fx.a],
            &["pipe-a"],
        ),
        (
            "link-out-of-shared",
            "pipe-s",
            false,
            &[fx.s, fx.a],
            "COPY",
            "pipe-s",
            "pipe-a",
            closed,
            copy_refusal,
            &[fx.s],
            &["pipe-s"],
        ),
        (
            "linked-into-shared",
            "pipe-a",
            true,
            &[fx.a, fx.s],
            "MOVE",
            "pipe-b",
            "pipe-s",
            closed,
            linked_refusal,
            &[fx.a, fx.b],
            &["pipe-a", "pipe-b"],
        ),
    ];
    let interests = [EventType::Security(SecurityEvent::Unauthorized)];
    for (row, initial, linked, calendars, method, from, to, status, refusal, parents, cleanup) in
        rows
    {
        let uid_p = format!("jmapcal-pipe-twin-{row}-p");
        let uid_c = format!("jmapcal-pipe-twin-{row}-c");
        let p = put_event(test, &fx.pair.key, client, initial, &uid_p, row).await;
        let c = put_event(test, &fx.pair.key, client, initial, &uid_c, row).await;
        if linked {
            for uid in [&uid_p, &uid_c] {
                client
                    .request_with_headers(
                        "COPY",
                        &format!("{HOME}pipe-a/{uid}.ics"),
                        [("destination", format!("{HOME}pipe-b/{uid}.ics").as_str())],
                        "",
                    )
                    .await
                    .with_status(StatusCode::CREATED);
            }
        }

        let token = session_token(test, owner).await;
        let traced = Traced::start(&interests);
        let result = update(&test.server, &token, owner, p, calendars, None).await;
        let pipeline_events = traced.finish().await;
        match (result, refusal) {
            (Ok(_), None) => (),
            (Err(ZaPipelineError::Forbidden(actual)), Some((expected, _))) => {
                assert_eq!(actual, expected, "{row}: placement refusal")
            }
            (Err(err), _) => panic!("{row}: unexpected pipeline error {err:?}"),
            (Ok(_), Some((expected, _))) => panic!("{row}: expected pipeline refusal {expected:?}"),
        }

        let traced = Traced::start(&interests);
        let source = format!("{HOME}{from}/{uid_c}.ics");
        let target = format!("{HOME}{to}/{uid_c}.ics");
        let headers = if method == "DELETE" {
            [NO_REPLY]
        } else {
            [("destination", target.as_str())]
        };
        client
            .request_with_headers(method, &source, headers, "")
            .await
            .with_status(status);
        let dav_events = traced.finish().await;

        let expected_details: BTreeSet<_> = refusal.into_iter().map(|(_, detail)| detail).collect();
        for (protocol, events) in [("pipeline", pipeline_events), ("CalDAV", dav_events)] {
            let details: BTreeSet<_> = events
                .iter()
                .filter(|event| event.account_id == Some(u64::from(owner)))
                .map(|event| event.details.as_str())
                .collect();
            assert_eq!(details, expected_details, "{row}: {protocol} trace details");
        }
        let mut expected_parents = parents.to_vec();
        expected_parents.sort_unstable();
        assert_eq!(
            parents_of(test, owner, p).await,
            expected_parents,
            "{row}: pipeline parents"
        );
        assert_eq!(
            parents_of(test, owner, c).await,
            expected_parents,
            "{row}: CalDAV parents"
        );
        for uid in [&uid_p, &uid_c] {
            for slug in cleanup {
                client
                    .request_with_headers(
                        "DELETE",
                        &format!("{HOME}{slug}/{uid}.ics"),
                        [NO_REPLY],
                        "",
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
    }
}

async fn placement_uid_taken_in_entered_calendar(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-uid";
    let client = &fx.pair.dav;
    let id = put_event(
        test,
        &fx.pair.key,
        client,
        "pipe-a",
        uid,
        "wants to enter b",
    )
    .await;
    let holder = format!("{HOME}pipe-b/holder.ics");
    client
        .request_with_headers(
            "PUT",
            &holder,
            [CONTENT_TYPE],
            event_ics(uid, "holds the uid in b"),
        )
        .await
        .with_status(StatusCode::CREATED);

    let token = session_token(test, owner).await;
    let version = stored_event(test, owner, id).await.version;
    match update(&test.server, &token, owner, id, &[fx.b], None).await {
        Err(ZaPipelineError::UidConflict { href: Some(href) }) => {
            assert!(href.ends_with("/pipe-b/holder.ics"), "{href}")
        }
        Err(err) => panic!("placement_uid_taken_in_entered_calendar: unexpected error {err:?}"),
        Ok(_) => panic!("placement_uid_taken_in_entered_calendar: conflicting UID was admitted"),
    }
    assert_eq!(stored_event(test, owner, id).await.version, version);
    assert_eq!(parents_of(test, owner, id).await, [fx.a]);
    let source = format!("{HOME}pipe-a/{uid}.ics");
    client
        .request_with_headers(
            "MOVE",
            &source,
            [("destination", format!("{HOME}pipe-b/{uid}.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    for path in [source, holder] {
        client
            .request_with_headers("DELETE", &path, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

async fn placement_with_content_change_commits_once(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-both";
    let client = &fx.pair.dav;
    let id = put_event(test, &fx.pair.key, client, "pipe-a", uid, "before").await;
    let tip = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();
    let token = session_token(test, owner).await;
    let replacement = event_ics(uid, "after").replace(
        "END:VEVENT\r\n",
        &format!(
            concat!(
                "BEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT10M\r\n",
                "ATTENDEE:mailto:{}\r\nSUMMARY:placement alarm\r\n",
                "DESCRIPTION:placement alarm\r\nEND:VALARM\r\nEND:VEVENT\r\n",
            ),
            fx.pair.key.name(),
        ),
    );
    let result = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let result = update(&test.server, &token, owner, id, &[fx.b], Some(&replacement)).await;
        set_second_chunk_fail_once(None);
        result
    };
    let written =
        result.expect("placement_with_content_change_commits_once: combined move and edit");

    assert_eq!(changes_after(test, owner, tip).await, [written.change_id]);
    assert_eq!(
        vanished_at(test, owner, written.change_id).await,
        [format!("{HOME}pipe-a/{uid}.ics")]
    );
    assert_eq!(parents_of(test, owner, id).await, [fx.b]);
    let new_path = format!("{HOME}pipe-b/{uid}.ics");
    let response = client
        .request("GET", &new_path, "")
        .await
        .with_status(StatusCode::OK);
    let body = response.expect_body();
    assert!(body.contains("SUMMARY:after"));
    assert!(!body.contains("SUMMARY:before"));
    assert!(written.ical.to_string().contains("SUMMARY:after"));

    let task_key = ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
        id: AlarmTarget::Owner.task_id().resolve(owner, id).into(),
    }));
    let task = test
        .server
        .store()
        .get_value::<Task>(task_key.clone())
        .await
        .unwrap();
    let Some(Task::CalendarAlarmEmail(task)) = task else {
        panic!("placement_with_content_change_commits_once: missing email alarm");
    };
    assert_eq!(task.account_id.document_id(), owner);
    assert_eq!(task.document_id.document_id(), id);
    let TaskStatus::Pending(status) = task.status else {
        panic!("placement_with_content_change_commits_once: alarm not pending");
    };
    // 2099-03-01T09:50:00Z, ten minutes before the event.
    assert_eq!(status.due.timestamp(), 4_076_041_800);

    client
        .request_with_headers("DELETE", &new_path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        test.server
            .store()
            .get_value::<Task>(task_key)
            .await
            .unwrap()
            .is_none()
    );
}

async fn placement_retry_after_grant_change(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let client = &fx.pair.dav;
    let g = make_calendar(test, &fx.pair.key, client, "pipe-g").await;
    let uid = "jmapcal-pipe-retry";
    let summary = "moves while pipe-g is shared";
    let id = put_event(test, &fx.pair.key, client, "pipe-a", uid, summary).await;
    let token = session_token(test, owner).await;
    let server = test.server.clone();
    let parked = HookParked::start(owner, set_update_commit_pause, async move {
        update(&server, &token, owner, id, &[g], None).await
    })
    .await;
    za_post(
        "shares/grant",
        &json!({
            "username": fx.pair.key.name(),
            "password": STRONG,
            "calendar": "pipe-g",
            "address": fx.pair.key2.name(),
            "level": "edit",
        }),
    )
    .await
    .expect(200);
    let tip = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();
    let result = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let result = parked.finish().await;
        set_second_chunk_fail_once(None);
        result
    };
    let written = result.expect("placement_retry_after_grant_change: retry in the fresh domain");

    assert_eq!(changes_after(test, owner, tip).await, [written.change_id]);
    assert_eq!(
        vanished_at(test, owner, written.change_id).await,
        [format!("{HOME}pipe-a/{uid}.ics")]
    );
    assert_eq!(parents_of(test, owner, id).await, [g]);
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, g)
        .await
        .unwrap();
    let stored = stored_event(test, owner, id).await;
    assert!(context.domain.calendar_id().is_some());
    assert!(unseal_event_in_domain(&mut stored.za_event(), &context.domain).is_ok());
    assert_eq!(stored.etag(), written.etag);
    let new_path = format!("{HOME}pipe-g/{uid}.ics");
    let response = fx
        .pair
        .dav2
        .request("GET", &new_path, "")
        .await
        .with_status(StatusCode::OK);
    assert!(
        response
            .expect_body()
            .contains(&format!("SUMMARY:{summary}"))
    );

    client
        .request_with_headers("DELETE", &new_path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", &format!("{HOME}pipe-g/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn placement_retry_after_source_rename(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let client = &fx.pair.dav;
    let name = "event%20name.ics";
    let old_calendar = format!("{HOME}pipe-old/");
    let renamed_calendar = format!("{HOME}pipe%20renamed/");
    let renamed_event = format!("{renamed_calendar}{name}");
    make_calendar(test, &fx.pair.key, client, "pipe-old").await;
    client
        .request_with_headers(
            "PUT",
            &format!("{old_calendar}{name}"),
            [CONTENT_TYPE],
            event_ics(
                "jmapcal-pipe-rename",
                "moves after the source calendar is renamed",
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    let id = resource_id(test, owner, &format!("pipe-old/{name}")).await;
    let token = session_token(test, owner).await;
    let server = test.server.clone();
    let b = fx.b;
    let parked = HookParked::start(owner, set_update_commit_pause, async move {
        update(&server, &token, owner, id, &[b], None).await
    })
    .await;

    client
        .request_with_headers(
            "MOVE",
            &old_calendar,
            [("destination", renamed_calendar.as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let initial = client
        .sync_collection(&renamed_calendar, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(initial.hrefs().contains(&renamed_event.as_str()));
    let sync_token = initial.sync_token().to_string();

    let written = parked
        .finish()
        .await
        .expect("placement_retry_after_source_rename: retry with the renamed source");
    assert_eq!(parents_of(test, owner, id).await, [fx.b]);
    assert_eq!(
        vanished_at(test, owner, written.change_id).await,
        [renamed_event.clone()]
    );

    client
        .sync_collection(
            &renamed_calendar,
            &sync_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .into_propfind_response(None)
        .properties(&renamed_event)
        .with_status(StatusCode::NOT_FOUND);
    client
        .request_with_headers("DELETE", &format!("{HOME}pipe-b/{name}"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", &renamed_calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn destroy_linked_event(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let client = &fx.pair.dav;
    let uid = "jmapcal-pipe-destroy";
    let value = "jmapcal-pipe-destroy-property";
    let ics = event_ics(uid, "destroy in both calendars").replace(
        "END:VEVENT\r\n",
        &format!(
            concat!(
                "BEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT10M\r\n",
                "ATTENDEE:mailto:{}\r\nSUMMARY:destroy alarm\r\n",
                "DESCRIPTION:destroy alarm\r\nEND:VALARM\r\nEND:VEVENT\r\n",
            ),
            fx.pair.key.name(),
        ),
    );
    let token = session_token(test, owner).await;
    let written = create(
        &test.server,
        &token,
        owner,
        &[fx.a, fx.b],
        &ics,
        ZaScheduling::On,
    )
    .await
    .expect("destroy_linked_event: create an event with two names and an email alarm");
    let id = written.document_id;
    let hrefs = [
        format!("{HOME}pipe-a/{uid}.ics"),
        format!("{HOME}pipe-b/{uid}.ics"),
    ];
    client
        .request("PROPPATCH", &hrefs[0], dead_proppatch(value))
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_dead_round_trip(client, &hrefs[0], value).await;
    let canaries: Vec<_> = DEAD_CANARIES.iter().copied().chain([value]).collect();
    assert_dead_container_sealed(test, owner, id, &canaries).await;

    let store = test.server.store();
    let alarm_id = TaskId::Assigned(AlarmTarget::Owner.task_id().resolve(owner, id));
    let task_key = ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: alarm_id }));
    let Some(Task::CalendarAlarmEmail(alarm)) =
        store.get_value::<Task>(task_key.clone()).await.unwrap()
    else {
        panic!("destroy_linked_event: missing email alarm");
    };
    assert_eq!(alarm.account_id.document_id(), owner);
    assert_eq!(alarm.document_id.document_id(), id);
    let TaskStatus::Pending(status) = alarm.status else {
        panic!("destroy_linked_event: alarm is not pending");
    };
    // 2099-03-01T09:50:00Z, ten minutes before the event.
    assert_eq!(status.due.timestamp(), 4_076_041_800);
    let due_key = ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Due {
        id: alarm_id,
        typ: TaskType::CalendarAlarmEmail,
        due: status.due.timestamp() as u64,
    }));
    assert!(
        store
            .get_value::<RawValue>(due_key.clone())
            .await
            .unwrap()
            .is_some()
    );
    let tip = store
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();

    let result = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let result = destroy(&test.server, &token, owner, id, ZaScheduling::On).await;
        set_second_chunk_fail_once(None);
        result
    };
    let deleted = result.expect("destroy_linked_event: all names commit together");

    assert_eq!(uid_copies(test, owner, uid).await, 0);
    assert_eq!(changes_after(test, owner, tip).await, [deleted.change_id]);
    assert_eq!(vanished_at(test, owner, deleted.change_id).await, hrefs);
    for href in hrefs {
        client
            .request("GET", &href, "")
            .await
            .with_status(StatusCode::NOT_FOUND);
    }
    assert_eq!(za_uid_leases(test).await, 0);
    for (label, key) in [
        (
            "event archive",
            ValueKey::archive(owner, Collection::CalendarEvent, id),
        ),
        (
            "event content",
            ValueKey::property(
                owner,
                Collection::CalendarEvent,
                id,
                CalendarEventField::Content,
            ),
        ),
        ("alarm Task", task_key),
        ("alarm Due", due_key),
    ] {
        assert!(
            store.get_value::<RawValue>(key).await.unwrap().is_none(),
            "{label} survived destroy"
        );
    }
    assert!(
        shared_container(test, owner, Collection::CalendarEvent, id)
            .await
            .is_none()
    );
}

async fn destroy_single_name_equals_dav_delete(test: &TestServer, fx: &Fx) {
    const DAV_UID: &str = "jmapcal-pipe-twin-dav";
    const PIPE_UID: &str = "jmapcal-pipe-twin-pipe";

    let owner = fx.pair.key_id;
    let address = fx.pair.key.name();
    let grantee = fx.pair.key2_id;
    let attendees = [fx.pair.key2.name(), OUTSIDE];
    let dav_href = format!("{HOME}pipe-a/{DAV_UID}.ics");
    let pipe_href = format!("{HOME}pipe-a/{PIPE_UID}.ics");
    for (uid, href) in [(DAV_UID, &dav_href), (PIPE_UID, &pipe_href)] {
        assert_eq!(
            put(
                address,
                STRONG,
                href.clone(),
                invitation(uid, "cancelled twice", &attendees)
            )
            .await,
            StatusCode::CREATED.as_u16(),
            "{uid}",
        );
    }
    wait_for_queue(test).await;
    let before = inbox_ids(test, grantee).await;
    sink().clear();

    let response = HttpRequest::with_credentials(8899, address, STRONG)
        .send_full(Method::DELETE, &dav_href, None, None)
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let id = resource_id(test, owner, &format!("pipe-a/{PIPE_UID}.ics")).await;
    let token = session_token(test, owner).await;
    let deleted = destroy(&test.server, &token, owner, id, ZaScheduling::On)
        .await
        .expect("destroy_single_name_equals_dav_delete: destroy the pipeline twin");
    wait_for_queue(test).await;

    assert_eq!(uid_copies(test, owner, DAV_UID).await, 0);
    assert_eq!(uid_copies(test, owner, PIPE_UID).await, 0);
    assert_eq!(
        vanished_at(test, owner, deleted.change_id).await,
        [pipe_href]
    );
    let items = new_inbox_items(test, grantee, &before).await;
    assert_eq!(
        items.len(),
        2,
        "destroy_single_name_equals_dav_delete: grantee's new inbox items"
    );
    let mut by_uid = BTreeMap::new();
    for (id, _) in &items {
        let (ical, _) = open_pending_item_as(test, grantee, STRONG, *id).await;
        let uid = ical.uids().next().expect("cancellation UID").to_string();
        let fields = (
            item_method(&ical),
            item_sequence(&ical),
            item_attendees(&ical),
        );
        assert!(
            by_uid.insert(uid, fields).is_none(),
            "duplicate cancellation"
        );
    }
    let dav = by_uid.get(DAV_UID).expect("CalDAV twin's cancellation");
    let pipeline = by_uid.get(PIPE_UID).expect("pipeline twin's cancellation");
    assert_eq!(dav.0, Some(ICalendarMethod::Cancel));
    assert_eq!(dav, pipeline);

    let mail = sink().take_to(OUTSIDE);
    assert_eq!(mail.len(), 2, "outside attendee's cancellation mail");
    let [dav_method, pipe_method] = [DAV_UID, PIPE_UID].map(|uid| {
        let uid_line = format!("UID:{uid}");
        let mut matches = mail.iter().filter(|message| {
            message
                .calendar_part()
                .is_some_and(|part| part.lines().any(|line| line == uid_line))
        });
        let method = matches
            .next()
            .unwrap_or_else(|| panic!("{uid}: missing cancellation mail"))
            .calendar_method()
            .expect("calendar MIME method");
        assert!(
            matches.next().is_none(),
            "{uid}: duplicate cancellation mail"
        );
        method
    });
    assert_eq!(dav_method, pipe_method);
    assert!(dav_method.eq_ignore_ascii_case("CANCEL"));
    assert_eq!(itip_tasks_for(test, owner).await, 0);

    for (id, _) in items {
        delete_inbox_item(test, &fx.pair.dav2, grantee, id).await;
    }
    sink().clear();
}

async fn delete_send_itip_matches_dav(test: &TestServer, fx: &Fx) {
    const NO_ADDRESS: &str =
        "This account has no calendar address to send scheduling messages from.";
    const NO_PERMISSION: &str = "This account is not allowed to send calendar scheduling messages.";

    let server = &test.server;
    let owner = fx.pair.key_id;
    let allowed = AccessToken::from_permissions(owner, [Permission::CalendarSchedulingSend]);
    let denied = AccessToken::from_permissions(owner, []);
    let stored = server.account_info(owner).await.unwrap();
    let mut no_address = stored.clone();
    no_address.addresses.clear();

    assert!(server.core.groupware.itip_enabled);
    for (token, info, sends, refusal) in [
        (&allowed, &stored, true, None),
        (&allowed, &no_address, false, Some(NO_ADDRESS)),
        (&denied, &stored, false, Some(NO_PERMISSION)),
        (&denied, &no_address, false, Some(NO_ADDRESS)),
    ] {
        let handler = server.core.groupware.itip_enabled
            && !info.addresses().is_empty()
            && token.has_permission(Permission::CalendarSchedulingSend);
        assert_eq!(handler, sends);
        assert!(!za_delete_send_itip(server, token, info, ZaScheduling::Off).unwrap());
        assert_eq!(
            za_delete_send_itip(server, token, info, ZaScheduling::On).unwrap(),
            handler
        );
        match (
            za_delete_send_itip(server, token, info, ZaScheduling::Required),
            refusal,
        ) {
            (Ok(true), None) => (),
            (Err(ZaPipelineError::SchedulingDenied(actual)), Some(expected)) => {
                assert_eq!(actual, expected);
            }
            (actual, expected) => {
                panic!("Required scheduling returned {actual:?}, expected refusal {expected:?}")
            }
        }
    }
}

async fn destroy_required_denied(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-denied";
    let client = &fx.pair.dav;
    let id = put_event(test, &fx.pair.key, client, "pipe-a", uid, "stays").await;
    let version = stored_event(test, owner, id).await.version;
    let token = AccessToken::from_permissions(owner, [])
        .with_session_keys(za_unlock(test, owner, STRONG).await);
    match destroy(&test.server, &token, owner, id, ZaScheduling::Required).await {
        Err(ZaPipelineError::SchedulingDenied(reason)) => {
            assert_eq!(
                reason,
                "This account is not allowed to send calendar scheduling messages."
            );
        }
        Err(err) => panic!("destroy_required_denied: unexpected refusal {err:?}"),
        Ok(_) => panic!("destroy_required_denied: destroyed an event despite denied scheduling"),
    }
    assert_eq!(uid_copies(test, owner, uid).await, 1);
    assert_eq!(stored_event(test, owner, id).await.version, version);
    client
        .request("GET", &format!("{HOME}pipe-a/{uid}.ics"), "")
        .await
        .with_status(StatusCode::OK);

    destroy(&test.server, &token, owner, id, ZaScheduling::On)
        .await
        .expect("destroy_required_denied: On saves despite denied scheduling");
    assert_eq!(uid_copies(test, owner, uid).await, 0);
}

/// Final review FB-1: an update handed no context (the JMAP update can pass one
/// when the owner stops reading as a key account between two objects) is
/// refused with `Forbidden(Context)` before anything is read, not a panic.
async fn update_without_contexts_is_refused(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-no-context";
    let client = &fx.pair.dav;
    let id = put_event(test, &fx.pair.key, client, "pipe-a", uid, "no context").await;
    let version = stored_event(test, owner, id).await.version;
    let token = session_token(test, owner).await;
    let server = &test.server;
    let resources = server
        .fetch_groupware_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let event_archive: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::archive(owner, Collection::CalendarEvent, id))
        .await
        .unwrap()
        .unwrap();
    let content_archive: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::property(
            owner,
            Collection::CalendarEvent,
            id,
            CalendarEventField::Content,
        ))
        .await
        .unwrap()
        .unwrap();
    let keys = za_writer_keys(server, &token, owner)
        .await
        .unwrap()
        .expect("pipeline fixture is a key account");
    let roles = za_roles(server, &token, owner).await.unwrap();
    let account = server.account(owner).await.unwrap();
    let result = za_commit_update(
        server,
        &token,
        &resources,
        &account,
        &keys,
        &roles,
        ZaEventUpdate {
            document_id: id,
            addressed: fx.a,
            stored_event: &event_archive,
            stored_content: &content_archive,
            new_event: event_archive.deserialize::<CalendarEvent>().unwrap(),
            new_content: content_archive
                .deserialize::<CalendarEventContent>()
                .unwrap(),
            display_name: None,
            prev_alarms: Default::default(),
            next_alarms: Default::default(),
            scheduled: ZaScheduled {
                messages: None,
                resolution: None,
            },
            contexts: ZaPlacementContexts::unchanged(Vec::new()),
            placement: None,
        },
    )
    .await;
    match result {
        Err(ZaPipelineError::Forbidden(ZaRefusal::Context)) => {}
        Err(err) => panic!("update_without_contexts_is_refused: unexpected refusal {err:?}"),
        Ok(_) => panic!("update_without_contexts_is_refused: updated without a context"),
    }
    assert_eq!(stored_event(test, owner, id).await.version, version);
    client
        .request("DELETE", &format!("{HOME}pipe-a/{uid}.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(uid_copies(test, owner, uid).await, 0);
}

async fn destroy_retry_after_source_rename(test: &TestServer, fx: &Fx) {
    let owner = fx.pair.key_id;
    let client = &fx.pair.dav;
    let uid = "jmapcal-pipe-destroy-rename";
    let name = "event%20name.ics";
    let old_calendar = format!("{HOME}pipe-destroy-old/");
    let renamed_calendar = format!("{HOME}pipe%20destroy%20renamed/");
    let renamed_event = format!("{renamed_calendar}{name}");
    make_calendar(test, &fx.pair.key, client, "pipe-destroy-old").await;
    client
        .request_with_headers(
            "PUT",
            &format!("{old_calendar}{name}"),
            [CONTENT_TYPE],
            event_ics(uid, "destroy after the source calendar is renamed"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let id = resource_id(test, owner, &format!("pipe-destroy-old/{name}")).await;
    let token = session_token(test, owner).await;
    let server = test.server.clone();
    let parked = HookParked::start(owner, set_delete_commit_pause, async move {
        destroy(&server, &token, owner, id, ZaScheduling::On).await
    })
    .await;

    client
        .request_with_headers(
            "MOVE",
            &old_calendar,
            [("destination", renamed_calendar.as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let initial = client
        .sync_collection(&renamed_calendar, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(initial.hrefs().contains(&renamed_event.as_str()));
    let sync_token = initial.sync_token().to_string();
    let tip = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();

    let result = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let result = parked.finish().await;
        set_second_chunk_fail_once(None);
        result
    };
    let deleted = result.expect("destroy_retry_after_source_rename: retry with the renamed source");

    assert_eq!(changes_after(test, owner, tip).await, [deleted.change_id]);
    assert_eq!(uid_copies(test, owner, uid).await, 0);
    assert_eq!(
        vanished_at(test, owner, deleted.change_id).await,
        [renamed_event.clone()]
    );
    client
        .sync_collection(
            &renamed_calendar,
            &sync_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .into_propfind_response(None)
        .properties(&renamed_event)
        .with_status(StatusCode::NOT_FOUND);
    client
        .request("DELETE", &renamed_calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// PR #90 review C4617611506: a calendar delete built from a child list that a
/// create made stale is refused (`Busy`) and leaves the calendar and the new
/// event; with a fresh list the same delete removes both.
async fn calendar_delete_with_a_stale_child_list(test: &TestServer, fx: &Fx) {
    const SLUG: &str = "pipe-fence";
    let owner = fx.pair.key_id;
    let uid = "jmapcal-pipe-fence-late";
    let server = &test.server;
    let token = session_token(test, owner).await;
    let calendar = make_calendar(test, &fx.pair.key, &fx.pair.dav, SLUG).await;
    let keys = za_writer_keys(server, &token, owner)
        .await
        .unwrap()
        .expect("pipeline fixture is a key account");
    let roles = za_roles(server, &token, owner).await.unwrap();
    let delete = |resources: std::sync::Arc<common::GroupwareResources>| {
        let (token, keys, roles) = (&token, &keys, &roles);
        async move {
            let stored: Archive<ArchiveBytes> = server
                .store()
                .get_value(ValueKey::archive(owner, Collection::Calendar, calendar))
                .await
                .unwrap()
                .expect("calendar archive");
            let target = za_calendar_delete_target(server, token, owner, calendar, stored)
                .await
                .unwrap();
            let children = resources.children_ids(calendar).collect::<Vec<_>>();
            let href = resources
                .container_resource_path_by_id(calendar)
                .map(|resource| resources.format_resource(resource));
            (
                children.clone(),
                za_delete_calendar(
                    server,
                    token,
                    &resources,
                    keys,
                    roles,
                    owner,
                    calendar,
                    target,
                    children,
                    &ContainerCleanup::empty(Collection::CalendarEvent),
                    href,
                    false,
                )
                .await,
            )
        }
    };

    // The snapshot is taken while the calendar is empty; the PUT lands after.
    let stale = server
        .fetch_groupware_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let event = put_event(test, &fx.pair.key, &fx.pair.dav, SLUG, uid, "late").await;
    let (children, result) = delete(stale).await;
    assert!(children.is_empty(), "{children:?}");
    match result {
        Err(ZaPipelineError::Busy(ZaBusy::CalendarChanged)) => {}
        Err(err) => panic!("calendar_delete_with_a_stale_child_list: unexpected refusal {err:?}"),
        Ok(_) => panic!("calendar_delete_with_a_stale_child_list: deleted with a stale child list"),
    }
    assert!(
        server
            .store()
            .get_value::<RawValue>(ValueKey::archive(owner, Collection::Calendar, calendar))
            .await
            .unwrap()
            .is_some(),
        "calendar_delete_with_a_stale_child_list: the calendar was deleted"
    );
    assert_eq!(parents_of(test, owner, event).await, [calendar]);

    let fresh = server
        .fetch_groupware_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let (children, result) = delete(fresh).await;
    assert_eq!(children, [event]);
    result.expect("calendar_delete_with_a_stale_child_list: the fresh delete");
    assert!(
        server
            .store()
            .get_value::<RawValue>(ValueKey::archive(owner, Collection::Calendar, calendar))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(uid_copies(test, owner, uid).await, 0);
}

