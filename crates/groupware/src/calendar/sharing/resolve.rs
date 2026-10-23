/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

#[cfg(test)]
use super::format::{GrantRingHeader, OwnerRingHeader};
use super::format::{
    open_grant_ring, open_owner_ring, parse_grant_header, parse_owner_header,
    za_replace_calendar_entries,
};
use crate::calendar::{
    Calendar, CalendarEvent,
    seal::{COLLECTION_MARKER, SealError, ZaKeyDomain},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{Server, auth::AccessToken};
use registry::schema::enums::Permission;
use std::{collections::BTreeMap, sync::Arc};
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use types::{
    acl::{Acl, AclGrant},
    collection::Collection,
    field::Field,
};
use utils::map::bitmap::Bitmap;
use vault::{
    keys::Secret,
    session::{CollectionKeyId, SessionKeys},
};

/// A resolver failure keeps access refusal separate from corruption and store errors.
#[derive(Debug)]
pub enum ZaCalendarError {
    Forbidden,
    LinkedEvent,
    Seal(SealError),
    Store(trc::Error),
}

impl From<SealError> for ZaCalendarError {
    fn from(value: SealError) -> Self {
        Self::Seal(value)
    }
}
impl From<trc::Error> for ZaCalendarError {
    fn from(value: trc::Error) -> Self {
        Self::Store(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZaSyncBoundary {
    pub grant_change_id: u64,
    pub rename_change_id: u64,
    pub pending: bool,
}

/// The original stored calendar is an assertion source, never a response view.
pub struct ZaCalendarContext {
    pub owner_id: u32,
    pub calendar_id: u32,
    pub personal_id: u32,
    pub stored: Archive<AlignedBytes>,
    pub domain: ZaKeyDomain,
    pub boundary: Option<ZaSyncBoundary>,
}

impl ZaCalendarContext {
    pub fn assert_in(&self, batch: &mut BatchBuilder) {
        batch
            .with_account_id(self.owner_id)
            .with_collection(Collection::Calendar)
            .with_document(self.calendar_id)
            .assert_value(Field::ARCHIVE, &self.stored);
    }

    /// Recheck operation bits even when a read adapter reuses this request's
    /// context. This does not perform a new archive read and is never for writes.
    pub fn check_access(
        &self,
        token: &AccessToken,
        required: Bitmap<Acl>,
    ) -> Result<(), ZaCalendarError> {
        let calendar = self.stored.deserialize::<Calendar>()?;
        authorize_calendar(token, self.owner_id, &calendar, required)
    }
}

/// Only the authenticated principal's own grant can make a foreign calendar visible.
pub fn za_foreign_calendar_eligible(token: &AccessToken, grants: &[AclGrant]) -> bool {
    !token.za_is_impersonated()
        && !token.has_permission(Permission::Impersonate)
        && token
            .session_keys()
            .is_some_and(|keys| keys.account_id == token.account_id())
        && grants
            .iter()
            .any(|grant| grant.account_id == token.account_id() && grant.grants.contains(Acl::Read))
}

fn forbid(owner_id: u32) -> ZaCalendarError {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: calendar access without a collection key"
    );
    ZaCalendarError::Forbidden
}

fn session<'a>(token: &'a AccessToken, owner_id: u32) -> Result<&'a SessionKeys, ZaCalendarError> {
    token
        .session_keys()
        .filter(|keys| keys.account_id == token.account_id() && !token.za_is_impersonated())
        .map(Arc::as_ref)
        .ok_or_else(|| forbid(owner_id))
}

fn authorize_calendar(
    token: &AccessToken,
    owner_id: u32,
    calendar: &Calendar,
    required: Bitmap<Acl>,
) -> Result<(), ZaCalendarError> {
    session(token, owner_id)?;
    if token.account_id() == owner_id {
        return Ok(());
    }
    if !za_foreign_calendar_eligible(token, &calendar.acls)
        || !calendar.acls.iter().any(|grant| {
            grant.account_id == token.account_id()
                && grant.grants.bitmap & required.bitmap == required.bitmap
        })
        || !calendar
            .preferences
            .iter()
            .any(|entry| entry.account_id == token.account_id())
    {
        return Err(forbid(owner_id));
    }
    Ok(())
}

async fn read_calendar(
    server: &Server,
    owner_id: u32,
    calendar_id: u32,
) -> Result<Archive<AlignedBytes>, ZaCalendarError> {
    #[cfg(feature = "test_mode")]
    super::test::record_read(owner_id, calendar_id);
    server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            owner_id,
            Collection::Calendar,
            calendar_id,
        ))
        .await?
        .ok_or_else(|| {
            ZaCalendarError::Store(
                trc::StoreEvent::NotFound
                    .into_err()
                    .account_id(owner_id)
                    .document_id(calendar_id),
            )
        })
}

/// Fresh resolution for a write or a standalone read. Read response adapters may
/// keep the result for their request; mutation callers must call this anew.
pub async fn za_calendar_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
) -> Result<Option<ZaCalendarContext>, ZaCalendarError> {
    if !server.za_is_key_account(owner_id).await? {
        return Ok(None);
    }
    let keys = session(token, owner_id)?;
    if token.account_id() != owner_id && !token.has_shared_access(owner_id, Collection::Calendar) {
        return Err(forbid(owner_id));
    }
    let stored = read_calendar(server, owner_id, calendar_id).await?;
    // Authorization precedes even private-key self-check and any cache lookup.
    let calendar = stored.deserialize::<Calendar>()?;
    authorize_calendar(token, owner_id, &calendar, required)?;
    let private = if token.account_id() != owner_id {
        Some(server.za_private_key(keys).await?)
    } else {
        None
    };
    resolve_stored_calendar(stored, token, owner_id, calendar_id, required, private).map(Some)
}

pub async fn za_owned_calendar_context(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
) -> Result<ZaCalendarContext, ZaCalendarError> {
    let stored = read_calendar(server, keys.account_id, calendar_id).await?;
    resolve_owner_stored(stored, keys, calendar_id)
}

fn owner_parts(
    calendar: &Calendar,
    owner_id: u32,
    calendar_id: u32,
) -> Result<Option<Vec<&str>>, ZaCalendarError> {
    // Untouched defaults can precede the account's keys and can have no entry.
    // That exception cannot carry retained sharing metadata.
    if calendar.preferences.is_empty() {
        return if calendar.acls.is_empty() {
            Ok(None)
        } else {
            Err(SealError::Structure("unshared calendar ACLs").into())
        };
    }
    let owner = calendar
        .preferences
        .iter()
        .find(|entry| entry.account_id == owner_id)
        .ok_or(SealError::Structure("owner entry"))?;
    let Some(name) = owner.name.strip_prefix(COLLECTION_MARKER) else {
        return if calendar.preferences.len() == 1 && calendar.acls.is_empty() {
            Ok(None)
        } else {
            Err(SealError::Structure("unshared calendar entries").into())
        };
    };
    let parts = name.split('|').collect::<Vec<_>>();
    match parts.len() {
        2 => {
            // Part count alone cannot distinguish a legacy EWK entry from a
            // truncated shared ring or a legacy entry retaining foreign grants.
            validate_sealed_entries(calendar, owner_id, calendar_id)?;
            Ok(None)
        }
        5 => Ok(Some(parts)),
        _ => Err(SealError::Format.into()),
    }
}

fn validate_sealed_entries(
    calendar: &Calendar,
    owner_id: u32,
    calendar_id: u32,
) -> Result<(), ZaCalendarError> {
    let mut checked = calendar.clone();
    za_replace_calendar_entries(
        &mut checked,
        owner_id,
        calendar_id,
        calendar.preferences.clone(),
        calendar.acls.clone(),
    )?;
    Ok(())
}

fn resolve_owner_stored(
    stored: Archive<AlignedBytes>,
    keys: &SessionKeys,
    calendar_id: u32,
) -> Result<ZaCalendarContext, ZaCalendarError> {
    let owner_id = keys.account_id;
    let calendar = stored.deserialize::<Calendar>()?;
    let (domain, boundary) = if let Some(parts) = owner_parts(&calendar, owner_id, calendar_id)? {
        validate_sealed_entries(&calendar, owner_id, calendar_id)?;
        let ring_bytes = STANDARD.decode(parts[0]).map_err(|_| SealError::Decode)?;
        // Fresh encrypted journal/ledger state is always authenticated.
        let (header, ring) = open_owner_ring(&ring_bytes, owner_id, calendar_id, keys.ewk())?;
        // A new grant can be pending while the owner's rename flag is clear.
        // Expose that fact for the later explicit owner repair adapter, without
        // opening any grant or performing a write here.
        let pending = calendar
            .preferences
            .iter()
            .filter(|entry| entry.account_id != owner_id)
            .try_fold(header.boundary_pending, |pending, entry| {
                let encoded = entry
                    .name
                    .strip_prefix(COLLECTION_MARKER)
                    .ok_or(SealError::NotSealed)?
                    .split('|')
                    .next()
                    .ok_or(SealError::Format)?;
                let bytes = STANDARD.decode(encoded).map_err(|_| SealError::Decode)?;
                Ok::<_, SealError>(parse_grant_header(&bytes)?.boundary_pending || pending)
            })?;
        let opened = header
            .generations
            .iter()
            .copied()
            .zip(ring.keys.iter().copied().map(Secret::from_bytes))
            .collect::<BTreeMap<_, _>>();
        for (&generation, key) in &opened {
            keys.cache_collection_key(
                CollectionKeyId {
                    owner_id,
                    calendar_id,
                    domain_id: header.domain_id,
                    generation,
                },
                key.clone(),
            );
        }
        let domain = ZaKeyDomain::shared(
            owner_id,
            owner_id,
            Some(keys.ewk().clone()),
            calendar_id,
            header.domain_id,
            opened,
            ring.reverting,
        )?;
        (
            domain,
            Some(ZaSyncBoundary {
                grant_change_id: 0,
                rename_change_id: header.rename_change_id,
                pending,
            }),
        )
    } else {
        (ZaKeyDomain::owner(keys), None)
    };
    Ok(ZaCalendarContext {
        owner_id,
        calendar_id,
        personal_id: owner_id,
        stored,
        domain,
        boundary,
    })
}

fn resolve_stored_calendar(
    stored: Archive<AlignedBytes>,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
    private: Option<&Secret>,
) -> Result<ZaCalendarContext, ZaCalendarError> {
    let calendar = stored.deserialize::<Calendar>()?;
    authorize_calendar(token, owner_id, &calendar, required)?;
    let keys = session(token, owner_id)?;
    if token.account_id() == owner_id {
        return resolve_owner_stored(stored, keys, calendar_id);
    }
    let Some(parts) = owner_parts(&calendar, owner_id, calendar_id)? else {
        return Err(forbid(owner_id));
    };
    validate_sealed_entries(&calendar, owner_id, calendar_id)?;
    let owner_ring = STANDARD.decode(parts[0]).map_err(|_| SealError::Decode)?;
    let owner_header = parse_owner_header(&owner_ring)?;
    let entry = calendar
        .preferences
        .iter()
        .find(|entry| entry.account_id == token.account_id())
        .ok_or_else(|| forbid(owner_id))?;
    let ring = STANDARD
        .decode(
            entry
                .name
                .strip_prefix(COLLECTION_MARKER)
                .ok_or(SealError::NotSealed)?
                .split('|')
                .next()
                .ok_or(SealError::Format)?,
        )
        .map_err(|_| SealError::Decode)?;
    let header = parse_grant_header(&ring)?;
    // Entry validation checked all headers and envelopes before this cache hit.
    let ids = header
        .generations
        .iter()
        .map(|&generation| CollectionKeyId {
            owner_id,
            calendar_id,
            domain_id: owner_header.domain_id,
            generation,
        })
        .collect::<Vec<_>>();
    let opened = match keys.authenticated_collection_keys(&ids, &ring) {
        Some(opened) => opened,
        None => {
            let (_, opened) = open_grant_ring(
                &ring,
                owner_id,
                calendar_id,
                token.account_id(),
                owner_header.domain_id,
                private.ok_or(SealError::Aead)?,
            )?;
            keys.cache_authenticated_collection_keys(&ids, &ring, &opened);
            opened
        }
    };
    let domain = ZaKeyDomain::shared(
        owner_id,
        token.account_id(),
        None,
        calendar_id,
        owner_header.domain_id,
        opened,
        false,
    )?;
    Ok(ZaCalendarContext {
        owner_id,
        calendar_id,
        personal_id: token.account_id(),
        stored,
        domain,
        boundary: Some(ZaSyncBoundary {
            grant_change_id: header.grant_change_id,
            rename_change_id: header.rename_change_id,
            pending: header.boundary_pending,
        }),
    })
}

pub async fn za_event_contexts(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    event: &CalendarEvent,
    addressed_calendar: u32,
    required: Bitmap<Acl>,
) -> Result<Vec<ZaCalendarContext>, ZaCalendarError> {
    if !server.za_is_key_account(owner_id).await? {
        return Ok(Vec::new());
    }
    let parents = event_parents(event, addressed_calendar)?;
    let mut contexts = Vec::with_capacity(parents.len());
    for parent in parents {
        if let Some(context) =
            za_calendar_context(server, token, owner_id, parent, required).await?
        {
            contexts.push(context);
        }
    }
    check_event_domains(contexts, event.names.len() > 1)
}

pub async fn za_owned_event_contexts(
    server: &Server,
    keys: &SessionKeys,
    event: &CalendarEvent,
    addressed_calendar: u32,
) -> Result<Vec<ZaCalendarContext>, ZaCalendarError> {
    let parents = event_parents(event, addressed_calendar)?;
    let mut contexts = Vec::with_capacity(parents.len());
    for parent in parents {
        contexts.push(za_owned_calendar_context(server, keys, parent).await?);
    }
    check_event_domains(contexts, event.names.len() > 1)
}

fn event_parents(event: &CalendarEvent, addressed: u32) -> Result<Vec<u32>, ZaCalendarError> {
    let parents = event
        .names
        .iter()
        .map(|name| name.parent_id)
        .collect::<std::collections::BTreeSet<_>>();
    if !parents.contains(&addressed) {
        return Err(ZaCalendarError::LinkedEvent);
    }
    Ok(parents.into_iter().collect())
}
fn check_event_domains(
    contexts: Vec<ZaCalendarContext>,
    linked: bool,
) -> Result<Vec<ZaCalendarContext>, ZaCalendarError> {
    if linked
        && contexts
            .iter()
            .any(|context| context.domain.calendar_id().is_some())
    {
        Err(ZaCalendarError::LinkedEvent)
    } else {
        Ok(contexts)
    }
}

#[cfg(test)]
mod tests {
    use super::super::format::{ZaGrant, ZaOwnerRing, seal_grant_ring, seal_owner_ring};
    use super::*;
    use crate::calendar::{
        CalendarPreferences,
        seal::collection::{seal_personal_entry, seal_shared_properties},
    };
    use store::{Deserialize, Serialize, write::Archiver};
    use vault::keys::derive_public_key;

    const OWNER: u32 = 8301;
    const READER: u32 = 8302;
    const CALENDAR: u32 = 8303;

    fn token(id: u32, keys: Arc<SessionKeys>, impersonate: bool) -> AccessToken {
        AccessToken::from_permissions(
            id,
            if impersonate {
                vec![Permission::Impersonate]
            } else {
                vec![]
            },
        )
        .with_session_keys(keys)
    }

    fn archive(calendar: &Calendar) -> Archive<AlignedBytes> {
        <Archive<AlignedBytes> as Deserialize>::deserialize(
            &Archiver::new(calendar.clone()).serialize().unwrap(),
        )
        .unwrap()
    }

    fn fixture(domain_id: [u8; 16]) -> (Arc<SessionKeys>, Arc<SessionKeys>, Secret, Calendar) {
        let owner = Arc::new(SessionKeys::new(OWNER, 1, Secret::from_bytes([31; 32])));
        let reader = Arc::new(SessionKeys::new(READER, 1, Secret::from_bytes([32; 32])));
        let private = Secret::from_bytes([33; 32]);
        let public = derive_public_key(&private);
        let keys = BTreeMap::from([
            (1, Secret::from_bytes([34; 32])),
            (2, Secret::from_bytes([35; 32])),
        ]);
        let header = OwnerRingHeader {
            calendar_id: CALENDAR,
            domain_id,
            rename_change_id: 40,
            boundary_pending: false,
            generations: vec![1, 2],
        };
        let ring = ZaOwnerRing {
            version: 1,
            keys: vec![[34; 32], [35; 32]],
            converted: true,
            reverting: false,
            grants: vec![ZaGrant {
                account_id: READER,
                address: "reader@example.org".into(),
                public_key: public,
                level: 1,
                granted_at: 1,
                change_id: 30,
            }],
        };
        let owner_ring = seal_owner_ring(&header, &ring, OWNER, owner.ewk()).unwrap();
        let grant = GrantRingHeader {
            calendar_id: CALENDAR,
            domain_id,
            grantee_id: READER,
            grant_change_id: 30,
            rename_change_id: 40,
            boundary_pending: false,
            generations: vec![1, 2],
        };
        let grant_ring = seal_grant_ring(&grant, &keys, OWNER, &public).unwrap();
        let domain = ZaKeyDomain::shared(
            OWNER,
            OWNER,
            Some(owner.ewk().clone()),
            CALENDAR,
            domain_id,
            keys,
            false,
        )
        .unwrap();
        let mut own = seal_personal_entry(
            &CalendarPreferences {
                account_id: OWNER,
                name: "Owner private name".into(),
                ..Default::default()
            },
            &domain,
            &owner_ring,
        )
        .unwrap();
        let (envelope, bundle) = seal_shared_properties(&Default::default(), &domain).unwrap();
        own.name.push_str(&format!("|{envelope}|{bundle}"));
        let grantee = seal_personal_entry(
            &CalendarPreferences {
                account_id: READER,
                name: "Reader private name".into(),
                ..Default::default()
            },
            &domain,
            &grant_ring,
        )
        .unwrap();
        let calendar = Calendar {
            name: "shared".into(),
            preferences: vec![own, grantee],
            acls: vec![AclGrant {
                account_id: READER,
                grants: Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems),
            }],
            ..Default::default()
        };
        (owner, reader, private, calendar)
    }

    #[test]
    fn resolver_requires_own_grant_and_all_bits() {
        let (owner, reader, _, calendar) = fixture([51; 16]);
        let required = Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems);
        let reader_token = token(READER, reader.clone(), false);
        assert!(authorize_calendar(&reader_token, OWNER, &calendar, required).is_ok());
        assert!(matches!(
            authorize_calendar(
                &reader_token,
                OWNER,
                &calendar,
                required.with_item(Acl::ModifyItems)
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        let mut group = calendar.clone();
        group.acls[0].account_id = 8399;
        assert!(!za_foreign_calendar_eligible(&reader_token, &group.acls));
        assert!(matches!(
            authorize_calendar(&reader_token, OWNER, &group, required),
            Err(ZaCalendarError::Forbidden)
        ));
        assert!(!za_foreign_calendar_eligible(
            &reader_token.clone().with_za_impersonation(),
            &calendar.acls
        ));
        assert!(!za_foreign_calendar_eligible(
            &token(READER, reader.clone(), true),
            &calendar.acls
        ));
        assert!(matches!(
            authorize_calendar(
                &token(READER, owner.clone(), false),
                OWNER,
                &calendar,
                required
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        assert!(
            authorize_calendar(
                &token(OWNER, owner.clone(), true),
                OWNER,
                &calendar,
                required
            )
            .is_ok()
        );
        assert!(matches!(
            authorize_calendar(
                &token(OWNER, owner, false).with_za_impersonation(),
                OWNER,
                &calendar,
                required
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        assert!(!za_foreign_calendar_eligible(
            &reader_token.without_session_keys(),
            &calendar.acls
        ));
    }

    #[test]
    fn resolver_checks_fresh_ring_before_cache() {
        let (owner, reader, private, calendar) = fixture([61; 16]);
        let token = token(READER, reader, false);
        let required = Bitmap::new().with_item(Acl::ReadItems);
        let first = resolve_stored_calendar(
            archive(&calendar),
            &token,
            OWNER,
            CALENDAR,
            required,
            Some(&private),
        )
        .unwrap();
        assert_eq!(first.domain.domain_id(), Some(&[61; 16]));
        assert!(matches!(
            first.check_access(&token, required.with_item(Acl::ModifyItems)),
            Err(ZaCalendarError::Forbidden)
        ));
        // An unchanged authenticated ring needs no second private-key unwrap.
        assert!(
            resolve_stored_calendar(
                archive(&calendar),
                &token,
                OWNER,
                CALENDAR,
                required,
                Some(&Secret::random())
            )
            .is_ok()
        );
        for record_tail in [45 + 4 + 32 + 72 - 1, 45 + 2 * (4 + 32 + 72) - 1] {
            let mut corrupt = calendar.clone();
            let mut parts = corrupt.preferences[1]
                .name
                .strip_prefix(COLLECTION_MARKER)
                .unwrap()
                .split('|')
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut ring = STANDARD.decode(&parts[0]).unwrap();
            ring[record_tail] ^= 1;
            parts[0] = STANDARD.encode(ring);
            corrupt.preferences[1].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
            assert!(matches!(
                resolve_stored_calendar(
                    archive(&corrupt),
                    &token,
                    OWNER,
                    CALENDAR,
                    required,
                    Some(&private)
                ),
                Err(ZaCalendarError::Seal(_))
            ));
        }
        let mut missing = calendar.clone();
        missing.preferences.pop();
        assert!(matches!(
            resolve_stored_calendar(
                archive(&missing),
                &token,
                OWNER,
                CALENDAR,
                required,
                Some(&private)
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        let mut revoked = calendar.clone();
        revoked.acls.clear();
        assert!(matches!(
            resolve_stored_calendar(
                archive(&revoked),
                &token,
                OWNER,
                CALENDAR,
                required,
                Some(&private)
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        let (_, _, _, next) = fixture([62; 16]);
        let own_token = token_fn(OWNER, owner);
        let next =
            resolve_stored_calendar(archive(&next), &own_token, OWNER, CALENDAR, required, None)
                .unwrap();
        assert_eq!(next.domain.domain_id(), Some(&[62; 16]));
    }

    #[test]
    fn owner_refreshes_journal_and_authenticates_warm_ring() {
        let (owner, _, _, mut calendar) = fixture([71; 16]);
        let token = token_fn(OWNER, owner.clone());
        resolve_stored_calendar(
            archive(&calendar),
            &token,
            OWNER,
            CALENDAR,
            Bitmap::new(),
            None,
        )
        .unwrap();
        calendar.preferences.truncate(1);
        calendar.acls.clear();
        let mut parts = calendar.preferences[0]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap()
            .split('|')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let bytes = STANDARD.decode(&parts[0]).unwrap();
        let (header, mut ring) = open_owner_ring(&bytes, OWNER, CALENDAR, owner.ewk()).unwrap();
        ring.converted = false;
        ring.reverting = true;
        ring.grants.clear();
        parts[0] = STANDARD.encode(seal_owner_ring(&header, &ring, OWNER, owner.ewk()).unwrap());
        calendar.preferences[0].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
        let fresh = resolve_stored_calendar(
            archive(&calendar),
            &token,
            OWNER,
            CALENDAR,
            Bitmap::new(),
            None,
        )
        .unwrap();
        assert!(fresh.domain.is_reverting());
        let mut bytes = STANDARD.decode(&parts[0]).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        parts[0] = STANDARD.encode(bytes);
        calendar.preferences[0].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
        assert!(matches!(
            resolve_stored_calendar(
                archive(&calendar),
                &token,
                OWNER,
                CALENDAR,
                Bitmap::new(),
                None
            ),
            Err(ZaCalendarError::Seal(_))
        ));
    }

    #[test]
    fn owner_notices_a_pending_grant_with_a_clear_owner_header() {
        let (owner, _, _, mut calendar) = fixture([91; 16]);
        let clear = resolve_owner_stored(archive(&calendar), &owner, CALENDAR).unwrap();
        assert_eq!(clear.boundary.unwrap().pending, false);
        let mut parts = calendar.preferences[1]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap()
            .split('|')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut grant = STANDARD.decode(&parts[0]).unwrap();
        grant[42] = 1;
        grant[26..34].fill(0);
        parts[0] = STANDARD.encode(grant);
        calendar.preferences[1].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
        let pending = resolve_owner_stored(archive(&calendar), &owner, CALENDAR).unwrap();
        assert!(pending.boundary.unwrap().pending);
        assert_eq!(pending.boundary.unwrap().grant_change_id, 0);
        assert_eq!(pending.boundary.unwrap().rename_change_id, 40);
    }

    #[test]
    fn resolved_domains_keep_stale_and_foreign_envelopes_unreadable() {
        use crate::calendar::seal::{seal_event_in_domain, unseal_event_in_domain};
        use calcard::{Entry, Parser};
        let (owner, _, _, calendar) = fixture([81; 16]);
        owner.cache_collection_key(
            CollectionKeyId {
                owner_id: OWNER,
                calendar_id: CALENDAR,
                domain_id: [81; 16],
                generation: 3,
            },
            Secret::from_bytes([99; 32]),
        );
        let token = token_fn(OWNER, owner);
        let context = resolve_stored_calendar(
            archive(&calendar),
            &token,
            OWNER,
            CALENDAR,
            Bitmap::new(),
            None,
        )
        .unwrap();
        let Entry::ICalendar(tree) = Parser::new("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:resolver-envelope\r\nDTSTART:20240101T090000Z\r\nSUMMARY:Private summary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").entry() else { panic!() };
        let mut event = CalendarEvent::default();
        event.names.push(common::DavName {
            name: "event.ics".into(),
            parent_id: CALENDAR,
        });
        event.data.event = tree;
        seal_event_in_domain(&mut event, &context.domain, None).unwrap();
        for (offset, bytes) in [
            (2usize, (CALENDAR + 1).to_be_bytes()),
            (6, 3u32.to_be_bytes()),
        ] {
            let mut damaged = event.clone();
            let key = damaged.data.event.components[0]
                .entries
                .iter_mut()
                .find(|entry| crate::calendar::seal::tree::is_carrier(entry, "X-ZA-KEY"))
                .unwrap();
            let calcard::icalendar::ICalendarValue::Text(encoded) = &mut key.values[0] else {
                panic!()
            };
            let mut envelope = STANDARD.decode(&*encoded).unwrap();
            envelope[offset..offset + 4].copy_from_slice(&bytes);
            *encoded = STANDARD.encode(envelope);
            assert!(unseal_event_in_domain(&mut damaged, &context.domain).is_err());
        }
        let (_, _, _, next) = fixture([82; 16]);
        let next =
            resolve_stored_calendar(archive(&next), &token, OWNER, CALENDAR, Bitmap::new(), None)
                .unwrap();
        assert!(unseal_event_in_domain(&mut event.clone(), &next.domain).is_err());
        let mut linked = event.clone();
        linked.names.push(common::DavName {
            name: "other.ics".into(),
            parent_id: CALENDAR,
        });
        assert_eq!(event_parents(&linked, CALENDAR).unwrap(), vec![CALENDAR]);
        assert!(matches!(
            event_parents(&linked, CALENDAR + 1),
            Err(ZaCalendarError::LinkedEvent)
        ));
        assert!(matches!(
            check_event_domains(vec![context], true),
            Err(ZaCalendarError::LinkedEvent)
        ));
    }

    fn legacy_calendar(owner: &SessionKeys) -> Calendar {
        let mut calendar = Calendar {
            name: "default".into(),
            preferences: vec![CalendarPreferences {
                account_id: OWNER,
                name: "Legacy owner display".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        crate::calendar::seal::seal_calendar(&mut calendar, owner, OWNER).unwrap();
        calendar
    }

    #[test]
    fn resolver_refuses_legacy_entry_with_retained_sharing_metadata() {
        let (owner, reader, private, mut calendar) = fixture([101; 16]);
        let legacy = legacy_calendar(&owner);
        calendar.preferences[0] = legacy.preferences[0].clone();
        let own_token = token_fn(OWNER, owner.clone());
        let reader_token = token_fn(READER, reader);
        for principal in [&own_token, &reader_token] {
            assert!(matches!(
                resolve_stored_calendar(
                    archive(&calendar),
                    principal,
                    OWNER,
                    CALENDAR,
                    Bitmap::new().with_item(Acl::Read),
                    Some(&private)
                ),
                Err(ZaCalendarError::Seal(_))
            ));
        }
        // An absent own grant is still a refusal before any malformed shape opens.
        let mut no_grant = calendar.clone();
        no_grant.acls.clear();
        assert!(matches!(
            resolve_stored_calendar(
                archive(&no_grant),
                &reader_token,
                OWNER,
                CALENDAR,
                Bitmap::new().with_item(Acl::Read),
                Some(&private)
            ),
            Err(ZaCalendarError::Forbidden)
        ));
        calendar.preferences.truncate(1);
        assert!(matches!(
            resolve_owner_stored(archive(&calendar), &owner, CALENDAR),
            Err(ZaCalendarError::Seal(_))
        ));
        let valid = resolve_owner_stored(archive(&legacy), &owner, CALENDAR).unwrap();
        assert!(valid.domain.calendar_id().is_none());
        let view =
            crate::calendar::seal::calendar_view_in_domain(&valid.stored, &valid.domain, OWNER)
                .unwrap();
        assert_eq!(
            view.deserialize::<Calendar>().unwrap().preferences[0].name,
            "Legacy owner display"
        );
    }

    #[test]
    fn resolver_refuses_truncated_owner_ring_and_malformed_legacy_carriers() {
        let (owner, _, _, mut calendar) = fixture([102; 16]);
        calendar.preferences.truncate(1);
        calendar.acls.clear();
        let parts = calendar.preferences[0]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap()
            .split('|')
            .collect::<Vec<_>>();
        calendar.preferences[0].name = format!("{COLLECTION_MARKER}{}|{}", parts[0], parts[2]);
        assert!(matches!(
            resolve_owner_stored(archive(&calendar), &owner, CALENDAR),
            Err(ZaCalendarError::Seal(_))
        ));

        let legacy = legacy_calendar(&owner);
        let parts = legacy.preferences[0]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap()
            .split('|')
            .collect::<Vec<_>>();
        let mut wrong_type = legacy.clone();
        let mut envelope = STANDARD.decode(parts[0]).unwrap();
        envelope[1] = 4;
        wrong_type.preferences[0].name = format!(
            "{COLLECTION_MARKER}{}|{}",
            STANDARD.encode(envelope),
            parts[1]
        );
        let mut bad_bundle = legacy.clone();
        bad_bundle.preferences[0].name = format!("{COLLECTION_MARKER}{}|not-base64", parts[0]);
        let mut bad_envelope = legacy.clone();
        bad_envelope.preferences[0].name = format!(
            "{COLLECTION_MARKER}{}|{}",
            STANDARD.encode([1, 1]),
            parts[1]
        );
        let mut bad_timezone = legacy;
        let calcard::Entry::ICalendar(tree) = calcard::Parser::new("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nCOMMENT:unsealed timezone detail\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n").entry() else { panic!() };
        bad_timezone.preferences[0].time_zone = crate::calendar::Timezone::Custom(tree);
        for malformed in [wrong_type, bad_bundle, bad_envelope, bad_timezone] {
            assert!(matches!(
                resolve_owner_stored(archive(&malformed), &owner, CALENDAR),
                Err(ZaCalendarError::Seal(_))
            ));
        }
    }

    #[test]
    fn resolver_preserves_empty_and_single_owner_unmarked_defaults() {
        let owner = SessionKeys::new(OWNER, 1, Secret::random());
        let empty = Calendar {
            name: "default".into(),
            ..Default::default()
        };
        let single = Calendar {
            name: "default".into(),
            preferences: vec![CalendarPreferences {
                account_id: OWNER,
                name: "Default (owner@example.org)".into(),
                description: Some("existing plaintext value".into()),
                sort_order: 7,
                ..Default::default()
            }],
            ..Default::default()
        };
        for calendar in [empty, single] {
            let stored = archive(&calendar);
            let original_bytes = stored.as_bytes().to_vec();
            let context = resolve_owner_stored(stored, &owner, CALENDAR).unwrap();
            assert!(context.domain.calendar_id().is_none());
            assert!(context.boundary.is_none());
            assert_eq!(context.stored.as_bytes(), original_bytes);
            let view = crate::calendar::seal::calendar_view_in_domain(
                &context.stored,
                &context.domain,
                OWNER,
            )
            .unwrap();
            assert_eq!(view.deserialize::<Calendar>().unwrap(), calendar);
        }
    }

    #[test]
    fn resolver_rejects_unmarked_forms_with_foreign_entries_or_acls() {
        let (owner, reader, private, mut mixed) = fixture([103; 16]);
        mixed.preferences[0].name = "Default (owner@example.org)".into();
        let reader_token = token_fn(READER, reader);
        assert!(matches!(
            resolve_stored_calendar(
                archive(&mixed),
                &reader_token,
                OWNER,
                CALENDAR,
                Bitmap::new().with_item(Acl::Read),
                Some(&private)
            ),
            Err(ZaCalendarError::Seal(_))
        ));
        let mut foreign_without_acl = mixed.clone();
        foreign_without_acl.acls.clear();
        let mut owner_with_acl = mixed.clone();
        owner_with_acl.preferences.truncate(1);
        let mut empty_with_acl = mixed.clone();
        empty_with_acl.preferences.clear();
        let mut no_owner = mixed.clone();
        no_owner.preferences.remove(0);
        no_owner.acls.clear();
        for calendar in [
            mixed,
            foreign_without_acl,
            owner_with_acl,
            empty_with_acl,
            no_owner,
        ] {
            assert!(matches!(
                resolve_owner_stored(archive(&calendar), &owner, CALENDAR),
                Err(ZaCalendarError::Seal(_))
            ));
        }
    }

    fn token_fn(id: u32, keys: Arc<SessionKeys>) -> AccessToken {
        token(id, keys, false)
    }
}
