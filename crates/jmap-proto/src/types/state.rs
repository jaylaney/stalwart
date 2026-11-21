/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use encodify::base32::STALWART;
use std::{
    borrow::Cow,
    fmt::{self, Write},
    io::{Cursor, Write as _},
};
use types::ChangeId;
use utils::codec::leb128::{Leb128, Leb128Iterator};

// Release 6 spec 6.8 (D29): `z` holds two one-byte fields and four LEB128 values.
const MAX_SERIALIZED_LEN: usize = 42;
const ZA_CALENDAR_STATE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JMAPIntermediateState {
    pub from_id: ChangeId,
    pub to_id: ChangeId,
    pub items_sent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Initial,
    Exact(ChangeId),
    Intermediate(JMAPIntermediateState),
    /// Release 6 spec 6.8 (D29): a key-calendar continuation, the existing range and
    /// count with the state its sync started from. Transient protocol data, never stored.
    ZaCalendarIntermediate {
        range: JMAPIntermediateState,
        origin: ZaCalendarOrigin,
    },
}

/// Release 6 spec 6.8 (D29): the start of a paged key-calendar sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZaCalendarOrigin {
    Initial,
    Exact(u64),
}

impl From<Option<ChangeId>> for State {
    fn from(change_id: Option<ChangeId>) -> Self {
        match change_id {
            Some(change_id) => State::Exact(change_id),
            None => State::Initial,
        }
    }
}

impl State {
    pub fn parse(value: &str) -> Option<Self> {
        let mut it = value.as_bytes().iter();

        match it.next()? {
            b'n' => Some(State::Initial),
            b's' => {
                let mut reader = STALWART.decoder_from_iter(it);
                reader
                    .next_leb128::<ChangeId>()
                    .map(|change_id| (change_id != 0).then_some(change_id).into())
            }
            b'r' => {
                let mut it = STALWART.decoder_from_iter(it);

                if let (Some(from_id), Some(to_id), Some(items_sent)) = (
                    it.next_leb128::<ChangeId>(),
                    it.next_leb128::<ChangeId>(),
                    it.next_leb128::<usize>(),
                ) {
                    if items_sent > 0 {
                        Some(State::Intermediate(JMAPIntermediateState {
                            from_id,
                            to_id: from_id.saturating_add(to_id),
                            items_sent,
                        }))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            b'z' => Self::parse_za_calendar(it),
            _ => None,
        }
    }

    /// Release 6 spec 6.8 (D29): version 1, origin tag (0 Initial, 1 Exact), the nonzero
    /// exact ID for tag 1, `from_id`, `to_id - from_id` and a positive `items_sent`.
    /// Unknown values, missing or trailing input and arithmetic overflow are refused.
    fn parse_za_calendar(it: std::slice::Iter<'_, u8>) -> Option<Self> {
        let mut reader = STALWART.decoder_from_iter(it);
        if reader.next_leb128::<u8>()? != ZA_CALENDAR_STATE_VERSION {
            return None;
        }
        let origin = match reader.next_leb128::<u8>()? {
            0 => ZaCalendarOrigin::Initial,
            1 => ZaCalendarOrigin::Exact(reader.next_leb128::<ChangeId>().filter(|id| *id != 0)?),
            _ => return None,
        };
        let from_id = reader.next_leb128::<ChangeId>()?;
        let to_id = from_id.checked_add(reader.next_leb128::<ChangeId>()?)?;
        let items_sent = reader.next_leb128::<usize>().filter(|sent| *sent > 0)?;
        (reader.next().is_none() && reader.remaining().is_empty()).then_some(
            State::ZaCalendarIntermediate {
                range: JMAPIntermediateState {
                    from_id,
                    to_id,
                    items_sent,
                },
                origin,
            },
        )
    }

    pub fn new_initial() -> Self {
        State::Initial
    }

    pub fn new_exact(id: ChangeId) -> Self {
        State::Exact(id)
    }

    pub fn new_intermediate(from_id: ChangeId, to_id: ChangeId, items_sent: usize) -> Self {
        State::Intermediate(JMAPIntermediateState {
            from_id,
            to_id,
            items_sent,
        })
    }

    pub fn get_change_id(&self) -> ChangeId {
        match self {
            State::Exact(id) => *id,
            State::Intermediate(intermediate) => intermediate.to_id,
            State::ZaCalendarIntermediate { range, .. } => range.to_id,
            State::Initial => ChangeId::MAX,
        }
    }
}

impl serde::Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

pub(crate) struct ExpectedState(pub Option<State>);

impl<'de> serde::Deserialize<'de> for ExpectedState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(ExpectedState(
            <Option<Cow<'de, str>>>::deserialize(deserializer)?.map(|value| {
                // Release 6 spec 6.8 (D29): `z` is never a current state, so it takes the
                // invalid string's sentinel and the precondition answers stateMismatch.
                match State::parse(&value) {
                    Some(State::ZaCalendarIntermediate { .. }) | None => {
                        State::Intermediate(JMAPIntermediateState {
                            from_id: 0,
                            to_id: 0,
                            items_sent: 0,
                        })
                    }
                    Some(state) => state,
                }
            }),
        ))
    }
}

impl<'de> serde::Deserialize<'de> for State {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        State::parse(<&str>::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("invalid JMAP State"))
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bytes = Cursor::new([0u8; MAX_SERIALIZED_LEN]);

        let prefix = match self {
            State::Initial => 'n',
            State::Exact(id) => {
                bytes.write_all(&id.to_leb128()).map_err(|_| fmt::Error)?;
                's'
            }
            State::Intermediate(intermediate) => {
                bytes
                    .write_all(&intermediate.from_id.to_leb128())
                    .map_err(|_| fmt::Error)?;
                bytes
                    .write_all(&(intermediate.to_id - intermediate.from_id).to_leb128())
                    .map_err(|_| fmt::Error)?;
                bytes
                    .write_all(&intermediate.items_sent.to_leb128())
                    .map_err(|_| fmt::Error)?;
                'r'
            }
            State::ZaCalendarIntermediate { range, origin } => {
                bytes
                    .write_all(&ZA_CALENDAR_STATE_VERSION.to_leb128())
                    .map_err(|_| fmt::Error)?;
                match origin {
                    ZaCalendarOrigin::Initial => {
                        bytes.write_all(&0u8.to_leb128()).map_err(|_| fmt::Error)?;
                    }
                    ZaCalendarOrigin::Exact(id) => {
                        bytes.write_all(&1u8.to_leb128()).map_err(|_| fmt::Error)?;
                        bytes.write_all(&id.to_leb128()).map_err(|_| fmt::Error)?;
                    }
                }
                bytes
                    .write_all(&range.from_id.to_leb128())
                    .map_err(|_| fmt::Error)?;
                bytes
                    .write_all(
                        &range
                            .to_id
                            .checked_sub(range.from_id)
                            .ok_or(fmt::Error)?
                            .to_leb128(),
                    )
                    .map_err(|_| fmt::Error)?;
                bytes
                    .write_all(&range.items_sent.to_leb128())
                    .map_err(|_| fmt::Error)?;
                'z'
            }
        };

        f.write_char(prefix)?;
        let len = bytes.position() as usize;
        write!(
            f,
            "{}",
            STALWART.display(bytes.get_ref().get(..len).unwrap_or_default())
        )
    }
}

#[cfg(all(test, stalwart_tests))]
#[rustfmt::skip]
#[path = "../../../../../stalwart-tests/unit/jmap-proto/src/types/state/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "state_restored_tests.rs"]
mod tests;
