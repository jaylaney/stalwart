/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{
    ImapResponse,
    capability::{Capability, Response},
};

#[test]
fn serialize_capability() {
    assert_eq!(
        &Response {
            capabilities: vec![
                Capability::IMAP4rev2,
                Capability::StartTLS,
                Capability::LoginDisabled
            ],
        }
        .serialize(),
        "* CAPABILITY IMAP4rev2 STARTTLS LOGINDISABLED\r\n".as_bytes()
    );
}
