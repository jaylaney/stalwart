/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::ZA_CALENDAR_KEYLESS;

/// Release 6 spec 6.1: the JMAP error for calendar access without session keys.
pub fn za_calendar_keyless_refusal() -> trc::Error {
    trc::JmapEvent::Forbidden
        .into_err()
        .details(ZA_CALENDAR_KEYLESS)
}

#[cfg(test)]
mod tests {
    use crate::za::mail::za_is_keyless_refusal;
    use trc::{EventType, JmapEvent, Key};

    #[test]
    fn calendar_keyless_refusal_is_forbidden_with_the_fixed_text() {
        let error = super::za_calendar_keyless_refusal();
        let message = super::super::ZA_CALENDAR_KEYLESS;

        assert_eq!(
            message,
            "Calendars in this account are encrypted at rest and are not available to this session."
        );
        assert!(error.matches(EventType::Jmap(JmapEvent::Forbidden)));
        assert_eq!(error.value_as_str(Key::Details), Some(message));
        assert!(error.value(Key::Code).is_none());
        assert!(!za_is_keyless_refusal(&error));
    }
}
