/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

// The 0.16 module this replaces held only its `tests` child, kept here
// under the same path.
mod tests {
    // 0.16's `imap_proto::utf7` moved to the external `encodify` crate
    // (`encodify::utf7::IMAP`), which the receiver and the response writers now
    // call. The cases run against it. 0.16's decoder was lenient; 1.0 decodes a
    // mailbox name with `IMAP.lenient()` (`Token::unwrap_mailbox_name`), so the
    // decode cases use that decoder.
    use encodify::utf7::IMAP;

    #[test]
    fn utf7_decode() {
        for (input, expected_result) in [
            ("~peter/mail/&U,BTFw-/&ZeVnLIqe-", "~peter/mail/台北/日本語"),
            ("&U,BTF2XlZyyKng-", "台北日本語"),
            ("Hello, World&ACE-", "Hello, World!"),
            ("Hi Mom -&Jjo--!", "Hi Mom -☺-!"),
            ("&ZeVnLIqe-", "日本語"),
            ("Item 3 is &AKM-1.", "Item 3 is £1."),
            ("Plus minus &- -&- &--", "Plus minus & -& &-"),
            (
                "&APw-ber ihre mi&AN8-liche Lage&ADs- &ACI-wir",
                "über ihre mißliche Lage; \"wir",
            ),
            (
                concat!(
                    "&ACI-The sayings of Confucius,&ACI- James R. Ware, trans.  &U,BTFw-:\n",
                    "&ZYeB9FH6ckh5Pg-, 1980.\n",
                    "&Vttm+E6UfZM-, &W4tRQ066bOg-, &UxdOrA-:  &Ti1XC2b4Xpc-, 1990."
                ),
                concat!(
                    "\"The sayings of Confucius,\" James R. Ware, trans.  台北:\n",
                    "文致出版社, 1980.\n",
                    "四書五經, 宋元人注, 北京:  中國書店, 1990."
                ),
            ),
            ("Test-ąęć-Test", "Test-ąęć-Test"),
            (r#"&A8g- "&A9QD1APUA9gD3APcA-+""#, "ψ \"ϔϔϔϘϜϜ+\""),
        ] {
            assert_eq!(
                IMAP.lenient().decode(input).expect(input),
                expected_result,
                "while decoding {:?}",
                input
            );
        }
    }

    #[test]
    fn utf7_encode() {
        for (expected_result, input) in [
            ("~peter/mail/&U,BTFw-/&ZeVnLIqe-", "~peter/mail/台北/日本語"),
            ("&U,BTF2XlZyyKng-", "台北日本語"),
            ("Hi Mom -&Jjo--!", "Hi Mom -☺-!"),
            ("&ZeVnLIqe-", "日本語"),
            ("Item 3 is &AKM-1.", "Item 3 is £1."),
            ("Plus minus &- -&- &--", "Plus minus & -& &-"),
            ("&VMhUyNg93gQ-", "哈哈😄"),
        ] {
            assert_eq!(
                IMAP.encode(input),
                expected_result,
                "while encoding {:?}",
                expected_result
            );
        }
    }
}
