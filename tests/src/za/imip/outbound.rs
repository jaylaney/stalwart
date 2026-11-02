/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, outbound (iMIP spec 5): a key account's scheduling messages
//! to the mail class leave as upstream's email through the SMTP queue, read
//! back at the suite's SMTP sink.

use super::ATT;
use crate::{
    utils::{
        server::TestServer,
        smtp_sink::sink,
        za::{queued_recipients, wait_for_queue},
    },
    za::{
        STRONG,
        fanout::Party,
        mail::{Traced, TracedEvent},
        user_permissions,
    },
};
use ::registry::{schema::structs::TaskCalendarItipContents, types::map::Map};
use common::za::{
    imip::{ZaImipMail, ZaImipOutcome, za_imip_submit},
    test_hooks,
};
use groupware::scheduling::ItipSummary;
use std::collections::HashSet;
use trc::{CalendarEvent, EventType, Key};

/// The key organizer of this module's cases.
pub const OUT_KEY: &str = "imip-out-key@example.com";

/// The seam case's iTIP REQUEST from `OUT_KEY` to `ATT`.
const SEAM_ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//imip seam//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:imip-out-seam\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSUMMARY:Seam\r\nORGANIZER:mailto:imip-out-key@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP outbound tests...");
    let account = test
        .account("admin@example.com")
        .create_key_user_account(OUT_KEY, STRONG, "iMIP out key", &[], user_permissions())
        .await;
    test.insert_account(account);
    let owner = Party::new(test, OUT_KEY);

    seam(test, &owner).await;
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
