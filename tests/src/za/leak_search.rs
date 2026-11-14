/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    mail::{deliver, key_mail_account, message_ids, message_with, reply_to},
};
use crate::utils::{
    account::Account,
    server::TestServer,
    smtp::SmtpConnection,
    za::{
        assert_search_publication, current_mailbox_ids, email_log_tip,
        finish_bulk_delivery_indexing, log_records_since, principal_blob_links,
        raise_bulk_lmtp_rates, restore_bulk_lmtp_rates, search_config_guard, search_manifest,
        search_pass_request, wait_for_delivery, za_index_current, za_search_thread_part, za_unlock,
    },
};
use email::message::{index::extractors::VisitText, thread::ThreadFields};
use mail_parser::{HeaderName, MessageParser};
use registry::schema::{
    prelude::Property,
    structs::{Jmap, MtaStageData},
};
use serde_json::json;
use std::collections::BTreeSet;
use store::{
    query::log::Change,
    write::compress::{COMPRESS_WATERMARK_DICTIONARY, decompress},
    za_search::{
        BlobHeader, BlobKind, K,
        blob::{SEAL_OVERHEAD, public_len},
        compaction_allowance,
        manifest::ThreadBlobEntry,
        padded_len,
    },
};
use types::{collection::SyncCollection, id::Id};

const SENDER: &str = "sender@example.org";
const SUBJECT: &str = "Oracle thread";
const PAD: usize = 65536;

pub async fn test(test: &mut TestServer) {
    let key = key_mail_account(test, "keymail65@example.com", STRONG).await;
    let twin = key_mail_account(test, "keymail66@example.com", STRONG).await;
    let compact = key_mail_account(test, "keymail67@example.com", STRONG).await;
    let compact_twin = key_mail_account(test, "keymail68@example.com", STRONG).await;
    let saved_rates = raise_bulk_lmtp_rates(test).await;
    let _segments = search_config_guard!(test, segments: 64);

    println!("leak_search: membership");
    membership(test, &key, &twin).await;
    clean(test, &[&key, &twin]).await;
    println!("leak_search: F1");
    filler_references(test, &key, &twin).await;
    clean(test, &[&key, &twin]).await;
    println!("leak_search: G1");
    junk_expunged(test, &key).await;
    clean(test, &[&key]).await;
    println!("leak_search: R21");
    cohorts(test, &key).await;
    clean(test, &[&key]).await;
    println!("leak_search: compaction class");
    {
        let _compact = search_config_guard!(test, segments: 4);
        let merged = compact_class(test, &compact, true).await;
        let separate = compact_class(test, &compact_twin, false).await;
        assert_eq!(merged, padded_len(compaction_allowance(40), PAD));
        assert_eq!(separate, merged);
    }
    clean(test, &[&compact, &compact_twin]).await;
    restore_bulk_lmtp_rates(test, saved_rates).await;
}

fn account_id(account: &Account) -> u32 {
    account.id().document_id()
}

async fn clean(test: &TestServer, accounts: &[&Account]) {
    wait_for_delivery(test).await;
    for account in accounts {
        let id = account_id(account);
        let before = message_ids(test, id).await;
        let mailbox_ids = current_mailbox_ids(account).await;
        let mailboxes = mailbox_ids.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(mailboxes.len(), mailbox_ids.len());
        assert!(!mailboxes.is_empty());

        let inventory = account
            .jmap_get("Email", ["id"], Vec::<String>::new())
            .await;
        assert_eq!(inventory.name_at(0), "Email/get");
        assert_eq!(inventory.not_found().count(), 0);
        let ids = inventory
            .list()
            .iter()
            .map(|item| item["id"].as_str().unwrap().parse::<Id>().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), inventory.list().len());
        let documents = ids
            .iter()
            .map(|id| id.document_id())
            .collect::<BTreeSet<_>>();
        assert_eq!(documents.len(), ids.len());
        assert_eq!(documents.into_iter().collect::<Vec<_>>(), before);

        // Retain the mailbox records so the next delivery's INBOX link
        // still names an existing mailbox.
        if !ids.is_empty() {
            let reply = account
                .jmap_destroy("Email", ids.iter(), Vec::<(&str, &str)>::new())
                .await;
            assert_eq!(reply.name_at(0), "Email/set");
            let destroyed = reply.destroyed_ids().collect::<Vec<_>>();
            assert_eq!(destroyed.len(), ids.len());
            assert_eq!(destroyed.into_iter().collect::<BTreeSet<_>>(), ids);
            let not_destroyed = &reply.method_response()["notDestroyed"];
            assert!(
                not_destroyed.is_null() || not_destroyed.as_object().is_some_and(|v| v.is_empty())
            );
        }

        test.wait_for_tasks().await;
        assert!(message_ids(test, id).await.is_empty());
        // Drive the deletion pass before another keyed request can do it.
        search_pass_request(account).await;
        let after = current_mailbox_ids(account).await;
        assert_eq!(after.len(), mailboxes.len());
        assert_eq!(after.into_iter().collect::<BTreeSet<_>>(), mailboxes);
    }
}

fn message_id(message: &str) -> &str {
    message
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap()
}

fn bridge(a: &str, b: &str, joins: bool) -> String {
    let a_id = message_id(a);
    let b_id = message_id(b);
    let msg = reply_to(a, SUBJECT, "bridge").replace(
        &format!("References: {a_id}"),
        &format!("References: {a_id} {b_id}"),
    );
    if joins {
        msg
    } else {
        without_matches(&msg, a, b)
    }
}

// Thread reference hashes are case-sensitive. The header search tokenizer
// folds case, so the twin leaves the indexed header tokens identical.
fn without_matches(message: &str, a: &str, b: &str) -> String {
    let a_id = message_id(a);
    let b_id = message_id(b);
    message
        .replace(a_id, &a_id.to_ascii_uppercase())
        .replace(b_id, &b_id.to_ascii_uppercase())
}

async fn delivered_to_setting(test: &mut TestServer, admin: &Account, enabled: bool) {
    admin
        .registry_update_setting(
            MtaStageData {
                add_delivered_to_header: enabled,
                ..Default::default()
            },
            &[Property::AddDeliveredToHeader],
        )
        .await;
    admin.reload_settings().await;
    test.reload_core();
    assert_eq!(test.server.core.smtp.session.data.add_delivered_to, enabled);
}

async fn newest(test: &TestServer, account: &Account) -> ThreadBlobEntry {
    search_manifest(test, account_id(account))
        .await
        .thread_blobs
        .pop()
        .unwrap()
}

async fn thread_wire(test: &TestServer, entry: &ThreadBlobEntry) -> (usize, usize) {
    let raw = test
        .server
        .get_blob(entry.hash.as_slice(), 0..usize::MAX)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw.len(), entry.size as usize);
    let (header, sections) = BlobHeader::decode(&raw).unwrap();
    assert_eq!(header.kind, BlobKind::Thread);
    assert_eq!(header.tier, 0);
    assert_eq!(header.segment, entry.segment);
    assert!(header.predecessor.is_none());
    let hidden = u32::from_le_bytes(sections[..4].try_into().unwrap()) as usize;
    (hidden, sections.len().checked_sub(4 + hidden).unwrap())
}

async fn groups(account: &Account) -> Vec<Vec<u32>> {
    let emails = account
        .jmap_get("Email", ["id", "threadId"], Vec::<String>::new())
        .await;
    assert_eq!(emails.name_at(0), "Email/get");
    let mut thread_ids = emails
        .list()
        .iter()
        .map(|email| email["threadId"].as_str().unwrap())
        .collect::<Vec<_>>();
    thread_ids.sort_unstable();
    thread_ids.dedup();
    let reply = account
        .jmap_get("Thread", ["id", "emailIds"], thread_ids)
        .await;
    assert_eq!(reply.name_at(0), "Thread/get");
    assert_eq!(reply.not_found().count(), 0);
    let mut groups = reply
        .list()
        .iter()
        .map(|group| {
            let mut docs = group["emailIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().parse::<Id>().unwrap().document_id())
                .collect::<Vec<_>>();
            docs.sort_unstable();
            docs
        })
        .collect::<Vec<_>>();
    groups.sort();
    groups
}

fn membership_mime(message_id: Option<&str>, references: &[&str]) -> String {
    let mut raw = String::new();
    if let Some(id) = message_id {
        raw.push_str(&format!("Message-ID: <{id}>\r\n"));
    }
    if !references.is_empty() {
        raw.push_str("References:");
        for reference in references {
            raw.push_str(&format!(" <{reference}>"));
        }
        raw.push_str("\r\n");
    }
    raw.push_str("\r\n");

    let message = MessageParser::new().parse(raw.as_bytes()).unwrap();
    let headers = message.headers();
    assert!(headers.has_known());
    assert_eq!(
        headers.len(),
        usize::from(message_id.is_some()) + usize::from(!references.is_empty())
    );
    assert_eq!(message.message_id(), message_id);
    let mut parsed_references = Vec::new();
    if let Some(header) = headers.get(HeaderName::References) {
        header
            .value()
            .visit_text(|id| parsed_references.push(id.to_owned()));
    }
    assert_eq!(parsed_references, references);
    let thread = ThreadFields::scan(&message);
    assert!(thread.subject.is_empty() && thread.sent_at.is_none());
    raw
}

async fn membership_terms(test: &TestServer, account: &Account, docs: &[u32]) -> Vec<Vec<u8>> {
    let id = account_id(account);
    // Map requests need not load content. Serve a real content query
    // after the publication checks and before taking the read guard.
    let before = search_manifest(test, id).await;
    za_index_current(account).await;
    let after = search_manifest(test, id).await;
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.newest_row, before.newest_row);
    assert_eq!(after.content, before.content);
    assert_eq!(after.thread_blobs, before.thread_blobs);

    let keys = za_unlock(test, id, STRONG).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    let view = entry.read_loaded().await.unwrap();
    let content = &view.content.as_ref().unwrap().part;
    docs.iter()
        .map(|&doc| {
            let bytes = content.record_bytes(doc).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), doc);
            // The record starts with doc ID, flags and sent time, followed
            // by the three empty sort keys and the length-prefixed terms.
            let (sort_lengths, bytes) = bytes[13..].split_at(3);
            assert_eq!(sort_lengths, [0; 3]);
            let (length, terms) = bytes.split_at(4);
            assert_eq!(u32::from_le_bytes(length.try_into().unwrap()) as usize, terms.len());
            let (&marker, encoded) = terms.split_last().unwrap();
            let raw_len = match marker {
                0 => encoded.len(),
                1 => decompress(encoded).unwrap().len(),
                _ => panic!("unexpected membership term frame marker {marker}"),
            };
            if raw_len < COMPRESS_WATERMARK_DICTIONARY {
                assert_eq!(marker, 0, "membership term frame below compression threshold");
            }
            println!("leak_search: membership document {doc} term frame marker {marker}, raw length {raw_len}");
            terms.to_vec()
        })
        .collect()
}

async fn membership(test: &mut TestServer, key: &Account, twin: &Account) {
    let seeds = [
        membership_mime(Some("a"), &[]),
        membership_mime(Some("b"), &["a"]),
        membership_mime(Some("c"), &["b"]),
        membership_mime(Some("d"), &[]),
        membership_mime(Some("e"), &["d"]),
        membership_mime(Some("f"), &["e"]),
    ];
    let x = membership_mime(None, &["c", "f"]);
    let x_twin = membership_mime(None, &["C", "F"]);
    assert_eq!(x.len(), x_twin.len());

    // LMTP's per-recipient Delivered-To metadata is itself indexed. Keep
    // this membership probe's independent content inputs equal.
    let admin = test.account("admin@example.com").clone();
    let saved = admin
        .registry_get::<MtaStageData>(Id::singleton())
        .await
        .add_delivered_to_header;
    delivered_to_setting(test, &admin, false).await;
    let mailboxes = [
        current_mailbox_ids(key).await,
        current_mailbox_ids(twin).await,
    ];
    let mut lmtp = SmtpConnection::connect().await;
    for message in &seeds {
        lmtp.mail_from(SENDER, 2).await;
        lmtp.rcpt_to(key.name(), 2).await;
        lmtp.rcpt_to(twin.name(), 2).await;
        lmtp.bdat_last(message, 2, 2).await;
        wait_for_delivery(test).await;
    }
    lmtp.quit().await;
    let mut sizes = Vec::new();
    let mut frames = Vec::new();

    for ((account, bridge, joins), mailboxes) in [(key, x, true), (twin, x_twin, false)]
        .into_iter()
        .zip(mailboxes)
    {
        let id = account_id(account);
        search_pass_request(account).await;
        let docs = message_ids(test, id).await;
        assert_eq!(docs.len(), 6);
        assert_eq!(
            groups(account).await,
            [docs[..3].to_vec(), docs[3..].to_vec()]
        );
        assert_eq!(
            za_search_thread_part(test, id, STRONG)
                .await
                .aliases()
                .count(),
            0
        );
        let before = email_log_tip(test, account).await;
        deliver(test, SENDER, account.name(), &bridge).await;
        let new_docs = message_ids(test, id).await;
        assert_eq!(new_docs.len(), 7);
        let added = new_docs
            .iter()
            .copied()
            .find(|doc| !docs.contains(doc))
            .unwrap();
        let since = email_log_tip(test, account).await;
        let inserts = log_records_since(test, id, SyncCollection::Email, before).await;
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].0, since);
        assert!(
            inserts[0]
                .1
                .iter()
                .any(|row| matches!(row, Change::InsertItem(item) if *item as u32 == added))
        );

        search_pass_request(account).await;
        assert_search_publication(test, account, since, added, &mailboxes).await;
        let descriptor = newest(test, account).await;
        assert_eq!(
            thread_wire(test, &descriptor).await,
            (PAD + SEAL_OVERHEAD, public_len(0, 1) + SEAL_OVERHEAD)
        );
        let mut links = principal_blob_links(test, id)
            .await
            .into_iter()
            .map(|(_, size)| size)
            .collect::<Vec<_>>();
        links.sort_unstable();
        sizes.push((descriptor.size, links));
        assert_eq!(
            za_search_thread_part(test, id, STRONG)
                .await
                .aliases()
                .count(),
            usize::from(joins)
        );
        let expected = if joins {
            vec![new_docs]
        } else {
            let mut groups = vec![docs[..3].to_vec(), docs[3..].to_vec(), vec![added]];
            groups.sort();
            groups
        };
        assert_eq!(groups(account).await, expected);
        let mut paired_order = docs;
        paired_order.push(added);
        frames.push(membership_terms(test, account, &paired_order).await);
    }
    delivered_to_setting(test, &admin, saved).await;
    assert!(
        frames[0] == frames[1],
        "membership twins must have identical encoded terms in seed/bridge order"
    );
    assert_eq!(sizes[0], sizes[1]);
}

async fn filler_references(test: &TestServer, key: &Account, twin: &Account) {
    let probe = message_with("F1 probe", "probe");
    let filler = message_with("F1 filler", "filler");
    let make_refs = |n: usize| {
        (0..n)
            .map(|i| format!("<f1-{i:04}@za.test>"))
            .collect::<Vec<_>>()
            .join("\r\n ")
    };
    let many = filler.replace(
        "Date: ",
        &format!("References: {}\r\nDate: ", make_refs(1000)),
    );
    let mut few = filler.replace("Date: ", &format!("References: {}\r\nDate: ", make_refs(2)));
    // Pad the raw twin to the exact same size without a long physical line.
    let padding = many.len() - few.len();
    few.push_str(&"a\r\n".repeat(padding / 3));
    few.push_str(&"a".repeat(padding % 3));
    assert_eq!(many.len(), few.len());

    let mut sizes = Vec::new();
    for (account, filler) in [(key, many), (twin, few)] {
        deliver(test, SENDER, account.name(), &probe).await;
        let before = message_ids(test, account_id(account)).await;
        assert_eq!(before.len(), 1);
        deliver(test, SENDER, account.name(), &filler).await;
        let doc = super::mail::new_message(test, account_id(account), &before).await;
        search_pass_request(account).await;
        let descriptor = newest(test, account).await;
        assert_eq!(
            thread_wire(test, &descriptor).await,
            (PAD + SEAL_OVERHEAD, public_len(0, 1) + SEAL_OVERHEAD)
        );
        sizes.push(descriptor.size);
        if account.id() == key.id() {
            let part = za_search_thread_part(test, account_id(key), STRONG).await;
            assert_eq!(part.record(doc).unwrap().hashes.len(), K);
        }
    }
    assert_eq!(sizes[0], sizes[1]);
}

async fn junk_expunged(test: &mut TestServer, key: &Account) {
    let id = account_id(key);
    for group in 0..125 {
        let mut lmtp = SmtpConnection::connect().await;
        assert!(
            lmtp.lhlo()
                .await
                .iter()
                .any(|line| line.contains("CHUNKING"))
        );
        for _ in 0..8 {
            lmtp.mail_from(SENDER, 2).await;
            lmtp.rcpt_to(key.name(), 2).await;
            lmtp.bdat_last(&message_with("junk", "x"), 1, 2).await;
        }
        lmtp.quit().await;
        if group % 8 == 7 {
            wait_for_delivery(test).await;
        }
    }
    wait_for_delivery(test).await;
    let docs = message_ids(test, id).await;
    assert_eq!(docs.len(), 1000);
    deliver(
        test,
        SENDER,
        key.name(),
        &message_with("G1 probe", "survives"),
    )
    .await;
    let probe = super::mail::new_message(test, id, &docs).await;
    finish_bulk_delivery_indexing(test, key).await;
    za_index_current(key).await;
    let mut destroy = Vec::new();
    for position in [0, 400, 800] {
        let ids = key
            .jmap_method_call(
                "Email/query",
                json!({
                    "accountId": key.id_string(), "position": position, "limit": 400,
                    "calculateTotal": true
                }),
            )
            .await;
        assert_eq!(ids.name_at(0), "Email/query");
        assert_eq!(ids.method_response()["total"], 1001);
        destroy.extend(ids.object_ids().filter(|id| id.document_id() != probe));
    }
    destroy.sort_unstable_by_key(|id| id.document_id());
    assert_eq!(
        destroy
            .iter()
            .map(|id| id.document_id())
            .collect::<Vec<_>>(),
        docs
    );

    let admin = test.account("admin@example.com").clone();
    let saved = admin
        .registry_get::<Jmap>(Id::singleton())
        .await
        .max_set_objects;
    let limit = 1000.max(saved);
    admin
        .registry_update_setting(
            Jmap {
                max_set_objects: limit,
                ..Default::default()
            },
            &[Property::MaxSetObjects],
        )
        .await;
    admin.reload_settings().await;
    test.reload_core();
    assert_eq!(test.server.core.jmap.set_max_objects, limit as usize);
    let reply = key
        .jmap_destroy("Email", destroy.iter(), Vec::<(&str, &str)>::new())
        .await;
    admin
        .registry_update_setting(
            Jmap {
                max_set_objects: saved,
                ..Default::default()
            },
            &[Property::MaxSetObjects],
        )
        .await;
    admin.reload_settings().await;
    test.reload_core();
    assert_eq!(test.server.core.jmap.set_max_objects, saved as usize);
    assert_eq!(reply.name_at(0), "Email/set");
    let mut got = reply.destroyed_ids().collect::<Vec<_>>();
    got.sort_unstable_by_key(|id| id.document_id());
    assert_eq!(got, destroy);
    assert!(
        reply.method_response()["notDestroyed"]
            .as_object()
            .is_none_or(|v| v.is_empty())
    );
    assert_eq!(message_ids(test, id).await, [probe]);

    let before = search_manifest(test, id).await;
    search_pass_request(key).await;
    let after = search_manifest(test, id).await;
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.newest_row, before.newest_row);
    assert_eq!(after.thread_blobs.len(), before.thread_blobs.len() + 1);
    let newest = after.thread_blobs.last().unwrap();
    assert_eq!(
        thread_wire(test, newest).await,
        (0, public_len(1000, 0) + SEAL_OVERHEAD)
    );
    assert_eq!(
        newest.size as usize,
        BlobHeader::LEN + 4 + public_len(1000, 0) + SEAL_OVERHEAD
    );
}

async fn cohorts(test: &TestServer, key: &Account) {
    let a = message_with(SUBJECT, "M1");
    let b = message_with(SUBJECT, "M2");
    let x = bridge(&a, &b, true);
    let mut a = a;
    assert!(a.len() <= x.len());
    a.push_str(&"x".repeat(x.len() - a.len()));
    assert_eq!(a.len(), x.len());
    deliver(test, SENDER, key.name(), &a).await;
    search_pass_request(key).await;
    let first = newest(test, key).await;
    assert_eq!(
        thread_wire(test, &first).await,
        (PAD + SEAL_OVERHEAD, public_len(0, 1) + SEAL_OVERHEAD)
    );
    deliver(test, SENDER, key.name(), &b).await;
    search_pass_request(key).await;
    for root in [&a, &b] {
        for _ in 0..20 {
            deliver(test, SENDER, key.name(), &reply_to(root, SUBJECT, "helper")).await;
        }
        search_pass_request(key).await;
    }
    let mut before_groups = groups(key).await;
    assert_eq!(before_groups.len(), 2);
    assert!(before_groups.iter().all(|group| group.len() == 21));
    let id = account_id(key);
    let aliases = za_search_thread_part(test, id, STRONG)
        .await
        .aliases()
        .count();
    deliver(test, SENDER, key.name(), &x).await;
    search_pass_request(key).await;
    let next = newest(test, key).await;
    assert_eq!(
        thread_wire(test, &next).await,
        (PAD + SEAL_OVERHEAD, public_len(0, 1) + SEAL_OVERHEAD)
    );
    assert_eq!(next.size, first.size);
    assert_eq!(
        za_search_thread_part(test, id, STRONG)
            .await
            .aliases()
            .count(),
        aliases + 1
    );
    let mut joined = before_groups.pop().unwrap();
    joined.extend(before_groups.pop().unwrap());
    joined.sort_unstable();
    joined.push(super::mail::new_message(test, id, &joined).await);
    joined.sort_unstable();
    assert_eq!(groups(key).await, [joined]);
}

async fn compact_class(test: &TestServer, key: &Account, joins: bool) -> usize {
    let a = message_with(SUBJECT, "compact a");
    let b = message_with(SUBJECT, "compact b");
    let id = account_id(key);
    for group in 0..4 {
        for member in 0..10 {
            let raw = match (group, member) {
                (0, 0) => a.clone(),
                (0, 1) => b.clone(),
                (3, 0) => bridge(&a, &b, joins),
                _ => message_with("compaction filler", "filler"),
            };
            deliver(test, SENDER, key.name(), &raw).await;
        }
        search_pass_request(key).await;
        let manifest = search_manifest(test, id).await;
        assert_eq!(
            manifest.thread_blobs.len(),
            if group == 3 { 1 } else { group + 1 }
        );
        assert_eq!(message_ids(test, id).await.len(), (group + 1) * 10);
    }
    let part = za_search_thread_part(test, id, STRONG).await;
    assert_eq!(part.records().count(), 40);
    assert!(part.records().all(|record| record.deleted.is_none()));
    let manifest = search_manifest(test, id).await;
    let [descriptor] = manifest.thread_blobs.as_slice() else {
        panic!("the compacted thread manifest must hold exactly one blob");
    };
    // The loaded view also contains the manifest's separately stored row.
    let sealed_rows = part
        .rows()
        .iter()
        .filter(|row| **row != manifest.newest_row)
        .count();
    assert_eq!(
        part.rows().len() - sealed_rows,
        1,
        "the manifest's newest row must appear exactly once in the loaded view"
    );
    let (_, public) = thread_wire(test, descriptor).await;
    assert_eq!(public, public_len(0, sealed_rows) + SEAL_OVERHEAD);
    compaction_class(test, id).await
}

pub async fn compaction_class(test: &TestServer, account_id: u32) -> usize {
    let manifest = search_manifest(test, account_id).await;
    let [descriptor] = manifest.thread_blobs.as_slice() else {
        panic!("the compacted thread manifest must hold exactly one blob");
    };
    let (hidden, _) = thread_wire(test, descriptor).await;
    hidden.checked_sub(SEAL_OVERHEAD).unwrap()
}
