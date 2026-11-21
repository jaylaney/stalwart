/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::ZA_CALENDAR_KEYLESS;
use crate::auth::{AccessToken, oauth::scope::OAuthScopes};
use std::sync::Arc;
use vault::session::SessionKeys;

/// Invitations spec 6.1, release 6 spec 5.5 (PR #90 review C4617611594): the
/// keys a request gives to the invitation drain. Only the caller's own, and
/// only when the token's scope admits calendars; a Basic login and a token
/// with full scope have no restricted scope, so they are unaffected. The one
/// rule of the CalDAV and the JMAP trigger.
pub fn za_drain_keys(access_token: &AccessToken) -> Option<&Arc<SessionKeys>> {
    access_token
        .za_keys_for(access_token.account_id())
        .filter(|_| access_token.allows_oauth_scopes(OAuthScopes::CALENDARS))
}

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
