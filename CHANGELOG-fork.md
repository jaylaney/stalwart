# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

## 0.4.0-beta.1 — 2026-11-05

Limited prerelease candidate `55bb9ca67d91fe84e9b7122681f71c5746c86214`. Fully integrated upstream Stalwart `v1.0.3`, commit
`8b32686532d34623e9ce8e1237cf2bed9554af38`. Release identity is fork `0.4.0-beta.1`. CI on
the final documentation head, Linux ELF inspection and publication remain pending.

### Operator actions

Qualification covers fresh stores only. If an operator carries over an existing store,
previously enabled per-account encryption at rest resets to `Disabled`. Reselect the public
key in each affected account's encryption settings to resume encrypting new mail. Start once
with `STALWART_UPDATE_ROLE_PERMISSIONS=true` to add missing default-role permissions, then
remove the variable. Third-party clients that manage app passwords, API keys or account
settings need the applicable new OAuth scopes. See the [upstream changelog](CHANGELOG.md).

### User-visible changes

- Only a committed ordinary delivery contributes to key harvest, as decided after each
  recipient's outcome. Key accounts, refused or failed deliveries, no-write Sieve outcomes
  and duplicates never contribute. An ordinary co-recipient with a committed delivery still
  contributes its own correspondent information.
- Per-account encryption-at-rest settings are inert for key-account ingest. Ordinary ingest
  that fails during provisioning can leave the upstream `sample_blob` staged. Sealed staging
  uses no compression and the Email profile's encryption. The operator-held blob key remains
  outside the guarantee. Backups carry decoded payloads and swap snapshots use plain codec
  settings.
- Key-account `EmailPush` becomes a content-free `StateChange`. For configured `EmailPush`
  subscribers, that converted notification is delivered regardless of subscription `types`,
  including an empty list. Generic state changes still use type filtering.
- iTIP gates now cover trait entry points. Full REQUEST pinning removes omitted overrides only
  up to the master SEQUENCE. Explicit CANCEL keeps its existing sequence rules.
- Upload checks membership before the keyless refusal, hiding key-account classification from
  nonmembers. The configured outbound gateway recipient-key fingerprint is traced beside queue
  event addresses.
- Default-role edits bump the permission epoch. The inherited narrow spill-cache publication
  race remains. The epoch bump narrows stale reuse.
- The leak scanner now reports physical matching files and current decoded matching blob
  values separately. Stop the local server first. Compatible `ldb` and Python 3.14+ are
  required, and encrypted or undecodable logical values make the check fail.

The local matrix spans several tested commits. Unit rows U1–U4 (598, 16, 6 and 238 tests),
zero-access Z1–Z4 including cache swap, and protocol rows D1–D3, I1/I2, J1/J2, S1–S4, T1 and Y1
passed. The final SMTP aggregate on `55bb9ca67` recorded 75 passed, 1 failed and 3 ignored.
One exact-name retry of `smtp::reporting::analyze::report_analyze` passed. The failure cause
is undetermined. Ten mutations each failed at the intended assertion. Product build, identity,
CalDAV (plain 68 passed, 1 skipped and 3 xfailed, key 74 passed, 1 skipped and 2 xfailed) and
all nine fresh-store restart readbacks passed on `55bb9ca67`. See
[merge outcome](docs/superpowers/plans/2026-11-05-upstream-1.0.3-merge-outcome.md).

## 0.3.0-beta.1 — 2026-11-04

Immutable tag: `fork-v0.3.0-beta.1`. A limited prerelease on upstream Stalwart `v1.0.1`, and
the first with release 5: invitations by email (iMIP) for key accounts. Published as a
GitHub prerelease, it claims no general client or production readiness until its pending
gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.3.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.3.0-beta.1^{commit}'`. The published release notes and shipped
  `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.1`, commit
  `23b69755cb3cd8ecd952fe0443c5ac067a506184`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)), unchanged
  from `0.2.0-beta.1` and pinned in `fork-release.toml` and in the verifier's
  `PINNED_UPSTREAM_TAGS`.
- No additional upstream backports. Upstream `v1.0.2` and `v1.0.3` are released and not
  integrated: none of their changes is in this preview.
- The published `fork-v0.1.0-beta.1`, `fork-v0.1.1-beta.1` and `fork-v0.2.0-beta.1` tags and
  artifacts retain their original identity.

### Changes

Release 5 (issue #18; [spec](docs/superpowers/specs/2026-11-02-zero-access-imip-design.md),
[outcome](docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md)), merged through
PR #76. Before it, a key account's outside and plain local attendees were not invited
(`5.3`), and iMIP mail delivered to a key account had no calendar effect.

- **Key accounts invite by email.** A key account's CalDAV PUT or DELETE, as organizer or as
  attendee, mails the recipients that are not key accounts on this server: outside
  addresses, plain local accounts, local mailing lists and groups. A key attendee's reply to
  an outside or plain local organizer is mailed the same way, and a grantee writing in a
  shared calendar mails from the owner's address with `SENT-BY` naming the grantee. The
  message is upstream's iMIP email, composed in the user's own request and handed to the
  SMTP queue only after the change commits, never through the task queue. It carries no
  RSVP link or token: recipients answer from their own mail or calendar client, and the HTTP
  RSVP page stays refused for key accounts. Invitations between key accounts are unchanged:
  sealed internal delivery, never mail.
- **Key accounts are invited by email.** An iMIP request, update, cancellation, reply, ADD
  or REFRESH delivered to a key account becomes a pending item sealed to its public key,
  with the sender's address only inside the seal; the mail itself is stored sealed whatever
  happens to the item. The account's next session (any DAV request holding its keys)
  drains the item and applies upstream's merge, so the change appears at the client's next
  sync, not on arrival. Accepting or declining in the client mails the reply back.
- **What is mailed, and `SCHEDULE-STATUS`.** Each mailed recipient records `1.1` (sent,
  delivery not confirmed), written before the commit; a later emailed reply replaces it, as
  upstream does, with the reply's REQUEST-STATUS (or `2.0` when the reply carries none),
  and a submission that fails after the commit is traced and leaves `1.1`
  in place. Never mailed:
  - an active local key account's address, reached only through sealed internal delivery
    (one that cannot receive scheduling is not attempted, `5.3`);
  - any other address in a local domain that the registry does not resolve to a local
    account that is not a key account, or to a list (directly, or through the catch-all when
    the address has no entry): a disabled address, no entry and no catch-all, a catch-all
    that resolves to neither, an account the registry no longer finds. It records `5.1` and
    is traced;
  - an address that holds a contact pin in the sender's pin record, which records `3.8`.
    When the sender's pin state cannot be read or decided, every address that would be
    mailed records `5.1`. Mail recipients are never pinned.

  A failed recipient lookup records `5.1`. The drain's delegating fan-out and key pinning's
  redelivery stay internal: a recipient they would mail is not attempted (`5.3`).
- **Inbound requirements.** An item is written only past upstream's gate (scheduling
  enabled, not spam, the recipient allowed to receive scheduling) for a sender that passes
  the authentication upstream accepts (DMARC, or an authenticated local submission) and is
  not a local key account; key accounts reach each other only through internal delivery. A
  request for an unknown UID is kept only when the sender is a local principal, a contact
  card of the account holds the sender's address, or `autoAddInvitations` is on (off by
  default); any other method needs a UID the account already knows (an event or a pending
  item). PUBLISH, COUNTER and DECLINECOUNTER are not applied. Delivery decides from the
  message and keyless indexes only; the drain repeats every check it can with the keys.
- **REFRESH.** A listed attendee's emailed REFRESH is answered by mail with the event's
  current state, after the item's deletion commits, so one session answers each REFRESH
  item. It is withheld (the item deleted, no answer) for an address the rules above would
  not mail, for a pinned address, and when the sender's pin record is unusable; a store
  error reading the pin state leaves the item pending for a retry.
- **Integrity limit (Jay's decision 11, issue #72).** A mail-borne item, sealed to the public
  key, has no authenticity, so an operator who can write to the store can forge one, sender
  included. At the user's next session a forged REFRESH naming a listed attendee makes the
  drain mail the event's current full state through the SMTP queue, and a forged delegating
  reply adds delegates, of whom a key account is invited at once. Both need a listed
  attendee that is not a local key account, unless the store writer also remaps or disables
  a key attendee's address.
- **Traces.** The fork's iMIP events (delivery, the drain, each submission, withheld and
  dropped mail) carry ids, counts, SMTP stages, reply codes and fixed reasons, never an
  address or content; delivery events show `From` as `[redacted]`, and every SMTP
  transaction the outbound path opens is redacted as a key account's. The SMTP layer still
  traces a mailed invitation's envelope, as it does for every message.
- **Rate limiters.** The release 5 submitter's own local SMTP session is exempt from the
  inbound rate limiters (ruling O-1), because a temporary refusal there is never retried:
  upstream's default throttle (25 messages an hour per sender domain and recipient) would
  otherwise lose invitations and cancellations permanently. The exemption covers every
  inbound limiter of that session, an operator's limiter keyed on the sender included; the
  outbound queue's quota and throttles still apply.
- Ordinary accounts keep upstream's behaviour: every new path is a key account's.

### Operator actions

- Invitations to outside addresses leave through the server's SMTP routing (the
  deployment's relay, or delivery by MX), from the organizer's or replier's address. The
  deployment needs working outbound mail with DKIM signing and DMARC alignment for the
  sender's domain; otherwise providers junk or refuse the invitations, and no
  `SCHEDULE-STATUS` reports it (a remote bounce returns to the sender's mailbox as a DSN).
- A mailed invitation, update, cancellation or reply exists in plaintext in the SMTP queue
  (until relay or local delivery, and its blob until the next blob purge) and at the
  recipient, and the relay sees it in transit: the recorded amendment of release 2's
  invariant 12, whose task-queue part still holds. A deployment that delivers by MX instead
  of a relay keeps an undeliverable invitation in plaintext for the queue's whole retry
  window. A local list or group is mailed as upstream mails it, so the invitation crosses
  the queue in plaintext to every member, key members included (who receive it as sealed
  mail, with no calendar effect). SMTP logs record each mailed invitation's envelope, and
  `ItipMessageSent`'s queue id links it to the event's account and document ids: the
  operator learns who meets whom outside the server, and when.
- Inbound trust is upstream's: mail that fails DMARC (forwarded mail, many mailing lists)
  has no calendar effect, and with `mustMatchSender` off any authenticated local account can
  send iMIP from any address. Turning `autoAddInvitations` on lets any authenticated
  sender's invitation for an unknown UID create a pending item.
- Keep the calendar notification limit (`maxEventNotifications`) at its default, unlimited,
  or above the longest plausible logged-out stream: once every stored notification of an
  account is pending, new items are refused, a cancellation for a pending invitation
  included, which then exists only as sealed mail.
- Local domains fail closed: an address in a local domain that the registry does not
  resolve to a local account or list gets no invitation (`5.1`). That includes colleagues
  of a relay-enabled local domain hosted elsewhere, and unknown addresses of a local domain
  whose catch-all forwards to an outside mailbox.

### Compatibility and upgrade instructions

From `0.2.0-beta.1`: the store schema version is unchanged and release 5 changes no stored
layout (a mail-borne item is a release 2 pending item with one more sealed carrier), so a
store written by `0.2.0-beta.1` is expected to open in place. No automated test covers this
upgrade: CI's fresh-store job checks a fresh store only. Back up the complete database, blob
store and configuration, and verify the restoration, before replacing the binary. A
rollback to `0.2.0-beta.1` restores that backup and loses every write made after the
upgrade; this release does not claim that a `0.2.0-beta.1` binary reads data that
`0.3.0-beta.1` has written. Events created before the upgrade keep `5.3` for their outside
and plain local attendees, and nothing re-invites them automatically: they are mailed on
the next change that builds a message for them, and an edit that builds none (an alarm, a
change without scheduling content) mails nobody.

From `0.1.x`: not supported. Upstream 1.0.1 cannot open a `0.1.x` store, so moving is the
fresh-store rebuild the `0.2.0-beta.1` entry below describes. Fresh stores are supported.

The planned shipping configuration is unchanged: Linux `x86_64-unknown-linux-gnu` with
RocksDB, without the enterprise and `test_mode` features, requiring an `x86-64-v2` CPU.
Check the artifact's provenance for its highest referenced GLIBC symbol version and test
its runtime dependencies on the deployment host; it is not a static or broadly portable
musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md), and release
5's in its spec. Key-account JMAP calendars, calendar full-text search, re-sharing,
free/busy-only grants, cross-server sharing and key-account mail sharing remain refused or
deferred, as do free/busy with outside parties and iSchedule.

### Known limits and follow-ups

- **#77, mail lost on disconnect.** Mail is submitted inside the user's request, after the
  commit. A client that disconnects before the last submission loses the mail not yet
  queued (traced `ZA_IMIP_ABANDONED`, outcome unknown), and a crash there loses it
  silently. Nothing is retried: a lost cancellation or declining reply from a DELETE is
  permanent, where upstream's task retries for three days. Issue #77 moves submission to a
  detached task.
- **#78, a vanished list.** A mailing list the registry no longer finds while a cached entry
  still names it (a registry inconsistency, or a cluster node that has not applied the
  invalidation) is mailed, and SMTP delivers to the domain's catch-all, which can be a key
  account: a plaintext invitation through the queue. The product runs one node.
- **#72, DKIM provenance** of mail-borne items (decision 11, above). A REFRESH that claims a
  listed attendee's address also needs no store write when it arrives as an authenticated
  local submission or as DMARC-passing mail from a domain the server signs for; the DKIM
  check #72 plans would not cover the submission route.
- **#70, no notification email between key accounts.** An invitation between key accounts
  sends no email; the invitee sees it at its client's next sync.
- Temporary refusals are not retried: after the commit, a 4xx from the local SMTP layer (an
  address-verification error, the outbound queue's quota, a transient queue write failure,
  or an operator's milter, hook or script) loses that message permanently. The recipient
  keeps `1.1`, as it does after a local 5xx refusal.
- Each mailed recipient is one local SMTP transaction before the PUT or DELETE answers, so a
  write that invites many outside addresses answers more slowly.
- A retried inbound delivery writes one item per attempt (key accounts skip upstream's
  duplicate check), so a retried request can drain as a second change notification and a
  retried REFRESH can be answered more than once. A sender who knows a UID can fill a
  logged-out account's pending slots, evicting drained history.
- An alias removed between delivery and the drain makes the drain delete the pending
  invitation; the sealed mail remains.
- A mail-borne item, and the event imported from it, keep the outside organizer's `PRODID`,
  time zone ids and timestamps visible, beside release 1's visible fields.
- Inherited from upstream 1.0.1: DMARC is the basis of trust for outside senders; a request,
  ADD or cancellation must come from the organizer's own address (no `SENT-BY` assistant,
  no forwarded request); a reply is applied whatever its SEQUENCE; a key attendee invited at
  a sub-address cannot reply from it (`mustMatchSender` compares MAIL FROM exactly); no
  `SCHEDULE-STATUS` follows a mail's fate.
- In a domain whose external directory looks up recipients and which has a catch-all to a
  list or a plain account, the registry's classification can differ from SMTP's, and a key
  account's unsynchronized alias can be mailed in plaintext. The product's shape (key
  accounts on the internal directory) never has this.

### Verification and outstanding gates

The [release 5 outcome](docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md)
records the local runs, mutation checks, rulings and residuals. In summary:

- PR #76's final head `95543bfd1`:
  [run 41852181302](https://github.com/jaylaney/stalwart/actions/runs/41852181302) passed
  every pull-request job: style and the gates' workflow check, unit tests, upstream's
  suites, the `Test` job, the zero-access suites, the product build with the release
  verifier, the independent python-caldav client and the fresh-store restart gate (the test
  retention gates run only weekly and on dispatch). The merge `92680dd3b` has the same tree
  as `95543bfd1`, and `81c23b37b`, where this release's branch starts, changes `CLAUDE.md`
  only.
- The test retention gates on `466f83d4e`, gates dispatch
  [run 41749193954, attempt 2](https://github.com/jaylaney/stalwart/actions/runs/41749193954/attempts/2)
  (attempt 1's runner ran out of disk during the build): the inventory gate passed, 795
  tests; coverage, runtime-only 13,180 / 13,898 covered lines against the 0.16.27
  baseline's 10,646 / 11,246, 23 lines of margin; full 23,052 / 23,859 against
  17,821 / 18,490, 56 lines of margin. An earlier dispatch had failed the runtime-only
  figure by one line, and the PR added tests for it. `466f83d4e` precedes the drain fixes
  of rulings P2-R and P2-R2 and their tests.
- Still running at preparation, and required before the tag:
  - the test retention gates on `95543bfd1`
    ([run 41852258119](https://github.com/jaylaney/stalwart/actions/runs/41852258119));
  - CI on `main` after the merge:
    [run 41858534411](https://github.com/jaylaney/stalwart/actions/runs/41858534411) at
    `92680dd3b`, and [run 41860172138](https://github.com/jaylaney/stalwart/actions/runs/41860172138)
    at `81c23b37b`.

  The published release notes record their results.
- Reviews: the final whole-branch review (three Opus lenses and a docs review); the
  PR-stage review (four Fable lenses and two PR comments, a fix wave and a scoped Fable
  re-review); the Codex re-review of `466f83d4e`, whose fixes (rulings P2-R and P2-R2) each
  had a scoped Fable review. No Critical or Important finding remains.
- Release-preparation checks, run at the preparation commit: 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed, and the
  gates' workflow check (`python3 -I scripts/tests/ci_steps.py`, 0 differences) and tests
  (`python3 -I scripts/tests/test_gates.py`, 43 passed, 2 skipped) stayed green. No product
  was built during preparation.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, from the tag through
  `product-build.yml`;
- the release 5 real-provider round, the Gmail, Outlook.com and iCloud section
  ("Invitations by email (release 5)") of the
  [manual checklist](docs/zero-access/manual-checklist.md): pending;
- native client verification of release 5's flows: pending, with no recorded results on
  this release or on `0.2.0-beta.1`;
- a clean-context whole-branch review, carried over from `0.2.0-beta.1`: pending.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the
one named above, upstream's private test suites and the external protocol suites (Dovecot
imaptest, Fastmail's JMAP test suite, Apple's CalDAVTester).

## 0.2.0-beta.1 — 2026-11-02

Immutable tag: `fork-v0.2.0-beta.1`. A limited prerelease on upstream Stalwart `v1.0.1`,
published as a GitHub prerelease. It claims no general client or production readiness until
its pending gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.2.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.2.0-beta.1^{commit}'`. The published release notes and shipped
  `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.1`, commit
  `23b69755cb3cd8ecd952fe0443c5ac067a506184`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)), fetched
  by URL and pinned in `fork-release.toml` and in the verifier's `PINNED_UPSTREAM_TAGS`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` and `fork-v0.1.1-beta.1` tags and artifacts retain their
  original identity.

### Changes

- Moves the fork from upstream `v0.16.27` to upstream `v1.0.1`. Every zero-access feature is
  carried over: key accounts, sealed calendars, invitations between key accounts, key
  pinning, calendar sharing and key-account mail at rest. Ordinary accounts take 1.0.1's
  behaviour.
- Field policy 2: an event's, task's or journal's `CLASS` is visible to the operator when its
  value is PUBLIC, PRIVATE or CONFIDENTIAL, so that sharers can hide events; any other value
  stays sealed.
- 1.0's object metadata (DAV dead properties, IMAP annotations, JMAP metadata) is refused for
  key accounts, and setup refuses an account that already holds some with 409 "account
  already holds object metadata".
- 1.0 indexes in a queue of its own. Setup refuses an account with queued index work (409
  "account has unfinished tasks"); after a transient failure an administrator resets the
  partition with `x:IndexQueueStatus/set`, and after a permanent one deletes the offending
  document or the account. A key account's mail search document now holds only its ids.
- Every account: an undecodable Bearer value is no longer quoted in authentication traces,
  and HTTP request traces print `[redacted]` for `Authorization`, `Proxy-Authorization` and
  `Cookie` headers. Every ordinary calendar write asserts that the account was not
  provisioned in the meantime, as ordinary mail writes already did.
- Setting up a key account revokes OAuth grants minted before it, and a key account refuses
  a grant token minted while it had no credential. The scheduling inbox's expiry no longer
  removes invitations the drain has not yet opened.
- A key account's sealed events and calendars use about 485 and 491 bytes more quota each
  than their plaintext, which 1.0 enforces at commit.
- Inherited from upstream 1.0.1 for ordinary accounts: a delegate is not invited when the
  delegator forwards the REQUEST (1.0 accepts a REQUEST only from the organizer), and
  calendar text search sees an event only once the asynchronous index has caught up. Key
  accounts' delegates are still invited, by the drain.
- The shipping binary's `--version` and help name the fork version, the upstream tag and
  commit, and the source commit, for example `Stalwart Server fork v0.2.0-beta.1 (upstream
  v1.0.1 23b69755cb3cd8ecd952fe0443c5ac067a506184; source <commit>; clean)`. The release
  verifier also checks that the upstream pin names its tag's commit, is an ancestor of the
  source, is the latest known upstream release in the source's history, and matches the
  internal crates' upstream version.

### Compatibility and upgrade instructions

Fresh stores only. Upstream 1.0.1 cannot open a store written by upstream 0.16 or by the
fork's `0.1.x` releases, and the fork adds no conversion: the server stamps a new store with
its schema version and edition and refuses any other. Stores from `0.1.x` are not
supported. Moving an installation from `0.1.x` means a rebuild on a fresh store: create the
accounts again, issue new setup tokens and enrol each key account again (new keys), and reload data through the clients. Keep the pre-cutover store
snapshot and the `0.1.x` binary for rollback; a rollback restores the old store as it was,
without anything written after the cutover. Back up the complete database, blob store and
configuration and verify restoration before any test.

The planned shipping configuration is Linux `x86_64-unknown-linux-gnu` with RocksDB,
without the enterprise and `test_mode` features, and requires an `x86-64-v2` CPU. Check the
artifact's provenance for its highest referenced GLIBC symbol version and test its runtime
dependencies on the deployment host; it is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). Key-account
JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants, cross-server
sharing and key-account mail sharing remain refused or deferred.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md)
records every command and result; its "Final record" section indexes them. In summary:

- The test matrix, the coverage net and the test retention gates (the design's C10 inventory
  and coverage gates) ran at `f135a9881`, every row green
  (documented SMTP flakes passed rerun by name). CI's `test.yml` at `2867bc24f`, the
  last code change before PR #69's review,
  [run 41432100574](https://github.com/jaylaney/stalwart/actions/runs/41432100574), passed
  every job: style and the gates' workflow check, unit tests (593 passed), upstream's suites,
  the zero-access suites in both blob-store modes and in plain and key mode, the independent
  python-caldav client (plain 68 passed, 3 xfailed; key 74 passed, 2 xfailed), the fresh
  store restart gate, and the test retention gates (full 21,599 / 22,335 and
  runtime-only 12,537 / 13,197 covered lines, each 72 and 44 lines above the 0.16.27
  baseline's fraction).
- PR #69's review found two port regressions (the setup-token request's concurrency slot,
  and a delegated PUT that could exceed the recipient limit), fixed with tests in
  `4a3dcde7f` and `7b088e8dc`. CI's `test.yml` on the PR's final commit `b69bb841b`,
  [run 41472049105](https://github.com/jaylaney/stalwart/actions/runs/41472049105), passed
  every job that runs on pull requests (the test retention gates run only weekly and on
  dispatch).
- The test retention gates were run again on `main` at `32d264cba`
  ([run 41484056674](https://github.com/jaylaney/stalwart/actions/runs/41484056674))
  alongside the release build, and passed with every other job of that run: 795 tests;
  full 21,639 / 22,374 and runtime-only 12,539 / 13,198 covered lines, each 74 and 45 lines
  above the 0.16.27 baseline's fraction.
- Release-preparation checks, rerun at the preparation commit: 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed. A local
  shipping-configuration build at `2867bc24f` (macOS arm64, not a release artifact) printed
  the expected identity and carried no enterprise or test feature.
- The final whole-branch review ran as four Fable lenses and two scoped re-reviews; no
  Critical or Important finding remains.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, run from the tag through
  `product-build.yml`: passed
  ([run 41485088188](https://github.com/jaylaney/stalwart/actions/runs/41485088188)). The
  published release notes record the checksums, the attestation and the measured GLIBC
  requirement of the real artifact;
- the native client [manual checklist](docs/zero-access/manual-checklist.md): no recorded
  results; pending. Results from the `0.1.x` base are not evidence for this release;
- a clean-context whole-branch review: pending. The final review's four lenses were given
  the execution ledger and its rulings, which is not the clean context the plan specifies.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the
ones named above, upstream's private test suites and the external protocol suites (Dovecot
imaptest, Fastmail's JMAP test suite, Apple's CalDAVTester).

## 0.1.1-beta.1 — 2026-10-29

Immutable tag: `fork-v0.1.1-beta.1`. This preview remains a prerelease.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.1-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` must record the exact
  full commit after release preparation is committed and tagged.
- Fully integrated upstream base: Stalwart `v0.16.27`, commit
  `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` tag and artifacts retain their original identity.

### Changes

- Integrates the [.27 fixes](https://github.com/stalwartlabs/stalwart/releases/tag/v0.16.27):
  iTIP updates retain incoming timezone definitions for moved occurrences; OAuth accepts
  registered loopback redirects without a path; WebDAV `allprop` omits
  `DAV:current-user-principal`; IMAP `APPEND` and cross-account `COPY`/`MOVE` update
  `EmailDelivery` state, and combined `SEARCH RETURN` options retain all matching results
  where requested. Key-account refusal and sealed-data boundaries remain in force.
- Pins WebUI downloads to `v1.0.11` and checks the version before installation, avoiding
  incompatible WebUI updates on the .27 server base.
- Adds fork regressions for the .27 calendar/discovery changes and IMAP saved-search result
  combinations. Corrects system-test timing and upload-limit restoration fixtures while
  retaining their behavioral assertions.
- Uses the public repository's fork regressions and upstream suites for qualification;
  no private upstream test suite is required.
- Restores clean-checkout retrieval of OpenTelemetry from the original project's Git URL,
  pinned to the same `ae66e97b140f70e477ab710686aafce665cc2f8b` source commit.
- Adds no stored-format migration or legacy reader. The upstream 1.0 port remains deferred;
  this .27 integration does not promise ongoing upstream .16 maintenance or support.

### Compatibility and upgrade instructions

This experimental preview is intended for disposable installations. Existing-data upgrades
from `0.1.0-beta.1`, upstream production installations or unpublished fork builds have not
been qualified. Before testing existing data, back up the complete database, blob store and
configuration and verify restoration. Keep the matching pre-upgrade backup: replacing only
the binary is not a rollback guarantee. Unchanged stored layouts and formats do not establish
cross-version archive compatibility or a supported upgrade/rollback contract.

WebUI downloads remain pinned to `v1.0.11`; use the server's checked installer rather than
substituting a newer WebUI. The planned shipping configuration is Linux
`x86_64-unknown-linux-gnu` with RocksDB and excludes enterprise and `test_mode` features.
It requires an `x86-64-v2` CPU. Check the future artifact's provenance for its highest
referenced GLIBC symbol version and test runtime dependencies on the deployment host;
this is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). This update
establishes no new backend, cluster, platform or native-client qualification.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-01627-upgrade-outcome.md)
records historical public-suite commands, results, source/artifact identities, bounded retries
and retained skip/xfail policy. Those results retain their named source identities and do not
establish execution against the future release commit or its shipping artifact.

Fresh release-preparation checks passed: eight release-metadata tests
(`python3 -B -m unittest discover -s scripts/tests -p 'test_release_metadata.py' -v`)
and thirteen product-verifier tests
(`python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`).

All four [PR62 CI jobs](https://github.com/jaylaney/stalwart/actions/runs/40904792249)
passed at `7ba688c3051fe258eea15a9fcbcd68e6e594a8c9`. PR64 changes only duplicate-test
selection in the CI workflow; runtime sources, dependency versions/features and release
metadata are unchanged from that qualified source. Its
[CI run](https://github.com/jaylaney/stalwart/actions/runs/40917113519) at
`cce8d8d8f1d60a46242a156045479978f10b17f6` passed Test (including Directory) and Check
Style; remaining jobs were still running during release preparation. The dated changelog
is the only subsequent preparation edit. These CI results do not establish verification
of the future tagged shipping binary.

**TODO before artifact publication:** verify the exact tagged Linux shipping binary,
`--version`/help identity, clean source provenance, ELF CPU/GLIBC/runtime requirements,
checksums and build attestations. No Linux shipping artifact for this release commit has
been qualified. The product tag workflow builds and verifies artifacts after tag push and
uploads them to Actions; it does not create or publish a GitHub release.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL/other stores,
multi-node execution, additional platforms and production deployment remain unqualified.
This preview must remain a prerelease and does not claim general native-client or production
readiness.

## 0.1.0-beta.1 — 2026-10-26

Preview title: `release-0.1.0-beta-1`. Immutable tag: `fork-v0.1.0-beta.1`.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.0-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` record the exact full
  commit after release preparation is committed.
- Fully integrated upstream base: Stalwart `v0.16.26`, commit
  `18f4e9505027d790fd7e9fdad7f922be6719101d`.
- No additional upstream backports are recorded separately for this preview.

### Included fork behavior

- Zero-access account setup with password, recovery key, TOTP and app-password support.
- Private calendar events, tasks, journals and calendar metadata encrypted at rest, with
  decryption during authenticated CalDAV requests for ordinary clients.
- Local invitations between key accounts, pinned recipient keys, and owner/grantee view/edit
  calendar sharing, including delegated scheduling with `SENT-BY`.
- Key-account mail encrypted at rest. Contacts and files retain upstream storage behavior.
- Independent product version and full fork/upstream/source identity in operator-facing
  version/help/startup output. Internal crate compatibility versions remain upstream-based.
- Shipping Linux `x86_64-unknown-linux-gnu` build with RocksDB, binary/provenance checksums and
  build attestations. It excludes enterprise and `test_mode` features.

### Compatibility and upgrade instructions

This is the first experimental preview, intended for disposable installations. Upgrades from
upstream production installations or earlier unpublished fork builds have not been qualified.
Before testing existing data, back up the complete database, blob store and configuration and
verify restoration. Keep the matching pre-upgrade backup: rollback by replacing only the
binary is not supported or promised. The release preparation changes no stored layout or
cryptographic format; that does not establish an upgrade/rollback contract for the fork.

The GNU/Linux binary requires an `x86-64-v2` CPU. Consult its provenance for the highest
referenced GLIBC symbol version and test runtime library compatibility on the target host;
it is not a static or broadly portable musl build.

Key-account JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants,
cross-server sharing and key-account mail sharing remain refused or deferred. The running
server receives credentials and handles plaintext during authenticated use; this is
server-side encryption at rest, not end-to-end encryption against a compromised server.
Revocation rotates future-write keys and does not reseal already readable data. See the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md) for the
remaining privacy and functionality boundaries.

### Verification and outstanding gates

The [calendar-sharing outcome](docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md)
records earlier automated qualification at its named source commits. Those historical
results are not executions against this beta's release commit. The published beta notes
must record fresh release-preparation checks and the exact-source Linux shipping build,
with their commands/results and artifact checksums before publication.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL backends,
multi-node execution and production deployment are not qualified. This preview does not
claim complete native-client or production readiness.
