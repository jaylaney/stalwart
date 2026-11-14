/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::message::{index::extractors::VisitText, thread::ThreadFields};
use mail_parser::{HeaderName, HeaderValue, Message};
use store::{
    xxhash_rust::xxh3::xxh3_128,
    za_search::{K, ThreadFieldsK},
};

/// Capture the first K distinct hashes in thread-reference priority order.
pub fn za_thread_fields_k(message: &Message<'_>) -> ThreadFieldsK {
    let scanned = ThreadFields::scan(message);
    let subject_hash = xxh3_128(scanned.subject.as_bytes());
    let sent_at = scanned.sent_at;
    let headers = message.root_part().headers();
    let mut hashes = Vec::with_capacity(K);
    let mut capture = |id: &str| {
        if !id.is_empty() && hashes.len() < K {
            let hash = xxh3_128(id.as_bytes());
            if !hashes.contains(&hash) {
                hashes.push(hash);
            }
        }
    };

    if let Some(id) = scanned.message_id.as_deref() {
        capture(id);
    }
    for header in headers {
        if header.name() == HeaderName::InReplyTo {
            header.value().visit_text(&mut capture);
        }
    }
    for header in headers.iter().rev() {
        if header.name() == HeaderName::References {
            match header.value() {
                HeaderValue::Text(id) => capture(id),
                HeaderValue::TextList(ids) => {
                    for id in ids.iter().rev() {
                        capture(id);
                    }
                }
                _ => {}
            }
        }
    }
    for header in headers {
        if header.name() == HeaderName::ResentMessageId {
            header.value().visit_text(&mut capture);
        }
    }

    ThreadFieldsK {
        hashes,
        subject_hash,
        sent_at,
    }
}

#[cfg(test)]
mod tests {
    use super::za_thread_fields_k;
    use crate::message::thread::ThreadFields;
    use mail_parser::MessageParser;
    use store::{xxhash_rust::xxh3::xxh3_128, za_search::K};

    fn hash(id: &str) -> u128 {
        xxh3_128(id.as_bytes())
    }

    #[test]
    fn the_capture_keeps_own_id_parent_then_nearest_references_before_any_sort() {
        let raw = format!(
            "Message-ID: <m>\r\nIn-Reply-To: <p>\r\nReferences: {} <p>\r\nResent-Message-ID: <x>\r\n\r\nBody",
            (1..=20)
                .map(|i| format!("<r{i}>"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        let message = MessageParser::new().parse(raw.as_bytes()).unwrap();
        let fields = za_thread_fields_k(&message);
        let mut expected = vec![hash("m"), hash("p")];
        expected.extend((7..=20).rev().map(|i| hash(&format!("r{i}"))));
        assert_eq!(fields.hashes.len(), K);
        assert_eq!(fields.hashes, expected);
    }

    #[test]
    fn a_short_chain_keeps_everything_once_and_resent_last() {
        let message = MessageParser::new().parse(
            b"Message-ID: <m>\r\nIn-Reply-To: <r3>\r\nReferences: <r1>\r\nReferences: <r2> <r3>\r\nResent-Message-ID: <x>\r\n\r\nBody",
        ).unwrap();
        let fields = za_thread_fields_k(&message);
        assert_eq!(fields.hashes, ["m", "r3", "r2", "r1", "x"].map(hash));
    }

    #[test]
    fn own_id_subject_and_sent_at_follow_upstream() {
        let message = MessageParser::new().parse(
            b"Message-ID: <earlier>\r\nMessage-ID: <chosen>\r\nSubject: Re: Thread subject\r\nDate: Tue, 7 Nov 2023 09:17:35 +0000\r\n\r\nBody",
        ).unwrap();
        let scanned = ThreadFields::scan(&message);
        let fields = za_thread_fields_k(&message);
        assert_eq!(
            fields.hashes,
            [hash(scanned.message_id.as_deref().unwrap())]
        );
        assert_eq!(fields.subject_hash, hash(scanned.subject));
        assert_eq!(fields.sent_at, scanned.sent_at);
    }
}
