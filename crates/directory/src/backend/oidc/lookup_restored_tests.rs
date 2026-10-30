/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use serde_json::json;

#[test]
fn extract_string_list_distinguishes_absent_from_empty() {
    assert_eq!(
        extract_string_list(&json!(["sales", "support"])),
        Some(vec!["sales".to_string(), "support".to_string()])
    );
    assert_eq!(
        extract_string_list(&json!("sales support")),
        Some(vec!["sales".to_string(), "support".to_string()])
    );
    assert_eq!(extract_string_list(&json!([])), Some(vec![]));
    assert_eq!(extract_string_list(&json!("")), Some(vec![]));
    assert_eq!(extract_string_list(&json!(null)), None);
    assert_eq!(extract_string_list(&json!({"groups": []})), None);
    assert_eq!(extract_string_list(&json!(42)), None);
}

#[test]
fn is_claim_missing_treats_null_as_absent() {
    let claims = json!({"groups": null, "name": "John Doe", "roles": []});

    assert!(is_claim_missing(&claims, Some(&"groups".to_string())));
    assert!(is_claim_missing(&claims, Some(&"unknown".to_string())));
    assert!(!is_claim_missing(&claims, Some(&"name".to_string())));
    assert!(!is_claim_missing(&claims, Some(&"roles".to_string())));
    assert!(!is_claim_missing(&claims, None));
}
