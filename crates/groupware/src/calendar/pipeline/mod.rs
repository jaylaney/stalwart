/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod admission;
pub mod context;
pub mod create;
pub mod error;
pub mod placement;
pub mod schedule;
pub mod uid;

use calcard::icalendar::ICalendar;

/// Whether a write schedules, and whether a denied status fails it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaScheduling {
    /// No status is resolved, nothing is built, statuses stay untouched.
    /// CalDAV: a DELETE with `Schedule-Reply: F`. JMAP: `sendSchedulingMessages` false or absent.
    Off,
    /// Resolve; a denied status is traced and the write saved.
    /// CalDAV; JMAP `Calendar/set` destroy (D16).
    On,
    /// Resolve; a denied status returns `SchedulingDenied` before any write.
    /// JMAP with `sendSchedulingMessages: true`.
    Required,
}

/// What a committed event write returns.
pub struct ZaWritten {
    pub document_id: u32,
    /// `AssignedIds::last_change_id(owner, SyncCollection::Calendar)` of the commit
    /// that stored the event.
    pub change_id: u64,
    pub etag: String,
    pub schedule_tag: Option<u32>,
    /// The plaintext tree as committed: after the fan-out's statuses and sequences,
    /// before sealing.
    pub ical: ICalendar,
}

pub struct ZaDeleted {
    pub change_id: u64,
}
