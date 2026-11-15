/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    dav_seal::{resource_id, share_calendar},
    fanout::{CONTENT_TYPE, MKCALENDAR},
    user_permissions,
};
use crate::utils::{
    account::Account,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{za_uid_leases, za_unlock},
};
use ::registry::schema::enums::Permission;
use common::auth::AccessToken;
use hyper::StatusCode;

pub mod pipeline;

/// A module's two key accounts and their CalDAV clients.
pub(super) struct Pair {
    pub key: Account,
    pub key2: Account,
    pub key_id: u32,
    pub key2_id: u32,
    pub dav: DummyWebDavClient,
    #[allow(dead_code)]
    pub dav2: DummyWebDavClient,
}

pub(super) async fn pair(test: &mut TestServer, module: &str) -> Pair {
    let owner: &'static str = format!("jmapcal-{module}").leak();
    let grantee: &'static str = format!("jmapcal-{module}-2").leak();
    let admin = test.account("admin@example.com");
    let key = admin
        .create_key_user_account(
            format!("{owner}@example.com").leak(),
            STRONG,
            owner,
            &[],
            user_permissions(),
        )
        .await;
    let key2 = admin
        .create_key_user_account(
            format!("{grantee}@example.com").leak(),
            STRONG,
            grantee,
            &[],
            user_permissions(),
        )
        .await;
    test.insert_account(key.clone());
    test.insert_account(key2.clone());
    let key_id = key.id().document_id();
    let key2_id = key2.id().document_id();
    Pair {
        key_id,
        key2_id,
        dav: DummyWebDavClient::new(key_id, key.name(), STRONG, key.name()),
        dav2: DummyWebDavClient::new(key2_id, key2.name(), STRONG, key2.name()),
        key,
        key2,
    }
}

#[allow(dead_code)]
pub(super) async fn key_account(
    test: &mut TestServer,
    local: &str,
    permissions: Vec<Permission>,
) -> (Account, u32) {
    let local: &'static str = format!("jmapcal-{local}").leak();
    let key = test
        .account("admin@example.com")
        .create_key_user_account(
            format!("{local}@example.com").leak(),
            STRONG,
            local,
            &[],
            permissions,
        )
        .await;
    let id = key.id().document_id();
    test.insert_account(key.clone());
    (key, id)
}

pub(super) async fn make_calendar(
    test: &TestServer,
    owner: &Account,
    dav: &DummyWebDavClient,
    slug: &str,
) -> u32 {
    let home = owner.name().replace('@', "%40");
    dav.request(
        "MKCALENDAR",
        &format!("/dav/cal/{home}/{slug}/"),
        MKCALENDAR,
    )
    .await
    .with_status(StatusCode::CREATED);
    resource_id(test, owner.id().document_id(), slug).await
}

pub(super) async fn share(
    test: &TestServer,
    owner: &Account,
    grantee: &Account,
    grantee_id: u32,
    slug: &str,
    level: &str,
) {
    share_calendar(test, owner.name(), grantee.name(), grantee_id, slug, level).await;
}

pub(super) fn event_ics(uid: &str, summary: &str) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990301T100000Z\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:{summary}\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        summary = summary,
    )
}

pub(super) async fn put_event(
    test: &TestServer,
    owner: &Account,
    dav: &DummyWebDavClient,
    slug: &str,
    uid: &str,
    summary: &str,
) -> u32 {
    let home = owner.name().replace('@', "%40");
    let path = format!("{slug}/{uid}.ics");
    dav.request_with_headers(
        "PUT",
        &format!("/dav/cal/{home}/{path}"),
        [CONTENT_TYPE],
        event_ics(uid, summary),
    )
    .await
    .with_status(StatusCode::CREATED);
    resource_id(test, owner.id().document_id(), &path).await
}

pub(super) async fn session_token(test: &TestServer, account_id: u32) -> AccessToken {
    AccessToken::new_maybe_invalid(test.server.access_token(account_id).await.unwrap())
        .with_session_keys(za_unlock(test, account_id, STRONG).await)
}

pub(super) async fn finish(test: &TestServer, module: &str) {
    assert_eq!(za_uid_leases(test).await, 0, "{module} left UID leases");
    test.wait_for_tasks().await;
}
