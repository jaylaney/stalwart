/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{AutodiscoverRequest, ResponseSchema, parse_autodiscover_request};

#[test]
fn parse_autodiscover() {
    const OUTLOOK: &str =
        "http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a";
    const MOBILESYNC: &str =
        "http://schemas.microsoft.com/exchange/autodiscover/mobilesync/responseschema/2006";

    for (request, expected) in [
        (
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
            <Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006">
                <Request>
                        <EMailAddress>Email@Example.com</EMailAddress>
                        <AcceptableResponseSchema>{OUTLOOK}</AcceptableResponseSchema>
                </Request>
            </Autodiscover>"#
            ),
            ResponseSchema::Outlook,
        ),
        (
            format!(
                r#"<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006">
                <Request>
                        <AcceptableResponseSchema>{OUTLOOK}</AcceptableResponseSchema>
                        <EMailAddress>email@example.com</EMailAddress>
                </Request>
            </Autodiscover>"#
            ),
            ResponseSchema::Outlook,
        ),
        (
            r#"<Autodiscover>
                <Request>
                        <EMailAddress>email@example.com</EMailAddress>
                </Request>
            </Autodiscover>"#
                .to_string(),
            ResponseSchema::Outlook,
        ),
        (
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
            <Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/mobilesync/requestschema/2006">
                <Request>
                        <EMailAddress>email@example.com</EMailAddress>
                        <AcceptableResponseSchema>{MOBILESYNC}</AcceptableResponseSchema>
                </Request>
            </Autodiscover>"#
            ),
            ResponseSchema::Unsupported,
        ),
        (
            format!(
                r#"<Autodiscover>
                <Request>
                        <LegacyDN>/o=Example/ou=Users/cn=email</LegacyDN>
                        <Unknown><Nested>value</Nested><Empty/></Unknown>
                        <AcceptableResponseSchema>{MOBILESYNC}</AcceptableResponseSchema>
                        <EMailAddress>email@example.com</EMailAddress>
                </Request>
            </Autodiscover>"#
            ),
            ResponseSchema::Unsupported,
        ),
    ] {
        assert_eq!(
            parse_autodiscover_request(request.as_bytes()).expect("valid request"),
            AutodiscoverRequest {
                email: "email@example.com".to_string(),
                response_schema: expected,
            },
            "{request}"
        );
    }

    for request in [
        "",
        "<Autodiscover><Request></Request></Autodiscover>",
        "<Autodiscover><Request><EMailAddress>no-domain</EMailAddress></Request></Autodiscover>",
        "<Autodiscover><Request><EMailAddress>email@example.com</Request></Autodiscover>",
        "<Request><EMailAddress>email@example.com</EMailAddress></Request>",
    ] {
        assert!(
            parse_autodiscover_request(request.as_bytes()).is_err(),
            "{request}"
        );
    }
}

#[test]
fn autodiscover_encryption() {
    use registry::schema::{enums::ServiceProtocol, structs::Service};
    use utils::map::vec_map::VecMap;

    fn tag<'x>(block: &'x str, name: &str) -> &'x str {
        block
            .split_once(&format!("<{name}>"))
            .and_then(|(_, rest)| rest.split_once(&format!("</{name}>")))
            .map(|(value, _)| value)
            .unwrap()
    }

    for (cleartext, expected) in [
        (
            false,
            vec![
                ("IMAP", "993", "on", "SSL"),
                ("POP3", "995", "on", "SSL"),
                ("SMTP", "465", "on", "SSL"),
            ],
        ),
        (
            true,
            vec![
                ("IMAP", "993", "on", "SSL"),
                ("IMAP", "143", "off", "TLS"),
                ("POP3", "995", "on", "SSL"),
                ("POP3", "110", "off", "TLS"),
                ("SMTP", "465", "on", "SSL"),
                ("SMTP", "587", "off", "TLS"),
            ],
        ),
    ] {
        let services: VecMap<ServiceProtocol, Service> = [
            ServiceProtocol::Imap,
            ServiceProtocol::Pop3,
            ServiceProtocol::Smtp,
            ServiceProtocol::Jmap,
        ]
        .into_iter()
        .map(|protocol| {
            (
                protocol,
                Service {
                    hostname: None,
                    cleartext,
                },
            )
        })
        .collect();
        let response =
            super::build_autodiscover_response("user@example.com", "mail.example.com", &services);

        assert_eq!(
            response
                .split("<Protocol>")
                .skip(1)
                .map(|block| (
                    tag(block, "Type"),
                    tag(block, "Port"),
                    tag(block, "SSL"),
                    tag(block, "Encryption"),
                ))
                .collect::<Vec<_>>(),
            expected,
            "cleartext: {cleartext}"
        );
    }
}
