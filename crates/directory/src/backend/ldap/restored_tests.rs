/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::LdapFilter;

#[test]
fn filter_placeholders_are_escaped() {
    let filter = LdapFilter::new("(&(uid={local})(dc={domain})(mail=?))").unwrap();

    assert_eq!(
        filter.build("*)(uid=*@ex)(dc=*"),
        "(&(uid=\\2a\\29\\28uid=\\2a)(dc=ex\\29\\28dc=\\2a)(mail=\\2a\\29\\28uid=\\2a@ex\\29\\28dc=\\2a))"
    );
    assert_eq!(
        filter.build("john@example.com"),
        "(&(uid=john)(dc=example.com)(mail=john@example.com))"
    );
}
