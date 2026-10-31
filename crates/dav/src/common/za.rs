/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{DavError, DavResourceName};
use common::{
    Server, auth::AccessToken, storage::metadata::ContainerChange, za::metadata::ZaMetadataFence,
};
use groupware::calendar::{
    ArchivedCalendarEvent, CalendarEvent, CalendarEventNotification,
    seal::{
        SealError, ZaContainerOf, ZaContainerScope, ZaEventView, ZaKeyDomain,
        archived_item_is_pending, calendar_view_in_domain, event_view_in_domain, open_container,
        reseal_container, seal_container, seal_error, unseal_item_archive,
    },
    sharing::resolve::{ZaCalendarContext, ZaCalendarError, za_calendar_context},
};
use hyper::StatusCode;
use registry::schema::enums::Permission;
use std::{borrow::Cow, collections::HashMap, future::Future, sync::Arc};
use store::{
    ValueKey,
    write::{
        Archive, ArchiveBytes,
        metadata::{MetadataBuf, StoredMetadata},
        serialize::rkyv_deserialize,
    },
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::Collection,
    metadata::{EncodedMetadata, MetadataEdit, MetadataView},
};
use utils::map::bitmap::Bitmap;
use vault::session::SessionKeys;

/// What a free-busy computation may read from an account (ruling R6).
pub(crate) enum ZaFreeBusy {
    /// Not a key account: the upstream path.
    Plain,
    /// A key account whose keys this session holds: read unsealed events.
    Unsealed(Arc<SessionKeys>),
    /// A key account whose keys this session does not hold: report no busy
    /// periods, so nothing of the account is disclosed.
    Withheld,
}

pub(crate) trait ZeroAccessGate: Sync + Send {
    /// Calendar URI admission. The per-calendar resolver still checks every operation bit.
    fn za_calendar_gate(
        &self,
        token: &AccessToken,
        owner_id: u32,
    ) -> impl Future<Output = crate::Result<()>> + Send;

    /// The URI gate of `validate_uri` (and of the free-busy handler, which
    /// parses without validating): calendar sharing admits only an
    /// authenticated grantee's own session, and scheduling stays confined to
    /// the account whose keys are held.
    fn za_uri_gate(
        &self,
        token: &AccessToken,
        account_id: u32,
        collection: Collection,
    ) -> impl Future<Output = crate::Result<()>> + Send;

    /// `None` for a non-key account (unchanged upstream path). For a key
    /// account, the keys of that account held by this session, or 403.
    fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<Option<Arc<SessionKeys>>>> + Send;

    /// Ruling R6: like `za_session_keys`, but a key account without its
    /// keys yields `Withheld` instead of 403.
    fn za_freebusy_access(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<ZaFreeBusy>> + Send;

    /// Spec 8.1: a calendar COPY or MOVE across accounts is refused (403)
    /// when either side is a key account.
    fn za_refuse_cross_account(
        &self,
        from_account_id: u32,
        to_account_id: u32,
    ) -> impl Future<Output = crate::Result<()>> + Send;
}

impl ZeroAccessGate for Server {
    async fn za_calendar_gate(&self, token: &AccessToken, owner_id: u32) -> crate::Result<()> {
        if !is_key_account(self, owner_id).await? || token.za_keys_for(owner_id).is_some() {
            return Ok(());
        }
        if token
            .session_keys()
            .is_some_and(|keys| keys.account_id == token.account_id())
            && !token.za_is_impersonated()
            && !token.has_permission(Permission::Impersonate)
            && token.has_shared_access(owner_id, Collection::Calendar)
        {
            return Ok(());
        }
        trc::event!(
            Security(trc::SecurityEvent::Unauthorized),
            AccountId = owner_id,
            Details = "zero-access: calendar access without session keys",
        );
        Err(DavError::Code(StatusCode::FORBIDDEN))
    }

    async fn za_uri_gate(
        &self,
        token: &AccessToken,
        account_id: u32,
        collection: Collection,
    ) -> crate::Result<()> {
        match collection {
            Collection::Calendar => self.za_calendar_gate(token, account_id).await,
            Collection::CalendarEventNotification => {
                self.za_session_keys(token, account_id).await.map(|_| ())
            }
            _ => Ok(()),
        }
    }

    async fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<Option<Arc<SessionKeys>>> {
        // An unknown account is not a key account: upstream's outcome stands.
        if !is_key_account(self, account_id).await? {
            return Ok(None);
        }
        match access_token.za_keys_for(account_id) {
            Some(keys) => Ok(Some(keys.clone())),
            None => {
                trc::event!(
                    Security(trc::SecurityEvent::Unauthorized),
                    AccountId = account_id,
                    Details = "zero-access: calendar access without session keys",
                );
                Err(DavError::Code(StatusCode::FORBIDDEN))
            }
        }
    }

    async fn za_freebusy_access(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<ZaFreeBusy> {
        if !is_key_account(self, account_id).await? {
            return Ok(ZaFreeBusy::Plain);
        }
        Ok(match access_token.za_keys_for(account_id) {
            Some(keys) => ZaFreeBusy::Unsealed(keys.clone()),
            None => ZaFreeBusy::Withheld,
        })
    }

    async fn za_refuse_cross_account(
        &self,
        from_account_id: u32,
        to_account_id: u32,
    ) -> crate::Result<()> {
        if from_account_id != to_account_id
            && (is_key_account(self, from_account_id).await?
                || is_key_account(self, to_account_id).await?)
        {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = from_account_id,
                Details = "zero-access: calendar copy or move across accounts",
            );
            Err(DavError::Code(StatusCode::FORBIDDEN))
        } else {
            Ok(())
        }
    }
}

/// Calendar REPORTs (calendar-query, calendar-multiget, free-busy-query) read
/// the calendar store whatever the request URI's prefix, but the gate only
/// sees calendar and scheduling URIs: refuse every other prefix (405, as
/// upstream does for principal REPORTs sent elsewhere).
pub(crate) fn za_calendar_report_uri(resource: DavResourceName) -> crate::Result<()> {
    if matches!(resource, DavResourceName::Cal | DavResourceName::Scheduling) {
        Ok(())
    } else {
        Err(DavError::Code(StatusCode::METHOD_NOT_ALLOWED))
    }
}

/// Spec 7.3 at 1.0: the read view of a stored event in `context`'s key
/// domain. `meta` is the stored metadata row and `content` the stored
/// content archive; the view's content keeps the stored archive's version,
/// so identity stays bound to the stored bytes. Read-only, never written
/// back.
pub(crate) fn za_event_view_in(
    context: &ZaCalendarContext,
    meta: &ArchivedCalendarEvent,
    content: &Archive<ArchiveBytes>,
    owner_id: u32,
    document_id: u32,
) -> trc::Result<ZaEventView> {
    let meta = rkyv_deserialize::<_, CalendarEvent>(meta).caused_by(trc::location!())?;
    event_view_in_domain(&meta, content, &context.domain)
        .map_err(|error| seal_error(error, owner_id, document_id))
}

/// Invitations spec 6.6: the inbox counterpart of the event view, on the
/// item's content archive, same contract. A drained item is unsealed under
/// the account's keys; a pending (`pk`) item reached here is an unseal
/// failure (spec 10): the caller answers 500 for it and changes nothing.
pub(crate) fn za_item_view<'x>(
    content: &'x Archive<ArchiveBytes>,
    keys: Option<&Arc<SessionKeys>>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Cow<'x, Archive<ArchiveBytes>>> {
    match keys {
        Some(keys) => unseal_item_archive(content, keys, account_id)
            .map(Cow::Owned)
            .map_err(|err| seal_error(err, account_id, document_id)),
        None => Ok(Cow::Borrowed(content)),
    }
}

/// Invitations spec 6.6: whether a loaded item row is a key account's
/// pending inbox item, which clients do not see until the drain rewrites it.
/// `false` for every other collection and for non-key accounts (invariant
/// 9). The gate runs before the pending bit is read: a key account's item
/// read without its keys is the gate's 403 and trace (release 1 spec 10).
/// The resource cache knows no flags, so readers ask here after loading.
pub(crate) async fn za_item_hidden(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
    stored: &Archive<ArchiveBytes>,
) -> crate::Result<bool> {
    if !za_reads_pending_bit(server, access_token, account_id, collection).await? {
        return Ok(false);
    }
    stored_item_is_pending(stored)
}

/// Invitations spec 6.6: [`za_item_hidden`] for a path taken from the
/// resource cache whose row is not loaded, as the `propname` form of
/// PROPFIND has it. Same gate, so other collections and non-key accounts
/// answer `false` with no store read; a missing row is listed as upstream
/// lists it.
pub(crate) async fn za_cached_item_hidden(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
    document_id: u32,
) -> crate::Result<bool> {
    if !za_reads_pending_bit(server, access_token, account_id, collection).await? {
        return Ok(false);
    }
    match server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(account_id, collection, document_id))
        .await
        .caused_by(trc::location!())?
    {
        Some(stored) => stored_item_is_pending(&stored),
        None => Ok(false),
    }
}

/// The gate both readers above run before the pending bit is read: `false`
/// outside the inbox (no lookup) and for a non-key account, the gate's 403
/// for a key account's item read without its keys.
async fn za_reads_pending_bit(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
) -> crate::Result<bool> {
    Ok(collection == Collection::CalendarEventNotification
        && server
            .za_session_keys(access_token, account_id)
            .await?
            .is_some())
}

fn stored_item_is_pending(stored: &Archive<ArchiveBytes>) -> crate::Result<bool> {
    Ok(archived_item_is_pending(
        stored
            .unarchive::<CalendarEventNotification>()
            .caused_by(trc::location!())?,
    ))
}

/// This map belongs to one read request only. Writers never consume it.
pub(crate) type ZaReadMemo = HashMap<(u32, u32), Arc<ZaCalendarContext>>;

/// Explicit owner-session repair; the groupware resolver remains read-only.
pub(crate) async fn za_mutation_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
) -> Result<Option<ZaCalendarContext>, ZaCalendarError> {
    let context = za_calendar_context(server, token, owner_id, calendar_id, required).await?;
    match (context, token.session_keys()) {
        (Some(context), Some(keys)) => {
            groupware::calendar::sharing::boundary::za_repair_owned_context(server, keys, context)
                .await
                .map(Some)
        }
        (context, _) => Ok(context),
    }
}

pub(crate) async fn za_read_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
    memo: &mut ZaReadMemo,
) -> Result<Option<Arc<ZaCalendarContext>>, ZaCalendarError> {
    if let Some(context) = memo.get(&(owner_id, calendar_id)) {
        context.check_access(token, required)?;
        return Ok(Some(context.clone()));
    }
    let context = za_mutation_context(server, token, owner_id, calendar_id, required)
        .await?
        .map(Arc::new);
    if let Some(context) = &context {
        memo.insert((owner_id, calendar_id), context.clone());
    }
    Ok(context)
}

/// Refusals fail the request; corrupt content and general store errors fail an
/// individual listing item. Only the precise missing-resource error becomes 404.
fn calendar_item_error(
    error: ZaCalendarError,
    owner_id: u32,
    document_id: u32,
) -> crate::Result<trc::Error> {
    match error {
        ZaCalendarError::Forbidden => Err(DavError::Code(StatusCode::FORBIDDEN)),
        ZaCalendarError::LinkedEvent => Err(DavError::Code(StatusCode::CONFLICT)),
        ZaCalendarError::Seal(error) => Ok(seal_error(error, owner_id, document_id)),
        ZaCalendarError::Store(error)
            if error.matches(trc::EventType::Store(trc::StoreEvent::NotFound)) =>
        {
            Err(DavError::Code(StatusCode::NOT_FOUND))
        }
        ZaCalendarError::Store(error) => Ok(error),
    }
}

/// The read context of an event's calendar, with `za_read_event_view`'s
/// error scopes: `Ok(Ok(None))` for a non-key account (upstream's path).
pub(crate) async fn za_read_event_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    memo: &mut ZaReadMemo,
) -> crate::Result<trc::Result<Option<Arc<ZaCalendarContext>>>> {
    match za_read_context(
        server,
        token,
        owner_id,
        calendar_id,
        Bitmap::new().with_item(Acl::ReadItems),
        memo,
    )
    .await
    {
        Ok(context) => Ok(Ok(context)),
        Err(error) => Ok(Err(calendar_item_error(error, owner_id, document_id)?)),
    }
}

/// The read view of a stored event (`meta` row, `content` archive) through a
/// fresh or memoized read context. The outer `Err` is a refusal that fails
/// the request; the inner one fails one item. `None` for a non-key account.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn za_read_event_view(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    meta: &ArchivedCalendarEvent,
    content: &Archive<ArchiveBytes>,
    memo: &mut ZaReadMemo,
) -> crate::Result<trc::Result<Option<ZaEventView>>> {
    Ok(
        match za_read_event_context(server, token, owner_id, document_id, calendar_id, memo).await?
        {
            Ok(Some(context)) => {
                za_event_view_in(&context, meta, content, owner_id, document_id).map(Some)
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        },
    )
}

/// Ruling R6: the unsealed content of each of a key account's events a
/// free-busy computation selected, keyed by document id, read before
/// upstream's synchronous scan. A missing event is left out.
pub(crate) async fn za_freebusy_views(
    server: &Server,
    token: &AccessToken,
    account_id: u32,
    document_ids: &store::roaring::RoaringBitmap,
) -> crate::Result<store::ahash::AHashMap<u32, Archive<ArchiveBytes>>> {
    let mut memo = ZaReadMemo::new();
    let mut views = store::ahash::AHashMap::with_capacity(document_ids.len() as usize);
    for document_id in document_ids {
        let Some(meta_) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            continue;
        };
        let meta = meta_
            .unarchive::<CalendarEvent>()
            .caused_by(trc::location!())?;
        let Some(calendar_id) = meta.names.first().map(|name| name.parent_id.to_native()) else {
            continue;
        };
        let Some(content) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                account_id,
                Collection::CalendarEvent,
                document_id,
                types::field::CalendarEventField::Content,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            continue;
        };
        if let Some(view) = za_read_event_view(
            server,
            token,
            account_id,
            document_id,
            calendar_id,
            meta,
            &content,
            &mut memo,
        )
        .await??
        {
            views.insert(document_id, view.content);
        }
    }
    Ok(views)
}

/// A calendar's read view through a fresh or memoized read context: the
/// requested principal's personal bundle and the shared properties opened,
/// the stored version kept. Error scopes as `za_read_event_view`.
pub(crate) async fn za_read_calendar_view(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    memo: &mut ZaReadMemo,
) -> crate::Result<trc::Result<Option<(Archive<ArchiveBytes>, Arc<ZaCalendarContext>)>>> {
    Ok(
        match za_read_context(
            server,
            token,
            owner_id,
            calendar_id,
            Bitmap::new().with_item(Acl::Read),
            memo,
        )
        .await
        {
            Ok(Some(context)) => {
                calendar_view_in_domain(&context.stored, &context.domain, context.personal_id)
                    .map(|view| Some((view, context.clone())))
                    .map_err(|error| seal_error(error, owner_id, calendar_id))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(calendar_item_error(error, owner_id, calendar_id)?),
        },
    )
}

pub(crate) async fn is_key_account(server: &Server, account_id: u32) -> crate::Result<bool> {
    Ok(server
        .za_is_key_account(account_id)
        .await
        .caused_by(trc::location!())?)
}

/// Ruling 70: the collections whose DAV dead-property containers carry the
/// ordinary-writer assertion. A key account's calendar and event containers
/// are sealed (rulings 16 and 23), so a plaintext one is an ordinary
/// writer's; address books, cards and files keep their dead properties in
/// the clear for every account, a key account's included, and are excluded.
pub(crate) fn za_dead_fenced(collection: Collection) -> bool {
    matches!(collection, Collection::Calendar | Collection::CalendarEvent)
}

/// Ruling 70 (with ruling 66 and the Task 6 review's F1): the fence of a DAV
/// dead-property container write on `account_id`'s `collection` object.
/// `sealed` is the writer's classification: `Some(true)` for a write sealed
/// for a key account, `Some(false)` for upstream's plaintext path, `None` to
/// read it (`account()`, which fails closed). A plaintext calendar or event
/// container write commits only while the account holds no vault record
/// (`ZaMetadataFence`, applied before the write in the same commit chunk),
/// as mail's ordinary writers do; every other write gets no fence.
pub(crate) async fn za_dead_fence(
    server: &Server,
    account_id: u32,
    collection: Collection,
    sealed: Option<bool>,
) -> trc::Result<Option<ZaMetadataFence>> {
    if !za_dead_fenced(collection) {
        return Ok(None);
    }
    let sealed = match sealed {
        Some(sealed) => sealed,
        None => server
            .account(account_id)
            .await
            .caused_by(trc::location!())?
            .is_key_account(),
    };
    Ok(if sealed {
        None
    } else {
        server.za_metadata_fence(account_id, None)
    })
}

/// The scope of a key account's dead-property container (upgrade rulings 16
/// and 23): which document it belongs to and the key domain it is sealed in.
pub(crate) struct ZaDeadSeal<'x> {
    pub domain: &'x ZaKeyDomain,
    /// The scope a new container is sealed under.
    pub scope: ZaContainerScope<'x>,
    /// The scope the stored container is opened for.
    pub of: ZaContainerOf<'x>,
    pub owner_id: u32,
    pub document_id: u32,
}

impl<'x> ZaDeadSeal<'x> {
    /// A calendar's container. `calendar_id` is `None` at MKCOL, where the
    /// id is a pending slot (ruling 16's recorded limit).
    pub fn calendar(domain: &'x ZaKeyDomain, owner_id: u32, calendar_id: Option<u32>) -> Self {
        ZaDeadSeal {
            domain,
            scope: ZaContainerScope::Calendar(calendar_id),
            of: ZaContainerOf::Calendar(calendar_id.unwrap_or(u32::MAX)),
            owner_id,
            document_id: calendar_id.unwrap_or(u32::MAX),
        }
    }

    /// An event's container, bound by the event's UID.
    pub fn event(domain: &'x ZaKeyDomain, owner_id: u32, document_id: u32, uid: &'x str) -> Self {
        ZaDeadSeal {
            domain,
            scope: ZaContainerScope::Event(uid),
            of: ZaContainerOf::Event(uid),
            owner_id,
            document_id,
        }
    }

    /// Opens the stored container for an edit's view (ruling 23).
    pub fn open(&self, stored: Option<&MetadataBuf>) -> trc::Result<Option<MetadataBuf>> {
        stored
            .map(|stored| {
                za_open_container(
                    stored,
                    self.domain,
                    self.of,
                    self.owner_id,
                    self.document_id,
                )
            })
            .transpose()
    }

    /// The change of a sealed edit (ruling 23): `None` when the plaintext is
    /// unchanged; otherwise `next` sealed in this scope, with the stored
    /// sealed container as the previous value, so the compare-and-swap is
    /// upstream's.
    pub fn change(
        &self,
        stored: Option<&MetadataBuf>,
        opened: Option<&MetadataBuf>,
        next: Option<&EncodedMetadata>,
        edit: MetadataEdit,
    ) -> trc::Result<Option<ContainerChange>> {
        let unchanged = match (opened, next) {
            (Some(opened), Some(next)) => opened.view().as_bytes() == next.as_bytes(),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return Ok(None);
        }
        let sealed = next
            .map(|next| {
                seal_container(next.as_bytes(), self.domain, self.scope)
                    .map_err(|error| seal_error(error, self.owner_id, self.document_id))
            })
            .transpose()?;
        Ok(ContainerChange::new(stored, sealed, edit))
    }
}

/// Opens a sealed container (PROPFIND, PROPPATCH): the plaintext as a
/// buffer that keeps the stored container's size and hash, so keyless
/// bookkeeping on it stays the stored one's.
pub(crate) fn za_open_container(
    stored: &MetadataBuf,
    domain: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
    owner_id: u32,
    document_id: u32,
) -> trc::Result<MetadataBuf> {
    let plain = open_container(stored.view().as_bytes(), domain, of)
        .map_err(|error| seal_error(error, owner_id, document_id))?;
    let view = MetadataView::new(&plain)
        .ok_or_else(|| seal_error(SealError::Structure("container"), owner_id, document_id))?;
    Ok(MetadataBuf::from_view(
        &view,
        stored.stored_len(),
        stored.hash(),
    ))
}

/// COPY and MOVE between calendars or key domains (rulings 16 and 29): the
/// source container opened in its domain and sealed in the destination's
/// under a fresh data key, as a buffer `ContainerWrites::copy` takes (its
/// stored size is the sealed one, for the quota).
pub(crate) fn za_reseal_container(
    stored: &MetadataBuf,
    from: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
    to: &ZaKeyDomain,
    scope: ZaContainerScope<'_>,
    owner_id: u32,
    document_id: u32,
) -> trc::Result<MetadataBuf> {
    let sealed = reseal_container(stored.view().as_bytes(), from, of, to, scope)
        .map_err(|error| seal_error(error, owner_id, document_id))?;
    za_container_buf(sealed)
}

fn za_container_buf(container: EncodedMetadata) -> trc::Result<MetadataBuf> {
    let stored = StoredMetadata::new(container).caused_by(trc::location!())?;
    MetadataBuf::read(stored.as_bytes()).caused_by(trc::location!())
}

#[cfg(test)]
mod dead_fence_tests {
    use super::za_dead_fenced;
    use types::collection::Collection;

    /// Ruling 70: only calendar and event containers are fenced.
    #[test]
    fn only_calendar_containers_are_fenced() {
        assert!(za_dead_fenced(Collection::Calendar));
        assert!(za_dead_fenced(Collection::CalendarEvent));
        for collection in [
            Collection::AddressBook,
            Collection::ContactCard,
            Collection::FileNode,
            Collection::CalendarEventNotification,
            Collection::Mailbox,
            Collection::Email,
        ] {
            assert!(!za_dead_fenced(collection), "{collection:?}");
        }
    }
}

#[cfg(test)]
mod sharing_read_tests {
    use super::*;
    use groupware::calendar::{seal::SealError, sharing::resolve::ZaCalendarError};

    #[test]
    fn read_errors_keep_their_scope() {
        assert!(matches!(
            calendar_item_error(ZaCalendarError::Forbidden, 1, 2),
            Err(DavError::Code(StatusCode::FORBIDDEN))
        ));
        assert!(matches!(
            calendar_item_error(ZaCalendarError::LinkedEvent, 1, 2),
            Err(DavError::Code(StatusCode::CONFLICT))
        ));
        assert!(calendar_item_error(ZaCalendarError::Seal(SealError::Aead), 1, 2).is_ok());
        assert!(matches!(
            calendar_item_error(ZaCalendarError::Store(trc::StoreEvent::DataCorruption.into_err()), 1, 2),
            Ok(error) if error.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption))
        ));
        assert!(matches!(
            calendar_item_error(
                ZaCalendarError::Store(trc::StoreEvent::NotFound.into_err()),
                1,
                2
            ),
            Err(DavError::Code(StatusCode::NOT_FOUND))
        ));
    }
}
