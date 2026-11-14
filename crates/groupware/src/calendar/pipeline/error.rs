/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    calendar::{seal::seal_error, sharing::resolve::ZaCalendarError},
    scheduling::ItipError,
};
use trc::{EventType, StoreEvent};

#[derive(Debug)]
pub enum ZaPipelineError {
    Forbidden(ZaRefusal),
    /// A shared calendar is not the event's only calendar (`ZaCalendarError::LinkedEvent`).
    LinkedEvent,
    NotFound,
    Busy(ZaBusy),
    /// `href` is `None` when the conflict is an event the caller may not see.
    UidConflict {
        href: Option<String>,
    },
    /// The fan-out's messages reach `itip_outbound_max_recipients`.
    TooManyRecipients,
    /// Only for an error `za_itip_fails_write` accepts.
    Scheduling(ItipError),
    /// `ZaScheduling::Required` met a denied status; the text is `ItipSendStatus::reason()`.
    SchedulingDenied(&'static str),
    /// Everything else, carried unchanged: store, seal (`seal_error`),
    /// `Security`, assertion and quota errors.
    Store(trc::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaRefusal {
    NoKeys,
    Context,
    SharedCopy,
    LinkedMove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaBusy {
    LeaseBusy,
    CreateLost,
}

impl From<trc::Error> for ZaPipelineError {
    fn from(error: trc::Error) -> Self {
        Self::Store(error)
    }
}

impl ZaPipelineError {
    /// In write scope, a resolver error fails the whole write.
    pub fn from_calendar(error: ZaCalendarError, owner_id: u32, document_id: u32) -> Self {
        match error {
            ZaCalendarError::Forbidden => Self::Forbidden(ZaRefusal::Context),
            ZaCalendarError::LinkedEvent => Self::LinkedEvent,
            ZaCalendarError::Store(error)
                if error.matches(EventType::Store(StoreEvent::NotFound)) =>
            {
                Self::NotFound
            }
            error => Self::Store(error.into_store_error(owner_id, document_id)),
        }
    }
}

/// In read scope, a refusal fails the request; any other error fails one item.
pub fn za_read_refusal(
    error: ZaCalendarError,
    owner_id: u32,
    document_id: u32,
) -> Result<trc::Error, ZaPipelineError> {
    match error {
        ZaCalendarError::Forbidden => Err(ZaPipelineError::Forbidden(ZaRefusal::Context)),
        ZaCalendarError::LinkedEvent => Err(ZaPipelineError::LinkedEvent),
        ZaCalendarError::Seal(error) => Ok(seal_error(error, owner_id, document_id)),
        ZaCalendarError::Store(error) if error.matches(EventType::Store(StoreEvent::NotFound)) => {
            Err(ZaPipelineError::NotFound)
        }
        ZaCalendarError::Store(error) => Ok(error),
    }
}

/// Whether a scheduling error fails the write. Matches DAV's
/// `ItipPrecondition::failed_precondition` and requires new variants to be classified.
pub fn za_itip_fails_write(error: &ItipError) -> bool {
    match error {
        ItipError::MultipleOrganizer
        | ItipError::OrganizerIsLocalAddress
        | ItipError::SenderIsNotParticipant(_)
        | ItipError::OrganizerMismatch
        | ItipError::CannotModifyProperty(_)
        | ItipError::CannotModifyInstance
        | ItipError::CannotModifyAddress
        | ItipError::MissingUid
        | ItipError::MultipleUid
        | ItipError::MultipleObjectTypes
        | ItipError::MultipleObjectInstances
        | ItipError::MissingMethod
        | ItipError::InvalidComponentType
        | ItipError::OutOfSequence
        | ItipError::UnknownParticipant(_)
        | ItipError::UnsupportedMethod(_)
        | ItipError::TooManyRecipients => true,
        ItipError::NoSchedulingInfo
        | ItipError::OtherSchedulingAgent
        | ItipError::NotOrganizer
        | ItipError::NotOrganizerNorAttendee
        | ItipError::NothingToSend
        | ItipError::SenderIsNotOrganizerNorAttendee
        | ItipError::SenderIsNotOrganizer
        | ItipError::ICalendarParseError
        | ItipError::EventNotFound
        | ItipError::EventTooLarge
        | ItipError::QuotaExceeded
        | ItipError::NoDefaultCalendar
        | ItipError::AutoAddDisabled => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::calendar::{
        pipeline::error::{ZaPipelineError, ZaRefusal, za_read_refusal},
        seal::SealError,
        sharing::resolve::ZaCalendarError,
    };
    use trc::{EventType, StoreEvent};

    #[test]
    fn calendar_errors_keep_their_scopes() {
        let (owner_id, document_id) = (1, 2);
        let write = |error| ZaPipelineError::from_calendar(error, owner_id, document_id);
        let read = |error| za_read_refusal(error, owner_id, document_id);

        assert!(matches!(
            write(ZaCalendarError::Forbidden),
            ZaPipelineError::Forbidden(ZaRefusal::Context)
        ));
        assert!(matches!(
            write(ZaCalendarError::LinkedEvent),
            ZaPipelineError::LinkedEvent
        ));
        assert!(matches!(
            write(ZaCalendarError::Store(StoreEvent::NotFound.into_err())),
            ZaPipelineError::NotFound
        ));
        assert!(matches!(
            write(ZaCalendarError::Store(StoreEvent::DataCorruption.into_err())),
            ZaPipelineError::Store(error)
                if error.matches(EventType::Store(StoreEvent::DataCorruption))
        ));
        assert!(matches!(
            write(ZaCalendarError::Seal(SealError::Aead)),
            ZaPipelineError::Store(_)
        ));

        assert!(matches!(
            read(ZaCalendarError::Forbidden),
            Err(ZaPipelineError::Forbidden(ZaRefusal::Context))
        ));
        assert!(matches!(
            read(ZaCalendarError::LinkedEvent),
            Err(ZaPipelineError::LinkedEvent)
        ));
        assert!(matches!(
            read(ZaCalendarError::Store(StoreEvent::NotFound.into_err())),
            Err(ZaPipelineError::NotFound)
        ));
        assert!(read(ZaCalendarError::Seal(SealError::Aead)).is_ok());
        assert!(matches!(
            read(ZaCalendarError::Store(StoreEvent::DataCorruption.into_err())),
            Ok(error) if error.matches(EventType::Store(StoreEvent::DataCorruption))
        ));
    }
}
