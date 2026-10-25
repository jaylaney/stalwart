# Calendar sharing API

This handoff describes the release 4 implementation against revision 14 of the
[sharing design](../superpowers/specs/2026-10-22-zero-access-sharing-design.md).
The separate Circulo page and its `docs/api.md` integration remain pending. Automated evidence and
measurement conditions are recorded in the [release
outcome](../superpowers/plans/2026-10-23-zero-access-plan14-outcome.md).
Native-client qualification remains pending.

## Common request and error contract

All three routes are `POST /api/vault/...` with a JSON body and `username` and `password` strings,
plus optional `totp` string. They freshly verify the primary password and TOTP, build request-local
session keys, and never use the CalDAV key cache. App passwords and master-user login syntax are
400. CORS is limited to the configured vault account-page origin. Success and fixed 409 responses
are `application/json; charset=utf-8`, `Cache-Control: no-store`, and excluded from response-body
tracing. Anonymous request limiting and failed-auth accounting apply; business 409s such as `not a
key account` are not failed logins. A malformed request, missing `DavCalAcl` permission, 401
incorrect credentials/code, 402 missing enrolled TOTP code, 403 disabled account/tenant, 429 rate
refusal, and server errors use the server's generic `application/problem+json` body; callers should
branch on status, not problem wording. The JSON serialization parser deliberately suppresses echoed
input values. The exact generic problem representation is the server's existing error adapter, not a
new sharing-specific schema. A 409 produced by these handlers has exactly `{"error":"<fixed
reason>"}`. A missing or stale vault gives the shared fixed reasons `vault record missing`, `account
is not active`, or `vault changed since verification`; a non-key caller gets `not a zero-access
account`. Corrupt sealed data/store failures are server errors, not normalized to a sharing 409.

A grant or revoke error does not prove that nothing changed: conversion work or the mutation
batch can commit before a later boundary-completion error. Re-list to check current state and
retry the same operation as needed, using the stable grant id for revoke. Retries are idempotent;
work counts still describe only that HTTP request's acknowledged work. A pending sync boundary
continues to refuse old tokens until a later owner operation completes its repair.

`calendar` in grant/revoke is the caller's own calendar URL slug, excluding inbox/outbox. Empty,
over 255 bytes, or `/` is 400 `invalid calendar` through the generic problem adapter. An `address`
must be a normalizable bare address or is 400 `invalid address`. Input and enum validation precedes
password verification. Mutations require `DavCalAcl`; list does not.

## List: `POST /api/vault/shares`

Request: `{"username":"alice@example.com","password":"...","totp":"..."}` (`totp` optional). 200
body shape:

```json
{"calendars":[{"calendar":"family","name":"Family","shared":true,"grants":[{"grant":42,"address":"bob@example.com","level":"edit","fingerprint":"<display fingerprint>","granted_at":1790000000,"pin_source":"confirmed","state":"active"}]}],"shared_with_me":[{"owner":"carol@example.com","calendar":"trips","href":"/dav/cal/carol%40example.com/trips/","name":"Trips","level":"view"}]}
```

The values above are illustrative, not a fixture result. `calendars` covers every own calendar and
sorts by `calendar`; its `grants` sort by recorded `address`. `shared` describes shared storage form
and stays true after the last revoke. `grant` is the stable grantee account id. `pin_source` is
`first-use`, `confirmed`, or null; `state` is `active`, `key changed`, or `gone`. `fingerprint` is
of the key recorded in the grant, and `granted_at` is Unix seconds of grant or last re-key.
`shared_with_me` covers eligible foreign key-account calendars only and sorts by owner then
calendar. `href` is the encoded CalDAV path. `name` can be null if the caller's grant ring will not
open at the grant-ring AEAD step; it otherwise comes from the caller's personal entry. Failure to
open the caller's own private key, or a vault/store failure before that step, can fail the whole
list instead of returning a partial own-calendar list. An assisted own-home DAV listing can
likewise fail at request level for an eligible corrupt foreign calendar; the accepted error
contract permits request or item 500. Plain-account shares are omitted. List
may perform the existing private-key self-check/owner public-key repair while opening a caller's
foreign grants; that possible side effect is distinct from a grant/revoke.

## Grant: `POST /api/vault/shares/grant`

Request:
`{"username":"alice@example.com","password":"...","totp":"...","calendar":"family","address":"bob@example.com","level":"view"}`
(`totp` optional; `level` exactly `view` or `edit`). On 200:
`{"ok":true,"changed":true,"rewrapped":N,"sealed":M}`. `changed:false` means this request
acknowledged **no write**, including no phase-1 ring-only conversion write. Level changes, re-key,
first grant/conversion, and gone-grant sweep can make it true. Counts are this HTTP request's
acknowledged event DEK-envelope rewrites and full legacy-plaintext seals, including a rollback then
forward rewrite of the same event twice; they are work counts, not distinct-event totals. Ring-only
writes, skips, failed attempts, another request's work, and previous invocation work are excluded. A
converted ring skips the historical event scan for later grants/no-ops/level changes/re-keys. First
grant can take the time needed to convert every event.

A disposable in-process test-profile measurement used 3,000 events: 1,500 EWK-sealed and 1,500
legacy plaintext. The full grant took 2928 ms and returned rewrapped=1500, sealed=1500. After an
injected store error following 100 acknowledged conversions (50 envelope rewrites and 50 full
seals), the new resumed request took 3040 ms and returned rewrapped=1450, sealed=1450. Each fixture
is a small single VEVENT with a unique UID and fixed summary, without attendees, alarms or
recurrence. Both calendars retained their complete 3,000-id/UID inventory; all Alice bodies were
unchanged and every Bob GET using Bob's own credentials matched. Request timers include sending
through complete response-body parsing and exclude verification reads. Conditions: Mac13,1, 10 CPU
cores, 64 GiB RAM, macOS 27.0.1 arm64, Rust 1.99.0, RocksDB, warm storage/owner paths, test profile
opt-level=0 with test_mode password parameters; this is not release-profile throughput. The failed
request included two induced pause points; its duration and separate workflow wall time are in the
outcome.

Address resolution requires a local, setup-complete key account other than the caller. The address
must have a fresh owner pin whose public key matches that account's freshly served key. A first-use
pin is sufficient; the page may recommend out-of-band confirmation. A grant to a different alias of
the same account updates its existing id-based grant and retains the ledger's recorded address.
`maxShares` counts the entire current ledger, including gone entries, before conversion or sweep. If
full, revoke or make an unmatched revoke that sweeps gone records, then retry the grant. A refused
capacity check writes nothing. The first conversion refuses linked events.

Fixed 409 bodies: `{"error":"unknown calendar"}`, `{"error":"not a key account"}`, `{"error":"cannot
share with yourself"}`, `{"error":"contact not pinned"}`, `{"error":"contact key changed"}`,
`{"error":"pin record missing"}`, `{"error":"vault record missing"}`, `{"error":"too many shares"}`,
`{"error":"calendar has linked events"}`, `{"error":"account deleted"}`, `{"error":"vault changed
since verification"}`, and `{"error":"conflict"}`. `vault record missing` can also mean the
authenticated absent/unusable-pin WrapMark was neither pinned nor legacy; `pin record missing` is
the pinned-mark state, and `contact not pinned` the legacy-mark state. Other shared 409 reasons may
arise from verification. 400 covers missing/malformed fields and invalid level; 401/402/403/429 use
the common contract.

The usable-pin path does not open the owner's private wrap. With an absent or unusable pin record,
grant transiently opens the existing private-key carrier **solely to authenticate its WrapMark**,
drops the zeroizing private Secret before another await, rereads pins fresh, and either uses the
final pin/CAS or returns the mark-classified reason above. It does not use that private key for
sharing, run self-check, publish a lazy private-key result, or repair the owner's public key. This
is the narrow accepted exception to the old plan sentence that said grant would never open it.
Revoke never reads pins or this mark.

## Revoke: `POST /api/vault/shares/revoke`

Request: `{"username":"alice@example.com","password":"...","calendar":"family","grant":42}` or the
same with `"address":"bob@example.com"` instead of `grant`; `totp` optional. Exactly one target is
required; `grant` must be an unsigned 32-bit integer (null, negative and out-of-range values fail).
On 200: `{"ok":true,"changed":true}` or `changed:false` if nothing at all was written. A target miss
may still sweep gone grants, rotate, and return true. Revoke by id removes exactly that
ledger/ACL/ring entry. The UI should use the id from `shares`: aliases may move. Address convenience
compares **all** exact normalized recorded ledger matches and the resolved account id if it
currently names a grant, deduplicates by id, and accepts only a single target. Multiple distinct ids
return `{"error":"address is ambiguous, revoke by grant"}` with 409 and no mutation. A recorded
address of a gone grantee can still select it.

Fixed 409 bodies: `{"error":"unknown calendar"}`, `{"error":"address is ambiguous, revoke by
grant"}`, `{"error":"account deleted"}`, `{"error":"vault changed since verification"}`, and
`{"error":"conflict"}`, plus shared verification reasons. 400 covers calendar or target
shape/address, 401/402/403/429 the common contract. Revoke neither reads a pin nor opens the owner's
private key.

## Page flow, authority, and privacy

For a new share, list calendars, run `contacts/lookup`, compare the fingerprint out of band, use
`contacts/pin` if needed, then grant at view/edit. `contact not pinned` and `contact key changed`
return to comparison; `pin record missing` uses the key-pinning record-recovery flow. A level change
repeats grant with a new level. A `key changed` grant requires comparing and confirming the new key
before pin and re-key; a `gone` grant can be revoked. Stop sharing by stable `grant` id. Product
copy: **“Removing someone stops them seeing anything new; what they could already see, they could
already have copied.”** Revocation rotates future-write CK; existing event ciphertext is not
resealed and could have been copied. A grantee's next fresh request is denied; a request already in
flight may finish from its existing keys, while its write loses the calendar assertion. Grant/revoke
cache invalidation runs after the commit. Cluster nodes require pubsub for prompt token refresh; a
missed invalidation can leave resident keys until their existing hard cap, although fresh use-time
ACL/ring checks refuse revoked access. This is a deployment limitation, not a claim that all nodes
synchronously evict.

View grants read/query/sync and cannot write. Edit adds event and calendar-property writes,
including shared Apple colour/order dead properties; it does not grant calendar deletion,
re-sharing, the WebDAV `ACL` method, or free-busy-only rights. Assisted discovery makes eligible
foreign calendars visible; the `shared_with_me.href` supports manual setup. JMAP calendars/shareWith
stay refused for key accounts. A grantee's scheduled writes from the owner's calendar send as the
owner with `SENT-BY` the grantee; the recipient validates it at drain. An attendee invited to a
calendar also shared with them should respond from their **own invitation copy**. A delegated `3.8`
delivery refusal was decided under the grantee's pins: the owner's contacts page cannot redeliver
it; only an event edit that builds a new message to that attendee tries again. A grantee organizing
under its own address from the owner's calendar inherits the upstream limit that its replies are
history, not merged.

No endpoint returns CK, EWK, MK, private key, event content, or another account's grant list.
Owner-visible addresses and fingerprints are deliberate metadata. New sharing traces carry fixed
details, ids/levels/counts only. The inherited auth success/failed-auth AccountName field may
contain the caller's login address; do not generalize that exception to sharing traces or target
aliases. Persistent shared content is encrypted; the server handles plaintext, credentials and
session keys during authenticated use. Logout alone is not an immediate key-erasure guarantee:
existing cache residency and its hard cap still apply. Current stored old-generation ciphertext
remains accessible to someone who already held its key.

Addressed PROPFIND retains an existence residual: a caller with some grant from an owner can
distinguish an absent guessed calendar slug or event filename (404) from an existing path in a
calendar the resolver refuses (403). This confirms guesses; it does not enumerate names or expose
event content. A foreign caller without any grant from that owner is refused uniformly at the URI
gate. The vanished-path filter separately prevents bulk disclosure of ungranted deletion paths.

## Calendar sync compatibility

Foreign incremental calendar sync authorizes deletions by source calendar ID and that same
calendar's fresh current path prefix. Current live items retain calendar-ID authorization.
Every new Calendar tombstone writes its original path to collection 252 and an identity/path
sidecar to reserved log collection 250 in the same physical transaction. Ordinary readers keep
using the unchanged original path format. Calendar retention and account destruction remove
both logs; calendar, ring and token formats do not change.

Relevant raw and identity history must match as multisets of `(commit change ID, exact path)`
before any deletion response or pagination count. Private identities cover their raw history
but never authorize a response. Missing, extra or malformed relevant identity coverage, or
truncated Calendar history, returns 403 `DAV:valid-sync-token`; initial sync emits no tombstones.
Unrelated history does not widen a direct-calendar request's authorization/error scope.
Independent log scans can conservatively require resync if a write or purge falls between them.
The existing resource cursor is retained, never advanced to an independently observed log tail.
Legacy or mixed-version writers can leave relevant raw-only history. A raw tail above the latest
Calendar resource cursor can continue requiring resync after an initial sync; exactly one full
sync is not guaranteed to clear that condition. No path-only fallback or guessed cursor floor
is used. This additive correction increases ordinary Calendar log storage, write-batch byte
accounting, and foreign sync scan/memory costs.

## Corrupt sharing state

A mismatch between a calendar's encrypted grant ledger and its plaintext ACL makes grant,
revoke and gone-grantee sweeping fail closed with a server error. No sharing API repairs the
encrypted ledger from the plaintext ACL. Own reads or listing may still work when the remaining
archive is valid, but arbitrary corruption can also prevent them.

Preserve readable content before destructive recovery. Use a verified consistent backup of the
encrypted calendar and grant state, or, where the owner can still read/export, an owner-controlled
export, recreation and regrant to verified recipients. Verify the replacement's content and access
before deleting the damaged calendar. Blind deletion is not a general recovery procedure.

## Collection COPY/MOVE conflicts

Key-account collection MOVE over an existing destination commits the overwrite, children, source
deletion and default-calendar change in one physical transaction. An original calendar/event
condition loss leaves both collections unchanged by that request. A fresh membership scan is
fenced by the account change counter; any concurrent indexed change in that account can
conservatively return 409. Refresh before retrying. An identical unconditional retry starts from
the still-readable source; no moved prefix was published by the failed request.

This key-account existing-destination collection MOVE route returns 409 before planning
mutations on FoundationDB, SQLReadReplica and Ephemeral stores. FoundationDB's cached read
versions and independently selected SQL replicas cannot guarantee the membership scan is at
least as new as its counter sample; Ephemeral does not roll back failed batches. Existing
source/target shared-domain checks still return 403 first. Direct RocksDB, SQLite, PostgreSQL
and MySQL retain the bounded route. Runtime qualification recorded for this correction is
RocksDB only; the other backend dispositions are source-based.

This route refuses with 409 before mutation if its prepared transaction splits, exceeds 1,000
structural operations, or exceeds 5,000,000 accounted bytes. Accounting includes materialized
logs, keys/scalars and callback archive parameters; it bounds the known archive-copy callbacks,
not arbitrary callback output. Oversized operations require smaller client operations.
Collection COPY retains its existing committed-prefix behavior, so inspect both collections on
a conflict. Event COPY/MOVE, ordinary accounts and an absent-destination calendar rename retain
their existing routes. Shared collection COPY/MOVE remains refused except the allowed rename.
