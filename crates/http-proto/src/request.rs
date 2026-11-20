/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::borrow::Cow;

use compact_str::{CompactString, ToCompactString};
use http_body_util::BodyExt;
use hyper::body::Body;

use crate::HttpRequest;

const MIN_FIRST_BODY_ALLOCATION: usize = 4 * 1024;
const MIN_BODY_GROWTH: usize = 64 * 1024;
const BODY_GROWTH_FACTOR: usize = 4;

#[inline]
pub fn decode_path_element(item: &str) -> Cow<'_, str> {
    percent_encoding::percent_decode_str(item)
        .decode_utf8()
        .unwrap_or_else(|_| item.into())
}

pub async fn fetch_body(
    req: &mut HttpRequest,
    max_size: usize,
    session_id: u64,
) -> Option<Vec<u8>> {
    fetch_body_inner(req, max_size, session_id, true).await
}

/// Like `fetch_body`, but never emits `HttpEvent::RequestBody`: for bodies
/// that carry passwords, tokens or recovery keys.
pub async fn fetch_body_untraced(req: &mut HttpRequest, max_size: usize) -> Option<Vec<u8>> {
    fetch_body_inner(req, max_size, 0, false).await
}

/// A request header as recorded in `HttpEvent::RequestBody`. Credential
/// headers are redacted for every request: a Basic credential is reversible
/// and, for a key account, unwraps its keys.
fn traced_header(k: &hyper::header::HeaderName, v: &hyper::header::HeaderValue) -> trc::Value {
    const REDACTED: &[&str] = &["authorization", "proxy-authorization", "cookie"];
    let value = if REDACTED.contains(&k.as_str()) {
        "[redacted]".to_compact_string()
    } else if k == hyper::header::REFERER {
        // The page that sent the request may be an OAuth callback.
        traced_url(v.to_str().unwrap_or_default())
    } else {
        v.to_str().unwrap_or_default().to_compact_string()
    };
    trc::Value::Array(vec![k.as_str().to_compact_string().into(), value.into()])
}

/// A URL as it may be traced: the value of every query parameter whose
/// percent-decoded name is `code` is replaced by `[redacted]`, and every
/// other byte is kept. An OAuth redirect carries the authorization code
/// there, and for a key account the hand-off secret behind it (release 6
/// final review FA-3). It applies to every request, since no account is
/// known when a URL is traced.
pub fn traced_url(url: &str) -> CompactString {
    let query_end = url.find('#').unwrap_or(url.len());
    let Some(query_start) = url[..query_end].find('?').map(|pos| pos + 1) else {
        return url.to_compact_string();
    };
    let query = &url[query_start..query_end];
    let is_code =
        |name: &str| percent_encoding::percent_decode_str(name).eq(b"code".iter().copied());
    if !query
        .split('&')
        .any(|pair| pair.split_once('=').is_some_and(|(name, _)| is_code(name)))
    {
        return url.to_compact_string();
    }
    let mut traced = CompactString::with_capacity(url.len());
    traced.push_str(&url[..query_start]);
    for (pos, pair) in query.split('&').enumerate() {
        if pos > 0 {
            traced.push('&');
        }
        match pair.split_once('=') {
            Some((name, _)) if is_code(name) => {
                traced.push_str(name);
                traced.push_str("=[redacted]");
            }
            _ => traced.push_str(pair),
        }
    }
    traced.push_str(&url[query_end..]);
    traced
}

async fn fetch_body_inner(
    req: &mut HttpRequest,
    max_size: usize,
    session_id: u64,
    trace: bool,
) -> Option<Vec<u8>> {
    let declared = req
        .body()
        .size_hint()
        .exact()
        .and_then(|len| usize::try_from(len).ok())
        .filter(|&len| max_size == 0 || len <= max_size);
    let mut bytes = Vec::new();
    while let Some(Ok(frame)) = req.frame().await {
        if let Some(data) = frame.data_ref() {
            let len = bytes.len() + data.len();
            if len <= max_size || max_size == 0 {
                if let Some(declared) = declared
                    && len > bytes.capacity()
                {
                    if bytes.capacity() == 0 {
                        bytes = Vec::with_capacity(
                            len.saturating_mul(BODY_GROWTH_FACTOR)
                                .max(MIN_FIRST_BODY_ALLOCATION)
                                .min(declared)
                                .max(len),
                        );
                    } else {
                        let target = bytes
                            .capacity()
                            .saturating_mul(BODY_GROWTH_FACTOR)
                            .max(MIN_BODY_GROWTH)
                            .min(declared)
                            .max(len);
                        bytes.reserve_exact(target - bytes.len());
                    }
                }
                bytes.extend_from_slice(data);
            } else if !trace {
                return None;
            } else {
                trc::event!(
                    Http(trc::HttpEvent::RequestBody),
                    SpanId = session_id,
                    Headers = req
                        .headers()
                        .iter()
                        .map(|(k, v)| traced_header(k, v))
                        .collect::<Vec<_>>(),
                    Contents =
                        CompactString::from(std::str::from_utf8(&bytes).unwrap_or("[binary data]")),
                    Size = bytes.len(),
                    Limit = max_size,
                );

                return None;
            }
        }
    }

    if !trace {
        return bytes.into();
    }

    trc::event!(
        Http(trc::HttpEvent::RequestBody),
        SpanId = session_id,
        Headers = req
            .headers()
            .iter()
            .map(|(k, v)| traced_header(k, v))
            .collect::<Vec<_>>(),
        Contents = CompactString::from(std::str::from_utf8(&bytes).unwrap_or("[binary data]")),
        Size = bytes.len(),
    );

    bytes.into()
}

#[cfg(test)]
mod tests {
    use super::{traced_header, traced_url};
    use hyper::header::{HeaderName, HeaderValue, REFERER, USER_AGENT};

    #[test]
    fn traced_url_without_query_is_unchanged() {
        for url in [
            "/",
            "/auth/token",
            "https://mail.example.com/callback#code=x",
            "https://mail.example.com/callback#frag?code=x",
        ] {
            assert_eq!(traced_url(url), url);
        }
        assert_eq!(traced_url("/path?"), "/path?");
        assert_eq!(traced_url("/path?a=1&b=2"), "/path?a=1&b=2");
    }

    #[test]
    fn traced_url_masks_code_anywhere() {
        assert_eq!(
            traced_url("/cb?code=CANARY&state=x"),
            "/cb?code=[redacted]&state=x"
        );
        assert_eq!(
            traced_url("/cb?state=x&code=CANARY&iss=y"),
            "/cb?state=x&code=[redacted]&iss=y"
        );
        assert_eq!(
            traced_url("https://app.example.com/cb?state=x&code=CANARY"),
            "https://app.example.com/cb?state=x&code=[redacted]"
        );
        assert_eq!(
            traced_url("/cb?code=ONE&state=x&code=TWO"),
            "/cb?code=[redacted]&state=x&code=[redacted]"
        );
    }

    #[test]
    fn traced_url_matches_the_decoded_name_only() {
        assert_eq!(
            traced_url("/cb?%63ode=CANARY&state=x"),
            "/cb?%63ode=[redacted]&state=x"
        );
        assert_eq!(
            traced_url("/cb?%63%6F%64%65=CANARY"),
            "/cb?%63%6F%64%65=[redacted]"
        );
        assert_eq!(traced_url("/cb?barcode=1&code1=2"), "/cb?barcode=1&code1=2");
        assert_eq!(traced_url("/cb?Code=1"), "/cb?Code=1");
    }

    #[test]
    fn traced_url_empty_value_and_bare_name() {
        assert_eq!(
            traced_url("/cb?code=&state=x"),
            "/cb?code=[redacted]&state=x"
        );
        // A bare name has no value to mask.
        assert_eq!(traced_url("/cb?code&state=x"), "/cb?code&state=x");
        assert_eq!(traced_url("/cb?code"), "/cb?code");
    }

    #[test]
    fn traced_url_keeps_the_fragment() {
        assert_eq!(
            traced_url("/cb?code=CANARY&state=x#code=frag"),
            "/cb?code=[redacted]&state=x#code=frag"
        );
        assert_eq!(traced_url("/cb?code=CANARY#"), "/cb?code=[redacted]#");
    }

    fn header_value(name: HeaderName, value: &str) -> String {
        match traced_header(&name, &HeaderValue::from_str(value).unwrap()) {
            trc::Value::Array(items) => match items.as_slice() {
                [trc::Value::String(k), trc::Value::String(v)] => {
                    assert_eq!(k.as_str(), name.as_str());
                    v.to_string()
                }
                other => panic!("unexpected header items {other:?}"),
            },
            other => panic!("unexpected header value {other:?}"),
        }
    }

    #[test]
    fn traced_referer_masks_code() {
        assert_eq!(
            header_value(REFERER, "https://app.example.com/cb?code=CANARY&state=x"),
            "https://app.example.com/cb?code=[redacted]&state=x"
        );
        assert_eq!(
            header_value(REFERER, "https://app.example.com/page?barcode=1"),
            "https://app.example.com/page?barcode=1"
        );
        // Other headers are traced as they were.
        assert_eq!(header_value(USER_AGENT, "agent?code=1"), "agent?code=1");
        assert_eq!(
            header_value(HeaderName::from_static("authorization"), "Basic abc"),
            "[redacted]"
        );
    }
}
