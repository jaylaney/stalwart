/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Fixed trace details for zero-access search (search spec 4.3 and 8).

pub const ZA_SEARCH_PASS: &str = "zero-access: search pass";
pub const ZA_SEARCH_SKIPPED: &str = "zero-access: search message skipped";
pub const ZA_SEARCH_ABORTED: &str = "zero-access: search pass aborted";
pub const ZA_SEARCH_COMPACTION_SKIPPED: &str = "zero-access: search compaction skipped";
pub const ZA_SEARCH_LOST: &str = "zero-access: search manifest write lost";
pub const ZA_SEARCH_REBUILD: &str = "zero-access: search rebuild";
pub const ZA_SEARCH_SUSPENDED: &str = "zero-access: search content indexing suspended";
pub const ZA_SEARCH_UNAVAILABLE: &str = "zero-access: search unavailable";
pub const ZA_SEARCH_EVICTED: &str = "zero-access: search index evicted";
pub const ZA_SEARCH_UNRESOLVED: &str = "zero-access: search leaf unresolved";
pub const ZA_SEARCH_OVER_BUDGET: &str = "zero-access: search thread budget exceeded";

#[doc(hidden)]
pub const ZA_SEARCH_CALENDAR_EVENT: trc::CalendarEvent = trc::CalendarEvent::ItipMessageError;

pub const ZA_SEARCH_EVENT_TYPE: trc::EventType = trc::EventType::Calendar(ZA_SEARCH_CALENDAR_EVENT);

/// Classify only the typed discriminator. Context and nested causes may hold
/// plaintext or arbitrary bytes and must never enter a search trace.
pub fn backend_error_class(error: &trc::Error) -> &'static str {
    use trc::{EventType, NetworkEvent, StoreEvent};
    match error.as_ref() {
        EventType::Store(StoreEvent::FoundationdbError) => "foundationdb",
        EventType::Store(StoreEvent::MysqlError) => "mysql",
        EventType::Store(StoreEvent::PostgresqlError) => "postgresql",
        EventType::Store(StoreEvent::RocksdbError) => "rocksdb",
        EventType::Store(StoreEvent::SqliteError) => "sqlite",
        EventType::Store(StoreEvent::RedisError) => "redis",
        EventType::Store(StoreEvent::S3Error) => "s3",
        EventType::Store(StoreEvent::AzureError) => "azure",
        EventType::Store(StoreEvent::FilesystemError) => "filesystem",
        EventType::Store(StoreEvent::PoolError) => "pool",
        EventType::Store(StoreEvent::DataCorruption) => "data-corruption",
        EventType::Store(StoreEvent::DecompressError) => "decompress",
        EventType::Store(StoreEvent::DecryptError) => "decrypt",
        EventType::Store(StoreEvent::DeserializeError) => "deserialize",
        EventType::Store(StoreEvent::NotFound) => "not-found",
        EventType::Store(StoreEvent::NotConfigured) => "not-configured",
        EventType::Store(StoreEvent::NotSupported) => "not-supported",
        EventType::Store(StoreEvent::UnexpectedError) => "unexpected",
        EventType::Store(StoreEvent::CryptoError) => "crypto",
        EventType::Store(StoreEvent::HttpStoreError) => "http-store",
        EventType::Network(NetworkEvent::Timeout) => "timeout",
        _ => "other",
    }
}

/// Emit a fixed search trace with ids, counts, revisions or reason classes.
#[macro_export]
macro_rules! za_search_trace {
    ($details:expr $(, $key:ident = $value:expr)* $(,)?) => {
        trc::event!(
            Calendar($crate::za::search::trace::ZA_SEARCH_CALENDAR_EVENT),
            Details = $details,
            $($key = $value,)*
        )
    };
}

pub use crate::za_search_trace;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_search_trace_is_a_fixed_prefixed_string() {
        for s in [
            ZA_SEARCH_PASS,
            ZA_SEARCH_SKIPPED,
            ZA_SEARCH_ABORTED,
            ZA_SEARCH_COMPACTION_SKIPPED,
            ZA_SEARCH_LOST,
            ZA_SEARCH_REBUILD,
            ZA_SEARCH_SUSPENDED,
            ZA_SEARCH_UNAVAILABLE,
            ZA_SEARCH_EVICTED,
            ZA_SEARCH_UNRESOLVED,
            ZA_SEARCH_OVER_BUDGET,
        ] {
            assert!(s.starts_with("zero-access: search "), "{s}");
            assert!(!s.contains('{'), "{s}");
        }
        assert_eq!(
            crate::za::ZA_MAIL_SEARCH_UNAVAILABLE,
            "Search for this account is not available right now; try again shortly."
        );
    }

    #[test]
    fn the_search_trace_accepts_two_keys() {
        let _emit = || za_search_trace!(ZA_SEARCH_PASS, AccountId = 5u32, Total = 3u64);
    }
}
