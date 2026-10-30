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
<<<<<<< HEAD
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
        "[redacted]"
    } else {
        v.to_str().unwrap_or_default()
    };
    trc::Value::Array(vec![
        k.as_str().to_compact_string().into(),
        value.to_compact_string().into(),
    ])
}

async fn fetch_body_inner(
    req: &mut HttpRequest,
    max_size: usize,
    session_id: u64,
    trace: bool,
) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(1024);
=======
    let declared = req
        .body()
        .size_hint()
        .exact()
        .and_then(|len| usize::try_from(len).ok())
        .filter(|&len| max_size == 0 || len <= max_size);
    let mut bytes = Vec::new();
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
