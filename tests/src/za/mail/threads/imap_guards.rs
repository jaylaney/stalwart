/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{deliver_doc, key_mail_account, mapped, message_with, reply_to};
use crate::{
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        server::TestServer,
        za::{HookParked, pop3_delete, run_search_pass, za_unlock},
    },
    za::{STRONG, user_permissions},
};
use common::{
    auth::AccessToken,
    network::{SessionStream, stream::NullIo},
    za::test_hooks::{set_search_manifest_fail_once, set_search_swap_pause},
};
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    mailbox::INBOX_ID,
    message::za::search::{
        pass::{PassOutcome, ZaSearchPass},
        snapshot::ZaSnapshotFetch,
    },
};
use futures::task::AtomicWaker;
use imap::core::{MailboxId, MailboxView, SavedSearch, SelectedMailbox, SessionData};
use imap_proto::{
    ResponseType,
    protocol::{Sequence, fetch, search, thread},
};
use std::{
    borrow::Cow,
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::Notify,
};
use types::id::Id;

const DEADLINE: Duration = Duration::from_secs(4);

pub(super) async fn test(test: &mut TestServer) {
    println!("Running zero-access IMAP guard lifetime regressions...");
    let key = key_mail_account(test, "keymail-pr84-imap@example.com", STRONG).await;
    let root = message_with("PR84 IMAP guarded output", "root");
    let root_id = deliver_doc(test, &key, &root).await;
    let reply_id = deliver_doc(
        test,
        &key,
        &reply_to(&root, "PR84 IMAP guarded output", "reply"),
    )
    .await;
    run_search_pass(test, key.id().document_id(), key.secret()).await;

    for with_keys in [true, false] {
        metadata_without_map(test, &key, root_id, reply_id, with_keys).await;
    }
    metadata_saved_search(test, &key).await;
    for kind in [
        OutputKind::Rows,
        OutputKind::Saved,
        OutputKind::Limited,
        OutputKind::Changed,
        OutputKind::Empty,
        OutputKind::Vanished,
    ] {
        fetch_releases_map_before_output(test, &key, root_id, reply_id, kind).await;
    }
    test.destroy_all_mailboxes(&key).await;
}

async fn metadata_saved_search(test: &TestServer, key: &Account) {
    let plain = test
        .account("admin@example.com")
        .create_user_account(
            "plainmail-pr84-save@example.com",
            STRONG,
            "Plain saved search",
            &[],
            user_permissions(),
        )
        .await;
    for body in ["first", "second"] {
        deliver_doc(test, &plain, &message_with("PR84 saved search", body)).await;
    }
    for account in [&plain, key] {
        let mut imap = ImapConnection::connect(b"_pr84_save ").await;
        imap.authenticate(account.name(), account.secret()).await;
        imap.send_ok("SELECT INBOX").await;
        // A composite SAVE must reject a missing prior result, rather than
        // waiting for the channel that this same command just registered.
        imap.send("UID SEARCH RETURN (SAVE) UID $,1").await;
        imap.assert_read(Type::Tagged, ResponseType::No)
            .await
            .assert_contains("No saved search found.");
        imap.send_ok("UID SEARCH RETURN (SAVE) UID 1").await;
        imap.send_ok("UID SEARCH RETURN (SAVE) UID $,2").await;
        imap.send("UID SEARCH RETURN (COUNT) $").await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_contains("COUNT 2");
        imap.send_ok("LOGOUT").await;
    }
    test.destroy_all_mailboxes(&plain).await;
}

async fn session<T: SessionStream>(
    test: &TestServer,
    key: &Account,
    stream: T,
    with_keys: bool,
) -> (Arc<SessionData<T>>, Arc<SelectedMailbox>) {
    let account_id = key.id().document_id();
    let access_token = AccessToken::from_permissions(account_id, []);
    let access_token = if with_keys {
        access_token.with_session_keys(za_unlock(test, account_id, key.secret()).await)
    } else {
        access_token
    };
    let cache = test.server.get_cached_messages(account_id).await.unwrap();
    let (_, stream_tx) = tokio::io::split(stream);
    let data = Arc::new(SessionData {
        account_id,
        access_token,
        server: test.server.clone(),
        session_id: 0,
        mailboxes: Default::default(),
        stream_tx: Arc::new(tokio::sync::Mutex::new(stream_tx)),
        state: Default::default(),
        remote_addr: "127.0.0.1".parse().unwrap(),
        in_flight: None,
    });
    let mailbox = Arc::new(SelectedMailbox {
        id: MailboxId {
            account_id,
            mailbox_id: INBOX_ID,
        },
        view: MailboxView::build(&cache, INBOX_ID).into(),
        saved_search: SavedSearch::None.into(),
        is_select: true,
        is_condstore: false,
    });
    (data, mailbox)
}

async fn metadata_without_map(
    test: &TestServer,
    key: &Account,
    root: u32,
    reply: u32,
    with_keys: bool,
) {
    // An owner's connection opened before provisioning can remain keyless.
    // Its metadata commands keep working, but it must not expose singleton
    // identities or content when the validated map cannot be read.
    let (data, mailbox) = session(test, key, NullIo::default(), with_keys).await;
    let identity_refusal = if with_keys { "UNAVAILABLE" } else { "NOPERM" };
    for (sort, identity) in [
        (
            None,
            vec![search::Filter::EmailId(mapped(root, reply).into())],
        ),
        (
            Some(vec![search::Comparator {
                sort: search::Sort::Arrival,
                ascending: true,
            }]),
            vec![search::Filter::ThreadId(Id::from(root).to_string().into())],
        ),
        (
            Some(vec![search::Comparator {
                sort: search::Sort::Size,
                ascending: true,
            }]),
            vec![search::Filter::EmailId(mapped(root, reply).into())],
        ),
        (
            None,
            vec![
                search::Filter::Or,
                search::Filter::All,
                search::Filter::Not,
                search::Filter::EmailId(mapped(root, reply).into()),
                search::Filter::End,
                search::Filter::End,
            ],
        ),
    ] {
        // Removing metadata's no-map fast path must make this request fail.
        // Identity requests must still require keys and a validated map.
        let _failure = ManifestFailure::arm(key.id().document_id());
        let (response, _) = data
            .search(
                search::Arguments {
                    tag: "metadata".into(),
                    is_esearch: false,
                    sort,
                    result_options: vec![],
                    filter: vec![search::Filter::All],
                },
                mailbox.clone(),
                None,
                None,
                true,
                100,
                Instant::now(),
            )
            .await
            .expect("metadata SEARCH/SORT must not load the unavailable map");
        assert_eq!(response.ids, [1, 2]);
        let error = data
            .search(
                search::Arguments {
                    tag: "identity".into(),
                    is_esearch: false,
                    sort: None,
                    result_options: vec![],
                    filter: identity,
                },
                mailbox.clone(),
                None,
                None,
                true,
                100,
                Instant::now(),
            )
            .await
            .expect_err("identity SEARCH must require keys and an available map");
        assert_eq!(
            error.value_as_str(trc::Key::Code),
            Some(identity_refusal),
            "{error:?}"
        );
    }
    let _failure = ManifestFailure::arm(key.id().document_id());
    let error = data
        .thread(
            thread::Arguments {
                tag: "grouping".into(),
                algorithm: thread::Algorithm::References,
                filter: vec![search::Filter::All],
            },
            mailbox.clone(),
            true,
            Instant::now(),
        )
        .await
        .expect_err("THREAD grouping must require keys and an available map");
    assert_eq!(
        error.value_as_str(trc::Key::Code),
        Some(identity_refusal),
        "{error:?}"
    );
    if !with_keys {
        for (filter, sort) in [
            (vec![search::Filter::Body("root".into())], None),
            (
                vec![search::Filter::All],
                Some(vec![search::Comparator {
                    sort: search::Sort::Subject,
                    ascending: true,
                }]),
            ),
        ] {
            let error = data
                .search(
                    search::Arguments {
                        tag: "keyless-content".into(),
                        is_esearch: false,
                        sort,
                        result_options: vec![],
                        filter,
                    },
                    mailbox.clone(),
                    None,
                    None,
                    true,
                    100,
                    Instant::now(),
                )
                .await
                .expect_err("keyless content SEARCH/SORT must still refuse");
            assert_eq!(error.value_as_str(trc::Key::Code), Some("CANNOT"));
        }
        for attribute in [fetch::Attribute::ObjectId, fetch::Attribute::Body] {
            let error = data
                .fetch(
                    fetch::Arguments {
                        tag: "keyless-fetch".into(),
                        sequence_set: Sequence::number(1),
                        attributes: vec![attribute],
                        changed_since: None,
                        include_vanished: false,
                    },
                    mailbox.clone(),
                    None,
                    true,
                    false,
                    false,
                    false,
                    false,
                    100,
                    Instant::now(),
                )
                .await
                .expect_err("keyless FETCH must refuse identities and content");
            assert_eq!(error.value_as_str(trc::Key::Code), Some("NOPERM"));
        }
    }
}

struct ManifestFailure;

impl ManifestFailure {
    fn arm(account_id: u32) -> Self {
        set_search_manifest_fail_once(Some(account_id));
        Self
    }
}

impl Drop for ManifestFailure {
    fn drop(&mut self) {
        set_search_manifest_fail_once(None);
    }
}

#[derive(Clone, Copy, Debug)]
enum OutputKind {
    Rows,
    Saved,
    Limited,
    Changed,
    Empty,
    Vanished,
}

async fn fetch_releases_map_before_output(
    test: &TestServer,
    key: &Account,
    root: u32,
    reply: u32,
    kind: OutputKind,
) {
    let account_id = key.id().document_id();
    let output = OutputPause::new();
    let (data, mailbox) = session(test, key, PausedOutput(output.0.clone()), true).await;
    let rows = mailbox.view.lock().live_rows_by_uid();
    let root_uid = rows.iter().find(|row| row.id == root).unwrap().uid;
    let reply_uid = rows.iter().find(|row| row.id == reply).unwrap().uid;
    assert_eq!((root_uid, reply_uid), (1, 2));
    let sequence_set = match kind {
        OutputKind::Saved => {
            // Populate $ through the real SEARCH SAVE path. It selects the
            // root alone, so reusing the default reply or all rows is visible.
            let (results_tx, _results_rx) = tokio::sync::watch::channel(Arc::new(Vec::new()));
            data.search(
                search::Arguments {
                    tag: "save-root".into(),
                    is_esearch: false,
                    sort: None,
                    result_options: vec![search::ResultOption::Save],
                    filter: vec![search::Filter::Sequence(Sequence::number(1), true)],
                },
                mailbox.clone(),
                Some(results_tx),
                None,
                true,
                100,
                Instant::now(),
            )
            .await
            .unwrap();
            Sequence::SavedSearch
        }
        OutputKind::Limited | OutputKind::Changed => Sequence::range(Some(1), Some(2)),
        _ => Sequence::number(reply_uid),
    };
    let changed_since = match kind {
        OutputKind::Rows | OutputKind::Saved | OutputKind::Limited => None,
        OutputKind::Changed => {
            let cache = test.server.get_cached_messages(account_id).await.unwrap();
            let root_change = cache.email_by_id(&root).unwrap().change_id();
            let reply_change = cache.email_by_id(&reply).unwrap().change_id();
            assert!(root_change < reply_change);
            // The two deliveries have distinct commits. Select both UIDs but
            // return only the later reply, not merely any nonempty result.
            Some(reply_change)
        }
        OutputKind::Empty => Some(u64::MAX),
        OutputKind::Vanished => {
            pop3_delete(test, key, reply).await;
            run_search_pass(test, account_id, key.secret()).await;
            Some(0)
        }
    };
    let fetch = tokio::spawn(async move {
        data.fetch(
            fetch::Arguments {
                tag: "paused".into(),
                sequence_set,
                attributes: vec![fetch::Attribute::ObjectId],
                changed_since,
                include_vanished: matches!(kind, OutputKind::Vanished),
            },
            mailbox,
            None,
            true,
            true,
            false,
            matches!(kind, OutputKind::Empty),
            false,
            if matches!(kind, OutputKind::Limited) {
                1
            } else {
                100
            },
            Instant::now(),
        )
        .await
        .unwrap()
    });
    tokio::time::timeout(DEADLINE, output.0.arrived.notified())
        .await
        .expect("FETCH never reached the blocked output write");
    assert!(!fetch.is_finished());

    let added = deliver_doc(
        test,
        key,
        &message_with(&format!("PR84 publication {kind:?}"), "new row"),
    )
    .await;
    let keys = za_unlock(test, account_id, key.secret()).await;
    let server = test.server.clone();
    let publisher_keys = keys.clone();
    let publisher = HookParked::start(account_id, set_search_swap_pause, async move {
        server
            .za_search_pass(account_id, &publisher_keys)
            .await
            .unwrap()
    })
    .await;
    // The real publication is ready to acquire index.write(). It must finish
    // while FETCH is still stopped inside AsyncWrite, including early outputs.
    let outcome = tokio::time::timeout(DEADLINE, publisher.finish())
        .await
        .expect("FETCH output retained the map guard and blocked publication");
    assert!(
        matches!(
            outcome,
            PassOutcome::Advanced {
                published: true,
                ..
            }
        ),
        "{outcome:?}"
    );
    let current = tokio::time::timeout(DEADLINE, test.server.za_snapshot(account_id, &keys))
        .await
        .expect("another validated read was blocked by paused FETCH output")
        .unwrap();
    assert!(current.cache.has_email_id(&added));
    assert!(current.index.thread.record(added).is_some());
    assert_eq!(current.map_id(added), Id::from_parts(added, added));
    drop(current);
    assert!(
        !fetch.is_finished(),
        "FETCH unexpectedly escaped its output gate"
    );

    output.release();
    let status = tokio::time::timeout(DEADLINE, fetch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.rtype, ResponseType::Ok, "{kind:?}: {status:?}");
    let status = status.into_bytes();
    let status = String::from_utf8_lossy(&status);
    let bytes = output.0.bytes.lock().unwrap();
    let response = String::from_utf8_lossy(&bytes);
    let returned = response
        .lines()
        .filter(|line| line.contains(" FETCH ("))
        .map(|line| {
            let field = |name: &str| {
                line.split_once(name)
                    .unwrap_or_else(|| panic!("missing {name} in {line}"))
                    .1
                    .split([' ', ')'])
                    .next()
                    .unwrap()
                    .to_string()
            };
            (field("UID "), field("EMAILID "), field("THREADID "))
        })
        .collect::<Vec<_>>();
    // Expected selection comes from the fixture, never the selection or map
    // helpers being tested. Equality also rejects extra or duplicate FETCH rows.
    let expected = match kind {
        OutputKind::Saved => vec![(
            "1".to_string(),
            mapped(root, root),
            Id::from(root).to_string(),
        )],
        OutputKind::Rows | OutputKind::Limited | OutputKind::Changed => {
            vec![(
                "2".to_string(),
                mapped(root, reply),
                Id::from(root).to_string(),
            )]
        }
        OutputKind::Empty | OutputKind::Vanished => vec![],
    };
    assert_eq!(returned, expected, "{kind:?}: {response}");
    match kind {
        OutputKind::Rows | OutputKind::Saved => {}
        OutputKind::Limited => assert!(status.contains("[MESSAGELIMIT 1 2]"), "{status}"),
        OutputKind::Changed => assert!(response.contains("MODSEQ ("), "{response}"),
        OutputKind::Empty => assert!(response.contains("HIGHESTMODSEQ"), "{response}"),
        OutputKind::Vanished => assert!(
            response.contains(&format!("VANISHED (EARLIER) {reply_uid}")),
            "{response}"
        ),
    }
}

#[derive(Default)]
struct OutputState {
    released: AtomicBool,
    arrived: Notify,
    waker: AtomicWaker,
    bytes: Mutex<Vec<u8>>,
}

struct OutputPause(Arc<OutputState>);

impl OutputPause {
    fn new() -> Self {
        Self(Arc::default())
    }
    fn release(&self) {
        self.0.released.store(true, Ordering::Release);
        self.0.waker.wake();
    }
}

impl Drop for OutputPause {
    fn drop(&mut self) {
        self.release();
    }
}

struct PausedOutput(Arc<OutputState>);

impl AsyncWrite for PausedOutput {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.waker.register(cx.waker());
        if !self.0.released.load(Ordering::Acquire) {
            self.0.arrived.notify_one();
            return Poll::Pending;
        }
        self.0.bytes.lock().unwrap().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for PausedOutput {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        unreachable!("the FETCH worker only writes")
    }
}

impl SessionStream for PausedOutput {
    fn is_tls(&self) -> bool {
        true
    }
    fn tls_version_and_cipher(&self) -> (Cow<'static, str>, Cow<'static, str>) {
        (Cow::Borrowed(""), Cow::Borrowed(""))
    }
}
