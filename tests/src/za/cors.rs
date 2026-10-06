/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{ACCOUNT_PAGE_ORIGIN, STRONG};
use crate::utils::{server::TestServer, za::SERVER_URL};
use hyper::header;
use reqwest::{Method, Response};
use serde_json::json;
use std::time::Duration;

async fn send(method: Method, path: &str, body: Option<Vec<u8>>) -> Response {
    let mut request = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .request(method, format!("{SERVER_URL}{path}"))
        .header(header::ORIGIN, ACCOUNT_PAGE_ORIGIN);
    if let Some(body) = body {
        request = request
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
    }
    request.send().await.unwrap()
}

fn header_of<'x>(response: &'x Response, name: header::HeaderName) -> Option<&'x str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

fn assert_allows_origin(response: &Response) {
    assert_eq!(
        header_of(response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(ACCOUNT_PAGE_ORIGIN),
        "{} {:?}",
        response.status(),
        response.headers()
    );
    assert_eq!(header_of(response, header::VARY), Some("Origin"));
}

/// CORS for the account page (spec 4.1). The suite sets
/// `ZA_ACCOUNT_PAGE_ORIGIN` before the server starts; the unset case is
/// covered by the `http` crate's unit tests.
pub async fn test(test: &mut TestServer) {
    println!("Running zero-access CORS tests...");

    // Preflight on a vault path.
    let response = send(Method::OPTIONS, "/api/vault/password", None).await;
    assert_eq!(response.status().as_u16(), 204);
    assert_allows_origin(&response);
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_ALLOW_METHODS),
        Some("POST, OPTIONS")
    );
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_ALLOW_HEADERS),
        Some("Content-Type, Authorization")
    );
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_MAX_AGE),
        Some("600")
    );

    // Every vault response: success, bad request and unknown endpoint.
    let response = send(
        Method::POST,
        "/api/vault/recovery-key",
        Some(
            serde_json::to_vec(&json!({ "username": "key1@example.com", "password": STRONG }))
                .unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_allows_origin(&response);
    let reply: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    test.accounts
        .get_mut("key1@example.com")
        .unwrap()
        .recovery_key = Some(reply["recovery_key"].as_str().unwrap().to_string());
    let response = send(
        Method::POST,
        "/api/vault/recovery-key",
        Some(b"not json".to_vec()),
    )
    .await;
    assert_eq!(response.status().as_u16(), 400);
    assert_allows_origin(&response);
    let response = send(
        Method::POST,
        "/api/vault/no-such-endpoint",
        Some(b"{}".to_vec()),
    )
    .await;
    assert_eq!(response.status().as_u16(), 404);
    assert_allows_origin(&response);

    // Other API paths are unaffected.
    let response = send(Method::OPTIONS, "/api/auth", None).await;
    assert_eq!(response.status().as_u16(), 204);
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None
    );
    let response = send(Method::POST, "/api/auth", Some(b"not json".to_vec())).await;
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None
    );
}
