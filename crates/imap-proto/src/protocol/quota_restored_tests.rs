/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{ImapResponse, capability::QuotaResourceName};

use super::{QuotaItem, QuotaResource};

#[test]
fn serialize_quota() {
    for (response, expected) in [
        (
            super::Response {
                quota_root_items: vec!["INBOX".into(), "#test".into()],
                quota_items: vec![],
            },
            "* QUOTAROOT \"INBOX\" \"#test\"\r\n",
        ),
        (
            super::Response {
                quota_root_items: vec![],
                quota_items: vec![QuotaItem {
                    name: "INBOX".into(),
                    resources: vec![QuotaResource {
                        resource: QuotaResourceName::Storage,
                        total: 1073741824,
                        used: 1048576,
                    }],
                }],
            },
            "* QUOTA \"INBOX\" (STORAGE 1024 1048576)\r\n",
        ),
        (
            super::Response {
                quota_root_items: vec!["my mailbox".into(), "".into()],
                quota_items: vec![QuotaItem {
                    name: "INBOX".into(),
                    resources: vec![
                        QuotaResource {
                            resource: QuotaResourceName::Storage,
                            total: 1073741824,
                            used: 1048576,
                        },
                        QuotaResource {
                            resource: QuotaResourceName::Message,
                            total: 100,
                            used: 2,
                        },
                    ],
                }],
            },
            concat!(
                "* QUOTAROOT \"my mailbox\" \"\"\r\n",
                "* QUOTA \"INBOX\" (STORAGE 1024 1048576 MESSAGE 2 100)\r\n"
            ),
        ),
    ] {
        assert_eq!(String::from_utf8(response.serialize()).unwrap(), expected);
    }
}
