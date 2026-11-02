/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ArchivedCalendar, ArchivedCalendarEvent, Calendar, CalendarEvent, CalendarPreferences,
    alarm::CalendarAlarm,
};
use crate::{
    DavResourceName, DestroyArchive, RFC_3986, SizeWriter,
    cache::GroupwareCache,
    calendar::{
        ArchivedCalendarEventContent, ArchivedCalendarEventNotification, CalendarEventContent,
        CalendarEventNotification, CalendarEventNotificationContent, ChangedBy as EventChangedBy,
        EVENT_DRAFT, EVENT_HAS_ALARMS, EVENT_NOTIFICATION_IS_CHANGE, EVENT_NOTIFICATION_IS_DESTROY,
        EVENT_NOTIFICATION_IS_DIRECT, EVENT_NOTIFICATION_IS_DRAFT,
        alarm::{AlarmTarget, CalendarAlarmType},
        alerts::DefaultAlertsResolver,
        fanout::ZaDeleteMessage,
        notification::{
            AccountViewers, CalendarNotificationReap, CalendarNotificationViewers,
            NotificationAudience, has_viewers, may_have_viewers,
        },
        privacy::ICalendarPrivacy,
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarmUsers, EventAlarms},
        seal::{
            SealError, ZaEvent, ZaKeyDomain, archived_event_is_sealed, seal_error,
            unseal_event_in_domain,
        },
        sharing::resolve::{ZaCalendarError, za_owned_calendar_context},
    },
    scheduling::{ItipMessages, event_cancel::itip_cancel, recipient::RecipientPolicy},
};
use calcard::icalendar::ICalendar;
use common::{
    DavName, GroupwareResources, Server,
    auth::{AccessToken, AccountCache, AccountInfo, ChangedBy},
    storage::{
        batch::MAX_BATCH_DOCUMENTS,
        index::{GroupwareWrite, ObjectIndexBuilder, SplitCurrent, SplitUpdate},
        metadata::ContainerCleanup,
    },
};
use registry::{
    schema::enums::{StorageQuota, TaskType},
    schema::structs::{Task, TaskCalendarAlarmEmail, TaskCalendarAlarmNotification, TaskStatus},
    types::{ObjectImpl, datetime::UTCDateTime},
};
use std::sync::Arc;
use store::{
    IndexKey, IterateParams, U32_LEN, ValueKey,
    roaring::RoaringBitmap,
    write::{
        Archive, ArchiveBytes, BatchBuilder, Operation, PendingId, SetValue, Slot, TaskId,
        TaskQueueClass, ValueClass, ValueOp, key::DeserializeBigEndian, now,
        serialize::rkyv_deserialize,
    },
};
use trc::AddContext;
use types::{
    collection::Collection,
    field::{CalendarEventField, CalendarNotificationField, Field},
    id::Id,
};
use vault::session::SessionKeys;

pub trait ItipAutoExpunge: Sync + Send {
    fn itip_auto_expunge(
        &self,
        account_id: u32,
        hold_period: u64,
    ) -> impl Future<Output = trc::Result<()>> + Send;
}

impl ItipAutoExpunge for Server {
    async fn itip_auto_expunge(&self, account_id: u32, hold_period: u64) -> trc::Result<()> {
        let mut destroy_ids = RoaringBitmap::new();
        let expire_before = (now().saturating_sub(hold_period) as i64).to_be_bytes();
        self.store()
            .iterate(
                IterateParams::new(
                    IndexKey {
                        account_id,
                        collection: Collection::CalendarEventNotification.into(),
                        document_id: 0,
                        field: CalendarNotificationField::Created.into(),
                        key: &[][..],
                    },
                    IndexKey {
                        account_id,
                        collection: Collection::CalendarEventNotification.into(),
                        document_id: 0,
                        field: CalendarNotificationField::Created.into(),
                        key: &expire_before[..],
                    },
                )
                .no_values()
                .ascending(),
                |key, _| {
                    destroy_ids.insert(key.deserialize_be_u32(key.len() - U32_LEN)?);

                    Ok(true)
                },
            )
            .await
            .caused_by(trc::location!())?;

        if destroy_ids.is_empty() {
            return Ok(());
        }

        // Tombstone messages
        let mut batch = BatchBuilder::new();
        // Upgrade ruling 189: the trace counts the items expunged, not the
        // pending ones kept (ruling 174).
        let mut expunged = 0u64;
        let changed_by = self
            .account(account_id)
            .await
            .caused_by(trc::location!())?
            .changed_by();

        self.archives(
            account_id,
            Collection::CalendarEventNotification,
            Field::ARCHIVE,
            &destroy_ids,
            |document_id, event_| {
                let event = event_
                    .to_unarchived::<CalendarEventNotification>()
                    .caused_by(trc::location!())?;
                // Upgrade ruling 174: a sealed item the drain has not opened
                // waits for it (invitations spec 6.6), however old.
                if crate::calendar::seal::archived_item_is_pending(event.inner) {
                    return Ok(true);
                }
                DestroyArchive(event)
                    .delete(changed_by, account_id, document_id, &mut batch)
                    .caused_by(trc::location!())?;
                expunged += 1;
                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;

        if expunged == 0 {
            return Ok(());
        }
        trc::event!(
            Store(trc::StoreEvent::AutoExpunge),
            AccountId = account_id,
            Collection = Collection::CalendarEventNotification.as_str(),
            Total = expunged,
        );

        self.commit_batch(batch).await.caused_by(trc::location!())?;

        Ok(())
    }
}

impl CalendarEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn update_full(
        self,
        content: CalendarEventContent,
        changed_by: ChangedBy,
        event: Archive<&ArchivedCalendarEvent>,
        event_content: &ArchivedCalendarEventContent,
        account_id: u32,
        document_id: u32,
        parent_id: Option<Slot>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        self.update_full_at(
            content,
            changed_by,
            event,
            event_content,
            account_id,
            document_id,
            parent_id,
            now() as i64,
            batch,
        )
    }

    /// `update_full`, rebuilding a fenced event write without advancing its
    /// original timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn update_full_at(
        self,
        content: CalendarEventContent,
        changed_by: ChangedBy,
        event: Archive<&ArchivedCalendarEvent>,
        event_content: &ArchivedCalendarEventContent,
        account_id: u32,
        document_id: u32,
        parent_id: Option<Slot>,
        modified: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        let mut new_event = self;
        new_event.modified = modified;

        let update = SplitUpdate::full(event, event_content, new_event, content, ())?;
        let etag = update.etag();

        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEvent)
            .with_document(document_id)
            .custom(update.into_builder(changed_by, parent_id))?
            .commit_point();

        Ok(etag)
    }

    pub fn update_meta(
        self,
        changed_by: ChangedBy,
        event: Archive<&ArchivedCalendarEvent>,
        account_id: u32,
        document_id: u32,
        parent_id: Option<Slot>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        self.update_meta_at(
            changed_by,
            event,
            account_id,
            document_id,
            parent_id,
            now() as i64,
            batch,
        )
    }

    /// `update_meta`, rebuilding a fenced event write without advancing its
    /// original timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn update_meta_at(
        self,
        changed_by: ChangedBy,
        event: Archive<&ArchivedCalendarEvent>,
        account_id: u32,
        document_id: u32,
        parent_id: Option<Slot>,
        modified: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        let mut new_event = self;
        new_event.modified = modified;

        let update = SplitUpdate::meta_only(event, new_event);
        let etag = update.etag();

        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEvent)
            .with_document(document_id)
            .custom(update.into_builder(changed_by, parent_id))?
            .commit_point();

        Ok(etag)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        self,
        content: CalendarEventContent,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: impl Into<PendingId>,
        parent_id: Option<Slot>,
        next_alarms: EventAlarms,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        self.insert_at(
            content,
            changed_by,
            account_id,
            document_id,
            parent_id,
            next_alarms,
            now() as i64,
            batch,
        )
    }

    /// `insert`, rebuilding a fenced create with its initially assigned
    /// timestamps.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_at(
        self,
        content: CalendarEventContent,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: impl Into<PendingId>,
        parent_id: Option<Slot>,
        next_alarms: EventAlarms,
        timestamp: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<String> {
        let mut event = self;
        event.modified = timestamp;
        event.created = timestamp;

        let changes = GroupwareWrite::full(event, content, ())?;
        let etag = format!("\"{}\"", changes.meta().etag);

        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEvent)
            .with_pending_document(document_id.into())
            .custom(
                ObjectIndexBuilder::<(), _>::new()
                    .with_changes(changes)
                    .with_changed_by(changed_by)
                    .with_pending_id_opt(parent_id),
            )?;

        for next_alarm in next_alarms {
            next_alarm.write_task(batch)?;
        }
        batch.commit_point();

        Ok(etag)
    }
}

impl Calendar {
    pub fn insert(
        self,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: impl Into<PendingId>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<&mut BatchBuilder> {
        // Build address calendar
        let mut calendar = self;
        let now = now() as i64;
        calendar.modified = now;
        calendar.created = now;

        if calendar.preferences.is_empty() {
            calendar.preferences.push(CalendarPreferences {
                account_id,
                name: "default".to_string(),
                ..Default::default()
            });
        }

        // Prepare write batch
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Calendar)
            .with_pending_document(document_id.into())
            .custom(
                ObjectIndexBuilder::<(), _>::new()
                    .with_changes(calendar)
                    .with_changed_by(changed_by),
            )
            .map(|b| b.commit_point())
    }

    pub fn update<'x>(
        self,
        changed_by: ChangedBy,
        calendar: Archive<&ArchivedCalendar>,
        account_id: u32,
        document_id: u32,
        batch: &'x mut BatchBuilder,
    ) -> trc::Result<&'x mut BatchBuilder> {
        // Build address calendar
        let mut new_calendar = self;
        new_calendar.modified = now() as i64;

        // Prepare write batch
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Calendar)
            .with_document(document_id)
            .custom(
                ObjectIndexBuilder::new()
                    .with_current(calendar)
                    .with_changes(new_calendar)
                    .with_changed_by(changed_by),
            )
            .map(|b| b.commit_point())
    }
}

impl CalendarEventNotification {
    pub fn update_meta(
        self,
        changed_by: ChangedBy,
        notification: Archive<&ArchivedCalendarEventNotification>,
        account_id: u32,
        document_id: u32,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        let mut updated = self;
        updated.modified = now() as i64;

        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_document(document_id)
            .custom(SplitUpdate::meta_only(notification, updated).into_builder(changed_by, None))?
            .commit_point();

        Ok(())
    }

    pub fn insert(
        self,
        content: CalendarEventNotificationContent,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: impl Into<PendingId>,
        event_id: Option<Slot>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<&mut BatchBuilder> {
        // Build event
        let mut event = self;
        let now = now() as i64;
        event.modified = now;
        event.created = now;

        // Prepare write batch
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_pending_document(document_id.into())
            .custom(
                ObjectIndexBuilder::<(), _>::new()
                    .with_changes(GroupwareWrite::full(event, content, ())?)
                    .with_changed_by(changed_by)
                    .with_pending_id_opt(event_id),
            )
            .map(|batch| batch.commit_point())
    }
}

impl DestroyArchive<Archive<&ArchivedCalendar>> {
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_events(
        self,
        server: &Server,
        access_token: &AccessToken,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        children_ids: Vec<u32>,
        alarm_ids: &RoaringBitmap,
        cleanup: &ContainerCleanup,
        delete_path: Option<String>,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_with_events_za(
            server,
            access_token,
            account_info,
            account_id,
            document_id,
            children_ids,
            alarm_ids,
            cleanup,
            delete_path,
            send_itip,
            None,
            batch,
        )
        .await
        .map(|_| ())
    }

    /// `delete_with_events`. With `za_keys` (a key account, invitations
    /// spec 5.5) each event's write asserts the calendar's context, and its
    /// CANCEL is built from its unsealed tree and returned for the fan-out
    /// instead of queued.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_events_za(
        self,
        server: &Server,
        access_token: &AccessToken,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        mut children_ids: Vec<u32>,
        alarm_ids: &RoaringBitmap,
        cleanup: &ContainerCleanup,
        delete_path: Option<String>,
        send_itip: bool,
        za_keys: Option<&SessionKeys>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Vec<ZaDeleteMessage>> {
        // Process deletions
        let calendar_id = document_id;
        let context = match za_keys {
            // Upgrade ruling 176 (B-M1): the calendar is `account_id`'s, so
            // keys of another account (a grantee's) are refused, never used
            // to resolve a context in their own account.
            Some(keys) if keys.account_id != account_id => {
                return Err(ZaCalendarError::Forbidden.into_store_error(account_id, calendar_id));
            }
            Some(keys) => Some(
                za_owned_calendar_context(server, keys, calendar_id)
                    .await
                    .map_err(|error| error.into_store_error(account_id, calendar_id))?,
            ),
            None => None,
        };
        let mut za_messages = Vec::new();
        let cutoff = now() as i64;
        let mut resolver = DefaultAlertsResolver::default();
        let mut quota = NotificationQuota::default();
        server
            .reap_calendar_notifications(access_token, account_id, &[calendar_id], batch)
            .await
            .caused_by(trc::location!())?;
        let calendars = if !access_token.is_member(account_id) {
            server
                .calendars_if_any(access_token, account_id)
                .await
                .caused_by(trc::location!())?
        } else {
            None
        };
        children_ids.sort_unstable();
        children_ids.dedup();
        let content_for_all = send_itip || calendars.is_some();
        let mut content_ids = Vec::new();
        for chunk in children_ids.chunks(MAX_BATCH_DOCUMENTS) {
            content_ids.clear();
            content_ids.extend(
                chunk
                    .iter()
                    .copied()
                    .filter(|document_id| content_for_all || alarm_ids.contains(*document_id)),
            );
            let mut archives = server
                .archive_chunk(
                    account_id,
                    Collection::CalendarEvent,
                    chunk,
                    Some((CalendarEventField::Content.field(), &content_ids)),
                )
                .await
                .caused_by(trc::location!())?;
            let mut tasks = AlarmTasks::for_chunk(account_id, chunk, alarm_ids, &archives)?;

            for &document_id in chunk {
                let Some(event_) = archives.take_archive(document_id) else {
                    continue;
                };
                let event = event_
                    .to_unarchived::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                let event_flags = event.inner.flags.to_native();
                let mut content = archives.take_value(document_id);
                if let Some(calendars) = calendars.as_ref()
                    && let Some(previous) = server
                        .stored_event_if_notifiable(
                            access_token,
                            account_id,
                            calendars,
                            &[calendar_id],
                            event_flags,
                            (account_id, document_id),
                            &mut content,
                        )
                        .await
                        .caused_by(trc::location!())?
                {
                    content = previous.content;
                    server
                        .notify_direct_change(
                            access_token,
                            account_id,
                            DirectChange::Destroyed {
                                event_id: document_id,
                                previous: previous.event,
                                calendar_ids: vec![calendar_id],
                                event_flags,
                            },
                            Some(calendars),
                            &mut quota,
                            batch,
                        )
                        .await
                        .caused_by(trc::location!())?;
                }
                // Every commit chunk that holds an event's write asserts the
                // calendar's context (each write below ends in a commit point).
                if let Some(context) = &context {
                    context.assert_in(batch);
                }
                za_messages.extend(
                    DestroyArchive(event)
                        .delete_from_calendar_za(
                            server,
                            account_info,
                            account_id,
                            document_id,
                            calendar_id,
                            content,
                            &mut tasks,
                            None,
                            send_itip,
                            &mut resolver,
                            cleanup,
                            context.as_ref().map(|context| &context.domain),
                            cutoff,
                            batch,
                        )
                        .await?,
                );
            }
        }

        if let Some(context) = &context {
            context.assert_in(batch);
        }
        self.delete(
            server,
            account_info.changed_by(),
            account_id,
            document_id,
            delete_path,
            batch,
        )
        .await?;
        Ok(za_messages)
    }

    pub async fn delete(
        self,
        server: &Server,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: u32,
        delete_path: Option<String>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_with_cleanup(
            server,
            changed_by,
            account_id,
            document_id,
            delete_path,
            &ContainerCleanup::empty(Collection::Calendar),
            batch,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_cleanup(
        self,
        server: &Server,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: u32,
        delete_path: Option<String>,
        cleanup: &ContainerCleanup,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        let calendar = self.0;
        server
            .release_or_read(
                cleanup,
                batch,
                changed_by,
                account_id,
                document_id,
                calendar.inner.metadata_kinds(),
            )
            .await?;

        // Delete calendar
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Calendar)
            .with_document(document_id)
            .custom(
                ObjectIndexBuilder::<_, ()>::new()
                    .with_changed_by(changed_by)
                    .with_current(calendar),
            )
            .caused_by(trc::location!())?;
        if let Some(delete_path) = delete_path {
            batch.log_calendar_vanished(document_id, delete_path);
        }
        batch.commit_point();

        Ok(())
    }
}

impl DestroyArchive<Archive<&ArchivedCalendarEvent>> {
    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        delete_path: Option<String>,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_za_at(
            server,
            account_info,
            account_id,
            document_id,
            calendar_id,
            content,
            delete_path,
            send_itip,
            None,
            now() as i64,
            batch,
        )
        .await
        .map(|_| ())
    }

    /// `delete`. With `domain` (a key account's event, invitations spec
    /// 5.5), the CANCEL is built from the unsealed tree and returned for the
    /// fan-out instead of queued. `cutoff` is the initial DELETE's time, kept
    /// when a fenced mutation is rebuilt. The caller asserts the event's
    /// calendar context in the same commit chunk.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_za_at(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        delete_path: Option<String>,
        send_itip: bool,
        domain: Option<&ZaKeyDomain>,
        cutoff: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Option<ZaDeleteMessage>> {
        self.delete_from_calendar_za(
            server,
            account_info,
            account_id,
            document_id,
            calendar_id,
            content,
            &mut AlarmTasks::None,
            delete_path,
            send_itip,
            &mut DefaultAlertsResolver::default(),
            &ContainerCleanup::empty(Collection::CalendarEvent),
            domain,
            cutoff,
            batch,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_cleanup(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        tasks: &mut AlarmTasks,
        delete_path: Option<String>,
        cleanup: &ContainerCleanup,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_from_calendar(
            server,
            account_info,
            account_id,
            document_id,
            calendar_id,
            content,
            tasks,
            delete_path,
            false,
            &mut DefaultAlertsResolver::default(),
            cleanup,
            batch,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn delete_from_calendar(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        tasks: &mut AlarmTasks,
        delete_path: Option<String>,
        send_itip: bool,
        resolver: &mut DefaultAlertsResolver,
        cleanup: &ContainerCleanup,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_from_calendar_za(
            server,
            account_info,
            account_id,
            document_id,
            calendar_id,
            content,
            tasks,
            delete_path,
            send_itip,
            resolver,
            cleanup,
            None,
            now() as i64,
            batch,
        )
        .await
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    async fn delete_from_calendar_za(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        tasks: &mut AlarmTasks,
        delete_path: Option<String>,
        send_itip: bool,
        resolver: &mut DefaultAlertsResolver,
        cleanup: &ContainerCleanup,
        domain: Option<&ZaKeyDomain>,
        cutoff: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Option<ZaDeleteMessage>> {
        let mut za_message = None;
        if let Some(delete_idx) = self
            .0
            .inner
            .names
            .iter()
            .position(|name| name.parent_id == calendar_id)
        {
            if self.0.inner.names.len() > 1 {
                // Unlink calendar id from event
                let event = self.0;
                let mut new_event = event
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                new_event.names.swap_remove(delete_idx);
                server
                    .reschedule_event_alarms(
                        account_id,
                        document_id,
                        event.inner,
                        content,
                        &new_event
                            .names
                            .iter()
                            .map(DavName::parent_id)
                            .collect::<Vec<_>>(),
                        tasks,
                        resolver,
                        batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
                let update = SplitUpdate::meta_only(event, new_event);
                batch
                    .with_account_id(account_id)
                    .with_collection(Collection::CalendarEvent)
                    .with_document(document_id)
                    .custom(update.into_builder(account_info.changed_by(), None))
                    .caused_by(trc::location!())?;
            } else {
                za_message = self
                    .remove_all_za_at(
                        server,
                        account_info,
                        account_id,
                        document_id,
                        content,
                        tasks,
                        send_itip,
                        cleanup,
                        domain,
                        cutoff,
                        batch,
                    )
                    .await?;
            }

            if let Some(delete_path) = delete_path {
                batch.log_calendar_vanished(calendar_id, delete_path);
            }

            batch.commit_point();
        }

        Ok(za_message)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete_all(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.remove_all(
            server,
            account_info,
            account_id,
            document_id,
            content,
            &mut AlarmTasks::None,
            send_itip,
            &ContainerCleanup::empty(Collection::CalendarEvent),
            batch,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn remove_all(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        tasks: &mut AlarmTasks,
        send_itip: bool,
        cleanup: &ContainerCleanup,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.remove_all_za_at(
            server,
            account_info,
            account_id,
            document_id,
            content,
            tasks,
            send_itip,
            cleanup,
            None,
            now() as i64,
            batch,
        )
        .await
        .map(|_| ())
    }

    /// `remove_all`. With `domain` (a key account, invitations spec 5.5) the
    /// stored tree is unsealed before `itip_cancel`, and the CANCEL (or an
    /// attendee's declined REPLY) is returned with the stored pairs for the
    /// fan-out instead of queued; whenever the scheduling gate holds, with no
    /// message when `itip_cancel` builds none (key pinning spec 5.1 item 2).
    /// `cutoff` replaces upstream's `now` so a rebuilt mutation keeps the
    /// initial gate.
    #[allow(clippy::too_many_arguments)]
    pub async fn remove_all_za_at(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        content: Option<Archive<ArchiveBytes>>,
        tasks: &mut AlarmTasks,
        send_itip: bool,
        cleanup: &ContainerCleanup,
        domain: Option<&ZaKeyDomain>,
        cutoff: i64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Option<ZaDeleteMessage>> {
        let event = self.0;
        let now = cutoff;
        let mut za_message = None;

        let has_alarms = event.inner.flags.to_native() & EVENT_HAS_ALARMS != 0;
        let send_itip =
            send_itip && event.inner.schedule_tag.is_some() && event.inner.event_range_end() > now;

        let content_ = match content {
            Some(content) => Some(content),
            None if send_itip || has_alarms => server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                    CalendarEventField::Content,
                ))
                .await
                .caused_by(trc::location!())?,
            None => None,
        };

        let content = content_
            .as_ref()
            .map(|content_| content_.to_unarchived::<CalendarEventContent>())
            .transpose()
            .caused_by(trc::location!())?;

        if has_alarms {
            let targets = match &content {
                Some(content) => EventAlarmUsers::targets(account_id, content.inner).collect(),
                None => vec![AlarmTarget::Owner],
            };
            server
                .clear_event_alarms(account_id, document_id, targets, tasks, batch)
                .await
                .caused_by(trc::location!())?;
        }
        server
            .release_or_read(
                cleanup,
                batch,
                account_info.changed_by(),
                account_id,
                document_id,
                event.inner.metadata_kinds(),
            )
            .await?;
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEvent)
            .with_document(document_id);

        if send_itip && let Some(content) = content {
            // Upgrade ruling 176 (B-M2): a sealed event's CANCEL is built in
            // its key domain only; without it the delete fails rather than
            // reach upstream's queueing.
            if domain.is_none() && archived_event_is_sealed(content.inner) {
                return Err(seal_error(
                    SealError::Structure("sealed event without its key domain"),
                    account_id,
                    document_id,
                ));
            }
            let content = content
                .deserialize::<CalendarEventContent>()
                .caused_by(trc::location!())?;
            let policy =
                RecipientPolicy::new(&server.core.groupware, event.inner.flags.to_native());

            if let Some(domain) = domain {
                let meta = event
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                let mut opened = ZaEvent::new(meta, content);
                let pairs = unseal_event_in_domain(&mut opened, domain)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                let tree = opened.content.data.event;
                let messages = itip_cancel(&tree, account_info.addresses(), true, policy).ok();
                let uid = tree.uids().next().unwrap_or_default().to_string();
                za_message = Some(ZaDeleteMessage {
                    messages,
                    pairs,
                    event: tree,
                    document_id,
                    uid,
                    policy,
                });
            } else if let Ok(messages) =
                itip_cancel(&content.data.event, account_info.addresses(), true, policy)
            {
                ItipMessages::new(messages)
                    .queue(batch)
                    .caused_by(trc::location!())?;
            }
        }

        batch
            .custom(
                ObjectIndexBuilder::<_, ()>::new()
                    .with_changed_by(account_info.changed_by())
                    .with_current(SplitCurrent::<ArchivedCalendarEvent>::MetaOnly(event)),
            )
            .caused_by(trc::location!())?;

        Ok(za_message)
    }
}

impl DestroyArchive<Archive<&ArchivedCalendarEventNotification>> {
    #[allow(clippy::too_many_arguments)]
    pub fn delete(
        self,
        changed_by: ChangedBy,
        account_id: u32,
        document_id: u32,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        // Delete event
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_document(document_id)
            .custom(
                ObjectIndexBuilder::<_, ()>::new()
                    .with_changed_by(changed_by)
                    .with_current(SplitCurrent::<ArchivedCalendarEventNotification>::MetaOnly(
                        self.0,
                    )),
            )
            .caused_by(trc::location!())?
            .commit_point();

        Ok(())
    }
}

impl CalendarAlarm {
    pub fn task(&self, account_id: u32) -> Task {
        match &self.typ {
            CalendarAlarmType::Email {
                event_start,
                event_start_tz,
                event_end,
                event_end_tz,
                recurrence_id,
            } => Task::CalendarAlarmEmail(TaskCalendarAlarmEmail {
                account_id: account_id.into(),
                document_id: Id::default(),
                alarm_id: self.alarm_id.to_task_id(),
                event_id: self.event_id.into(),
                event_end: UTCDateTime::from_timestamp(*event_end),
                event_end_tz: (*event_end_tz).into(),
                event_start: UTCDateTime::from_timestamp(*event_start),
                event_start_tz: (*event_start_tz).into(),
                target_account_id: self.target.sharee_id().map(Id::from),
                recurrence_id: *recurrence_id,
                status: TaskStatus::at(self.alarm_time),
            }),
            CalendarAlarmType::Display { recurrence_id } => {
                Task::CalendarAlarmNotification(TaskCalendarAlarmNotification {
                    account_id: account_id.into(),
                    document_id: Id::default(),
                    alarm_id: self.alarm_id.to_task_id(),
                    event_id: self.event_id.into(),
                    recurrence_id: *recurrence_id,
                    target_account_id: self.target.sharee_id().map(Id::from),
                    status: TaskStatus::at(self.alarm_time),
                })
            }
        }
    }

    pub fn build_write_ops(&self, account_id: u32, document_id: u32) -> [Operation; 2] {
        let mut task = self.task(account_id);
        task.set_document_id(Id::from(document_id));
        let id = TaskId::Assigned(self.target.task_id().resolve(account_id, document_id));
        [
            Operation::Value {
                class: ValueClass::TaskQueue(TaskQueueClass::Due {
                    id,
                    typ: task.object_type(),
                    due: self.alarm_time as u64,
                }),
                op: ValueOp::Set(SetValue::Fixed(Vec::new())),
            },
            Operation::Value {
                class: ValueClass::TaskQueue(TaskQueueClass::Task { id }),
                op: ValueOp::Set(SetValue::Fixed(task.to_pickled_vec())),
            },
        ]
    }

    pub fn write_task(&self, batch: &mut BatchBuilder) -> trc::Result<()> {
        let account_id = batch.last_account_id().ok_or_else(|| {
            trc::StoreEvent::UnexpectedError
                .caused_by(trc::location!())
                .ctx(trc::Key::Reason, "No account is set for an alarm task.")
        })?;
        batch.schedule_document_task(self.target.task_id(), self.task(account_id))?;
        Ok(())
    }

    pub fn task_type(&self) -> TaskType {
        match self.typ {
            CalendarAlarmType::Email { .. } => TaskType::CalendarAlarmEmail,
            CalendarAlarmType::Display { .. } => TaskType::CalendarAlarmNotification,
        }
    }
}

impl ArchivedCalendarEvent {
    pub async fn webcal_uri(
        &self,
        server: &Server,
        account_info: &AccountInfo,
    ) -> trc::Result<String> {
        for event_name in self.names.iter() {
            if let Some(calendar_) = server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                    account_info.account_id(),
                    Collection::Calendar,
                    event_name.parent_id.to_native(),
                ))
                .await
                .caused_by(trc::location!())?
            {
                let calendar = calendar_
                    .unarchive::<Calendar>()
                    .caused_by(trc::location!())?;
                return Ok(format!(
                    "webcal://{}{}/{}/{}/{}",
                    server.core.network.server_name,
                    DavResourceName::Cal.base_path(),
                    percent_encoding::utf8_percent_encode(account_info.name(), RFC_3986),
                    calendar.name,
                    event_name.name
                ));
            }
        }

        Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("Event is not linked to any calendar"))
    }
}

pub struct NotifiableEvent {
    pub event: ICalendar,
    pub content: Option<Archive<ArchiveBytes>>,
}

pub enum DirectChange {
    Created {
        event_id: PendingId,
        current: ICalendar,
        calendar_ids: Vec<u32>,
        event_flags: u16,
    },
    Updated {
        event_id: u32,
        previous: ICalendar,
        current: ICalendar,
        calendar_ids: Vec<u32>,
        event_flags: u16,
    },
    Destroyed {
        event_id: u32,
        previous: ICalendar,
        calendar_ids: Vec<u32>,
        event_flags: u16,
    },
}

fn draft_flag(event_flags: u16) -> u16 {
    if event_flags & EVENT_DRAFT != 0 {
        EVENT_NOTIFICATION_IS_DRAFT
    } else {
        0
    }
}

impl DirectChange {
    fn into_notification(
        self,
        changed_by: u32,
    ) -> (
        CalendarEventNotification,
        CalendarEventNotificationContent,
        Option<Slot>,
    ) {
        let (flags, event_id, pending_event_id, previous, current, calendar_ids, event_flags) =
            match self {
                DirectChange::Created {
                    event_id,
                    current,
                    calendar_ids,
                    event_flags,
                } => {
                    let (assigned_id, pending_id) = match event_id {
                        PendingId::Assigned(event_id) => (Some(event_id), None),
                        PendingId::Slot(slot) => (None, Some(slot)),
                    };

                    (
                        draft_flag(event_flags),
                        assigned_id,
                        pending_id,
                        None,
                        Some(current),
                        calendar_ids,
                        event_flags,
                    )
                }
                DirectChange::Updated {
                    event_id,
                    previous,
                    current,
                    calendar_ids,
                    event_flags,
                } => (
                    EVENT_NOTIFICATION_IS_CHANGE | draft_flag(event_flags),
                    Some(event_id),
                    None,
                    Some(previous),
                    Some(current),
                    calendar_ids,
                    event_flags,
                ),
                DirectChange::Destroyed {
                    event_id,
                    previous,
                    calendar_ids,
                    event_flags,
                } => (
                    EVENT_NOTIFICATION_IS_DESTROY,
                    Some(event_id),
                    None,
                    Some(previous),
                    None,
                    calendar_ids,
                    event_flags,
                ),
            };
        let flags = flags
            | NotificationAudience::of_event(event_flags)
                .with_privacy(
                    [previous.as_ref(), current.as_ref()]
                        .into_iter()
                        .flatten()
                        .all(|snapshot| snapshot.privacy().is_public()),
                )
                .flags();

        (
            CalendarEventNotification {
                event_id,
                changed_by: EventChangedBy::PrincipalId(changed_by),
                calendar_ids,
                flags: flags | EVENT_NOTIFICATION_IS_DIRECT,
                ..Default::default()
            },
            CalendarEventNotificationContent::Direct { previous, current },
            pending_event_id,
        )
    }
}

#[derive(Default)]
pub struct NotificationQuota {
    usage: Option<NotificationUsage>,
}

struct NotificationUsage {
    account: Arc<AccountCache>,
    limit: Option<usize>,
    stored: usize,
    created: usize,
    expired: usize,
    cursor: Option<NotificationCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationCursor {
    created: [u8; CREATED_LEN],
    document_id: u32,
}

const CREATED_LEN: usize = std::mem::size_of::<i64>();

impl NotificationQuota {
    pub(crate) async fn reserve(
        &mut self,
        server: &Server,
        account_id: u32,
        size: u64,
        batch: &mut BatchBuilder,
    ) -> trc::Result<bool> {
        let usage = match &mut self.usage {
            Some(usage) => usage,
            usage @ None => usage.insert(NotificationUsage::load(server, account_id).await?),
        };

        match server.has_available_quota(&usage.account, size).await {
            Ok(()) => {}
            Err(err) if err.matches(trc::EventType::Limit(trc::LimitEvent::Quota)) => {
                return Ok(false);
            }
            // SPDX-SnippetBegin
            // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
            // SPDX-License-Identifier: LicenseRef-SEL
            #[cfg(feature = "enterprise")]
            Err(err) if err.matches(trc::EventType::Limit(trc::LimitEvent::TenantQuota)) => {
                return Ok(false);
            }
            // SPDX-SnippetEnd
            Err(err) => return Err(err.caused_by(trc::location!())),
        }

        if let Some(limit) = usage.limit
            && usage.stored + usage.created >= limit + usage.expired
        {
            if limit == 0 {
                return Ok(false);
            }
            let Some(cursor) = server.oldest_notification(account_id, usage.cursor).await? else {
                return Ok(false);
            };
            let document_id = cursor.document_id;
            if let Some(notification) = server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                    account_id,
                    Collection::CalendarEventNotification,
                    document_id,
                ))
                .await
                .caused_by(trc::location!())?
            {
                DestroyArchive(
                    notification
                        .to_unarchived::<CalendarEventNotification>()
                        .caused_by(trc::location!())?,
                )
                .delete(usage.account.changed_by(), account_id, document_id, batch)
                .caused_by(trc::location!())?;
            }
            usage.expired += 1;
            usage.cursor = Some(cursor);
        }

        usage.created += 1;
        Ok(true)
    }
}

impl NotificationUsage {
    async fn load(server: &Server, account_id: u32) -> trc::Result<Self> {
        let account = server
            .account(account_id)
            .await
            .caused_by(trc::location!())?;
        let limit =
            server.object_quota_limit(&account, StorageQuota::MaxCalendarEventNotifications);
        let stored = match limit {
            Some(limit) => server
                .count_documents(account_id, Collection::CalendarEventNotification, limit)
                .await
                .caused_by(trc::location!())?,
            None => 0,
        };
        Ok(NotificationUsage {
            account,
            limit,
            stored,
            created: 0,
            expired: 0,
            cursor: None,
        })
    }
}

/// Upgrade ruling 187: the fan-out's eviction at the recipient's
/// `maxEventNotifications`, upstream's rolling window
/// (`NotificationQuota::reserve`) less the items the drain has not opened:
/// the oldest notification whose row has no pending bit, with its stored
/// row, or `None` when every stored notification is pending (the backlog
/// cap: the fan-out then answers `5.1`). The caller deletes it in the item's
/// batch, which asserts the row it read.
pub(crate) async fn za_oldest_drained_notification(
    server: &Server,
    account_id: u32,
) -> trc::Result<Option<(u32, Archive<ArchiveBytes>)>> {
    let mut cursor = None;
    while let Some(next) = server
        .oldest_notification(account_id, cursor)
        .await
        .caused_by(trc::location!())?
    {
        cursor = Some(next);
        if let Some(stored) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEventNotification,
                next.document_id,
            ))
            .await
            .caused_by(trc::location!())?
            && !crate::calendar::seal::archived_item_is_pending(
                stored
                    .unarchive::<CalendarEventNotification>()
                    .caused_by(trc::location!())?,
            )
        {
            return Ok(Some((next.document_id, stored)));
        }
    }
    Ok(None)
}

pub trait DirectChangeNotification: Sync + Send {
    fn notify_direct_change(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        change: DirectChange,
        calendars: Option<&GroupwareResources>,
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> impl Future<Output = trc::Result<()>> + Send;

    fn oldest_notification(
        &self,
        account_id: u32,
        after: Option<NotificationCursor>,
    ) -> impl Future<Output = trc::Result<Option<NotificationCursor>>> + Send;

    fn may_have_notification_viewers(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        notification: &CalendarEventNotification,
        calendars: Option<&GroupwareResources>,
    ) -> impl Future<Output = trc::Result<bool>> + Send;

    #[allow(clippy::too_many_arguments)]
    fn notify_calendar_removal(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        event_flags: u16,
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> impl Future<Output = trc::Result<()>> + Send;

    #[allow(clippy::too_many_arguments)]
    fn notify_calendar_addition(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        event_id: PendingId,
        calendar_id: u32,
        event_flags: u16,
        source: (u32, u32),
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> impl Future<Output = trc::Result<()>> + Send;

    #[allow(clippy::too_many_arguments)]
    fn stored_event_if_notifiable(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        calendars: &GroupwareResources,
        calendar_ids: &[u32],
        event_flags: u16,
        source: (u32, u32),
        prefetched: &mut Option<Archive<ArchiveBytes>>,
    ) -> impl Future<Output = trc::Result<Option<NotifiableEvent>>> + Send;
}

impl DirectChangeNotification for Server {
    async fn notify_direct_change(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        change: DirectChange,
        calendars: Option<&GroupwareResources>,
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        // Upgrade ruling 13: no direct-change snapshot of a key account's
        // event is stored; its owner's notification collection holds only
        // the fork's sealed inbox items.
        if self
            .za_is_key_account(account_id)
            .await
            .caused_by(trc::location!())?
        {
            return Ok(());
        }
        let (notification, content, pending_event_id) =
            change.into_notification(access_token.account_id());
        if !self
            .may_have_notification_viewers(access_token, account_id, &notification, calendars)
            .await
            .caused_by(trc::location!())?
        {
            return Ok(());
        }
        let size = content
            .snapshots()
            .into_iter()
            .flatten()
            .map(SizeWriter::ical)
            .sum::<usize>();
        if !quota
            .reserve(self, account_id, size as u64, batch)
            .await
            .caused_by(trc::location!())?
        {
            return Ok(());
        }

        let document_id =
            batch.reserve_document_id(account_id, Collection::CalendarEventNotification);
        notification
            .insert(
                content,
                access_token.changed_by(),
                account_id,
                document_id,
                pending_event_id,
                batch,
            )
            .caused_by(trc::location!())?;

        Ok(())
    }

    async fn notify_calendar_removal(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        event_flags: u16,
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        let Some(calendars) = self
            .calendars_if_any(access_token, account_id)
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(());
        };
        let Some(previous) = self
            .stored_event_if_notifiable(
                access_token,
                account_id,
                &calendars,
                &[calendar_id],
                event_flags,
                (account_id, document_id),
                &mut None,
            )
            .await
            .caused_by(trc::location!())?
            .map(|notifiable| notifiable.event)
        else {
            return Ok(());
        };

        self.notify_direct_change(
            access_token,
            account_id,
            DirectChange::Destroyed {
                event_id: document_id,
                previous,
                calendar_ids: vec![calendar_id],
                event_flags,
            },
            Some(&calendars),
            quota,
            batch,
        )
        .await
        .caused_by(trc::location!())
    }

    async fn notify_calendar_addition(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        event_id: PendingId,
        calendar_id: u32,
        event_flags: u16,
        source: (u32, u32),
        quota: &mut NotificationQuota,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        let Some(calendars) = self
            .calendars_if_any(access_token, account_id)
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(());
        };
        let Some(current) = self
            .stored_event_if_notifiable(
                access_token,
                account_id,
                &calendars,
                &[calendar_id],
                event_flags,
                source,
                &mut None,
            )
            .await
            .caused_by(trc::location!())?
            .map(|notifiable| notifiable.event)
        else {
            return Ok(());
        };

        self.notify_direct_change(
            access_token,
            account_id,
            DirectChange::Created {
                event_id,
                current,
                calendar_ids: vec![calendar_id],
                event_flags,
            },
            Some(&calendars),
            quota,
            batch,
        )
        .await
        .caused_by(trc::location!())
    }

    async fn stored_event_if_notifiable(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        calendars: &GroupwareResources,
        calendar_ids: &[u32],
        event_flags: u16,
        source: (u32, u32),
        prefetched: &mut Option<Archive<ArchiveBytes>>,
    ) -> trc::Result<Option<NotifiableEvent>> {
        if !may_have_viewers(
            access_token,
            account_id,
            calendars,
            calendar_ids,
            NotificationAudience::of_event(event_flags),
        ) {
            return Ok(None);
        }

        let (source_account_id, source_document_id) = source;
        let content = match prefetched.take() {
            Some(content) => content,
            None => {
                let Some(content) = self
                    .store()
                    .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                        source_account_id,
                        Collection::CalendarEvent,
                        source_document_id,
                        CalendarEventField::Content,
                    ))
                    .await
                    .caused_by(trc::location!())?
                else {
                    return Ok(None);
                };
                content
            }
        };

        content
            .unarchive::<CalendarEventContent>()
            .and_then(|archived| rkyv_deserialize::<_, ICalendar>(&archived.data.event))
            .map(|event| {
                Some(NotifiableEvent {
                    event,
                    content: Some(content),
                })
            })
            .caused_by(trc::location!())
    }

    async fn may_have_notification_viewers(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        notification: &CalendarEventNotification,
        calendars: Option<&GroupwareResources>,
    ) -> trc::Result<bool> {
        let audience = NotificationAudience::from_flags(notification.flags);
        let account_viewers = AccountViewers::of(
            self.account(account_id)
                .await
                .caused_by(trc::location!())?
                .as_ref(),
        );
        let viewers = |calendars: &GroupwareResources| {
            has_viewers(
                access_token,
                account_id,
                calendars,
                &notification.calendar_ids,
                audience,
                account_viewers,
            )
        };

        match calendars {
            Some(calendars) => Ok(viewers(calendars)),
            None => Ok(self
                .calendars_if_any(access_token, account_id)
                .await
                .caused_by(trc::location!())?
                .is_some_and(|calendars| viewers(&calendars))),
        }
    }

    async fn oldest_notification(
        &self,
        account_id: u32,
        after: Option<NotificationCursor>,
    ) -> trc::Result<Option<NotificationCursor>> {
        let mut oldest = None;
        let (from_created, from_document_id) = after.as_ref().map_or((&[][..], 0), |after| {
            (&after.created[..], after.document_id)
        });
        self.store()
            .iterate(
                IterateParams::new(
                    IndexKey {
                        account_id,
                        collection: Collection::CalendarEventNotification.into(),
                        document_id: from_document_id,
                        field: CalendarNotificationField::Created.into(),
                        key: from_created,
                    },
                    IndexKey {
                        account_id,
                        collection: Collection::CalendarEventNotification.into(),
                        document_id: u32::MAX,
                        field: CalendarNotificationField::Created.into(),
                        key: &u64::MAX.to_be_bytes()[..],
                    },
                )
                .no_values()
                .ascending()
                .limit(1 + usize::from(after.is_some())),
                |key, _| {
                    let document_id = key.deserialize_be_u32(key.len() - U32_LEN)?;
                    if after.is_some_and(|after| after.document_id == document_id) {
                        return Ok(true);
                    }
                    oldest = Some(NotificationCursor {
                        created: key
                            .deserialize_be_u64(key.len().saturating_sub(U32_LEN + CREATED_LEN))?
                            .to_be_bytes(),
                        document_id,
                    });
                    Ok(false)
                },
            )
            .await
            .caused_by(trc::location!())?;
        Ok(oldest)
    }
}
