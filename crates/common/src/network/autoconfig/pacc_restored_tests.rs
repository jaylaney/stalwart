/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

/// The full example from Section 4.1 of the draft.
const EXAMPLE_JSON: &str = r#"{
    "protocols": {
        "jmap": {
            "url": "https://jmap.example.com/session"
        },
        "imap": {
            "host": "imap.example.com"
        },
        "pop3": {
            "host": "pop3.example.com"
        },
        "smtp": {
            "host": "smtp.example.com"
        },
        "caldav": {
            "url": "https://sync.example.com/calendar/"
        },
        "carddav": {
            "url": "https://sync.example.com/contacts/"
        }
    },
    "authentication": {
        "oauth-public": {
            "issuer": "https://auth.example.com/"
        },
        "password": true
    },
    "info": {
        "provider": {
            "name": "Example Provider Name",
            "shortName": "Example",
            "logo": [
                {
                    "url": "https://www.example.net/logo.svg",
                    "content-type": "image/svg+xml"
                }
            ]
        },
        "help": {
            "documentation": "https://help.example.net/howto/set-up-your-mail-app.html",
            "developer": "https://developer.example.net/client-apps/",
            "contact": ["mailto:it@team.example.net"]
        }
    }
}"#;

#[test]
fn deserialize_full_example() {
    let config: Configuration = serde_json::from_str(EXAMPLE_JSON).expect("failed to deserialize");

    // Protocols
    assert_eq!(
        config.protocols.jmap.as_ref().unwrap().url,
        "https://jmap.example.com/session"
    );
    assert_eq!(
        config.protocols.imap.as_ref().unwrap().host,
        "imap.example.com"
    );
    assert_eq!(
        config.protocols.smtp.as_ref().unwrap().host,
        "smtp.example.com"
    );
    assert_eq!(
        config.protocols.pop3.as_ref().unwrap().host,
        "pop3.example.com"
    );
    assert_eq!(
        config.protocols.caldav.as_ref().unwrap().url,
        "https://sync.example.com/calendar/"
    );
    assert_eq!(
        config.protocols.carddav.as_ref().unwrap().url,
        "https://sync.example.com/contacts/"
    );
    assert!(config.protocols.webdav.is_none());
    assert!(config.protocols.managesieve.is_none());

    // Authentication
    let auth = config.authentication.as_ref().unwrap();
    assert!(auth.password);
    assert_eq!(
        auth.oauth_public.as_ref().unwrap().issuer,
        "https://auth.example.com/"
    );

    // Info
    assert_eq!(config.info.provider.name, "Example Provider Name");
    assert_eq!(config.info.provider.short_name.as_deref(), Some("Example"));

    let logos = config.info.provider.logo.as_ref().unwrap();
    assert_eq!(logos.len(), 1);
    assert_eq!(logos[0].content_type, "image/svg+xml");
    assert!(logos[0].width.is_none());

    let help = config.info.help.as_ref().unwrap();
    assert_eq!(
        help.documentation.as_deref(),
        Some("https://help.example.net/howto/set-up-your-mail-app.html")
    );
    assert_eq!(
        help.contact.as_ref().unwrap(),
        &["mailto:it@team.example.net"]
    );
}

#[test]
fn roundtrip() {
    let config: Configuration = serde_json::from_str(EXAMPLE_JSON).expect("failed to deserialize");
    let serialized = serde_json::to_string_pretty(&config).expect("failed to serialize");
    let roundtripped: Configuration =
        serde_json::from_str(&serialized).expect("failed to re-deserialize");
    assert_eq!(config, roundtripped);
}

#[test]
fn minimal_config() {
    let json = r#"{
            "protocols": {},
            "info": {
                "provider": {
                    "name": "Minimal"
                }
            }
        }"#;
    let config: Configuration = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(config.info.provider.name, "Minimal");
    assert!(config.authentication.is_none());
    assert!(config.protocols.jmap.is_none());
}

#[test]
fn ignores_unknown_properties() {
    let json = r#"{
            "protocols": {
                "imap": { "host": "imap.example.com" },
                "future-protocol": { "endpoint": "wss://example.com" }
            },
            "info": {
                "provider": { "name": "Test" }
            },
            "futureField": 42
        }"#;
    let config: Configuration = serde_json::from_str(json).expect("should ignore unknowns");
    assert_eq!(
        config.protocols.imap.as_ref().unwrap().host,
        "imap.example.com"
    );
}

#[test]
fn logo_with_dimensions() {
    let json = r#"{
            "protocols": {},
            "info": {
                "provider": {
                    "name": "Test",
                    "logo": [
                        {
                            "url": "https://example.com/logo.svg",
                            "content-type": "image/svg+xml"
                        },
                        {
                            "url": "https://example.com/logo-128.png",
                            "content-type": "image/png",
                            "width": 128,
                            "height": 128
                        },
                        {
                            "url": "https://example.com/logo-512.png",
                            "content-type": "image/png",
                            "width": 512,
                            "height": 512
                        }
                    ]
                }
            }
        }"#;
    let config: Configuration = serde_json::from_str(json).unwrap();
    let logos = config.info.provider.logo.as_ref().unwrap();
    assert_eq!(logos.len(), 3);
    assert!(logos[0].width.is_none());
    assert_eq!(logos[1].width, Some(128));
    assert_eq!(logos[2].height, Some(512));
}

#[test]
fn password_only_auth() {
    let json = r#"{
            "protocols": { "imap": { "host": "mail.example.com" } },
            "authentication": { "password": true },
            "info": { "provider": { "name": "PW Only" } }
        }"#;
    let config: Configuration = serde_json::from_str(json).unwrap();
    let auth = config.authentication.unwrap();
    assert!(auth.password);
    assert!(auth.oauth_public.is_none());
}
