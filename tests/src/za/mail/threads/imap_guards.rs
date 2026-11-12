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

    metadata_without_map(test, &key, root_id, reply_id).await;
    metadata_saved_search(test, &key).await;
    for kind in [OutputKind::Rows, OutputKind::Empty, OutputKind::Vanished] {
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
) -> (Arc<SessionData<T>>, Arc<SelectedMailbox>) {
    let account_id = key.id().document_id();
    let keys = za_unlock(test, account_id, key.secret()).await;
    let cache = test.server.get_cached_messages(account_id).await.unwrap();
    let (_, stream_tx) = tokio::io::split(stream);
    let data = Arc::new(SessionData {
        account_id,
        access_token: AccessToken::from_permissions(account_id, []).with_session_keys(keys),
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

async fn metadata_without_map(test: &TestServer, key: &Account, root: u32, reply: u32) {
    let (data, mailbox) = session(test, key, NullIo::default()).await;
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
        // The following identity request must still encounter the unavailable map.
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
            .expect_err("identity SEARCH must refuse the unavailable map");
        assert_eq!(
            error.value_as_str(trc::Key::Code),
            Some("UNAVAILABLE"),
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
            mailbox,
            true,
            Instant::now(),
        )
        .await
        .expect_err("THREAD grouping must refuse the unavailable map");
    assert_eq!(
        error.value_as_str(trc::Key::Code),
        Some("UNAVAILABLE"),
        "{error:?}"
    );
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
    let (data, mailbox) = session(test, key, PausedOutput(output.0.clone())).await;
    let rows = mailbox.view.lock().live_rows_by_uid();
    let reply_uid = rows.iter().find(|row| row.id == reply).unwrap().uid;
    let changed_since = match kind {
        OutputKind::Rows => None,
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
                sequence_set: Sequence::number(reply_uid),
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
            100,
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
    tokio::time::timeout(DEADLINE, fetch)
        .await
        .unwrap()
        .unwrap();
    let bytes = output.0.bytes.lock().unwrap();
    let response = String::from_utf8_lossy(&bytes);
    match kind {
        OutputKind::Rows => assert!(
            response.contains(&format!(
                "EMAILID {} THREADID {}",
                mapped(root, reply),
                Id::from(root)
            )),
            "{response}"
        ),
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
