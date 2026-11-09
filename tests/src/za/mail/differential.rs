/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    key_mail_account, message_ids, message_with, new_message, open_message, stored_blob,
    stored_hash,
};
use crate::{
    store::deflate_test_resource,
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        server::TestServer,
        smtp::SmtpConnection,
        za::{
            finish_bulk_delivery_indexing, raise_bulk_lmtp_rates, restore_bulk_lmtp_rates,
            search_config_guard, search_manifest, search_pass_request, wait_for_delivery,
            za_index_current,
        },
    },
    za::{STRONG, user_permissions},
};
use chrono::Utc;
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    mailbox::INBOX_ID,
    message::delivery::test_received_at::PairedReceivedAtGuard,
};
use imap_proto::ResponseType;
use mail_parser::{HeaderName, HeaderValue, MessageParser, mailbox::mbox::MessageIterator};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};
use store::{
    write::now,
    za_search::{BlobHeader, BlobKind},
};
use types::{blob_hash::BlobHash, id::Id};

const BASE: usize = 3240;
const TOTAL: usize = BASE + 18;
const HEADERLESS_MBOX_ORDINALS: [usize; 3] = [292, 555, 595];
const LMTP_PARSE_REFUSAL: &str = "550 5.7.7 Failed to parse message.";
const SENDER: &str = "sender@example.org";
const PAGE: usize = 400;
const K_ROOT: usize = BASE + 7;
const K2_ROOT: usize = BASE + 8;
const K1: usize = BASE + 10;
const K2: usize = BASE + 11;
const BIG: usize = BASE + 16;
const ATTACHMENT_WORDS: [&str; 4] = [
    "attword-alpha",
    "attword-beta",
    "attword-gamma",
    "attword-delta",
];
const SUBJECTS: [&str; 3] = ["Ärger im Büro", "孫子兵法 heute", "café résumé"];
const PHRASE: &str = "\"the quick brown fox\"";

struct Fixture<'a> {
    account: &'a Account,
    emails: Vec<Id>,
    email_to_ordinal: BTreeMap<Id, usize>,
    uids: Vec<u32>,
    received_at: Vec<u64>,
    inbox: Id,
    threads: Vec<Id>,
    partition: Vec<Vec<usize>>,
}

pub async fn test(test: &mut TestServer) {
    println!("differential: paired corpus");
    let key = key_mail_account(test, "keymail64@example.com", STRONG).await;
    let admin = test.account("admin@example.com").clone();
    let plain = admin
        .create_user_account(
            "plainmail58@example.com",
            STRONG,
            "Ordinary differential",
            &[],
            user_permissions(),
        )
        .await;
    let corpus = corpus();
    // Validate the actual matrix input before the long delivery setup.
    let _ = first_terms(&corpus[0]);
    let saved_rates = raise_bulk_lmtp_rates(test).await;
    let (key_docs, plain_docs, expected_received_at) =
        deliver_corpus(test, &key, &plain, &corpus).await;
    restore_bulk_lmtp_rates(test, saved_rates).await;
    finish_bulk_delivery_indexing(test, &key).await;
    za_index_current(&key).await;
    test.wait_for_tasks().await;

    let key = inventory(test, &key, key_docs, &corpus, &expected_received_at).await;
    let plain = inventory(test, &plain, plain_docs, &corpus, &expected_received_at).await;
    for (ordinal, (key, plain)) in key.received_at.iter().zip(&plain.received_at).enumerate() {
        assert_eq!(
            key, plain,
            "paired stored arrival at corpus ordinal {ordinal}"
        );
    }
    compare_partitions(&key, &plain);
    for fixture in [&key, &plain] {
        let mut update = serde_json::Map::new();
        for (n, id) in fixture.emails[..10].iter().enumerate() {
            let mut flags = json!({"keywords/$seen": true});
            match n {
                0 => flags["keywords/$answered"] = json!(true),
                1 => flags["keywords/$draft"] = json!(true),
                2 => flags["keywords/$flagged"] = json!(true),
                3 => flags["keywords/$Label1"] = json!(true),
                _ => {}
            }
            update.insert(id.to_string(), flags);
        }
        let reply = fixture
            .account
            .jmap_method_call(
                "Email/set",
                json!({"accountId": fixture.account.id_string(), "update": update}),
            )
            .await;
        assert_eq!(reply.name_at(0), "Email/set");
        assert_eq!(
            reply.method_response()["updated"]
                .as_object()
                .unwrap()
                .len(),
            10
        );
    }

    let jmap = compare_jmap(&key, &plain, &corpus[0]).await;
    let imap = compare_imap(&key, &plain, &corpus).await;
    case_g5(test, key.account).await;

    for fixture in [&key, &plain] {
        test.destroy_all_mailboxes(fixture.account).await;
        assert!(
            message_ids(test, fixture.account.id().document_id())
                .await
                .is_empty()
        );
    }
    admin.destroy_account(plain.account.clone()).await;
    test.wait_for_tasks().await;
    println!(
        "differential: {} forms compared, 0 differences beyond the pinned K deviation",
        jmap + imap
    );
}

fn corpus() -> Vec<Vec<u8>> {
    let mut messages = MessageIterator::new(Cursor::new(deflate_test_resource("mailbox.gz")))
        .map(|item| item.unwrap().unwrap_contents())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), BASE);
    for (n, word) in ATTACHMENT_WORDS.into_iter().enumerate() {
        messages.push(
            format!(
                "From: fixture-sender@example.org\r\nTo: fixture-to@example.org\r\n\
             Cc: fixture-copy@example.org\r\nBcc: fixture-hidden@example.org\r\n\
             X-Private: fixture-enclosed\r\nComments: fixture-attributed\r\nSubject: Attachment fixture {n}\r\n\
             Message-ID: <attachment-{n}@za.test>\r\nDate: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
             MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=za\r\n\r\n\
             --za\r\nContent-Type: text/plain\r\n\r\nfixture body\r\n\
             --za\r\nContent-Type: text/plain; name=proof.txt\r\n\
             Content-Disposition: attachment; filename=proof.txt\r\n\r\n{word}\r\n--za--\r\n"
            )
            .into_bytes(),
        );
    }
    for subject in SUBJECTS {
        messages.push(message_with(subject, "fixture subject").into_bytes());
    }
    for (id, subject, body) in [
        ("k-root", "K bound first root", "the quick brown fox"),
        ("k2-root", "K bound second root", "brown fox the quick"),
        ("phrase-three", "Phrase permutation", "quick fox brown the"),
    ] {
        messages.push(
            format!(
                "From: fixture-sender@example.org\r\nTo: fixture-to@example.org\r\n\
             Subject: {subject}\r\nMessage-ID: <{id}@za.test>\r\n\
             Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\r\n{body}\r\n"
            )
            .into_bytes(),
        );
    }
    for (root, subject, root_pos) in [
        ("k-root", "K bound first root", 0),
        ("k2-root", "K bound second root", 19),
    ] {
        let refs = (0..20)
            .map(|n| {
                if n == root_pos {
                    format!("<{root}@za.test>")
                } else {
                    format!("<{root}-absent-{n}@za.test>")
                }
            })
            .collect::<Vec<_>>()
            .join("\r\n ");
        messages.push(
            message_with(&format!("Re: {subject}"), "fixture K bound reply")
                .replace("Date: ", &format!("References: {refs}\r\nDate: "))
                .into_bytes(),
        );
    }
    for (n, date) in [
        None,
        None,
        Some("yesterday"),
        Some("Fri, 31 Dec 9999 23:59:59 +0000"),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = message_with(&format!("Fixture date edge {n}"), "dateedge");
        let original = "Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n";
        let replacement = date.map_or_else(String::new, |date| format!("Date: {date}\r\n"));
        messages.push(raw.replace(original, &replacement).into_bytes());
    }
    let mut body = String::new();
    let mut n = 0;
    while body.len() < 1_258_292 {
        for _ in 0..12 {
            body.push_str(&format!("uniquefixture{n:06} "));
            n += 1;
        }
        body.push_str("\r\n");
    }
    body.push_str("lastfixturetruncationword\r\n");
    messages.push(message_with("Fixture oversized search body", &body).into_bytes());
    messages.push(
        b"From: fixture-sender@example.org\r\nSubject: \t \r\n\r\nfixture blank\r\n".to_vec(),
    );
    assert_eq!(messages.len(), TOTAL);
    let parser = MessageParser::new();
    let mut headerless = Vec::new();
    for (ordinal, raw) in messages.iter().enumerate() {
        let parsed = parser
            .parse(raw)
            .unwrap_or_else(|| panic!("unparsed corpus ordinal {ordinal}"));
        if !parsed.headers().has_known() {
            assert!(
                ordinal < BASE,
                "unexpected headerless generated record {ordinal}"
            );
            assert_eq!(parsed.headers().len(), 0, "corpus ordinal {ordinal}");
            assert!(parsed.message_id().is_none(), "corpus ordinal {ordinal}");
            headerless.push(ordinal);
        }
    }
    assert_eq!(headerless, HEADERLESS_MBOX_ORDINALS);
    messages
}

async fn deliver_corpus(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    corpus: &[Vec<u8>],
) -> (Vec<u32>, Vec<u32>, Vec<u64>) {
    let mut received_at = Vec::with_capacity(TOTAL);
    let mut paired_docs = (Vec::new(), Vec::new());
    let mut seen = (BTreeSet::new(), BTreeSet::new());
    let mut ordinal = 0;
    let mut accepted_lmtp = [0; 2];
    let mut accepted_imap = 0;
    for (batch, messages) in corpus.chunks(48).enumerate() {
        // The normal SMTP/LMTP session limit is ten transactions.
        for transactions in messages.chunks(8) {
            let mut lmtp = SmtpConnection::connect().await;
            assert!(
                lmtp.lhlo()
                    .await
                    .iter()
                    .any(|line| line.contains("CHUNKING"))
            );
            for raw in transactions {
                let before_refusal = if HEADERLESS_MBOX_ORDINALS.contains(&ordinal) {
                    wait_for_delivery(test).await;
                    Some([
                        message_ids(test, key.id().document_id()).await,
                        message_ids(test, plain.id().document_id()).await,
                    ])
                } else {
                    None
                };
                lmtp.mail_from(SENDER, 2).await;
                lmtp.rcpt_to(key.name(), 2).await;
                lmtp.rcpt_to(plain.name(), 2).await;
                let arrival = before_refusal.is_none().then(|| {
                    let timestamp = now();
                    let guard = PairedReceivedAtGuard::new(
                        SENDER,
                        [
                            (key.name(), key.id().document_id()),
                            (plain.name(), plain.id().document_id()),
                        ],
                        timestamp,
                    );
                    (timestamp, guard)
                });
                // Count both final replies here so a mismatch names the corpus ordinal.
                let replies = lmtp.bdat_last_bytes(raw, 2, u8::MAX).await;
                if let Some(before) = before_refusal {
                    assert_eq!(replies, [LMTP_PARSE_REFUSAL; 2], "corpus ordinal {ordinal}");
                    wait_for_delivery(test).await;
                    for (account, docs) in [key, plain].into_iter().zip(&before) {
                        assert_eq!(&message_ids(test, account.id().document_id()).await, docs);
                    }
                    received_at
                        .push(append_headerless_pair(test, [key, plain], &before, raw).await);
                    accepted_imap += 1;
                    println!(
                        "differential: corpus ordinal {ordinal} refused by both LMTP recipients and stored by both IMAP APPENDs"
                    );
                } else {
                    assert_eq!(
                        replies
                            .iter()
                            .filter(|line| line.starts_with("250 "))
                            .count(),
                        2,
                        "corpus ordinal {ordinal}: {replies:?}"
                    );
                    accepted_lmtp[usize::from(ordinal >= BASE)] += 1;
                    // LMTP acknowledges queuing. Drain each pair before the
                    // next message can start its ordinary threading lookup.
                    wait_for_delivery(test).await;
                    let (timestamp, guard) = arrival.unwrap();
                    guard.assert_used();
                    received_at.push(timestamp);
                }
                ordinal += 1;
            }
            lmtp.quit().await;
        }
        wait_for_delivery(test).await;
        let (key_docs, key_hashes) =
            identify_deliveries(test, key, true, &mut seen.0, messages, batch).await;
        let (plain_docs, plain_hashes) =
            identify_deliveries(test, plain, false, &mut seen.1, messages, batch).await;
        assert_eq!(
            key_hashes, plain_hashes,
            "paired stored MIME digests in batch {batch}"
        );
        paired_docs.0.extend(key_docs);
        paired_docs.1.extend(plain_docs);
    }
    assert_eq!((paired_docs.0.len(), paired_docs.1.len()), (TOTAL, TOTAL));
    assert_eq!(ordinal, TOTAL);
    assert_eq!(
        accepted_lmtp,
        [BASE - HEADERLESS_MBOX_ORDINALS.len(), TOTAL - BASE]
    );
    assert_eq!(accepted_imap, HEADERLESS_MBOX_ORDINALS.len());
    println!(
        "differential: {} base and {} generated paired LMTP deliveries, {accepted_imap} paired IMAP APPENDs after expected paired refusals",
        accepted_lmtp[0], accepted_lmtp[1]
    );
    assert_eq!(received_at.len(), TOTAL);
    (paired_docs.0, paired_docs.1, received_at)
}

/// LMTP declines these known headerless records. IMAP accepts the original
/// bytes, with one captured INTERNALDATE shared by both copies.
async fn append_headerless_pair(
    test: &TestServer,
    accounts: [&Account; 2],
    before: &[Vec<u32>; 2],
    raw: &[u8],
) -> u64 {
    let now = Utc::now();
    let internal_date = now.format("%d-%b-%Y %H:%M:%S +0000");
    let mut command =
        format!("APPEND INBOX \"{internal_date}\" {{{}+}}\r\n", raw.len()).into_bytes();
    command.extend_from_slice(raw);
    let entry = test
        .server
        .inner
        .cache
        .za_search
        .entry(accounts[0].id().document_id());
    // Keep the keyed APPEND hooks out of the bulk preparation window.
    let _pass = entry.pass_lock.lock().await;
    for (side, account) in accounts.into_iter().enumerate() {
        let mut imap = ImapConnection::connect(b"_headerless ").await;
        imap.authenticate(account.name(), account.secret()).await;
        imap.send_bytes(&command).await;
        let uid = imap
            .assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .into_append_uid()
            .parse::<u32>()
            .unwrap();
        imap.send_ok("LOGOUT").await;

        let account_id = account.id().document_id();
        let doc = new_message(test, account_id, &before[side]).await;
        let cache = test.server.get_cached_messages(account_id).await.unwrap();
        let email = cache.email_by_id(&doc).unwrap();
        let [mailbox] = email.mailboxes() else {
            panic!("headerless document {doc} is not in one mailbox")
        };
        assert_eq!((mailbox.mailbox_id, mailbox.uid), (INBOX_ID, uid));
        assert_eq!(email.received_at(), now.timestamp() as u64);
        let stored = if side == 0 {
            open_message(test, account_id, account.secret(), doc)
                .await
                .1
        } else {
            stored_blob(test, &stored_hash(test, account_id, doc).await).await
        };
        assert!(
            stored == raw,
            "headerless document {doc} must retain its exact input octets"
        );
    }
    now.timestamp() as u64
}

async fn identify_deliveries(
    test: &TestServer,
    account: &Account,
    keyed: bool,
    seen: &mut BTreeSet<u32>,
    messages: &[Vec<u8>],
    batch: usize,
) -> (Vec<u32>, Vec<BlobHash>) {
    let id = account.id().document_id();
    let added = message_ids(test, id)
        .await
        .into_iter()
        .filter(|doc| !seen.contains(doc))
        .collect::<Vec<_>>();
    assert_eq!(
        added.len(),
        messages.len(),
        "delivery count for {} in batch {batch}",
        account.name()
    );
    let mut matched = vec![None; messages.len()];
    for doc in added {
        assert!(seen.insert(doc));
        let raw = if keyed {
            open_message(test, id, account.secret(), doc).await.1
        } else {
            stored_blob(test, &stored_hash(test, id, doc).await).await
        };
        let candidates = messages
            .iter()
            .enumerate()
            .filter(|(n, input)| matched[*n].is_none() && raw.ends_with(input))
            .collect::<Vec<_>>();
        let (n, input) = *candidates
            .iter()
            .max_by_key(|(_, input)| input.len())
            .expect("each stored MIME must contain the original input octets");
        // Identical raw inputs can still be distinct deliveries. Within the
        // batch assign their witnesses in local insert order.
        let n = candidates
            .iter()
            .find(|(_, other)| *other == input)
            .map_or(n, |(n, _)| *n);
        matched[n] = Some((doc, BlobHash::generate(&raw)));
    }
    matched.into_iter().map(|match_| match_.unwrap()).unzip()
}

fn header_ids(raw: &[u8]) -> Vec<Vec<String>> {
    let parsed = MessageParser::new().parse(raw).unwrap();
    parsed
        .headers()
        .all(HeaderName::MessageId)
        .map(|header| match header.value() {
            HeaderValue::Text(text) => vec![text.to_owned()],
            HeaderValue::TextList(list) => list.iter().map(String::from).collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn unique_message_ids(ids: &[Vec<Vec<String>>]) -> BTreeMap<&str, usize> {
    let mut occurrences = BTreeMap::new();
    for id in ids.iter().flatten().flatten() {
        *occurrences.entry(id.as_str()).or_insert(0) += 1;
    }
    ids.iter()
        .enumerate()
        .filter_map(|(ordinal, headers)| {
            let [values] = headers.as_slice() else {
                return None;
            };
            let [id] = values.as_slice() else { return None };
            (occurrences[id.as_str()] == 1).then_some((id.as_str(), ordinal))
        })
        .collect()
}

async fn inventory<'a>(
    test: &TestServer,
    account: &'a Account,
    docs: Vec<u32>,
    corpus: &[Vec<u8>],
    expected_received_at: &[u64],
) -> Fixture<'a> {
    let id = account.id().document_id();
    let mailboxes = account
        .jmap_get("Mailbox", ["id", "role"], Vec::<String>::new())
        .await;
    assert_eq!(mailboxes.name_at(0), "Mailbox/get");
    let inbox = parse_id(
        &mailboxes
            .list()
            .iter()
            .find(|mailbox| mailbox["role"] == "inbox")
            .unwrap()["id"],
    );
    assert_eq!(inbox.document_id(), INBOX_ID);
    let order = query_ids(account, &Value::Null, &received_sort(), false).await;
    assert_eq!(order.len(), TOTAL);
    let doc_to_ordinal = docs
        .iter()
        .enumerate()
        .map(|(n, doc)| (*doc, n))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(doc_to_ordinal.len(), TOTAL);
    let ids = corpus.iter().map(|raw| header_ids(raw)).collect::<Vec<_>>();
    let unique_ids = unique_message_ids(&ids);
    assert!(
        unique_ids.len() < TOTAL,
        "the fallback needs actual corpus records"
    );
    let mut emails = vec![Id::default(); TOTAL];
    let mut threads = vec![Id::default(); TOTAL];
    let mut email_to_ordinal = BTreeMap::new();
    let cache = test.server.get_cached_messages(id).await.unwrap();
    let (uids, received_at): (Vec<_>, Vec<_>) = docs
        .iter()
        .enumerate()
        .map(|(ordinal, doc)| {
            let email = cache.email_by_id(doc).unwrap();
            let [mailbox] = email.mailboxes() else {
                panic!("document {doc} is not in one mailbox")
            };
            assert_eq!(mailbox.mailbox_id, INBOX_ID);
            let timestamp = email.received_at();
            assert_eq!(
                timestamp, expected_received_at[ordinal],
                "stored arrival for account {id} at corpus ordinal {ordinal}"
            );
            (mailbox.uid, timestamp)
        })
        .unzip();
    assert_eq!(uids.iter().copied().collect::<BTreeSet<_>>().len(), TOTAL);

    for chunk in order.chunks(PAGE) {
        let reply = account
            .jmap_get(
                "Email",
                [
                    "id",
                    "threadId",
                    "messageId",
                    "mailboxIds",
                    "subject",
                    "sentAt",
                    "size",
                ],
                chunk.iter(),
            )
            .await;
        assert_eq!(reply.name_at(0), "Email/get");
        assert_eq!(reply.not_found().count(), 0);
        assert_eq!(reply.list().len(), chunk.len());
        for email in reply.list() {
            let email_id = parse_id(&email["id"]);
            let witness = doc_to_ordinal[&email_id.document_id()];
            let ordinal = email["messageId"]
                .as_array()
                .and_then(|values| {
                    let [id] = values.as_slice() else { return None };
                    unique_ids.get(id.as_str()?).copied()
                })
                .unwrap_or(witness);
            assert_eq!(
                ordinal, witness,
                "JMAP identity must corroborate the delivery"
            );
            let parsed = ids[ordinal]
                .last()
                .filter(|values| !values.is_empty())
                .map_or(Value::Null, |values| json!(values));
            assert!(
                email["messageId"] == parsed,
                "JMAP Message-ID values at ordinal {ordinal}"
            );
            assert_eq!(email["mailboxIds"], json!({(inbox.to_string()): true}));
            assert!(email["size"].as_u64().unwrap() >= corpus[ordinal].len() as u64);
            if (BASE + 12..BASE + 15).contains(&ordinal) {
                assert!(email["sentAt"].is_null(), "date edge {ordinal}");
            } else if ordinal == BASE + 15 {
                assert!(
                    email["sentAt"]
                        .as_str()
                        .is_some_and(|date| date.starts_with("9999-"))
                );
            } else if ordinal == TOTAL - 1 {
                assert!(
                    email["subject"].is_null()
                        || email["subject"]
                            .as_str()
                            .is_some_and(|s| s.trim().is_empty())
                );
            }
            assert!(email_to_ordinal.insert(email_id, ordinal).is_none());
            emails[ordinal] = email_id;
            threads[ordinal] = parse_id(&email["threadId"]);
            assert_eq!(emails[ordinal].prefix_id(), threads[ordinal].document_id());
        }
    }
    assert_eq!(email_to_ordinal.len(), TOTAL);
    let mut partition = Vec::new();
    let mut covered = BTreeSet::new();
    let distinct = threads
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for chunk in distinct.chunks(PAGE) {
        let reply = account
            .jmap_get("Thread", ["id", "emailIds"], chunk.iter())
            .await;
        assert_eq!(reply.name_at(0), "Thread/get");
        assert_eq!(reply.not_found().count(), 0);
        assert_eq!(reply.list().len(), chunk.len());
        let mut returned = BTreeSet::new();
        for thread in reply.list() {
            let id = parse_id(&thread["id"]);
            assert!(chunk.contains(&id) && returned.insert(id));
            let mut group = thread["emailIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| {
                    let ordinal = email_to_ordinal[&parse_id(value)];
                    assert_eq!(threads[ordinal], id);
                    assert!(covered.insert(ordinal));
                    ordinal
                })
                .collect::<Vec<_>>();
            group.sort_unstable();
            assert!(!group.is_empty());
            partition.push(group);
        }
    }
    assert_eq!(covered.len(), TOTAL);
    partition.sort();
    Fixture {
        account,
        emails,
        email_to_ordinal,
        uids,
        received_at,
        inbox,
        threads,
        partition,
    }
}

fn compare_partitions(key: &Fixture<'_>, plain: &Fixture<'_>) {
    assert!(plain.partition.contains(&vec![K_ROOT, K1]));
    assert!(plain.partition.contains(&vec![K2_ROOT, K2]));
    let mut expected = plain
        .partition
        .iter()
        .flat_map(|group| {
            if group == &[K_ROOT, K1] {
                vec![vec![K_ROOT], vec![K1]]
            } else {
                vec![group.clone()]
            }
        })
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(key.partition.len(), expected.len());
    for (actual, expected) in key.partition.iter().zip(&expected) {
        assert_eq!(actual, expected, "JMAP partition by delivery ordinal");
    }
}

fn parse_id(id: &Value) -> Id {
    id.as_str().unwrap().parse().unwrap()
}

fn received_sort() -> Value {
    json!([{"property": "receivedAt", "isAscending": false}])
}

async fn query_ids(account: &Account, filter: &Value, sort: &Value, collapse: bool) -> Vec<Id> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        let mut args = json!({
            "accountId": account.id_string(),
            "sort": sort, "collapseThreads": collapse, "position": ids.len(),
            "limit": PAGE, "calculateTotal": true,
        });
        if !filter.is_null() {
            args["filter"] = filter.clone();
        }
        let reply = account.jmap_method_call("Email/query", args).await;
        assert_eq!(
            reply.name_at(0),
            "Email/query",
            "query error: {:?}",
            reply.error_type_at(0)
        );
        let body = reply.method_response();
        assert_eq!(body["position"], ids.len());
        let total = body["total"].as_u64().unwrap() as usize;
        assert!(total <= TOTAL);
        let page = reply.object_ids().collect::<Vec<_>>();
        assert!(
            page.iter().all(|id| seen.insert(*id)),
            "query pages overlap"
        );
        ids.extend(page.iter().copied());
        if ids.len() == total {
            return ids;
        }
        assert!(
            !page.is_empty() && ids.len() < total,
            "query total must match pages"
        );
    }
}

fn ordinals(fixture: &Fixture<'_>, ids: &[Id]) -> Vec<usize> {
    ids.iter().map(|id| fixture.email_to_ordinal[id]).collect()
}

fn collapse(plain: &Fixture<'_>, ordered: &[usize], split: bool) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    ordered
        .iter()
        .copied()
        .filter(|n| seen.insert((plain.threads[*n], split && *n == K1)))
        .collect()
}

fn assert_order(actual: &[usize], expected: &[usize], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: result count");
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, b, "{label}: delivery ordinal at result offset {i}");
    }
}

async fn snippets(
    fixture: &Fixture<'_>,
    filter: &Value,
    ordered: &[usize],
) -> BTreeMap<usize, (Value, Value)> {
    let mut snippets = BTreeMap::new();
    for chunk in ordered.chunks(90) {
        let email_ids = chunk
            .iter()
            .map(|ordinal| fixture.emails[*ordinal].to_string())
            .collect::<Vec<_>>();
        let reply = fixture.account.jmap_method_call("SearchSnippet/get", json!({
            "accountId": fixture.account.id_string(), "filter": filter, "emailIds": email_ids,
        })).await;
        assert_eq!(reply.name_at(0), "SearchSnippet/get");
        assert!(
            reply
                .method_response()
                .get("notFound")
                .is_some_and(|value| {
                    value.is_null() || value.as_array().is_some_and(Vec::is_empty)
                }),
            "SearchSnippet/get must return null or an empty notFound array"
        );
        assert_eq!(reply.list().len(), chunk.len());
        for snippet in reply.list() {
            let n = fixture.email_to_ordinal[&parse_id(&snippet["emailId"])];
            assert!(chunk.contains(&n));
            assert!(
                snippets
                    .insert(n, (snippet["subject"].clone(), snippet["preview"].clone()))
                    .is_none()
            );
        }
    }
    snippets
}

async fn compare_query(
    key: &Fixture<'_>,
    plain: &Fixture<'_>,
    label: &str,
    filters: [&Value; 2],
    sort: &Value,
    text: bool,
) -> Vec<usize> {
    let [key_filter, plain_filter] = filters;
    let label = if key_filter.is_null() {
        label.to_owned()
    } else {
        format!("filter {label}")
    };
    let ordinary = ordinals(
        plain,
        &query_ids(plain.account, plain_filter, sort, false).await,
    );
    let keyed = ordinals(key, &query_ids(key.account, key_filter, sort, false).await);
    assert_order(&keyed, &ordinary, &format!("{label} / uncollapsed parity"));
    let plain_collapsed = ordinals(
        plain,
        &query_ids(plain.account, plain_filter, sort, true).await,
    );
    let key_collapsed = ordinals(key, &query_ids(key.account, key_filter, sort, true).await);
    assert_order(
        &plain_collapsed,
        &collapse(plain, &ordinary, false),
        &format!("{label} / ordinary collapse"),
    );
    assert_order(
        &key_collapsed,
        &collapse(plain, &ordinary, true),
        &format!("{label} / keyed collapse"),
    );
    if text {
        // The uncollapsed protocol results include the union of all collapsed
        // representatives, including both sides of the permitted K1 split.
        let expected = snippets(plain, plain_filter, &ordinary).await;
        let actual = snippets(key, key_filter, &keyed).await;
        assert_eq!(actual.len(), expected.len());
        for (ordinal, snippet) in actual {
            assert!(
                expected.get(&ordinal) == Some(&snippet),
                "{label}: snippet at ordinal {ordinal}"
            );
        }
    }
    ordinary
}

fn first_terms(raw: &[u8]) -> (String, String, String, String) {
    let parsed = MessageParser::new().parse(raw).unwrap();
    let from = parsed
        .from()
        .unwrap()
        .first()
        .unwrap()
        .address()
        .unwrap()
        .split('@')
        .next()
        .unwrap();
    let to = parsed
        .to()
        .unwrap()
        .first()
        .unwrap()
        .address()
        .unwrap()
        .split('@')
        .next()
        .unwrap();
    let subject = parsed
        .subject()
        .unwrap()
        .split_whitespace()
        .find(|word| word.len() >= 4)
        .unwrap();
    let body = parsed.text_body().find_map(|part| part.text()).unwrap();
    let word = body
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .max_by_key(|word| word.len())
        .expect("the first corpus text part must contain an ASCII alphabetic word");
    (from.into(), to.into(), subject.into(), word.into())
}

async fn compare_jmap(key: &Fixture<'_>, plain: &Fixture<'_>, first: &[u8]) -> usize {
    let (from, to, subject, word) = first_terms(first);
    let and = |terms: Vec<Value>| json!({"operator": "AND", "conditions": terms});
    let filters = [
        ("from", json!({"from": from}), false),
        ("to", json!({"to": to}), false),
        ("cc", json!({"cc": "fixture-copy"}), false),
        ("bcc", json!({"bcc": "fixture-hidden"}), false),
        ("subject", json!({"subject": subject}), true),
        ("body", json!({"body": word}), true),
        ("text", json!({"text": word}), true),
        (
            "header value",
            json!({"header": ["X-Private", "fixture-enclosed"]}),
            false,
        ),
        ("header presence", json!({"header": ["X-Private"]}), false),
        ("has attachment", json!({"hasAttachment": true}), false),
        ("has no attachment", json!({"hasAttachment": false}), false),
        (
            "sent before",
            json!({"sentBefore": "2027-01-01T00:00:00Z"}),
            false,
        ),
        (
            "sent after",
            json!({"sentAfter": "2025-01-01T00:00:00Z"}),
            false,
        ),
        (
            "after and from",
            and(vec![
                json!({"after": "2000-01-01T00:00:00Z"}),
                json!({"from": from}),
            ]),
            false,
        ),
        (
            "keyword and not and body",
            and(vec![
                json!({"hasKeyword": "$seen"}),
                json!({"operator":"NOT", "conditions":[{"from": "missingfromwitness"}]}),
                json!({"body": word}),
            ]),
            true,
        ),
        (
            "after and before and text",
            and(vec![
                json!({"after": "2000-01-01T00:00:00Z"}),
                json!({"before": "2090-01-01T00:00:00Z"}),
                json!({"text": word}),
            ]),
            true,
        ),
        ("exact phrase", json!({"body": PHRASE}), true),
        ("AND quoted text", and(vec![json!({"text": PHRASE})]), true),
        (
            "early large document term",
            json!({"body": "uniquefixture000000"}),
            true,
        ),
        (
            "late large document term",
            json!({"body": "lastfixturetruncationword"}),
            true,
        ),
    ];
    let mut forms = 0;
    let sort = received_sort();
    for (label, filter, snippets) in filters {
        let matches = compare_query(key, plain, label, [&filter, &filter], &sort, snippets).await;
        forms += 2;
        match label {
            "early large document term" => assert_eq!(matches, [BIG]),
            "late large document term" => assert!(matches.is_empty()),
            "exact phrase" | "AND quoted text" => {
                assert!(matches.contains(&K_ROOT));
                assert!(!matches.contains(&K2_ROOT) && !matches.contains(&(BASE + 9)));
            }
            _ => assert!(!matches.is_empty(), "{label}: no ordinary match"),
        }
    }
    let mailbox_filter = |inbox: Id| {
        and(vec![
            json!({"inMailbox": inbox.to_string()}),
            json!({"cc": "fixture-copy"}),
        ])
    };
    assert!(
        !compare_query(
            key,
            plain,
            "inMailbox and cc",
            [&mailbox_filter(key.inbox), &mailbox_filter(plain.inbox)],
            &sort,
            false,
        )
        .await
        .is_empty()
    );
    forms += 2;
    for (n, word) in ATTACHMENT_WORDS.iter().enumerate() {
        let filter = json!({"text": word});
        let matches = compare_query(
            key,
            plain,
            "attachment text",
            [&filter, &filter],
            &sort,
            true,
        )
        .await;
        assert!(matches.contains(&(BASE + n)));
        forms += 2;
    }
    for (n, subject) in SUBJECTS.iter().enumerate() {
        let filter = json!({"subject": subject});
        let matches = compare_query(
            key,
            plain,
            "international subject",
            [&filter, &filter],
            &sort,
            true,
        )
        .await;
        assert!(matches.contains(&(BASE + 4 + n)));
        forms += 2;
    }
    for property in [
        "from",
        "to",
        "cc",
        "subject",
        "sentAt",
        "hasKeyword",
        "allInThreadHaveKeyword",
        "someInThreadHaveKeyword",
    ] {
        for ascending in [true, false] {
            for secondary in [false, true] {
                let mut primary = json!({"property": property, "isAscending": ascending});
                if property.contains("Keyword") {
                    primary["keyword"] = json!("$seen");
                }
                let mut sort = vec![primary];
                if secondary {
                    sort.push(json!({"property": "receivedAt", "isAscending": false}));
                }
                let sort = json!(sort);
                if property == "cc" {
                    for collapse in [false, true] {
                        let mut refusals = Vec::new();
                        for fixture in [plain, key] {
                            let reply = fixture.account.jmap_method_call("Email/query", json!({
                                "accountId": fixture.account.id_string(), "sort": sort, "collapseThreads": collapse,
                            })).await;
                            assert_eq!(reply.error_type_at(0), Some("unsupportedSort"));
                            let result = reply.method_response();
                            assert!(result.get("ids").is_none());
                            assert!(
                                result["description"]
                                    .as_str()
                                    .is_some_and(|msg| msg.contains("cc"))
                            );
                            refusals.push(result.clone());
                        }
                        assert_eq!(refusals[0], refusals[1]);
                    }
                } else {
                    let matches = compare_query(
                        key,
                        plain,
                        &format!(
                            "sort {property} ascending={ascending} receivedAt-secondary={secondary}"
                        ),
                        [&Value::Null, &Value::Null],
                        &sort,
                        false,
                    )
                    .await;
                    assert_eq!(matches.len(), TOTAL);
                }
                forms += 2;
            }
        }
    }
    forms
}

async fn compare_imap(key: &Fixture<'_>, plain: &Fixture<'_>, corpus: &[Vec<u8>]) -> usize {
    let mut keyed = ImapConnection::connect(b"_diff ").await;
    let mut ordinary = ImapConnection::connect(b"_diff ").await;
    let key_map = fetch_uid_map(&mut keyed, key, corpus).await;
    let plain_map = fetch_uid_map(&mut ordinary, plain, corpus).await;

    let (from, to, subject, word) = first_terms(&corpus[0]);
    let first_id = header_ids(&corpus[0]).into_iter().flatten().next().unwrap();
    let mut searches = [
        "ALL",
        "1:2",
        "UID 1:2",
        "ANSWERED",
        "UNANSWERED",
        "DELETED",
        "UNDELETED",
        "DRAFT",
        "UNDRAFT",
        "FLAGGED",
        "UNFLAGGED",
        "SEEN",
        "UNSEEN",
        "KEYWORD $Label1",
        "UNKEYWORD $Label1",
        "RECENT",
        "NEW",
        "OLD",
        "MODSEQ 1",
        "BEFORE 1-Jan-2090",
        "ON 1-Jan-2000",
        "SINCE 1-Jan-2000",
        "OLDER 0",
        "YOUNGER 86400",
        "LARGER 100",
        "SMALLER 100000",
        "UIDAFTER 1",
        "UIDBEFORE 5",
        "BCC fixture-hidden",
        "CC fixture-copy",
        "HEADER X-Private fixture-enclosed",
        "SENTBEFORE 1-Jan-2027",
        "SENTON 20-Oct-2026",
        "SENTSINCE 1-Jan-2025",
        "KEYWORD $HasAttachment",
        "KEYWORD $HasNoAttachment",
        "UNKEYWORD $HasAttachment",
        "UNKEYWORD $HasNoAttachment",
    ]
    .into_iter()
    .map(|s| (s.to_string(), s.to_string()))
    .collect::<Vec<_>>();
    for term in [
        format!("FROM {}", quote(&from)),
        format!("TO {}", quote(&to)),
        format!("SUBJECT {}", quote(&subject)),
        format!("BODY {}", quote(&word)),
        format!("TEXT {}", quote(&word)),
        format!("HEADER Message-ID {}", quote(&first_id)),
        format!("OR FROM {} SEEN", quote(&from)),
        format!("NOT (FROM {} ANSWERED)", quote(&from)),
    ] {
        searches.push((term.clone(), term));
    }
    // Resolve wire arguments separately, using each account's real IDs.
    searches.extend([
        (
            format!("EMAILID {}", key.emails[0]),
            format!("EMAILID {}", plain.emails[0]),
        ),
        (
            format!("THREADID {}", key.threads[K2_ROOT]),
            format!("THREADID {}", plain.threads[K2_ROOT]),
        ),
    ]);

    let mut forms = 0;
    for (key_form, plain_form) in searches {
        let actual = imap_numbers(
            &mut keyed,
            format!("UID SEARCH {key_form}").as_bytes(),
            "* SEARCH",
            &key_map,
        )
        .await;
        let expected = imap_numbers(
            &mut ordinary,
            format!("UID SEARCH {plain_form}").as_bytes(),
            "* SEARCH",
            &plain_map,
        )
        .await;
        assert_order(&actual, &expected, &key_form);
        if !matches!(
            key_form.as_str(),
            "DELETED" | "RECENT" | "NEW" | "OLD" | "ON 1-Jan-2000"
        ) {
            assert!(!expected.is_empty(), "{key_form}: no ordinary matches");
        }
        forms += 1;
    }
    // Use the actual sequence-number namespace as well as UID.
    let seq_key = key_map
        .values()
        .copied()
        .enumerate()
        .map(|(n, ordinal)| ((n + 1) as u32, ordinal))
        .collect();
    let seq_plain = plain_map
        .values()
        .copied()
        .enumerate()
        .map(|(n, ordinal)| ((n + 1) as u32, ordinal))
        .collect();
    let actual = imap_numbers(&mut keyed, b"SEARCH 1:2", "* SEARCH", &seq_key).await;
    let expected = imap_numbers(&mut ordinary, b"SEARCH 1:2", "* SEARCH", &seq_plain).await;
    assert_order(&actual, &expected, "IMAP sequence search");
    assert_eq!(actual.len(), 2);
    forms += 1;

    let latin = b"UID SEARCH CHARSET ISO-8859-1 SUBJECT {4+}\r\ncaf\xE9";
    let actual = imap_numbers(&mut keyed, latin, "* SEARCH", &key_map).await;
    let expected = imap_numbers(&mut ordinary, latin, "* SEARCH", &plain_map).await;
    assert_order(&actual, &expected, "IMAP Latin-1 subject literal");
    assert!(expected.contains(&(BASE + 6)));
    forms += 1;

    for sort in [
        "SUBJECT",
        "DATE",
        "FROM",
        "DISPLAYFROM",
        "TO",
        "DISPLAYTO",
        "REVERSE SIZE",
        "ARRIVAL",
        "REVERSE SUBJECT DATE",
    ] {
        let cmd = format!("UID SORT ({sort}) UTF-8 ALL");
        let actual = imap_numbers(&mut keyed, cmd.as_bytes(), "* SORT", &key_map).await;
        let expected = imap_numbers(&mut ordinary, cmd.as_bytes(), "* SORT", &plain_map).await;
        assert_order(&actual, &expected, sort);
        assert_eq!(expected.len(), TOTAL);
        forms += 1;
    }
    let mut refusal = Vec::new();
    for imap in [&mut ordinary, &mut keyed] {
        imap.send("UID SORT (CC) UTF-8 ALL").await;
        let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
        assert!(lines.iter().all(|line| !line.starts_with("* SORT")));
        assert!(
            lines
                .last()
                .unwrap()
                .contains("Sorting by CC is not supported.")
        );
        refusal.push(lines);
    }
    assert_eq!(refusal[0], refusal[1]);
    forms += 1;

    for filter in [
        "ALL".to_string(),
        format!("FROM {}", quote(&from)),
        format!("BODY {}", quote(&word)),
    ] {
        let cmd = format!("UID THREAD REFERENCES UTF-8 {filter}");
        ordinary.send(&cmd).await;
        let expected = parse_thread(&response_data(&mut ordinary, "* THREAD").await, &plain_map);
        keyed.send(&cmd).await;
        let actual = parse_thread(&response_data(&mut keyed, "* THREAD").await, &key_map);
        assert!(!expected.is_empty());
        let projected = project_thread(expected, &plain.uids);
        assert_eq!(actual.len(), projected.len(), "{filter}: IMAP thread count");
        for (n, (actual, expected)) in actual.iter().zip(&projected).enumerate() {
            assert_eq!(actual, expected, "{filter}: thread group {n}");
        }
        forms += 1;
    }
    keyed.send_ok("LOGOUT").await;
    ordinary.send_ok("LOGOUT").await;
    forms
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

async fn response_data(imap: &mut ImapConnection, prefix: &str) -> String {
    let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let mut data = lines.iter().filter_map(|line| line.strip_prefix(prefix));
    let value = data
        .next()
        .expect("command has no result line")
        .trim_start()
        .to_owned();
    assert!(data.next().is_none());
    value
}

async fn imap_numbers(
    imap: &mut ImapConnection,
    command: &[u8],
    prefix: &str,
    map: &BTreeMap<u32, usize>,
) -> Vec<usize> {
    imap.send_bytes(command).await;
    let response = response_data(imap, prefix).await;
    let ids = if let Some((ids, modseq)) = response.split_once(" (MODSEQ ") {
        assert!(command.ends_with(b"MODSEQ 1"));
        assert!(modseq.strip_suffix(')').unwrap().parse::<u64>().unwrap() > 0);
        ids
    } else {
        assert!(!command.ends_with(b"MODSEQ 1"));
        response.as_str()
    };
    let ordinals = ids
        .split_ascii_whitespace()
        .map(|part| map[&part.parse::<u32>().unwrap()])
        .collect::<Vec<_>>();
    assert_eq!(
        ordinals.iter().copied().collect::<BTreeSet<_>>().len(),
        ordinals.len()
    );
    ordinals
}

async fn fetch_uid_map(
    imap: &mut ImapConnection,
    fixture: &Fixture<'_>,
    corpus: &[Vec<u8>],
) -> BTreeMap<u32, usize> {
    imap.authenticate(fixture.account.name(), fixture.account.secret())
        .await;
    imap.send_ok("SELECT INBOX").await;
    imap.send("UID FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let by_uid = fixture
        .uids
        .iter()
        .enumerate()
        .map(|(n, uid)| (*uid, n))
        .collect::<BTreeMap<_, _>>();
    let mut remaining = imap.last_response_bytes();
    let mut last_uid = 0;
    let mut count = 0;
    loop {
        let end = remaining.windows(2).position(|w| w == b"\r\n").unwrap();
        let (line, rest) = remaining.split_at(end);
        remaining = &rest[2..];
        if line.starts_with(b"_diff OK ") {
            assert!(remaining.is_empty());
            break;
        }
        let line = std::str::from_utf8(line).unwrap();
        let (seq, attrs) = line
            .strip_prefix("* ")
            .unwrap()
            .split_once(" FETCH (UID ")
            .unwrap();
        count += 1;
        assert_eq!(seq.parse::<usize>().unwrap(), count);
        let (uid, literal) = attrs
            .split_once(" BODY[HEADER.FIELDS (MESSAGE-ID)] {")
            .unwrap();
        let uid = uid.parse::<u32>().unwrap();
        assert!(uid > last_uid);
        last_uid = uid;
        let length = literal.strip_suffix('}').unwrap().parse::<usize>().unwrap();
        let (bytes, rest) = remaining.split_at(length);
        remaining = rest.strip_prefix(b")\r\n").unwrap();
        let ordinal = by_uid[&uid];
        let mut headers = bytes.to_vec();
        if headers.is_empty() || headers == b"\r\n" {
            assert!(
                header_ids(&corpus[ordinal]).is_empty(),
                "FETCH missing Message-ID at ordinal {ordinal}"
            );
        } else {
            // Keep every raw literal byte. A harmless additional field
            // ensures a header-only fragment has a parser-visible envelope.
            headers.splice(..0, b"X-Parse: fixture\r\n".iter().copied());
            assert_eq!(
                header_ids(&headers),
                header_ids(&corpus[ordinal]),
                "FETCH Message-ID at ordinal {ordinal}"
            );
        }
    }
    assert_eq!(count, TOTAL);
    by_uid
}

fn parse_thread(wire: &str, map: &BTreeMap<u32, usize>) -> Vec<Vec<usize>> {
    // The server currently emits flat siblings within each reference group.
    // Reject any change of wire nesting instead of silently flattening it.
    let mut wire = wire.trim();
    let mut groups = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(next) = wire.strip_prefix('(') {
        let (group, tail) = next.split_once(')').unwrap();
        let group = group
            .split_ascii_whitespace()
            .map(|s| {
                let ordinal = map[&s.parse::<u32>().unwrap()];
                assert!(seen.insert(ordinal));
                ordinal
            })
            .collect::<Vec<_>>();
        assert!(!group.is_empty());
        groups.push(group);
        wire = tail.trim_start();
    }
    assert!(wire.is_empty(), "malformed THREAD grouping");
    groups
}

fn project_thread(mut groups: Vec<Vec<usize>>, ordinary_uids: &[u32]) -> Vec<Vec<usize>> {
    if let Some(group) = groups
        .iter_mut()
        .find(|g| g.contains(&K_ROOT) && g.contains(&K1))
    {
        assert_eq!(group.len(), 2);
        group.retain(|n| *n != K1);
        groups.push(vec![K1]);
        groups.sort_by_key(|group| ordinary_uids[group[0]]);
    }
    groups
}

async fn case_g5(test: &TestServer, key: &Account) {
    println!("differential: G5 compression isolation");
    let _guard = search_config_guard!(test, segments: 64);
    let id = key.id().document_id();
    let head = search_manifest(test, id).await.content.unwrap();
    assert!(head.length + 4 < 64);
    let vocabulary = |seed: u64| {
        // The words are distinct, fixed width, and identical across all
        // documents using a vocabulary. Repeated whole-blob frames benefit.
        (0..32)
            .map(|i| {
                (0..8)
                    .map(|n| {
                        let x = seed
                            .wrapping_add(i * 8 + n)
                            .wrapping_mul(0x9e3779b97f4a7c15);
                        format!("word{x:016x}")
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    // The sole header contributes one indexed field. The body vocabulary
    // keeps its fixed position after Headers in the term encoder.
    let mime = |id: &str, body: &str| {
        let raw = format!("Message-ID: <{id}@za.test>\r\n\r\n{body}\r\n");
        let message = MessageParser::new().parse(raw.as_bytes()).unwrap();
        assert_eq!(message.headers().len(), 1);
        assert_eq!(message.message_id(), Some(format!("{id}@za.test").as_str()));
        raw
    };
    let shared = vocabulary(42);
    let neighbor_a = mime("g5-neighbor-a", &shared);
    let neighbor_b = mime("g5-neighbor-b", &vocabulary(4242));
    let twenty = (0..20)
        .map(|n| mime(&format!("g5-fixture-{n:02}"), &shared))
        .collect::<Vec<_>>();
    let batches = [
        vec![neighbor_a.as_str()],
        std::iter::once(neighbor_a.as_str())
            .chain(twenty.iter().map(String::as_str))
            .collect(),
        vec![neighbor_b.as_str()],
        std::iter::once(neighbor_b.as_str())
            .chain(twenty.iter().map(String::as_str))
            .collect(),
    ];
    let mut imap = ImapConnection::connect(b"_g5 ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    let mut deltas = Vec::new();
    for batch in batches {
        let entry = test.server.inner.cache.za_search.entry(id);
        let pass = entry.pass_lock.lock().await;
        let before = search_manifest(test, id).await;
        let existing = message_ids(test, id)
            .await
            .into_iter()
            .collect::<BTreeSet<_>>();
        for (n, message) in batch.iter().enumerate() {
            if n == 0 {
                imap.send(&format!("APPEND INBOX {{{}}}", message.len()))
                    .await;
            } else {
                imap.send_untagged(&format!(" {{{}}}", message.len())).await;
            }
            imap.assert_read(Type::Continuation, ResponseType::Ok).await;
            if n + 1 == batch.len() {
                imap.send_untagged(message).await;
                imap.assert_read(Type::Tagged, ResponseType::Ok).await;
            } else {
                imap.send_raw(message).await;
            }
        }
        let docs = message_ids(test, id)
            .await
            .into_iter()
            .filter(|doc| !existing.contains(doc))
            .collect::<Vec<_>>();
        assert_eq!(docs.len(), batch.len());
        let mut expected = batch
            .iter()
            .map(|s| BlobHash::generate(s.as_bytes()))
            .collect::<Vec<_>>();
        let mut actual = Vec::new();
        for doc in docs {
            actual.push(BlobHash::generate(
                open_message(test, id, key.secret(), doc).await.1,
            ));
        }
        for digest in actual {
            let n = expected
                .iter()
                .position(|value| *value == digest)
                .expect("APPEND preserved exact bytes");
            expected.swap_remove(n);
        }
        assert!(expected.is_empty());
        assert_eq!(search_manifest(test, id).await, before);
        drop(pass);
        search_pass_request(key).await;
        let after = search_manifest(test, id).await;
        assert_eq!(after.revision, before.revision + 1);
        let prior = before.content.unwrap();
        let next = after.content.unwrap();
        assert_eq!(next.length, prior.length + 1);
        let growth = next.size.checked_sub(prior.size).unwrap();
        let raw = test
            .server
            .get_blob(next.hash.as_slice(), 0..usize::MAX)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(raw.len() as u64, growth);
        let (header, _) = BlobHeader::decode(&raw).unwrap();
        assert_eq!(
            (header.kind, header.tier, header.segment),
            (BlobKind::Content, 0, next.segment)
        );
        assert_eq!(header.predecessor, Some((prior.hash, prior.segment)));
        deltas.push(growth);
    }
    let [a, b, c, d]: [u64; 4] = deltas.try_into().unwrap();
    assert!(b > a && d > c);
    assert_eq!(b - a, d - c, "G5 own-neighbor deltas: {a}, {b}, {c}, {d}");
    imap.send_ok("LOGOUT").await;
}
