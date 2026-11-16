/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::request::Request;

const TEST: &str = r#"
    {
        "using": [ "urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail" ],
        "methodCalls": [
          [ "method1", {
            "arg1": "arg1data",
            "arg2": "arg2data"
          }, "c1" ],
          [ "Core/echo", {
            "hello": true,
            "high": 5
          }, "c2" ],
          [ "method3", {"hello": [{"a": {"b": true}}]}, "c3" ]
        ],
        "createdIds": {
            "c1": "m1",
            "c2": "m2"
        }
      }
    "#;

const TEST1: &str = r#"
    {
    "using": [
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail"
    ],
    "methodCalls": [
        [
        "Email/query",
        {
            "accountId": "0",
            "filter": { "conditions": [ { "hasKeyword": "music", "maxSize": 455 }, { "hasKeyword": "video" }, { "operator": "AND", "conditions": [ { "subject": "test" }, { "minSize": 100 } ] } ], "operator": "OR" },
            "sort": [
            {
                "property": "subject",
                "isAscending": true
            },
            {
                "property": "allInThreadHaveKeyword",
                "isAscending": false,
                "keyword": "$seen"
            },
            {
                "keyword": "$junk",
                "property": "someInThreadHaveKeyword",
                "collation": "i;octet",
                "isAscending": false
            }
            ],
            "position": 0,
            "limit": 10
        },
        "c1"
        ]
    ],
    "createdIds": {}
    }
    "#;

const TEST2: &str = r##"
    {
        "using": [
          "urn:ietf:params:jmap:submission",
          "urn:ietf:params:jmap:mail",
          "urn:ietf:params:jmap:core"
        ],
        "methodCalls": [
          [
            "Email/set",
            {
              "accountId": "c",
              "create": {
                "c37ee58b-e224-4799-88e6-1d7484e3b782": {
                  "mailboxIds": {
                    "9": true
                  },
                  "subject": "test",
                  "from": [
                    {
                      "name": "Foo",
                      "email": "foo@bar.com"
                    }
                  ],
                  "to": [
                    {
                      "name": null,
                      "email": "bar@foo.com"
                    }
                  ],
                  "cc": [],
                  "bcc": [],
                  "replyTo": [
                    {
                      "name": null,
                      "email": "foo@bar.com"
                    }
                  ],
                  "htmlBody": [
                    {
                      "partId": "c37ee58b-e224-4799-88e6-1d7484e3b782",
                      "type": "text/html"
                    }
                  ],
                  "bodyValues": {
                    "c37ee58b-e224-4799-88e6-1d7484e3b782": {
                      "value": "<p>test email<br></p>",
                      "isEncodingProblem": false,
                      "isTruncated": false
                    }
                  },
                  "header:User-Agent:asText": "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/113.0"
                }
              }
            },
            "c0"
          ],
          [
            "EmailSubmission/set",
            {
              "accountId": "c",
              "create": {
                "c37ee58b-e224-4799-88e6-1d7484e3b782": {
                  "identityId": "a",
                  "emailId": "#c37ee58b-e224-4799-88e6-1d7484e3b782",
                  "envelope": {
                    "mailFrom": {
                      "email": "foo@bar.com"
                    },
                    "rcptTo": [
                      {
                        "email": "bar@foo.com"
                      }
                    ]
                  }
                }
              },
              "onSuccessUpdateEmail": {
                "#c37ee58b-e224-4799-88e6-1d7484e3b782": {
                  "mailboxIds/d": true,
                  "mailboxIds/9": null,
                  "keywords/$seen": true,
                  "keywords/$draft": null
                }
              }
            },
            "c1"
          ]
        ]
      }
    "##;

const TEST_ESCAPED_SOLIDUS: &str = r#"
    {
        "using": [ "urn:ietf:params:jmap:core" ],
        "methodCalls": [
          [ "Core\/echo", { "hello": true }, "c1" ]
        ]
      }
    "#;

#[test]
fn parse_request() {
    println!("{:#?}", Request::parse(TEST.as_bytes(), 10, 10240));
    println!("{:#?}", Request::parse(TEST1.as_bytes(), 10, 10240));
    println!("{:#?}", Request::parse(TEST2.as_bytes(), 10, 10240));
}

// Release 6 spec 6.8 (D29): `z` is accepted only in the normal state slots of
// Calendar/changes, CalendarEvent/changes and CalendarEvent/queryChanges.
#[test]
fn za_calendar_state_contexts() {
    use super::RequestMethod;
    use crate::{
        request::QueryChangesRequestMethod,
        types::state::{JMAPIntermediateState, State, ZaCalendarOrigin},
    };
    use serde_json::{Value, json};
    use trc::{EventType, JmapEvent, Key};

    fn za_state(origin: ZaCalendarOrigin) -> State {
        State::ZaCalendarIntermediate {
            range: JMAPIntermediateState {
                from_id: 42,
                to_id: 50,
                items_sent: 3,
            },
            origin,
        }
    }

    fn parse(calls: Value) -> Vec<RequestMethod<'static>> {
        let body = json!({ "using": ["urn:ietf:params:jmap:core"], "methodCalls": calls });
        let body: &'static str = body.to_string().leak();
        Request::parse(body.as_bytes(), 1024, 1 << 20)
            .unwrap()
            .method_calls
            .into_iter()
            .map(|call| call.method)
            .collect()
    }

    fn error(method: &RequestMethod<'_>) -> Option<(EventType, Option<String>)> {
        match method {
            RequestMethod::Error(error) => Some((
                error.event_type(),
                error.value_as_str(Key::Details).map(str::to_string),
            )),
            _ => None,
        }
    }

    fn since(method: &RequestMethod<'_>) -> Option<State> {
        match method {
            RequestMethod::Changes(request) => Some(request.since_state.clone()),
            RequestMethod::QueryChanges(request) => Some(match request {
                QueryChangesRequestMethod::Email(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::Mailbox(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::EmailSubmission(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::Principal(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::Quota(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::ContactCard(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::FileNode(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::CalendarEvent(r) => r.since_query_state.clone(),
                QueryChangesRequestMethod::CalendarEventNotification(r) => {
                    r.since_query_state.clone()
                }
                QueryChangesRequestMethod::ShareNotification(r) => r.since_query_state.clone(),
            }),
            _ => None,
        }
    }

    const CHANGES: &[&str] = &[
        "Mailbox",
        "Thread",
        "Email",
        "Identity",
        "EmailSubmission",
        "Principal",
        "Quota",
        "AddressBook",
        "ContactCard",
        "FileNode",
        "ShareNotification",
        "Calendar",
        "CalendarEvent",
        "CalendarEventNotification",
        "ParticipantIdentity",
    ];
    const QUERY_CHANGES: &[&str] = &[
        "Mailbox",
        "Email",
        "EmailSubmission",
        "Principal",
        "Quota",
        "ContactCard",
        "FileNode",
        "ShareNotification",
        "CalendarEvent",
        "CalendarEventNotification",
    ];
    let invalid = EventType::Jmap(JmapEvent::InvalidArguments);
    let unknown = EventType::Jmap(JmapEvent::UnknownMethod);

    for origin in [ZaCalendarOrigin::Initial, ZaCalendarOrigin::Exact(41)] {
        let state = za_state(origin);
        let encoded = state.to_string();
        let mut calls = Vec::new();
        let mut expected = Vec::new();
        for (objects, function, slot) in [
            (CHANGES, "changes", "sinceState"),
            (QUERY_CHANGES, "queryChanges", "sinceQueryState"),
        ] {
            for object in objects {
                let name = format!("{object}/{function}");
                calls.push(json!([name, { "accountId": "a", slot: encoded }, name]));
                expected.push(matches!(
                    name.as_str(),
                    "Calendar/changes" | "CalendarEvent/changes" | "CalendarEvent/queryChanges"
                ));
            }
        }
        let methods = parse(Value::Array(calls.clone()));
        assert_eq!(methods.len(), expected.len());
        for ((call, method), accepted) in calls.iter().zip(&methods).zip(expected) {
            if accepted {
                assert_eq!(since(method), Some(state.clone()), "{call}");
            } else {
                let (event, _) = error(method).unwrap_or_else(|| panic!("{call}: accepted"));
                assert_eq!(event, invalid, "{call}");
            }
        }

        // An out-of-scope `z` is answered as an invalid state string is; a missing
        // account changes neither, since the parser reads no account.
        let methods = parse(json!([
            ["Email/changes", { "sinceState": encoded }, "c0"],
            ["Email/changes", { "sinceState": "garbage" }, "c1"],
            ["CalendarEvent/changes", { "sinceState": encoded }, "c2"],
            ["CalendarEvent/queryChanges", { "sinceQueryState": encoded }, "c3"],
            ["CalendarEvent/changes", { "accountId": "!!", "sinceState": encoded }, "c4"],
            ["Email/changes", { "accountId": "!!", "sinceState": encoded }, "c5"]
        ]));
        let (event, out_of_scope) = error(&methods[0]).unwrap();
        assert_eq!(event, invalid);
        assert!(out_of_scope.is_some());
        assert_eq!(error(&methods[1]).unwrap().0, invalid);
        assert_eq!(since(&methods[2]), Some(state.clone()));
        assert_eq!(since(&methods[3]), Some(state.clone()));
        assert_eq!(since(&methods[4]), Some(state.clone()));
        assert_eq!(error(&methods[5]), Some((invalid, out_of_scope.clone())));

        // An argument that fails to decode keeps its own error, ahead of the state
        // check: the arguments below are written in this order on purpose.
        for (object, slot, after) in [
            ("Email/changes", "sinceState", r#""maxChanges": "x""#),
            ("Calendar/changes", "sinceState", r#""maxChanges": "x""#),
            (
                "Email/queryChanges",
                "sinceQueryState",
                r#""maxChanges": "x""#,
            ),
            (
                "CalendarEvent/queryChanges",
                "sinceQueryState",
                r#""maxChanges": "x""#,
            ),
        ] {
            for value in [encoded.as_str(), "n"] {
                let body = format!(
                    r#"{{"using": [], "methodCalls": [["{object}", {{"accountId": "a", "{slot}": "{value}", {after}}}, "c0"]]}}"#
                );
                let body: &'static str = body.leak();
                let request = Request::parse(body.as_bytes(), 10, 10240).unwrap();
                let (event, details) = error(&request.method_calls[0].method)
                    .unwrap_or_else(|| panic!("{object} {value}: decoded"));
                assert_eq!(event, invalid, "{object} {value}");
                assert_ne!(details, out_of_scope, "{object} {value}");
            }
        }
        let methods = parse(json!([
            ["Calendar/changes", { "accountId": 5, "sinceState": encoded }, "c0"],
            ["Email/changes", { "accountId": 5, "sinceState": encoded }, "c1"]
        ]));
        for method in &methods {
            let (event, details) = error(method).unwrap();
            assert_eq!(event, invalid);
            assert_ne!(details, out_of_scope);
        }

        // Unknown methods stay unknown, whatever their arguments hold.
        let methods = parse(json!([
            ["Calendar/queryChanges", { "accountId": "a", "sinceQueryState": encoded }, "c0"],
            ["Foo/changes", { "accountId": "a", "sinceState": encoded }, "c1"],
            ["Core/changes", { "accountId": "a", "sinceState": encoded }, "c2"]
        ]));
        for method in &methods {
            assert_eq!(error(method).unwrap().0, unknown, "{method:?}");
        }
    }

    // Ordinary states are accepted in every slot, as before.
    let methods = parse(json!([
        ["Email/changes", { "accountId": "a", "sinceState": "rqaeiacde" }, "c0"],
        ["ParticipantIdentity/changes", { "accountId": "a", "sinceState": "sae" }, "c1"],
        ["Email/queryChanges", { "accountId": "a", "sinceQueryState": "n" }, "c2"]
    ]));
    assert_eq!(
        since(&methods[0]),
        Some(State::new_intermediate(1024, 2048, 100))
    );
    assert_eq!(since(&methods[1]), Some(State::Exact(1)));
    assert_eq!(since(&methods[2]), Some(State::Initial));
}

#[test]
fn parse_method_name_with_escaped_solidus() {
    let request = Request::parse(TEST_ESCAPED_SOLIDUS.as_bytes(), 10, 10240)
        .expect("escaped solidus in method name must parse");
    assert_eq!(request.method_calls.len(), 1);
    assert!(matches!(
        request.method_calls[0].method,
        super::RequestMethod::Echo(_)
    ));
}
