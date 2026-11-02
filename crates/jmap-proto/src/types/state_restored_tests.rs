/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::State;
use types::ChangeId;

#[test]
fn test_state_id() {
    for id in [
        State::new_initial(),
        State::new_exact(1),
        State::new_exact(12345678),
        State::new_exact(ChangeId::MAX),
        State::new_intermediate(0, 0, 1),
        State::new_intermediate(1024, 2048, 100),
        State::new_intermediate(12345678, 87654321, 1),
        State::new_intermediate(0, 0, 12345678),
        State::new_intermediate(0, 87654321, 12345678),
        State::new_intermediate(12345678, 87654321, 1),
        State::new_intermediate(12345678, 87654321, 12345678),
        State::new_intermediate(ChangeId::MAX, ChangeId::MAX, ChangeId::MAX as usize),
    ] {
        assert_eq!(State::parse(&id.to_string()).unwrap(), id);
    }
}

#[test]
fn test_state_zero_change_id_is_initial() {
    assert_eq!(
        State::parse(&State::new_exact(0).to_string()).unwrap(),
        State::Initial
    );
    assert_eq!(State::from(None), State::Initial);
}
