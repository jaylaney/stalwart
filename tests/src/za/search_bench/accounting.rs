/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::za::mail::{Traced, TracedEvent};
use common::za::{
    search::trace::{ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_PASS},
    test_hooks::{SearchContentWrite, set_search_content_writes, take_search_content_writes},
};
use trc::{Key, Value};

#[derive(Debug, Default)]
pub(super) struct ContentAttempt {
    pub original: u64,
    pub encoded: Vec<(u8, u64)>,
    pub uploaded: Option<(u8, u64)>,
    pub linked: Option<(u8, u64)>,
}

impl ContentAttempt {
    pub fn rewritten(&self) -> u64 {
        self.encoded
            .iter()
            .filter_map(|&(tier, bytes)| (tier != 0).then_some(bytes))
            .sum()
    }
}

#[derive(Debug)]
pub(super) struct Commit {
    pub chain_length: u32,
    pub full_merge: bool,
    pub thread_compaction: bool,
    pub content: Option<ContentAttempt>,
}

#[derive(Default)]
pub(super) struct Accounting {
    pending: Option<ContentAttempt>,
    pub commits: Vec<Commit>,
    pub aborted: Vec<ContentAttempt>,
}

impl Accounting {
    pub fn ingest(&mut self, events: impl IntoIterator<Item = SearchContentWrite>) {
        for event in events {
            match event {
                SearchContentWrite::Original(original) => {
                    self.discard_uncommitted();
                    self.pending = Some(ContentAttempt {
                        original,
                        ..Default::default()
                    });
                }
                SearchContentWrite::Encoded { tier, bytes } => {
                    let attempt = self.pending.as_mut().expect("encoding without Original");
                    assert!(attempt.uploaded.is_none());
                    attempt.encoded.push((tier, bytes));
                }
                SearchContentWrite::Uploaded { tier, bytes } => {
                    let attempt = self.pending.as_mut().expect("upload without Original");
                    assert_eq!(attempt.encoded.last(), Some(&(tier, bytes)));
                    assert!(attempt.uploaded.replace((tier, bytes)).is_none());
                }
                SearchContentWrite::Linked { tier, bytes } => {
                    let attempt = self.pending.as_mut().expect("link without Original");
                    assert_eq!(attempt.uploaded, Some((tier, bytes)));
                    assert!(attempt.linked.replace((tier, bytes)).is_none());
                }
                SearchContentWrite::Committed {
                    chain_length,
                    full_merge,
                    thread_compaction,
                } => {
                    let content = if self
                        .pending
                        .as_ref()
                        .is_some_and(|attempt| attempt.linked.is_some())
                    {
                        self.pending.take()
                    } else {
                        self.discard_uncommitted();
                        None
                    };
                    assert!(!full_merge || content.as_ref().is_some_and(|c| c.rewritten() > 0));
                    self.commits.push(Commit {
                        chain_length,
                        full_merge,
                        thread_compaction,
                        content,
                    });
                }
                SearchContentWrite::StagedMemory(_) | SearchContentWrite::RetirementMemory(_) => {}
            }
        }
    }

    pub fn discard_uncommitted(&mut self) {
        if let Some(attempt) = self.pending.take() {
            assert!(attempt.linked.is_none(), "missing metadata after Linked");
            self.aborted.push(attempt);
        }
    }

    pub fn validate_traces(&self, account_id: u32, events: &[TracedEvent]) {
        // Different listener tasks can deliver trace batches in a different
        // order. Require the full multiset, including repeated/no-content rows.
        let value = |event: &TracedEvent, key| {
            event
                .keys
                .iter()
                .find_map(|(k, v)| match v {
                    Value::UInt(n) if *k == key => Some(*n),
                    _ => None,
                })
                .expect("numeric pass trace")
        };
        let mut traced = events
            .iter()
            .filter(|event| {
                event.account_id == Some(u64::from(account_id)) && event.details == ZA_SEARCH_PASS
            })
            .map(|event| (value(event, Key::Size), value(event, Key::Used)))
            .collect::<Vec<_>>();
        let mut observed = self
            .commits
            .iter()
            .map(|commit| {
                let written = commit
                    .content
                    .as_ref()
                    .and_then(|c| c.uploaded)
                    .map_or(0, |(_, b)| b);
                let work = commit.content.as_ref().map_or(0, ContentAttempt::rewritten);
                (written, work)
            })
            .collect::<Vec<_>>();
        traced.sort_unstable();
        observed.sort_unstable();
        assert_eq!(
            observed, traced,
            "successful writer commits versus pass traces"
        );
    }
}

struct Observer(u32);

impl Drop for Observer {
    fn drop(&mut self) {
        set_search_content_writes(None);
    }
}

pub(super) struct Capture {
    observer: Observer,
    trace: Traced,
    pub accounting: Accounting,
}

impl Capture {
    pub fn start(id: u32) -> Self {
        let trace = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
        set_search_content_writes(Some(id));
        Self {
            observer: Observer(id),
            trace,
            accounting: Accounting::default(),
        }
    }

    pub fn drain(&mut self) -> &[Commit] {
        let offset = self.accounting.commits.len();
        self.accounting
            .ingest(take_search_content_writes(self.observer.0));
        &self.accounting.commits[offset..]
    }

    pub async fn finish(mut self) -> (Accounting, Vec<TracedEvent>) {
        self.drain();
        self.accounting.discard_uncommitted();
        let events = self.trace.finish().await;
        self.accounting.validate_traces(self.observer.0, &events);
        (self.accounting, events)
    }
}

#[test]
fn split_observer_drains() {
    use SearchContentWrite::*;

    let mut accounting = Accounting::default();
    // A stale upload and a subsequent witness/checkpoint-only write are
    // adjacent in the account stream. The latter must not promote the former.
    accounting.ingest([
        Original(500),
        Encoded {
            tier: 0,
            bytes: 500,
        },
        Uploaded {
            tier: 0,
            bytes: 500,
        },
    ]);
    accounting.ingest([Committed {
        chain_length: 4,
        full_merge: false,
        thread_compaction: false,
    }]);
    assert!(accounting.commits[0].content.is_none());
    assert_eq!(accounting.aborted[0].original, 500);

    accounting.ingest([Original(19), Encoded { tier: 1, bytes: 61 }]);
    accounting.ingest([
        Encoded {
            tier: 2,
            bytes: 173,
        },
        Uploaded {
            tier: 2,
            bytes: 173,
        },
        Linked {
            tier: 2,
            bytes: 173,
        },
    ]);
    assert_eq!(accounting.commits.len(), 1);
    accounting.ingest([Committed {
        chain_length: 1,
        full_merge: false,
        thread_compaction: true,
    }]);
    let commit = &accounting.commits[1];
    let content = commit.content.as_ref().unwrap();
    assert_eq!(content.original, 19);
    assert_eq!(content.rewritten(), 234);
    assert_eq!(content.linked, Some((2, 173)));
}
