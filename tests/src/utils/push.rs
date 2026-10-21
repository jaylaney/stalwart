/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The mock Web Push server of the push subscription tests, on port 19000:
//! every POST must carry the server's VAPID authorization; an `aes128gcm`
//! body is decrypted with the subscription keys; each JSON payload is handed
//! to the test.

use crate::{AssertConfig, utils::server::TestServer};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{config::server::Listeners, network::SessionData};
use ece::EcKeyComponents;
use http_proto::{HtmlResponse, ToHttpResponse, request::fetch_body};
use hyper::{
    StatusCode, body,
    header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use registry::{
    schema::{
        enums::NetworkListenerProtocol,
        prelude::{ObjectType, SocketAddr},
        structs::{NetworkListener, SystemSettings},
    },
    types::{id::ObjectId, map::Map},
};
use serde_json::Value;
use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use store::registry::{RegistryObject, bootstrap::Bootstrap};
use tokio::sync::{mpsc, watch};

/// The mock server's origin; subscriptions push to `{PUSH_ORIGIN}/push`.
pub const PUSH_ORIGIN: &str = "https://127.0.0.1:19000";

/// A running mock push server; dropping it stops the listener.
pub struct MockPush {
    pub server: Arc<PushServer>,
    /// The subscription keys' public key (P-256, raw) and auth secret, to
    /// register as `p256dh` and `auth`.
    pub public_key: Vec<u8>,
    pub auth_secret: Vec<u8>,
    _shutdown: (watch::Sender<bool>, watch::Receiver<bool>),
}

/// Starts the mock push server; each payload it receives (a push
/// verification or a push object, decrypted) arrives on the receiver.
pub async fn spawn_mock_push_server(test: &TestServer) -> (mpsc::Receiver<Value>, MockPush) {
    let (tx, rx) = mpsc::channel::<Value>(100);
    let (keypair, auth_secret) = ece::generate_keypair_and_auth_secret().unwrap();
    let vapid_public_key = test
        .server
        .core
        .jmap
        .vapid
        .as_ref()
        .expect("A VAPID key must be configured")
        .public_key()
        .to_string();
    let server = Arc::new(PushServer {
        keypair: keypair.raw_components().unwrap(),
        auth_secret: auth_secret.to_vec(),
        vapid_public_key,
        endpoint_origin: PUSH_ORIGIN.to_string(),
        tx,
        fail_requests: false.into(),
    });

    let mut bp = Bootstrap::new_uninitialized(test.server.registry().clone());
    let mut servers = Listeners::default();
    servers.parse_server(
        &mut bp,
        RegistryObject {
            id: ObjectId::new(ObjectType::NetworkListener, 0u64.into()),
            object: NetworkListener {
                name: "mock-push".into(),
                bind: Map::new(vec![SocketAddr::from_str("127.0.0.1:19000").unwrap()]),
                protocol: NetworkListenerProtocol::Http,
                tls_implicit: true,
                use_tls: true,
                socket_reuse_address: true,
                socket_reuse_port: true,
                ..Default::default()
            },
            revision: 0,
        },
        &SystemSettings::default(),
    );
    servers
        .parse_tcp_acceptors(&mut bp, test.server.inner.clone())
        .await;
    servers.bind_and_drop_priv(&mut bp);
    bp.assert_no_errors();
    let shutdown = servers.spawn(|listener, acceptor, shutdown_rx| {
        listener.spawn(
            SessionManager::from(server.clone()),
            test.server.inner.clone(),
            acceptor,
            shutdown_rx,
        );
    });

    (
        rx,
        MockPush {
            server,
            public_key: keypair.pub_as_raw().unwrap(),
            auth_secret: auth_secret.to_vec(),
            _shutdown: shutdown,
        },
    )
}

#[derive(Clone)]
pub struct SessionManager {
    pub inner: Arc<PushServer>,
}

impl From<Arc<PushServer>> for SessionManager {
    fn from(inner: Arc<PushServer>) -> Self {
        SessionManager { inner }
    }
}

pub struct PushServer {
    keypair: EcKeyComponents,
    auth_secret: Vec<u8>,
    vapid_public_key: String,
    endpoint_origin: String,
    tx: mpsc::Sender<Value>,
    /// While set, every request is answered 429.
    pub fail_requests: AtomicBool,
}

impl common::network::SessionManager for SessionManager {
    #[allow(clippy::manual_async_fn)]
    fn handle<T: common::network::SessionStream>(
        self,
        session: SessionData<T>,
    ) -> impl std::future::Future<Output = ()> + Send {
        async move {
            let push = self.inner;
            let _ = http1::Builder::new()
                .keep_alive(false)
                .serve_connection(
                    TokioIo::new(session.stream),
                    service_fn(|mut req: hyper::Request<body::Incoming>| {
                        let push = push.clone();

                        async move {
                            if push.fail_requests.load(Ordering::Relaxed) {
                                return Ok(HtmlResponse::with_status(
                                    StatusCode::TOO_MANY_REQUESTS,
                                    "too many requests".to_string(),
                                )
                                .into_http_response()
                                .build());
                            }

                            // Every push POST must be authenticated with a VAPID token (RFC 9749)
                            let authorization = req
                                .headers()
                                .get(AUTHORIZATION)
                                .map(|value| value.to_str().unwrap().to_string())
                                .expect("Push POST must carry a VAPID Authorization header");
                            assert_vapid_authorization(
                                &authorization,
                                &push.vapid_public_key,
                                &push.endpoint_origin,
                            );

                            let is_encrypted = req
                                .headers()
                                .get(CONTENT_ENCODING)
                                .is_some_and(|encoding| encoding.to_str().unwrap() == "aes128gcm");

                            let content_type = req
                                .headers()
                                .get(CONTENT_TYPE)
                                .map(|value| value.to_str().unwrap())
                                .expect("Push POST must carry a Content-Type header");
                            assert_eq!(
                                content_type,
                                if is_encrypted {
                                    "application/octet-stream"
                                } else {
                                    "application/json"
                                },
                                "unexpected Content-Type for encrypted={is_encrypted} push"
                            );
                            let body = fetch_body(&mut req, 1024 * 1024, 0).await.unwrap();
                            let message = serde_json::from_slice::<Value>(&if is_encrypted {
                                ece::decrypt(&push.keypair, &push.auth_secret, &body).unwrap()
                            } else {
                                body
                            })
                            .unwrap();

                            //println!("Push received ({}): {:?}", is_encrypted, message);

                            push.tx.send(message).await.unwrap();

                            Ok::<_, hyper::Error>(
                                HtmlResponse::new("ok".to_string())
                                    .into_http_response()
                                    .build(),
                            )
                        }
                    }),
                )
                .await;
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn shutdown(&self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

fn assert_vapid_authorization(header: &str, expected_key: &str, expected_origin: &str) {
    let (token, key) = header
        .strip_prefix("vapid ")
        .and_then(|rest| rest.split_once(", "))
        .expect("VAPID header must be 'vapid t=<jwt>, k=<key>'");
    let jwt = token.strip_prefix("t=").expect("Missing t= parameter");
    let key = key.strip_prefix("k=").expect("Missing k= parameter");
    assert_eq!(
        key, expected_key,
        "The k= parameter must match the advertised applicationServerKey"
    );

    let parts = jwt.split('.').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3, "A JWT must have three parts");
    let decode = |part: &str| {
        URL_SAFE_NO_PAD
            .decode(part)
            .expect("Each JWT part must be base64url encoded")
    };
    assert_eq!(
        decode(parts[0]),
        br#"{"typ":"JWT","alg":"ES256"}"#,
        "The JWT header must declare typ JWT and alg ES256"
    );

    let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
    assert_eq!(
        claims["aud"], expected_origin,
        "The aud claim must be the push endpoint origin"
    );
    let now = store::write::now();
    let exp = claims["exp"]
        .as_u64()
        .expect("The exp claim must be a number");
    assert!(
        exp > now && exp <= now + 24 * 3600,
        "The exp claim must be no more than 24 hours in the future (exp={exp}, now={now})"
    );
}
