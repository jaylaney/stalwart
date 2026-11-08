/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::mail::{message_ids, row_is_stub, stored_blob, stored_metadata};
use crate::utils::{
    server::{TestServer, TestServerBuilder},
    za::za_unlock,
};
use ahash::AHashSet;
use common::auth::AccessToken;
use email::{
    mailbox::INBOX_ID,
    message::{
        delivery::SenderAuthentication,
        ingest::{EmailIngest, IngestEmail, IngestSource},
        sortkeys::MessageSortKeys,
        thread::ThreadFields,
        za::view::{ZaMailView, za_has_attachment},
    },
};
use mail_builder::{
    MessageBuilder,
    headers::{content_type::ContentType, date::Date},
    mime::MimePart,
};
use mail_parser::MessageParser;
use std::time::{Duration, Instant};
use store::{
    search::{EmailSearchField, SearchField},
    write::{
        Dictionary, SearchIndex,
        compress::{compress, decompress_into},
    },
};
use utils::{cheeky_hash::CheekyHash, codec::leb128::Leb128Reader};
use vault::session::SessionKeys;

const ACCOUNT: &str = "search-bench@example.com";
const PASSWORD: &str = "bench-secret";
const BODY_WORD: &str = "bodycanary";
const ATTACHMENT_WORD: &str = "attachmentcanary";
const K: usize = 16;
const R: usize = 304;

pub struct Generated {
    pub raw: Vec<u8>,
    pub attachment_bytes: usize,
    pub references: usize,
}

struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

struct Vocabulary {
    words: Vec<String>,
    weights: Vec<u64>,
}

impl Vocabulary {
    fn new(rng: &mut Xorshift) -> Self {
        // A fixed, portable list of 5,000 distinct, variable-length pseudo-words.
        let mut words = Vec::with_capacity(5000);
        for a in ["ba", "ce", "di", "fo", "gu", "ha", "je", "ki", "lo", "mu"] {
            for b in [
                "bra", "clo", "dri", "fra", "gru", "kra", "pre", "sma", "tra", "zle",
            ] {
                for c in [
                    "ad", "el", "in", "or", "us", "aft", "ert", "ink", "orm", "ump",
                ] {
                    for d in ["", "y", "en", "ian", "ment"] {
                        words.push(format!("{a}{b}{c}{d}"));
                    }
                }
            }
        }
        // Randomize which words occupy the highest-frequency ranks once per corpus.
        for i in (1..words.len()).rev() {
            words.swap(i, rng.below(i + 1));
        }
        let mut total = 0;
        let weights = (1..=words.len())
            .map(|rank| {
                total += 1_000_000 / rank as u64;
                total
            })
            .collect();
        Self { words, weights }
    }

    fn text(
        &self,
        rng: &mut Xorshift,
        bytes: usize,
        canary: &str,
        sentence_count: &mut usize,
    ) -> String {
        const PHRASE: &str = "Please confirm the quarterly review by Thursday.\r\n";
        const WORDS_PER_SENTENCE: usize = 8;
        let mut text = String::with_capacity(bytes);
        text.push_str(canary);
        text.push(' ');
        while text.len() < bytes {
            let remaining = bytes - text.len();
            if (*sentence_count + 1).is_multiple_of(20) {
                if PHRASE.len() > remaining {
                    break;
                }
                text.push_str(PHRASE);
            } else {
                let start = text.len();
                for _ in 0..WORDS_PER_SENTENCE {
                    let pick = rng.next() % self.weights.last().unwrap();
                    let word = &self.words[self.weights.partition_point(|&weight| weight <= pick)];
                    text.push_str(word);
                    text.push(' ');
                }
                text.pop();
                text.push_str(".\r\n");
                if text.len() > bytes {
                    text.truncate(start);
                    break;
                }
            }
            *sentence_count += 1;
        }
        text.extend(std::iter::repeat_n(' ', bytes - text.len()));
        text
    }
}

fn message_id(i: usize) -> String {
    format!("bench-{i}@example.com")
}

/// Deterministic xorshift64 corpus, about 70 KB per message with the attachment mix.
pub fn corpus(seed: u64, count: usize) -> Vec<Generated> {
    let mut rng = Xorshift(if seed == 0 { 1 } else { seed });
    let vocabulary = Vocabulary::new(&mut rng);
    let mut date = 1_735_689_600;
    let mut body_sentences = 0;
    let mut attachment_sentences = 0;
    let mut generated = Vec::with_capacity(count);
    let mut roots = Vec::new();
    let mut chain_tail = [None; 2];
    let mut replies_in_block = 0;
    let mut ids = Vec::with_capacity(count);
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(count);
    let mut thread_roots = Vec::with_capacity(count);
    let mut attachment_slots: [usize; 100] = std::array::from_fn(|i| i);

    for i in 0..count {
        if i.is_multiple_of(1000) {
            roots.clear();
            chain_tail = [None; 2];
            replies_in_block = 0;
        }
        if i.is_multiple_of(100) {
            for j in (1..100).rev() {
                attachment_slots.swap(j, rng.below(j + 1));
            }
        }

        let slot = i % 20;
        // Twenty five-message localized threads per 1,000 messages. Keep
        // their normalized subjects consistent across each root and reply.
        let localized = i % 1000 >= 200 && (i / 20).is_multiple_of(2) && slot <= 4;
        let mut references = Vec::new();
        let (prefix, thread_root) = match slot {
            1..=12 => {
                // Two 40-reply chains per 1,000 messages yield 48 messages
                // with more than 16 References and 50 over the hash limit.
                let chain = replies_in_block / 40;
                let parent = if localized {
                    i - slot
                } else if let Some(tail) = chain_tail.get_mut(chain) {
                    tail.replace(i).unwrap_or(roots[chain])
                } else {
                    roots[rng.below(roots.len())]
                };
                replies_in_block += 1;
                references.extend_from_slice(&chains[parent]);
                references.push(parent);
                ("Re: ", thread_roots[parent])
            }
            13..=14 => ("Fwd: ", roots[rng.below(roots.len())]),
            _ => {
                if !localized {
                    roots.push(i);
                }
                ("", i)
            }
        };
        let topic = if localized {
            ["Réunion", "会議", "Überprüfung"][(thread_root / 20) % 3]
        } else {
            "Quarterly review"
        };
        // These earlier slots are forwards and never ancestors, so the
        // repeated IDs do not shorten the two deep reference chains.
        let id = if i % 50 == 49 {
            ((i - 13) / 20) * 20 + 13
        } else {
            i
        };
        let mut builder = MessageBuilder::new()
            .message_id(message_id(id))
            .date(Date::new(date))
            .from(format!("person{}@example.com", rng.below(200)))
            .to(format!("person{}@example.com", rng.below(200)))
            .subject(format!("{prefix}{topic} {thread_root}"));
        date += 1 + rng.below(90) as i64;
        if let Some(&parent) = references.last() {
            builder = builder.in_reply_to(message_id(ids[parent])).references(
                references
                    .iter()
                    .map(|&p| message_id(ids[p]))
                    .collect::<Vec<_>>(),
            );
        }

        let body_bytes = 1024 + rng.below(7 * 1024 + 1);
        builder = builder.text_body(vocabulary.text(
            &mut rng,
            body_bytes,
            BODY_WORD,
            &mut body_sentences,
        ));
        let attachment_bytes = match attachment_slots[i % 100] {
            0..=24 => 40 * 1024 + rng.below(160 * 1024 + 1),
            25..=29 => 500 * 1024 + rng.below(524 * 1024 + 1),
            _ => 0,
        };
        if attachment_bytes > 0 {
            let attachment = vocabulary.text(
                &mut rng,
                attachment_bytes,
                ATTACHMENT_WORD,
                &mut attachment_sentences,
            );
            builder = builder.attachment("text/plain", format!("notes-{i}.txt"), attachment);
            // The builder normally uses the clock to generate multipart boundaries.
            // Supply a fixed boundary while retaining the builder's two MIME parts.
            let mut parts = builder.attachments.take().unwrap();
            parts.insert(0, builder.text_body.take().unwrap());
            builder = builder.body(MimePart::new(
                ContentType::new("multipart/mixed").attribute("boundary", "za-search-bench"),
                parts,
            ));
        }

        generated.push(Generated {
            raw: builder.write_to_vec().unwrap(),
            attachment_bytes,
            references: references.len(),
        });
        ids.push(id);
        chains.push(references);
        thread_roots.push(thread_root);
    }
    generated
}

#[derive(Default)]
struct Column {
    raw: Vec<u8>,
    lengths: Vec<usize>,
    no_dictionary: usize,
    term: usize,
    email: usize,
    term_frames: Vec<Vec<u8>>,
}

impl Column {
    fn record(&mut self, bytes: &[u8]) {
        self.lengths.push(bytes.len());
        self.raw.extend_from_slice(bytes);
        self.no_dictionary += compress(None, bytes, 0).unwrap().len();
        self.email += compress(Some(Dictionary::Email), bytes, 0).unwrap().len();
        let term_frame = compress(Some(Dictionary::Term), bytes, 0).unwrap();
        self.term += term_frame.len();
        self.term_frames.push(term_frame);
    }

    fn summarize(mut self) -> Summary {
        let total = self.raw.len();
        let mean = total as f64 / self.lengths.len() as f64;
        let aggregate = compress(None, &self.raw, 0).unwrap().len();
        self.lengths.sort_unstable();
        let p95 = self.lengths[(self.lengths.len() * 95).div_ceil(100) - 1];
        let max = *self.lengths.last().unwrap();
        let mut scratch = Vec::new();
        let started = Instant::now();
        for frame in &self.term_frames {
            decompress_into(frame, &mut scratch, max).unwrap();
        }
        Summary {
            total,
            mean,
            p95,
            no_dictionary: self.no_dictionary,
            term: self.term,
            email: self.email,
            aggregate,
            decompress_seconds: started.elapsed().as_secs_f64(),
        }
    }
}

struct Summary {
    total: usize,
    mean: f64,
    p95: usize,
    no_dictionary: usize,
    term: usize,
    email: usize,
    aggregate: usize,
    decompress_seconds: f64,
}

struct Sample {
    with_positions: Vec<u8>,
    without_positions: Vec<u8>,
    elapsed: Duration,
    thread: Option<(bool, usize)>,
}

async fn open_and_index(
    test: &TestServer,
    keys: &SessionKeys,
    account_id: u32,
    document_id: u32,
    index_fields: &AHashSet<SearchField>,
    measure_thread: bool,
) -> Sample {
    let started = Instant::now();
    let server = &test.server;
    let stored = stored_metadata(test, account_id, document_id).await;
    assert!(row_is_stub(&stored));
    let hash = stored.unarchive().unwrap().blob_hash();
    let row = server
        .za_open_metadata_row(&stored, Some(keys), account_id, document_id)
        .await
        .unwrap()
        .expect("opened stub");
    let blob = stored_blob(test, &hash).await;
    let blob = server
        .za_open_message_blob(true, Some(keys), account_id, document_id, blob)
        .await
        .unwrap();
    let headers = row.raw_headers().unwrap();
    let metadata = row.unarchive().unwrap();
    let email = &server.core.email;
    let (with_positions, without_positions) = metadata
        .index_document(
            account_id,
            document_id,
            &headers,
            &blob,
            index_fields,
            email.default_language,
            &email.extract_limits,
        )
        .za_bench_term_bytes();
    let elapsed = started.elapsed();
    let thread = measure_thread.then(|| {
        std::hint::black_box(MessageSortKeys::from_envelope(metadata.root().envelope()));
        let raw = MessageParser::new().parse_headers(blob.as_slice()).unwrap();
        (
            za_has_attachment(&metadata),
            ThreadFields::scan(&raw).message_ids.len(),
        )
    });
    Sample {
        with_positions,
        without_positions,
        elapsed,
        thread,
    }
}

// Read just the term table at the start of the raw forward-index encoding.
fn contains_term(mut bytes: &[u8], word: &str) -> bool {
    let target = CheekyHash::new(word.as_bytes());
    let count: usize = bytes.read_leb128().unwrap();
    for _ in 0..count {
        let (&len, tail) = bytes.split_first().unwrap();
        let (key, tail) = tail.split_at(usize::from(len).min(CheekyHash::HASH_SIZE));
        if CheekyHash::from_key_bytes(key, len).unwrap() == target {
            return true;
        }
        bytes = tail;
        let _: u32 = bytes.read_leb128().unwrap();
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
pub async fn format() {
    let count = std::env::var("SEARCH_BENCH_MESSAGES")
        .map(|value| {
            value
                .parse::<usize>()
                .expect("SEARCH_BENCH_MESSAGES must be a count")
        })
        .unwrap_or(15_000);
    assert!(count > 0, "the benchmark needs messages");
    assert!(
        std::env::var_os("NO_INSERT").is_none(),
        "the benchmark needs a fresh store"
    );

    let test = TestServerBuilder::new("search_bench")
        .await
        .with_default_listeners()
        .await
        .build()
        .await;
    let admin = test.create_admin_account("admin@example.com").await;
    let account = admin
        .create_key_user_account(ACCOUNT, PASSWORD, "Search benchmark", &[], vec![])
        .await;
    let account_id = account.id().document_id();
    let token = AccessToken::from_id_maybe_invalid(account_id);
    let parser = MessageParser::new();
    println!("Generating {count} messages");
    let messages = corpus(1, count);
    let raw_bytes = messages
        .iter()
        .map(|message| message.raw.len())
        .sum::<usize>();
    let attachments = messages
        .iter()
        .filter(|message| message.attachment_bytes > 0)
        .count();
    let large_attachments = messages
        .iter()
        .filter(|message| message.attachment_bytes >= 500 * 1024)
        .count();
    let max_refs = messages
        .iter()
        .map(|message| message.references)
        .max()
        .unwrap();
    let over_k_refs = messages
        .iter()
        .filter(|message| message.references > K)
        .count();
    println!(
        "Generated {count} messages, {raw_bytes} raw bytes, {attachments} attachments ({large_attachments} large), \
         {over_k_refs} messages with more than {K} References, max {max_refs}"
    );
    let started = Instant::now();
    for (i, message) in messages.into_iter().enumerate() {
        let parsed = parser.parse(&message.raw).unwrap();
        test.server
            .email_ingest(IngestEmail {
                raw_message: &message.raw,
                message: Some(&parsed),
                blob_hash: None,
                access_token: &token,
                mailbox_ids: vec![INBOX_ID],
                keywords: vec![],
                received_at: None,
                source: IngestSource::Smtp {
                    deliver_to: ACCOUNT,
                    envelope_from: "bench@remote.test",
                    sender_authentication: SenderAuthentication::DmarcPass,
                    is_spam: false,
                },
                metadata: None,
                session_id: 0,
            })
            .await
            .unwrap();
        if (i + 1).is_multiple_of(1000) {
            println!("Ingested {}/{count} messages", i + 1);
        }
    }
    println!("Ingest seconds {:.6}", started.elapsed().as_secs_f64());

    let keys = za_unlock(&test, account_id, PASSWORD).await;
    let ids = message_ids(&test, account_id).await;
    assert_eq!(ids.len(), count);
    let fields = test
        .server
        .core
        .email
        .index_fields
        .get(&SearchIndex::Email)
        .unwrap();
    let mut no_attachment = fields.clone();
    assert!(no_attachment.remove(&SearchField::Email(EmailSearchField::Attachment)));
    assert!(
        !no_attachment.is_empty(),
        "an empty field set would enable every field"
    );

    let mut columns = [Column::default(), Column::default()];
    let mut with_attachment_time = Duration::ZERO;
    let mut without_attachment_time = Duration::ZERO;
    let mut thread_total = 0;
    let mut thread_max = 0;
    let mut over_k_hashes = 0;
    let mut seen_attachments = 0;

    for (i, document_id) in ids.into_iter().enumerate() {
        // Alternate which configuration reads each document first.
        let (sample, control) = if i.is_multiple_of(2) {
            let sample = open_and_index(&test, &keys, account_id, document_id, fields, true).await;
            let control =
                open_and_index(&test, &keys, account_id, document_id, &no_attachment, false).await;
            (sample, control)
        } else {
            let control =
                open_and_index(&test, &keys, account_id, document_id, &no_attachment, false).await;
            let sample = open_and_index(&test, &keys, account_id, document_id, fields, true).await;
            (sample, control)
        };
        let (has_attachment, hashes) = sample.thread.unwrap();
        if has_attachment {
            if seen_attachments == 0 {
                assert!(contains_term(&sample.with_positions, ATTACHMENT_WORD));
                assert!(!contains_term(&control.with_positions, ATTACHMENT_WORD));
                assert!(contains_term(&sample.with_positions, BODY_WORD));
                assert!(contains_term(&control.with_positions, BODY_WORD));
            }
            seen_attachments += 1;
        }
        let thread_bytes = hashes.min(K) * 16 + 48;
        thread_total += thread_bytes;
        thread_max = thread_max.max(thread_bytes);
        over_k_hashes += usize::from(hashes > K);
        with_attachment_time += sample.elapsed;
        without_attachment_time += control.elapsed;
        columns[0].record(&sample.with_positions);
        columns[1].record(&sample.without_positions);
        if (i + 1).is_multiple_of(1000) {
            println!("Measured {}/{count} messages", i + 1);
        }
    }

    assert_eq!(seen_attachments, attachments);
    assert!(
        thread_max <= R,
        "thread record {thread_max} exceeds the {R}-byte bound"
    );
    println!(
        "Thread record bytes, mean / max: {:.3} / {thread_max}",
        thread_total as f64 / count as f64
    );
    println!(
        "Messages with more than {K} hashes: {over_k_hashes} / {count} ({:.2}%)",
        over_k_hashes as f64 * 100.0 / count as f64
    );
    println!(
        "Open-and-index documents/s, with / without attachment text: {:.3} / {:.3}",
        count as f64 / with_attachment_time.as_secs_f64(),
        count as f64 / without_attachment_time.as_secs_f64(),
    );
    let [with, without] = columns.map(Column::summarize);
    println!("| Figure | With positions | Without positions |");
    println!("|---|---|---|");
    println!(
        "| Content term bytes, total / mean / p95 per message | {} / {:.3} / {} | {} / {:.3} / {} |",
        with.total, with.mean, with.p95, without.total, without.mean, without.p95,
    );
    println!(
        "| Per-record zstd, no dictionary, total | {} | {} |",
        with.no_dictionary, without.no_dictionary
    );
    println!(
        "| Per-record zstd, Dictionary::Term, total | {} | {} |",
        with.term, without.term
    );
    println!(
        "| Per-record zstd, Dictionary::Email, total | {} | {} |",
        with.email, without.email
    );
    println!(
        "| One frame over all records, total | {} | {} |",
        with.aggregate, without.aggregate
    );
    println!(
        "| Projected content part at 30,000 messages (mean × 30,000) | {:.0} | {:.0} |",
        with.mean * 30_000.0,
        without.mean * 30_000.0
    );
    println!(
        "| Decompression of every per-record Term frame, seconds | {:.6} | {:.6} |",
        with.decompress_seconds, without.decompress_seconds
    );

    test.shutdown();
    test.temp_dir.delete();
}
