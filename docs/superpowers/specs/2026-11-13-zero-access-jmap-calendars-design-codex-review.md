# Release 6 JMAP calendars design review

Reviewed: 2026-11-14. Spec: revision 1 of
[the design](2026-11-13-zero-access-jmap-calendars-design.md).
Branch: `za-jmap-calendars-design`.
Reviewed commit: `80aa51e1d22f8c584a5dda733582e3d7369e81de`.
Checkout: `/Users/jay/Development/stalwart/.worktrees/za-jmap-calendars-design`.

Three findings remain open. Revise these contracts before implementation. This is a static
design/source review; the scenarios below were traced, not executed. No builds, tests, servers,
or runtime probes ran. The spec and product source are unchanged.

## 1. [P1] Fence token-record publication against the vault revision

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

## 2. [P2] Do not revoke a healthy grant from an already-accepted refresh

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

## 3. [P2] Aggregate unexpanded search terms across event components

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

## Coverage and evidence limits

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
