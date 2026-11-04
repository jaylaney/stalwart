/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5: invitations by email (iMIP spec). Outside parties are
//! addresses at `remote.test`, not accounts: the suite routes every
//! non-local recipient to its SMTP sink (`route_outside_mail`).

use crate::utils::{
    account::Account,
    dns::DnsCache,
    smtp::SmtpConnection,
    smtp_sink::{self, SinkMessage, ZA_SINK_PORT},
    webdav::DummyWebDavClient,
};
use ::registry::{
    schema::{
        enums::{MtaInboundThrottleKey, MtaProtocol},
        prelude::{ObjectType, Property},
        structs::{
            Expression, ExpressionMatch, MtaInboundThrottle, MtaOutboundStrategy, MtaRoute,
            MtaRouteRelay,
        },
    },
    types::list::List,
};
use common::Server;
use hyper::StatusCode;
use serde_json::json;
use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};
use types::id::Id;

pub mod inbound;
pub mod oracle;
pub mod outbound;
pub mod traces;

/// Outside parties (spec 8.2), mail-only addresses read at the sink.
pub const ORG: &str = "imip-org@remote.test";
pub const ATT: &str = "imip-att@remote.test";
pub const DELEGATE: &str = "imip-delegate@remote.test";
pub const NODMARC: &str = "imip-nodmarc@remote.test";
pub const STRANGER: &str = "imip-stranger@remote.test";

/// The sink's host name, resolved through the server's DNS cache.
const SINK_HOST: &str = "za-sink.test";

/// The MIME boundary of `itip_mail`'s messages.
const BOUNDARY: &str = "za-imip-boundary";

/// A VCALENDAR with one VEVENT in 2099, organized by `organizer`, without
/// METHOD; each of `attendees` is an `ATTENDEE;PARTSTAT=NEEDS-ACTION;
/// RSVP=TRUE`. `extra` is inserted before `END:VEVENT` (a line break is
/// added when it does not end with one).
pub fn event(uid: &str, organizer: &str, attendees: &[&str], sequence: u32, extra: &str) -> String {
    let mut ics = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//imip//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990110T090000Z\r\n",
            "DTEND:20990110T100000Z\r\nSEQUENCE:{sequence}\r\nSUMMARY:imip {uid}\r\n",
            "ORGANIZER:mailto:{organizer}\r\n"
        ),
        uid = uid,
        sequence = sequence,
        organizer = organizer
    );
    for attendee in attendees {
        ics.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    if !extra.is_empty() {
        ics.push_str(extra);
        if !extra.ends_with("\r\n") {
            ics.push_str("\r\n");
        }
    }
    ics.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ics
}

/// `ics` with `METHOD:<method>` as the root's first property.
pub fn with_method(ics: &str, method: &str) -> String {
    ics.replacen(
        "BEGIN:VCALENDAR\r\n",
        &format!("BEGIN:VCALENDAR\r\nMETHOD:{method}\r\n"),
        1,
    )
}

/// An iMIP message from `from_header` (the From field's value) to `to`:
/// `multipart/mixed` with a `text/plain` part, then a `text/calendar;
/// method=<method>; charset=utf-8` part holding `ics`, given the root
/// `METHOD` (`with_method`) when it has none, as an iTIP message carries
/// one. The calendar part's text is that ICS exactly.
pub fn itip_mail(from_header: &str, to: &str, method: &str, ics: &str) -> String {
    let ics = if ics.contains("\r\nMETHOD:") {
        ics.to_string()
    } else {
        with_method(ics, method)
    };
    format!(
        concat!(
            "From: {from}\r\nTo: <{to}>\r\nSubject: iMIP {method}\r\n",
            "Message-ID: <{id:016x}.za-imip@remote.test>\r\n",
            "Date: Mon, 02 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n",
            "--{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n",
            "An invitation by email.\r\n",
            "--{boundary}\r\nContent-Type: text/calendar; method={method}; charset=utf-8\r\n\r\n",
            "{ics}\r\n--{boundary}--\r\n"
        ),
        from = from_header,
        to = to,
        method = method,
        id = store::rand::random::<u64>(),
        boundary = BOUNDARY,
        ics = ics
    )
}

/// Spec 8.2: delivers `message` to `to` over LMTP (port 11200) with MAIL
/// FROM `envelope_from`, where a `dmarc-...` address passes the test-mode
/// DMARC check. The caller waits for the queue.
pub async fn deliver(envelope_from: &str, to: &str, message: &str) {
    SmtpConnection::connect()
        .await
        .ingest(envelope_from, &[to], message)
        .await;
}

/// A ContactCard for `email` in `account`'s default address book (a CardDAV
/// PUT); returns its href.
pub async fn add_contact(client: &DummyWebDavClient, account: &str, email: &str) -> String {
    let uid = format!("imip-card-{}", email.replace(['@', '.'], "-"));
    let href = format!(
        "/dav/card/{}/default/{uid}.vcf",
        account.replace('@', "%40")
    );
    client
        .request_with_headers(
            "PUT",
            &href,
            [("content-type", "text/vcard; charset=utf-8")],
            format!(
                "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{uid}\r\nFN:{email}\r\nEMAIL:{email}\r\nEND:VCARD\r\n"
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    href
}

/// Whether a message the sink received is an iTIP REQUEST for `uid`.
pub fn is_request_for(message: &SinkMessage, uid: &str) -> bool {
    let uid_line = format!("UID:{uid}");
    message.calendar_method().as_deref() == Some("REQUEST")
        && message.calendar_part().is_some_and(|ics| {
            ics.replace("\r\n ", "")
                .lines()
                .any(|line| line.trim_end() == uid_line)
        })
}

/// The enabled inbound throttles keyed by recipient (upstream's default
/// "Sender address to recipient throttle" among them, 25 messages an hour
/// per sender domain and recipient), for the modules that mail one
/// recipient more often (`imip::inbound`, `imip::oracle`).
pub async fn rcpt_throttles(admin: &Account) -> Vec<Id> {
    admin
        .registry_get_all::<MtaInboundThrottle>()
        .await
        .into_iter()
        .filter(|(_, throttle)| {
            throttle.enable
                && throttle
                    .key
                    .iter()
                    .any(|key| *key == MtaInboundThrottleKey::Rcpt)
        })
        .map(|(id, _)| id)
        .collect()
}

/// Enables or disables the throttles `ids`, then reloads the settings.
pub async fn set_throttles(admin: &Account, ids: &[Id], enable: bool) {
    for id in ids {
        admin
            .registry_update_object(
                ObjectType::MtaInboundThrottle,
                *id,
                json!({ "enable": enable }),
            )
            .await;
    }
    admin.reload_settings().await;
}

/// Deletes the ContactCard whose href `add_contact` returned.
pub async fn delete_contact(client: &DummyWebDavClient, href: &str) {
    client
        .request("DELETE", href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 8.2: starts the suite's SMTP sink and routes every recipient outside
/// the local domains to it through a relay route; local recipients keep
/// upstream's `'local'` route. Only the outbound strategy's route changes
/// (its schedule, TLS and connection keep their defaults); the caller
/// reloads the settings.
pub async fn route_outside_mail(admin: &Account, server: &Server) {
    smtp_sink::start(ZA_SINK_PORT).await;
    server.ipv4_add(
        SINK_HOST,
        vec![Ipv4Addr::LOCALHOST],
        Instant::now() + Duration::from_secs(24 * 60 * 60),
    );
    admin
        .registry_create_object(MtaRoute::Relay(MtaRouteRelay {
            name: "za-sink".into(),
            host: SINK_HOST.into(),
            port: ZA_SINK_PORT.into(),
            protocol: MtaProtocol::Smtp,
            implicit_tls: false,
            allow_invalid_certs: true,
            ..Default::default()
        }))
        .await;
    admin
        .registry_update_setting(
            MtaOutboundStrategy {
                route: Expression {
                    match_: List::from_iter([ExpressionMatch {
                        if_: "is_local_domain(rcpt_domain)".into(),
                        then: "'local'".into(),
                    }]),
                    else_: "'za-sink'".into(),
                },
                ..Default::default()
            },
            &[Property::Route],
        )
        .await;
}
