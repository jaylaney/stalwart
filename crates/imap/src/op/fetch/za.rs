/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 7, 10: FETCH of a key account's message. The stored row is a
//! stub, opened into the message's full row with the session's keys
//! (`za_open_metadata_row`); its blob is sealed, opened whole
//! (`za_open_message_blob`) and held in a `Zeroizing` buffer for the
//! command. Every section, header, part and source reader then reads the
//! opened row and bytes as it reads an ordinary message's.

use common::{network::memory::MemoryReservation, storage::blob::FetchedBlob};
use email::message::za::view::Zeroizing;
use std::ops::Deref;

/// The blob a FETCH reads: the stored bytes of an ordinary message, or a
/// sealed blob opened, with the memory reservation the stored bytes were
/// read under.
pub(super) enum ZaFetchedBlob {
    Stored(FetchedBlob),
    Opened {
        bytes: Zeroizing<Vec<u8>>,
        _reservation: MemoryReservation,
    },
}

impl ZaFetchedBlob {
    /// The sealed blob `blob` opened into `bytes`; `blob`'s reservation is
    /// kept until the command drops the opened bytes.
    pub(super) fn opened(bytes: Zeroizing<Vec<u8>>, reservation: MemoryReservation) -> Self {
        ZaFetchedBlob::Opened {
            bytes,
            _reservation: reservation,
        }
    }
}

impl Deref for ZaFetchedBlob {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            ZaFetchedBlob::Stored(blob) => blob,
            ZaFetchedBlob::Opened { bytes, .. } => bytes.as_slice(),
        }
    }
}
