/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::references::Graph;
use crate::references::eval::EvalObjectReferences;
use crate::{
    method::{changes::ChangesResponse, get::GetResponse, query::QueryResponse},
    object::{
        email::{EmailProperty, EmailValue},
        mailbox::{MailboxProperty, MailboxValue},
        thread::{ThreadProperty, ThreadValue},
    },
    request::{
        Call, GetRequestMethod, Request, RequestMethod, SetRequestMethod,
        reference::{MaybeIdReference, MaybeResultReference},
    },
    response::{ChangesResponseMethod, GetResponseMethod, Response, ResponseMethod},
};
use ahash::AHashMap;
use jmap_tools::{Key, Map, Value};
use types::id::Id;

#[test]
fn eval_value_references() {
    let request = Request::parse(
        br##"{
                    "using":["urn:ietf:params:jmap:mail"],
                    "methodCalls": [[ "Email/query", {
                            "accountId": "a",
                            "filter": { "inMailbox": "a" },
                            "sort": [{ "property": "receivedAt", "isAscending": false }],
                            "collapseThreads": true,
                            "position": 0,
                            "limit": 10,
                            "calculateTotal": true
                        }, "t0" ],
                        [ "Email/get", {
                            "accountId": "a",
                            "#ids": {
                            "resultOf": "t0",
                            "name": "Email/query",
                            "path": "/ids"
                            },
                            "properties": [ "threadId" ]
                        }, "t1" ],
                        [ "Thread/get", {
                            "accountId": "a",
                            "#ids": {
                            "resultOf": "t1",
                            "name": "Email/get",
                            "path": "/list/*/threadId"
                            }
                        }, "t2" ],
                        [ "Email/get", {
                            "accountId": "a",
                            "#ids": {
                            "resultOf": "t2",
                            "name": "Thread/get",
                            "path": "/list/*/emailIds"
                            },
                            "properties": [ "from", "receivedAt", "subject" ]
                        }, "t3" ]]
                    }"##,
        100,
        1024 * 1024,
    )
    .unwrap();

    let mut response = Response::new(
        1234,
        request.created_ids.unwrap_or_default(),
        request.method_calls.len(),
    );

    assert_eq!(request.method_calls.len(), 4);

    for (test_num, mut call) in request.method_calls.into_iter().enumerate() {
        match test_num {
            0 => {
                response.method_responses.push(Call {
                    id: call.id,
                    name: call.name,
                    method: ResponseMethod::Query(QueryResponse {
                        account_id: Id::new(1),
                        query_state: Default::default(),
                        can_calculate_changes: Default::default(),
                        position: Default::default(),
                        ids: vec![Id::new(4), Id::new(5)],
                        total: Default::default(),
                        limit: Default::default(),
                    }),
                });
            }
            1 => {
                response.resolve_references(&mut call.method).unwrap();
                match call.method {
                    RequestMethod::Get(GetRequestMethod::Email(req)) => {
                        assert_eq!(
                            req.ids,
                            Some(MaybeResultReference::Value(vec![
                                MaybeIdReference::Id(Id::new(4)),
                                MaybeIdReference::Id(Id::new(5))
                            ]))
                        );
                    }
                    _ => panic!("Expected Email Get Request"),
                }
                response.method_responses.push(Call {
                    id: call.id,
                    name: call.name,
                    method: ResponseMethod::Get(GetResponseMethod::Email(GetResponse {
                        account_id: Id::new(1).into(),
                        state: Default::default(),
                        list: vec![
                            Value::Object(Map::from(vec![(
                                Key::Property(EmailProperty::ThreadId),
                                Value::Element(EmailValue::Id(Id::new(9))),
                            )])),
                            Value::Object(Map::from(vec![(
                                Key::Property(EmailProperty::ThreadId),
                                Value::Element(EmailValue::Id(Id::new(10))),
                            )])),
                        ],
                        not_found: Default::default(),
                    })),
                });
            }
            2 => {
                response.resolve_references(&mut call.method).unwrap();
                match call.method {
                    RequestMethod::Get(GetRequestMethod::Thread(req)) => {
                        assert_eq!(
                            req.ids,
                            Some(MaybeResultReference::Value(vec![
                                MaybeIdReference::Id(Id::new(9)),
                                MaybeIdReference::Id(Id::new(10))
                            ]))
                        );
                    }
                    _ => panic!("Expected Thread Get Request"),
                }
                response.method_responses.push(Call {
                    id: call.id,
                    name: call.name,
                    method: ResponseMethod::Get(GetResponseMethod::Thread(GetResponse {
                        account_id: Id::new(1).into(),
                        state: Default::default(),
                        list: vec![
                            Value::Object(Map::from(vec![(
                                Key::Property(ThreadProperty::EmailIds),
                                Value::Array(vec![
                                    Value::Element(ThreadValue::Id(Id::new(100))),
                                    Value::Element(ThreadValue::Id(Id::new(101))),
                                ]),
                            )])),
                            Value::Object(Map::from(vec![(
                                Key::Property(ThreadProperty::EmailIds),
                                Value::Array(vec![
                                    Value::Element(ThreadValue::Id(Id::new(102))),
                                    Value::Element(ThreadValue::Id(Id::new(103))),
                                ]),
                            )])),
                        ],
                        not_found: Default::default(),
                    })),
                });
            }
            3 => {
                response.resolve_references(&mut call.method).unwrap();
                match call.method {
                    RequestMethod::Get(GetRequestMethod::Email(req)) => {
                        assert_eq!(
                            req.ids,
                            Some(MaybeResultReference::Value(vec![
                                MaybeIdReference::Id(Id::new(100)),
                                MaybeIdReference::Id(Id::new(101)),
                                MaybeIdReference::Id(Id::new(102)),
                                MaybeIdReference::Id(Id::new(103)),
                            ]))
                        );
                    }
                    _ => panic!("Expected Mailbox Get Request"),
                }
            }
            _ => panic!("Unexpected invocation {}", test_num),
        }
    }
}

#[test]
fn eval_property_references() {
    let request = Request::parse(
        br##"{
                    "using":["urn:ietf:params:jmap:mail"],
                    "methodCalls": [
                    ["Mailbox/changes",{
                    "accountId":"s",
                    "sinceState":"srxqk071myhgkyay"
                    },"0"],
                    ["Mailbox/get",{
                    "accountId":"s",
                    "#ids":{"name":"Mailbox/changes","path":"/created","resultOf":"0"}
                    },"1"],
                    ["Mailbox/get",{
                    "accountId":"s",
                    "#ids":{"name":"Mailbox/changes","path":"/updated","resultOf":"0"},
                    "#properties":{"name":"Mailbox/changes","path":"/updatedProperties","resultOf":"0"}
                    },"2"]
                    ]
                    }"##,
        100,
        1024 * 1024,
    )
    .unwrap();

    let mut response = Response::new(
        1234,
        request.created_ids.unwrap_or_default(),
        request.method_calls.len(),
    );

    assert_eq!(request.method_calls.len(), 3);

    for (test_num, mut call) in request.method_calls.into_iter().enumerate() {
        match test_num {
            0 => {
                response.method_responses.push(Call {
                    id: call.id,
                    name: call.name,
                    method: ResponseMethod::Changes(ChangesResponseMethod::Mailbox(Box::new(
                        ChangesResponse {
                            account_id: Id::new(1),
                            old_state: Default::default(),
                            new_state: Default::default(),
                            has_more_changes: Default::default(),
                            created: Default::default(),
                            updated: vec![Id::new(2), Id::new(3)],
                            destroyed: Default::default(),
                            updated_properties: Some(vec![
                                MailboxProperty::Name.into(),
                                MailboxProperty::ParentId.into(),
                            ]),
                        },
                    ))),
                });
            }
            1 => {
                response.resolve_references(&mut call.method).unwrap();
                match call.method {
                    RequestMethod::Get(GetRequestMethod::Mailbox(req)) => {
                        assert_eq!(req.ids, Some(MaybeResultReference::Value(vec![])));
                    }
                    _ => panic!("Expected Mailbox Get Request"),
                }
            }
            2 => {
                response.resolve_references(&mut call.method).unwrap();
                match call.method {
                    RequestMethod::Get(GetRequestMethod::Mailbox(req)) => {
                        assert_eq!(
                            req.ids,
                            Some(MaybeResultReference::Value(vec![
                                MaybeIdReference::Id(Id::new(2)),
                                MaybeIdReference::Id(Id::new(3))
                            ]))
                        );
                    }
                    _ => panic!("Expected Mailbox Get Request"),
                }
            }
            _ => panic!("Unexpected invocation {}", test_num),
        }
    }
}

#[test]
fn eval_create_references() {
    let request = Request::parse(
        br##"{
                    "using": [
                        "urn:ietf:params:jmap:core",
                        "urn:ietf:params:jmap:mail"
                    ],
                    "methodCalls": [
                        [
                            "Mailbox/set",
                            {
                                "accountId": "b",
                                "create": {
                                    "a": {
                                        "name": "Folder a",
                                        "parentId": "#b"
                                    },
                                    "b": {
                                        "name": "Folder b",
                                        "parentId": "#c"
                                    },
                                    "c": {
                                        "name": "Folder c",
                                        "parentId": "#d"
                                    },
                                    "d": {
                                        "name": "Folder d",
                                        "parentId": "#e"
                                    },
                                    "e": {
                                        "name": "Folder e",
                                        "parentId": "#f"
                                    },
                                    "f": {
                                        "name": "Folder f",
                                        "parentId": "#g"
                                    },
                                    "g": {
                                        "name": "Folder g",
                                        "parentId": null
                                    }
                                }
                            },
                            "fulltree"
                        ],
                        [
                            "Mailbox/set",
                            {
                                "accountId": "b",
                                "create": {
                                    "a1": {
                                        "name": "Folder a1",
                                        "parentId": null
                                    },
                                    "b2": {
                                        "name": "Folder b2",
                                        "parentId": "#a1"
                                    },
                                    "c3": {
                                        "name": "Folder c3",
                                        "parentId": "#a1"
                                    },
                                    "d4": {
                                        "name": "Folder d4",
                                        "parentId": "#b2"
                                    },
                                    "e5": {
                                        "name": "Folder e5",
                                        "parentId": "#b2"
                                    },
                                    "f6": {
                                        "name": "Folder f6",
                                        "parentId": "#d4"
                                    },
                                    "g7": {
                                        "name": "Folder g7",
                                        "parentId": "#e5"
                                    }
                                }
                            },
                            "fulltree2"
                        ],
                        [
                            "Mailbox/set",
                            {
                                "accountId": "b",
                                "create": {
                                    "z": {
                                        "name": "Folder Z",
                                        "parentId": "#x"
                                    },
                                    "y": {
                                        "name": null
                                    },
                                    "x": {
                                        "name": "Folder X"
                                    }
                                }
                            },
                            "xyz"
                        ],
                        [
                            "Mailbox/set",
                            {
                                "accountId": "b",
                                "create": {
                                    "a": {
                                        "name": "Folder a",
                                        "parentId": "#b"
                                    },
                                    "b": {
                                        "name": "Folder b",
                                        "parentId": "#c"
                                    },
                                    "c": {
                                        "name": "Folder c",
                                        "parentId": "#d"
                                    },
                                    "d": {
                                        "name": "Folder d",
                                        "parentId": "#a"
                                    }
                                }
                            },
                            "circular"
                        ]
                    ]
                }"##,
        100,
        1024 * 1024,
    )
    .unwrap();

    let response = Response::new(
        1234,
        request.created_ids.unwrap_or_default(),
        request.method_calls.len(),
    );

    for (test_num, mut call) in request.method_calls.into_iter().enumerate() {
        match response.resolve_references(&mut call.method) {
            Ok(_) => assert!(
                (0..3).contains(&test_num),
                "Unexpected invocation {}",
                test_num
            ),
            Err(err) => {
                assert_eq!(test_num, 3);
                assert!(
                    err.matches(trc::EventType::Jmap(trc::JmapEvent::InvalidArguments)),
                    "{:?}",
                    err
                );
                continue;
            }
        }

        if let RequestMethod::Set(SetRequestMethod::Mailbox(request)) = call.method {
            if test_num == 0 {
                assert_eq!(
                    request
                        .create
                        .unwrap()
                        .into_iter()
                        .map(|b| b.0)
                        .collect::<Vec<_>>(),
                    ["g", "f", "e", "d", "c", "b", "a"]
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                );
            } else if test_num == 1 {
                let mut pending_ids = vec!["a1", "b2", "d4", "e5", "f6", "c3", "g7"];

                for (id, _) in request.create.as_ref().unwrap() {
                    match id.as_str() {
                        "a1" => (),
                        "b2" | "c3" => assert!(!pending_ids.contains(&"a1")),
                        "d4" | "e5" => assert!(!pending_ids.contains(&"b2")),
                        "f6" => assert!(!pending_ids.contains(&"d4")),
                        "g7" => assert!(!pending_ids.contains(&"e5")),
                        _ => panic!("Unexpected ID"),
                    }
                    pending_ids.retain(|i| i != id);
                }

                if !pending_ids.is_empty() {
                    panic!(
                        "Unexpected order: {:?}",
                        request
                            .create
                            .as_ref()
                            .unwrap()
                            .iter()
                            .map(|b| b.0.to_string())
                            .collect::<Vec<_>>()
                    );
                }
            } else if test_num == 2 {
                assert_eq!(
                    request
                        .create
                        .unwrap()
                        .into_iter()
                        .map(|b| b.0)
                        .collect::<Vec<_>>(),
                    ["x", "z", "y"]
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                );
            }
        } else {
            panic!("Expected Set Mailbox Request");
        }
    }

    let request = Request::parse(
        br##"{
                "using": [
                    "urn:ietf:params:jmap:core",
                    "urn:ietf:params:jmap:mail"
                ],
                "methodCalls": [
                    [
                        "Mailbox/set",
                        {
                            "accountId": "b",
                            "create": {
                                "a": {
                                    "name": "a",
                                    "parentId": "#x"
                                },
                                "b": {
                                    "name": "b",
                                    "parentId": "#y"
                                },
                                "c": {
                                    "name": "c",
                                    "parentId": "#z"
                                }
                            }
                        },
                        "ref1"
                    ],
                    [
                        "Mailbox/set",
                        {
                            "accountId": "b",
                            "create": {
                                "a1": {
                                    "name": "a1",
                                    "parentId": "#a"
                                },
                                "b2": {
                                    "name": "b2",
                                    "parentId": "#b"
                                },
                                "c3": {
                                    "name": "c3",
                                    "parentId": "#c"
                                }
                            }
                        },
                        "red2"
                    ]
                ],
                "createdIds": {
                    "x": "b",
                    "y": "c",
                    "z": "d"
                }
            }"##,
        1024,
        1024 * 1024,
    )
    .unwrap();

    let mut response = Response::new(
        1234,
        request.created_ids.unwrap_or_default(),
        request.method_calls.len(),
    );

    let mut invocations = request.method_calls.into_iter();
    let mut call = invocations.next().unwrap();
    response.resolve_references(&mut call.method).unwrap();

    if let RequestMethod::Set(SetRequestMethod::Mailbox(request)) = call.method {
        let create = request
            .create
            .as_ref()
            .unwrap()
            .iter()
            .map(|(p, v)| {
                (
                    p.as_str(),
                    v.as_object()
                        .unwrap()
                        .get(&Key::Property(MailboxProperty::ParentId))
                        .unwrap(),
                )
            })
            .collect::<AHashMap<_, _>>();
        assert_eq!(
            *create.get("a").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(1)))
        );
        assert_eq!(
            *create.get("b").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(2)))
        );
        assert_eq!(
            *create.get("c").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(3)))
        );
    } else {
        panic!("Expected Mailbox Set Request");
    }

    response
        .created_ids
        .insert("a".to_string(), Id::new(5).into());
    response
        .created_ids
        .insert("b".to_string(), Id::new(6).into());
    response
        .created_ids
        .insert("c".to_string(), Id::new(7).into());

    let mut call = invocations.next().unwrap();
    response.resolve_references(&mut call.method).unwrap();

    if let RequestMethod::Set(SetRequestMethod::Mailbox(request)) = call.method {
        let create = request
            .create
            .as_ref()
            .unwrap()
            .iter()
            .map(|(p, v)| {
                (
                    p.as_str(),
                    v.as_object()
                        .unwrap()
                        .get(&Key::Property(MailboxProperty::ParentId))
                        .unwrap(),
                )
            })
            .collect::<AHashMap<_, _>>();
        assert_eq!(
            *create.get("a1").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(5)))
        );
        assert_eq!(
            *create.get("b2").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(6)))
        );
        assert_eq!(
            *create.get("c3").unwrap(),
            &Value::Element(MailboxValue::Id(Id::new(7)))
        );
    } else {
        panic!("Expected Mailbox Set Request");
    }
}

#[test]
fn eval_nested_element_ref() {
    let mut created_ids = AHashMap::new();
    created_ids.insert("server-1".to_string(), Id::new(42).into());
    let response = Response::new(0, created_ids, 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![
        (
            Key::Property(MailboxProperty::Name),
            Value::Str("inbox".into()),
        ),
        (
            Key::Property(MailboxProperty::ParentId),
            Value::Object(Map::from(vec![(
                Key::Property(MailboxProperty::ParentId),
                Value::Element(MailboxValue::IdReference("server-1".into())),
            )])),
        ),
    ]));

    value
        .eval_object_references(&response, &mut Graph::None, 0, 5, true)
        .unwrap();

    let nested = value
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap();
    assert_eq!(nested, &Value::Element(MailboxValue::Id(Id::new(42))));
}

#[test]
fn eval_array_element_ref() {
    let mut created_ids = AHashMap::new();
    created_ids.insert("a".to_string(), Id::new(1).into());
    created_ids.insert("b".to_string(), Id::new(2).into());
    let response = Response::new(0, created_ids, 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Array(vec![
            Value::Element(MailboxValue::IdReference("a".into())),
            Value::Element(MailboxValue::IdReference("b".into())),
        ]),
    )]));

    value
        .eval_object_references(&response, &mut Graph::None, 0, 5, true)
        .unwrap();

    let arr = value
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], Value::Element(MailboxValue::Id(Id::new(1))));
    assert_eq!(arr[1], Value::Element(MailboxValue::Id(Id::new(2))));
}

#[test]
fn eval_nested_array_of_objects_with_element_ref() {
    let mut created_ids = AHashMap::new();
    created_ids.insert("a".to_string(), Id::new(7).into());
    let response = Response::new(0, created_ids, 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Array(vec![Value::Object(Map::from(vec![(
            Key::Property(MailboxProperty::ParentId),
            Value::Element(MailboxValue::IdReference("a".into())),
        )]))]),
    )]));

    value
        .eval_object_references(&response, &mut Graph::None, 0, 5, true)
        .unwrap();

    let resolved = value
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap();
    assert_eq!(resolved, &Value::Element(MailboxValue::Id(Id::new(7))));
}

#[test]
fn eval_graph_collects_nested_ref() {
    let response = Response::new(0, AHashMap::new(), 0);
    let mut graph_map: AHashMap<String, Vec<String>> = AHashMap::new();
    let child_id = "outer".to_string();

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Object(Map::from(vec![(
            Key::Property(MailboxProperty::ParentId),
            Value::Element(MailboxValue::IdReference("inner".into())),
        )])),
    )]));

    {
        let mut graph = Graph::Some {
            child_id: &child_id,
            graph: &mut graph_map,
        };
        value
            .eval_object_references(&response, &mut graph, 0, 5, true)
            .unwrap();
    }

    assert_eq!(graph_map.get("outer"), Some(&vec!["inner".to_string()]));
}

#[test]
fn eval_unresolved_nested_ref_errors_without_graph() {
    let response = Response::new(0, AHashMap::new(), 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Object(Map::from(vec![(
            Key::Property(MailboxProperty::ParentId),
            Value::Element(MailboxValue::IdReference("missing".into())),
        )])),
    )]));

    let err = value
        .eval_object_references(&response, &mut Graph::None, 0, 5, true)
        .unwrap_err();
    assert!(
        err.matches(trc::EventType::Jmap(trc::JmapEvent::InvalidResultReference)),
        "{:?}",
        err
    );
}

#[test]
fn eval_depth_limit_blocks_walk_into_inner_object() {
    let mut created_ids = AHashMap::new();
    created_ids.insert("inner".to_string(), Id::new(99).into());
    let response = Response::new(0, created_ids, 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Object(Map::from(vec![(
            Key::Property(MailboxProperty::ParentId),
            Value::Object(Map::from(vec![(
                Key::Property(MailboxProperty::ParentId),
                Value::Element(MailboxValue::IdReference("inner".into())),
            )])),
        )])),
    )]));

    value
        .eval_object_references(&response, &mut Graph::None, 0, 2, true)
        .unwrap();

    let deepest = value
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap();
    assert_eq!(
        deepest,
        &Value::Element(MailboxValue::IdReference("inner".into()))
    );
}

#[test]
fn eval_depth_limit_substitutes_element_at_max_depth() {
    let mut created_ids = AHashMap::new();
    created_ids.insert("inner".to_string(), Id::new(99).into());
    let response = Response::new(0, created_ids, 0);

    let mut value: Value<'_, MailboxProperty, MailboxValue> = Value::Object(Map::from(vec![(
        Key::Property(MailboxProperty::ParentId),
        Value::Object(Map::from(vec![(
            Key::Property(MailboxProperty::ParentId),
            Value::Element(MailboxValue::IdReference("inner".into())),
        )])),
    )]));

    value
        .eval_object_references(&response, &mut Graph::None, 0, 2, true)
        .unwrap();

    let resolved = value
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap()
        .as_object()
        .unwrap()
        .get(&Key::Property(MailboxProperty::ParentId))
        .unwrap();
    assert_eq!(resolved, &Value::Element(MailboxValue::Id(Id::new(99))));
}

#[test]
fn trusted_email_references_are_per_occurrence_and_follow_binding_replacement() {
    use crate::method::{get::GetRequest, set::SetResponse};
    use crate::object::email::Email;
    use crate::object::mailbox::Mailbox;
    let id = Id::from_parts(3, 42);
    let mut response = Response::new(0, AHashMap::from_iter([("client".into(), id.into())]), 0);
    let mut created = SetResponse::<Email>::from_request(&Default::default(), 0).unwrap();
    created.account_id = Some(Id::new(7));
    created.created("server".into(), id);
    created.update_created_ids(&mut response);
    response.trust_email_creations(&created);

    let resolve = |response: &Response<'_>| {
        let mut request = RequestMethod::Get(GetRequestMethod::Email(Box::new(
            serde_json::from_str::<GetRequest<Email>>(
                &serde_json::json!({
                    "accountId": Id::new(7).to_string(),
                    "ids": ["invalid-id!", "#server", id.to_string(), "#client"]
                })
                .to_string(),
            )
            .unwrap(),
        )));
        response.resolve_references(&mut request).unwrap();
        let RequestMethod::Get(GetRequestMethod::Email(request)) = request else {
            panic!()
        };
        request
    };
    let request = resolve(&response);
    assert_eq!(request.arguments.trusted_ids.len(), 1);
    let target = request.arguments.trusted_ids.get(&1).unwrap();
    assert_eq!(target.account_id(), 7);
    assert_eq!(target.document_id(), 42);
    // A non-Email object replaces the same label with the same numeric ID.
    // Numeric equality must not preserve the earlier type provenance.
    let mut mailbox = SetResponse::<Mailbox>::from_request(&Default::default(), 0).unwrap();
    mailbox.created("server".into(), id);
    mailbox.update_created_ids(&mut response);
    assert!(resolve(&response).arguments.trusted_ids.is_empty());
    // A later successful Email creation can replace that binding again.
    created.account_id = Some(Id::new(8));
    created.update_created_ids(&mut response);
    response.trust_email_creations(&created);
    assert_eq!(
        resolve(&response)
            .arguments
            .trusted_ids
            .get(&1)
            .unwrap()
            .account_id(),
        8
    );
}

#[test]
fn submission_trust_follows_create_name_and_does_not_cover_equal_literals() {
    use crate::method::set::SetResponse;
    use crate::object::email::Email;
    let id = Id::from_parts(3, 42);
    let mut response = Response::new(0, AHashMap::from_iter([("client".into(), id.into())]), 0);
    let mut created = SetResponse::<Email>::from_request(&Default::default(), 0).unwrap();
    created.account_id = Some(Id::new(7));
    created.created("server".into(), id);
    created.update_created_ids(&mut response);
    response.trust_email_creations(&created);
    let json = serde_json::json!({"using": ["urn:ietf:params:jmap:submission"], "methodCalls": [
        ["EmailSubmission/set", {"accountId": Id::new(7).to_string(), "create": {
            "trusted": {"emailId": "#server", "identityId": "#later"},
            "literal": {"emailId": id.to_string()},
            "incoming": {"emailId": "#client"},
            "later": {"emailId": id.to_string()}
        }}, "s"]
    ]});
    let json = json.to_string();
    let mut request = Request::parse(json.as_bytes(), 16, usize::MAX).unwrap();
    let method = &mut request.method_calls[0].method;
    response.resolve_references(method).unwrap();
    let RequestMethod::Set(SetRequestMethod::EmailSubmission(request)) = method else {
        panic!()
    };
    assert_eq!(request.arguments.trusted_email_ids.len(), 1);
    assert!(request.arguments.trusted_email_ids.contains_key("trusted"));
    assert_ne!(
        request.create.as_ref().unwrap().iter().next().unwrap().0,
        "trusted"
    );
}

#[test]
fn duplicate_submission_properties_do_not_share_reference_trust() {
    use crate::{method::set::SetResponse, object::email::Email};
    let id = Id::from_parts(3, 42);
    let mut response = Response::new(0, AHashMap::new(), 0);
    let mut created = SetResponse::<Email>::from_request(&Default::default(), 0).unwrap();
    created.account_id = Some(Id::new(7));
    created.created("server".into(), id);
    created.update_created_ids(&mut response);
    response.trust_email_creations(&created);
    // Value's object vector retains duplicate property occurrences.
    let json = format!(
        r##"{{"using":["urn:ietf:params:jmap:submission"],"methodCalls":[
        ["EmailSubmission/set",{{"accountId":"{}","create":{{
            "s":{{"emailId":"#server","emailId":"{}"}}
        }}}},"s"]]}}"##,
        Id::new(7),
        id
    );
    let mut request = Request::parse(json.as_bytes(), 16, usize::MAX).unwrap();
    response
        .resolve_references(&mut request.method_calls[0].method)
        .unwrap();
    let RequestMethod::Set(SetRequestMethod::EmailSubmission(request)) =
        &request.method_calls[0].method
    else {
        panic!()
    };
    let trusted = request.arguments.trusted_email_ids.get("s").unwrap();
    assert_eq!(trusted.len(), 1);
    assert!(trusted.contains_key(&0));
    assert!(!trusted.contains_key(&1));
}

#[test]
fn untrusted_wire_arguments_cannot_mint_email_target_provenance() {
    use crate::method::{get::GetRequest, set::SetRequest};
    use crate::object::{email::Email, email_submission::EmailSubmission};
    let forged = serde_json::json!({
        "trusted_ids": {"0": {"account_id": 1, "document_id": 2}},
        "trusted_email_ids": {"s": {"0": {"account_id": 1, "document_id": 2}}},
        "trusted_updates": [{"account_id": 1, "document_id": 2}],
        "trusted_destroy": [{"account_id": 1, "document_id": 2}]
    });
    let get: GetRequest<Email> = serde_json::from_str(&forged.to_string()).unwrap();
    assert!(get.arguments.trusted_ids.is_empty());
    let json = forged.to_string();
    let set: SetRequest<Email> = serde_json::from_str(&json).unwrap();
    assert!(set.arguments.trusted_updates.is_empty());
    assert!(set.arguments.trusted_destroy.is_empty());
    let submission: SetRequest<EmailSubmission> = serde_json::from_str(&json).unwrap();
    assert!(submission.arguments.trusted_email_ids.is_empty());
}
