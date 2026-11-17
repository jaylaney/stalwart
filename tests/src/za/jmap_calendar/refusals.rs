/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.9: a key account's participant identities, where only the
//! default can be switched; spec 6.1 (D25), 6.2, 6.8 (D29) and 6.11: what a
//! key account's calendars refuse, and to whom.

use super::{event_ics, finish, key_account, make_calendar, put_event, share};
use crate::{
    utils::{
        account::Account,
        jmap::JmapResponse,
        server::TestServer,
        webdav::DummyWebDavClient,
        za::{
            CALENDAR_JMAP_USING, Keyless, SERVER_URL, assert_no_event_preferences, pending_ids,
            za_bearer_jmap, za_bearer_session, za_grant_token, za_post, za_revoke_grant,
        },
    },
    za::{
        STRONG,
        fanout::{CONTENT_TYPE, NO_REPLY, Party, uid_copies},
        gating::method_error,
        tracing::{ws_close, ws_connect},
        user_permissions,
    },
};
use ::registry::schema::enums::Permission;
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use hyper::StatusCode;
use jmap_proto::types::state::{State, ZaCalendarOrigin};
use serde_json::{Value, json};
use std::time::Duration;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
};
use types::{collection::Collection, field::PrincipalField, id::Id};

const FIXED: &str = "Participant identities in this account cannot be created, changed or destroyed; only the default identity can be switched.";
const SHARE_WITH: &str = "Calendar sharing is managed on the account page.";
const METADATA: &str = "Object metadata is not available for zero-access accounts.";
const ATTACHMENTS: &str = "Event attachments are not available for zero-access accounts.";

/// The capabilities of a request that names object metadata: a property
/// layer active only when the request uses its capability.
const METADATA_USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:metadata",
];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar refusal tests...");
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(
            "jmapcal-refusals@example.com",
            STRONG,
            "jmapcal-refusals",
            &["jmapcal-refusals-alias@example.com"],
            user_permissions(),
        )
        .await;
    test.insert_account(owner.clone());
    let (grantee, grantee_id) = key_account(test, "refusals-2", user_permissions()).await;
    identity_set_only_switches_default(test, &owner).await;
    parse_by_grantee_and_keyless(test, &owner, &grantee, grantee_id).await;
    keyless_token_on_each_served_method(test, &owner).await;
    impersonation_refused(test, &owner, &grantee, grantee_id).await;
    // Before 6.11's rows, whose `CalendarEventNotification/set` row would
    // otherwise stop the suite first when that method is let through.
    notification_destroy_leaves_the_item(test, &owner, &grantee, grantee_id).await;
    every_row_of_6_11(test, &owner, &grantee).await;
    calendar_continuation_is_not_a_write_precondition(test, &owner, &grantee).await;
    finish(test, "refusals").await;
}

async fn identities(owner: &Account) -> Value {
    let reply = owner
        .jmap_method_call(
            "ParticipantIdentity/get",
            json!({ "accountId": owner.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    Value::Array(reply.list().to_vec())
}

async fn set(owner: &Account, args: Value) -> JmapResponse {
    let mut args = args;
    args["accountId"] = json!(owner.id_string());
    owner
        .jmap_method_call("ParticipantIdentity/set", args)
        .await
}

/// Release 6 spec 6.9: every create, update and destroy of a key account's
/// identities is refused with one fixed text; the default can be switched.
async fn identity_set_only_switches_default(test: &TestServer, owner: &Account) {
    let account_id = owner.id().document_id();
    let identity = |id: &str, address: &str, default: bool| {
        json!({
            "id": id,
            "name": "jmapcal-refusals",
            "calendarAddress": format!("mailto:{address}"),
            "isDefault": default,
        })
    };
    let expected = |default: &str| {
        json!([
            identity("a", "jmapcal-refusals@example.com", default == "a"),
            identity("b", "jmapcal-refusals-alias@example.com", default == "b"),
        ])
    };

    let reply = set(
        owner,
        json!({
            "create": { "c": {
                "name": "jmapcal identity canary",
                "calendarAddress": "mailto:jmapcal-refusals-alias@example.com",
            } },
            "update": { "a": { "name": "jmapcal identity canary" } },
            "destroy": ["b"],
        }),
    )
    .await;
    let response = reply.response_at(0);
    let refused = json!({ "type": "forbidden", "description": FIXED });
    assert_eq!(response["notCreated"]["c"], refused, "{reply:?}");
    assert_eq!(response["notUpdated"]["a"], refused, "{reply:?}");
    assert_eq!(response["notDestroyed"]["b"], refused, "{reply:?}");
    assert_eq!(identities(owner).await, expected("a"));

    // Nothing of the refused request is stored.
    let record = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
            account_id,
            Collection::Principal,
            0,
            PrincipalField::ParticipantIdentities,
        ))
        .await
        .unwrap()
        .expect("the first read writes the identities record");
    let text = String::from_utf8_lossy(record.as_bytes());
    assert!(
        !text.contains("canary"),
        "the record holds the refused name"
    );

    // The default can be switched on its own.
    let reply = set(owner, json!({ "onSuccessSetIsDefault": "b" })).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(identities(owner).await, expected("b"));

    // A refused destroy keeps upstream's rule: the default does not move.
    let reply = set(
        owner,
        json!({ "destroy": ["b"], "onSuccessSetIsDefault": "a" }),
    )
    .await;
    assert_eq!(
        reply.response_at(0)["notDestroyed"]["b"],
        refused,
        "{reply:?}"
    );
    assert_eq!(identities(owner).await, expected("b"));
}

/// Release 6 spec 6.1 and 6.6: `CalendarEvent/parse` admits the owner with
/// keys only. A grantee's own keys do not open a blob in the owner's account,
/// and a keyless token cannot parse at all.
async fn parse_by_grantee_and_keyless(
    test: &TestServer,
    owner: &Account,
    grantee: &Account,
    grantee_id: u32,
) {
    let owner_id = owner.id().document_id();
    let dav = DummyWebDavClient::new(owner_id, owner.name(), STRONG, owner.name());
    make_calendar(test, owner, &dav, "jmapcal-shared").await;
    // A view grant passes upstream's access check for the grantee.
    share(test, owner, grantee, grantee_id, "jmapcal-shared", "view").await;

    let ics = event_ics("jmapcal-refusals-parse", "jmapcal parse canary").into_bytes();
    let blob = owner
        .jmap_client()
        .await
        .upload(None, ics.clone(), None)
        .await
        .unwrap()
        .take_blob_id();
    let parse = |blob: &str| json!({ "accountId": owner.id_string(), "blobIds": [blob] });
    let refused =
        json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, "0"]);

    // The grantee, with its own keys, is refused the owner's blob.
    let reply = grantee
        .jmap_method_call("CalendarEvent/parse", parse(&blob))
        .await;
    assert_eq!(reply.0["methodResponses"][0], refused, "{reply:?}");

    // The owner's keyless token is refused the same way.
    let (grant, token) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;
    let (status, reply) = za_bearer_jmap(
        &token,
        CALENDAR_JMAP_USING,
        json!([["CalendarEvent/parse", parse(&blob), "0"]]),
    )
    .await;
    za_revoke_grant(&test.server, grant).await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(reply.0["methodResponses"][0], refused, "{reply:?}");

    // Control: the keyed owner parses its own blob.
    let reply = owner
        .jmap_method_call("CalendarEvent/parse", parse(&blob))
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["parsed"][&blob][0]["title"],
        "jmapcal parse canary",
        "{reply:?}"
    );

    // Control: the grantee parses a blob of its own, in its own account.
    let own = grantee
        .jmap_client()
        .await
        .upload(None, ics, None)
        .await
        .unwrap()
        .take_blob_id();
    let reply = grantee
        .jmap_method_call(
            "CalendarEvent/parse",
            json!({ "accountId": grantee.id_string(), "blobIds": [own] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");

    dav.request_with_headers(
        "DELETE",
        &format!(
            "/dav/cal/{}/jmapcal-shared/",
            owner.name().replace('@', "%40")
        ),
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
}

/// Spec 6.2: the thirteen methods a key account serves, each naming `account`.
fn served(account: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("Calendar/get", json!({ "accountId": account })),
        ("Calendar/query", json!({ "accountId": account })),
        ("CalendarEvent/get", json!({ "accountId": account })),
        ("CalendarEvent/query", json!({ "accountId": account })),
        ("ParticipantIdentity/get", json!({ "accountId": account })),
        (
            "Calendar/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "CalendarEvent/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "ParticipantIdentity/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "CalendarEvent/queryChanges",
            json!({ "accountId": account, "sinceQueryState": "n" }),
        ),
        (
            "Calendar/set",
            json!({ "accountId": account, "create": {} }),
        ),
        (
            "CalendarEvent/set",
            json!({ "accountId": account, "create": {} }),
        ),
        (
            "ParticipantIdentity/set",
            json!({ "accountId": account, "create": {} }),
        ),
        (
            "CalendarEvent/parse",
            json!({ "accountId": account, "blobIds": [] }),
        ),
    ]
}

/// Spec 6.1 (D25): the first response is the calendar refusal, its
/// description compared whole.
fn assert_calendar_keyless(reply: &JmapResponse, what: &str) {
    let call = &reply.0["methodResponses"][0];
    assert_eq!(call[0], "error", "{what}: {reply:?}");
    assert_eq!(
        call[1],
        json!({ "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }),
        "{what}: {reply:?}"
    );
}

/// A request with Basic credentials `user` and `secret` (a master-user
/// login, `account%master`), under the calendar capabilities.
async fn jmap_basic(user: &str, secret: &str, calls: Value) -> JmapResponse {
    JmapResponse(
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("{SERVER_URL}/jmap"))
            .basic_auth(user, Some(secret))
            .body(json!({ "using": CALENDAR_JMAP_USING, "methodCalls": calls }).to_string())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap(),
    )
}

fn one(method: &str, args: &Value) -> Value {
    json!([[method, args, "0"]])
}

fn home(owner: &Account, slug: &str) -> String {
    format!("/dav/cal/{}/{slug}/", owner.name().replace('@', "%40"))
}

/// A set error's type and description.
fn set_error(value: &Value) -> (&str, Option<&str>) {
    (
        value["type"].as_str().unwrap_or_default(),
        value["description"].as_str(),
    )
}

/// Spec 6.1 (D25) and D13: a token that holds no keys for the key account is
/// refused every served method, over a Bearer token and over a WebSocket; the
/// account still advertises the calendar capability to it.
async fn keyless_token_on_each_served_method(test: &TestServer, owner: &Account) {
    let account = owner.id_string();
    // `Keyless::bearer`'s token, minted here so that its grant is revoked.
    let (grant, token) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;

    // D13: the capability is advertised for every token, keyed or not.
    let (status, session) = za_bearer_session(&token).await;
    assert_eq!(status, 200, "the keyless session: {session}");
    assert!(
        session["accounts"][account]["accountCapabilities"]
            .get("urn:ietf:params:jmap:calendars")
            .is_some(),
        "the keyless session's account capabilities: {session}"
    );
    assert_eq!(
        session["primaryAccounts"]["urn:ietf:params:jmap:calendars"], account,
        "the keyless session's primary calendar account: {session}"
    );

    let channels = [
        Keyless::Bearer(token),
        Keyless::WebSocket(ws_connect(owner.name(), STRONG).await),
    ];
    for mut channel in channels {
        for (method, args) in served(account) {
            let reply = channel
                .call_using(CALENDAR_JMAP_USING, one(method, &args))
                .await;
            assert_calendar_keyless(&reply, &format!("{method} over {}", channel.name()));
        }
        if let Keyless::WebSocket(socket) = channel {
            ws_close(socket).await;
        }
    }
    za_revoke_grant(&test.server, grant).await;

    // Control: the owner's password opens its keys, and each call is served.
    for (method, args) in served(account) {
        let reply = owner
            .jmap_request(CALENDAR_JMAP_USING, one(method, &args))
            .await;
        assert_eq!(method_error(&reply), None, "control {method}: {reply:?}");
    }
}

/// Spec 6.1: impersonation is refused each served method, in three forms.
///
/// 1. A master-user login names the key account with the administrator's
///    secret. Its token is marked impersonated and holds no keys: the HTTP
///    layer never lets another account's keys travel with a token. Admission
///    refuses it at the session-keys clause of `za_calendar_admission`.
/// 2. The administrator, an ordinary account holding Impersonate, names the
///    key account. It holds no keys at all, so it is refused at the same
///    clause.
/// 3. A key account holding Impersonate and a view grant on one of the
///    owner's calendars, signed in with its own password, holds its own keys.
///    It passes the session-keys clause and is refused by the
///    `!token.has_permission(Permission::Impersonate)` clause. The control is
///    an otherwise identical grantee without the permission (the module's
///    second account, given the same grant), which is served.
///
/// The `!token.za_is_impersonated()` clause cannot be reached by a login. A
/// master-user login returns a fresh token of the named account with no
/// session keys (`route_auth_request`'s master branch in
/// `crates/common/src/auth/authentication.rs` builds it with
/// `self.access_token(account_id)`), so the session-keys clause refuses it
/// before the impersonation clause is read (review R37-14).
///
/// `CalendarEvent/parse` admits the owner with keys only, so every form, and
/// the control, is refused it at that rule rather than at the clauses above.
async fn impersonation_refused(
    test: &mut TestServer,
    owner: &Account,
    grantee: &Account,
    grantee_id: u32,
) {
    const SLUG: &str = "jmapcal-refusals-impersonate";
    let mut permissions = user_permissions();
    permissions.push(Permission::Impersonate);
    let (holder, holder_id) = key_account(test, "refusals-impersonate", permissions).await;

    let admin = test.account("admin@example.com");
    let master = format!("{}%{}", owner.name(), admin.name());
    for (method, args) in served(owner.id_string()) {
        let reply = jmap_basic(&master, admin.secret(), one(method, &args)).await;
        assert_calendar_keyless(&reply, &format!("{method} by a master-user login"));
        let reply = admin
            .jmap_request(CALENDAR_JMAP_USING, one(method, &args))
            .await;
        assert_calendar_keyless(&reply, &format!("{method} by an Impersonate holder"));
    }

    // The third form, beside its control: the same view grant to both.
    let owner_id = owner.id().document_id();
    let dav = DummyWebDavClient::new(owner_id, owner.name(), STRONG, owner.name());
    make_calendar(test, owner, &dav, SLUG).await;
    share(test, owner, &holder, holder_id, SLUG, "view").await;
    share(test, owner, grantee, grantee_id, SLUG, "view").await;
    for (method, args) in served(owner.id_string()) {
        let reply = holder
            .jmap_request(CALENDAR_JMAP_USING, one(method, &args))
            .await;
        assert_calendar_keyless(
            &reply,
            &format!("{method} by a keyed grantee holding Impersonate"),
        );
        let reply = grantee
            .jmap_request(CALENDAR_JMAP_USING, one(method, &args))
            .await;
        if method == "CalendarEvent/parse" {
            assert_calendar_keyless(&reply, "control: CalendarEvent/parse by a grantee");
        } else {
            assert_eq!(
                method_error(&reply),
                None,
                "control: {method} by a keyed grantee without Impersonate: {reply:?}"
            );
        }
    }
    for address in [holder.name(), grantee.name()] {
        let revoked = za_post(
            "shares/revoke",
            &json!({
                "username": owner.name(), "password": STRONG,
                "calendar": SLUG, "address": address
            }),
        )
        .await
        .expect(200);
        assert_eq!(revoked["changed"], true, "{address}");
    }
    dav.request_with_headers("DELETE", &home(owner, SLUG), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 6.11: every row of what stays refused, on the keyed owner.
async fn every_row_of_6_11(test: &TestServer, owner: &Account, grantee: &Account) {
    const SLUG: &str = "jmapcal-refusals-rows";
    let owner_id = owner.id().document_id();
    let account = owner.id_string();
    let dav = DummyWebDavClient::new(owner_id, owner.name(), STRONG, owner.name());
    let calendar = Id::from(make_calendar(test, owner, &dav, SLUG).await).to_string();
    let event = |uid: &str| {
        json!({
            "@type": "Event",
            "uid": uid,
            "title": uid,
            "start": "2099-03-01T10:00:00",
            "duration": "PT1H",
            "timeZone": "Etc/UTC",
            "calendarIds": { (calendar.as_str()): true },
        })
    };
    let calendar_names = || async move {
        owner
            .jmap_method_call("Calendar/get", json!({ "accountId": account, "ids": null }))
            .await
            .list()
            .iter()
            .map(|calendar| calendar["name"].clone())
            .collect::<Vec<_>>()
    };

    // `shareWith`, on a create and on an update.
    let grant = json!({ grantee.id_string(): { "mayReadItems": true } });
    let reply = owner
        .jmap_method_call(
            "Calendar/set",
            json!({
                "accountId": account,
                "create": { "c": { "name": "jmapcal-refusals", "shareWith": grant } },
            }),
        )
        .await;
    assert_eq!(
        set_error(&reply.response_at(0)["notCreated"]["c"]),
        ("forbidden", Some(SHARE_WITH)),
        "{reply:?}"
    );
    assert!(
        !calendar_names().await.contains(&json!("jmapcal-refusals")),
        "a refused shareWith create stored a calendar"
    );
    let reply = owner
        .jmap_method_call(
            "Calendar/set",
            json!({ "accountId": account, "update": { &calendar: { "shareWith": grant } } }),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&calendar)),
        ("forbidden", Some(SHARE_WITH)),
        "{reply:?}"
    );

    // The scheduling inbox and the cross-account copy.
    for (method, args) in [
        (
            "CalendarEventNotification/get",
            json!({ "accountId": account }),
        ),
        (
            "CalendarEventNotification/query",
            json!({ "accountId": account }),
        ),
        (
            "CalendarEventNotification/set",
            json!({ "accountId": account, "destroy": [] }),
        ),
        (
            "CalendarEventNotification/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "CalendarEventNotification/queryChanges",
            json!({ "accountId": account, "sinceQueryState": "n" }),
        ),
        (
            "CalendarEvent/copy",
            json!({ "fromAccountId": account, "accountId": account, "create": {} }),
        ),
    ] {
        let reply = owner.jmap_method_call(method, args).await;
        assert_eq!(
            method_error(&reply),
            Some("accountNotSupportedByMethod"),
            "{method}: {reply:?}"
        );
    }

    // Per-user event data (D10): the owner's own alerts, keywords and color
    // are the event's, and no preference entry is stored.
    let mut object = event("jmapcal-refusals-prefs");
    object["alerts"] = json!({ "a1": {
        "@type": "Alert",
        "action": "display",
        "trigger": { "@type": "OffsetTrigger", "relativeTo": "start", "offset": "-PT5M" },
    } });
    object["keywords"] = json!({ "kw": true });
    object["color"] = json!("#aabbcc");
    let reply = owner
        .jmap_create("CalendarEvent", [object], Vec::<(&str, &str)>::new())
        .await;
    let prefs = reply.created_id(0);
    assert_no_event_preferences(test, owner_id, prefs.document_id()).await;
    let prefs = prefs.to_string();

    // Object metadata (upgrade ruling 66).
    let metadata = json!({ "example.com": { "note": "metadata-canary" } });
    let reply = owner
        .jmap_request(
            METADATA_USING,
            one(
                "Calendar/set",
                &json!({
                    "accountId": account,
                    "create": { "c": { "name": "jmapcal-refusals-meta", "metadata": metadata } },
                }),
            ),
        )
        .await;
    assert_eq!(
        set_error(&reply.response_at(0)["notCreated"]["c"]),
        ("forbidden", Some(METADATA)),
        "{reply:?}"
    );
    assert!(
        !calendar_names()
            .await
            .contains(&json!("jmapcal-refusals-meta")),
        "a refused metadata create stored a calendar"
    );
    let mut object = event("jmapcal-refusals-meta");
    object["metadata"] = metadata.clone();
    let reply = owner
        .jmap_request(
            METADATA_USING,
            one(
                "CalendarEvent/set",
                &json!({ "accountId": account, "create": { "e": object } }),
            ),
        )
        .await;
    assert_eq!(
        set_error(&reply.response_at(0)["notCreated"]["e"]),
        ("forbidden", Some(METADATA)),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, owner_id, "jmapcal-refusals-meta").await, 0);
    let reply = owner
        .jmap_request(
            METADATA_USING,
            one(
                "CalendarEvent/get",
                &json!({
                    "accountId": account, "ids": [&prefs],
                    "properties": ["id", "title", "metadata"],
                }),
            ),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let [got] = reply.list() else {
        panic!("one event: {reply:?}");
    };
    assert_eq!(got["id"], prefs.as_str(), "{reply:?}");
    assert!(got.get("metadata").is_none(), "{reply:?}");
    let reply = owner
        .jmap_request(
            METADATA_USING,
            one(
                "CalendarEvent/query",
                &json!({
                    "accountId": account,
                    "filter": { "metadataExists": "example.com/note" },
                }),
            ),
        )
        .await;
    assert_eq!(method_error(&reply), Some("unsupportedFilter"), "{reply:?}");

    // An update naming metadata, of the event and of its calendar, is refused
    // in each writer's update arm, and neither object changes.
    let read_one = |method: &'static str, id: String| async move {
        let reply = owner
            .jmap_method_call(method, json!({ "accountId": account, "ids": [id] }))
            .await;
        assert_eq!(method_error(&reply), None, "{reply:?}");
        (
            reply.state().to_string(),
            reply.list().first().cloned().expect("the object"),
        )
    };
    let event_before = read_one("CalendarEvent/get", prefs.clone()).await;
    let calendar_before = read_one("Calendar/get", calendar.clone()).await;
    for (method, id) in [("CalendarEvent/set", &prefs), ("Calendar/set", &calendar)] {
        let reply = owner
            .jmap_request(
                METADATA_USING,
                one(
                    method,
                    &json!({
                        "accountId": account,
                        "update": { id: { "metadata": metadata } },
                    }),
                ),
            )
            .await;
        assert_eq!(method_error(&reply), None, "{method}: {reply:?}");
        assert_eq!(
            set_error(reply.not_updated(id)),
            ("forbidden", Some(METADATA)),
            "{method}: {reply:?}"
        );
    }
    assert_eq!(
        read_one("CalendarEvent/get", prefs.clone()).await,
        event_before,
        "a refused metadata update changed the event"
    );
    assert_eq!(
        read_one("Calendar/get", calendar.clone()).await,
        calendar_before,
        "a refused metadata update changed the calendar"
    );

    // Inline attachment blobs: the download of an event's inline ATTACH is
    // denied, as for any key account's calendar data (spec 9; `has_access_blob`
    // answers `Denied` for an embedded blob of a key account's event), and the
    // route answers it not found.
    const ATTACHED: &str = "jmapcal-refusals-attach";
    let attach_path = format!("{}{ATTACHED}.ics", home(owner, SLUG));
    dav.request_with_headers(
        "PUT",
        &attach_path,
        [CONTENT_TYPE],
        format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
                "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990301T100000Z\r\n",
                "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:jmapcal refusals attachment\r\n",
                "ATTACH;FMTTYPE=text/plain;ENCODING=BASE64;VALUE=BINARY:{data}\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            uid = ATTACHED,
            // "jmapcal refusals inline attachment", base64.
            data = "am1hcGNhbCByZWZ1c2FscyBpbmxpbmUgYXR0YWNobWVudA==",
        ),
    )
    .await
    .with_status(StatusCode::CREATED);
    let attached = Id::from(
        crate::za::dav_seal::resource_id(test, owner_id, &format!("{SLUG}/{ATTACHED}.ics")).await,
    )
    .to_string();
    let reply = owner
        .jmap_method_call(
            "CalendarEvent/get",
            json!({
                "accountId": account, "ids": [&attached],
                "properties": ["id", "uid", "links"],
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let [event_with_link] = reply.list() else {
        panic!("the attached event: {reply:?}");
    };
    assert_eq!(event_with_link["uid"], ATTACHED, "{reply:?}");
    let links = event_with_link["links"]
        .as_object()
        .unwrap_or_else(|| panic!("the attached event's links: {reply:?}"));
    assert_eq!(links.len(), 1, "{reply:?}");
    let attached_blob = links.values().next().unwrap()["blobId"]
        .as_str()
        .unwrap_or_else(|| panic!("the inline ATTACH's blobId: {reply:?}"))
        .to_string();
    let response = owner
        .http_get_raw(
            &format!(
                "{}/jmap/download/{account}/{attached_blob}/attachment.txt",
                owner.base_url()
            ),
            None,
        )
        .await;
    assert_eq!(response.status, 404, "{}", response.text());
    let problem = response
        .json()
        .unwrap_or_else(|| panic!("{}", response.text()));
    assert_eq!(problem["title"], "Not Found", "{problem}");
    assert_eq!(
        problem["detail"], "The requested resource does not exist on this server.",
        "{problem}"
    );
    assert!(
        !response.text().contains("inline attachment"),
        "the denied download carried the attachment"
    );
    dav.request_with_headers("DELETE", &attach_path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(uid_copies(test, owner_id, ATTACHED).await, 0);

    // Inline attachment blobs: a link to an upload of the account.
    let blob = owner
        .jmap_client()
        .await
        .upload(None, b"jmapcal refusals attachment".to_vec(), None)
        .await
        .unwrap()
        .take_blob_id();
    let mut object = event("jmapcal-refusals-link");
    object["links"] = json!({ "l1": { "@type": "Link", "blobId": blob, "rel": "enclosure" } });
    let reply = owner
        .jmap_method_call(
            "CalendarEvent/set",
            json!({ "accountId": account, "create": { "e": object } }),
        )
        .await;
    let refused = &reply.response_at(0)["notCreated"]["e"];
    assert_eq!(
        set_error(refused),
        ("invalidProperties", Some(ATTACHMENTS)),
        "{reply:?}"
    );
    assert_eq!(refused["properties"], json!(["links"]), "{reply:?}");
    assert_eq!(uid_copies(test, owner_id, "jmapcal-refusals-link").await, 0);

    // The key principal's availability, asked by the administrator, over the
    // hour the owner's event is busy.
    let admin = test.account("admin@example.com");
    let reply = admin
        .jmap_method_call(
            "Principal/getAvailability",
            json!({
                "accountId": admin.id_string(),
                "id": account,
                "utcStart": "2099-03-01T00:00:00Z",
                "utcEnd": "2099-03-02T00:00:00Z",
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(reply.list_array(), &json!([]), "{reply:?}");

    // Participant identities: a create, an update and a destroy.
    let refused = json!({ "type": "forbidden", "description": FIXED });
    let reply = set(
        owner,
        json!({ "create": { "c": {
            "name": "Work", "calendarAddress": "mailto:work@example.com"
        } } }),
    )
    .await;
    assert_eq!(
        reply.response_at(0)["notCreated"]["c"],
        refused,
        "{reply:?}"
    );
    let reply = set(owner, json!({ "update": { "a": { "name": "Work" } } })).await;
    assert_eq!(
        reply.response_at(0)["notUpdated"]["a"],
        refused,
        "{reply:?}"
    );
    let reply = set(owner, json!({ "destroy": ["a"] })).await;
    assert_eq!(
        reply.response_at(0)["notDestroyed"]["a"],
        refused,
        "{reply:?}"
    );
    assert_eq!(identities(owner).await.as_array().unwrap().len(), 2);

    let reply = owner
        .jmap_destroy(
            "CalendarEvent",
            [prefs.as_str()],
            Vec::<(&str, &str)>::new(),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    dav.request_with_headers("DELETE", &home(owner, SLUG), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Invariant 66: a scheduling inbox item is never listed, opened or destroyed
/// through JMAP. The store is read before and after.
async fn notification_destroy_leaves_the_item(
    test: &TestServer,
    owner: &Account,
    inviter: &Account,
    inviter_id: u32,
) {
    const UID: &str = "jmapcal-refusals-notif";
    let owner_id = owner.id().document_id();
    let account = owner.id_string();
    assert!(pending_ids(test, owner_id).await.is_empty());
    let dav2 = DummyWebDavClient::new(inviter_id, inviter.name(), STRONG, inviter.name());
    let path = format!("{}{UID}.ics", home(inviter, "default"));
    dav2.request_with_headers(
        "PUT",
        &path,
        [CONTENT_TYPE],
        format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
                "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990301T100000Z\r\n",
                "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:jmapcal refusals invitation\r\n",
                "ORGANIZER:mailto:{organizer}\r\n",
                "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            uid = UID,
            organizer = inviter.name(),
            attendee = owner.name(),
        ),
    )
    .await
    .with_status(StatusCode::CREATED);
    // The suite's drain skip keeps the item pending.
    let pending = pending_ids(test, owner_id).await;
    assert_eq!(
        pending.len(),
        1,
        "the invitation's pending item: {pending:?}"
    );
    let item = Id::from(pending[0]).to_string();

    let reply = owner
        .jmap_method_call(
            "CalendarEventNotification/set",
            json!({ "accountId": account, "destroy": [&item] }),
        )
        .await;
    assert_eq!(
        method_error(&reply),
        Some("accountNotSupportedByMethod"),
        "{reply:?}"
    );
    assert_eq!(
        pending_ids(test, owner_id).await,
        pending,
        "the destroy removed the pending item"
    );
    let reply = owner
        .jmap_method_call(
            "CalendarEventNotification/get",
            json!({ "accountId": account, "ids": [&item] }),
        )
        .await;
    assert_eq!(
        method_error(&reply),
        Some("accountNotSupportedByMethod"),
        "{reply:?}"
    );

    Party::new(test, "jmapcal-refusals@example.com")
        .delete_items(test, pending)
        .await;
    dav2.request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(pending_ids(test, owner_id).await.is_empty());
    assert_eq!(uid_copies(test, owner_id, UID).await, 0);
    assert_eq!(uid_copies(test, inviter_id, UID).await, 0);
}

async fn changes(owner: &Account, method: &str, since: &State, max: Option<usize>) -> JmapResponse {
    let mut args = json!({ "accountId": owner.id_string(), "sinceState": since.to_string() });
    if let Some(max) = max {
        args["maxChanges"] = json!(max);
    }
    owner.jmap_method_call(method, args).await
}

fn new_state(reply: &JmapResponse) -> State {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["newState"]
        .as_str()
        .and_then(State::parse)
        .unwrap_or_else(|| panic!("no parsable newState: {reply:?}"))
}

/// The first page of the owner's paged sync of `method` from `since`: a `z`
/// continuation of `origin`, as the served changes path encodes it.
async fn first_page(
    owner: &Account,
    method: &str,
    since: &State,
    origin: ZaCalendarOrigin,
) -> String {
    let reply = changes(owner, method, since, Some(1)).await;
    assert_eq!(reply.response_at(0)["hasMoreChanges"], true, "{reply:?}");
    let encoded = reply.response_at(0)["newState"]
        .as_str()
        .unwrap()
        .to_string();
    match new_state(&reply) {
        State::ZaCalendarIntermediate { origin: got, .. } => {
            assert_eq!(got, origin, "{method}: {reply:?}");
        }
        state => panic!("{method}: expected a z continuation, got {state:?}"),
    }
    assert!(encoded.starts_with('z'), "{encoded}");
    encoded
}

/// A string that starts with `z` and that `State::parse` refuses, derived
/// from a served continuation.
fn malformed_z(valid: &str) -> String {
    let mut candidates = vec![format!("z{}", &valid[1..].replace('a', "b"))];
    candidates.extend((2..valid.len()).rev().map(|len| valid[..len].to_string()));
    candidates.push("z".to_string());
    candidates
        .into_iter()
        .find(|candidate| State::parse(candidate).is_none())
        .unwrap()
}

/// Spec 6.8 (D29): a key-calendar continuation is a sync position, never a
/// write precondition. As `ifInState` on an authorized write it is a state
/// mismatch and nothing changes, as is a malformed `z`; the refused copy keeps
/// its admission refusal; an identity changes slot refuses it as an argument;
/// and a keyless or unauthorized caller is answered as without it.
async fn calendar_continuation_is_not_a_write_precondition(
    test: &TestServer,
    owner: &Account,
    stranger: &Account,
) {
    let owner_id = owner.id().document_id();
    let account = owner.id_string();
    let dav = DummyWebDavClient::new(owner_id, owner.name(), STRONG, owner.name());
    let slugs = [
        "jmapcal-refusals-k1",
        "jmapcal-refusals-k2",
        "jmapcal-refusals-k3",
    ];
    let uids = [
        "jmapcal-refusals-e1",
        "jmapcal-refusals-e2",
        "jmapcal-refusals-e3",
    ];

    // Valid `z` states of each origin, from the owner's own paged syncs.
    let k1 = Id::from(make_calendar(test, owner, &dav, slugs[0]).await).to_string();
    let t = new_state(&changes(owner, "Calendar/changes", &State::Initial, None).await);
    assert!(matches!(t, State::Exact(_)), "{t:?}");
    for slug in &slugs[1..] {
        make_calendar(test, owner, &dav, slug).await;
    }
    let e1 = Id::from(put_event(test, owner, &dav, slugs[0], uids[0], uids[0]).await).to_string();
    let te = new_state(&changes(owner, "CalendarEvent/changes", &State::Initial, None).await);
    assert!(matches!(te, State::Exact(_)), "{te:?}");
    for uid in &uids[1..] {
        put_event(test, owner, &dav, slugs[0], uid, uid).await;
    }
    let za_states = [
        first_page(
            owner,
            "Calendar/changes",
            &State::Initial,
            ZaCalendarOrigin::Initial,
        )
        .await,
        first_page(
            owner,
            "Calendar/changes",
            &t,
            ZaCalendarOrigin::Exact(t.get_change_id()),
        )
        .await,
        first_page(
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            ZaCalendarOrigin::Initial,
        )
        .await,
        first_page(
            owner,
            "CalendarEvent/changes",
            &te,
            ZaCalendarOrigin::Exact(te.get_change_id()),
        )
        .await,
    ];
    let malformed = malformed_z(&za_states[1]);
    assert!(malformed.starts_with('z'), "{malformed}");
    assert_eq!(State::parse(&malformed), None, "{malformed}");

    let read = |method: &'static str, id: String| async move {
        let reply = owner
            .jmap_method_call(method, json!({ "accountId": account, "ids": [id] }))
            .await;
        assert_eq!(method_error(&reply), None, "{reply:?}");
        (
            reply.state().to_string(),
            reply.list().first().cloned().expect("the object"),
        )
    };
    let calendar_before = read("Calendar/get", k1.clone()).await;
    let event_before = read("CalendarEvent/get", e1.clone()).await;
    let calendar_update = |value: Option<&str>| {
        let mut args = json!({
            "accountId": account,
            "update": { &k1: { "name": "jmapcal-refusals-changed" } },
        });
        if let Some(value) = value {
            args["ifInState"] = json!(value);
        }
        args
    };
    let event_update = |value: Option<&str>| {
        let mut args = json!({
            "accountId": account,
            "update": { &e1: { "title": "jmapcal-refusals-changed" } },
        });
        if let Some(value) = value {
            args["ifInState"] = json!(value);
        }
        args
    };

    for value in za_states.iter().chain([&malformed]) {
        // The owner, with its password, on its own account.
        for (method, args) in [
            ("Calendar/set", calendar_update(Some(value))),
            ("CalendarEvent/set", event_update(Some(value))),
        ] {
            let reply = owner.jmap_method_call(method, args).await;
            assert_eq!(
                method_error(&reply),
                Some("stateMismatch"),
                "{method} ifInState {value}: {reply:?}"
            );
        }
        assert_eq!(
            read("Calendar/get", k1.clone()).await,
            calendar_before,
            "{value}"
        );
        assert_eq!(
            read("CalendarEvent/get", e1.clone()).await,
            event_before,
            "{value}"
        );

        // The refused copy keeps its admission refusal in every state slot.
        for (slot, extra) in [
            ("ifInState", json!({})),
            ("ifFromInState", json!({})),
            (
                "destroyFromIfInState",
                json!({ "onSuccessDestroyOriginal": true }),
            ),
        ] {
            let mut args = json!({
                "fromAccountId": account, "accountId": account,
                slot: value,
                "create": { &e1: { "id": &e1, "calendarIds": { &k1: true } } },
            });
            for (key, item) in extra.as_object().unwrap() {
                args[key] = item.clone();
            }
            let reply = owner.jmap_method_call("CalendarEvent/copy", args).await;
            assert_eq!(
                method_error(&reply),
                Some("accountNotSupportedByMethod"),
                "CalendarEvent/copy {slot} {value}: {reply:?}"
            );
        }
    }
    assert_eq!(uid_copies(test, owner_id, uids[0]).await, 1);

    // An out-of-scope changes slot of the key account refuses a served `z`.
    for value in &za_states {
        let reply = owner
            .jmap_method_call(
                "ParticipantIdentity/changes",
                json!({ "accountId": account, "sinceState": value }),
            )
            .await;
        assert_eq!(
            method_error(&reply),
            Some("invalidArguments"),
            "ParticipantIdentity/changes {value}: {reply:?}"
        );
    }

    // A keyless token and a caller without access are answered as they are
    // without the `z` value: it supplies neither keys nor rights.
    let (grant, keyless) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;
    for value in za_states.iter().chain([&malformed]) {
        for (method, with, without) in [
            (
                "Calendar/set",
                calendar_update(Some(value)),
                calendar_update(None),
            ),
            (
                "CalendarEvent/set",
                event_update(Some(value)),
                event_update(None),
            ),
        ] {
            let (status, reply) =
                za_bearer_jmap(&keyless, CALENDAR_JMAP_USING, one(method, &with)).await;
            assert_eq!(status, 200, "{reply:?}");
            let (status, control) =
                za_bearer_jmap(&keyless, CALENDAR_JMAP_USING, one(method, &without)).await;
            assert_eq!(status, 200, "{control:?}");
            assert_calendar_keyless(&control, &format!("keyless {method}"));
            assert_eq!(
                reply.0["methodResponses"][0], control.0["methodResponses"][0],
                "keyless {method} ifInState {value}"
            );

            let reply = stranger.jmap_method_call(method, with.clone()).await;
            let control = stranger.jmap_method_call(method, without.clone()).await;
            assert_eq!(
                method_error(&control),
                Some("accountNotFound"),
                "unauthorized {method}: {control:?}"
            );
            assert_eq!(
                reply.0["methodResponses"][0], control.0["methodResponses"][0],
                "unauthorized {method} ifInState {value}"
            );
        }
    }
    za_revoke_grant(&test.server, grant).await;
    assert_eq!(
        read("Calendar/get", k1.clone()).await,
        calendar_before,
        "after the controls"
    );
    assert_eq!(
        read("CalendarEvent/get", e1.clone()).await,
        event_before,
        "after the controls"
    );

    // The same updates apply, and the same writers accept the current state as
    // `ifInState`: the mismatches above are the `z` value's, not the update's.
    let (current, _) = read("Calendar/get", k1.clone()).await;
    let reply = owner
        .jmap_method_call("Calendar/set", calendar_update(Some(&current)))
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    reply.updated(&k1);
    let (current, _) = read("CalendarEvent/get", e1.clone()).await;
    let reply = owner
        .jmap_method_call("CalendarEvent/set", event_update(Some(&current)))
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    reply.updated(&e1);
    let calendar_after = read("Calendar/get", k1.clone()).await;
    assert_ne!(calendar_after, calendar_before, "the current-state update");
    assert_eq!(calendar_after.1["name"], "jmapcal-refusals-changed");
    let event_after = read("CalendarEvent/get", e1.clone()).await;
    assert_ne!(event_after, event_before, "the current-state update");
    assert_eq!(event_after.1["title"], "jmapcal-refusals-changed");

    for slug in slugs {
        dav.request_with_headers("DELETE", &home(owner, slug), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for uid in uids {
        assert_eq!(uid_copies(test, owner_id, uid).await, 0, "{uid}");
    }
}
