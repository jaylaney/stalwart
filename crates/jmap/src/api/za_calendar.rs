/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{Server, auth::AccessToken, za::calendar::za_calendar_keyless_refusal};
use groupware::calendar::pipeline::{admission::za_calendar_admission, error::ZaPipelineError};
use jmap_proto::request::method::{MethodFunction, MethodObject};
use types::id::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZaCalendarMethod {
    /// An owner with keys or a grantee with their own keys and shared access.
    #[allow(dead_code)] // Task 21 first serves calendar methods.
    Served,
    /// CalendarEvent/parse requires the owner's keys.
    #[allow(dead_code)] // Task 24 first serves CalendarEvent/parse.
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
    if !server.za_is_key_account(owner).await? {
        return Ok(());
    }

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
    use ZaCalendarMethod::Refused;

    match (obj, function) {
        (Calendar, Get) => Refused,
        (Calendar, Query) => Refused,
        (Calendar, Changes) => Refused,
        (Calendar, Set) => Refused,
        (CalendarEvent, Get) => Refused,
        (CalendarEvent, Changes) => Refused,
        (CalendarEvent, Parse) => Refused,
        (CalendarEvent, Query) => Refused,
        (CalendarEvent, QueryChanges) => Refused,
        (CalendarEvent, Set) => Refused,
        (CalendarEvent, Copy) => Refused,
        (ParticipantIdentity, Get) => Refused,
        (ParticipantIdentity, Changes) => Refused,
        (ParticipantIdentity, Set) => Refused,
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
    fn calendar_method_classes() {
        for (object, function, expected) in [
            (Calendar, Get, Refused),
            (Calendar, Query, Refused),
            (Calendar, Changes, Refused),
            (Calendar, Set, Refused),
            (CalendarEvent, Get, Refused),
            (CalendarEvent, Changes, Refused),
            (CalendarEvent, Parse, Refused),
            (CalendarEvent, Query, Refused),
            (CalendarEvent, QueryChanges, Refused),
            (CalendarEvent, Set, Refused),
            (CalendarEvent, Copy, Refused),
            (ParticipantIdentity, Get, Refused),
            (ParticipantIdentity, Changes, Refused),
            (ParticipantIdentity, Set, Refused),
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
