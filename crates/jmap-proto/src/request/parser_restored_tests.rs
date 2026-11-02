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
