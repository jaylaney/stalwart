/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::Status;
use hyper::StatusCode;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl Serialize for Status {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the status code as a u16
        serializer.serialize_u16(self.0.as_u16())
    }
}

impl<'de> Deserialize<'de> for Status {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize as u16
        let status_value = u16::deserialize(deserializer)?;

        // Convert u16 to StatusCode
        let status_code = StatusCode::try_from(status_value).map_err(|_| {
            serde::de::Error::custom(format!("Invalid status code: {}", status_value))
        })?;

        Ok(Status(status_code))
    }
}
