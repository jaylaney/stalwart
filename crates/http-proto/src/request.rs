/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::borrow::Cow;

use compact_str::ToCompactString;
use http_body_util::BodyExt;

use crate::HttpRequest;

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

async fn fetch_body_inner(
    req: &mut HttpRequest,
    max_size: usize,
    session_id: u64,
    trace: bool,
) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(1024);
    while let Some(Ok(frame)) = req.frame().await {
        if let Some(data) = frame.data_ref() {
            if bytes.len() + data.len() <= max_size || max_size == 0 {
                bytes.extend_from_slice(data);
            } else if !trace {
                return None;
            } else {
                trc::event!(
                    Http(trc::HttpEvent::RequestBody),
                    SpanId = session_id,
                    Details = req
                        .headers()
                        .iter()
                        .map(|(k, v)| trc::Value::Array(vec![
                            k.as_str().to_compact_string().into(),
                            v.to_str().unwrap_or_default().to_compact_string().into()
                        ]))
                        .collect::<Vec<_>>(),
                    Contents = std::str::from_utf8(&bytes)
                        .unwrap_or("[binary data]")
                        .to_string(),
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
        Details = req
            .headers()
            .iter()
            .map(|(k, v)| trc::Value::Array(vec![
                k.as_str().to_compact_string().into(),
                v.to_str().unwrap_or_default().to_compact_string().into()
            ]))
            .collect::<Vec<_>>(),
        Contents = std::str::from_utf8(&bytes)
            .unwrap_or("[binary data]")
            .to_string(),
        Size = bytes.len(),
    );

    bytes.into()
}
