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
    smtp_sink::{self, ZA_SINK_PORT},
};
use ::registry::{
    schema::{
        enums::MtaProtocol,
        prelude::Property,
        structs::{Expression, ExpressionMatch, MtaOutboundStrategy, MtaRoute, MtaRouteRelay},
    },
    types::list::List,
};
use common::Server;
use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

pub mod outbound;

/// Outside parties (spec 8.2), mail-only addresses read at the sink.
pub const ORG: &str = "imip-org@remote.test";
pub const ATT: &str = "imip-att@remote.test";
pub const DELEGATE: &str = "imip-delegate@remote.test";
pub const NODMARC: &str = "imip-nodmarc@remote.test";
pub const STRANGER: &str = "imip-stranger@remote.test";

/// The sink's host name, resolved through the server's DNS cache.
const SINK_HOST: &str = "za-sink.test";

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
