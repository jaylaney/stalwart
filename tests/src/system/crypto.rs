/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{jmap::JmapUtils, server::TestServer, smtp::SmtpConnection};
use base64::{Engine, engine::general_purpose::STANDARD};
use crypto::{
    EncryptOptions, KeyError, KeyKind, Profile, Protection, RecipientIds, RecipientSet,
    is_ciphertext, parse_public_keys, protection,
};
use mail_parser::MessageParser;
use registry::schema::{
    enums::HeaderProtection,
    prelude::{ObjectType, Property},
    structs::{AccountSettings, EncryptionAtRest, EncryptionSettings},
};
use serde_json::json;
use std::path::PathBuf;
use store::write::now;
use types::id::Id;

pub async fn test(test: &mut TestServer) {
    println!("Running Encryption-at-rest tests...");

    // Check encryption
    check_is_encrypted();
    import_certs_and_encrypt();

    // Create test account
    let account = test
        .create_user_account(
            "admin@example.org",
            "jdoe@example.org",
            "this is a very strong password",
            &[],
            "jdoe@example.org",
        )
        .await;
    let client = account.jmap_client().await;

    // Import all certs
    let mut cert_ids = Vec::new();
    let mut certs_parsed = Vec::new();
    for cert_file in ["cert_smime.pem", "cert_pgp.pem"] {
        let certs = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("crypto")
                .join(cert_file),
        )
        .unwrap();

        certs_parsed.push(parse_public_keys(certs.as_bytes(), now()).unwrap());

        let cert_id = account
            .registry_create_many(
                ObjectType::PublicKey,
                [json!({
                    Property::Description: "This is a public key",
                    Property::Key: certs
                })],
            )
            .await
            .created(0)
            .object_id();

        cert_ids.push(cert_id);
    }

    // Keep subjects visible for the delivery checks below.
    let setting = EncryptionAtRest::Enabled(EncryptionSettings {
        allow_spam_training: true,
        encrypt_on_append: true,
        public_key: cert_ids[1],
        header_protection: HeaderProtection::None,
    });
    account
        .registry_update_object(
            ObjectType::AccountSettings,
            Id::singleton(),
            json!({ Property::EncryptionAtRest: setting }),
        )
        .await;
    assert_eq!(
        account
            .registry_get::<AccountSettings>(Id::singleton())
            .await
            .encryption_at_rest,
        setting
    );
    let cached_account = test
        .server
        .account(account.id().document_id())
        .await
        .unwrap();
    let recipients = cached_account
        .encryption()
        .unwrap()
        .recipients(now())
        .unwrap();
    assert_eq!(recipients.kind(), KeyKind::OpenPgp);
    let expected_recipients = &certs_parsed[1].set;
    assert_eq!(recipients.key_type(), expected_recipients.key_type());
    assert_eq!(
        openpgp_recipient_ids(recipients),
        openpgp_recipient_ids(expected_recipients),
        "cached recipients must match the registered OpenPGP fixture"
    );

    // Send a new message, which should be encrypted
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest(
        "bill@example.org",
        &["jdoe@example.org"],
        concat!(
            "From: bill@example.org\r\n",
            "To: jdoe@example.org\r\n",
            "Subject: TPS Report (should be encrypted)\r\n",
            "\r\n",
            "I'm going to need those TPS reports ASAP. ",
            "So, if you could do that, that'd be great."
        ),
    )
    .await;

    // Send an encrypted message
    lmtp.ingest(
        "bill@example.org",
        &["jdoe@example.org"],
        concat!(
            "From: bill@example.org\r\n",
            "To: jdoe@example.org\r\n",
            "Subject: TPS Report (already encrypted)\r\n",
            "Content-Type: application/pkcs7-mime; name=\"smime.p7m\"; smime-type=enveloped-data\r\n",
            "\r\n",
            "xjMEZMYfNhYJKwYBBAHaRw8BAQdAYyTN1HzqapLw8xwkCGwa0OjsgT/JqhcB/+Dy",
            "Ga1fsBrNG0pvaG4gRG9lIDxqb2huQGV4YW1wbGUub3JnPsKJBBMWCAAxFiEEg836",
            "pwbXpuQ/THMtpJwd4oBfIrUFAmTGHzYCGwMECwkIBwUVCAkKCwUWAgMBAAAKCRCk",
            "nB3igF8itYhyAQD2jEdeYa3gyQ47X9YWZTK1wEJkN8W9//V1fYl2XQwqlQEA0qBv",
            "Ai6nUh99oDw+/zQ8DFIKdeb5Ti4tu/X58PdpiQ7OOARkxh82EgorBgEEAZdVAQUB",
            "AQdAvXz2FbFN0DovQF/ACnZyczTsSIQp0mvmF1PE+aijbC8DAQgHwngEGBYIACAW",
            "IQSDzfqnBtem5D9Mcy2knB3igF8itQUCZMYfNgIbDAAKCRCknB3igF8itRnoAQC3",
            "GzPmgx7TnB+SexPuJV/DoKSMJ0/X+hbEFcZkulxaDQEAh+xiJCvf+ZNAKw6kFhsL",
            "UuZhEDktxnY6Ehz3aB7FawA=",
            "=KGrr",
        ),
    )
    .await;

    // Disable encryption
    account
        .registry_update_object(
            ObjectType::AccountSettings,
            Id::singleton(),
            json!({
                Property::EncryptionAtRest: EncryptionAtRest::Disabled
            }),
        )
        .await;

    // Send a new message, which should NOT be encrypted
    lmtp.ingest(
        "bill@example.org",
        &["jdoe@example.org"],
        concat!(
            "From: bill@example.org\r\n",
            "To: jdoe@example.org\r\n",
            "Subject: TPS Report (plain text)\r\n",
            "\r\n",
            "I'm going to need those TPS reports ASAP. ",
            "So, if you could do that, that'd be great."
        ),
    )
    .await;

    // Check messages
    let mut request = client.build();
    request.get_email();
    let emails = request.send_get_email().await.unwrap().take_list();
    assert_eq!(emails.len(), 3, "3 messages were expected: {:#?}.", emails);

    for email in emails {
        let message =
            String::from_utf8(client.download(email.blob_id().unwrap()).await.unwrap()).unwrap();
        if message.contains("should be encrypted") {
            assert!(
                message.contains("Content-Type: multipart/encrypted"),
                "got message {message}, expected encrypted message"
            );
        } else if message.contains("already encrypted") {
            assert!(
                message.contains("Content-Type: application/pkcs7-mime")
                    && message.contains("xjMEZMYfNhYJKwYBBAHaRw8BAQdAYy"),
                "got message {message}, expected message to be left intact"
            );
        } else if message.contains("plain text") {
            assert!(
                message.contains("I'm going to need those TPS reports ASAP."),
                "got message {message}, expected plain text message"
            );
        } else {
            panic!("Unexpected message: {:#?}", message)
        }
    }

    test.account("admin@example.org")
        .destroy_account(account)
        .await;
    test.cleanup().await;
}

// The fixture check observes the actual keys used to wrap a session key. PKESK
// recipient IDs are visible even though the message and session key are encrypted.
fn openpgp_recipient_ids(recipients: &RecipientSet) -> Vec<Vec<u8>> {
    let message = MessageParser::new()
        .parse(b"Subject: recipient check\r\n\r\nfixture")
        .unwrap();
    let encrypted = recipients
        .encrypt(
            &message,
            &EncryptOptions {
                // Modern includes all usable keys, including post-quantum keys
                // that Compatible can omit when a certificate also has a classical key.
                profile: Profile::Modern,
                recipient_ids: RecipientIds::Real,
                ..Default::default()
            },
        )
        .unwrap();
    let encrypted = MessageParser::new().parse(&encrypted.message).unwrap();
    let armor = encrypted.part(2).unwrap().decoded();
    let armor = std::str::from_utf8(&armor).unwrap().trim_ascii();
    let encoded: String = armor
        .strip_prefix("-----BEGIN PGP MESSAGE-----\r\n\r\n")
        .unwrap()
        .strip_suffix("-----END PGP MESSAGE-----")
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('='))
        .collect();
    let bytes = STANDARD.decode(encoded).unwrap();

    // Read only the leading Public-Key Encrypted Session Key (PKESK)
    // packets. Stalwart writes them with new-format packet headers.
    const PKESK: u8 = 0xc1;
    const SEIPD: u8 = 0xd2;
    let mut packets = bytes.as_slice();
    let mut ids = Vec::new();
    while let [PKESK, length @ ..] = packets {
        let (len, rest) = match length {
            [a, rest @ ..] if *a < 192 => (usize::from(*a), rest),
            [a, b, rest @ ..] if *a < 224 => {
                (((usize::from(*a) - 192) << 8) + usize::from(*b) + 192, rest)
            }
            [255, a, b, c, d, rest @ ..] => (u32::from_be_bytes([*a, *b, *c, *d]) as usize, rest),
            _ => panic!("unexpected PKESK packet length"),
        };
        let (body, rest) = rest.split_at_checked(len).unwrap();
        let id = match body {
            [3, recipient @ ..] => recipient.get(..8),
            [6, len @ (21 | 33), recipient @ ..] => recipient.get(..usize::from(*len)),
            _ => None,
        }
        .expect("PKESK must carry a real key ID or fingerprint");
        assert!(id.iter().any(|byte| *byte != 0));
        ids.push(id.to_vec());
        packets = rest;
    }
    assert!(!ids.is_empty(), "OpenPGP message has no recipient packets");
    assert_eq!(packets.first(), Some(&SEIPD));
    ids.sort_unstable();
    ids
}

pub fn import_certs_and_encrypt() {
    let now = now();
    for (cert, kind) in [
        (
            include_bytes!("../../resources/crypto/cert_pgp.pem").as_slice(),
            KeyKind::OpenPgp,
        ),
        (
            include_bytes!("../../resources/crypto/cert_smime.pem").as_slice(),
            KeyKind::Smime,
        ),
    ] {
        let params = parse_public_keys(cert, now).unwrap();
        assert_eq!(params.kind, kind);

        let message = MessageParser::new()
            .parse(b"Subject: test\r\ntest\r\n")
            .unwrap();
        assert_eq!(protection(&message), Protection::None);
        let encrypted = params
            .set
            .encrypt(
                &message,
                &EncryptOptions {
                    profile: params.set.inferred_profile(false),
                    ..Default::default()
                },
            )
            .unwrap();
        let encrypted = MessageParser::new().parse(&encrypted.message).unwrap();
        assert_eq!(protection(&encrypted), Protection::Encrypted);
        assert!(is_ciphertext(&encrypted));
    }

    // S/MIME and PGP should not be allowed mixed.
    assert_eq!(
        parse_public_keys(include_bytes!("../../resources/crypto/cert_mixed.pem"), now)
            .unwrap_err(),
        KeyError::MixedKeyKinds
    );
}

pub fn check_is_encrypted() {
    let messages = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("crypto")
            .join("is_encrypted.txt"),
    )
    .unwrap();

    for raw_message in messages.split("!!!") {
        let message = MessageParser::new()
            .parse(raw_message.trim().as_bytes())
            .unwrap();
        assert!(message.content_type().is_some());
        let expected = match message.subject() {
            Some("Encrypted") => Protection::Encrypted,
            Some("Signed") => Protection::Signed,
            Some("None") => Protection::None,
            subject => panic!("unknown protection fixture label: {subject:?}"),
        };
        assert_eq!(protection(&message), expected, "failed for {raw_message}");
    }
}
