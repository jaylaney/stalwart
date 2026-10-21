/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 3: mail encryption at rest for key accounts (mail spec). Each
//! module creates the `keymail<n>@example.com` accounts it names and leaves
//! no message, mailbox, app password or queued mail behind.

use super::user_permissions;
use crate::utils::{
    account::Account, server::TestServer, smtp::SmtpConnection, za::wait_for_delivery,
};

pub mod login;

/// A fresh key account, registered with the test server.
pub async fn key_mail_account(
    test: &mut TestServer,
    name: &'static str,
    password: &'static str,
) -> Account {
    let account = test
        .account("admin@example.com")
        .create_key_user_account(name, password, "Key mail", &[], user_permissions())
        .await;
    test.insert_account(account.clone());
    account
}

/// One message delivered over LMTP (the real queue and local delivery),
/// waited for until the queue and the index tasks are done.
pub async fn deliver(test: &TestServer, from: &str, to: &str, message: &str) {
    SmtpConnection::connect()
        .await
        .ingest(from, &[to], message)
        .await;
    wait_for_delivery(test).await;
}

/// A minimal RFC 5322 message with a unique `Message-ID`.
pub fn message_with(subject: &str, body: &str) -> String {
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{:016x}.za-mail@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         \r\n\
         {body}\r\n",
        store::rand::random::<u64>()
    )
}

/// The `SessionKeys` of `account_id` alive in the server's process
/// (instances, not `Arc` references).
pub fn live_keys(account_id: u32) -> usize {
    vault::session::live::session_keys(account_id)
}
