/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    account::Account, jmap::JmapResponse, server::TestServer, webdav::DummyWebDavClient,
    za::SERVER_URL,
};
use common::auth::vault::RawValue;
use serde_json::{Value, json};
use std::time::Duration;
use store::{ValueKey, write::BatchBuilder};
use types::{collection::Collection, field::PrincipalField};
use vault::tokens::{SECRET_SEPARATOR, ZaTokenRecord};

pub(crate) const CALENDAR_JMAP_USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:calendars:parse",
    "urn:ietf:params:jmap:principals",
    "urn:ietf:params:jmap:mail",
];

pub fn za_split_token(token: &str) -> (&str, Option<&str>) {
    token
        .split_once(SECRET_SEPARATOR)
        .map_or((token, None), |(upstream, secret)| (upstream, Some(secret)))
}

pub fn za_tamper_secret(token: &str) -> String {
    let (upstream, secret) = za_split_token(token);
    let secret = secret.expect("expected a token secret");
    let first = if secret.starts_with('A') { 'B' } else { 'A' };
    format!("{upstream}{SECRET_SEPARATOR}{first}{}", &secret[1..])
}

pub async fn za_bearer_jmap(token: &str, using: &[&str], calls: Value) -> (u16, JmapResponse) {
    let (status, body) = bearer_json(
        bearer_client()
            .post(format!("{SERVER_URL}/jmap"))
            .bearer_auth(token)
            .json(&json!({ "using": using, "methodCalls": calls })),
    )
    .await;
    (status, JmapResponse(body))
}

pub async fn za_bearer_session(token: &str) -> (u16, Value) {
    bearer_json(
        bearer_client()
            .get(format!("{SERVER_URL}/jmap/session"))
            .bearer_auth(token),
    )
    .await
}

pub fn za_bearer_dav(account: &Account, token: &str) -> DummyWebDavClient {
    let mut client = DummyWebDavClient::new(
        account.id().document_id(),
        account.name(),
        account.secret(),
        account.name(),
    );
    client.credentials = format!("Bearer {token}");
    client
}

fn bearer_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}

async fn bearer_json(request: reqwest::RequestBuilder) -> (u16, Value) {
    let response = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("Bearer request failed"));
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("Bearer response read failed"));
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// The stored bytes of the account's token record (field 153).
pub async fn raw_token_record(test: &TestServer, account_id: u32) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<RawValue>(ValueKey::property(
            account_id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessTokens,
        ))
        .await
        .unwrap()
        .map(|raw| raw.0)
}

/// An unconditional set of field 153 to `raw`, or its removal with `None`.
pub async fn set_raw_token_record(test: &TestServer, account_id: u32, raw: Option<&[u8]>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    match raw {
        Some(raw) => batch.set(PrincipalField::ZeroAccessTokens, raw.to_vec()),
        None => batch.clear(PrincipalField::ZeroAccessTokens),
    };
    test.server.store().write_batch(&mut batch).await.unwrap();
}

pub async fn token_record(test: &TestServer, account_id: u32) -> Option<ZaTokenRecord> {
    test.server.za_token_read(account_id).await.unwrap().record
}
