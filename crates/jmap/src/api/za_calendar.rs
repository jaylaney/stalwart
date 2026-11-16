/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{Server, auth::AccessToken, za::calendar::za_calendar_keyless_refusal};
use groupware::calendar::pipeline::{admission::za_calendar_admission, error::ZaPipelineError};
use jmap_proto::request::method::{MethodFunction, MethodObject};
use std::future::Future;
use trc::{EventType, Key};
use types::id::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZaCalendarMethod {
    /// An owner with keys or a grantee with their own keys and shared access.
    Served,
    /// CalendarEvent/parse requires the owner's keys.
    OwnerOnly,
    /// The method is unavailable for key accounts.
    Refused,
}

/// Release 6 spec 6.1: admit a calendar method after its own access check.
pub(crate) async fn za_admit_calendar_method(
    server: &Server,
    access_token: &AccessToken,
    account_id: Id,
    method: ZaCalendarMethod,
) -> trc::Result<()> {
    let owner = account_id.document_id();
    if server.za_is_key_account(owner).await? {
        za_admit_key_calendar_method(server, access_token, owner, method).await
    } else {
        Ok(())
    }
}

/// Release 6 spec 8: keep the admission decision through the read result.
pub(crate) async fn za_serve_calendar_read<T>(
    server: &Server,
    access_token: &AccessToken,
    account_id: Id,
    method: ZaCalendarMethod,
    read: impl Future<Output = trc::Result<T>> + Send,
) -> trc::Result<T> {
    let owner = account_id.document_id();
    let za_key = server.za_is_key_account(owner).await?;
    if za_key {
        za_admit_key_calendar_method(server, access_token, owner, method)
            .await
            .map_err(|error| calendar_read_error(za_key, owner, error))?;
    }

    read.await
        .map_err(|error| calendar_read_error(za_key, owner, error))
}

pub(crate) fn calendar_read_error(za_key: bool, owner: u32, error: trc::Error) -> trc::Error {
    let event = error.event_type();
    if !za_key || !matches!(event, EventType::Store(_)) {
        return error;
    }

    // The dispatched method error is also traced. Store context can contain archive bytes.
    let document = error
        .value_as_uint(Key::DocumentId)
        .and_then(|id| u32::try_from(id).ok());
    let error = event
        .into_err()
        .account_id(owner)
        .details("zero-access: calendar read failed");
    if let Some(document) = document {
        error.document_id(document)
    } else {
        error
    }
}

async fn za_admit_key_calendar_method(
    server: &Server,
    access_token: &AccessToken,
    owner: u32,
    method: ZaCalendarMethod,
) -> trc::Result<()> {
    match method {
        ZaCalendarMethod::Refused => Err(trc::JmapEvent::AccountNotSupportedByMethod.into_err()),
        ZaCalendarMethod::Served => {
            match za_calendar_admission(server, access_token, owner).await {
                Ok(()) => Ok(()),
                Err(ZaPipelineError::Store(error)) => Err(error),
                Err(_) => Err(za_calendar_keyless_refusal().account_id(owner)),
            }
        }
        ZaCalendarMethod::OwnerOnly => {
            if access_token.za_keys_for(owner).is_some() {
                Ok(())
            } else {
                trc::event!(
                    Security(trc::SecurityEvent::Unauthorized),
                    AccountId = owner,
                    Details = "zero-access: calendar access without session keys",
                );
                Err(za_calendar_keyless_refusal().account_id(owner))
            }
        }
    }
}

/// Release 6 spec 6.2: classify each JMAP calendar method.
pub(crate) fn za_calendar_method(obj: MethodObject, function: MethodFunction) -> ZaCalendarMethod {
    use MethodFunction::*;
    use MethodObject::*;
    use ZaCalendarMethod::{OwnerOnly, Refused, Served};

    match (obj, function) {
        (Calendar, Get) => Served,
        (Calendar, Query) => Served,
        // Release 6 spec 6.2: Calendar/changes and CalendarEvent/changes are served.
        (Calendar, Changes) => Served,
        (Calendar, Set) => Refused,
        (CalendarEvent, Get) => Served,
        (CalendarEvent, Changes) => Served,
        // Release 6 spec 6.2: CalendarEvent/parse admits the owner with keys only.
        (CalendarEvent, Parse) => OwnerOnly,
        (CalendarEvent, Query) => Refused,
        (CalendarEvent, QueryChanges) => Refused,
        (CalendarEvent, Set) => Refused,
        (CalendarEvent, Copy) => Refused,
        // Release 6 spec 6.2: the identities are served; /set only switches the default.
        (ParticipantIdentity, Get) => Served,
        (ParticipantIdentity, Changes) => Served,
        (ParticipantIdentity, Set) => Served,
        (CalendarEventNotification, Get) => Refused,
        (CalendarEventNotification, Query) => Refused,
        (CalendarEventNotification, Set) => Refused,
        (CalendarEventNotification, Changes) => Refused,
        (CalendarEventNotification, QueryChanges) => Refused,
        _ => Refused,
    }
}

/// Whether the object's methods trigger the calendar drain.
#[allow(dead_code)] // Task 34 adds the drain call.
pub(crate) fn is_calendar_method(obj: MethodObject) -> bool {
    matches!(
        obj,
        MethodObject::Calendar | MethodObject::CalendarEvent | MethodObject::ParticipantIdentity
    )
}

#[cfg(test)]
mod tests {
    use super::{ZaCalendarMethod::*, is_calendar_method, za_calendar_method};
    use jmap_proto::request::method::{MethodFunction::*, MethodObject::*};

    #[test]
    fn calendar_read_error_context() {
        use super::calendar_read_error;
        use jmap_proto::error::method::MethodErrorWrapper;
        use serde_json::json;
        use trc::{EventType, JmapEvent, Key, StoreEvent, Value};

        const OWNER: u32 = 17;
        const DOC: u32 = 23;
        const PAYLOAD: &[u8] = b"synthetic raw archive bytes";
        for event in [
            StoreEvent::DataCorruption,
            StoreEvent::DeserializeError,
            StoreEvent::DecompressError,
        ] {
            let original = event
                .into_err()
                .account_id(99)
                .document_id(DOC)
                .details("original details")
                .caused_by("original context")
                .ctx(Key::Value, PAYLOAD);
            let wire = serde_json::to_value(MethodErrorWrapper::from(original.clone())).unwrap();
            let ordinary = calendar_read_error(false, OWNER, original.clone());
            assert_eq!(ordinary.event_type(), EventType::Store(event));
            assert_eq!(ordinary.value_as_uint(Key::AccountId), Some(99));
            assert_eq!(
                ordinary.value_as_uint(Key::DocumentId),
                Some(u64::from(DOC))
            );
            assert_eq!(
                ordinary.value_as_str(Key::Details),
                Some("original details")
            );
            assert_eq!(
                ordinary.value_as_str(Key::CausedBy),
                Some("original context")
            );
            assert!(
                matches!(ordinary.value(Key::Value), Some(Value::Bytes(bytes)) if bytes.as_slice() == PAYLOAD)
            );

            let keyed = calendar_read_error(true, OWNER, original);
            assert_eq!(keyed.event_type(), EventType::Store(event));
            assert_eq!(keyed.value_as_uint(Key::AccountId), Some(u64::from(OWNER)));
            assert_eq!(keyed.value_as_uint(Key::DocumentId), Some(u64::from(DOC)));
            assert_eq!(
                keyed.value_as_str(Key::Details),
                Some("zero-access: calendar read failed")
            );
            assert_eq!(keyed.keys().len(), 3);
            assert_eq!(
                wire,
                json!({"type": "serverUnavailable",
                       "description": "This server is temporarily unavailable. Attempting this same operation later may succeed."})
            );
            assert_eq!(
                serde_json::to_value(MethodErrorWrapper::from(keyed)).unwrap(),
                wire
            );
        }

        for document_id in [
            Value::String("23".into()),
            Value::UInt(u64::from(u32::MAX) + 1),
        ] {
            let error = StoreEvent::DataCorruption
                .into_err()
                .ctx(Key::DocumentId, document_id);
            let keyed = calendar_read_error(true, OWNER, error);
            assert!(keyed.value(Key::DocumentId).is_none());
        }

        let upstream = JmapEvent::InvalidArguments
            .into_err()
            .details("upstream reason");
        let same = calendar_read_error(true, OWNER, upstream);
        assert_eq!(
            same.event_type(),
            EventType::Jmap(JmapEvent::InvalidArguments)
        );
        assert_eq!(same.value_as_str(Key::Details), Some("upstream reason"));
        assert_eq!(same.keys().len(), 1);
    }

    #[test]
    fn calendar_method_classes() {
        for (object, function, expected) in [
            (Calendar, Get, Served),
            (Calendar, Query, Served),
            // Release 6 spec 6.2: both /changes rows are served.
            (Calendar, Changes, Served),
            (Calendar, Set, Refused),
            (CalendarEvent, Get, Served),
            (CalendarEvent, Changes, Served),
            // Release 6 spec 6.2: CalendarEvent/parse admits the owner with keys only.
            (CalendarEvent, Parse, OwnerOnly),
            (CalendarEvent, Query, Refused),
            (CalendarEvent, QueryChanges, Refused),
            (CalendarEvent, Set, Refused),
            (CalendarEvent, Copy, Refused),
            // Release 6 spec 6.2: the identities are served.
            (ParticipantIdentity, Get, Served),
            (ParticipantIdentity, Changes, Served),
            (ParticipantIdentity, Set, Served),
            (CalendarEventNotification, Get, Refused),
            (CalendarEventNotification, Query, Refused),
            (CalendarEventNotification, Set, Refused),
            (CalendarEventNotification, Changes, Refused),
            (CalendarEventNotification, QueryChanges, Refused),
            (CalendarEvent, Import, Refused),
            (Email, Get, Refused),
        ] {
            assert_eq!(
                za_calendar_method(object, function),
                expected,
                "{object:?}/{function:?}",
            );
        }

        for (object, expected) in [
            (Calendar, true),
            (CalendarEvent, true),
            (ParticipantIdentity, true),
            (CalendarEventNotification, false),
            (Email, false),
            (Principal, false),
            (ShareNotification, false),
        ] {
            assert_eq!(is_calendar_method(object), expected, "{object:?}");
        }
    }
}
