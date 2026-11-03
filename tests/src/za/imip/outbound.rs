/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, outbound (iMIP spec 5): a key account's scheduling messages
//! to the mail class leave as upstream's email through the SMTP queue, read
//! back at the suite's SMTP sink.

use super::{ATT, deliver, event, is_request_for, itip_mail};
use crate::{
    utils::{
        account::Account,
        server::TestServer,
        smtp_sink::sink,
        za::{
            HookParked, inbox_ids, new_inbox_items, queued_recipients, trigger_drain,
            wait_for_queue, za_app_password, za_setup_token,
        },
    },
    za::{
        STRONG,
        drain::DrainOn,
        fanout::{NO_REPLY, Party, attendee_statuses},
        mail::{Traced, TracedEvent},
        uid_lease::{self, put},
        user_permissions,
    },
};
use ::registry::{
    schema::{
        enums::Permission,
        prelude::{ObjectType, Property},
        structs::{MailingList, Permissions, PermissionsList, TaskCalendarItipContents},
    },
    types::map::Map,
};
use common::za::{
    imip::{ZaImipMail, ZaImipOutcome, za_imip_submit},
    test_hooks,
};
use groupware::scheduling::ItipSummary;
use hyper::StatusCode;
use std::collections::HashSet;
use trc::{CalendarEvent, EventType, Key};
use types::id::Id;

/// The key organizer of this module's cases.
pub const OUT_KEY: &str = "imip-out-key@example.com";
/// A second key account: an internal recipient (iMIP spec 5.1).
pub const OUT_KEY2: &str = "imip-out-key2@example.com";
/// A plain local account: a mail recipient (D9).
pub const OUT_PLAIN: &str = "imip-out-plain@example.com";
/// A key account without `CalendarSchedulingReceive`: not attempted (5.1).
const NOPERM: &str = "imip-noperm@example.com";
/// A key account whose setup is pending, so it serves no public key: not
/// attempted (5.1).
const PENDING: &str = "imip-pending@example.com";
/// A local mailing list of `OUT_KEY2` and `OUT_PLAIN`: a mail recipient
/// (Jay's decision 10).
const LIST: &str = "imip-list@example.com";

/// The seam case's iTIP REQUEST from `OUT_KEY` to `ATT`.
const SEAM_ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//imip seam//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:imip-out-seam\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSUMMARY:Seam\r\nORGANIZER:mailto:imip-out-key@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP outbound tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description) in [
        (OUT_KEY, "iMIP out key"),
        (OUT_KEY2, "iMIP out key two"),
        (NOPERM, "iMIP no receive"),
    ] {
        let account = admin
            .create_key_user_account(name, STRONG, description, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    // As `fanout::test` removes it from key3.
    admin
        .registry_update_object(
            ObjectType::Account,
            test.account(NOPERM).id(),
            serde_json::json!({ Property::Permissions: Permissions::Merge(PermissionsList {
                disabled_permissions: Map::new(vec![Permission::CalendarSchedulingReceive]),
                enabled_permissions: Map::new(user_permissions()),
            }) }),
        )
        .await;
    // Passwordless with a setup token: a key account that serves no key.
    let pending = admin
        .create_passwordless_user_account(PENDING, STRONG, "iMIP pending", &[], user_permissions())
        .await;
    za_setup_token(&admin, PENDING).await;
    let plain = admin
        .create_user_account(OUT_PLAIN, STRONG, "iMIP out plain", &[], user_permissions())
        .await;
    let domain_id = admin.find_or_create_domain("example.com").await;
    let list = admin
        .registry_create_object(MailingList {
            name: LIST.split_once('@').unwrap().0.to_string(),
            domain_id,
            recipients: Map::new(vec![OUT_KEY2.to_string(), OUT_PLAIN.to_string()]),
            ..Default::default()
        })
        .await;
    let owner = Party::new(test, OUT_KEY);
    let key2 = Party::new(test, OUT_KEY2);
    let start2 = inbox_ids(test, key2.id).await;

    seam(test, &owner).await;
    refresh_two_sessions(test, &owner).await;
    statuses(test, &owner).await;

    cleanup(
        test,
        &admin,
        &owner,
        (&key2, &start2),
        [pending, plain],
        list,
    )
    .await;
}

/// Deletes what the module created: its event, `OUT_KEY2`'s items, the
/// pending and plain accounts and the list. The key accounts go with the
/// suite's (`destroy_key_accounts`).
async fn cleanup(
    test: &TestServer,
    admin: &Account,
    owner: &Party,
    (key2, start2): (&Party, &[u32]),
    accounts: [Account; 2],
    list: Id,
) {
    owner
        .client
        .request_with_headers("DELETE", &statuses_path(owner), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    key2.delete_items_since(test, start2).await;
    for account in accounts {
        admin.destroy_account(account).await;
    }
    admin
        .registry_destroy(ObjectType::MailingList, [list])
        .await
        .assert_destroyed(&[list]);
    test.wait_for_tasks().await;
}

/// The href of `statuses`' event.
fn statuses_path(owner: &Party) -> String {
    format!(
        "/dav/cal/{}/default/imip-out-1.ics",
        owner.name.replace('@', "%40")
    )
}

/// Spec 5.3, D11, D12: one (message, recipient) in process memory, handed to
/// the SMTP queue: upstream's email without RSVP links.
async fn seam(test: &TestServer, owner: &Party) {
    let mail = |to: &str| ZaImipMail {
        account_id: owner.id,
        document_id: 0,
        contents: TaskCalendarItipContents {
            from: owner.name.into(),
            to: Map::new(vec![to.into()]),
            is_from_organizer: true,
            i_calendar_data: SEAM_ICS.into(),
            summary: serde_json::to_string(&ItipSummary::Invite(vec![])).unwrap(),
        },
    };
    // Every event of the three submissions below, for `assert_quiet`.
    let traced = Traced::start(EventType::variants());
    assert!(matches!(
        za_imip_submit(&test.server, mail(ATT)).await,
        ZaImipOutcome::Queued(_)
    ));
    wait_for_queue(test).await;
    let got = sink().take_to(ATT);
    assert_eq!(got.len(), 1, "one email, one recipient");
    assert_eq!(
        (got[0].mail_from.as_str(), got[0].rcpt_to.as_slice()),
        (owner.name, [ATT.to_string()].as_slice())
    );
    assert_eq!(
        got[0].header("Auto-Submitted").as_deref(),
        Some("auto-generated")
    );
    assert_eq!(got[0].calendar_method().as_deref(), Some("REQUEST"));
    assert_eq!(
        got[0].calendar_part().unwrap().trim_end(),
        SEAM_ICS.trim_end()
    );
    assert!(
        !got[0].html_part().unwrap().contains("/calendar/rsvp"),
        "no RSVP link"
    );
    // A local recipient the SMTP layer refuses at RCPT: nothing queued.
    assert!(matches!(
        za_imip_submit(&test.server, mail("imip-nobody@example.com")).await,
        ZaImipOutcome::Refused { stage: "RCPT-TO", code } if code / 100 == 5
    ));
    assert!(queued_recipients(test).await.is_empty());
    // The hook: traced, nothing sent.
    test_hooks::set_imip_submit_fail_once(Some(owner.id));
    assert!(matches!(
        za_imip_submit(&test.server, mail(ATT)).await,
        ZaImipOutcome::Failed
    ));
    test_hooks::set_imip_submit_fail_once(None);
    wait_for_queue(test).await;
    assert!(sink().take_to(ATT).is_empty());
    assert_quiet(&traced.finish().await, owner);
}

/// D14, 8.2: two sessions drain one REFRESH item; exactly one answer goes out.
async fn refresh_two_sessions(test: &TestServer, owner: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-out-r";
    let href = format!(
        "/dav/cal/{}/default/{uid}.ics",
        owner.name.replace('@', "%40")
    );
    assert_eq!(
        put(
            owner.name,
            STRONG,
            href.clone(),
            event(uid, owner.name, &[ATT], 0, "")
        )
        .await,
        201
    );
    // The second session first: an app password is a second key-cache
    // entry, so a second drain lock (invitations spec 6.3).
    let second = za_app_password(owner.name, STRONG, "imip-second-session").await;
    sink().clear();
    let before = inbox_ids(test, owner.id).await;
    deliver(
        "dmarc-imip-att@remote.test",
        owner.name,
        &itip_mail(
            ATT,
            owner.name,
            "REFRESH",
            &event(uid, owner.name, &[ATT], 0, ""),
        ),
    )
    .await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, owner.id, &before).await;
    assert_eq!(items.len(), 1, "the REFRESH item");
    // The losing session's pass defers; no deferral outlives this case.
    test_hooks::set_drain_retry_override(Some(0));
    // Session 1 (the password) parks after its UID lookup, before the merge
    // (spec question 3); session 2 drains the item and answers it; session
    // 1 then loses the item deletion's assertion and mails nothing.
    let parked = HookParked::start(
        owner.id,
        test_hooks::set_drain_lookup_pause,
        trigger_drain(owner.name, STRONG),
    )
    .await;
    assert_eq!(trigger_drain(owner.name, &second).await, 207);
    assert_eq!(parked.finish().await, 207);
    test_hooks::set_drain_retry_override(None);
    wait_for_queue(test).await;
    assert_eq!(
        sink()
            .take_to(ATT)
            .iter()
            .filter(|m| is_request_for(m, uid))
            .count(),
        1,
        "one answer per REFRESH"
    );
    assert!(
        !inbox_ids(test, owner.id).await.contains(&items[0].0),
        "the REFRESH item outlived its answer"
    );
    owner
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 5.1, 5.4, D9, decision 10: the status of each class.
async fn statuses(_test: &TestServer, owner: &Party) {
    let path = statuses_path(owner);
    assert_eq!(
        uid_lease::put(
            owner.name,
            STRONG,
            path.clone(),
            event(
                "imip-out-1",
                owner.name,
                &[
                    ATT,
                    "imip-out-plain@example.com",
                    "imip-out-key2@example.com",
                    "imip-pending@example.com",
                    "imip-noperm@example.com",
                    "imip-out-key+sub@example.com",
                    "imip-list@example.com"
                ],
                0,
                ""
            )
        )
        .await,
        201
    );
    for (address, status) in [
        (ATT, "1.1"),
        ("imip-out-plain@example.com", "1.1"),
        ("imip-list@example.com", "1.1"),
        ("imip-out-key2@example.com", "1.2"),
        ("imip-pending@example.com", "5.3"),
        ("imip-noperm@example.com", "5.3"),
        ("imip-out-key+sub@example.com", "5.3"),
    ] {
        assert_eq!(
            attendee_statuses(&owner.client, &path, address).await,
            [status],
            "{address}"
        );
    }
}

/// D17, invariant 47: the submission's own lookups are quiet and its own
/// events carry no address. Only the SMTP session and the queue trace the
/// envelope (section 7: transit metadata), so the events of their spans are
/// set aside. An address-bearing cache event carries the address's local
/// part as its `Key` and the domain in another event
/// (`common::cache::principals`, `rcpt_id_from_parts_inner` and
/// `domain_inner`), so the needle is the owner's local part; the domain
/// names every local account and is traced by upstream's own expressions
/// (`is_local_domain(sender_domain)`), so it is not one.
fn assert_quiet(events: &[TracedEvent], owner: &Party) {
    let span_id = |event: &TracedEvent| {
        event
            .keys
            .iter()
            .find_map(|(key, value)| match (key, value) {
                (Key::SpanId, trc::Value::UInt(id)) => Some(*id),
                _ => None,
            })
    };
    let is_transit = |event: &TracedEvent| {
        matches!(
            event.typ,
            EventType::Smtp(_) | EventType::Queue(_) | EventType::Delivery(_)
        )
    };
    let local_part = owner.name.split_once('@').unwrap().0;
    let transit_spans = events
        .iter()
        .filter(|event| is_transit(event))
        .filter_map(span_id)
        .collect::<HashSet<_>>();
    // Positive control: the capture saw the transit name the sender.
    assert!(
        events
            .iter()
            .any(|event| is_transit(event) && event.text.contains(owner.name)),
        "no transit event named the sender; the capture saw: {events:#?}"
    );
    let naming = events
        .iter()
        .filter(|event| {
            !is_transit(event) && span_id(event).is_none_or(|id| !transit_spans.contains(&id))
        })
        .filter(|event| event.text.contains(local_part))
        .map(|event| event.text.as_str())
        .collect::<Vec<_>>();
    assert!(
        naming.is_empty(),
        "the submission traced the sender's address outside the transit: {naming:#?}"
    );
    let own = events
        .iter()
        .filter(|event| {
            matches!(
                event.typ,
                EventType::Calendar(
                    CalendarEvent::ItipMessageSent
                        | CalendarEvent::ItipMessageError
                        | CalendarEvent::ItipMessageReceived
                )
            ) && event.account_id == Some(owner.id as u64)
        })
        .collect::<Vec<_>>();
    // Not vacuous: the queued submission's event is among them.
    assert!(
        own.iter()
            .any(|event| event.typ == EventType::Calendar(CalendarEvent::ItipMessageSent)),
        "no ItipMessageSent for the owner: {own:#?}"
    );
    for event in own {
        assert!(
            !event.text.contains('@'),
            "an iMIP event carries an address: {}",
            event.text
        );
    }
}
