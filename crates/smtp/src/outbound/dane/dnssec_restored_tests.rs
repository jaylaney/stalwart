/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use mail_auth::hickory_resolver::proto::rr::rdata::{A, CNAME};
use std::net::Ipv4Addr;

fn name(value: &str) -> Name {
    Name::from_ascii(value).unwrap()
}

fn address(owner: &str, proof: Proof) -> Record {
    let mut record =
        Record::from_rdata(name(owner), 3600, RData::A(A(Ipv4Addr::new(192, 0, 2, 1))));
    record.proof = proof;
    record
}

fn alias(owner: &str, target: &str, proof: Proof) -> Record {
    let mut record = Record::from_rdata(name(owner), 3600, RData::CNAME(CNAME(name(target))));
    record.proof = proof;
    record
}

#[test]
fn tlsa_base_status_follows_address_records() {
    let query = name("mx.example.org.");

    for (proof, expected) in [
        (Proof::Secure, DnssecStatus::Secure),
        (Proof::Insecure, DnssecStatus::Insecure),
        (Proof::Bogus, DnssecStatus::Bogus),
        (Proof::Indeterminate, DnssecStatus::Indeterminate),
    ] {
        assert_eq!(
            tlsa_base_status(&query, &[address("mx.example.org.", proof)], RecordType::A),
            expected,
            "proof {proof}"
        );
    }
}

#[test]
fn tlsa_base_status_is_indeterminate_without_addresses() {
    assert_eq!(
        tlsa_base_status(&name("mx.example.org."), &[], RecordType::A),
        DnssecStatus::Indeterminate
    );
}

#[test]
fn tlsa_base_status_takes_least_secure_address() {
    let query = name("mx.example.org.");

    assert_eq!(
        tlsa_base_status(
            &query,
            &[
                address("mx.example.org.", Proof::Secure),
                address("mx.example.org.", Proof::Insecure),
            ],
            RecordType::A
        ),
        DnssecStatus::Insecure
    );
}

#[test]
fn tlsa_base_status_keeps_secure_alias_to_insecure_zone() {
    let query = name("mx.example.org.");

    assert_eq!(
        tlsa_base_status(
            &query,
            &[
                alias("mx.example.org.", "mx.provider.net.", Proof::Secure),
                address("mx.provider.net.", Proof::Insecure),
            ],
            RecordType::A
        ),
        DnssecStatus::Secure
    );
}

#[test]
fn tlsa_base_status_skips_insecure_alias() {
    let query = name("mx.example.org.");

    assert_eq!(
        tlsa_base_status(
            &query,
            &[
                alias("mx.example.org.", "mx.provider.net.", Proof::Insecure),
                address("mx.provider.net.", Proof::Insecure),
            ],
            RecordType::A
        ),
        DnssecStatus::Insecure
    );
}

#[test]
fn tlsa_base_status_ignores_alias_below_query_name() {
    let query = name("mx.example.org.");

    assert_eq!(
        tlsa_base_status(
            &query,
            &[
                alias("mx.provider.net.", "mx.other.net.", Proof::Secure),
                address("mx.other.net.", Proof::Insecure),
            ],
            RecordType::A
        ),
        DnssecStatus::Insecure
    );
}
