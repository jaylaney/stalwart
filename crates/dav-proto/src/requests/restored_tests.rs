/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use calcard::vcard::VCardVersion;

use crate::{
    parser::{DavParser, Error, tokenizer::Tokenizer},
    schema::{
        property::{CardDavProperty, DavProperty},
        request::{Acl, LockInfo, MkCol, PropFind, PropertyUpdate, Report},
    },
};

#[test]
fn parse_address_data_version() {
    let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
            <C:addressbook-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:carddav">
              <D:prop>
                <C:address-data content-type="text/vcard" version="3.0">
                  <C:prop name="FN"/>
                </C:address-data>
              </D:prop>
            </C:addressbook-query>"#;

    let mut tokenizer = Tokenizer::new(xml.as_bytes());
    let report = Report::parse(&mut tokenizer).unwrap();
    let Report::AddressbookQuery(query) = report else {
        panic!("expected addressbook-query, got {report:?}");
    };
    let PropFind::Prop(properties) = query.properties else {
        panic!("expected prop, got {:?}", query.properties);
    };

    let version = properties.iter().find_map(|property| match property {
        DavProperty::CardDav(CardDavProperty::AddressData { version, .. }) => Some(*version),
        _ => None,
    });

    assert_eq!(version, Some(Some(VCardVersion::V3_0)));
}

#[test]
fn parse_requests() {
    for entry in std::fs::read_dir("resources/requests").unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().map(|ext| ext == "xml").unwrap_or(false) {
            println!("Parsing: {:?}", path);
            let filename = path.file_name().unwrap().to_str().unwrap();
            let xml = std::fs::read_to_string(&path).unwrap();
            let mut tokenizer = Tokenizer::new(xml.as_bytes());

            // This fixture binds a prefix to an empty namespace.
            if filename == "propfind-009.xml" {
                let Err(Error::Xml(error)) = PropFind::parse(&mut tokenizer) else {
                    panic!("Expected an XML namespace error in {filename}");
                };
                assert!(
                    matches!(*error, quick_xml::Error::Namespace(_)),
                    "Expected an XML namespace error in {filename}, got {error}"
                );
                continue;
            }

            match filename.split_once('-').unwrap().0 {
                "propfind" => {
                    serde_json::to_string_pretty(&PropFind::parse(&mut tokenizer).unwrap())
                }
                "propertyupdate" => {
                    serde_json::to_string_pretty(&PropertyUpdate::parse(&mut tokenizer).unwrap())
                }
                "mkcol" => serde_json::to_string_pretty(&MkCol::parse(&mut tokenizer).unwrap()),
                "lockinfo" => {
                    serde_json::to_string_pretty(&LockInfo::parse(&mut tokenizer).unwrap())
                }
                "report" => serde_json::to_string_pretty(&Report::parse(&mut tokenizer).unwrap()),
                "acl" => serde_json::to_string_pretty(&Acl::parse(&mut tokenizer).unwrap()),
                _ => {
                    panic!("Unknown method: {}", filename);
                }
            }
            .unwrap_or_else(|error| panic!("Could not serialize {filename}: {error}"));
        }
    }
}
