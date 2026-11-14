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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientClass {
    Store,
    BlobStore,
    Budget,
    AbsentWithinWindow,
    CodecKey,
    Validation,
}

impl TransientClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::BlobStore => "blob_store",
            Self::Budget => "budget",
            Self::AbsentWithinWindow => "absent_within_window",
            Self::CodecKey => "codec_key",
            Self::Validation => "validation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMapReason {
    NewerFormat,
    Destroying,
    Transient(TransientClass),
}

impl NoMapReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewerFormat => "newer_format",
            Self::Destroying => "destroying",
            Self::Transient(class) => class.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    Loss(LossClass),
    Transient(TransientClass),
    NoMap(NoMapReason),
    OverCap,
}

impl LoadFailure {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Loss(class) => class.as_str(),
            Self::Transient(class) => class.as_str(),
            Self::NoMap(reason) => reason.as_str(),
            Self::OverCap => "over_cap",
        }
    }
}
