/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{Pair, event_ics, finish, make_calendar, pair, put_event, session_token, share};
use crate::{
    utils::{
        server::TestServer,
        webdav::DummyWebDavClient,
        za::{
            HookParked, assert_no_event_preferences, delete_inbox_item, inbox_ids,
            plant_sealed_event, stored_event, za_uid_leases, za_unlock,
        },
    },
    za::{
        STRONG,
        fanout::{NO_REPLY, uid_copies},
        mail::Traced,
    },
};
use calcard::{common::timezone::Tz, icalendar::ICalendar};
use common::{
    DavName, Server,
    auth::AccessToken,
    za::{
        lease::ZaUidLease,
        test_hooks::{set_commit_pause, set_lease_secs_override},
    },
};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData,
        alerts::{DefaultAlerts, DefaultAlertsResolver},
        pipeline::{
            ZaScheduling, ZaWritten,
            admission::{za_roles, za_writer_keys},
            context::za_write_contexts,
            create::{ZaEventCreate, za_create_event},
            error::{ZaBusy, ZaPipelineError},
            schedule::za_schedule_create,
            uid::za_take_uid_lease,
        },
        schedule::{EventAlarmScheduler, EventAlarmUsers},
        seal::{TokenPairs, unseal_event_in_domain},
        sharing::resolve::za_owned_calendar_context,
    },
};
use hyper::StatusCode;
use std::time::Duration;
use store::write::now;
use trc::{EventType, SecurityEvent};
use types::{acl::Acl, collection::SyncCollection};

const HOME: &str = "/dav/cal/jmapcal-pipeline%40example.com/";

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

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar pipeline tests...");
    let fx = Fx::new(test).await;
    create_in_two_calendars(test, &fx).await;
    create_uid_taken_in_second_calendar(test, &fx).await;
    create_naming_a_shared_calendar_with_another(test, &fx).await;
    lost_fence_rechecks_every_calendar(test, &fx).await;
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
