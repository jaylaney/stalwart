/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::alternate_links;

#[test]
fn parses_single_alternate_link() {
    let links =
        alternate_links([r#"<https://acme.example/cert/1/1>;rel="alternate""#].into_iter());
    assert_eq!(links, vec!["https://acme.example/cert/1/1".to_string()]);
}

#[test]
fn parses_multiple_alternates_in_one_header() {
    let links = alternate_links(
        [r#"<https://acme.example/cert/1/1>;rel="alternate", <https://acme.example/cert/1/2>;rel="alternate""#]
            .into_iter(),
    );
    assert_eq!(
        links,
        vec![
            "https://acme.example/cert/1/1".to_string(),
            "https://acme.example/cert/1/2".to_string(),
        ]
    );
}

#[test]
fn parses_alternates_across_multiple_headers() {
    let links = alternate_links(
        [
            r#"<https://acme.example/cert/1/1>;rel="alternate""#,
            r#"<https://acme.example/cert/1/2>;rel="alternate""#,
        ]
        .into_iter(),
    );
    assert_eq!(
        links,
        vec![
            "https://acme.example/cert/1/1".to_string(),
            "https://acme.example/cert/1/2".to_string(),
        ]
    );
}

#[test]
fn ignores_non_alternate_relations() {
    let links = alternate_links(
        [r#"<https://acme.example/index>;rel="index", <https://acme.example/cert/1/1>;rel="alternate""#]
            .into_iter(),
    );
    assert_eq!(links, vec!["https://acme.example/cert/1/1".to_string()]);
}

#[test]
fn tolerates_unquoted_rel_and_extra_whitespace() {
    let links =
        alternate_links([r#" <https://acme.example/cert/1/1> ; rel=alternate "#].into_iter());
    assert_eq!(links, vec!["https://acme.example/cert/1/1".to_string()]);
}

#[test]
fn returns_empty_when_no_alternates() {
    let links = alternate_links([r#"<https://acme.example/dir>;rel="index""#].into_iter());
    assert!(links.is_empty());
}
