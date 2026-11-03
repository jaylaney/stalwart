/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, outbound (iMIP spec 5): a key account's scheduling messages
//! to the mail class leave as upstream's email through the SMTP queue, read
//! back at the suite's SMTP sink.

use super::{
    ATT, DELEGATE, ORG, add_contact, delete_contact, deliver, event, is_request_for, itip_mail,
};
use crate::{
    utils::{
        account::Account,
        http::HttpRequest,
        server::TestServer,
        smtp_sink::{SinkMessage, sink},
        webdav::DummyWebDavClient,
        za::{
            HookParked, copy_href, hold_task_consumer, inbox_ids, item_partstat, itip_tasks_for,
            mail_borne_items, mail_count, new_inbox_items, open_pending_item_as, queued_recipients,
            trigger_drain, wait_for_delivery, wait_for_queue, za_app_password, za_post,
            za_setup_token,
        },
    },
    za::{
        STRONG,
        dav_seal::resource_id,
        drain::{DrainOn, body, partstat_of, set_partstat},
        fanout::{CONTENT_TYPE, MKCALENDAR, NO_REPLY, Party, attendee_statuses, uid_copies},
        mail::{Traced, TracedEvent, metadata_rows, row_is_stub, stored_blob},
        pinning::{pin, with_partstat},
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
use calcard::{Entry, Parser, icalendar::ICalendar};
use common::za::{
    imip::{ZA_IMIP_DROPPED, ZA_IMIP_FAILED, ZaImipMail, ZaImipOutcome, za_imip_submit},
    test_hooks::{self, ForceCommitSplit},
};
use groupware::{calendar::ChangedBy, scheduling::ItipSummary};
use hyper::{Method, StatusCode};
use mail_parser::MessageParser;
use serde_json::json;
use std::{collections::HashSet, future::Future, time::Duration};
use trc::{CalendarEvent, EventType, Key};
use types::id::Id;
use vault::keys::display_fingerprint;

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
/// A local mailing list of `OUT_KEY2`: a mail recipient (Jay's decision 10).
/// `OUT_PLAIN` is not a member (final review A-5): an attendee mailed both
/// directly and through the list gets two overlapping deliveries, which
/// upstream's ingest may both import (no lock on the UID).
const LIST: &str = "imip-list@example.com";
/// A key account granted `edit` on the owner's calendar `imip-shared`
/// (release 4, D12).
const GRANTEE: &str = "imip-grantee@example.com";
/// A key account whose second address is `ALIAS_TWO` (Review Focus 3).
const ALIAS: &str = "imip-alias@example.com";
const ALIAS_TWO: &str = "imip-alias-two@example.com";

/// The seam case's iTIP REQUEST from `OUT_KEY` to `ATT`.
const SEAM_ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//imip seam//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:imip-out-seam\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSUMMARY:Seam\r\nORGANIZER:mailto:imip-out-key@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP outbound tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description, aliases) in [
        (OUT_KEY, "iMIP out key", &[][..]),
        (OUT_KEY2, "iMIP out key two", &[][..]),
        (NOPERM, "iMIP no receive", &[][..]),
        (GRANTEE, "iMIP grantee", &[][..]),
        (ALIAS, "iMIP alias", &[ALIAS_TWO][..]),
    ] {
        let account = admin
            .create_key_user_account(name, STRONG, description, aliases, user_permissions())
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
            recipients: Map::new(vec![OUT_KEY2.to_string()]),
            ..Default::default()
        })
        .await;
    let owner = Party::new(test, OUT_KEY);
    let key2 = Party::new(test, OUT_KEY2);
    let grantee = Party::new(test, GRANTEE);
    let alias = Party::new(test, ALIAS);
    let starts = [
        inbox_ids(test, owner.id).await,
        inbox_ids(test, key2.id).await,
        inbox_ids(test, alias.id).await,
    ];
    // A new UID from ORG needs ORG in the address book (auto-add is off).
    let cards = [
        add_contact(&key2.client, OUT_KEY2, ORG).await,
        add_contact(&alias.client, ALIAS, ORG).await,
    ];
    // The drain cases import and answer; the drain is on for the module.
    let _drain = DrainOn::new();

    seam(test, &owner).await;
    refresh_two_sessions(test, &owner).await;
    mailed_once(test, &owner, &key2, &plain).await;
    plain_attendee_round_trip(test, &owner, &plain).await;
    update_and_delete(test, &owner).await;
    attendee_replies(test, &key2).await;
    delegation_to_outside(test, &key2).await;
    grantee_write(test, &owner, &grantee).await;
    submission_failure(test, &owner).await;
    commit_failures(test, &owner).await;
    store_commit_failures(test, &owner).await;
    chunked_collection_delete(test, &owner).await;
    rf_3_alias_attendee(test, &alias).await;
    rf_5_many_recipients(test, &owner).await;

    cleanup(
        test,
        &admin,
        [
            (&owner, &starts[0]),
            (&key2, &starts[1]),
            (&alias, &starts[2]),
        ],
        [(&key2, &cards[0]), (&alias, &cards[1])],
        [pending, plain],
        list,
    )
    .await;
}

/// Deletes what the module created and did not delete in its cases: the
/// key parties' inbox items and ContactCards, `OUT_KEY2`'s drained copy of
/// `imip-out-1`, the pending and plain accounts and the list. The key
/// accounts go with the suite's (`destroy_key_accounts`).
async fn cleanup(
    test: &TestServer,
    admin: &Account,
    items: [(&Party, &[u32]); 3],
    cards: [(&Party, &str); 2],
    accounts: [Account; 2],
    list: Id,
) {
    let key2 = items[1].0;
    if uid_copies(test, key2.id, "imip-out-1").await > 0 {
        let copy = copy_href(test, key2.id, "imip-out-1").await;
        key2.client
            .request_with_headers("DELETE", &copy, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for (party, start) in items {
        party.delete_items_since(test, start).await;
    }
    for (party, card) in cards {
        delete_contact(&party.client, card).await;
    }
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
    // iMIP spec 5.4: earlier writes (`imip::inbound`'s key organizer
    // inviting ATT) are mailed too; this case counts only its own.
    sink().clear();
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
    // The module holds `DrainOn`.
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

/// Spec 5.1, 5.4, D9, decision 10: the status of each class. `mailed_once`
/// runs it under the consumer hold.
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

/// The href of the owner's event `name` in its default calendar.
fn own_path(owner: &Party, name: &str) -> String {
    format!(
        "/dav/cal/{}/default/{name}.ics",
        owner.name.replace('@', "%40")
    )
}

/// A DELETE through the raw client (5 s), whose CANCELs may mail several
/// recipients or which waits at a pause point; resolves to the status.
fn delete_raw(
    name: &str,
    secret: &str,
    path: String,
) -> impl Future<Output = u16> + Send + 'static + use<> {
    let request = HttpRequest::with_credentials(8899, name, secret);
    async move {
        request
            .send_full(Method::DELETE, &path, None, None)
            .await
            .status
            .as_u16()
    }
}

/// Whether any message the sink holds carries `canary`, in its raw form or
/// in any decoded part.
fn sink_holds(canary: &str) -> bool {
    sink().messages().iter().any(|message| {
        String::from_utf8_lossy(&message.data).contains(canary)
            || message
                .decoded_parts()
                .iter()
                .any(|part| String::from_utf8_lossy(part).contains(canary))
    })
}

/// The sink copy's calendar part, parsed.
fn sink_calendar(message: &SinkMessage) -> ICalendar {
    match Parser::new(&message.calendar_part().expect("a text/calendar part")).entry() {
        Entry::ICalendar(ical) => ical,
        other => panic!("not an iCalendar part: {other:?}"),
    }
}

/// Whether the sink copy is an iTIP `method` naming `uid`.
fn is_method_for(message: &SinkMessage, method: &str, uid: &str) -> bool {
    let uid_line = format!("UID:{uid}");
    message.calendar_method().as_deref() == Some(method)
        && message.calendar_part().is_some_and(|ics| {
            ics.replace("\r\n ", "")
                .lines()
                .any(|line| line.trim_end() == uid_line)
        })
}

/// Spec 5.4, 5.6, 8.2, invariant 41: Task 6's PUT now mails ATT, imip-out-plain
/// and imip-list once each, with no task (the consumer held), no RSVP link; the
/// plain attendee imports it from its one delivery, whose copy has no RSVP link;
/// the key member's mail is sealed and its inbox gets no item (decision 10).
async fn mailed_once(test: &TestServer, owner: &Party, key2: &Party, plain: &Account) {
    let plain_id = plain.id().document_id();
    sink().clear();
    let (plain_mail, plain_rows, key2_rows, key2_items) = (
        mail_count(test, plain_id).await,
        metadata_rows(test, plain_id).await,
        metadata_rows(test, key2.id).await,
        inbox_ids(test, key2.id).await,
    );
    // Pre-flight ruling P4: the statuses PUT runs once, under the hold.
    let held = hold_task_consumer(test).await;
    statuses(test, owner).await;
    assert_eq!(
        itip_tasks_for(test, owner.id).await,
        0,
        "a mail recipient reached the task queue"
    );
    drop(held);
    wait_for_queue(test).await;
    let to_att = sink().take_to(ATT);
    assert_eq!(to_att.len(), 1, "ATT is mailed once");
    assert!(is_request_for(&to_att[0], "imip-out-1"));
    assert!(
        !to_att[0].html_part().unwrap().contains("/calendar/rsvp"),
        "an RSVP link in the key organizer's email"
    );
    assert_eq!(
        mail_count(test, plain_id).await,
        plain_mail + 1,
        "imip-out-plain is mailed once, directly"
    );
    assert_eq!(
        uid_copies(test, plain_id, "imip-out-1").await,
        1,
        "the plain attendee imports the invitation"
    );
    // Spec 8.2 (final review C-10): the plain local attendee's delivered copy
    // carries no RSVP link, in its HTML or its text.
    let new_plain_rows = metadata_rows(test, plain_id)
        .await
        .into_iter()
        .filter(|(id, _)| !plain_rows.iter().any(|(before, _)| before == id))
        .collect::<Vec<_>>();
    assert_eq!(new_plain_rows.len(), 1, "imip-out-plain's delivered copy");
    let raw = stored_blob(test, &new_plain_rows[0].1.unarchive().unwrap().blob_hash()).await;
    let delivered = MessageParser::new()
        .parse(&raw)
        .expect("the delivered copy parses");
    for (part, text) in [
        ("HTML", delivered.body_html(0)),
        ("text", delivered.body_text(0)),
    ] {
        let text = text.unwrap_or_else(|| panic!("the delivered copy has no {part} part"));
        assert!(
            !text.contains("/calendar/rsvp"),
            "an RSVP link in the {part} part of imip-out-plain's copy"
        );
    }
    // Decision 10: the key member's copy of the list's email is sealed, and
    // its inbox holds only the fan-out's internal item.
    let new_rows = metadata_rows(test, key2.id)
        .await
        .into_iter()
        .filter(|(id, _)| !key2_rows.iter().any(|(before, _)| before == id))
        .collect::<Vec<_>>();
    assert_eq!(new_rows.len(), 1, "the list's email reached imip-out-key2");
    assert!(
        row_is_stub(&new_rows[0].1),
        "the key member's email is sealed"
    );
    let items = new_inbox_items(test, key2.id, &key2_items).await;
    assert_eq!(
        items.len(),
        1,
        "imip-out-key2's inbox: the internal item only"
    );
    assert_eq!(items[0].1.changed_by, ChangedBy::PrincipalId(owner.id));

    // Control: a plain organizer's invitation, upstream's path, carries the
    // RSVP link the key organizer's lacks.
    let plain_client = DummyWebDavClient::new(plain_id, OUT_PLAIN, STRONG, OUT_PLAIN);
    let calendar = "/dav/cal/imip-out-plain%40example.com/imip-ctl/";
    plain_client
        .request("MKCALENDAR", calendar, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let control = format!("{calendar}imip-out-c.ics");
    plain_client
        .request_with_headers(
            "PUT",
            &control,
            [CONTENT_TYPE],
            event("imip-out-c", OUT_PLAIN, &[ATT], 0, ""),
        )
        .await
        .with_status(StatusCode::CREATED);
    wait_for_delivery(test).await;
    let to_att = sink().take_to(ATT);
    let invitation = to_att
        .iter()
        .find(|message| is_request_for(message, "imip-out-c"))
        .expect("the plain organizer's invitation");
    assert!(
        invitation
            .html_part()
            .unwrap()
            .contains("/calendar/rsvp?i="),
        "the control lacks upstream's RSVP link"
    );
    plain_client
        .request_with_headers("DELETE", &control, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 4.5, 5.6: the plain local attendee's REPLY returns through upstream's task
/// and mail into the key organizer's event.
async fn plain_attendee_round_trip(test: &TestServer, owner: &Party, plain: &Account) {
    let plain_id = plain.id().document_id();
    let plain_client = DummyWebDavClient::new(plain_id, OUT_PLAIN, STRONG, OUT_PLAIN);
    let copy = copy_href(test, plain_id, "imip-out-1").await;
    let before = inbox_ids(test, owner.id).await;
    set_partstat(&plain_client, &copy, OUT_PLAIN, "ACCEPTED").await;
    wait_for_delivery(test).await;
    let replies = mail_borne_items(test, owner.id)
        .await
        .into_iter()
        .filter(|(id, _)| !before.contains(id))
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 1, "the plain attendee's REPLY item");
    let (_, carriers) = open_pending_item_as(test, owner.id, STRONG, replies[0].0).await;
    assert_eq!(carriers.sender.as_deref(), Some(OUT_PLAIN));
    assert_eq!(trigger_drain(owner.name, STRONG).await, 207);
    let path = statuses_path(owner);
    assert_eq!(
        partstat_of(&owner.client, &path, OUT_PLAIN)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );
    assert_eq!(
        attendee_statuses(&owner.client, &path, OUT_PLAIN).await,
        ["2.0"]
    );
}

/// Spec 5.5: an update and a DELETE mail REQUEST and CANCEL.
async fn update_and_delete(test: &TestServer, owner: &Party) {
    let path = statuses_path(owner);
    sink().clear();
    let updated = body(&owner.client, &path)
        .await
        .replace("SUMMARY:imip imip-out-1", "SUMMARY:imip imip-out-1 v2");
    // ATT, imip-out-plain and the list: the raw client (5 s).
    assert_eq!(put(owner.name, STRONG, path.clone(), updated).await, 204);
    wait_for_queue(test).await;
    let to_att = sink().take_to(ATT);
    assert_eq!(to_att.len(), 1, "one email per update");
    assert!(is_request_for(&to_att[0], "imip-out-1"));
    assert!(
        to_att[0]
            .calendar_part()
            .unwrap()
            .contains("SUMMARY:imip imip-out-1 v2"),
        "the REQUEST lacks the new SUMMARY"
    );
    // The DELETE's CANCEL; the event goes with this case.
    assert_eq!(delete_raw(owner.name, STRONG, path).await, 204);
    wait_for_queue(test).await;
    let to_att = sink().take_to(ATT);
    assert_eq!(to_att.len(), 1, "one email per DELETE");
    assert!(is_method_for(&to_att[0], "CANCEL", "imip-out-1"));
}

/// An outside organizer's REQUEST for `uid`, delivered to `attendee` at
/// `address` and imported by its drain; returns the copy's href.
async fn invited_by_org(test: &TestServer, attendee: &Party, address: &str, uid: &str) -> String {
    deliver(
        "dmarc-org@remote.test",
        address,
        &itip_mail(ORG, address, "REQUEST", &event(uid, ORG, &[address], 0, "")),
    )
    .await;
    wait_for_queue(test).await;
    assert_eq!(trigger_drain(attendee.name, STRONG).await, 207);
    copy_href(test, attendee.id, uid).await
}

/// D10: a key attendee's ACCEPTED, DECLINED and DELETE mail REPLYs to ORG.
/// DECLINED goes first: upstream's DELETE of an already declined copy builds
/// no REPLY (`attendee_decline`, `scheduling/attendee.rs`, skips a DECLINED
/// attendee without `SCHEDULE-FORCE-SEND`), so the DELETE follows ACCEPTED.
async fn attendee_replies(test: &TestServer, attendee: &Party) {
    let copy = invited_by_org(test, attendee, attendee.name, "imip-out-a").await;
    sink().clear();
    // Each step's REPLY is read before the next, so the sink's order is the
    // steps' order.
    let reply = || {
        let replies = sink().take_to(ORG);
        assert_eq!(replies.len(), 1, "one REPLY per step");
        let reply = &replies[0];
        assert!(is_method_for(reply, "REPLY", "imip-out-a"));
        assert_eq!(reply.mail_from, attendee.name);
        assert_eq!(reply.from_address().as_deref(), Some(attendee.name));
        item_partstat(&sink_calendar(reply), attendee.name)
    };
    let mut partstats = Vec::new();
    for partstat in ["DECLINED", "ACCEPTED"] {
        set_partstat(&attendee.client, &copy, attendee.name, partstat).await;
        wait_for_queue(test).await;
        partstats.push(reply());
    }
    attendee
        .client
        .request("DELETE", &copy, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_queue(test).await;
    partstats.push(reply());
    assert_eq!(
        partstats,
        [
            Some("DECLINED".to_string()),
            Some("ACCEPTED".to_string()),
            Some("DECLINED".to_string())
        ]
    );
}

/// D10: a key delegator's REQUEST to an outside delegate.
async fn delegation_to_outside(test: &TestServer, attendee: &Party) {
    let copy = invited_by_org(test, attendee, attendee.name, "imip-out-d").await;
    let delegated = body(&attendee.client, &copy)
        .await
        .replace("\r\n ", "")
        .split("\r\n")
        .map(|line| {
            if line.starts_with("ATTENDEE") && line.ends_with(&format!("mailto:{}", attendee.name))
            {
                format!(
                    "{}\r\nATTENDEE;DELEGATED-FROM=\"mailto:{}\";PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{DELEGATE}",
                    with_partstat(line, &format!("DELEGATED;DELEGATED-TO=\"mailto:{DELEGATE}\"")),
                    attendee.name
                )
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(delegated.contains("DELEGATED-TO"), "{delegated}");
    sink().clear();
    // Two recipients: the raw client (5 s).
    assert_eq!(
        put(attendee.name, STRONG, copy.clone(), delegated).await,
        204
    );
    wait_for_queue(test).await;
    let replies = sink().take_to(ORG);
    assert_eq!(replies.len(), 1, "one REPLY to the organizer");
    assert!(is_method_for(&replies[0], "REPLY", "imip-out-d"));
    assert_eq!(
        item_partstat(&sink_calendar(&replies[0]), attendee.name).as_deref(),
        Some("DELEGATED")
    );
    let requests = sink().take_to(DELEGATE);
    assert_eq!(requests.len(), 1, "one REQUEST to the delegate");
    assert!(is_request_for(&requests[0], "imip-out-d"));
    assert_eq!(requests[0].mail_from, attendee.name);
    attendee
        .client
        .request_with_headers("DELETE", &copy, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// D12, release 4: a grantee's write mails from the owner's address with SENT-BY.
async fn grantee_write(test: &TestServer, owner: &Party, grantee: &Party) {
    let calendar = format!("/dav/cal/{}/imip-shared/", owner.name.replace('@', "%40"));
    owner
        .client
        .request("MKCALENDAR", &calendar, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let key = test
        .server
        .za_served_public_key(grantee.id)
        .await
        .unwrap()
        .unwrap();
    pin(owner.name, grantee.name, &display_fingerprint(&key))
        .await
        .expect(200);
    za_post(
        "shares/grant",
        &json!({"username":owner.name,"password":STRONG,"calendar":"imip-shared","address":grantee.name,"level":"edit"}),
    )
    .await
    .expect(200);
    sink().clear();
    let path = format!("{calendar}imip-out-g.ics");
    assert_eq!(
        put(
            grantee.name,
            STRONG,
            path.clone(),
            event("imip-out-g", owner.name, &[ATT], 0, "")
        )
        .await,
        201
    );
    wait_for_queue(test).await;
    let to_att = sink().take_to(ATT);
    assert_eq!(to_att.len(), 1, "the grantee's invitation");
    assert!(is_request_for(&to_att[0], "imip-out-g"));
    assert_eq!(to_att[0].mail_from, owner.name);
    assert_eq!(to_att[0].from_address().as_deref(), Some(owner.name));
    let ics = to_att[0].calendar_part().unwrap().replace("\r\n ", "");
    let organizer = ics
        .lines()
        .find(|line| line.starts_with("ORGANIZER"))
        .expect("an ORGANIZER");
    assert!(
        organizer
            .to_ascii_lowercase()
            .contains("sent-by=\"mailto:imip-grantee@example.com\""),
        "{organizer}"
    );
    owner
        .client
        .request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    za_post(
        "shares/revoke",
        &json!({"username":owner.name,"password":STRONG,"calendar":"imip-shared","grant":grantee.id}),
    )
    .await
    .expect(200);
    owner
        .client
        .request("DELETE", &calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Decision 9: a failed submission leaves 1.1 and is traced.
async fn submission_failure(test: &TestServer, owner: &Party) {
    let path = own_path(owner, "imip-out-f");
    sink().clear();
    let traced = Traced::start(&[EventType::Calendar(CalendarEvent::ItipMessageError)]);
    test_hooks::set_imip_submit_fail_once(Some(owner.id));
    owner
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            event("imip-out-f", owner.name, &[ATT], 0, ""),
        )
        .await
        .with_status(StatusCode::CREATED);
    test_hooks::set_imip_submit_fail_once(None);
    assert_eq!(attendee_statuses(&owner.client, &path, ATT).await, ["1.1"]);
    wait_for_queue(test).await;
    assert_eq!(
        sink().count_to(ATT),
        0,
        "the failed submission reached the sink"
    );
    let failed = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_IMIP_FAILED && event.account_id == Some(owner.id as u64)
        })
        .count();
    assert_eq!(failed, 1, "the failed submission is traced once");
    owner
        .client
        .request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// `event` inviting `ATT`, with its SUMMARY set to `summary`.
fn summarized(uid: &str, organizer: &str, summary: &str) -> String {
    event(uid, organizer, &[ATT], 0, "").replace(
        &format!("SUMMARY:imip {uid}"),
        &format!("SUMMARY:{summary}"),
    )
}

/// Decision 9, 8.2: a commit failure after the fan-out mails nothing, on each path.
async fn commit_failures(test: &TestServer, owner: &Party) {
    // Create: `COMMIT_FAIL_ONCE`.
    let create = own_path(owner, "imip-out-cf1");
    sink().clear();
    test_hooks::set_commit_fail_once(Some(owner.id));
    owner
        .client
        .request_with_headers(
            "PUT",
            &create,
            [CONTENT_TYPE],
            summarized("imip-out-cf1", owner.name, "imip-cf-create"),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    test_hooks::set_commit_fail_once(None);
    assert_eq!(uid_copies(test, owner.id, "imip-out-cf1").await, 0);
    wait_for_queue(test).await;
    assert!(!sink_holds("imip-cf-create"), "the failed create mailed");

    // Update: `UPDATE_COMMIT_FAIL_ONCE`.
    let update = own_path(owner, "imip-out-cf2");
    owner
        .client
        .request_with_headers(
            "PUT",
            &update,
            [CONTENT_TYPE],
            summarized("imip-out-cf2", owner.name, "imip-cf-base"),
        )
        .await
        .with_status(StatusCode::CREATED);
    wait_for_queue(test).await;
    sink().clear();
    test_hooks::set_update_commit_fail_once(Some(owner.id));
    owner
        .client
        .request_with_headers(
            "PUT",
            &update,
            [CONTENT_TYPE],
            summarized("imip-out-cf2", owner.name, "imip-cf-update"),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    test_hooks::set_update_commit_fail_once(None);
    wait_for_queue(test).await;
    assert!(!sink_holds("imip-cf-update"), "the failed update mailed");

    // Single-event DELETE: `DELETE_COMMIT_FAIL_ONCE`. Its CANCEL carries the
    // event's own SUMMARY, so the sink is cleared after the creation.
    let delete = own_path(owner, "imip-out-cf3");
    owner
        .client
        .request_with_headers(
            "PUT",
            &delete,
            [CONTENT_TYPE],
            summarized("imip-out-cf3", owner.name, "imip-cf-delete"),
        )
        .await
        .with_status(StatusCode::CREATED);
    wait_for_queue(test).await;
    sink().clear();
    test_hooks::set_delete_commit_fail_once(Some(owner.id));
    owner
        .client
        .request("DELETE", &delete, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    test_hooks::set_delete_commit_fail_once(None);
    assert_eq!(uid_copies(test, owner.id, "imip-out-cf3").await, 1);
    wait_for_queue(test).await;
    assert!(!sink_holds("imip-cf-delete"), "the failed DELETE mailed");

    for path in [update, delete] {
        owner
            .client
            .request_with_headers("DELETE", &path, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

/// 8.2: the store's own commit fails after every precommit step.
async fn store_commit_failures(test: &TestServer, owner: &Party) {
    // Update: the loser is parked before its commit while a competing
    // revision commits; its assertion fails (503), with no retry, since no
    // calendar context changed.
    let path = own_path(owner, "imip-out-sc1");
    assert_eq!(
        put(
            owner.name,
            STRONG,
            path.clone(),
            summarized("imip-out-sc1", owner.name, "imip-sc-base")
        )
        .await,
        201
    );
    wait_for_queue(test).await;
    sink().clear();
    let loser = HookParked::start(
        owner.id,
        test_hooks::set_update_commit_pause,
        put(
            owner.name,
            STRONG,
            path.clone(),
            summarized("imip-out-sc1", owner.name, "loser-update"),
        ),
    )
    .await;
    assert_eq!(
        put(
            owner.name,
            STRONG,
            path.clone(),
            summarized("imip-out-sc1", owner.name, "winner-update")
        )
        .await,
        204
    );
    assert_eq!(loser.finish().await, 503, "the parked update committed");
    wait_for_queue(test).await;
    assert!(sink_holds("winner-update"), "the winner's REQUEST");
    assert!(!sink_holds("loser-update"), "the losing update mailed");

    // Single-event DELETE: parked before its commit while a competing
    // revision commits; its assertion fails and no CANCEL goes out.
    sink().clear();
    let loser = HookParked::start(
        owner.id,
        test_hooks::set_delete_commit_pause,
        delete_raw(owner.name, STRONG, path.clone()),
    )
    .await;
    assert_eq!(
        put(
            owner.name,
            STRONG,
            path.clone(),
            summarized("imip-out-sc1", owner.name, "winner-again")
        )
        .await,
        204
    );
    assert_eq!(loser.finish().await, 503, "the parked DELETE committed");
    wait_for_queue(test).await;
    assert!(sink_holds("winner-again"), "the winner's REQUEST");
    assert!(
        !sink()
            .messages()
            .iter()
            .any(|message| is_method_for(message, "CANCEL", "imip-out-sc1")),
        "the losing DELETE mailed its CANCEL"
    );
    owner
        .client
        .request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // Create: the loser is parked before its commit past its one-second
    // lease; another create of the UID takes the lease over and commits;
    // the loser fails through `za_lost_create_fence` (403: the retake finds
    // the winner's copy, upgrade ruling 50).
    sink().clear();
    test_hooks::set_lease_secs_override(Some(1));
    let loser = HookParked::start(
        owner.id,
        test_hooks::set_commit_pause,
        put(
            owner.name,
            STRONG,
            own_path(owner, "imip-out-sc3a"),
            summarized("imip-out-sc3", owner.name, "loser-create"),
        ),
    )
    .await;
    // As `uid_lease::test` waits: past the lease at whole-second resolution.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let winner = own_path(owner, "imip-out-sc3b");
    assert_eq!(
        put(
            owner.name,
            STRONG,
            winner.clone(),
            summarized("imip-out-sc3", owner.name, "winner-create")
        )
        .await,
        201
    );
    test_hooks::set_lease_secs_override(None);
    assert_eq!(loser.finish().await, 403, "the parked create committed");
    wait_for_queue(test).await;
    assert!(sink_holds("winner-create"), "the winner's REQUEST");
    assert!(!sink_holds("loser-create"), "the losing create mailed");
    assert_eq!(uid_copies(test, owner.id, "imip-out-sc3").await, 1);
    owner
        .client
        .request_with_headers("DELETE", &winner, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// 5.4: a chunked collection DELETE whose second chunk fails mails nothing.
async fn chunked_collection_delete(test: &TestServer, owner: &Party) {
    let calendar = format!("/dav/cal/{}/imip-coll/", owner.name.replace('@', "%40"));
    owner
        .client
        .request("MKCALENDAR", &calendar, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let mut ids = Vec::new();
    for uid in ["imip-out-k1", "imip-out-k2"] {
        owner
            .client
            .request_with_headers(
                "PUT",
                &format!("{calendar}{uid}.ics"),
                [CONTENT_TYPE],
                event(uid, owner.name, &[ATT], 0, ""),
            )
            .await
            .with_status(StatusCode::CREATED);
        ids.push(resource_id(test, owner.id, &format!("imip-coll/{uid}.ics")).await);
    }
    // `delete_with_events_za` walks the children in ascending order, one
    // commit point after each: the first chunk holds the lower id.
    assert!(ids[0] < ids[1], "{ids:?}");
    wait_for_queue(test).await;
    sink().clear();
    let traced = Traced::start(&[EventType::Calendar(CalendarEvent::ItipMessageError)]);
    let split = ForceCommitSplit::new(owner.id);
    test_hooks::set_second_chunk_fail_once(Some(owner.id));
    let status = delete_raw(owner.name, STRONG, calendar.clone()).await;
    drop(split);
    test_hooks::set_second_chunk_fail_once(None);
    assert_eq!(status, 500, "the chunked DELETE did not fail");
    assert_eq!(
        uid_copies(test, owner.id, "imip-out-k1").await,
        0,
        "the first chunk's event stands deleted"
    );
    assert_eq!(
        uid_copies(test, owner.id, "imip-out-k2").await,
        1,
        "the second chunk's event remains"
    );
    wait_for_queue(test).await;
    assert_eq!(sink().count_to(ATT), 0, "the failed DELETE mailed a CANCEL");
    let mut dropped = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_IMIP_DROPPED && event.account_id == Some(owner.id as u64)
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
    dropped.sort_unstable();
    assert_eq!(dropped, ids, "one dropped trace per event of the batch");
    owner
        .client
        .request_with_headers(
            "DELETE",
            &format!("{calendar}imip-out-k2.ics"),
            [NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    owner
        .client
        .request("DELETE", &calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Review Focus 3.
async fn rf_3_alias_attendee(test: &TestServer, alias: &Party) {
    let copy = invited_by_org(test, alias, ALIAS_TWO, "imip-out-al").await;
    sink().clear();
    set_partstat(&alias.client, &copy, ALIAS_TWO, "ACCEPTED").await;
    wait_for_queue(test).await;
    let replies = sink().take_to(ORG);
    assert_eq!(replies.len(), 1, "one REPLY to the organizer");
    assert!(is_method_for(&replies[0], "REPLY", "imip-out-al"));
    assert_eq!(replies[0].mail_from, ALIAS_TWO);
    assert_eq!(replies[0].from_address().as_deref(), Some(ALIAS_TWO));
    alias
        .client
        .request_with_headers("DELETE", &copy, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Review Focus 5.
async fn rf_5_many_recipients(test: &TestServer, owner: &Party) {
    let attendees = (1..=20)
        .map(|n| format!("imip-many-{n:02}@remote.test"))
        .collect::<Vec<_>>();
    let path = own_path(owner, "imip-out-m");
    sink().clear();
    assert_eq!(
        put(
            owner.name,
            STRONG,
            path.clone(),
            event(
                "imip-out-m",
                owner.name,
                &attendees.iter().map(String::as_str).collect::<Vec<_>>(),
                0,
                ""
            )
        )
        .await,
        201,
        "the PUT did not answer 201 within 5 s"
    );
    for attendee in &attendees {
        assert_eq!(
            attendee_statuses(&owner.client, &path, attendee).await,
            ["1.1"],
            "{attendee}"
        );
    }
    wait_for_queue(test).await;
    let messages = sink().messages();
    assert_eq!(messages.len(), 20, "one email per attendee");
    let mut recipients = messages
        .iter()
        .map(|message| {
            assert_eq!(message.rcpt_to.len(), 1, "one recipient per email");
            message.rcpt_to[0].clone()
        })
        .collect::<Vec<_>>();
    recipients.sort();
    assert_eq!(recipients, attendees);
    owner
        .client
        .request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
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
