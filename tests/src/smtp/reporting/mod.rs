/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod analyze;
pub mod dmarc;
pub mod scheduler;
pub mod tls;

use smtp_report::dmarc::{Disposition, DmarcStatus, Record};
use std::net::IpAddr;

/// The builder of 0.16's `mail_auth::report::Record`, over 1.0's
/// `smtp_report::dmarc::Record`, whose fields are public and which has no
/// builder of its own.
pub trait DmarcRecordBuilder: Sized {
    fn with_source_ip(self, source_ip: IpAddr) -> Self;
    fn with_action_disposition(self, disposition: Disposition) -> Self;
    fn with_dmarc_dkim_result(self, result: DmarcStatus) -> Self;
    fn with_dmarc_spf_result(self, result: DmarcStatus) -> Self;
    fn with_envelope_from(self, envelope_from: &str) -> Self;
    fn with_envelope_to(self, envelope_to: &str) -> Self;
    fn with_header_from(self, header_from: &str) -> Self;
}

impl DmarcRecordBuilder for Record {
    fn with_source_ip(mut self, source_ip: IpAddr) -> Self {
        self.row.source_ip = Some(source_ip);
        self
    }

    fn with_action_disposition(mut self, disposition: Disposition) -> Self {
        self.row.policy_evaluated.disposition = disposition;
        self
    }

    fn with_dmarc_dkim_result(mut self, result: DmarcStatus) -> Self {
        self.row.policy_evaluated.dkim = result;
        self
    }

    fn with_dmarc_spf_result(mut self, result: DmarcStatus) -> Self {
        self.row.policy_evaluated.spf = result;
        self
    }

    fn with_envelope_from(mut self, envelope_from: &str) -> Self {
        self.identifiers.envelope_from = envelope_from.to_string();
        self
    }

    fn with_envelope_to(mut self, envelope_to: &str) -> Self {
        self.identifiers.envelope_to = Some(envelope_to.to_string());
        self
    }

    fn with_header_from(mut self, header_from: &str) -> Self {
        self.identifiers.header_from = header_from.to_string();
        self
    }
}
