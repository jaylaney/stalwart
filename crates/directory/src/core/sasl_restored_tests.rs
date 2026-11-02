/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_extract_oauth_bearer() {
    let input = b"auth=Bearer validtoken";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, Some(("validtoken".to_string(), None)));

    let input = b"auth=Invalid validtoken";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, None);

    let input = b"auth=Bearer";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, None);

    let input = b"";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, None);

    let input = b"auth=Bearer token1\x01auth=Bearer token2";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, Some(("token1".to_string(), None)));

    let input = b"auth=Bearer VALIDTOKEN";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, Some(("VALIDTOKEN".to_string(), None)));

    let input = b"auth=Bearer token with spaces";
    let result = extract_oauth_bearer(input);
    assert_eq!(result, Some(("token with spaces".to_string(), None)));

    let input = b"auth=Bearer token_with_special_chars!@#";
    let result = extract_oauth_bearer(input);
    assert_eq!(
        result,
        Some(("token_with_special_chars!@#".to_string(), None))
    );

    let input = "n,a=user@example.com,\x01host=server.example.com\x01port=143\x01auth=Bearer vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg==\x01\x01";
    let result = extract_oauth_bearer(input.as_bytes());
    assert_eq!(
        result,
        Some((
            "vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg==".to_string(),
            Some("user@example.com".to_string())
        ))
    );
}
