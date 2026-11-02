/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::queue::{Message, QueueId, Recipient};
use common::config::smtp::queue::QueueName;
use std::net::{IpAddr, Ipv4Addr};

fn wrapper(recipients: &[&str]) -> MessageWrapper {
    MessageWrapper {
        queue_id: 0 as QueueId,
        queue_name: QueueName::default(),
        is_multi_queue: false,
        span_id: 0,
        message: Message {
            created: 0,
            blob_hash: Default::default(),
            return_path: "sender@example.com".into(),
            recipients: recipients.iter().map(Recipient::new).collect(),
            received_from_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            received_via_port: 0,
            flags: 0,
            env_id: None,
            priority: 0,
            size: 0,
            metadata: Default::default(),
        },
    }
}

fn split(headers: &str, recipients: &[&str]) -> (Vec<String>, Vec<String>) {
    let raw = format!("{headers}\r\nSubject: test\r\n\r\nbody\r\n");
    let auth = AuthenticatedMessage::parse(raw.as_bytes()).expect("parse message");
    let message = wrapper(recipients);
    let envelopes = message.undisclosed_recipients(&auth);
    let mut disclosed = envelopes
        .disclosed_recipients
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut undisclosed = envelopes
        .undisclosed_recipients
        .iter()
        .map(|(_, s)| s.to_string())
        .collect::<Vec<_>>();
    disclosed.sort();
    undisclosed.sort();
    (disclosed, undisclosed)
}

#[test]
fn single_recipient_is_always_disclosed() {
    let (disclosed, undisclosed) = split("To: someone-else@example.com", &["bcc@example.org"]);
    assert_eq!(disclosed, vec!["bcc@example.org".to_string()]);
    assert!(undisclosed.is_empty());
}

#[test]
fn all_recipients_disclosed() {
    let (disclosed, undisclosed) = split(
        "To: alice@example.com, bob@example.com\r\nCc: carol@example.com",
        &["alice@example.com", "bob@example.com", "carol@example.com"],
    );
    assert_eq!(
        disclosed,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string(),
            "carol@example.com".to_string(),
        ]
    );
    assert!(undisclosed.is_empty());
}

#[test]
fn mixed_disclosed_and_undisclosed() {
    let (disclosed, undisclosed) = split(
        "To: alice@example.com\r\nCc: bob@example.com",
        &[
            "alice@example.com",
            "bob@example.com",
            "eve@secret.example.org",
            "mallory@secret.example.org",
        ],
    );
    assert_eq!(
        disclosed,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string()
        ]
    );
    assert_eq!(
        undisclosed,
        vec![
            "eve@secret.example.org".to_string(),
            "mallory@secret.example.org".to_string(),
        ]
    );
}

#[test]
fn no_to_or_cc_header_all_undisclosed() {
    let (disclosed, undisclosed) = split(
        "From: sender@example.com",
        &["eve@example.org", "mallory@example.org"],
    );
    assert!(disclosed.is_empty());
    assert_eq!(
        undisclosed,
        vec![
            "eve@example.org".to_string(),
            "mallory@example.org".to_string()
        ]
    );
}

#[test]
fn group_addresses_are_disclosed() {
    let (disclosed, undisclosed) = split(
        "To: Team:alice@example.com,bob@example.com;",
        &["alice@example.com", "bob@example.com", "eve@example.org"],
    );
    assert_eq!(
        disclosed,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string()
        ]
    );
    assert_eq!(undisclosed, vec!["eve@example.org".to_string()]);
}

#[test]
fn header_address_casing_is_ignored() {
    let (disclosed, undisclosed) = split(
        "To: Alice@Example.COM, BOB@EXAMPLE.com",
        &["alice@example.com", "bob@example.com", "eve@example.org"],
    );
    assert_eq!(
        disclosed,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string()
        ]
    );
    assert_eq!(undisclosed, vec!["eve@example.org".to_string()]);
}

#[test]
fn display_names_and_brackets_are_ignored() {
    let (disclosed, undisclosed) = split(
        "To: \"Alice Doe\" <alice@example.com>\r\nCc: Bob <bob@example.com>",
        &["alice@example.com", "bob@example.com", "eve@example.org"],
    );
    assert_eq!(
        disclosed,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string()
        ]
    );
    assert_eq!(undisclosed, vec!["eve@example.org".to_string()]);
}
