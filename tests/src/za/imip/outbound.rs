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
    za::{STRONG, fanout::Party, user_permissions},
};
use ::registry::{schema::structs::TaskCalendarItipContents, types::map::Map};
use common::za::{
    imip::{ZaImipMail, ZaImipOutcome, za_imip_submit},
    test_hooks,
};
use groupware::scheduling::ItipSummary;

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
}
