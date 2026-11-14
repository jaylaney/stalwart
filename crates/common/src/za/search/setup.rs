/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::seal::za_search_key;
use store::za_search::{
    Checkpoint,
    manifest::{ContentTo, Manifest, ManifestValue},
};
use vault::keys::Secret;

/// The first authenticated search checkpoint, committed with vault activation.
pub fn za_setup_manifest(swk: &Secret, indexed_to: u64) -> Vec<u8> {
    let checkpoint = Checkpoint {
        change_id: indexed_to,
        position: 0,
        complete: true,
    };
    ManifestValue::Index(Manifest {
        indexed_to: checkpoint,
        content_to: ContentTo {
            checkpoint,
            ..ContentTo::default()
        },
        ..Manifest::default()
    })
    .encode(&za_search_key(swk))
}
