/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::types::date::UTCDate;
use std::str::FromStr;

#[test]
fn parse_jmap_date() {
    for (input, expected_result) in [
        ("1997-11-21T09:55:06-06:00", "1997-11-21T09:55:06-06:00"),
        ("1997-11-21T09:55:06+00:00", "1997-11-21T09:55:06Z"),
        ("2021-01-01T09:55:06+02:00", "2021-01-01T09:55:06+02:00"),
        ("2004-06-28T23:43:45.000Z", "2004-06-28T23:43:45Z"),
        ("1997-11-21T09:55:06.123+00:00", "1997-11-21T09:55:06Z"),
        (
            "2021-01-01T09:55:06.4567+02:00",
            "2021-01-01T09:55:06+02:00",
        ),
    ] {
        let date = UTCDate::from_str(input).unwrap();
        assert_eq!(date.to_string(), expected_result);

        let timestamp = date.timestamp();
        assert_eq!(UTCDate::from_timestamp(timestamp).timestamp(), timestamp);
    }
}
