/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{
    HttpAuthCache, Server,
    auth::{
        AccessToken, AuthRequest,
        authentication::{Authenticated, UsernameParts},
    },
    cache::http_auth::RevocationEpoch,
    network::limiter::InFlight,
};
use directory::Credentials;
use encodify::base64::LENIENT;
use http_proto::{HttpRequest, HttpSessionData};
use hyper::header;
use std::future::Future;
use std::time::Instant;

pub struct HttpAuthenticated {
    pub in_flight: Option<InFlight>,
    pub access_token: AccessToken,
    pub epoch: RevocationEpoch,
}

pub trait Authenticator: Sync + Send {
    fn authenticate_headers(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
    ) -> impl Future<Output = trc::Result<HttpAuthenticated>> + Send;
}

impl Authenticator for Server {
    async fn authenticate_headers(
        &self,
        req: &HttpRequest,
        session: &HttpSessionData,
    ) -> trc::Result<HttpAuthenticated> {
        let revocations = &self.inner.cache.grant_revocations;
        let epoch = revocations.epoch();
        if let Some((mechanism, token)) = req.authorization() {
            // Check if the credentials are cached
            if let Some(http_cache) = self.inner.cache.http_auth.get(token) {
                // Make sure the revision is still valid
                if self.inner.cache.http_auth.is_live(&http_cache) {
                    let mut access_token = AccessToken::renew(
                        self.access_token(http_cache.account_id).await?,
                        http_cache.credential_id,
                        http_cache.token_scope,
                        session.remote_ip,
                    )?;

                    // Ruling 10 (mail ruling R35): an impersonation stays one.
                    if za_is_impersonation(token) {
                        access_token = access_token.with_za_impersonation();
                    }

                    if access_token.revision() == http_cache.revision
                        && let Some(access_token) =
                            za_cache_hit(self, token, http_cache.account_id, access_token).await?
                    {
                        // Enforce authenticated rate limit
                        return self
                            .is_http_authenticated_request_allowed(&access_token, session.remote_ip)
                            .await
                            .map(|in_flight| HttpAuthenticated {
                                in_flight,
                                access_token,
                                epoch,
                            });
                    }
                }

                // If the entry is not valid, remove the cached credentials and keys
                self.inner.cache.http_auth.remove(token);
                self.inner
                    .cache
                    .za_keys
                    .remove(&self.inner.cache.za_fingerprint(token));
            }

            authenticate_uncached(self, mechanism, token, session, epoch).await
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
}

/// The hit rule (interface sheet 2; C2). 1.0 keys `http_auth` on a hash of
/// the raw token, so an entry cannot be dropped when its keys are evicted or
/// swept: this rule alone keeps a key account's entry from being served
/// without them. A key account's entry is served only with resident keys of
/// the account's current authentication generation; `None` asks for a full
/// verification. An ordinary account's entry takes upstream's path.
async fn za_cache_hit(
    server: &Server,
    token: &str,
    account_id: u32,
    access_token: AccessToken,
) -> trc::Result<Option<AccessToken>> {
    let cache = &server.inner.cache;

    // Fail closed: an account that cannot be read is not served from the cache.
    let account = match server.account(account_id).await {
        Ok(account) => account,
        Err(err) => {
            cache.http_auth.remove(token);
            cache.za_keys.remove(&cache.za_fingerprint(token));
            return Err(err);
        }
    };

    if !account.is_key_account() {
        // Non-key account: unchanged upstream path.
        return Ok(Some(access_token));
    }

    // Generation fence (spec 5): the resident keys must still be present and
    // belong to the account's current authentication generation.
    let generation = account.za_generation();
    Ok(cache
        .za_keys
        .get(&cache.za_fingerprint(token), Instant::now())
        .filter(|keys| {
            keys.account_id == account_id && keys.generation != 0 && keys.generation == generation
        })
        .map(|keys| access_token.with_session_keys(keys)))
}

/// Ruling 10: R35's impersonation marker, re-derived from the cached token
/// itself whatever the mechanism. `HttpAuthEntries` keys on the token alone,
/// so a Bearer header carrying a Basic login's token hits that login's
/// entry. Every successful login whose username has the master form is an
/// impersonation; the marker only restricts, so a false positive fails safe.
fn za_is_impersonation(token: &str) -> bool {
    matches!(
        decode_plain_auth(token),
        Some(Credentials::Basic { username, .. }) if UsernameParts::new(&username).is_master()
    )
}

/// Full verification of an Authorization header, caching the result under
/// the header's own token, given here so that no caller can cache under
/// another key. The only place session keys are attached (spec 5).
async fn authenticate_uncached(
    server: &Server,
    mechanism: &str,
    token: &str,
    session: &HttpSessionData,
    epoch: RevocationEpoch,
) -> trc::Result<HttpAuthenticated> {
    // Ruling 28 (C3): the raw header value never reaches an error or event.
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
        server
            .is_http_anonymous_request_allowed(session.remote_ip)
            .await?;

        Credentials::Bearer {
            username: None,
            token: token.to_string(),
        }
    } else {
        // Enforce anonymous rate limit
        server
            .is_http_anonymous_request_allowed(session.remote_ip)
            .await?;

        return Err(trc::AuthEvent::Error
            .into_err()
            .reason("Unsupported authentication mechanism.")
            .caused_by(trc::location!()));
    };

    // Authenticate
    let (
        Authenticated {
            mut access_token,
            expires_in,
        },
        session_keys,
    ) = server
        .authenticate_with_keys(&AuthRequest::from_credentials(
            credentials,
            session.session_id,
            session.remote_ip,
        ))
        .await?;

    #[cfg(feature = "test_mode")]
    za_test::pause_point(access_token.account_id()).await;

    // Cache credentials
    let cache = &server.inner.cache;
    let revocations = &cache.grant_revocations;
    let account_id = access_token.account_id();
    let cache_ttl = expires_in.map_or(server.core.oauth.oauth_expiry_token, |expires_in| {
        expires_in.min(server.core.oauth.oauth_expiry_token)
    });
    let entry = HttpAuthCache {
        account_id,
        expires: cache.http_auth.expires_in(cache_ttl),
        credential_id: access_token.credential_id(),
        revision: access_token.revision(),
        token_scope: access_token.token_scope(),
    };
    match session_keys {
        None => {
            // A keyless result for a key account (Bearer/OAuth, an
            // impersonation, or a password verified against the pre-setup
            // hash while setup committed) is never cached: the hit rule would
            // refuse it anyway. Only ordinary accounts are cached here, as
            // upstream does, under upstream's revocation epoch.
            if !server.account(account_id).await?.is_key_account() {
                revocations.insert(&cache.http_auth, token, entry, epoch);

                // Double-check after publish: the check above may have read
                // a stale non-key entry while setup's `Account` invalidation
                // was still pending. If the account is a key account now,
                // withdraw the entry just inserted. A failed lookup also
                // withdraws it (fail closed) before the error propagates.
                match server.account(account_id).await {
                    Ok(account) if !account.is_key_account() => {}
                    result => {
                        cache.http_auth.remove(token);
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
        }
        Some(keys) => {
            // Generation fence (spec 5). Race defended: this login read the
            // vault record at revision r and spent its time in Argon2 while
            // a password change committed r+1 and called `remove_account`.
            // Caching now would resurrect the superseded verification, so
            // both inserts happen only if the verified generation still
            // equals the one rebuilt from the store; otherwise the request
            // succeeds uncached. Generation 0 is never cached. The entry
            // also passes upstream's revocation epoch.
            let account = server.account(account_id).await?;
            if keys.generation != 0 && keys.generation == account.za_generation() {
                revocations.insert(&cache.http_auth, token, entry, epoch);
                cache.za_insert_keys(cache.za_fingerprint(token), keys.clone(), Instant::now());
            }
            access_token = access_token.with_session_keys(keys);
        }
    }

    // Enforce authenticated rate limit
    server
        .is_http_authenticated_request_allowed(&access_token, session.remote_ip)
        .await
        .map(|in_flight| HttpAuthenticated {
            in_flight,
            access_token,
            epoch,
        })
}

/// Test-only pause points, each keyed on an account id and one-shot: only
/// the first request for that account reaching the point takes the pause;
/// requests for other accounts pass through and leave it set. `PAUSE` sits
/// between credential verification and the cache insert fence (paused
/// verification tests); `ENDPOINT_PAUSE` sits in the vault endpoints between
/// the fresh verification and the record re-read (generation fence tests);
/// `PUBLISH_PAUSE` sits in `app-password` between the Pending wrap and the
/// registry credential (interleaving tests); `REGISTERED_PAUSE` sits in
/// `app-password` after the registry write, before the publication.
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

    pub static PUBLISH_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

    pub static REGISTERED_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

    pub fn set(pause: Option<Arc<Pause>>) {
        *PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
    }

    pub fn set_endpoint(pause: Option<Arc<Pause>>) {
        *ENDPOINT_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
    }

    pub fn set_publish(pause: Option<Arc<Pause>>) {
        *PUBLISH_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
    }

    pub fn set_registered(pause: Option<Arc<Pause>>) {
        *REGISTERED_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
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

    pub(crate) async fn publish_pause_point(account_id: u32) {
        wait_at(&PUBLISH_PAUSE, account_id).await;
    }

    pub(crate) async fn registered_pause_point(account_id: u32) {
        wait_at(&REGISTERED_PAUSE, account_id).await;
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
    LENIENT.decode_to_string(token).ok().and_then(|token| {
        token
            .split_once(':')
            .map(|(login, secret)| Credentials::Basic {
                username: login.trim().to_lowercase(),
                secret: secret.to_string(),
                mfa_token: None,
            })
    })
}
