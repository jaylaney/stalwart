/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossClass {
    ManifestMissing,
    StubAbsent,
    BlobAbsent,
    Aead,
    Parse,
    Tag,
    OlderFormat,
    Hash,
}

impl LossClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ManifestMissing => "manifest_missing",
            Self::StubAbsent => "stub_absent",
            Self::BlobAbsent => "blob_absent",
            Self::Aead => "aead",
            Self::Parse => "parse",
            Self::Tag => "tag",
            Self::OlderFormat => "older_format",
            Self::Hash => "hash",
        }
    }
}
