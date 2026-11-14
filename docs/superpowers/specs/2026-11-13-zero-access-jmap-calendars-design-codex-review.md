# Release 6 JMAP calendars design review

Latest review: 2026-11-14, spec revision 3 at
`9d6a9f40011382216741cefb1dcb6265e1106f68`, branch `za-jmap-calendars-design`.

No open review findings remain. Findings 1–3 remain resolved in the design; revision 3
addresses finding 4 through an explicit bounded-retention contract and resolves findings 5–6.
No new actionable finding was identified in the revised contracts. This is a static design
assessment, not evidence that the feature is implemented or qualified.

## Revision 3 disposition

The following anchors refer to revision 3 of
[the design](2026-11-13-zero-access-jmap-calendars-design.md).

| Finding | Status | Current evidence |
|---|---|---|
| 4. Interrupted refresh retains a superseded wrap | Addressed by documented bounded retention and reconciliation | Lines 134–137 and 456–469 explicitly allow the wrap to remain for up to one maintenance interval. Lines 342–348 and 535–545 require ordinary writes and the sweep to remove refresh entries below the grant's stored generation. Proposed tests at 1002–1005 cover cleanup without a client retry; invariant 67 at 1139–1140 records the obligation. |
| 5. Paused-refresh test demands grant deletion | Resolved in design | Lines 997–1001 require refusal and an absent token record while allowing the unusable grant object to remain. Initial-issuance cleanup remains separately required at 994–996. |
| 6. Expanded negative-query test contradicts admission | Resolved in design | Lines 1027–1031 limit successful expanded matching to supported single conditions and flat AND filters and require `invalidArguments` for negated variants. |

The retention in finding 4 is an explicit residual in revision 3: after an interrupted refresh,
the superseded secret and the current stored record can still open MK until reconciliation.
The spec now requires cleanup independent of a client retry and states the maintenance bound;
it no longer promises immediate destruction for that interruption.

The token reviewer and primary checked the new pruning threshold against
`crates/common/src/auth/oauth/grant.rs:82–102,454–482`: generations below the stored generation
are replayed, while the current generation and its pending successor remain eligible. The
revised refresh test agrees with vault/cache behavior at
`crates/common/src/auth/vault.rs:527–536,784–790`. The primary also verified the expanded-filter
expectations against `crates/jmap/src/calendar_event/query.rs:102–116`.

The revision changes the spec and incorporates the previous report; product source under
`crates/` is unchanged. The earlier vault-fence, concurrent-refresh and event-level matching
corrections remain in the spec. Section 13's preflight and the planned automated/manual gates
remain future work. No builds, tests, servers, or runtime probes ran. This review changes only
this report; the spec is unchanged. The reviewer retained the inherited route and capability-tier
limitation documented below.

## Revision 2 review — historical record

Reviewed: 2026-11-14, spec revision 2 at
`fdcc7766d8d74e5fc429a4b8dd6884b32f47a4c6`, branch `za-jmap-calendars-design`.

At revision 2, all three revision-1 findings were resolved in the design. That rereview recorded
one further token-retirement issue (P2) and two acceptance-test contradictions (P3), all addressed
in revision 3 as described above. The original findings and their revision-2 anchors follow.

## Revision 2 disposition of prior findings

Line references in this section and findings 4–6 refer to revision 2 of
[the design](2026-11-13-zero-access-jmap-calendars-design.md).

| Prior finding | Status | Current evidence |
|---|---|---|
| 1. Vault-write race restores token wraps | Resolved in design | Sections 4.3–4.5, lines 326–334, 363–365 and 434–440, require a same-batch vault assertion and prohibit relabelling stale wraps. Tests at 960–967 exercise the race. |
| 2. Accepted older refresh revokes a healthy grant | Resolved in design | Lines 410–424 distinguish a pruned generation from missing keys; 436–440 reclassify after a lost CAS and forbid restoring the pruned entry. Tests at 968–974 cover both interleavings. |
| 3. Unexpanded text matching differs across components | Resolved in design | Lines 719–730 require event-level aggregation with index ordering and joining. Tests at 935–936 and 984–989, mutation at 1049–1050, and dependencies at 1169–1177 cover the original mismatch. |

## 4. [P2] Retire superseded refresh wraps after an interrupted publication

**Status:** addressed in revision 3 by documented bounded retention and reconciliation.
**Original spec anchors:** section 4.5, lines 441–447; section 4.7, lines 513–519.

The concurrency correction protects successful token-record writes, but grant advancement and
wrap retirement still occur in separate writes. The supported failure path does not guarantee
that a superseded secret loses its wrap:

1. The grant is pending at generation `g`; the token record holds `g` and `g+1`.
2. A request presents `g+1`. Upstream commits the grant's advancement to `g+1`.
3. The subsequent token-record write fails, or the process stops between these writes.
4. No client retries. The record still holds `g` and its MK wrap, although the grant now treats
   `g` as superseded.

The current-record wrap still opens with the old secret. The specified sweep does not remove
it: the grant exists and the entry is unexpired. It can remain until its refresh-token expiry,
contradicting the immediate-destruction claim at lines 445–447. This concerns at-rest wrap
retention, not online acceptance of a superseded token, and does not depend on an old database
backup. It is distinct from finding 2's successfully completed concurrent-refresh sequence.

**Current-source evidence:** `crates/common/src/auth/oauth/grant.rs:454–482` commits grant
advancement before returning to the caller. Its rotation classification at `:93–99` then treats
the older generation as replay. The source is unchanged in revision 2; section 4.5 expressly
preserves this ordering and permits the later token-record write to fail.

**Required correction:** Define retirement across this interruption. Either coordinate retirement
with advancement, or explicitly accept a bounded retention interval and specify generation-aware
reconciliation that works without a client retry. In the latter case, state the bound in the
threat model and superseded-token guarantee rather than promising immediate destruction.

**Validation criterion:** Fail immediately after successful successor rotation and before the
token-record write. Make no client retry. Inspect the current record and run maintenance;
verify that the old wrap disappears within the chosen contract's bound.

## 5. [P3] Align the paused-refresh test with the grant-cleanup rule

**Status:** resolved in revision 3.
**Original spec anchors:** section 9.2, lines 963–967; section 4.5, lines 434–435.

The first test requires a paused code exchange to leave no token record and no grant, then
says the same for a paused refresh. Initial issuance explicitly deletes its new grant on a
failed vault assertion (lines 388–392), but the refresh rule only stops with `invalid_grant`.
A refresh paused after opening MK can therefore fail its vault assertion and leave the existing
grant object behind. Its tokens are unusable, which differs from the test's absence assertion.

**Current-source evidence:** `crates/common/src/auth/vault.rs:527–536,652–660,784–790` commits
the vault and invalidates authentication/account caches; it does not delete OAuth grants.

**Required correction:** Either explicitly require grant cleanup on this refresh failure, or
change the refresh test to require an absent token record and unusable old tokens while allowing
the grant object to remain. Keep the initial-issuance cleanup assertion separate.

**Validation criterion:** Pause after the refresh has opened MK, change the vault, then resume
through the failed-assertion branch. Check grant presence against the selected rule, as well as
the token-record absence and authentication refusal.

## 6. [P3] Exclude negated filters from expanded matching assertions

**Status:** resolved in revision 3.
**Original spec anchors:** section 9.2, lines 987–991; section 6.4, lines 716–718.

The revised differential test includes negated queries, then requires the expanded form of
"each query" to match the selected occurrence's component. Expanded negation is expressly
unsupported by section 6.4. A literal implementation of both test requirements cannot pass.

**Current-source evidence:** `crates/jmap/src/calendar_event/query.rs:102–116` rejects expanded
`NOT` or `OR` filters with `invalidArguments`.

**Required correction and validation:** Limit expanded matching comparisons to supported single
conditions and flat AND filters. For expanded negated variants, assert `invalidArguments`.
Retain the full unexpanded positive/negative equivalence checks.

## Revision 2 evidence limits

The token and query reviewers rechecked their revised contracts against the current source;
the primary verified the findings and disposition evidence. Source files under `crates/` are
unchanged from the original review. The added placement paragraph at lines 602–606 explicitly
retains the old/new context assertions and destination checks already required by the design.
The review remains read-only apart from this report. No builds, tests, servers, or runtime
probes ran. Reviewers retained the inherited route and capability-tier limitation documented
in the original review.

## Revision 1 review — historical record

Reviewed: 2026-11-14. Spec: revision 1 of
[the design](2026-11-13-zero-access-jmap-calendars-design.md).
Branch: `za-jmap-calendars-design`.
Reviewed commit: `80aa51e1d22f8c584a5dda733582e3d7369e81de`.
Checkout: `/Users/jay/Development/stalwart/.worktrees/za-jmap-calendars-design`.

The original review recorded the three findings below; all are now resolved in revision 2 as
described above. Their original anchors refer to commit `80aa51e1d`, not current spec lines.
This was a static design/source review; the scenarios were traced, not executed. No builds,
tests, servers, or runtime probes ran. The spec and product source were unchanged by the review.

### 1. [P1] Fence token-record publication against the vault revision

**Spec anchors:** section 4.3, lines 320–322; section 4.4, lines 365–372; section 4.7,
lines 450–455; invariant 64, line 1008.

The token-record CAS protects only the previous token-record bytes. It does not prevent a
code exchange that obtained MK under vault revision `r` from publishing those wraps after a
vault write has committed `r+1` and deleted the token record.

A concrete first-issuance sequence is:

1. The code exchange validates revision `r`; the token record is absent.
2. It pauses after its last version check, before creating the grant and publishing its wraps.
3. A password change commits revision `r+1` and clears the absent token record.
4. The exchange resumes, creates its grant, and writes wraps made under `r`. Its assertion that
   the token record is absent succeeds.

This is an at-rest revocation failure, not an online authentication bypass: upstream can still
reject the old credential version, but the newly persisted wraps let the returned token secrets
open MK after the password change. The specified sweep removes expired entries and missing
grants, so this still-existing grant's unexpired wraps need not disappear on the next sweep.
Deleting the token record inside the vault batch alone cannot enforce the promised destruction.

**Current-source evidence:**

- `crates/http/src/auth/oauth/token.rs:514–538` checks the credential version before the awaited
  `create_oauth_grant` call; that check does not fence a later token-record write.
- `crates/common/src/auth/oauth/grant.rs:236–265` creates the grant in a separate registry write.
- `crates/common/src/auth/vault.rs:214–238` shows the existing pattern of asserting the exact
  vault bytes and companion-record state in the same batch before writing either.

**Required correction:** Every publication of token wraps must assert the exact vault state
under which MK was obtained in the same batch as the token-record CAS. Apply that rule to
initial issuance and refresh, including retry paths. A failed vault fence must abandon the stale
operation rather than relabel its key material with a newer revision. Preserve the specified
cleanup of a grant created by a failed issuance.

**Validation criterion:** Pause first issuance after its final version check, commit a password
change or recovery, then resume. Issuance must fail and the token record must remain absent.
Repeat with a refresh and a lost-CAS retry crossing the same vault change.

### 2. [P2] Do not revoke a healthy grant from an already-accepted refresh

**Spec anchors:** section 4.5, lines 386–405, especially the missing-entry deletion at
lines 390–395 and pruning/retry at lines 403–405.

The grant rotation and token-record update are separate operations. Deleting the grant whenever
the presented generation's entry is missing confuses concurrent progress with missing key data.

The prescribed sequence permits this interleaving:

1. The grant is pending at generation `g`; the token record contains `g` and `g+1`.
2. Request A presents `g`. Upstream accepts it as a retry that issues `g+1`. A pauses before
   reading the token record.
3. Request B presents the already-issued `g+1`. It advances the grant and publishes entries
   `g+1` and `g+2`, pruning `g`.
4. A resumes, finds no entry for `g`, and step 2 deletes the healthy grant. B's fresh tokens are
   revoked even though A was accepted before the advancement.

There is a second unresolved branch if A pauses after opening MK but before its CAS. On losing
that CAS, the instruction to reread and repeat from step 3 does not say how to handle the now
pruned presented entry. Recreating it from A's retained MK would violate the guarantee that a
superseded refresh secret no longer opens anything in the current record.

**Current-source evidence:**

- `crates/common/src/auth/oauth/grant.rs:82–102` defines the accepted retry and successor cases.
- `crates/common/src/auth/oauth/grant.rs:427–435` returns the retry issuance without a registry
  mutation or a lock held across the later token-record work.
- `crates/common/src/auth/oauth/grant.rs:454–482` allows the successor to advance the grant in
  its own conditional registry write.

**Required correction:** Define how refresh publication coordinates the grant state and token
record, including advancement between acceptance and publication. Distinguish an actually lost
key entry from a generation pruned by a newer successful request. Stale accepted work must
neither revoke the newer grant nor restore an older generation. Record-byte CAS alone is not
sufficient without rules for interpreting the new state.

**Validation criterion:** Use barriers to run the sequence above, plus the variant where A
already opened MK before B commits. B's grant and tokens must remain usable, and generation `g`
must remain absent. Separately retain the intentional revocation case for a genuinely missing
current entry and upstream's replay rule.

### 3. [P2] Aggregate unexpanded search terms across event components

**Spec anchor:** section 6.4, lines 671–676; the parity assertion in section 9.2,
lines 908–912.

D12 says each text condition matches an event when any one component matches it, then claims
this reproduces the ordinary account's index. A condition containing multiple words disproves
that equivalence. Given a recurring event with `SUMMARY:alpha` on its master and `SUMMARY:beta`
on an override, the ordinary account's unexpanded `{"title":"alpha beta"}` query matches the
event. The proposed component-local matcher rejects it because neither component contains
both words. Negating the condition reverses which account returns the event.

**Current-source evidence:**

- `crates/groupware/src/calendar/index.rs:749–806` indexes all scheduling components into one
  document, reusing the same field for each component's summary.
- `crates/store/src/search/document.rs:76–85` concatenates successive text values for that field.
- `crates/store/src/search/leaf.rs:61–85` plans the terms of a standard text condition together.
- `crates/groupware/src/calendar/instance_filter.rs:171–179,220–237` matches one component and
  requires all words of the condition to match within that component.
- `crates/jmap/src/calendar_event/query.rs:265–274` submits a title condition as one index filter.

**Required correction:** Define an event-level matcher for unexpanded queries that aggregates
the same field values, in the same order and with the same token semantics, as the index.
Keep component-local matching for expanded queries. If different unexpanded behavior is
intentional, explicitly accept it and revise the equivalence promise and differential oracle.

**Validation criterion:** Put identical events with disjoint words in their master and override
titles into ordinary and key accounts. Compare unexpanded multiword queries, quoted phrases,
and their negations after the ordinary index is current. Expanded queries must continue to
match the selected occurrence's component.

### Original coverage and evidence limits

Three read-only reviewers covered OAuth lifecycle, calendar mutation/scheduling/persistence,
and read/query/change/session contracts. The primary checked the reported findings against
the pinned source. The runtime did not establish the originating model's capability tier;
reviewers deliberately inherited the parent route rather than claiming a separately selected
top-tier review.

The mutation review found no additional confirmed defect. The plan still needs to implement
the spec's stated obligations to fence both old and new placement contexts and check every
destination under the account-and-UID lease. Existing helpers cannot simply be reused unchanged.
The earlier specs' accepted collection-delete partial-commit behavior was not reopened as a
new finding.

The reference client's tagged source was also spot-checked at Bulwark `1.21.2`, including its
[static token flow](https://github.com/bulwarkmail/webmail/blob/1.21.2/lib/auth/lite-tokens.ts),
[login request](https://github.com/bulwarkmail/webmail/blob/1.21.2/lib/auth/stalwart-login-request.ts),
and [JMAP client](https://github.com/bulwarkmail/webmail/blob/1.21.2/lib/jmap/client.ts).
This does not establish browser compatibility or satisfy section 9.8's manual gate. The
section 13 preflight items remain implementation-plan prerequisites.
