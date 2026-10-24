/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Fresh password-verified calendar sharing. Business refusals never count as auth failures.
use super::{
    contacts::za_resolve_key_account,
    vault::{
        CredentialsRequest, ZaVerified, bad_request, conflict, deserialize_some, json,
        za_reread_verified, za_verify_primary,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{
    Server,
    auth::vault::{ZaMark, za_normalize_address},
    cache::invalidate::CacheInvalidationBuilder,
    ipc::CacheInvalidation,
};
use groupware::{
    DavResourceName, RFC_3986,
    calendar::{
        Calendar, SCHEDULE_INBOX_ID, SCHEDULE_OUTBOX_ID,
        seal::{COLLECTION_MARKER, SealError, calendar_view_in_domain, seal_error},
        sharing::{
            boundary::za_complete_boundary,
            convert::{
                ZaConversionCounts, ZaShareError, ZaShareGuards, za_convert_calendar_tracked,
            },
            format::open_owner_ring,
            mutations::{
                ZaRevokeTarget, ZaShareChange, ZaShareLevel, za_share_apply, za_share_key_account,
                za_share_precheck,
            },
            resolve::{ZaCalendarError, za_calendar_context, za_owned_calendar_context},
        },
    },
    encode_path_segment,
};
use http_proto::{HttpResponse, HttpSessionData};
use registry::{
    schema::{enums::Permission, prelude::ObjectType},
    types::id::ObjectId,
};
use serde_json::json as object;
use types::{acl::Acl, collection::Collection, field::Field, id::Id};
use utils::map::bitmap::Bitmap;
use vault::{keys::display_fingerprint, pins::PinSource, record::VaultState};

#[derive(serde::Deserialize)]
pub(super) struct ShareGrantRequest {
    #[serde(flatten)]
    credentials: CredentialsRequest,
    calendar: String,
    address: String,
    level: String,
}
#[derive(serde::Deserialize)]
pub(super) struct ShareRevokeRequest {
    #[serde(flatten)]
    credentials: CredentialsRequest,
    calendar: String,
    #[serde(default, deserialize_with = "deserialize_some")]
    grant: Option<Option<u32>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    address: Option<Option<String>>,
}
fn calendar_input(slug: &str) -> trc::Result<()> {
    if slug.is_empty() || slug.len() > 255 || slug.contains('/') {
        Err(bad_request("invalid calendar"))
    } else {
        Ok(())
    }
}
fn address_input(address: &str) -> trc::Result<String> {
    za_normalize_address(address).ok_or_else(|| bad_request("invalid address"))
}
async fn verify(
    server: &Server,
    session: &HttpSessionData,
    credentials: CredentialsRequest,
    mutation: bool,
) -> trc::Result<Result<ZaVerified, HttpResponse>> {
    let result = za_verify_primary(
        server,
        session,
        &credentials.username,
        &credentials.password,
        credentials.totp,
    )
    .await?;
    if let Ok(verified) = &result
        && mutation
    {
        verified
            .access_token
            .enforce_permission(Permission::DavCalAcl)?;
    }
    Ok(result)
}

/// Classify EVERY opaque assertion failure independently: Account wins over vault,
/// and both win over either exhausted request-wide retry budget.
async fn guards(
    server: &Server,
    verified: &ZaVerified,
) -> trc::Result<Result<ZaShareGuards, HttpResponse>> {
    if server
        .registry()
        .get(ObjectId::new(
            ObjectType::Account,
            Id::from(verified.account_id),
        ))
        .await?
        .is_none()
    {
        return Ok(Err(conflict("account deleted")));
    }
    let read = match za_reread_verified(server, verified).await? {
        Ok(read)
            if read.record.state == VaultState::Active
                && read.record.revision == verified.keys.generation =>
        {
            read
        }
        _ => return Ok(Err(conflict("vault changed since verification"))),
    };
    Ok(Ok(ZaShareGuards {
        owner_id: verified.account_id,
        verified_generation: verified.keys.generation,
        vault_cas: read.cas,
        pin_assertion: None,
    }))
}

/// T7-M: usable pins never open a private wrap. The exceptional mark read drops
/// its transient zeroizing private Secret before the final fresh pin read/await.
async fn grant_pin(
    server: &Server,
    verified: &ZaVerified,
    address: &str,
    public_key: &[u8; 32],
) -> trc::Result<Result<store::write::assert::AssertValue, HttpResponse>> {
    #[cfg(feature = "test_mode")]
    test::record_private(&verified.keys);
    let result = grant_pin_inner(server, verified, address, public_key).await;
    #[cfg(feature = "test_mode")]
    test::record_private(&verified.keys);
    result
}

async fn grant_pin_inner(
    server: &Server,
    verified: &ZaVerified,
    address: &str,
    public_key: &[u8; 32],
) -> trc::Result<Result<store::write::assert::AssertValue, HttpResponse>> {
    let mut pins = server.za_pin_read(&verified.keys).await?;
    if !pins.is_usable() {
        #[cfg(feature = "test_mode")]
        common::za::share_pin_mark_pause_point(verified.account_id).await;
        let mark = {
            let read = server.za_mark_read(&verified.keys).await?;
            read.mark
        };
        pins = server.za_pin_read(&verified.keys).await?;
        if !pins.is_usable() {
            return Ok(Err(conflict(match mark {
                ZaMark::Pinned => "pin record missing",
                ZaMark::Legacy => "contact not pinned",
                ZaMark::Neither => "vault record missing",
            })));
        }
    }
    match pins.record.as_ref().and_then(|record| record.pin(address)) {
        None => Ok(Err(conflict("contact not pinned"))),
        Some(pin) if &pin.public_key != public_key => Ok(Err(conflict("contact key changed"))),
        Some(_) => Ok(Ok(pins.assertion())),
    }
}
async fn calendar_id(server: &Server, owner: u32, slug: &str) -> trc::Result<Option<u32>> {
    let mut found = None;
    server
        .all_archives(
            owner,
            Collection::Calendar,
            Field::ARCHIVE.into(),
            |id, stored| {
                if id != SCHEDULE_INBOX_ID
                    && id != SCHEDULE_OUTBOX_ID
                    && stored.unarchive::<Calendar>()?.name.as_str() == slug
                {
                    found = Some(id);
                }
                Ok(())
            },
        )
        .await?;
    Ok(found)
}

pub(super) async fn za_shares_grant(
    server: &Server,
    session: &HttpSessionData,
    request: ShareGrantRequest,
) -> trc::Result<HttpResponse> {
    calendar_input(&request.calendar)?;
    let address = address_input(&request.address)?;
    let level = match request.level.as_str() {
        "view" => ZaShareLevel::View,
        "edit" => ZaShareLevel::Edit,
        _ => return Err(bad_request("invalid level")),
    };
    let verified = match verify(server, session, request.credentials, true).await? {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    mutate(
        server,
        &verified,
        &request.calendar,
        Some((&address, level)),
        None,
    )
    .await
}
pub(super) async fn za_shares_revoke(
    server: &Server,
    session: &HttpSessionData,
    request: ShareRevokeRequest,
) -> trc::Result<HttpResponse> {
    calendar_input(&request.calendar)?;
    let target = match (request.grant, request.address) {
        (Some(Some(id)), None) => ZaRevokeTarget::Grant(id),
        (None, Some(Some(address))) => ZaRevokeTarget::Address(address_input(&address)?),
        _ => return Err(bad_request("exactly one grant or address is required")),
    };
    let verified = match verify(server, session, request.credentials, true).await? {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    mutate(server, &verified, &request.calendar, None, Some(target)).await
}
async fn mutate(
    server: &Server,
    verified: &ZaVerified,
    slug: &str,
    grant: Option<(&str, ZaShareLevel)>,
    revoke: Option<ZaRevokeTarget>,
) -> trc::Result<HttpResponse> {
    let mut decision_losses = 0;
    let mut conversion_losses = 0;
    let mut counts = ZaConversionCounts::default();
    let mut conversion_wrote = false;
    loop {
        let mut guard = match guards(server, verified).await? {
            Ok(g) => g,
            Err(r) => return Ok(r),
        };
        let Some(id) = calendar_id(server, verified.account_id, slug).await? else {
            return Ok(conflict("unknown calendar"));
        };
        let change = if let Some((address, level)) = grant {
            let Some((account_id, public_key)) = za_resolve_key_account(server, address).await?
            else {
                return Ok(conflict("not a key account"));
            };
            if account_id == verified.account_id {
                return Ok(conflict("cannot share with yourself"));
            }
            guard.pin_assertion = Some(
                match grant_pin(server, verified, address, &public_key).await? {
                    Ok(pin) => pin,
                    Err(r) => return Ok(r),
                },
            );
            // The absent-pin path can observe an atomic first-pin batch that
            // also changes the vault generation. Recheck even for a later no-op;
            // retain exactly the successful final pin read's CAS assertion.
            match guards(server, verified).await? {
                Ok(fresh) => guard.vault_cas = fresh.vault_cas,
                Err(response) => return Ok(response),
            }
            ZaShareChange::Grant {
                account_id,
                address: address.to_string(),
                public_key,
                level,
            }
        } else {
            ZaShareChange::Revoke {
                target: match revoke.as_ref().unwrap() {
                    ZaRevokeTarget::Grant(id) => ZaRevokeTarget::Grant(*id),
                    ZaRevokeTarget::Address(address) => ZaRevokeTarget::Address(address.clone()),
                },
            }
        };
        let precheck = match za_share_precheck(server, &verified.keys, id, &change).await {
            Ok(check) => check,
            Err(error) => return share_error(error, verified.account_id, id),
        };
        if precheck.needs_conversion {
            match za_convert_calendar_tracked(
                server,
                &verified.keys,
                id,
                &guard,
                &mut conversion_wrote,
            )
            .await
            {
                Ok(delta) => {
                    counts.rewrapped += delta.rewrapped;
                    counts.sealed += delta.sealed;
                }
                Err(ZaShareError::ConversionConflict {
                    committed,
                    progress_changed,
                }) => {
                    counts.rewrapped += committed.rewrapped;
                    counts.sealed += committed.sealed;
                    conversion_losses += 1;
                    if !progress_changed {
                        decision_losses += 1;
                    }
                    #[cfg(feature = "test_mode")]
                    common::za::share_conflict_pause_point(verified.account_id).await;
                    if let Err(response) = guards(server, verified).await? {
                        return Ok(response);
                    }
                    if conversion_losses >= 8 || decision_losses >= 8 {
                        return Ok(conflict("conflict"));
                    }
                    continue;
                }
                Err(error) => return share_error(error, verified.account_id, id),
            }
        }
        match za_share_apply(server, &verified.keys, id, change, &guard).await {
            Ok(applied) => {
                // Acknowledged writes impose an immediate duty, before any fallible
                // boundary completion can return. Equal ACL bits still invalidate.
                let mut invalidation = CacheInvalidationBuilder::default();
                for grantee in applied.affected_grantees {
                    invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
                }
                server.invalidate_caches(invalidation).await?;
                if let Some(commit) = applied.boundary_commit {
                    za_complete_boundary(server, &verified.keys, id, Some(commit))
                        .await
                        .map_err(|e| e.into_store_error(verified.account_id, id))?;
                }
                trc::event!(
                    Store(trc::StoreEvent::DataWrite),
                    AccountId = verified.account_id,
                    DocumentId = id,
                    Total = counts.rewrapped,
                    Size = counts.sealed,
                    Details = "zero-access: calendar sharing completed"
                );
                return Ok(if grant.is_some() {
                    json(
                        object!({"ok":true,"changed":applied.changed || conversion_wrote,"rewrapped":counts.rewrapped,"sealed":counts.sealed}),
                    )
                } else {
                    json(object!({"ok":true,"changed":applied.changed}))
                });
            }
            Err(ZaShareError::Store(error)) if error.is_assertion_failure() => {}
            Err(ZaShareError::NotReady) => {}
            Err(error) => return share_error(error, verified.account_id, id),
        }
        decision_losses += 1;
        #[cfg(feature = "test_mode")]
        common::za::share_conflict_pause_point(verified.account_id).await;
        if let Err(response) = guards(server, verified).await? {
            return Ok(response);
        }
        if decision_losses >= 8 {
            return Ok(conflict("conflict"));
        }
    }
}
fn share_error(error: ZaShareError, owner: u32, id: u32) -> trc::Result<HttpResponse> {
    Ok(match error {
        ZaShareError::LinkedEvents => conflict("calendar has linked events"),
        ZaShareError::TooManyShares => conflict("too many shares"),
        ZaShareError::AmbiguousAddress => conflict("address is ambiguous, revoke by grant"),
        ZaShareError::NotReady | ZaShareError::ConversionConflict { .. } => conflict("conflict"),
        ZaShareError::Forbidden => return Err(trc::SecurityEvent::Unauthorized.into_err()),
        ZaShareError::Seal(error) => return Err(seal_error(error, owner, id)),
        ZaShareError::Store(error) => return Err(error),
    })
}

pub(super) async fn za_shares(
    server: &Server,
    session: &HttpSessionData,
    request: CredentialsRequest,
) -> trc::Result<HttpResponse> {
    let verified = match verify(server, session, request, false).await? {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let pins = server.za_pin_read(&verified.keys).await?;
    let mut ids = Vec::new();
    server
        .all_archives(
            verified.account_id,
            Collection::Calendar,
            Field::ARCHIVE.into(),
            |id, _| {
                if id != SCHEDULE_INBOX_ID && id != SCHEDULE_OUTBOX_ID {
                    ids.push(id);
                }
                Ok(())
            },
        )
        .await?;
    let mut calendars = Vec::new();
    for id in ids {
        let context = za_owned_calendar_context(server, &verified.keys, id)
            .await
            .map_err(|e| e.into_store_error(verified.account_id, id))?;
        let view = calendar_view_in_domain(&context.stored, &context.domain, verified.account_id)
            .map_err(|e| seal_error(e, verified.account_id, id))?
            .deserialize::<Calendar>()?;
        let stored = context.stored.deserialize::<Calendar>()?;
        let mut grants = Vec::new();
        if context.domain.calendar_id().is_some() {
            let encoded = stored
                .preferences
                .iter()
                .find(|e| e.account_id == verified.account_id)
                .ok_or_else(|| seal_error(SealError::Format, verified.account_id, id))?
                .name
                .strip_prefix(COLLECTION_MARKER)
                .unwrap()
                .split('|')
                .next()
                .unwrap();
            let (_, ring) = open_owner_ring(
                &STANDARD
                    .decode(encoded)
                    .map_err(|_| seal_error(SealError::Decode, verified.account_id, id))?,
                verified.account_id,
                id,
                verified.keys.ewk(),
            )
            .map_err(|e| seal_error(e, verified.account_id, id))?;
            for grant in &ring.grants {
                let state = if !za_share_key_account(server, grant.account_id).await? {
                    "gone"
                } else if server.za_served_public_key(grant.account_id).await?
                    != Some(grant.public_key)
                {
                    "key changed"
                } else {
                    "active"
                };
                let source = pins
                    .record
                    .as_ref()
                    .and_then(|p| p.pin(&grant.address))
                    .map(|p| match p.source {
                        PinSource::FirstUse => "first-use",
                        PinSource::Confirmed => "confirmed",
                    });
                grants.push(object!({"grant":grant.account_id,"address":grant.address,"level":if grant.level==1 {"view"} else {"edit"},"fingerprint":display_fingerprint(&grant.public_key),"granted_at":grant.granted_at,"pin_source":source,"state":state}));
            }
        }
        grants.sort_by(|a, b| a["address"].as_str().cmp(&b["address"].as_str()));
        calendars.push(object!({"calendar":view.name,"name":view.preferences.iter().find(|p|p.account_id==verified.account_id).map(|p|p.name.as_str()).unwrap_or("default"),"shared":context.domain.calendar_id().is_some(),"grants":grants}));
    }
    calendars.sort_by(|a, b| a["calendar"].as_str().cmp(&b["calendar"].as_str()));
    let owners = verified
        .access_token
        .shared_accounts(Collection::Calendar)
        .copied()
        .filter(|id| *id != verified.account_id)
        .collect::<std::collections::BTreeSet<_>>();
    let token = verified
        .access_token
        .with_session_keys(verified.keys.clone());
    let mut shared = Vec::new();
    for owner in owners {
        if !za_share_key_account(server, owner).await? {
            continue;
        }
        let owner_name = server.account(owner).await?.name().to_string();
        let mut candidates = Vec::new();
        server
            .all_archives(
                owner,
                Collection::Calendar,
                Field::ARCHIVE.into(),
                |id, stored| {
                    let cal = stored.deserialize::<Calendar>()?;
                    if let Some(level) = cal
                        .acls
                        .iter()
                        .find(|a| a.account_id == verified.account_id)
                        .and_then(|a| ZaShareLevel::from_bitmap(a.grants))
                    {
                        candidates.push((id, cal.name, level));
                    }
                    Ok(())
                },
            )
            .await?;
        for (id, slug, level) in candidates {
            let name = match za_calendar_context(
                server,
                &token,
                owner,
                id,
                Bitmap::new().with_item(Acl::Read),
            )
            .await
            {
                Ok(Some(context)) => {
                    let view = calendar_view_in_domain(
                        &context.stored,
                        &context.domain,
                        verified.account_id,
                    )
                    .map_err(|e| seal_error(e, owner, id))?
                    .deserialize::<Calendar>()?;
                    view.preferences
                        .iter()
                        .find(|p| p.account_id == verified.account_id)
                        .map(|p| p.name.clone())
                }
                Err(ZaCalendarError::Seal(SealError::Aead)) => None,
                Err(ZaCalendarError::Forbidden) | Ok(None) => continue,
                Err(error) => return Err(error.into_store_error(owner, id)),
            };
            let href = format!(
                "{}/{}/{}/",
                DavResourceName::Cal.base_path(),
                percent_encoding::utf8_percent_encode(&owner_name, RFC_3986),
                encode_path_segment(&slug)
            );
            shared.push(object!({"owner":owner_name,"calendar":slug,"href":href,"name":name,"level":level.as_str()}));
        }
    }
    shared.sort_by(|a, b| {
        a["owner"]
            .as_str()
            .cmp(&b["owner"].as_str())
            .then_with(|| a["calendar"].as_str().cmp(&b["calendar"].as_str()))
    });
    Ok(json(
        object!({"calendars":calendars,"shared_with_me":shared}),
    ))
}

/// Test-only observations of the exact request-scoped keys, never their material.
#[cfg(feature = "test_mode")]
pub mod test {
    use std::sync::Mutex;
    static PRIVATE_OUTCOMES: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());
    pub(super) fn record_private(keys: &vault::session::SessionKeys) {
        PRIVATE_OUTCOMES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((keys.account_id, keys.private_key().is_some()));
    }
    pub fn take_private_outcomes(owner: u32) -> Vec<bool> {
        let mut entries = PRIVATE_OUTCOMES.lock().unwrap_or_else(|e| e.into_inner());
        let mut outcomes = Vec::new();
        entries.retain(|(id, has_private)| {
            if *id == owner {
                outcomes.push(*has_private);
                false
            } else {
                true
            }
        });
        outcomes
    }
}
