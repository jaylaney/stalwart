/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

#[test]
fn serialize_search() {
    for (mut response, tag, expected_v2, expected_v1) in [
        (
            super::Response {
                is_uid: false,
                is_esearch: true,
                is_sort: false,
                ids: vec![2, 10, 11],
                min: 2.into(),
                max: 11.into(),
                count: 3.into(),
                highest_modseq: None,
            },
            "A283",
            "* ESEARCH (TAG \"A283\") COUNT 3 MIN 2 MAX 11 ALL 2,10:11\r\n",
            "* SEARCH 2 10 11\r\n",
        ),
        (
            super::Response {
                is_uid: false,
                is_esearch: true,
                is_sort: false,
                ids: vec![
                    1, 2, 3, 5, 10, 11, 12, 13, 90, 92, 93, 94, 95, 96, 97, 98, 99,
                ],
                min: None,
                max: None,
                count: None,
                highest_modseq: None,
            },
            "A283",
            "* ESEARCH (TAG \"A283\") ALL 1:3,5,10:13,90,92:99\r\n",
            "* SEARCH 1 2 3 5 10 11 12 13 90 92 93 94 95 96 97 98 99\r\n",
        ),
        (
            super::Response {
                is_uid: false,
                is_esearch: true,
                is_sort: false,
                ids: vec![],
                min: None,
                max: None,
                count: None,
                highest_modseq: None,
            },
            "A283",
            "* ESEARCH (TAG \"A283\")\r\n",
            "* SEARCH\r\n",
        ),
        (
            super::Response {
                is_uid: false,
                is_esearch: true,
                is_sort: false,
                ids: vec![10, 11, 12, 13, 21],
                min: None,
                max: None,
                count: None,
                highest_modseq: 12345.into(),
            },
            "A283",
            "* ESEARCH (TAG \"A283\") ALL 10:13,21 MODSEQ 12345\r\n",
            "* SEARCH 10 11 12 13 21 (MODSEQ 12345)\r\n",
        ),
    ] {
        let response_v2 = String::from_utf8(response.clone().serialize(tag)).unwrap();
        response.is_esearch = false;
        let response_v1 = String::from_utf8(response.serialize(tag)).unwrap();

        assert_eq!(response_v2, expected_v2);
        assert_eq!(response_v1, expected_v1);
    }
}
