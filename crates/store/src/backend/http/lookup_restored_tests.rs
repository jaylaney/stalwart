/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use arc_swap::ArcSwap;
use reqwest::Client;
use std::{
    sync::atomic::{AtomicBool, AtomicU64},
    time::Duration,
};

fn http_store(format: HttpStoreFormat, feed: &str) -> Arc<HttpStore> {
    let config = HttpStoreConfig {
        id: "test".into(),
        url: "https://lists.example.org/feed".into(),
        retry: 0,
        refresh_interval: 0,
        timeout: Duration::from_secs(1),
        gzipped: false,
        max_size: 1024 * 1024,
        max_entries: 100,
        max_entry_size: 512,
        format,
        custom_user_agent: false,
    };
    let entries = config
        .parse_entries(feed.as_bytes())
        .expect("feed is readable");

    Arc::new(HttpStore {
        entries: ArcSwap::from_pointee(entries),
        expires: AtomicU64::new(u64::MAX),
        in_flight: AtomicBool::new(false),
        config,
        client: Client::new(),
    })
}

#[test]
fn list_keys_ignore_case() {
    let store = http_store(
        HttpStoreFormat::List,
        "https://phish.example.org/Account/Verify?Token=AbC123\n\
             https://PHISH.example.net/lower\n",
    );

    assert!(store.contains("https://phish.example.org/account/verify?token=abc123"));
    assert!(store.contains("https://phish.example.org/Account/Verify?Token=AbC123"));
    assert!(store.contains("https://phish.example.net/lower"));
    assert!(store.contains("HTTPS://PHISH.EXAMPLE.NET/LOWER"));
    assert!(!store.contains("https://phish.example.org/account/verify"));
}

#[test]
fn csv_keys_ignore_case() {
    let store = http_store(
        HttpStoreFormat::Csv {
            index_key: 1,
            index_value: None,
            separator: ',',
            skip_first: true,
        },
        "phish_id,url,phish_detail_url\n\
             1,\"https://phish.example.org/Login.PHP?Id=Xy\",https://phishtank.example/1\n",
    );

    assert!(store.contains("https://phish.example.org/login.php?id=xy"));
    assert!(store.contains("https://phish.example.org/Login.PHP?Id=Xy"));
    assert!(!store.contains("phish_id"));
    assert!(!store.contains("url"));
}

#[test]
fn csv_values_keep_case() {
    let store = http_store(
        HttpStoreFormat::Csv {
            index_key: 0,
            index_value: Some(1),
            separator: ',',
            skip_first: false,
        },
        "Example.ORG,Some Value\n",
    );

    assert_eq!(
        store.get("example.org"),
        Some(Value::Text("Some Value".into()))
    );
    assert_eq!(
        store.get("EXAMPLE.org"),
        Some(Value::Text("Some Value".into()))
    );
}
