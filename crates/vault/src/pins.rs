/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The pin record (key pinning spec 4.1): a key account's pinned contact
//! keys and its withheld deliveries, stored as `PrincipalField::ZeroAccessPins`
//! sealed under MK in the format of spec 7.

use crate::keys::{AAD_PINS, PUBLIC_KEY_LEN, Secret, aad, open, seal};
use zeroize::{Zeroize, Zeroizing};

/// `ZaPinRecord::version`; any other value makes a record unusable (spec 4.4).
pub const PIN_RECORD_VERSION: u8 = 1;
/// Instance keys per withheld entry, the master not counted; past it the
/// entry covers every qualifying instance (spec 4.1).
pub const INSTANCE_KEY_CAP: usize = 64;
/// The sealed plaintext is zero-padded to a multiple of this (spec 7).
pub const PIN_PAD: usize = 256;

/// One per account, sealed under MK (spec 4.1). No derived `Debug`: it holds
/// addresses.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaPinRecord {
    pub version: u8,
    /// Incremented by every write; informational (spec 4.2).
    pub revision: u64,
    /// Unique by `address`.
    pub pins: Vec<ZaPin>,
    /// Refused deliveries awaiting redelivery, unique by (`document_id`,
    /// `uid`, `address`).
    pub withheld: Vec<ZaWithheld>,
    /// Unix seconds of the last self-check repair (spec 3.3).
    pub key_repaired_at: Option<i64>,
}

/// A contact's pinned key. No derived `Debug`: it holds an address.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaPin {
    /// Normalized (spec 4.3).
    pub address: String,
    pub public_key: [u8; PUBLIC_KEY_LEN],
    /// Unix seconds.
    pub pinned_at: i64,
    pub source: PinSource,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinSource {
    /// The fan-out pinned the served key.
    FirstUse,
    /// The user pinned it on the page.
    Confirmed,
}

/// A refused delivery awaiting redelivery. It holds no message (spec 4.1).
/// No derived `Debug`: it holds addresses and the UID.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaWithheld {
    /// The source event's document id.
    pub document_id: u32,
    /// The source event's UID.
    pub uid: String,
    /// The refused recipient, normalized as pins are.
    pub address: String,
    /// Unix seconds of the first refusal.
    pub since: i64,
    /// Regenerated on every refusal of the entry, so a delivery retires
    /// exactly the refusal it consumed (spec 5.2).
    pub refusal_id: u64,
    /// Attendee-side entries only; organizer-side entries leave it empty.
    pub instances: ZaInstances,
}

/// The instance keys of refused REPLYs: the drain's `InstanceKey` (a UTC
/// instant and the THISANDFUTURE flag), the master apart.
#[derive(
    rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Default, Clone, PartialEq, Eq,
)]
pub struct ZaInstances {
    pub master: bool,
    /// At most `INSTANCE_KEY_CAP`; empty once `all` is set.
    pub keys: Vec<(i64, bool)>,
    /// Every qualifying instance, the master included.
    pub all: bool,
}

impl std::fmt::Debug for ZaPinRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZaPinRecord")
            .field("version", &self.version)
            .field("revision", &self.revision)
            .field("pins", &format_args!("{}", self.pins.len()))
            .field("withheld", &format_args!("{}", self.withheld.len()))
            .field("key_repaired", &self.key_repaired_at.is_some())
            .finish()
    }
}

impl std::fmt::Debug for ZaPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZaPin")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ZaWithheld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZaWithheld")
            .field("document_id", &self.document_id)
            .field(
                "instance_keys",
                &format_args!("{}", self.instances.keys.len()),
            )
            .finish_non_exhaustive()
    }
}

/// 64 random bits.
pub fn new_refusal_id() -> u64 {
    rand::random::<u64>()
}

/// Spec 7: `nonce || XChaCha20-Poly1305(len (u32 big endian) || rkyv
/// archive || zero padding to a multiple of PIN_PAD) || tag`, under MK with
/// `aad("pins", account_id)`. Nothing precedes the nonce.
pub fn seal_pin_record(record: &ZaPinRecord, mk: &Secret, account_id: u32) -> Vec<u8> {
    let mut archive = rkyv::to_bytes::<rkyv::rancor::Error>(record)
        .expect("rkyv serialization of an in-memory pin record cannot fail");
    // Capacity covers the padding, so `resize` never reallocates a copy.
    let mut padded = Zeroizing::new(Vec::with_capacity(4 + archive.len() + PIN_PAD));
    padded.extend_from_slice(&(archive.len() as u32).to_be_bytes());
    padded.extend_from_slice(&archive);
    archive.as_mut_slice().zeroize();
    let target = padded.len().div_ceil(PIN_PAD) * PIN_PAD;
    padded.resize(target, 0);
    seal(mk, &aad(AAD_PINS, account_id), &padded)
}

/// Opens a stored pin record. `None` is "unusable" (spec 4.4): it does not
/// open, does not decode, or its version is not `PIN_RECORD_VERSION`. The
/// decode is checked: opened bytes are copied into an aligned buffer, which
/// is zeroed after decoding.
pub fn open_pin_record(stored: &[u8], mk: &Secret, account_id: u32) -> Option<ZaPinRecord> {
    let padded = open(mk, &aad(AAD_PINS, account_id), stored).ok()?;
    let len = padded
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(|b| u32::from_be_bytes(b) as usize)?;
    let archive = padded.get(4..).and_then(|rest| rest.get(..len))?;
    let mut aligned = rkyv::util::AlignedVec::<16>::with_capacity(archive.len());
    aligned.extend_from_slice(archive);
    let decoded = rkyv::from_bytes::<ZaPinRecord, rkyv::rancor::Error>(&aligned).ok();
    aligned.as_mut_slice().zeroize();
    decoded.filter(|record| record.version == PIN_RECORD_VERSION)
}

impl ZaWithheld {
    fn is(&self, document_id: u32, uid: &str, address: &str) -> bool {
        self.document_id == document_id && self.uid == uid && self.address == address
    }
}

impl ZaPinRecord {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        ZaPinRecord {
            version: PIN_RECORD_VERSION,
            revision: 0,
            pins: Vec::new(),
            withheld: Vec::new(),
            key_repaired_at: None,
        }
    }

    pub fn pin(&self, address: &str) -> Option<&ZaPin> {
        self.pins.iter().find(|pin| pin.address == address)
    }

    /// Pins `public_key` for `address`, replacing any pin for it.
    pub fn set_pin(
        &mut self,
        address: &str,
        public_key: [u8; PUBLIC_KEY_LEN],
        source: PinSource,
        now: i64,
    ) {
        let pin = ZaPin {
            address: address.to_string(),
            public_key,
            pinned_at: now,
            source,
        };
        match self.pins.iter_mut().find(|pin| pin.address == address) {
            Some(existing) => *existing = pin,
            None => self.pins.push(pin),
        }
    }

    /// Returns whether a pin was removed.
    pub fn remove_pin(&mut self, address: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|pin| pin.address != address);
        self.pins.len() != before
    }

    pub fn entry(&self, document_id: u32, uid: &str, address: &str) -> Option<&ZaWithheld> {
        self.withheld
            .iter()
            .find(|entry| entry.is(document_id, uid, address))
    }

    /// The entries of one source event.
    pub fn entries_of<'a>(
        &'a self,
        document_id: u32,
        uid: &'a str,
    ) -> impl Iterator<Item = &'a ZaWithheld> {
        self.withheld
            .iter()
            .filter(move |entry| entry.document_id == document_id && entry.uid == uid)
    }

    /// Records a refusal: creates the entry with `since = now`, or re-arms
    /// the existing one. Either way the entry gets a fresh `refusal_id`,
    /// which is returned, and `instances` is unioned in.
    pub fn refuse(
        &mut self,
        document_id: u32,
        uid: &str,
        address: &str,
        now: i64,
        instances: Option<&ZaInstances>,
    ) -> u64 {
        let index = match self
            .withheld
            .iter()
            .position(|entry| entry.is(document_id, uid, address))
        {
            Some(index) => index,
            None => {
                self.withheld.push(ZaWithheld {
                    document_id,
                    uid: uid.to_string(),
                    address: address.to_string(),
                    since: now,
                    refusal_id: new_refusal_id(),
                    instances: ZaInstances::default(),
                });
                self.withheld.len() - 1
            }
        };
        let entry = &mut self.withheld[index];
        let mut refusal_id = new_refusal_id();
        while refusal_id == entry.refusal_id {
            refusal_id = new_refusal_id();
        }
        entry.refusal_id = refusal_id;
        if let Some(instances) = instances {
            entry.instances.union(instances);
        }
        refusal_id
    }

    /// Removes the entry only when its `refusal_id` is the one the delivery
    /// consumed (spec 5.2, phase 4). Returns whether it was removed.
    pub fn retire(&mut self, document_id: u32, uid: &str, address: &str, consumed: u64) -> bool {
        match self
            .withheld
            .iter()
            .position(|entry| entry.is(document_id, uid, address) && entry.refusal_id == consumed)
        {
            Some(index) => {
                self.withheld.remove(index);
                true
            }
            None => false,
        }
    }
}

impl ZaInstances {
    /// Adds an instance key; `None` is the master. A key past
    /// `INSTANCE_KEY_CAP` distinct keys sets `all` and clears `keys`, which
    /// stay empty after.
    pub fn add(&mut self, key: Option<(i64, bool)>) {
        match key {
            None => self.master = true,
            Some(_) if self.all => {}
            Some(key) if self.keys.contains(&key) => {}
            Some(_) if self.keys.len() >= INSTANCE_KEY_CAP => {
                self.all = true;
                self.keys.clear();
            }
            Some(key) => self.keys.push(key),
        }
    }

    pub fn union(&mut self, other: &ZaInstances) {
        self.master |= other.master;
        if other.all {
            self.all = true;
            self.keys.clear();
        }
        for key in &other.keys {
            self.add(Some(*key));
        }
    }

    /// Under `all` every qualifying instance is contained, the master
    /// included (spec 4.1).
    pub fn contains(&self, key: Option<(i64, bool)>) -> bool {
        match key {
            None => self.master || self.all,
            Some(key) => self.all || self.keys.contains(&key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{NONCE_LEN, Secret, TAG_LEN};

    /// Two pins and one entry with instances.
    fn sample_record() -> ZaPinRecord {
        let mut instances = ZaInstances::default();
        instances.add(None);
        instances.add(Some((1_700_000_000, false)));
        instances.add(Some((1_700_086_400, true)));
        ZaPinRecord {
            version: PIN_RECORD_VERSION,
            revision: 3,
            pins: vec![
                ZaPin {
                    address: "key2@example.com".into(),
                    public_key: [2; 32],
                    pinned_at: 100,
                    source: PinSource::FirstUse,
                },
                ZaPin {
                    address: "key3@example.com".into(),
                    public_key: [3; 32],
                    pinned_at: 200,
                    source: PinSource::Confirmed,
                },
            ],
            withheld: vec![ZaWithheld {
                document_id: 5,
                uid: "uid-1".into(),
                address: "key2@example.com".into(),
                since: 300,
                refusal_id: 42,
                instances,
            }],
            key_repaired_at: Some(400),
        }
    }

    /// `count` pins with 40-character addresses.
    fn record_with_pins(count: usize) -> ZaPinRecord {
        let mut record = ZaPinRecord::new();
        for i in 0..count {
            let address = format!("key2+{i:023}@example.com");
            assert_eq!(address.len(), 40);
            record.set_pin(&address, [i as u8; 32], PinSource::FirstUse, 100);
        }
        record
    }

    #[test]
    fn a_pin_record_round_trips_and_binds_account_and_mk() {
        let (mk, record) = (Secret::random(), sample_record());
        let sealed = seal_pin_record(&record, &mk, 7);
        assert_eq!(open_pin_record(&sealed, &mk, 7), Some(record.clone()));
        assert_eq!(
            open_pin_record(&sealed, &mk, 8),
            None,
            "another account's associated data"
        );
        assert_eq!(
            open_pin_record(&sealed, &Secret::random(), 7),
            None,
            "another MK"
        );
        assert_eq!(open_pin_record(&sealed[..sealed.len() - 1], &mk, 7), None);
        assert_eq!(open_pin_record(b"garbage", &mk, 7), None);
    }

    #[test]
    fn the_plaintext_pads_to_a_multiple_of_256_bytes() {
        let mk = Secret::random();
        assert_eq!(
            seal_pin_record(&ZaPinRecord::new(), &mk, 7).len(),
            NONCE_LEN + 256 + TAG_LEN
        );
        let big = record_with_pins(40);
        let padded = seal_pin_record(&big, &mk, 7).len() - NONCE_LEN - TAG_LEN;
        assert!(padded > 256 && padded % 256 == 0, "{padded}");
    }

    #[test]
    fn an_unknown_version_is_unusable() {
        let mk = Secret::random();
        let mut record = sample_record();
        record.version = 2;
        assert_eq!(
            open_pin_record(&seal_pin_record(&record, &mk, 7), &mk, 7),
            None
        );
    }

    #[test]
    fn every_refusal_changes_the_refusal_id_and_retirement_needs_it() {
        let mut r = ZaPinRecord::new();
        let first = r.refuse(5, "u", "bob@example.com", 100, None);
        let second = r.refuse(5, "u", "bob@example.com", 200, None);
        assert_ne!(first, second);
        assert_eq!(
            (
                r.withheld.len(),
                r.withheld[0].since,
                r.withheld[0].refusal_id
            ),
            (1, 100, second)
        );
        assert!(
            !r.retire(5, "u", "bob@example.com", first),
            "a re-armed entry stays"
        );
        assert!(r.retire(5, "u", "bob@example.com", second));
        r.refuse(5, "u", "bob@example.com", 1, None);
        r.refuse(6, "u", "bob@example.com", 1, None);
        r.refuse(5, "v", "bob@example.com", 1, None);
        assert_eq!(
            r.withheld.len(),
            3,
            "identity is (document_id, uid, address)"
        );
    }

    #[test]
    fn refused_instances_union_and_switch_to_all_past_64_keys() {
        let (mut a, mut b) = (ZaInstances::default(), ZaInstances::default());
        a.add(Some((10, false)));
        b.add(Some((20, true)));
        b.add(None);
        a.union(&b);
        assert!(
            a.master && a.contains(Some((10, false))) && a.contains(Some((20, true))) && !a.all
        );
        let mut c = ZaInstances::default();
        c.add(None);
        for i in 0..64 {
            c.add(Some((i, false)));
        }
        assert!(!c.all && c.keys.len() == 64, "the master does not count");
        c.add(Some((64, false)));
        assert!(c.all && c.keys.is_empty() && c.contains(Some((999, true))));
        assert!(c.contains(None));
        assert!(
            ZaInstances {
                master: false,
                keys: vec![],
                all: true
            }
            .contains(None),
            "all includes the master"
        );
        c.add(Some((65, false)));
        assert!(c.keys.is_empty(), "keys stay empty once all is set");
    }

    #[test]
    fn pin_structs_print_no_address() {
        let text = format!(
            "{:?} {:?} {:?}",
            sample_record(),
            sample_record().pins[0],
            sample_record().withheld[0]
        );
        assert!(!text.contains("example") && !text.contains('@'), "{text}");
    }
}
