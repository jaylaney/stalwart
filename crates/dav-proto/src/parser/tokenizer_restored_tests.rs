/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::borrow::Cow;

use crate::schema::{Collation, MatchType, Namespace};

use super::*;

#[derive(Debug, PartialEq, Eq)]
pub enum TestToken<'x> {
    ElementStart(NamedElement),
    ElementEnd,
    Attribute(Attribute<String>),
    Bytes(Cow<'x, [u8]>),
    Text(Cow<'x, str>),
}

#[test]
fn test_tokenizer() {
    for (input, expected) in [
        (
            r#"<?xml version="1.0" encoding="utf-8" ?>
   <C:calendar-query xmlns:D="DAV:"
                    xmlns:C="urn:ietf:params:xml:ns:caldav">
    <D:prop>
      <D:getetag/>
      <C:calendar-data/>
    </D:prop>
    <C:filter>
      <C:comp-filter name="VCALENDAR"/>
    </C:filter>
   </C:calendar-query>"#,
            vec![
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CalDav,
                    element: Element::CalendarQuery,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::Dav,
                    element: Element::Prop,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::Dav,
                    element: Element::Getetag,
                }),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CalDav,
                    element: Element::CalendarData,
                }),
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CalDav,
                    element: Element::Filter,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CalDav,
                    element: Element::CompFilter,
                }),
                TestToken::Attribute(Attribute::Name("VCALENDAR".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementEnd,
            ],
        ),
        (
            r#" <?xml version="1.0" encoding="utf-8" ?>
   <C:addressbook-query xmlns:D="DAV:"
                     xmlns:C="urn:ietf:params:xml:ns:carddav">
     <D:prop>
       <D:getetag/>
       <C:address-data>
         <C:prop name="VERSION"/>
         <C:prop name="UID"/>
         <C:prop name="NICKNAME"/>
         <C:prop name="EMAIL"/>
         <C:prop name="FN"/>
       </C:address-data>
     </D:prop>
     <C:filter>
       <C:prop-filter name="NICKNAME">
         <C:text-match collation="i;unicode-casemap"
                       match-type="equals"
         >me</C:text-match>
       </C:prop-filter>
     </C:filter>
   </C:addressbook-query>"#,
            vec![
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::AddressbookQuery,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::Dav,
                    element: Element::Prop,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::Dav,
                    element: Element::Getetag,
                }),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::AddressData,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Prop,
                }),
                TestToken::Attribute(Attribute::Name("VERSION".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Prop,
                }),
                TestToken::Attribute(Attribute::Name("UID".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Prop,
                }),
                TestToken::Attribute(Attribute::Name("NICKNAME".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Prop,
                }),
                TestToken::Attribute(Attribute::Name("EMAIL".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Prop,
                }),
                TestToken::Attribute(Attribute::Name("FN".to_string())),
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::Filter,
                }),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::PropFilter,
                }),
                TestToken::Attribute(Attribute::Name("NICKNAME".to_string())),
                TestToken::ElementStart(NamedElement {
                    ns: Namespace::CardDav,
                    element: Element::TextMatch,
                }),
                TestToken::Attribute(Attribute::Collation(Collation::UnicodeCasemap)),
                TestToken::Attribute(Attribute::MatchType(MatchType::Equals)),
                TestToken::Text("me".into()),
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementEnd,
                TestToken::ElementEnd,
            ],
        ),
    ] {
        let mut tokenizer = Tokenizer::new(input.as_bytes());
        let mut result = vec![];

        loop {
            match tokenizer.token() {
                Ok(token) => match token {
                    Token::ElementStart { name, raw } => {
                        result.push(TestToken::ElementStart(name));
                        for attr in raw.attributes::<String>() {
                            result.push(TestToken::Attribute(attr.unwrap()));
                        }
                    }
                    Token::ElementEnd => {
                        result.push(TestToken::ElementEnd);
                    }
                    Token::Bytes(cow) => {
                        result.push(TestToken::Bytes(cow.into_owned().into()));
                    }
                    Token::Text(cow) => {
                        result.push(TestToken::Text(cow.into_owned().into()));
                    }
                    Token::UnknownElement(_) => {
                        //result.push(TestToken::UnknownElement(unknown_element));
                    }
                    // 1.0's raw-content mode, which this test never turns on.
                    Token::Content(event) => panic!("Unexpected raw content: {event:?}"),
                    Token::Eof => break,
                },
                Err(err) => {
                    panic!("Error: {:?}", err);
                }
            }
        }

        assert_eq!(result, expected);
    }
}
