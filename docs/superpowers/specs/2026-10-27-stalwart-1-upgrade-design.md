# Stalwart 1.0 fresh store upgrade design

Revision 5, 2026-10-30. It adopts
[Fable's check of revision 4](2026-10-27-stalwart-1-upgrade-design-review-r4.md) and records
Jay's answers to its two questions; the "Revision 4 re-review" section of the
[Codex review](../plans/2026-10-27-stalwart-1-upgrade-codex-review-2026-10-30.md) found no
new finding. The main change is a visibility-policy decision: sharers can hide events by
`CLASS`, so `CLASS` becomes visible under field-policy version 2 (calendar spec revision 8)
and upstream's privacy filters work unchanged for key accounts. The dead-property wrapper,
its seal sites and the COPY/MOVE re-key are now specified. The findings of
[Fable's check of revision 5](2026-10-27-stalwart-1-upgrade-design-review-r5.md), which
found nothing critical or high, are fixed in place, and the reviews stop there, as agreed.

Revision 4 (2026-10-30) adopted both re-reviews of revision 3
([Fable's](2026-10-27-stalwart-1-upgrade-design-review-r3.md) and the Codex review's
"Revision 3 re-review") and Jay's answers to their four questions. Its main correction was
to the dead-property design, which now seals exactly the unit upstream stores.

Revision 3 (2026-10-30) adopted the first reviews of both
([Fable](2026-10-27-stalwart-1-upgrade-design-review.md) and Codex), under one principle:
maintain upstream. Revision 2 (2026-10-29) moved the base to 0.16.27 and the target to
v1.0.1, and made the fork own the whole 0.16.27 suite. Revision 1 (2026-10-27) targeted
v1.0.0 from the 0.16.26 base.

## Purpose and scope

Move the zero-access fork from its 0.16.27 base to upstream 1.0.1 while preserving its
account, mail, calendar, invitation, pinning and sharing contracts, and as much of its test
coverage as can be kept. Implementation has not started; this design and its execution plan
are proposed for review.

- **Starting fork:** `main` at `9a7574203e9821a8313239f51a080c0657672be9`, the merge of PR
  #66, which makes CI enforce every test that passes at 0.16.27. The baseline below was
  measured one commit earlier, at `5b914ddb5`.
- **Upstream base:** `v0.16.27`, `aa47d8ef7bfba7200d3293198ac2592747b20e73`, merged by PR #62.
- **Target:** the immutable `v1.0.1` release, `23b69755cb3cd8ecd952fe0443c5ac067a506184`.
  It is one commit on top of `v1.0.0` (`47df4c0baeb88d83424dfda6aa7cf6040706999e`), which
  is one commit on top of `v0.16.27`, so the incoming change is exactly upstream's two
  release commits.

A later upstream patch needs its own delta assessment; this plan does not silently follow
upstream `main` or `latest`.

**Deployments.** Only test deployments of the 0.16 fork exist, including `mail.circolo.me`.
Jay confirmed on 2026-10-29 that none of their data needs to be kept. The 1.0 cutover
creates a fresh store and provisions the accounts again: new setup tokens, enrolment, and
clients added again. That runbook belongs in the private deploy repository, not here.

**Qualification scope** is unchanged: single-node RocksDB on the macOS development host and
the existing Linux x86_64 GNU product artifact, with embedded and filesystem blob storage.
Existing backend refusals stay. SQL stores, other backends, clusters and new artifact
platforms gain no qualification from this project.

## Principle: maintain upstream

Where a choice exists, prefer upstream's mechanism and code unchanged. Add fork code only
where a contract (C1–C11) requires it, and then in fork-owned modules reached by one-line
insertions. Upstream's own builders, stores and tasks are reused with different inputs, or
skipped for key accounts, rather than re-derived by hand. This is the parity-by-construction
rule the sharing design settled on. For tests, restore upstream's tests verbatim wherever
their subject still exists.

## Approaches

1. **Merge 1.0.1 directly into the fork and initialize fresh stores (selected).** This keeps
   published Git history and the existing fork modules, tests and public API, and adapts each
   protection to upstream's new storage and runtime boundaries. It needs no migration or
   compatibility machinery, because there is no data to keep.
2. **A 0.16.27 bridge first.** Done (PR #62). Its value for this project is the merge base:
   Git compares the fork against exactly the two upstream commits.
3. **Rebuild the fork from a clean upstream checkout.** Rejected. It makes a clean first
   build easier, but it risks dropping small enforcement hooks and regression cases. The
   fork's history and explicit contract inventory give reviewers a better audit trail.

Use a normal merge in an isolated implementation worktree, and never rebase the published
branch. A dry merge of v1.0.1 into `5b914ddb5` reports 134 conflicts (106 content, 27
modify/delete, 1 add/add). Resolving them is only part of the port: upstream also
modified 849 files, added 670 and deleted 139 under `crates/`. It replaces interfaces and
introduces new writers, and Git cannot tell that those need a fork protection.

Fork hooks sit in four upstream files that 1.0.1 deleted or split:
- `crates/dav/src/common/propfind.rs` (15 call sites) became `propfind/`;
- `crates/imap/src/op/fetch.rs` (7) became `op/fetch/`;
- `crates/imap/src/core/message.rs` (1) became `core/view.rs`;
- `crates/spam-filter/src/analysis/llm.rs` (1) moved into the SEL-only `common/src/enterprise/ai/`.

## Scope exclusions

- No migration of 0.16 stores in either direction, no legacy encrypted archive reader, and
  no preservation of old client sync state. An old store is an unsupported input, not a
  bootstrap source.
- Existing same-store behaviour stays in scope: password and recovery wraps, pending setup,
  conversion of an owner's calendar to shared, resumable conversion and refusal of unsealed
  rows. A test named `legacy` is not a reason to remove it.
- No use of upstream's private tests. Do not enable `cfg(stalwart_tests)`, and do not
  recreate the private support modules that its 562 slots expect in a sibling
  `stalwart-tests` checkout.
- No SEL-only test is ported. Enterprise features are excluded from the product. Under SELv2
  (sections 4.1, 4.4 and 2.3), even a subscription permits modified copies only for internal
  use, never in this public repository. The 0.16.27 originals stay recoverable from history.
- No external protocol suite (Dovecot imaptest, Fastmail JMAP-TestSuite, Apple
  CalDAVTester) in this project. They add coverage rather than preserve it, and they are
  deferred to a follow-up plan. The python-caldav runner the fork already uses stays.
- No deletion of existing `.run/` state or other worktrees. New tests and manual checks use a
  newly allocated state directory and disposable accounts.
- No new signup, billing, provisioning service, webmail, AI feature or encryption redesign.
- No automatic adoption of newly added protocol operations for key accounts. A new content
  surface either gets a defined refusal or the same encryption and authority treatment as
  the existing operation it extends.
- Planning does not authorize implementation, commits, CI dispatch, publication or
  deployment.

## Binding contracts

The existing approved designs remain the source of product behaviour:

- [Calendar accounts and sealing](2026-10-06-zero-access-calendar-design.md).
- [Invitations](2026-10-11-zero-access-invitations-design.md).
- [Key pinning](2026-10-17-zero-access-key-pinning-design.md).
- [Mail](2026-10-20-zero-access-mail-design.md), including its resolved implementation items.
- [Sharing revision 14](2026-10-22-zero-access-sharing-design.md) and the accepted
  [implementation interpretations](../plans/2026-10-23-zero-access-plan14-outcome.md).
- [Release policy](../../releases.md) and [developer merge gates](../plans/README-dev.md).

The contracts below are acceptance criteria, not an opportunity to redesign those features.

| ID | Contract |
|---|---|
| C1 | The vault is the login authority. Every mutation keeps its revision CAS, the terminal account checks and `za_commit`/`za_vault_commit` invalidation. Registry credential edits cannot bypass it. |
| C2 | Authentication returns keyless common tokens. HTTP keys live for one request; IMAP and POP3 keys live for their authenticated connection. SMTP, ManageSieve, WebSocket and EventSource sessions hold no session keys. Keep the existing idle and hard cache bounds and the session-revocation semantics. |
| C3 | No credential, reversible credential cache key, private key, decrypted protected content or content-bearing diagnostic is added to caches, traces, errors or persistent snapshots. Keep the approved field-specific trace exceptions. |
| C4 | Ordinary accounts keep upstream 1.0 behaviour. Key accounts keep the existing refusals, including the unsupported credential, content-search, mail-sharing and JMAP-calendar operations, and the external-directory refusals and marker preservation (calendar spec 4.3). Keep upstream's OAuth expiry, grant-revocation and scope checks. |
| C5 | Key-account mail is a stub plus a sealed blob, whatever wrote it. Every ordinary writer asserts the vault's absence in its effective commit. Every protected reader has the owner's keys or refuses; background work never acquires those keys. |
| C6 | Calendar metadata and content are sealed or sanitized before serialization and derived indexing. No hidden property can reappear in metadata, alarms, notifications, dead properties or indexes, whether through a split or metadata-only write, through a background rewrite, or through a stored flag derived from it. Flags derived from visible properties are permitted: the alarm flags the calendar spec allows (sections 6 and 9) and, from field-policy version 2, `EVENT_PRIVATE` and `EVENT_SECRET`, derived from a visible `CLASS`. |
| C7 | Sharing uses the current principal's keys, the current own-id permission and the authenticated ring identity. Keep the physical parent and link fences, conversion, future-write rotation, sync boundaries, pin-before-deliver, UID leases, SEQUENCE and delegated scheduling semantics. |
| C8 | Search work for key mail keeps only the allowed arrival-time and size fields. Sealed calendar content and protected attachment text stay out of background indexes. Provisioning and deletion fences cover every new queue and commit segment. |
| C9 | Keep upstream 1.0 stored types as supplied: add no fork fields to them, and hand-edit no generated registry schema or event enum. Keep the reserved Principal fields 150/151, the account flag `1 << 9` and the vanished collection 250, after checking each for collisions. |
| C10 | Own the retained test suite. The required set is the 0.16.27 baseline minus its recorded dispositions. A required test cannot disappear, become ignored, stop being selected, or stop running to completion, while a green aggregate hides the loss. Test-only hooks and the weakened Argon2 never enter shipping builds. |
| C11 | Qualify fresh creation and restart with the actual OSS product binary. Enterprise-enabled harness results are separate evidence, because 1.0 stamps each store with its edition. Keep the fork's independent version and provenance and its immutable release artifacts. |

## Integration architecture

### Fresh persistence baseline

Use upstream 1.0's schema, split calendar storage, serializers and pending-ID transaction
model. Align the fork's archive dependencies with the target workspace, including its rkyv
alignment configuration and local calcard crate. Keep the fork's record fields, crypto
algorithms, KDF inputs, AAD ownership, purposes and wrap identifiers unchanged unless a
separately reviewed port correction is needed. Bytes written by the 0.16 fork need not
deserialize.

The 1.0 schema is the new compatibility boundary. Document that archived inner payloads are
specific to this baseline: a matching old payload version byte does not establish
compatibility across bases. Once fresh 1.0 fixtures exist, lock their formats with
checked-decode, round-trip and process-restart tests. Do not add an old calcard or a
duplicate rkyv dependency to reconstruct the undeployed baseline.

1.0 stamps each store with its edition: schema 7 for `enterprise` builds and `7 | 1 << 31`
without, and each refuses the other's store (`crates/common/src/manager/schema.rs`). The
harness builds with `enterprise`, so its stores are enterprise-edition. That is why C11's
product gate runs the OSS binary.

### Dead properties and object metadata

1.0.1 removed `types::DeadProperty` and the event record's `dead_properties` field. DAV
dead properties, IMAP METADATA and JMAP object metadata now share one store,
`Collection::Metadata` (14), written through `dav/src/common/dead/` and
`common/src/storage/metadata/`. The fork's sealed extras therefore lose their home in the
event record. The store has this shape:
- **Keys** hold only ids: `account / Collection::Metadata / class / collection /
  document_id`, with the viewer's id for `Private` (`store/src/write/key.rs:589-608`). No
  property name appears in a key.
- **Containers.** Only two classes carry one: `Shared` and `Private{viewer}`. The value is a
  per-document container of `Entry { key, value }` pairs, so the names and values are
  inside it (`types/src/metadata/entries.rs`).
- **Bookkeeping.** `Viewer{viewer}` and `Owner{owner}` hold upstream's 12-byte
  `ViewerState { change_id, containers }` counters, keyed without a document id. Keyless
  enumeration decodes them directly (`common/src/storage/metadata/{mod,read,purge}.rs`).
- **Scope.** The store serves eight object kinds: Email, Mailbox, SieveScript, Calendar,
  CalendarEvent, AddressBook, ContactCard and FileNode.

**Decision (parity by construction):** for a key account's calendars and events, seal
exactly the unit upstream stores.
- **Sealed unit:** the per-document container of a `Shared` entry, names and values
  together, which matches 0.16, where the whole dead property was sealed. DAV writes only
  `Shared` (`dead/patch.rs:190`; nothing under `crates/dav` calls `into_private_write`), and
  the other writers of `Private` (IMAP METADATA, JMAP object metadata) are refused for key
  accounts. So no `Private` container exists for a key account's calendar or event; a
  change that lets one be written needs its own review.
- **Key:** the calendar's collection key.
- **Wrapper:** a sealed container is still a valid upstream container, because keyless
  paths parse every container's header: `MetadataBuf::read` and `StoredMetadata::view`
  (through `MetadataView::from_trusted`), `StoredContainer::new` (`view.kinds()`),
  `containers_of`, `all_containers_of`, `container_kinds`, and the cleanup, purge and quota
  paths. The header `[FORMAT_VERSION, kinds, count]` (`types/src/metadata/view.rs:62-69`)
  must match the entries it holds, so the wrapper's header carries `MetadataKinds::DAV`
  (the only kind DAV writes), and its one entry is a `Dav` entry under a fork-reserved
  namespace and name. That entry's value is a text-only DAV value, with no attributes or
  children, whose text is the base64 of the fork's payload version byte followed by the
  ciphertext of the original container. Both the trusted span reader and the checked XML
  validator (`types/src/metadata/xml/view.rs`) accept that. The checked path has no
  production caller at 1.0.1, but the format does not rely on that. The presence derivation
  reads the same header and is unchanged.
- **Seal and unseal sites:** DAV's consumers, which hold the request's keys. In
  `dav/src/common/dead/patch.rs`, `prepare` reads the stored container and builds the edit
  from its view (`patch.rs:132-141`): unseal it there, before the edit is built, and seal
  `next` before `into_write`. Unseal after `.view()` in
  `dav/src/common/propfind/dead.rs` (`dead_properties`, `dead_property_names`) and in
  `dav/src/common/dead/containers.rs` (`get`, `stored_entries`). Each is a one-line call
  into a fork module. `common/src/storage/metadata/` and `store/src/write/metadata.rs`
  stay upstream's and keyless.
- **COPY and MOVE:** `DeadContainers::copy` rebuilds the destination's container from the
  stored bytes (`dead/containers.rs:135-150`). The collection key differs between two
  calendars, so the copy unseals under the source calendar's key and reseals under the
  destination's, in `za_copy_move_event`'s path. Cross-account copies stay refused
  (calendar spec section 8).
- **Bookkeeping:** the `Viewer` and `Owner` records stay in upstream's format.
- **Other object kinds:** a key account's address books, contacts and files keep their
  containers in the clear, as at 0.16 and upstream; those objects were never sealed for key
  accounts. Emails, mailboxes and Sieve scripts are reachable only through IMAP METADATA
  and JMAP object metadata, which are refused.
- **Leak rule:** a key account's calendar or event document has no `Private` container, and
  each of its `Shared` containers is sealed; the 12-byte bookkeeping records are permitted.
  It is not a name check: names are never in keys.

Keep the extras codec only for the parts of the sealed content that still live in the
content archive.

IMAP METADATA and JMAP object metadata are refused for key accounts in this port.
- **IMAP:** the refusal covers `GETMETADATA`, `SETMETADATA` and `LIST … RETURN (METADATA …)`
  (`imap/src/op/list.rs:181-182`).
- **JMAP:** object metadata is a property layer inside the existing `*/get`, `*/set`,
  `*/query` and `*/copy` methods, not a new method, so the dispatcher gate cannot catch it.
  The refusal sits in `jmap/src/api/metadata/` (`write`, `apply`, `get`, `preload`, `copy`).
- **Presence flags:** object writers record which metadata kinds exist
  (`email/src/presence.rs`, `common/src/storage/index/presence.rs`). They name kinds, not
  values, and stay upstream's.

### Authentication and authority

Target `Server::authenticate(&AuthRequest) -> trc::Result<Authenticated>` (unchanged in
1.0.1), keeping upstream's `Authenticated { access_token, expires_in }` and its scope and
revocation handling. Attach session keys separately in the authorized protocol layers.
Restore the marker diversion in the new basic-auth path, along with app-password publication,
the vault generation fences and credential redaction. The common authentication result stays
keyless. 1.0.1's `HttpAuthenticated { in_flight, access_token, epoch }` and
`GrantRevocations` are the cache's new scope and revocation state, and the fork's cache
adaptation keeps them.

Keep the external-directory refusals and the marker check at their 1.0.1 sites:
- `za_refuse_directory_login` goes in `authenticate_basic`'s directory branch;
- `za_refuse_directory_account` goes after `directory.authenticate` returns and before
  `assert_account_directory`, which keeps 0.16's error order (a key account is refused
  before a directory mismatch is reported);
- `merge_directory_secret`'s marker check goes in `synchronize_account`.

The `za::external_directory` module (PR #66) is their regression test. The Bearer arm needs
an OIDC provider and is not covered locally. What a directory synchronisation may change on
a key account besides its credential is an open product question (fork issue #67), outside
this port.

Extend the new account-cache loader with the existing key-account classification and epoch
rules. No cached hint replaces the vault and calendar reads a write needs. Keep the quiet
catch-all resolution and the exact ordering of account and vault failures.

### Calendar and sharing writes

`CalendarEvent` is metadata, and `CalendarEventContent { data, preferences }` holds the
tree. Port read and write views together. `CalendarEvent::{insert,update_full,update_meta}`
use `GroupwareWrite`/`SplitUpdate`, which serialize the content and recompute the metadata
from it (`SplitUpdate::full` calls `refresh_from_content`). Sealing and metadata
sanitization must therefore happen before those builders, including on metadata-only
updates, conversion, drain commits and COPY/MOVE.

Port the owner/grantee resolver, the conversion journal, the event and link assertions, and
the lease, pin and ring writes to `PendingId`, `AssignedIds` and the new store batch
interface. Keep the accepted scheduling limits; an upstream behaviour change needs an
explicit disposition, not a silent weakening of the lifecycle oracle. `itip_merge_changes`
now returns `Result<(), ItipError>`. The drain's hand-mirrored `merge` and `import`
propagate it, and are re-read against 1.0.1's `itip_ingest` and `commit_itip_merge`.

**The share-revocation cleanup.** 1.0.1's `ShareRevokeCleanup` task is queued by the generic
index builder whenever an ACL grant disappears. Its `stage_event` strips the lost grantee's
preferences and calls `CalendarEvent::update_full`. That re-serializes the content,
recomputes the metadata from it and changes the ETag. On a sealed event it would be a
keyless background rewrite.

**Decision:** for a key-account owner, `stage_event`, `stage_calendar` and
`stage_notification` skip the account. The fork's own revoke runs with the owner's keys:
it rotates the ring and removes the grantee's entries. The grantee-side writes the task
makes are metadata container counters, not content.

The periodic `rebuild_shares` task (from `AccountMaintenance`) runs unchanged for key
accounts. It writes only the `acl_grant` and `acl_revoke` index keys derived from object
ACLs, never event or calendar records, and keeps the grantee index in line with the ACLs
the fork's revoke maintains.

**Per-user event data.** 1.0.1 reshapes `EventPreferences` into
`{ account_id, updated, instances: Vec<EventUserData { recurrence_key, flags, color,
keywords, alerts }> }`, stored in `CalendarEventContent.preferences`. At 0.16 these
preferences are JMAP-only, and JMAP calendars are refused for key accounts, so they never
exist for a key account (calendar spec, table near line 717). The rule still holds at
1.0.1: no DAV path writes event preferences. The only DAV `preferences_mut` is
`mkcol.rs:114`, which is calendar-level, and `copy_move.rs` only retains the caller's own
entries. Keep the rule and assert it.

**New viewer-side readers.** 1.0.1 adds readers that filter or add to what a viewer sees:
- `calendar/privacy.rs`: CLASS-based filtering for non-owners, through `EventPrivacy`;
- `calendar/hidden.rs`, `participants.rs`, `overlap.rs` and `notification.rs`
  (`visible_notifications`);
- `dav/calendar/{view,filter}.rs`;
- `calendar/alerts.rs`'s `DefaultAlertsResolver`, which DAV GET and COPY run to add the
  calendar's default alerts to served events.

They run on the unsealed view, with keys, and never in keyless code. The one exception is
the privacy check, which reads the stored flags (below) wherever upstream reads them.

**Privacy flags.** 1.0.1 limits a non-owner's view of an event by its `CLASS`.
`EventPrivacy` maps PUBLIC to public, CONFIDENTIAL to secret and any other value to private
(`calendar/privacy.rs`). `refresh_from_content` stores the result as `EVENT_PRIVATE` or
`EVENT_SECRET` in the metadata flags (`calendar/index.rs:247`), on every full write
(`GroupwareWrite::full`). DAV reads those flags from the keyless resource cache, through
`EventPrivacy::from_flags` and `& EVENT_SECRET`
(`dav/src/calendar/{copy_move,freebusy,get,mod,query}.rs`,
`dav/src/common/{lock,propfind/dead,propfind/item,propfind/get}.rs`) and through the
callers of `assert_event_privacy_access` (`dav/src/calendar/mod.rs:289`) in
`copy_move.rs`, `delete.rs`, `proppatch.rs` and `update.rs`. Calendar spec
revision 7 seals `CLASS`, so on a sealed tree the flags would stay clear, and the hiding
would do nothing. 0.16.27 has no such hiding, for any account.

**Decision (Jay, 2026-10-30):** sharers can hide events, and the operator may learn each
event's classification. Calendar spec revision 8 adds field-policy version 2: `CLASS` is
visible on event, task and journal components when its value is PUBLIC, PRIVATE or
CONFIDENTIAL. Its parameters follow the level-3 rule, so an extension parameter on it is
sealed. A `CLASS` with any other value stays sealed. Upstream's `refresh_from_content` then
derives both flags from the stored, sealed tree on every write path, foreground or
background, and upstream's filters read them unchanged. No fork code computes, overrides or
re-derives the flags, so no write path can drift from the stored tree.
- **Change:** the allowlist in `groupware/src/calendar/seal/policy.rs` becomes value-aware
  for `CLASS`. `is_visible_property(component, property)` gives way to a predicate that
  takes the entry, so the build names every caller. At main they are `seal/tree.rs:366,
  482`, `seal/event.rs:435`, `sharing/format.rs:569` and the leak scanner
  (`tests/src/za/leak.rs:457`).
- **Version:** `POLICY_VERSION` stays 1. That byte opens every key-wrap envelope (calendar,
  invitation, sharing ring and mail) and is bound into the tree and timezone associated
  data. The checks compare it for equality, and the bundle records no other version.
  Bumping it would change formats that five specs describe, while field policy 2 changes no
  format. So field policy 2 is a property of the store: a 1.0 store starts fresh, and every
  object in it is written under policy 2. A later allowlist change on a store that holds
  data would need a per-object marker, a format change of its own.
- **What the operator learns:** for each event, task and journal component, whether it is
  public, private or confidential, and when that changes. The value sits in the stored
  tree. The most restrictive value over the components (upstream takes the maximum) also
  sits in the metadata row's flags, the resource cache and its swap snapshots, as for an
  ordinary account.
- **Limits:** a `CLASS` with an extension or other non-standard value stays sealed, so its
  event is served to sharees as public, where upstream would treat it as private. Common
  clients write only the three standard values. The hiding is enforced by the server, not by
  the encryption: a grantee holds the collection key.
- `EventPrivacy::may_be_set_by`, which stops a sharee from changing an owner's
  classification, runs on the submitted tree, and `set_privacy` on incoming iTIP content,
  both in the keyed write path before sealing. The drain's hand-mirrored merge must carry
  `itip_ingest`'s preservation of the stored classification (`calendar/itip.rs:854-875`).
- **Acceptance:** the same sharee requests against a key account's shared calendar and an
  ordinary account's give the same answers, for PUBLIC, PRIVATE, CONFIDENTIAL and a
  non-standard value (the recorded limit aside). Every stored event's flags match its
  `CLASS` after each kind of write.

### Mail writers and readers

Adapt stub construction and the unsealing readers to mail-parser 1.0 and
`email::message::metadata::{build,row,source,view}`. Integrate at `email_ingest_into`, at
grouped batch construction and flush (`IngestGroup`, used by IMAP APPEND and JMAP), and at
`email_ingest_finish`, not only in the single-message `email_ingest` that SMTP delivery
still calls. Classification assertions must survive pending-ID allocation, commit points,
retries, mixed-account work and post-commit notification.

Keep nested MIME and attachment offsets, section reads, encrypted uploads and every refusal
of a raw-blob consumer. Background delivery still seals to the served public key and never
borrows a user's session keys. Submission, spam training, threading and push keep their
existing content restrictions. The un-index path's `delete_email_metadata` reads the stored
row's `raw_headers` to resolve the thread subject. It must recognize a stub by its format
byte, as the index side does.

### Indexing, metadata and cache snapshots

Replace the protection that lived in `services::task_manager::index` at every new entry
point of `services::index_queue::{document,index,reindex,file}` and at the storage commit
boundaries they use. Index rebuild and attachment extraction fall under the same rule.

**The index queue.** 1.0.1 removes the `IndexDocument` and `UnindexDocument` tasks. The
queue is its own key space:
- `SearchIndexClass::Queue { index, id_prefix: account_id, id_suffix, created_at }`, with
  value 1 for index and 0 for un-index;
- a `QueueIndex { index, partition }` marker, plus control and status keys;
- all in subspace `SearchQueue` (`y`).

`services::index_queue::manager` works one `(index, account)` partition at a time under a
lock, in `created_at` order. The worker reads the metadata row first, and skips an index
entry whose row is gone. So the 0.16 race (an un-index overtaking a queued index write and
orphaning the document) is gone by construction. One window remains, between the row read
and the document write, and the next pass of the same partition repairs it.

**Decision:** adopt "ordered". Task 6 confirms it with the 0.16 interleaving reproduced by a
pause hook. Then:
- the in-flight index tests in `za::mail::fence` flip to expect no late write;
- the key-mode JMAP ACL variant's PR #66 wait comes out. It is the one under the comment
  "An unindex queued before the import's index write can orphan the document"; the
  variant's other three waits stay, for the emptiness check;
- no source-exists assertion is added.

**The provisioning fence (R54 and R58).** These retarget to the new key space.
- **R54's scan:** `Server::za_has_tasks` covers the account's `SearchIndexClass::Queue`
  entries for every `SearchIndex`, plus the partition's status key. It no longer matches the
  removed task types.
- **The exhaustive match over `Task`** classifies the three new types.
  `ShareRevokeCleanup` counts for its `account_id`, the owner: the task record holds only
  the owner, and its grantee-side writes are container counters, not content.
  `DmarcReportWindow` and `TlsReportWindow` are excluded.
- **No terminal failure.** A failing partition retries with backoff capped at 30 minutes
  and never reaches a "failed" state (`index_queue/manager.rs:316-359`). So a queue entry
  blocks provisioning until it drains. `x:IndexQueueStatus/set`
  (`jmap/src/registry/mapping/index_queue.rs:191-240`) resets a partition's retry state, so
  it retries at once; it removes no entry. The operator's remedy is that reset for a
  transient failure. For a permanent one (a document that fails to build every time), it
  is deleting the document, whose entry then becomes an un-index, or deleting and
  recreating the account, as for any account that setup refuses. This replaces CLAUDE.md's
  failed-task remedy for index work.
- **R58's vault-absence assertion** moves to the batch the worker commits, `account_batch`
  (`store/src/search/index.rs`), not to the queue entry, which the producer writes. The
  layering is the same as at 0.16. `store` cannot call into `common`, so the worker
  (`services/src/index_queue/index.rs`) attaches the assertion through a store-side field on
  the documents it passes to `index_account_documents`, and `account_batch` places it in the
  batch.
- **R62 and R63 have no subject.** `Store::index`'s chunk loop is gone, and `account_batch`
  commits each pass in one write with no commit point (`store/src/search/index.rs:299-341`),
  so both take the "subject deleted" disposition with that evidence.

**Cache snapshots.** Keep them working on stored, sanitized views only. Neither cache
population nor snapshot serialization and restoration may accept an unsealed response view
or `SessionKeys`. Test persistent file and blob snapshots and the shared codecs, including
stale loaders racing provisioning and revocation.
- The swap tier's registry default is `Disabled` (`CacheSwap::{Disabled, LocalFile, Redis,
  BlobStore}`). The tier is built once, at boot (`Caches::parse` → `SwapTier::build`,
  `common/src/config/inner.rs:125`), and `cache/reload.rs` does not rebuild it, so a running
  server cannot change tiers. The za suite's server keeps the default, which matches
  upstream and the baseline.
- The swap tests are a suite function of their own (Jay's decision, 2026-10-30):
  `za::cache_swap::cache_swap_tests` boots a server with `LocalFile` under its temporary
  directory, then a second one with `BlobStore` on a filesystem blob store, each with
  `cache.swap` set in its bootstrap. It hands `Caches::parse`'s receiver to
  `SwapTier::start`, as boot does. CI runs it as its own step.
- Redis and cluster operation remain unqualified. The central serialization and authority
  guards must not depend on which snapshot backend is selected.
- Checksum framing named `seal` is not encryption and cannot satisfy this contract.

## Test suite ownership

### The baseline

The baseline was measured on 2026-10-29 at `5b914ddb5`. Its raw listings, logs and coverage
exports are in the git-ignored `.superpowers/sdd/baseline-01627/`; the plan commits a
reduced form.

- **What 1.0.1 ships:** no public test.
  - The tree has no `#[test]`, no `tests/` crate, no `test_mode` feature and no
    `.github/workflows/test.yml`.
  - Upstream links its tests through 562 `#[path = "…/stalwart-tests/…"]` slots that point to
    a private sibling checkout (560 at 1.0.0).
  - 534 of the slots compile only when that crate's own tests are built. 27 are support
    modules compiled once the flag is set, two of them only with a store feature.
- **What the fork has at 0.16.27:** 795 test functions.
  - 697 are unit tests in `crates/`: 466 from upstream, 183 of those in the SEL-only scim
    crates.
  - 98 are in the `tests` crate. Its suites make 197 sub-test calls.
  - Six tests were already `#[ignore]`d: `nlp`'s `text_classifier`, `spam-filter`'s pyzor
    `send_message`, the SMTP `asn`, `milter_client_test` and `dane_live_smtp_hosts`, and
    `webdav::cal_query::ical_roundtrip_expansion`.
  - After PR #66, CI selects 609 of the 795. The other 186 are the SEL-only scim tests and
    the `cluster_tests`, `stress_tests` and `scim_tests` suites.
- **What passes:** every test in the local net except `automation_tests`, which needs
  Docker, as does `directory_tests`. Both pass in CI.
- **Coverage of that net,** measured with llvm-cov over 19 steps:

  | Code | Lines | Functions |
  |---|---:|---:|
  | Product code | 63.9% | 64.8% |
  | Product code, without the generated `registry` crate | 73.0% | 71.2% |
  | Fork-owned code (40 files) | 96.4% | 95.0% |
  | Upstream files the fork edited | 75.4% | 75.7% |
  | Upstream files the fork never touched | 57.1% | 57.8% |

  173 of the 197 sub-test calls ran, by the baseline's first, permissive rule: a positive
  count on either the async function or its body. The rest need Docker, are SEL-only, or
  are gated by a feature or an environment variable. Task 1 recomputes "ran" with the strict
  rule below from the stored region-level exports.
- **What a plain merge does:** merging v1.0.1 deletes 541 of the 599 files under `tests/` at
  `9a7574203`. 520 of those go without a conflict, and 21 as modify/delete conflicts. It
  also deletes every upstream unit test in `crates/` and `.github/workflows/test.yml`.

### Decisions

1. **Keep the 0.16.27 suite and port it, restoring upstream's tests verbatim wherever
   possible.** A test leaves the required set only with a recorded disposition.
   - **Subject unchanged or renamed:** restore the test verbatim, adapting only names and
     call shapes. This is the default.
   - **Internals replaced in 1.0:** write an equivalent test of the new code when the test
     exercised public behaviour (a parser, a protocol exchange, a stored format) or sits in
     a fork hotspot module. Otherwise record "internals replaced, no equivalent", with the
     1.0 diff hunk. Upstream-crate coverage is reported against the baseline, so what this
     costs is visible.
   - **Subject deleted upstream:** recorded with the 1.0 diff that removed it.
   - **SEL-only:**
     - the `scim` and `scim-proto` crates;
     - `tests/src/directory/oidc.rs`, the SEL-only `tests/src/scim/*` files,
       `tests/src/system/archiving.rs` and `tests/src/system/tenant.rs`;
     - the sub-test calls to them, including `system::tenant::test` and
       `system::archiving::test`.
   - **Ungated as today:** `cluster_tests`, `stress_tests` and `scim_tests` keep compiling
     but stay out of the required set.
   - **Ignored as today:** the six `#[ignore]`d tests stay ignored and keep compiling.
2. **Restore everything at merge time.**
   - Right after `git merge --no-commit`, restore `tests/` and `.github/workflows/test.yml`
     in full from the fork side, before resolving anything else, and record the restored
     paths. Task 8 rewrites the workflow later.
   - Move each 0.16.27 inline test module of upstream's into a fork-owned file next to its
     1.0 counterpart, with a file name unique to its module:
     - for a module file `<stem>.rs`: `#[cfg(test)] #[path = "<stem>_restored_tests.rs"] mod tests;`;
     - for `mod.rs`, `lib.rs` or `main.rs`: `#[path = "restored_tests.rs"]`.

     A `#[path]` resolves relative to the directory of the file that declares it, so one
     shared file name would collide wherever sibling modules share a directory: 20
     directories at 0.16.27, `common/src/auth/oauth/` among them. The module keeps the name
     `tests`, so its libtest identifiers are unchanged. As a child module it sees the
     parent's private items, as the inline module did. Task 1 records the mapping from each
     source module to its restored file.
   - A module whose subject moved follows its subject.
   - Bring the fork's own unit tests along with their modules.
3. **Restore only the test hooks the suite uses.** Restore the `test_mode` features only
   where the retained suite needs them. Write each hook as a one-line call into a fork-owned
   module, as `common::za::test_hooks` already does. Re-derive the unit-test step's feature
   list for 1.0's workspace at the same time.
4. **Replace the one SEL-only helper an AGPL test needs.** The dual-licensed
   `tests/src/directory/issuer.rs` imports `get_token_for_client` from the SEL-only
   `oidc.rs`. Write a replacement from the OAuth 2.0 token-endpoint specification (RFC 6749),
   not from the SEL file.
5. **Keep an expectation ledger.** Every changed assertion in a ported test names the 1.0
   change behind it (a diff hunk or release note) in the outcome note. So does every early
   return, `cfg` skip or new `#[ignore]` added to a ported test. Reviewers check those
   entries against source. Without upstream's own tests, this is what stops a port from
   quietly accepting a regression.
6. **Make the harness exhaustive.**
   - `wait_for_tasks` waits for the scheduled tasks and for the index queue's
     `SearchIndexClass::Queue` entries and status keys.
   - Cleanup enumerates 1.0's closed `Subspace` enum (26 variants) with no catch-all arm,
     so a new subspace stops the build. The 0.16 `SUBSPACE_*` constants are gone.
   - Against the fork's 0.16 constants, the new subspaces are `Immutable` (`c`),
     `IndexProperty` (`v`), `SearchDocument` (`m`), `SearchQueue` (`y`) and `System` (`u`).
     Three bytes changed meaning: `m` was `IN_MEMORY_VALUE`, `y` was `IN_MEMORY_COUNTER` and
     `u` was `QUOTA`. The scanner therefore keys its rules on the enum, never on the byte.
     `Value` and `Presence` are value shapes (`Shape`), not subspaces.
   - The leak scanner covers each new subspace and `Collection::Metadata`, each with a stated
     rule for what a key account may hold there in the clear, and a planted-violation
     control.

### Gates (C10)

- **Test list.** The candidate's libtest listings, per binary and feature set, are compared
  with the baseline, which carries each test's status there: run, ignored or not selected.
  - The gate fails on a required test that is missing or unselected.
  - It fails on a test that ran at 0.16.27 and is now ignored.
  - It fails on any status change that lacks a disposition.
  - A test ignored at 0.16.27 that is still ignored passes.
- **Executed sub-tests.** Rerun the baseline's 19-step coverage net on the candidate. A
  sub-test call counts as executed in a mode only if two things hold in that step's export,
  by llvm-cov's region counts:
  - **The body record has a positive count.** For an `async fn` sub-test, the body record
    is the async body's own record, whose first region starts after the signature. The
    outer function's record only constructs the future, and never counts. For a plain `fn`
    sub-test (for example `imap::body_structure::test`, `webdav::cal_itip::test`), the
    function's own record is the body.
  - **The final region has a positive count.** This is the exit region at the body's
    closing brace, whose start and end both lie on that brace. It is not the body's outer
    region, which starts at the opening brace and counts entries, so it stays positive after
    an early return. Since a failed assertion panics, reaching the exit region means every
    assertion before it passed.

  Each call must execute in every mode where it did at 0.16.27, under the same rule. When
  Task 1 recomputes the baseline, it reports every sub-test that completed at 0.16.27 but
  fails the rule, for example one whose body ends in a diverging or branching tail. Such a
  sub-test gets an explicit completion marker; the rule is not loosened.
- **Coverage.** Line coverage of the fork-owned cohort must not fall below 96.4%. The
  cohort is the baseline's 40 fork-owned files, mapped across moves, plus fork runtime code
  added during the port, listed as it is added.
  - Restored upstream tests (`*restored_tests.rs`) and the `tests` crate are reported as
    groups of their own. Neither counts toward the cohort or toward product code, so copied
    test code cannot offset lost coverage of the zero-access implementation.
  - Upstream crates are reported, and a drop is explained.
- **Gate self-tests.** In disposable changes, show that each of these fails the gates:
  - an omitted sub-test call, async and synchronous;
  - a constructed but unawaited sub-test future;
  - an early return before a sub-test's assertions;
  - a newly ignored required test;
  - a dropped restored module;
  - a coverage drop.

  Also show that the unchanged baseline passes, including its synchronous sub-tests.
- **Mutation sample.** Disable each re-ported protection hook in turn, and record that a
  named test fails. Tests adapted to new APIs can pass vacuously.
- **Upstream's hidden tests.** Record the number of private-test slots in each upstream
  release. A new slot marks a module where upstream added a test we cannot see; review that
  module for fork-relevant behaviour.
- **CI.** Port every job PR #66 added (`unit-tests`, `upstream-suites` and the
  `zero-access` additions) to the 1.0 commands. Keep the python-caldav runner. The external
  protocol suites are deferred (see the scope exclusions).
- **Product.** The OSS create-and-restart gate creates fresh accounts and data, stops the
  process, restarts on the same store, and checks mail, calendars, pins and sharing after a
  fresh authentication. This is a persistence test on one version, not a migration test.

## Execution and release gates

The [implementation plan](../plans/2026-10-27-stalwart-1-upgrade.md) uses one integration
branch with bounded ownership. No permissive shim, skipped protection, ignored test or
temporary production feature gate may manufacture a green build. Each component task ends
with a crate-level `cargo check`, so that implementers can check their own work before the
integrated gate. `common` depends on `store` and `types`, and every later crate depends on
`common`, so the order is:
- Task 2 closes with `types`, `store` and `vault`. The fork's hooks in those crates are
  ported first, with R58's carrier type kept compiling. Those hooks are the reserved fields
  and collection, `ZA_UID_LEASE_PREFIX`, `za_test.rs` and `has_account_search_terms`;
- Task 3 closes with `common`;
- Task 4 with `groupware` and `dav`;
- Task 5 with `email`, `imap`, `jmap`, `smtp` and `pop3`;
- Task 6 with `services` and `store`.

1. Approve this design and the plan.
2. Merge the pinned release in an isolated worktree, restore the tests and the test
   workflow, inventory the fork's hooks, and settle the shared interface changes.
3. Complete the coupled core port through its crate-level checkpoints, and reach a product
   and test compilation gate free of conflicts, with every contract mapped to code.
4. Pass the C10 gates, the units, the za and leak suites, the plain and key protocol suites,
   the retained ordinary-account CI, the python-caldav suites and the fresh OSS
   create-and-restart gate.
5. Complete a fresh, clean-context review of the whole branch. Close its findings, and rerun
   the affected complete gates after the fixes.
6. Record the exact candidate SHA, commands, results, skips, retries, dispositions,
   expectation-ledger entries and artifact provenance. Native client checks remain a
   separate gate under the existing manual checklist.

The next fork preview is `0.2.0-beta.1`, because storage compatibility changes (Jay
confirmed on 2026-10-30). This does not rename the fork to upstream 1.0. Publish only with
separate authorization, and keep any release with pending native gates explicitly limited
and marked as a prerelease.

## Source references

- [Pinned upstream release](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)
  and its parent [v1.0.0](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.0): the
  new database layout, metadata, cache and index changes, and the move to private tests.
- In the target:
  - `crates/common/src/auth/authentication.rs`, `cache/swap/`, `storage/index/split.rs` and
    `storage/metadata/`; `crates/common/src/manager/schema.rs`;
  - `crates/groupware/src/calendar/{mod,storage,privacy,hidden}.rs`; `crates/dav/src/common/dead/`;
  - `crates/email/src/message/{ingest,metadata}`; `crates/jmap/src/api/metadata/`;
    `crates/imap/src/op/metadata/`;
  - `crates/services/src/index_queue/` and `task_manager/tasks/{grantee_cleanup,rebuild_shares}`;
  - `crates/store/src/search/index.rs` and `write/{batch,key,metadata}.rs`.
- The baseline in `.superpowers/sdd/baseline-01627/`; PRs #62 and #66; fork issue #67.
- The fork's `CLAUDE.md`, existing tests and the linked approved designs take precedence for
  the preserved contracts. Upstream's performance claims are not qualification of this fork.
