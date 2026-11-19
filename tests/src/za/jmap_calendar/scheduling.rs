/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 5.4 (D4, D18), 3.3, 5.3 and 4.7 (review focus 1):
//! `CalendarEvent/set` scheduling for key accounts.

use super::{Pair, finish, key_account, pair};
use crate::{
    utils::{
        account::Account,
        http::HttpRequest,
        jmap::JmapResponse,
        server::TestServer,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{
            CALENDAR_JMAP_USING, ZaClientKind, clear_received_scheduling, copy_href, event_records,
            hold_task_consumer, inbox_ids, item_method, item_partstat, itip_tasks_for,
            new_inbox_items, open_pending_item_as, pending_ids, token_record, trigger_drain,
            wait_for_queue, za_bearer_dav, za_bearer_jmap, za_bearer_session, za_drop_oauth_client,
            za_keyed_grant, za_oauth_client, za_refresh, za_revoke_grant, za_revoke_token,
            za_sign_in, za_uid_leases, za_unlock,
        },
    },
    za::{
        STRONG,
        drain::{DrainOn, body, relative, set_partstat},
        fanout::{NO_REPLY, Party, attendee_statuses, invite, stored_pairs, uid_copies},
        gating::method_error,
        imip::{add_contact, delete_contact, deliver, event, itip_mail},
        mail::Traced,
        uid_lease::put,
        user_permissions,
    },
};
use ::registry::{
    schema::{
        enums::Permission,
        prelude::{ObjectType, Property},
        structs::{Permissions, PermissionsList},
    },
    types::map::Map,
};
use calcard::icalendar::ICalendarMethod;
use common::{
    auth::oauth::scope::OAuthScopes,
    za::{
        imip::ZA_IMIP_DROPPED,
        test_hooks::{self, Pause},
    },
};
use http::auth::oauth::TokenResponse;
use hyper::{Method, StatusCode};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use trc::{CalendarEvent as TrcCalendarEvent, EventType, Key};
use types::{collection::Collection, field::CalendarEventField, id::Id};
use vault::record::za_session_binding;

const OUT: &str = "jmapcal-scheduling-out@remote.test";
const PLAIN: &str = "plain@example.com";
const NOBODY: &str = "jmapcal-scheduling-nobody@example.com";
/// A calendar address no scheduling method reaches: it is no email address
/// (`itip_unreachable_recipient`, `crates/groupware/src/scheduling/itip.rs`).
const UNREACHABLE: &str = "https://jmapcal-scheduling.remote.test/unreachable";
const DENIED: &str = "This account is not allowed to send calendar scheduling messages.";
const INCOMPLETE: &str = "The server could not complete this change, try again.";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar scheduling tests...");
    sink().clear();
    let pair = pair(test, "scheduling").await;
    let (noperm, noperm_id) = key_account(
        test,
        "scheduling-noperm",
        user_permissions()
            .into_iter()
            .filter(|permission| *permission != Permission::CalendarSchedulingSend)
            .collect(),
    )
    .await;
    // `user_permissions()` lists no scheduling permission (the account's role
    // grants it), so it is disabled through the registry, as `fanout::test`
    // and `imip::outbound` disable theirs; the edit invalidates the token.
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            noperm.id(),
            json!({ Property::Permissions: Permissions::Merge(PermissionsList {
                disabled_permissions: Map::new(vec![Permission::CalendarSchedulingSend]),
                enabled_permissions: Map::new(user_permissions()),
            }) }),
        )
        .await;
    let test: &TestServer = test;
    let plain = test.account(PLAIN).clone();
    let s = Scheduling::new(test, &pair, &noperm, noperm_id, &plain).await;
    let key2_start = inbox_ids(test, pair.key2_id).await;
    let key_start = inbox_ids(test, pair.key_id).await;

    // First: it needs an account that has pinned nobody.
    first_invitation_and_the_session(&s).await;
    invite_key_outside_and_plain(&s).await;
    send_false_and_absent(&s).await;
    send_true_denied(&s).await;
    past_event_saved_unsent(&s).await;
    withheld_and_unreachable_statuses(&s).await;
    attendee_create_sends_no_reply(&s).await;
    mail_guard_on_failed_commit(&s).await;
    mail_guard_on_failed_create(&s).await;
    cancel_and_decline_on_destroy(&s).await;
    mail_guard_on_failed_delete(&s).await;
    no_itip_task_ever(&s).await;
    drain_runs_from_jmap(&s).await;
    mail_only_request_does_not_drain(&s).await;
    drain_only_within_the_token_scope(&s).await;
    invitation_by_email_found_by_uid(&s).await;
    drain_failure_does_not_fail_the_request(&s).await;

    wait_for_queue(test).await;
    Party::new(test, pair.key2.name())
        .delete_items_since(test, &key2_start)
        .await;
    Party::new(test, pair.key.name())
        .delete_items_since(test, &key_start)
        .await;
    clear_received_scheduling(test, &plain).await;
    s.delete_left().await;
    sink().clear();
    finish(test, "scheduling").await;
}

/// The module's accounts, their default calendars and the events it leaves.
struct Scheduling<'x> {
    test: &'x TestServer,
    pair: &'x Pair,
    noperm: &'x Account,
    noperm_id: u32,
    noperm_dav: DummyWebDavClient,
    plain: &'x Account,
    cal: String,
    noperm_cal: String,
    left: Mutex<Vec<(u32, &'static str)>>,
}

async fn default_calendar(account: &Account) -> String {
    let reply = account
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account.id_string(), "ids": null }),
        )
        .await;
    reply
        .list()
        .iter()
        .find(|entry| entry["isDefault"] == true)
        .or_else(|| reply.list().first())
        .and_then(|entry| entry["id"].as_str())
        .unwrap_or_else(|| panic!("no calendar for {}: {reply:?}", account.name()))
        .to_string()
}

/// `sendSchedulingMessages: true`.
fn send() -> Vec<(&'static str, Value)> {
    vec![("sendSchedulingMessages", Value::Bool(true))]
}

fn none() -> Vec<(&'static str, Value)> {
    Vec::new()
}

fn calendar_address(address: &str) -> String {
    if address.contains('@') {
        format!("mailto:{address}")
    } else {
        address.to_string()
    }
}

/// Task 30's `event(uid)` with participants: the organizer, owner and
/// chair, and each attendee expected to reply.
fn invitation(organizer: &str, calendar_id: &str, uid: &str, attendees: &[&str]) -> Value {
    let mut participants = serde_json::Map::new();
    participants.insert(
        "p0".to_string(),
        json!({
            "@type": "Participant",
            "calendarAddress": format!("mailto:{organizer}"),
            "roles": { "owner": true, "chair": true },
            "participationStatus": "accepted",
        }),
    );
    for (index, attendee) in attendees.iter().enumerate() {
        participants.insert(
            format!("p{}", index + 1),
            json!({
                "@type": "Participant",
                "calendarAddress": calendar_address(attendee),
                "kind": "individual",
                "participationStatus": "needs-action",
                "expectReply": true,
            }),
        );
    }
    json!({
        "@type": "Event",
        "uid": uid,
        "title": uid,
        "start": "2031-03-04T10:00:00",
        "duration": "PT1H",
        "timeZone": "Etc/UTC",
        "calendarIds": { calendar_id: true },
        "participants": participants,
    })
}

/// The ICS twin of `invitation`, organized by `organizer`.
fn invitation_ics(organizer: &str, uid: &str, attendees: &[&str]) -> String {
    let mut ics = invite(uid, "20310304T100000Z", &[])
        .replace("key1@example.com", organizer)
        .replace("END:VEVENT\r\nEND:VCALENDAR\r\n", "");
    for attendee in attendees {
        ics.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:{}\r\n",
            calendar_address(attendee)
        ));
    }
    ics.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ics
}

fn set_error(value: &Value) -> (&str, Option<&str>, Value) {
    (
        value["type"].as_str().unwrap_or_default(),
        value["description"].as_str(),
        value["properties"].clone(),
    )
}

fn assert_updated(reply: &JmapResponse, id: &str) {
    assert!(
        reply
            .pointer(&format!("/methodResponses/0/1/updated/{id}"))
            .is_some(),
        "{id} not updated: {reply:?}"
    );
}

fn home(account: &Account) -> String {
    format!("/dav/cal/{}/", account.name().replace('@', "%40"))
}

impl<'x> Scheduling<'x> {
    async fn new(
        test: &'x TestServer,
        pair: &'x Pair,
        noperm: &'x Account,
        noperm_id: u32,
        plain: &'x Account,
    ) -> Self {
        Scheduling {
            test,
            pair,
            noperm,
            noperm_id,
            noperm_dav: DummyWebDavClient::new(noperm_id, noperm.name(), STRONG, noperm.name()),
            plain,
            cal: default_calendar(&pair.key).await,
            noperm_cal: default_calendar(noperm).await,
            left: Mutex::new(Vec::new()),
        }
    }

    fn leaves(&self, account_id: u32, uid: &'static str) {
        self.left.lock().unwrap().push((account_id, uid));
    }

    fn forget(&self, uid: &str) {
        self.left.lock().unwrap().retain(|(_, left)| *left != uid);
    }

    async fn create(
        &self,
        account: &Account,
        object: Value,
        args: Vec<(&str, Value)>,
    ) -> JmapResponse {
        account.jmap_create("CalendarEvent", [object], args).await
    }

    async fn update(
        &self,
        account: &Account,
        id: &str,
        patch: Value,
        args: Vec<(&str, Value)>,
    ) -> JmapResponse {
        account
            .jmap_update("CalendarEvent", [(id.to_string(), patch)], args)
            .await
    }

    async fn destroy(&self, account: &Account, id: &str, args: Vec<(&str, Value)>) -> JmapResponse {
        account.jmap_destroy("CalendarEvent", [id], args).await
    }

    async fn href(&self, account_id: u32, uid: &str) -> String {
        copy_href(self.test, account_id, uid).await
    }

    /// The CalDAV deletion, without scheduling, of every event the module left.
    async fn delete_left(&self) {
        let left = std::mem::take(&mut *self.left.lock().unwrap());
        for (account_id, uid) in left {
            let client = if account_id == self.noperm_id {
                &self.noperm_dav
            } else {
                &self.pair.dav
            };
            let href = self.href(account_id, uid).await;
            client
                .request_with_headers("DELETE", &href, [NO_REPLY], "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
}

/// Review focus 1 (spec 4.7, D19): a browser's first invitation to another
/// key account pins its key; the pin's rewrap is a vault write that keeps
/// the session.
async fn first_invitation_and_the_session(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-first";
    let (test, pair) = (s.test, s.pair);
    let admin = test.account("admin@example.com");

    // 1.
    let app = za_oauth_client(test, admin, "za-jmapcal-app", ZaClientKind::Application).await;
    let tokens = za_sign_in(test, &app, pair.key.name(), STRONG, None).await;
    let vault = test
        .server
        .za_vault_record(pair.key_id)
        .await
        .unwrap()
        .unwrap();
    let (revision, binding) = (vault.record.revision, za_session_binding(&vault.record));
    let before = inbox_ids(test, pair.key2_id).await;
    let account = pair.key.id_string();

    // 2.
    let (status, reply) = za_bearer_jmap(
        &tokens.access,
        CALENDAR_JMAP_USING,
        json!([["CalendarEvent/set", {
            "accountId": account,
            "sendSchedulingMessages": true,
            "create": { "i0": invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name()]) },
        }, "c0"]]),
    )
    .await;
    assert_eq!(status, 200, "{reply:?}");
    let id = reply.created_id(0).to_string();
    s.leaves(pair.key_id, UID);
    assert_eq!(
        new_inbox_items(test, pair.key2_id, &before).await.len(),
        1,
        "first_invitation_and_the_session: key2's new items"
    );

    // 3. The rewrap raised the vault revision and evicted the resident keys;
    // the token opens them again from the token record.
    let get = |token: String| {
        let id = id.clone();
        async move {
            za_bearer_jmap(
                &token,
                CALENDAR_JMAP_USING,
                json!([["CalendarEvent/get", { "accountId": account, "ids": [id] }, "c0"]]),
            )
            .await
        }
    };
    let (status, reply) = get(tokens.access.clone()).await;
    assert_eq!(
        status, 200,
        "first_invitation_and_the_session: the same token after the pin"
    );
    assert_eq!(reply.list().len(), 1, "{reply:?}");

    // 4.
    let TokenResponse::Granted(refreshed) = za_refresh(&app, &tokens.refresh).await else {
        panic!("first_invitation_and_the_session: the refresh was refused");
    };
    let (status, reply) = get(refreshed.access_token.clone()).await;
    assert_eq!(
        status, 200,
        "first_invitation_and_the_session: the refreshed token"
    );
    let [event] = reply.list() else {
        panic!("first_invitation_and_the_session: {reply:?}");
    };
    let key2_address = format!("mailto:{}", pair.key2.name());
    let participant = event["participants"]
        .as_object()
        .and_then(|participants| {
            participants
                .values()
                .find(|participant| participant["calendarAddress"] == key2_address.as_str())
        })
        .unwrap_or_else(|| panic!("key2 is no participant: {event}"));
    assert_eq!(
        participant["participationStatus"], "needs-action",
        "{event}"
    );
    // Upstream's JSCalendar projection carries no SCHEDULE-STATUS (calcard
    // drops the parameter, `jscalendar/import/params.rs:357`), so the status
    // is read where the event records it: key2's item was delivered.
    let statuses =
        attendee_statuses(&pair.dav, &s.href(pair.key_id, UID).await, pair.key2.name()).await;
    assert_eq!(
        statuses,
        ["1.2"],
        "first_invitation_and_the_session: key2's status"
    );

    // 5.
    let record = token_record(test, pair.key_id)
        .await
        .expect("first_invitation_and_the_session: the token record is gone");
    assert!(
        record.grant(tokens.grant_id).is_some(),
        "first_invitation_and_the_session: the grant left the token record"
    );
    let vault = test
        .server
        .za_vault_record(pair.key_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(vault.record.revision, revision + 1);
    assert_eq!(za_session_binding(&vault.record), binding);

    // 6.
    assert_eq!(
        za_revoke_token(&app, refreshed.refresh_token.as_deref().unwrap()).await,
        200
    );
    za_drop_oauth_client(test, admin, app).await;
}

/// D4, D18, spec 5.4: a key attendee gets a sealed item, an outside one and
/// a plain one get mail, as the CalDAV twin gives them.
async fn invite_key_outside_and_plain(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-invite";
    const DAV_UID: &str = "jmapcal-sched-invite-dav";
    let (test, pair) = (s.test, s.pair);
    let attendees = [pair.key2.name(), OUT, PLAIN];

    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &attendees),
            send(),
        )
        .await;
    let id = reply.created_id(0).to_string();
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    // Final review FC1-11: the ordinary local attendee received the
    // invitation; upstream's delivery imports it into its calendar.
    assert_eq!(
        uid_copies(test, s.plain.id().document_id(), UID).await,
        1,
        "invite_key_outside_and_plain: the plain attendee's copy"
    );
    let items = new_inbox_items(test, pair.key2_id, &before).await;
    assert_eq!(items.len(), 1, "invite_key_outside_and_plain: key2's items");
    let (ical, _) = open_pending_item_as(test, pair.key2_id, STRONG, items[0].0).await;
    assert_eq!(item_method(&ical), Some(ICalendarMethod::Request));
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "invite_key_outside_and_plain: mail to OUT");
    assert_eq!(mail[0].calendar_method().as_deref(), Some("REQUEST"));
    let href = s.href(pair.key_id, UID).await;
    for address in [OUT, PLAIN] {
        assert_eq!(
            attendee_statuses(&pair.dav, &href, address).await,
            ["1.1"],
            "{address}"
        );
    }

    // The CalDAV twin.
    assert_eq!(
        put(
            pair.key.name(),
            STRONG,
            format!("{}default/{DAV_UID}.ics", home(&pair.key)),
            invitation_ics(pair.key.name(), DAV_UID, &attendees),
        )
        .await,
        StatusCode::CREATED.as_u16()
    );
    s.leaves(pair.key_id, DAV_UID);
    wait_for_queue(test).await;
    let dav_href = s.href(pair.key_id, DAV_UID).await;
    assert_eq!(
        attendee_statuses(&pair.dav, &href, pair.key2.name()).await,
        attendee_statuses(&pair.dav, &dav_href, pair.key2.name()).await,
        "invite_key_outside_and_plain: key2's schedule status"
    );
    let keys = za_unlock(test, pair.key_id, STRONG).await;
    let jmap_pairs = stored_pairs(test, &keys, pair.key_id, &relative(&href))
        .await
        .unwrap_or_default();
    let dav_pairs = stored_pairs(test, &keys, pair.key_id, &relative(&dav_href))
        .await
        .unwrap_or_default();
    assert!(!jmap_pairs.is_empty(), "the JMAP invitation stored no pair");
    assert_eq!(jmap_pairs.len(), dav_pairs.len());
    for address in attendees {
        assert_eq!(
            jmap_pairs.get(address).is_some(),
            dav_pairs.get(address).is_some(),
            "invite_key_outside_and_plain: the pair of {address}"
        );
    }

    // An update with the flag schedules again.
    wait_for_queue(test).await;
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let reply = s
        .update(
            &pair.key,
            &id,
            json!({ "start": "2031-03-04T12:00:00" }),
            send(),
        )
        .await;
    assert_updated(&reply, &id);
    wait_for_queue(test).await;
    let items = new_inbox_items(test, pair.key2_id, &before).await;
    assert_eq!(
        items.len(),
        1,
        "invite_key_outside_and_plain: key2's update"
    );
    let (ical, _) = open_pending_item_as(test, pair.key2_id, STRONG, items[0].0).await;
    assert_eq!(item_method(&ical), Some(ICalendarMethod::Request));
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "invite_key_outside_and_plain: update mail");
    assert_eq!(mail[0].calendar_method().as_deref(), Some("REQUEST"));
}

/// D4: `sendSchedulingMessages` false or absent turns scheduling off.
async fn send_false_and_absent(s: &Scheduling<'_>) {
    let (test, pair) = (s.test, s.pair);
    for (label, uid, args) in [
        (
            "false",
            "jmapcal-sched-false",
            vec![("sendSchedulingMessages", Value::Bool(false))],
        ),
        ("absent", "jmapcal-sched-absent", none()),
    ] {
        sink().clear();
        let before = inbox_ids(test, pair.key2_id).await;
        let reply = s
            .create(
                &pair.key,
                invitation(pair.key.name(), &s.cal, uid, &[pair.key2.name(), OUT]),
                args,
            )
            .await;
        let id = reply.created_id(0).to_string();
        s.leaves(pair.key_id, uid);
        assert_unscheduled(s, &format!("{label}, create"), uid, &before).await;

        let reply = s
            .update(
                &pair.key,
                &id,
                json!({ "start": "2031-03-04T12:00:00" }),
                none(),
            )
            .await;
        assert_updated(&reply, &id);
        assert_unscheduled(s, &format!("{label}, update"), uid, &before).await;
    }
}

/// No item for key2, no mail and no schedule status since `before`.
async fn assert_unscheduled(s: &Scheduling<'_>, step: &str, uid: &str, before: &[u32]) {
    let (test, pair) = (s.test, s.pair);
    wait_for_queue(test).await;
    assert!(
        new_inbox_items(test, pair.key2_id, before).await.is_empty(),
        "send_false_and_absent, {step}: key2 got an item"
    );
    assert!(
        sink().messages().is_empty(),
        "send_false_and_absent, {step}: mail was sent"
    );
    let href = s.href(pair.key_id, uid).await;
    for address in [pair.key2.name(), OUT] {
        assert!(
            attendee_statuses(&pair.dav, &href, address)
                .await
                .is_empty(),
            "send_false_and_absent, {step}: {address} has a schedule status"
        );
    }
}

/// D18: with `sendSchedulingMessages: true`, a denied status refuses the
/// object before any write; without the flag, and over CalDAV, it is saved.
async fn send_true_denied(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-denied";
    const DAV_UID: &str = "jmapcal-sched-denied-dav";
    let test = s.test;

    sink().clear();
    let reply = s
        .create(
            s.noperm,
            invitation(s.noperm.name(), &s.noperm_cal, UID, &[OUT]),
            send(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_created(0)),
        ("forbidden", Some(DENIED), Value::Null),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, s.noperm_id, UID).await, 0);
    assert_eq!(za_uid_leases(test).await, 0);
    wait_for_queue(test).await;
    assert!(sink().messages().is_empty(), "send_true_denied: mail");

    let reply = s
        .create(
            s.noperm,
            invitation(s.noperm.name(), &s.noperm_cal, UID, &[OUT]),
            none(),
        )
        .await;
    let document_id = reply.created_id(0).document_id();
    s.leaves(s.noperm_id, UID);
    let (meta, content) = event_records(test, s.noperm_id, document_id).await;
    let id = Id::from(document_id).to_string();
    let reply = s
        .update(
            s.noperm,
            &id,
            json!({ "start": "2031-03-04T12:00:00" }),
            send(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&id)),
        ("forbidden", Some(DENIED), Value::Null),
        "{reply:?}"
    );
    let (meta_after, content_after) = event_records(test, s.noperm_id, document_id).await;
    assert!(
        meta.as_bytes() == meta_after.as_bytes() && content.as_bytes() == content_after.as_bytes(),
        "send_true_denied: the refused update changed the event"
    );

    assert_eq!(
        put(
            s.noperm.name(),
            STRONG,
            format!("{}default/{DAV_UID}.ics", home(s.noperm)),
            invitation_ics(s.noperm.name(), DAV_UID, &[OUT]),
        )
        .await,
        StatusCode::CREATED.as_u16()
    );
    s.leaves(s.noperm_id, DAV_UID);
    wait_for_queue(test).await;
    assert!(
        sink().messages().is_empty(),
        "send_true_denied: CalDAV mail"
    );
}

/// D18: an event in the past is saved and nothing is sent.
async fn past_event_saved_unsent(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-past";
    let (test, pair) = (s.test, s.pair);
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let mut object = invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name(), OUT]);
    object["start"] = json!("2001-03-04T10:00:00");
    let reply = s.create(&pair.key, object, send()).await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    assert!(
        new_inbox_items(test, pair.key2_id, &before)
            .await
            .is_empty(),
        "past_event_saved_unsent: key2 got an item"
    );
    assert!(
        sink().messages().is_empty(),
        "past_event_saved_unsent: mail"
    );
}

/// Spec 3.3: a withheld and an unreachable attendee record their statuses
/// and the object is saved, as on CalDAV; upstream refuses the plain twin.
async fn withheld_and_unreachable_statuses(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-withheld";
    const DAV_UID: &str = "jmapcal-sched-withheld-dav";
    const PLAIN_UID: &str = "jmapcal-sched-withheld-plain";
    let (test, pair) = (s.test, s.pair);
    let attendees = [NOBODY, UNREACHABLE];

    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &attendees),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    assert_eq!(
        put(
            pair.key.name(),
            STRONG,
            format!("{}default/{DAV_UID}.ics", home(&pair.key)),
            invitation_ics(pair.key.name(), DAV_UID, &attendees),
        )
        .await,
        StatusCode::CREATED.as_u16()
    );
    s.leaves(pair.key_id, DAV_UID);
    let href = s.href(pair.key_id, UID).await;
    let dav_href = s.href(pair.key_id, DAV_UID).await;
    for address in attendees {
        let statuses = attendee_statuses(&pair.dav, &href, address).await;
        assert!(!statuses.is_empty(), "{address}: no schedule status");
        assert_eq!(
            statuses,
            attendee_statuses(&pair.dav, &dav_href, address).await,
            "withheld_and_unreachable_statuses: {address}"
        );
    }

    let plain_cal = default_calendar(s.plain).await;
    let reply = s
        .create(
            s.plain,
            invitation(PLAIN, &plain_cal, PLAIN_UID, &attendees),
            send(),
        )
        .await;
    assert_eq!(
        reply.not_created(0)["type"],
        "noSupportedScheduleMethods",
        "{reply:?}"
    );
    assert_eq!(
        uid_copies(test, s.plain.id().document_id(), PLAIN_UID).await,
        0
    );
    wait_for_queue(test).await;
}

/// Spec 5.3 (D3, D28), release 5 invariant 46: an object whose commit fails
/// drops its mail; the object before it keeps its commit and its mail, the
/// one after it is not attempted.
async fn mail_guard_on_failed_commit(s: &Scheduling<'_>) {
    const UIDS: [&str; 3] = [
        "jmapcal-sched-guard-1",
        "jmapcal-sched-guard-2",
        "jmapcal-sched-guard-3",
    ];
    let (test, pair) = (s.test, s.pair);
    let key_id = pair.key_id;
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UIDS[0], &[OUT]),
            send(),
        )
        .await;
    let mut created = vec![(reply.created_id(0), UIDS[0])];
    for uid in &UIDS[1..] {
        let reply = s
            .create(
                &pair.key,
                invitation(pair.key.name(), &s.cal, uid, &[OUT]),
                send(),
            )
            .await;
        created.push((reply.created_id(0), *uid));
    }
    for uid in UIDS {
        s.leaves(key_id, uid);
    }
    wait_for_queue(test).await;
    sink().clear();
    let account = pair.key.id_string().to_string();
    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": account, "ids": [] }),
        )
        .await;
    let state = reply.state().to_string();
    created.sort_by_key(|(id, _)| id.to_string());
    let [first, second, third] = [created[0], created[1], created[2]];
    let third_before = event_records(test, key_id, third.0.document_id()).await;
    let mut updates = serde_json::Map::new();
    for (index, (id, _)) in created.iter().enumerate() {
        updates.insert(
            id.to_string(),
            json!({ "title": format!("guard canary {}", index + 1), "start": "2031-03-04T12:00:00" }),
        );
    }

    // The first object reaches its commit; the second pause is armed before
    // the first is released, and the failure once the call is in the second.
    let traced = Traced::start(&[EventType::Calendar(TrcCalendarEvent::ItipMessageError)]);
    let first_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_update_commit_pause(Some(first_pause.clone()));
    let caller = pair.key.clone();
    let args = json!({
        "accountId": account,
        "sendSchedulingMessages": true,
        "update": updates,
    });
    let call =
        tokio::spawn(async move { caller.jmap_method_call("CalendarEvent/set", args).await });
    tokio::time::timeout(Duration::from_secs(5), first_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_commit: the first object did not reach its commit");
    let second_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_update_commit_pause(Some(second_pause.clone()));
    first_pause.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), second_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_commit: the second object did not reach its commit");
    test_hooks::set_update_commit_fail_once(Some(key_id));
    second_pause.release.notify_one();
    let reply = call.await.unwrap();
    test_hooks::set_update_commit_pause(None);
    test_hooks::set_update_commit_fail_once(None);
    let dropped = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_IMIP_DROPPED && event.account_id == Some(u64::from(key_id))
        })
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (Key::DocumentId, trc::Value::UInt(id)) => Some(*id as u32),
                    _ => None,
                })
                .expect("a document id")
        })
        .collect::<Vec<_>>();

    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_updated(&reply, &first.0.to_string());
    for (id, _) in [second, third] {
        assert_eq!(
            set_error(reply.not_updated(&id.to_string())),
            ("forbidden", Some(INCOMPLETE), Value::Null),
            "{reply:?}"
        );
    }
    let first_body = body(&pair.dav, &s.href(key_id, first.1).await).await;
    assert!(first_body.contains("guard canary 1"), "{first_body}");
    let second_body = body(&pair.dav, &s.href(key_id, second.1).await).await;
    assert!(!second_body.contains("guard canary"), "{second_body}");
    let third_after = event_records(test, key_id, third.0.document_id()).await;
    assert!(
        third_before.0.as_bytes() == third_after.0.as_bytes()
            && third_before.1.as_bytes() == third_after.1.as_bytes(),
        "mail_guard_on_failed_commit: the third object was written"
    );

    let changes = pair
        .key
        .jmap_method_call(
            "CalendarEvent/changes",
            json!({ "accountId": account, "sinceState": state }),
        )
        .await;
    let response = changes.response_at(0);
    assert_eq!(
        response["updated"],
        json!([first.0.to_string()]),
        "{changes:?}"
    );
    assert_eq!(response["created"], json!([]), "{changes:?}");
    assert_eq!(response["destroyed"], json!([]), "{changes:?}");
    assert_eq!(response["newState"], reply.response_at(0)["newState"]);

    wait_for_queue(test).await;
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "mail_guard_on_failed_commit: mail to OUT");
    let uid_line = format!("UID:{}", first.1);
    assert!(
        mail[0]
            .calendar_part()
            .is_some_and(|part| part.lines().any(|line| line == uid_line)),
        "mail_guard_on_failed_commit: the mail is not the first event's"
    );
    assert_eq!(
        dropped,
        [second.0.document_id()],
        "mail_guard_on_failed_commit: ZA_IMIP_DROPPED"
    );
}

/// Spec 5.3 (D28, invariant 75; final review FC1-2): a create whose commit
/// fails is answered on that object, and every later object of the call, of
/// any kind, gets the same answer unattempted; the create before it keeps
/// its commit and its mail, the failed create's mail is dropped.
async fn mail_guard_on_failed_create(s: &Scheduling<'_>) {
    const UIDS: [&str; 2] = ["jmapcal-sched-create-1", "jmapcal-sched-create-2"];
    const UPDATED: &str = "jmapcal-sched-create-updated";
    const KEPT: &str = "jmapcal-sched-create-kept";
    let (test, pair) = (s.test, s.pair);
    let key_id = pair.key_id;
    let account = pair.key.id_string().to_string();
    let mut targets = Vec::new();
    for uid in [UPDATED, KEPT] {
        let reply = s
            .create(
                &pair.key,
                invitation(pair.key.name(), &s.cal, uid, &[]),
                none(),
            )
            .await;
        targets.push(reply.created_id(0));
        s.leaves(key_id, uid);
    }
    let [updated, kept] = [targets[0], targets[1]];
    wait_for_queue(test).await;
    sink().clear();
    let state = pair
        .key
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": account, "ids": [] }),
        )
        .await
        .state()
        .to_string();
    let updated_before = event_records(test, key_id, updated.document_id()).await;
    let kept_before = event_records(test, key_id, kept.document_id()).await;

    let traced = Traced::start(&[EventType::Calendar(TrcCalendarEvent::ItipMessageError)]);
    let first_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_commit_pause(Some(first_pause.clone()));
    let caller = pair.key.clone();
    let updated_key = updated.to_string();
    let args = json!({
        "accountId": account,
        "sendSchedulingMessages": true,
        "create": {
            "i0": invitation(pair.key.name(), &s.cal, UIDS[0], &[OUT]),
            "i1": invitation(pair.key.name(), &s.cal, UIDS[1], &[OUT]),
        },
        "update": { updated_key: { "title": "create guard canary" } },
        "destroy": [kept.to_string()],
    });
    let call =
        tokio::spawn(async move { caller.jmap_method_call("CalendarEvent/set", args).await });
    tokio::time::timeout(Duration::from_secs(5), first_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_create: the first create did not reach its commit");
    let second_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_commit_pause(Some(second_pause.clone()));
    first_pause.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), second_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_create: the second create did not reach its commit");
    test_hooks::set_commit_fail_once(Some(key_id));
    second_pause.release.notify_one();
    let reply = call.await.unwrap();
    test_hooks::set_commit_pause(None);
    test_hooks::set_commit_fail_once(None);
    let dropped = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_IMIP_DROPPED && event.account_id == Some(u64::from(key_id))
        })
        .count();
    s.leaves(key_id, UIDS[0]);

    assert_eq!(method_error(&reply), None, "{reply:?}");
    let first = reply.created_id(0);
    let response = reply.response_at(0);
    assert_eq!(
        response["created"].as_object().map(|created| created.len()),
        Some(1),
        "{reply:?}"
    );
    let incomplete = ("forbidden", Some(INCOMPLETE), Value::Null);
    assert_eq!(set_error(reply.not_created(1)), incomplete, "{reply:?}");
    assert_eq!(
        set_error(reply.not_updated(&updated.to_string())),
        incomplete,
        "{reply:?}"
    );
    assert_eq!(
        set_error(reply.not_destroyed(&kept.to_string())),
        incomplete,
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, key_id, UIDS[0]).await, 1);
    assert_eq!(uid_copies(test, key_id, UIDS[1]).await, 0);
    for (target, before) in [(updated, &updated_before), (kept, &kept_before)] {
        let after = event_records(test, key_id, target.document_id()).await;
        assert!(
            before.0.as_bytes() == after.0.as_bytes() && before.1.as_bytes() == after.1.as_bytes(),
            "mail_guard_on_failed_create: {target} was written"
        );
    }

    let changes = pair
        .key
        .jmap_method_call(
            "CalendarEvent/changes",
            json!({ "accountId": account, "sinceState": state }),
        )
        .await;
    let changed = changes.response_at(0);
    assert_eq!(
        changed["created"],
        json!([first.to_string()]),
        "{changes:?}"
    );
    assert_eq!(changed["updated"], json!([]), "{changes:?}");
    assert_eq!(changed["destroyed"], json!([]), "{changes:?}");
    assert_eq!(response["newState"], changed["newState"], "{changes:?}");

    wait_for_queue(test).await;
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "mail_guard_on_failed_create: mail to OUT");
    let uid_line = format!("UID:{}", UIDS[0]);
    assert!(
        mail[0]
            .calendar_part()
            .is_some_and(|part| part.lines().any(|line| line == uid_line)),
        "mail_guard_on_failed_create: the mail is not the first create's"
    );
    assert_eq!(dropped, 1, "mail_guard_on_failed_create: ZA_IMIP_DROPPED");
    assert_eq!(za_uid_leases(test).await, 0);
}

/// Final review FC1-1. This case pins what the code does today; it is not a
/// requirement. Ruling D1: a JMAP write is the CalDAV write of the converted
/// tree, and CalDAV's create has no attendee fallback (put to the owner as a
/// line for spec 3.3). A key account that creates, with the flag, an event an
/// outside address organizes, in which it has already accepted, sends the
/// organizer no REPLY, and a later change of its own participation status
/// with the flag mails nothing either.
async fn attendee_create_sends_no_reply(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-attendee";
    let (test, pair) = (s.test, s.pair);
    let address = pair.key.name();
    let mut object = invitation(OUT, &s.cal, UID, &[]);
    object["participants"]["p1"] = json!({
        "@type": "Participant",
        "calendarAddress": calendar_address(address),
        "kind": "individual",
        "participationStatus": "accepted",
        "expectReply": true,
    });
    wait_for_queue(test).await;
    sink().clear();
    let reply = s.create(&pair.key, object, send()).await;
    let id = reply.created_id(0).to_string();
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    assert!(
        sink().take_to(OUT).is_empty(),
        "attendee_create_sends_no_reply: the create mailed the organizer"
    );

    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": pair.key.id_string(), "ids": [id], "properties": ["participants"] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let own = calendar_address(address);
    let participant = reply.list()[0]["participants"]
        .as_object()
        .and_then(|participants| {
            participants
                .iter()
                .find(|(_, value)| {
                    value["calendarAddress"]
                        .as_str()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&own))
                })
                .map(|(key, _)| key.clone())
        })
        .unwrap_or_else(|| panic!("no participant for {address}: {reply:?}"));
    let path = format!("participants/{participant}/participationStatus");
    let reply = s
        .update(&pair.key, &id, json!({ path: "declined" }), send())
        .await;
    assert_updated(&reply, &id);
    wait_for_queue(test).await;
    assert!(
        sink().take_to(OUT).is_empty(),
        "attendee_create_sends_no_reply: the declined update mailed the organizer"
    );
}

/// The ids a `/set` answer lists under `destroyed`, empty when it lists none.
fn destroyed(reply: &JmapResponse) -> Vec<String> {
    reply
        .pointer("/methodResponses/0/1/destroyed")
        .and_then(|ids| ids.as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The JMAP id of the account's one event with `uid` (a drained copy is
/// named by the drain, so it is found by UID).
async fn event_id(test: &TestServer, account_id: u32, uid: &str) -> String {
    let ids = test
        .server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "{uid}: {} copies", ids.len());
    Id::from(ids.min().unwrap()).to_string()
}

/// Spec 6.5, D4, D18: a destroy with `sendSchedulingMessages: true` runs the
/// pipeline's delete with scheduling, so an organizer's cancels and an
/// attendee's decline go through the fan-out; without the flag nothing is
/// sent; a denied status refuses the destroy before anything is deleted.
async fn cancel_and_decline_on_destroy(s: &Scheduling<'_>) {
    const CANCEL: &str = "jmapcal-sched-cancel";
    const DECLINE: &str = "jmapcal-sched-decline";
    const UNSENT: &str = "jmapcal-sched-unsent";
    const REFUSED: &str = "jmapcal-sched-destroy-denied";
    let (test, pair) = (s.test, s.pair);

    // The organizer cancels.
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, CANCEL, &[pair.key2.name(), OUT]),
            send(),
        )
        .await;
    let id = reply.created_id(0).to_string();
    s.leaves(pair.key_id, CANCEL);
    wait_for_queue(test).await;
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let reply = s.destroy(&pair.key, &id, send()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(destroyed(&reply), [id.clone()], "{reply:?}");
    s.forget(CANCEL);
    assert_eq!(uid_copies(test, pair.key_id, CANCEL).await, 0);
    wait_for_queue(test).await;
    let items = new_inbox_items(test, pair.key2_id, &before).await;
    assert_eq!(
        items.len(),
        1,
        "cancel_and_decline_on_destroy: key2's items after the organizer's destroy"
    );
    let (ical, _) = open_pending_item_as(test, pair.key2_id, STRONG, items[0].0).await;
    assert_eq!(item_method(&ical), Some(ICalendarMethod::Cancel));
    assert_eq!(
        sink().messages().len(),
        1,
        "cancel_and_decline_on_destroy: the organizer's mail"
    );
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "cancel_and_decline_on_destroy: mail to OUT");
    assert_eq!(mail[0].calendar_method().as_deref(), Some("CANCEL"));

    // The attendee declines.
    let before = inbox_ids(test, pair.key2_id).await;
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, DECLINE, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, DECLINE);
    wait_for_queue(test).await;
    let items = new_inbox_items(test, pair.key2_id, &before).await;
    assert_eq!(
        items.len(),
        1,
        "cancel_and_decline_on_destroy: key2's invitation"
    );
    // The drain applies every pending item of key2's inbox: the earlier
    // steps' items go first, so it imports this invitation alone.
    Party::new(test, pair.key2.name())
        .delete_items_since(test, &[items[0].0])
        .await;
    {
        let _drain = DrainOn::new();
        assert_eq!(trigger_drain(pair.key2.name(), STRONG).await, 207);
    }
    let copy = event_id(test, pair.key2_id, DECLINE).await;
    // Upstream declines on a deletion only for an attendee that answered
    // (`attendee_decline` skips NEEDS-ACTION when `is_deletion`), so key2
    // accepts first, as the CalDAV twin does (`za::reply`).
    set_partstat(
        &pair.dav2,
        &s.href(pair.key2_id, DECLINE).await,
        pair.key2.name(),
        "ACCEPTED",
    )
    .await;
    let key_before = inbox_ids(test, pair.key_id).await;
    let reply = s.destroy(&pair.key2, &copy, send()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(destroyed(&reply), [copy.clone()], "{reply:?}");
    assert_eq!(uid_copies(test, pair.key2_id, DECLINE).await, 0);
    wait_for_queue(test).await;
    let items = new_inbox_items(test, pair.key_id, &key_before).await;
    assert_eq!(
        items.len(),
        1,
        "cancel_and_decline_on_destroy: key's items after the attendee's destroy"
    );
    let (ical, _) = open_pending_item_as(test, pair.key_id, STRONG, items[0].0).await;
    assert_eq!(item_method(&ical), Some(ICalendarMethod::Reply));
    assert_eq!(
        item_partstat(&ical, pair.key2.name()).as_deref(),
        Some("DECLINED")
    );

    // Without the flag nothing is sent.
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UNSENT, &[pair.key2.name(), OUT]),
            send(),
        )
        .await;
    let id = reply.created_id(0).to_string();
    s.leaves(pair.key_id, UNSENT);
    wait_for_queue(test).await;
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let key_before = inbox_ids(test, pair.key_id).await;
    let reply = s.destroy(&pair.key, &id, none()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(destroyed(&reply), [id.clone()], "{reply:?}");
    s.forget(UNSENT);
    assert_eq!(uid_copies(test, pair.key_id, UNSENT).await, 0);
    wait_for_queue(test).await;
    assert!(
        new_inbox_items(test, pair.key2_id, &before)
            .await
            .is_empty(),
        "cancel_and_decline_on_destroy: an item without the flag"
    );
    assert!(
        new_inbox_items(test, pair.key_id, &key_before)
            .await
            .is_empty(),
        "cancel_and_decline_on_destroy: an organizer's item without the flag"
    );
    assert!(
        sink().messages().is_empty(),
        "cancel_and_decline_on_destroy: mail without the flag"
    );

    // A denied status refuses the destroy.
    let reply = s
        .create(
            s.noperm,
            invitation(s.noperm.name(), &s.noperm_cal, REFUSED, &[OUT]),
            none(),
        )
        .await;
    let document_id = reply.created_id(0).document_id();
    let id = Id::from(document_id).to_string();
    s.leaves(s.noperm_id, REFUSED);
    let (meta, content) = event_records(test, s.noperm_id, document_id).await;
    let reply = s.destroy(s.noperm, &id, send()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_destroyed(&id)),
        ("forbidden", Some(DENIED), Value::Null),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, s.noperm_id, REFUSED).await, 1);
    let (meta_after, content_after) = event_records(test, s.noperm_id, document_id).await;
    assert!(
        meta.as_bytes() == meta_after.as_bytes() && content.as_bytes() == content_after.as_bytes(),
        "cancel_and_decline_on_destroy: the refused destroy changed the event"
    );
    wait_for_queue(test).await;
    assert!(
        sink().messages().is_empty(),
        "cancel_and_decline_on_destroy: mail from a refused destroy"
    );
}

/// Spec 5.3 (D3, D28), release 5 invariant 46: a destroy whose commit fails
/// drops its mail; the destroy before it keeps its commit and its mail, the
/// one after it is not attempted.
async fn mail_guard_on_failed_delete(s: &Scheduling<'_>) {
    const UIDS: [&str; 3] = [
        "jmapcal-sched-delguard-1",
        "jmapcal-sched-delguard-2",
        "jmapcal-sched-delguard-3",
    ];
    let (test, pair) = (s.test, s.pair);
    let key_id = pair.key_id;
    let mut created = Vec::new();
    for uid in UIDS {
        let reply = s
            .create(
                &pair.key,
                invitation(pair.key.name(), &s.cal, uid, &[OUT]),
                send(),
            )
            .await;
        created.push((reply.created_id(0), uid));
        s.leaves(key_id, uid);
    }
    wait_for_queue(test).await;
    sink().clear();
    let account = pair.key.id_string().to_string();
    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": account, "ids": [] }),
        )
        .await;
    let state = reply.state().to_string();
    let [first, second, third] = [created[0], created[1], created[2]];
    let third_before = event_records(test, key_id, third.0.document_id()).await;

    // The first object reaches its commit; the second pause is armed before
    // the first is released, and the failure once the call is in the second.
    let traced = Traced::start(&[EventType::Calendar(TrcCalendarEvent::ItipMessageError)]);
    let first_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_delete_commit_pause(Some(first_pause.clone()));
    let caller = pair.key.clone();
    let args = json!({
        "accountId": account,
        "sendSchedulingMessages": true,
        "destroy": [first.0.to_string(), second.0.to_string(), third.0.to_string()],
    });
    let call =
        tokio::spawn(async move { caller.jmap_method_call("CalendarEvent/set", args).await });
    tokio::time::timeout(Duration::from_secs(5), first_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_delete: the first object did not reach its commit");
    let second_pause = Arc::new(Pause::new(key_id));
    test_hooks::set_delete_commit_pause(Some(second_pause.clone()));
    first_pause.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), second_pause.arrived.notified())
        .await
        .expect("mail_guard_on_failed_delete: the second object did not reach its commit");
    test_hooks::set_delete_commit_fail_once(Some(key_id));
    second_pause.release.notify_one();
    let reply = call.await.unwrap();
    test_hooks::set_delete_commit_pause(None);
    test_hooks::set_delete_commit_fail_once(None);
    let dropped = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_IMIP_DROPPED && event.account_id == Some(u64::from(key_id))
        })
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (Key::DocumentId, trc::Value::UInt(id)) => Some(*id as u32),
                    _ => None,
                })
                .expect("a document id")
        })
        .collect::<Vec<_>>();

    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(destroyed(&reply), [first.0.to_string()], "{reply:?}");
    s.forget(first.1);
    for (id, _) in [second, third] {
        assert_eq!(
            set_error(reply.not_destroyed(&id.to_string())),
            ("forbidden", Some(INCOMPLETE), Value::Null),
            "{reply:?}"
        );
    }
    let copies = [
        uid_copies(test, key_id, first.1).await,
        uid_copies(test, key_id, second.1).await,
        uid_copies(test, key_id, third.1).await,
    ];
    assert_eq!(copies, [0, 1, 1], "mail_guard_on_failed_delete: uid copies");
    let third_after = event_records(test, key_id, third.0.document_id()).await;
    assert!(
        third_before.0.as_bytes() == third_after.0.as_bytes()
            && third_before.1.as_bytes() == third_after.1.as_bytes(),
        "mail_guard_on_failed_delete: the third object was written"
    );

    let changes = pair
        .key
        .jmap_method_call(
            "CalendarEvent/changes",
            json!({ "accountId": account, "sinceState": state }),
        )
        .await;
    let response = changes.response_at(0);
    assert_eq!(
        response["destroyed"],
        json!([first.0.to_string()]),
        "{changes:?}"
    );
    assert_eq!(response["created"], json!([]), "{changes:?}");
    assert_eq!(response["updated"], json!([]), "{changes:?}");
    assert_eq!(response["newState"], reply.response_at(0)["newState"]);

    wait_for_queue(test).await;
    let mail = sink().take_to(OUT);
    assert_eq!(mail.len(), 1, "mail_guard_on_failed_delete: mail to OUT");
    assert_eq!(mail[0].calendar_method().as_deref(), Some("CANCEL"));
    let uid_line = format!("UID:{}", first.1);
    assert!(
        mail[0]
            .calendar_part()
            .is_some_and(|part| part.lines().any(|line| line == uid_line)),
        "mail_guard_on_failed_delete: the mail is not the first event's"
    );
    assert_eq!(
        dropped,
        [second.0.document_id()],
        "mail_guard_on_failed_delete: ZA_IMIP_DROPPED"
    );
}

/// Invariant 57: no key-account write queues an iTIP task.
async fn no_itip_task_ever(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-no-task";
    let (test, pair) = (s.test, s.pair);
    let held = hold_task_consumer(test).await;
    let reply = s
        .create(
            &pair.key,
            invitation(
                pair.key.name(),
                &s.cal,
                UID,
                &[pair.key2.name(), OUT, PLAIN],
            ),
            send(),
        )
        .await;
    let id = reply.created_id(0).to_string();
    let reply = s
        .update(
            &pair.key,
            &id,
            json!({ "start": "2031-03-04T12:00:00" }),
            send(),
        )
        .await;
    assert_updated(&reply, &id);
    wait_for_queue(test).await;
    let href = s.href(pair.key_id, UID).await;
    let response = HttpRequest::with_credentials(8899, pair.key.name(), STRONG)
        .send_full(Method::DELETE, &href, None, None)
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    s.forget(UID);
    wait_for_queue(test).await;
    assert_eq!(
        itip_tasks_for(test, pair.key_id).await,
        0,
        "no_itip_task_ever: an iTIP task was queued"
    );
    drop(held);
}

/// The ids of the account's `Calendar/get` of its own calendars: the request
/// that triggers the drain, whatever it asks for.
async fn calendar_get(account: &Account) -> JmapResponse {
    account
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account.id_string(), "ids": null }),
        )
        .await
}

/// Deletes every item of both accounts' scheduling inboxes, so the drain
/// cases below see only their own pending items (the cases before them leave
/// some). A non-pending item goes by a CalDAV `DELETE` as its account, which
/// runs that account's drain first, so the helper runs before the case turns
/// the drain on: with it on, the first `DELETE` would import every pending
/// item behind it and leave the events in the calendar.
async fn clear_inboxes(s: &Scheduling<'_>) {
    let (test, pair) = (s.test, s.pair);
    wait_for_queue(test).await;
    for (id, name) in [
        (pair.key_id, pair.key.name()),
        (pair.key2_id, pair.key2.name()),
    ] {
        Party::new(test, name)
            .delete_items(test, inbox_ids(test, id).await)
            .await;
    }
    assert!(pending_ids(test, pair.key_id).await.is_empty());
    assert!(pending_ids(test, pair.key2_id).await.is_empty());
}

/// Deletes key2's imported copy of `uid` without scheduling.
async fn destroy_copy(s: &Scheduling<'_>, uid: &str) {
    let (test, pair) = (s.test, s.pair);
    let id = event_id(test, pair.key2_id, uid).await;
    let reply = s.destroy(&pair.key2, &id, none()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(destroyed(&reply), [id], "{reply:?}");
    assert_eq!(uid_copies(test, pair.key2_id, uid).await, 0);
}

/// Spec 5.5: a JMAP request of calendar methods drains the caller's inbox
/// before dispatch.
async fn drain_runs_from_jmap(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-drain";
    let (test, pair) = (s.test, s.pair);
    clear_inboxes(s).await;
    assert_eq!(
        uid_copies(test, pair.key2_id, "jmapcal-sched-unsent").await,
        0,
        "drain_runs_from_jmap: clearing the inboxes imported an earlier case's invitation"
    );
    let _drain = DrainOn::new();

    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "drain_runs_from_jmap: key2's pending item before the request"
    );
    let reply = calendar_get(&pair.key2).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert!(
        pending_ids(test, pair.key2_id).await.is_empty(),
        "drain_runs_from_jmap: the item is still pending after Calendar/get"
    );
    assert_eq!(uid_copies(test, pair.key2_id, UID).await, 1);
    destroy_copy(s, UID).await;

    // The hook drains the caller's own inbox, whatever account the methods
    // name: key2's request names key's account.
    const OTHER: &str = "jmapcal-sched-drain-other";
    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, OTHER, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, OTHER);
    wait_for_queue(test).await;
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "drain_runs_from_jmap: key2's pending item before the other-account request"
    );
    // What this call itself answers is not what the step proves.
    let _ = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": pair.key.id_string(), "ids": null }),
        )
        .await;
    assert!(
        pending_ids(test, pair.key2_id).await.is_empty(),
        "drain_runs_from_jmap: a request naming another account did not drain the caller's inbox"
    );
    assert_eq!(uid_copies(test, pair.key2_id, OTHER).await, 1);
    destroy_copy(s, OTHER).await;
}

/// Spec 5.5: a request of mail methods only, or of `CalendarEventNotification`
/// methods only, does not drain; `ParticipantIdentity` does.
async fn mail_only_request_does_not_drain(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-nodrain";
    let (test, pair) = (s.test, s.pair);
    clear_inboxes(s).await;
    let _drain = DrainOn::new();
    let account = pair.key2.id_string();

    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "mail_only_request_does_not_drain: key2's pending item"
    );
    let reply = pair
        .key2
        .jmap_method_call("Mailbox/get", json!({ "accountId": account, "ids": null }))
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "mail_only_request_does_not_drain: Mailbox/get drained"
    );
    // The notification methods are served or refused as the table says; the
    // request does not drain either way.
    pair.key2
        .jmap_method_call(
            "CalendarEventNotification/get",
            json!({ "accountId": account, "ids": null }),
        )
        .await;
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "mail_only_request_does_not_drain: CalendarEventNotification/get drained"
    );
    let reply = pair
        .key2
        .jmap_method_call(
            "ParticipantIdentity/get",
            json!({ "accountId": account, "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert!(
        pending_ids(test, pair.key2_id).await.is_empty(),
        "mail_only_request_does_not_drain: ParticipantIdentity/get did not drain"
    );
    assert_eq!(uid_copies(test, pair.key2_id, UID).await, 1);
    destroy_copy(s, UID).await;
}

/// PR #90 review C4617611594: a keyed token gives its keys to the drain only
/// when its scope admits calendars, and on JMAP only for a calendar call that
/// parsed and whose capability is in the scope-filtered `using`. A mail-only
/// token's `Calendar/get` and `PROPFIND` and a contacts-only token's CardDAV
/// request leave the invitation pending; a calendar-scoped token drains.
async fn drain_only_within_the_token_scope(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-scope";
    let (test, pair) = (s.test, s.pair);
    clear_inboxes(s).await;
    let _drain = DrainOn::new();
    let account = pair.key2.id_string();
    let calendar_get = json!([["Calendar/get", { "accountId": account, "ids": null }, "c0"]]);
    let still_pending = |step: &'static str| async move {
        assert_eq!(
            pending_ids(test, pair.key2_id).await.len(),
            1,
            "drain_only_within_the_token_scope: {step} drained"
        );
        assert_eq!(uid_copies(test, pair.key2_id, UID).await, 0, "{step}");
    };

    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    still_pending("nothing").await;

    // A keyed mail-only token: it holds keys (Mailbox/get is served), its
    // Calendar/get answers unknownMethod and its PROPFIND is refused.
    let (mail_grant, mail_token, _) = za_keyed_grant(
        test,
        pair.key2.name(),
        pair.key2_id,
        STRONG,
        OAuthScopes::MAIL,
    )
    .await;
    let (status, reply) = za_bearer_jmap(
        &mail_token,
        CALENDAR_JMAP_USING,
        json!([["Mailbox/get", { "accountId": account, "ids": null }, "m0"]]),
    )
    .await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(method_error(&reply), None, "{reply:?}");
    still_pending("a mail-only Mailbox/get").await;
    let (status, reply) =
        za_bearer_jmap(&mail_token, CALENDAR_JMAP_USING, calendar_get.clone()).await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(method_error(&reply), Some("unknownMethod"), "{reply:?}");
    still_pending("a mail-only Calendar/get").await;
    let response = za_bearer_dav(&pair.key2, &mail_token)
        .request_with_headers(
            "PROPFIND",
            &format!("/dav/cal/{}/", pair.key2.name()),
            [("depth", "0")],
            "",
        )
        .await;
    assert_eq!(response.status.as_u16(), 403, "a mail-only PROPFIND");
    still_pending("a mail-only PROPFIND").await;

    // A keyed contacts-only token's CardDAV request is served and does not
    // drain the calendar inbox. A session read first caches the token, so the
    // DAV request, through the 500 ms client, is a cache hit (the mail-only
    // token's DAV step above follows its JMAP requests already).
    let (contacts_grant, contacts_token, _) = za_keyed_grant(
        test,
        pair.key2.name(),
        pair.key2_id,
        STRONG,
        OAuthScopes::CONTACTS,
    )
    .await;
    assert_eq!(za_bearer_session(&contacts_token).await.0, 200);
    let response = za_bearer_dav(&pair.key2, &contacts_token)
        .request_with_headers(
            "PROPFIND",
            &format!("/dav/card/{}/", pair.key2.name()),
            [("depth", "0")],
            "",
        )
        .await;
    assert_eq!(
        response.status.as_u16(),
        207,
        "a contacts-only CardDAV PROPFIND"
    );
    still_pending("a contacts-only CardDAV PROPFIND").await;

    // A calendar-scoped token: a Calendar/get whose `using` leaves the
    // calendars capability out is refused and does not drain; with it, the
    // request drains.
    let (calendar_grant, calendar_token, _) = za_keyed_grant(
        test,
        pair.key2.name(),
        pair.key2_id,
        STRONG,
        OAuthScopes::CALENDARS,
    )
    .await;
    let (status, reply) = za_bearer_jmap(
        &calendar_token,
        &["urn:ietf:params:jmap:core"],
        calendar_get.clone(),
    )
    .await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(method_error(&reply), Some("unknownMethod"), "{reply:?}");
    still_pending("a Calendar/get without the calendars capability").await;
    // A calendar call that does not parse keeps its name but is not
    // admitted: it answers a method error and does not drain.
    let (status, reply) = za_bearer_jmap(
        &calendar_token,
        CALENDAR_JMAP_USING,
        json!([["Calendar/get", { "accountId": account, "ids": 5 }, "c0"]]),
    )
    .await;
    assert_eq!(status, 200, "{reply:?}");
    assert!(method_error(&reply).is_some(), "{reply:?}");
    still_pending("a Calendar/get that did not parse").await;
    let (status, reply) = za_bearer_jmap(&calendar_token, CALENDAR_JMAP_USING, calendar_get).await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert!(
        pending_ids(test, pair.key2_id).await.is_empty(),
        "drain_only_within_the_token_scope: a calendar-scoped Calendar/get did not drain"
    );
    assert_eq!(uid_copies(test, pair.key2_id, UID).await, 1);

    destroy_copy(s, UID).await;
    for grant in [mail_grant, contacts_grant, calendar_grant] {
        za_revoke_grant(&test.server, grant).await;
    }
}

/// Spec 5.5 and 9.2: an invitation that arrived by email is drained by the
/// request that queries it, and the attendee's reply goes to the outside
/// organizer.
async fn invitation_by_email_found_by_uid(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-mail";
    let (test, pair) = (s.test, s.pair);
    clear_inboxes(s).await;
    let _drain = DrainOn::new();
    let account = pair.key.id_string();
    let address = pair.key.name();

    // A new UID from an outside organizer needs it in the address book.
    let card = add_contact(&pair.dav, address, OUT).await;
    sink().clear();
    // The envelope sender passes the test-mode DMARC check by its prefix.
    deliver(
        &format!("dmarc-{OUT}"),
        address,
        &itip_mail(OUT, address, "REQUEST", &event(UID, OUT, &[address], 0, "")),
    )
    .await;
    wait_for_queue(test).await;
    sink().clear();

    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/query",
            json!({ "accountId": account, "filter": { "uid": UID } }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let ids = reply.response_at(0)["ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        ids.len(),
        1,
        "the query did not find the invitation: {reply:?}"
    );
    let id = ids[0].as_str().unwrap_or_default().to_string();
    s.leaves(pair.key_id, UID);

    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": account, "ids": [id], "properties": ["participants"] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let own = calendar_address(address);
    let participant = reply.list()[0]["participants"]
        .as_object()
        .and_then(|participants| {
            participants
                .iter()
                .find(|(_, value)| {
                    value["calendarAddress"]
                        .as_str()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&own))
                })
                .map(|(key, _)| key.clone())
        })
        .unwrap_or_else(|| panic!("no participant for {address}: {reply:?}"));
    let path = format!("participants/{participant}/participationStatus");
    let reply = s
        .update(&pair.key, &id, json!({ path: "accepted" }), send())
        .await;
    assert_updated(&reply, &id);
    wait_for_queue(test).await;
    let mail = sink().take_to(OUT);
    assert_eq!(
        mail.len(),
        1,
        "invitation_by_email_found_by_uid: mail to OUT"
    );
    assert_eq!(mail[0].calendar_method().as_deref(), Some("REPLY"));
    let calendar = mail[0]
        .calendar_part()
        .unwrap_or_default()
        .replace("\r\n ", "");
    assert!(
        calendar.lines().any(|line| line.starts_with("ATTENDEE")
            && line.contains("PARTSTAT=ACCEPTED")
            && line
                .to_ascii_lowercase()
                .contains(&address.to_ascii_lowercase())),
        "invitation_by_email_found_by_uid: the reply's ATTENDEE line: {calendar}"
    );

    // What the case left: the contact and the delivered message.
    delete_contact(&pair.dav, &card).await;
    let reply = pair
        .key
        .jmap_method_call(
            "Email/get",
            json!({ "accountId": account, "ids": null, "properties": ["id", "subject"] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let mails: Vec<Value> = reply
        .list()
        .iter()
        .filter(|email| email["subject"] == "iMIP REQUEST")
        .map(|email| email["id"].clone())
        .collect();
    assert_eq!(mails.len(), 1, "the delivered message: {reply:?}");
    let reply = pair
        .key
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": account, "destroy": mails.clone() }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(reply.response_at(0)["destroyed"], json!(mails), "{reply:?}");
    sink().clear();
}

/// Invariant 14: a drain that fails does not fail the request, and the item
/// drains on a later request after the backoff.
async fn drain_failure_does_not_fail_the_request(s: &Scheduling<'_>) {
    const UID: &str = "jmapcal-sched-drainfail";
    let (test, pair) = (s.test, s.pair);
    clear_inboxes(s).await;
    let _drain = DrainOn::new();

    let reply = s
        .create(
            &pair.key,
            invitation(pair.key.name(), &s.cal, UID, &[pair.key2.name()]),
            send(),
        )
        .await;
    reply.created_id(0);
    s.leaves(pair.key_id, UID);
    wait_for_queue(test).await;
    assert_eq!(pending_ids(test, pair.key2_id).await.len(), 1);

    // The deferral is an absolute deadline taken at the failing pass, so the
    // short backoff is set before the failure.
    test_hooks::set_drain_retry_override(Some(2));
    test_hooks::set_drain_fail_once(Some(pair.key2_id));
    let reply = calendar_get(&pair.key2).await;
    assert_eq!(
        method_error(&reply),
        None,
        "a drain failure failed the request: {reply:?}"
    );
    assert_eq!(
        pending_ids(test, pair.key2_id).await.len(),
        1,
        "drain_failure_does_not_fail_the_request: the failed pass drained the item"
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    let reply = calendar_get(&pair.key2).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert!(
        pending_ids(test, pair.key2_id).await.is_empty(),
        "drain_failure_does_not_fail_the_request: no drain after the backoff"
    );
    test_hooks::set_drain_retry_override(None);
    destroy_copy(s, UID).await;
}
