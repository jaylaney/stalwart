/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::auth::AccessToken;
use common::{HttpAuthCache, Server, auth::AuthRequest, network::limiter::InFlight};
use directory::Credentials;
use http_proto::{HttpRequest, HttpSessionData};
use hyper::header;
use mail_parser::decoders::base64::base64_decode;
use std::future::Future;
use std::time::{Duration, Instant};

pub trait Authenticator: Sync + Send {
    fn authenticate_headers(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
    ) -> impl Future<Output = trc::Result<(Option<InFlight>, AccessToken)>> + Send;

    fn authenticate_uncached(
        &self,
        mechanism: &str,
        token: &str,
        fp: [u8; 32],
        session: &HttpSessionData,
    ) -> impl Future<Output = trc::Result<(Option<InFlight>, AccessToken)>> + Send;
}

impl Authenticator for Server {
    async fn authenticate_headers(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
    ) -> trc::Result<(Option<InFlight>, AccessToken)> {
        if let Some((mechanism, token)) = req.authorization() {
            // The cache key is a keyed fingerprint of the header value; the
            // value itself is never stored (spec 5).
            let fp = self.inner.cache.za_fingerprint(token);

            // Check if the credentials are cached
            if let Some(http_cache) = self.inner.cache.http_auth.get(&fp) {
                // Make sure the revision is still valid
                if http_cache.expires > Instant::now() {
                    let access_token = AccessToken::renew(
                        self.access_token(http_cache.account_id).await?,
                        http_cache.credential_id,
                        session.remote_ip,
                    )?;

                    if access_token.revision() == http_cache.revision {
                        if http_cache.generation == 0 {
                            // Non-key account: unchanged upstream path.
                            // Enforce authenticated rate limit
                            return self
                                .is_http_authenticated_request_allowed(
                                    &access_token,
                                    session.remote_ip,
                                )
                                .await
                                .map(|in_flight| (in_flight, access_token));
                        }

                        // Generation fence (spec 5): the entry must match the
                        // account's current authentication generation, and the
                        // resident keys must still be present and current.
                        // Anything else forces a full verification.
                        let account = self.account(http_cache.account_id).await?;
                        if account.za_generation == http_cache.generation
                            && let Some(keys) = self.inner.cache.za_keys.get(&fp, Instant::now())
                            && keys.generation == http_cache.generation
                            && keys.account_id == http_cache.account_id
                        {
                            let access_token = access_token.with_session_keys(keys);

                            // Enforce authenticated rate limit
                            return self
                                .is_http_authenticated_request_allowed(
                                    &access_token,
                                    session.remote_ip,
                                )
                                .await
                                .map(|in_flight| (in_flight, access_token));
                        }
                    }
                }

                // If the entry is not valid, remove the cached credentials and keys
                self.inner.cache.http_auth.remove(&fp);
                self.inner.cache.za_keys.remove(&fp);
            }

            self.authenticate_uncached(mechanism, token, fp, session)
                .await
        } else {
            // Enforce anonymous rate limit
            self.is_http_anonymous_request_allowed(session.remote_ip)
                .await?;

            Err(trc::AuthEvent::Failed
                .into_err()
                .details("Missing Authorization header.")
                .caused_by(trc::location!()))
        }
    }

    async fn authenticate_uncached(
        &self,
        mechanism: &str,
        token: &str,
        fp: [u8; 32],
        session: &HttpSessionData,
    ) -> trc::Result<(Option<InFlight>, AccessToken)> {
        // The raw header value never reaches an event (spec 5).
        let credentials = if mechanism.eq_ignore_ascii_case("basic") {
            // Decode the base64 encoded credentials
            decode_plain_auth(token).ok_or_else(|| {
                trc::AuthEvent::Error
                    .into_err()
                    .details("Failed to decode Basic auth request.")
                    .caused_by(trc::location!())
            })?
        } else if mechanism.eq_ignore_ascii_case("bearer") {
            // Enforce anonymous rate limit
            self.is_http_anonymous_request_allowed(session.remote_ip)
                .await?;

            Credentials::Bearer {
                username: None,
                token: token.to_string(),
            }
        } else {
            // Enforce anonymous rate limit
            self.is_http_anonymous_request_allowed(session.remote_ip)
                .await?;

            return Err(trc::AuthEvent::Error
                .into_err()
                .reason("Unsupported authentication mechanism.")
                .caused_by(trc::location!()));
        };

        // Authenticate
        let access_token = self
            .authenticate(&AuthRequest::from_credentials(
                credentials,
                session.session_id,
                session.remote_ip,
            ))
            .await?;

        #[cfg(feature = "test_mode")]
        za_test::pause_point(access_token.account_id()).await;

        let account_id = access_token.account_id();
        let expires = Instant::now() + Duration::from_secs(self.core.oauth.oauth_expiry_token);
        let mut access_token = access_token;
        match access_token.session_keys().cloned() {
            None => {
                // A keyless result for a key account (Bearer/OAuth, or a
                // password verified against the pre-setup hash while setup
                // committed) must not survive in the cache: a generation-0
                // entry is served on later hits without the fence. Only
                // non-key accounts are cached here, as upstream does.
                if !self.account(account_id).await?.is_key_account() {
                    self.inner.cache.http_auth.insert(
                        fp,
                        HttpAuthCache {
                            account_id,
                            revision: access_token.revision(),
                            credential_id: access_token.credential_id(),
                            expires,
                            generation: 0,
                        },
                    );

                    // Double-check after publish: the check above may have read
                    // a stale non-key entry while setup's `Account` invalidation
                    // was still pending. If the account is a key account now,
                    // withdraw the entry just inserted. A failed lookup also
                    // withdraws it (fail closed) before the error propagates.
                    match self.account(account_id).await {
                        Ok(account) if !account.is_key_account() => {}
                        result => {
                            self.inner.cache.http_auth.remove(&fp);
                            result?;
                        }
                    }
                }
            }
            Some(keys) if keys.account_id != account_id => {
                // Keys of another account must never travel with this token:
                // serve the request keyless and cache nothing.
                trc::error!(
                    trc::AuthEvent::Error
                        .into_err()
                        .details("Session keys do not belong to the authenticated account.")
                        .account_id(account_id)
                        .ctx(trc::Key::Id, keys.account_id)
                        .caused_by(trc::location!())
                );
                access_token = access_token.without_session_keys();
            }
            Some(keys) => {
                // Generation fence (spec 5). Race defended: this login read the
                // vault record at revision r and spent its time in Argon2 while
                // a password change committed r+1 and called `remove_account`.
                // Caching now would resurrect the superseded verification, so
                // both inserts happen only if the verified generation still
                // equals the one rebuilt from the store; otherwise the request
                // succeeds uncached. Generation 0 is never cached: such an
                // entry would be served from the non-key hit branch, unfenced,
                // with keys resident.
                let account = self.account(account_id).await?;
                if keys.generation != 0 && keys.generation == account.za_generation {
                    self.inner.cache.http_auth.insert(
                        fp,
                        HttpAuthCache {
                            account_id,
                            revision: access_token.revision(),
                            credential_id: access_token.credential_id(),
                            expires,
                            generation: keys.generation,
                        },
                    );
                    self.inner.cache.za_keys.insert(fp, keys, Instant::now());
                }
            }
        }

        // Enforce authenticated rate limit
        self.is_http_authenticated_request_allowed(&access_token, session.remote_ip)
            .await
            .map(|in_flight| (in_flight, access_token))
    }
}

/// Test-only pause points, each keyed on an account id and one-shot: only
/// the first request for that account reaching the point takes the pause;
/// requests for other accounts pass through and leave it set. `PAUSE` sits
/// between credential verification and the cache insert fence (paused
/// verification tests); `ENDPOINT_PAUSE` sits in the vault endpoints between
/// the fresh verification and the record re-read (generation fence tests).
#[cfg(feature = "test_mode")]
pub mod za_test {
    use std::sync::{Arc, Mutex};
    use tokio::sync::Notify;

    pub struct Pause {
        pub account_id: u32,
        pub arrived: Notify,
        pub release: Notify,
    }

    impl Pause {
        pub fn new(account_id: u32) -> Self {
            Pause {
                account_id,
                arrived: Notify::new(),
                release: Notify::new(),
            }
        }
    }

    pub static PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

    pub static ENDPOINT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

    pub fn set(pause: Option<Arc<Pause>>) {
        *PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
    }

    pub fn set_endpoint(pause: Option<Arc<Pause>>) {
        *ENDPOINT_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
    }

    fn take_for(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) -> Option<Arc<Pause>> {
        let mut pause = slot.lock().unwrap_or_else(|e| e.into_inner());
        if pause.as_ref().is_some_and(|p| p.account_id == account_id) {
            pause.take()
        } else {
            None
        }
    }

    async fn wait_at(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) {
        // The guard is dropped inside `take_for`, before any await.
        if let Some(pause) = take_for(slot, account_id) {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
    }

    pub(super) async fn pause_point(account_id: u32) {
        wait_at(&PAUSE, account_id).await;
    }

    pub(crate) async fn endpoint_pause_point(account_id: u32) {
        wait_at(&ENDPOINT_PAUSE, account_id).await;
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn pause_fires_once_for_its_account_only() {
            let pause = Arc::new(Pause::new(7));
            set(Some(pause.clone()));

            // Another account's request passes through and leaves it set.
            pause_point(8).await;
            assert!(PAUSE.lock().unwrap().is_some());

            let task = tokio::spawn(pause_point(7));
            pause.arrived.notified().await;
            assert!(
                PAUSE.lock().unwrap().is_none(),
                "the paused request takes the pause"
            );
            assert!(!task.is_finished());

            pause.release.notify_one();
            task.await.unwrap();

            // Unset: the pause point returns immediately.
            pause_point(7).await;
        }

        #[tokio::test]
        async fn endpoint_slot_is_independent() {
            let pause = Arc::new(Pause::new(9));
            set_endpoint(Some(pause.clone()));

            // The verification pause point does not consume the endpoint slot.
            pause_point(9).await;
            assert!(ENDPOINT_PAUSE.lock().unwrap().is_some());

            let task = tokio::spawn(endpoint_pause_point(9));
            pause.arrived.notified().await;
            assert!(ENDPOINT_PAUSE.lock().unwrap().is_none());
            pause.release.notify_one();
            task.await.unwrap();
        }
    }
}

pub trait HttpHeaders {
    fn authorization(&self) -> Option<(&str, &str)>;
    fn authorization_basic(&self) -> Option<&str>;
}

impl HttpHeaders for HttpRequest {
    fn authorization(&self) -> Option<(&str, &str)> {
        self.headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.split_once(' ').map(|(l, t)| (l, t.trim())))
    }

    fn authorization_basic(&self) -> Option<&str> {
        self.authorization().and_then(|(l, t)| {
            if l.eq_ignore_ascii_case("basic") {
                Some(t)
            } else {
                None
            }
        })
    }
}

fn decode_plain_auth(token: &str) -> Option<Credentials> {
    base64_decode(token.as_bytes())
        .and_then(|token| String::from_utf8(token).ok())
        .and_then(|token| {
            token
                .split_once(':')
                .map(|(login, secret)| Credentials::Basic {
                    username: login.trim().to_lowercase(),
                    secret: secret.to_string(),
                    mfa_token: None,
                })
        })
}
