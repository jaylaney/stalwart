# Plan 14 review: zero-access calendar sharing

Reviewed 2026-10-23 in `release3-mail-spec`, on branch `release4-sharing-spec-r1`.

- Plan: `2026-10-23-zero-access-14-sharing.md` at commit `840baab1` (888 lines).
- Spec: calendar sharing design, revision 11, commit `0c47b7f0`; its review (`c3e30e22`)
  found no actionable finding.
- Source: the plan's snapshot `c3e30e22` and HEAD `840baab1` hold the same code tree
  (`git diff --stat c3e30e22 840baab1 -- crates tests Cargo.toml` is empty). Every
  `file:line` below is at that tree.
- Method: two independent read-only reviewers (one for spec fidelity, coverage and the
  concurrency kernels; one for executability against the code, the commands and the test
  harness), each reporting every finding with severity and confidence; the session's
  controller checked the disagreements against the source and adjudicated. No build, test,
  server run or edit.
- Out of scope: the plan's model and agent-routing choices (it is written for Codex), its
  last paragraph, and the `REQUIRED SUB-SKILL` header.
- Result: no P1; four P2; eighteen P3; eleven notes. Two of the P2s are spec-level and need
  Jay's decision (D1, D2). No task needs restructuring: the task order, the interfaces, the
  commands, every path and symbol the plan reuses, all 25 integration cases and all 48
  kills of §11.6 check out. The plan is executable after the P2 edits.

Severity: P1, wrong behaviour or not executable; P2, fix before execution starts; P3, fix
before the owning task starts; Note, worth a sentence. Confidence is the reviewers' plus the
adjudicator's check.

## Decisions for Jay

**D1. MOVE-overwrite scheduling: the spec's "as upstream" premise is false.** Spec 5.6
(line 1609-1610) says a MOVE that overwrites a destination event deletes it "and that
deletion schedules as a DELETE does"; 6.2 (line 1680) lists "a MOVE that overwrites an
event" among the delegated writes that send; the plan's Review Focus 4 (87-89, "cancel the
destination once") and Task 5 step 3 (453-454, "carry its scheduling messages to Task 9's
common fan-out path") implement that. Upstream does not schedule it: the copy overwrite
(`crates/dav/src/calendar/copy_move.rs:569-578`), the move source unlink (`:703-712`), the
move overwrite (`:747-756`) and the container delete (`:1022-1023`) all call
`DestroyArchive::delete` with `send_itip` hard-coded `false` (`storage.rs:470-482`;
compare `delete.rs:82-85`, which computes it), and the overwrite's `account_info` is the
writer's own, not the merged list (`copy_move.rs:564-566`, `:655-657`). So the plan, as
written, makes a key account's MOVE-overwrite send CANCELs where a plain account's sends
nothing: new product behaviour nobody decided, and a key/plain divergence the fork's
invariants avoid. Recommended: follow upstream. Correct spec 5.6 and 6.2 in a revision 12
(two sentences), drop "cancel the destination once" from Review Focus 4, make Task 5 step 3
say the overwrite deletion keeps `send_itip=false` and stays out of the fan-out, and remove
`copy_move.rs` from Task 9's fan-out sites (it has none today: the five are
`update.rs:359`, `update.rs:571`, `delete.rs:258`, `drain.rs:1215`, `contacts.rs:700`). The
alternative, a deliberate fork addition for key accounts, would need its own case and the
merged `scheduling_account_info` on a path that does not build it today. The upstream gap
itself (an overwritten scheduled event's attendees get no CANCEL) belongs in the private
tracker, not in the spec. (Lens A finding 1, lens B finding 3; both high confidence.)

**D2. An unlisted sub-address of the writer breaks parity.** Spec 6.2 defines the writer's
addresses as the merged list minus the owner's, and locality is exact membership
(`crates/groupware/src/scheduling/mod.rs:244-250`), but delivery resolves recipients by
account with the `+tag` stripped (`fanout.rs:1082-1089`; `principals.rs:755`). An attendee
`bob+x@` that is not in Bob's address list: when Alice writes, her fan-out resolves it to
Bob's account and delivers; when Bob writes through the grant, the first run addresses it
(non-local under the merged list), `resolve_inner` drops it as the sender's own inbox, and
the second run does not keep it (not a writer address by exact match). Bob's copy gets
nothing: a divergence between the two runs, which the spec's own stopping rule calls a
defect. Case 25's alias fixture uses listed aliases only, so the differential test would not
see it. Recommended: in revision 12 define the writer's addresses for the second run's
override and for the kept messages as the addresses that resolve to the writer's account
(listed aliases and sub-addresses alike), and state that the fan-out's self-inbox carve-out
admits an item by its provenance (a retained second-run message), never by address
resolution alone; then add a sub-address step to case 25's alias fixture. The plan's Task 9
step 4 (659-662) already keys the carve-out by provenance; its "address exclusive to the
writer" wording needs the same change. (Lens A finding 8; medium confidence on frequency,
high on the mechanism.)

## P2 findings

**1. [P2] MOVE-overwrite scheduling.** See D1. Plan 87-89, 453-454, 623-625.

**2. [P2] The commit pause does not exist where Task 4 holds it.** Plan 395 ("Hold
`COMMIT_PAUSE` before PUT and DELETE commit") and case 22 (727-731). `COMMIT_PAUSE` fires at
one point today, `crates/dav/src/calendar/update.rs:631`, inside the key account's create
path (`za_commit_create`); the update PUT commits at `update.rs:405` and DELETE at
`delete.rs:276` with no pause point, and the hook's only users are creates
(`tests/src/za/uid_lease.rs:189`, `:231`). A test holding it on an update or a DELETE fails
after five seconds with "the request did not reach the pause point"
(`tests/src/utils/za.rs:849-851`), which the plan's red-phase rule (220-221) tolerates, but
the plan never says the two pause points must be added. Case 22 also holds "alice's and
bob's PUTs" at once; a one-shot pause keyed by one account id (both PUTs target Alice's
account) cannot hold two requests. Fix: in Task 4 step 3 name the additions, either
`COMMIT_PAUSE` extended to `update.rs:405` and `delete.rs:276` (then an existing one-shot
pause is consumed by whichever of the three sites the account reaches first, and the hook's
doc comment at `test_hooks.rs:20-22` changes) or `UPDATE_COMMIT_PAUSE` and
`DELETE_COMMIT_PAUSE` with the `Pause` shape (`crates/store/src/za_test.rs:25-39`;
`test_hooks.rs:169-171`, `:321-347`); and say whether case 22 means two sequential holds or
a per-request variant. (Lens B finding 1, lens A note 16.)

**3. [P2] `itip_finalize` is not callable from the DAV crate.** Plan 646-650 puts "finalize
once with second update increments or `[]` for create" in Task 9's DAV integration.
`itip_finalize` is `pub(crate)` in groupware (`crates/groupware/src/scheduling/itip.rs:354`),
and Task 8's interface (589-595, `ZaSelfMessages { messages, increments, finalize }`)
exposes no finalize entry. Fix: add to Task 8's interfaces a public
`za_finalize(tree: &mut ICalendar, increments: &[u16])` (or
`ZaSelfMessages::finalize_into(&self, tree)`) in `delegation.rs`, and name it in Task 9
step 3. (Lens B finding 2, high.)

**4. [P2] The sub-address parity gap.** See D2. Plan 659-662; spec 6.2 lines 1829-1842.

## P3 findings

**5. [P3] Task 9's placement sentence misleads.** Plan 647-648 ("take the second snapshot
before upstream's first run finalizes"). The first run finalizes inside `itip_update` and
`itip_create` on `Ok` (`event_update.rs:30-32`, `event_create.rs:23-25`), both taking
`&mut new_event.data.event` (`update.rs:259-266`, `:495`); there is no point after the first
run and before its finalize. Task 8 step 3's "build owner-only snapshots before mutation" is
right. Fix: "run the second run first, on the pre-finalize trees, before the
`itip_update`/`itip_create` call at `update.rs:259` and `:495`; the first run then mutates
the tree". (Lens B finding 4.)

**6. [P3] `access_to` is not readable outside `common`.** Plan 95-96, 170-172, 704-705.
`access_to` is a `pub(crate)` field of `AccessTokenInner` (`crates/common/src/auth/mod.rs:178-182`).
The public accessors are `AccessToken::shared_accounts(collection)`
(`access_token.rs:766`), `has_access(account, collection)` (`:779`) and `is_shared` (`:757`).
Fix: name `shared_accounts(Collection::Calendar)` and `has_access(owner, Collection::Calendar)`,
or add an accessor in `common` and list `access_token.rs` in Task 3's files. (Lens B
finding 5.)

**7. [P3] Task 7's loss classification is unspecified.** Plan 560 ("A vault/account loss is
terminal; other losses redo all decisions up to eight attempts"). The store reports one
`AssertValueFailed` without naming the key (`crates/store/src/backend/rocksdb/write.rs:295-312`;
spec 4.5 lines 1010-1015), so the loop cannot tell a vault, Account, pin or calendar loss
apart from the error. An implementer may redo every loss and answer `conflict` on the eighth
where case 14's race expects `account deleted`. Fix: on a loss, re-read the vault record
(revision against `verified.keys.generation`: terminal, `vault changed since verification`),
then the registry Account item (absent: terminal, `account deleted`), then re-decide; the
attempt counts either way. (Lens A finding 2.)

**8. [P3] A second loss of the rebuilt commit has no stated branch.** Plan 411-417 says
"rebuild once" and stops. Spec 5.5 line 1509 ("a second loss is upstream's answer below")
and 1511-1514 give it: 409 for an update or DELETE, `za_lost_create_fence`
(`crates/dav/src/calendar/mod.rs:230-251`) for a create, a transient item error in the
drain, with no further calendar read. Fix: add that sentence to step 4. (Lens A finding 3.)

**9. [P3] The expected post-commit version cannot be built from the prepared archive.**
Plan 431-433 and 456-459. The hash in `ArchiveVersion::Versioned` is computed over the rkyv
bytes inside `Archiver::serialize` (`crates/store/src/write/serialize.rs:149-210`) within
`ObjectIndexBuilder` (`crates/common/src/storage/index.rs:335-351`); nothing returns those
bytes, and `AssertValue::Archive` compares the hash only (`assert.rs:71-77`). The plan's
fallback keeps the mechanism correct (spec 1377-1380), but the instruction misleads. Fix:
state the identity test meant: read the archive back after the commit and accept it as the
archive that commit left when `version.change_id() == Some(assigned_change_id)`
(`crates/store/src/write/mod.rs:492-497`); assert that read-back version in step 2;
otherwise take the fallback. (Lens A finding 4.)

**10. [P3] The owner resolver's boundary repair has no loss or placement rule.** Plan
436-437, 460; spec 5.2 lines 1389-1391 and 4.2 line 803 (`shares` "writes nothing of its
own"). The repair asserts the archive it read; a grant mutation or another session's repair
can win. Fix: a lost repair is ignored (the read proceeds on the pending state, which is
safe) and never fails the request; the repair runs only with the owner's EWK; say whether
the `shares` listing's resolver runs it (recommended: no, per 4.2). (Lens A finding 5.)

**11. [P3] A conversion chunk that loses its calendar assertion has no rule; attempt
accounting over chunks is unclear.** Plan 497-499 asserts the calendar in every chunk,
stricter than spec 3.5 lines 634-638 and necessary (a second request's rewrap chunk could
otherwise land between a revert's steps 2 and 3), but only the per-event loss has a rule.
Plan 51 caps "share mutations" at 8 attempts; a 3,000-event conversion has 30 chunks. Fix:
on a lost chunk, re-read the ring: `reverting` set, join the rollback; otherwise continue
under the current ring; chunk losses re-read and continue without counting toward the
eight, which count decision-level losses only. (Lens A finding 6.)

**12. [P3] The trace detail strings are not stated.** Spec 10 lines 2413-2418 and 5.5 line
1478-1479 fix `zero-access: calendar access without a collection key`,
`zero-access: copy into or out of a shared calendar` and
`zero-access: event linked into a shared calendar`; the plan names only the drain's
`zero-access: delegated sender not verified` (670). Fix: list the four in Global
Constraints or in Tasks 3, 4, 5 and 9. (Lens A finding 7.)

**13. [P3] "All remaining `za_fan_out` call sites found by `rg`" (625).** There are five,
in four files, listed under D1; `copy_move.rs` has none. Fix: enumerate them in Task 9's
Files. (Lens A finding 9, lens B.)

**14. [P3] A fresh calendar read per item in listings.** Plan 367 ("Read the stored calendar
on every use") applied to the loader's per-item view (`za_archive_view`,
`crates/dav/src/common/za.rs:259-283`, called from `propfind.rs:481`) makes a depth-1
PROPFIND or multiget of a 3,000-event calendar read the calendar archive 3,000 times. Spec
5.5 line 1518 budgets one extra read per write; 4.6 lines 1070-1071 accept in-flight reads
on a revoke. Fix: allow a per-request memo of `ZaCalendarContext` keyed by (owner,
calendar) for reads, inside the request's `PropFindData`; keep the fresh read per write.
(Lens A finding 10.)

**15. [P3] No task owns the trace-content assertions the coverage table claims.** Row §2
(852) cites "trace assertions"; spec 4.7 lines 1085-1091 and 10 lines 2407-2412. No step in
Tasks 7, 9 or 11 adds a captured-trace test (the suite's `mail::traces` pattern). Fix: add
to Task 7 (grant, revoke, conversion) and Task 9 (the drain's delegated refusal) an
assertion that captured events carry no address, fingerprint, key bytes or calendar name.
Row §5.7's "existing refusal tests" is likewise named by no step; one line in Task 10 or 12
would do. (Lens A finding 11 and coverage check.)

**16. [P3] Fixture account names.** Spec 11.2 line 2477 and `CLAUDE.md` require names unique
across the suite's modules (next free `keymail27`, `plainmail23`; `pinning.rs:58-60` uses
`key1..3@example.com`, `leak.rs` uses `key1`/`key2`). Several Codex agents writing
`sharing.rs` and `sharing_races.rs` without a scheme will collide. Fix: give a prefix per
module (for example `share-alice@`, `share-bob@`, `share-carol@` and `race-*`) in Task 3
step 1, and update `CLAUDE.md`'s next-free note in Task 12. (Lens A finding 12, lens B
finding 9.)

**17. [P3] COPY refusal wording.** Plan 451 ("Refuse COPY involving any shared parent")
reads as source parents only; spec 5.6 lines 1584-1590 refuse an event COPY whose source or
destination calendar is shared, and a collection COPY or MOVE whose source or destination
is shared. Fix: "any shared calendar among the source's parents, the destination, or the
event's other parents". (Lens A finding 13.)

**18. [P3] The DELETE rebuild on retry would rebuild the CANCEL.** Plan 414-415. Rebuilding
the DELETE batch re-runs `delete_za`/`delete_all_za`, which rebuild the CANCEL
(`storage.rs:581-611`); the rebuild must pass `send_itip=false` or discard the returned
`ZaDeleteMessage`, or "never repeat fan-out" fails. Also `ZaDeleteMessage.message` is an
`Option<ItipMessage>` (`storage.rs:599-605`): "appends the writer-only CANCEL" (647; spec
1885-1886) needs a `Vec` at the loop. (Lens A note 17, raised.)

**19. [P3] The scanner's key-account check is missing.** Task 11 step 3 (759-762) omits
spec 11.4 lines 2845-2847: every account in `acls` is a current key account or a gone
grantee awaiting its sweep. Fix: add the check; the suite leaves no gone grantee at scan
time (case 14). (Lens A finding 20.)

**20. [P3] The SENT-BY helpers are misnamed.** Plan 666-667 ("normalize using pinning's
helper"). The normalizer is `common::auth::vault::za_normalize_address`
(`crates/common/src/auth/vault.rs:243`, imported by `pinning.rs:32`) and does no `mailto:`
decoding; `groupware::decode_mailto_address` (`crates/groupware/src/lib.rs:211`) or
`Email::new` (`scheduling/mod.rs:303-309`) strips the scheme. The snapshot's `Organizer`
has no `sent_by` field (`scheduling/mod.rs:107-113`; only `Attendee` parses it,
`snapshot.rs:202-205`), so an ORGANIZER's SENT-BY is read from the tree's entry
parameters. Fix: name both helpers and the tree read. (Lens B finding 6.)

**21. [P3] Hook homes.** Plan 154, 461, 505-506, 574 ("through existing-feature
forwarders"). Groupware and dav reach `common::za::test_hooks` directly under
`cfg(feature = "test_mode")` (pattern `fanout.rs:359`, `drain.rs:800`, `update.rs:631`);
the `http` crate's `test_mode` does not enable common's, so HTTP code uses the non-`cfg`
forwarders in `crates/common/src/za/mod.rs:48-133` or its own
`crate::auth::authenticate::za_test` (`ENDPOINT_PAUSE` and siblings, `authenticate.rs:256-282`).
Fix: say that `SHARE_BOUNDARY_PAUSE`, `SHARE_PRECHECK_PAUSE`, `SHARE_REWRAP_PAUSE` and
`SHARE_REWRAP_FAIL_ONCE` are statics in `test_hooks.rs` called directly from
`convert.rs`/`boundary.rs`, and whether `SHARE_COMMIT_PAUSE` sits in `mutations.rs`
(direct) or in `api/shares.rs` (forwarder). (Lens B finding 8.)

**22. [P3] Shared calendars left at scan time fail the suite before Task 11.** Plan 208,
762-763. `leak::test` walks every account and `check_calendar` (`tests/src/za/leak.rs:546`)
enforces release 1's one-entry rule until Task 11 extends it, so any shared calendar
alive when the leak scan runs fails the whole suite during Tasks 3-10, whose gate is that
suite. Fix: say in Tasks 3-10 that every shared calendar is revoked and deleted before the
scan, and in Task 11 that the healthy fixtures kept until the scan are created only after
the scanner's new rules land. (Lens B finding 10.)

## Notes

- **N1. Unit-test location.** Spec 11.1 lines 2442-2445 place the grant-wrap round trip and
  the `pk-dek` disjointness test in `crates/vault`; plan 270-274 places
  `grant_and_pk_dek_purposes_are_disjoint` beside the groupware codecs. Harmless; say which.
- **N2. `not a key account` accounting.** Spec 4.3 line 920 says that 409 is not counted as
  a failed authentication (as `contacts/lookup`, `contacts.rs:218`); plan 554-555 says only
  "auth accounting". State which 409s go through `za_refuse`.
- **N3. The lease guard on the retry's non-fence exits.** `ZaUidLeaseGuard` releases on drop
  (`crates/common/src/za/lease.rs:290-300`); `abandon` is for a lost fence only. The
  retry's 403 and 409 exits must drop or `release()` the guard, never `abandon()` it, or the
  UID is blocked for `ZA_UID_LEASE_SECS`.
- **N4. `preferences_mut` auto-inserts.** `Calendar::preferences_mut` inserts a copy of
  entry 0 for a principal without an entry (`crates/groupware/src/calendar/mod.rs:236-251`);
  upstream's calendar PROPPATCH calls it with `personal_id` (`proppatch.rs:308`, `:324`,
  `:356`). The resolver refuses a principal without an `acls` entry first, and Task 2's
  `seal_calendar_changes` must refuse a changed entry set; name the invariant (no write
  creates a preferences entry; only the grant does, spec 3.3 line 555).
- **N5. `assert_in` resets the batch context.** After
  `with_account_id(owner).with_collection(Calendar).with_document(calendar_id).assert_value(..)`
  the caller restores the event's account, collection and document before the event's
  operations, as `delete_all_za` does (`storage.rs:567-570`); Task 4 step 5 should check
  the reset as well as the chunk.
- **N6. The DELETE loop reaches a grantee only after Task 10.** `delete.rs:88` obtains the
  keys through `za_session_keys`, which answers 403 for a grantee until Task 10 swaps the
  gate, and `delete_all_za` returns a message only with keys (`storage.rs:581-609`). Task
  9's "run delegated DAV versions in Task 10" (645) covers it; step 3 should say the loop
  is reached by the owner alone until then.
- **N7. Task 9's PUT and DELETE integration has no executable test until Task 10.** The
  plan accepts this (242-244, 645). An alternative is to open the gate at the end of Task 9
  or to move the delegated-write HTTP cases into Task 9's commit; a planning choice.
- **N8. The ownership table under-reports.** Row `crates/dav/src/calendar/{get,freebusy,update,proppatch,delete,mod}.rs`
  says "3, 4"; Task 9 touches `update.rs` and `delete.rs`, Task 10 `proppatch.rs`. Task 3's
  Files line omits `tests/src/za/mod.rs`, where `pub mod sharing;` and the call are added.
- **N9. `EnumImpl::to_id` needs its trait import.** Plan 557-558's registry presence
  assertion is right in shape (`RegistryClass::Item { object_id: u16, item_id: u64 }`,
  `crates/store/src/write/mod.rs:266-270`; `batch.assert_value(class, AssertValue::Some)`),
  but `to_id` is a method of the trait `registry::types::EnumImpl`
  (`crates/registry/src/types/mod.rs:28-34`), and no fork code asserts a registry item yet.
- **N10. Undecided wording an implementer must guess at.** Task 1 Files "narrow seal
  helper exports" (255); Task 3 step 3 "add owner pending-boundary repair when Task 5
  supplies it" (372, a forward dependency with no step in Task 5 naming the call site);
  Task 6 Files "modify shared format/resolve helpers narrowly" (468-469); Task 7 Files
  "shared helpers, hooks and integration modules" (515); Task 10 Files "discovery plumbing
  as needed" (681); Task 12 Files "relevant existing product documentation" (802).
- **N11. Review Focus.** The five items are sound; the five likelier to bite, in order: D1,
  finding 7 with finding 8, D2 with the carve-out's provenance keying, N4, finding 14. A
  grantee's sync after the owner's destruction fails at `validate_uri`'s
  `account_id_from_email` (404) and case 14 covers discovery; one assertion there suffices.
  The writer's group address shared with the owner (inherited row 4) has a unit test only
  (600); one assertion in case 25's fixtures would close it.

## Verified as consistent

- **Spec coverage.** Every rule of spec 1.2, 1.3, 1.5, 2.2, 3.1-3.7, 4.1-4.7, 5.1-5.7,
  6.1-6.4, 7.1-7.5, 8.1-8.8, 9, 11.1-11.5 and the invariants 33-40 has an owning task with
  the spec's values: the seven purposes and the AAD table literally (41-43); the ring layouts
  and the grantee's snapshot entry with flags cleared; `0x03` at 82 bytes (policy, type,
  calendar id, generation, 72-byte wrap: `crates/vault/src/keys.rs:17-20`; existing
  envelopes `seal/tree.rs:164-199`); the three conversion phases, the walk before any rewrap,
  batches of 100, the three-step revert and resume; the cache identity and request-scoped
  endpoint keys; the one share batch with its four assertions and eight attempts; the grant
  ring born pending with a zero id and completed from the commit; explicit invalidation of
  every written and every remaining grantee; the gate's four conditions; the own-id
  resolver, eligibility, vanished filter, token bound, two-step rename and initial-versus-
  incremental sync; every writer fenced and the linked-event write rule; the second run of
  upstream's builders with exactly the two routing fields, the kept messages in upstream's
  order, "either run built" at the fan-out and the schedule tag, the finalize on
  `NothingToSend`, the cancel fallback unfinalized, the DELETE loop, the token union over
  the owner's addresses, the status on the writer's line; the inherited table left alone;
  rotation with ledger keys and every generation to a new grantee. All 25 integration cases
  have owners (1-2, 11-12, 15, 25 in Task 10; 3, 21, 23-24 in 6; 4-8, 13-14, 16-20 in 7;
  9-10 in 9; 22 in 4) and all 48 kills of §11.6 appear in the ledger with matching case
  numbers, the attendee-side equivalence rule included. The eleven "Decisions made for
  implementation" are each consistent with the spec.
- **Executability.** Every "modify" path exists and every "create" path has a real parent
  and no collision. Every reused symbol exists with a compatible signature: the wrap and
  padding primitives, `decode_archive`, `za_private_key`, `ZaPinRead::assertion`,
  `za_reread_verified` (private today, `vault.rs:605`), `AssertValue::Archive` and
  `ArchiveVersion` (hash-only comparison confirmed, `assert.rs:71-77`),
  `AssignedIds::last_change_id`, `commit_batch`, `Calendar::update` and
  `CalendarEvent::update` (the index, quota, ACL-index and share-notification writers),
  `delete_za`/`delete_all_za`, `za_lost_create_fence`, `za_fan_out` (`sender` first, `keys`
  second: "after `sender`" is unambiguous), `za_keys_for`, `za_session_keys`,
  `za_archive_view`, `AclGrant { account_id, grants }` and `Calendar.acls`,
  `vault::session::live::session_keys`, `HookParked`, `za_post_concurrent`, `za_unlock`,
  `plant_event`, `trigger_drain`, `copy_href`, `DrainOn`, `Secret`, `SessionKeys` (held as
  `Arc`, already carrying lazy per-session state, so a mutex-guarded CK cache fits),
  `AccountInfo`, `scheduling_account_info`, the scheduling types and the two inner builders
  (`pub(crate)`, reachable from `calendar::delegation`), `itip_cancel`'s split as a
  three-line extraction with untouched callers. The `/api/vault/*` dispatch
  (`api/mod.rs:281-290`, `vault.rs:283-320`) takes the three `shares` arms after the rate
  limit; `CredentialsRequest` is reachable. The attendee hash-set remark, the schedule-tag
  sites, the DELETE loop site, the disabled deletion reconciliation, the 32-item cap and 30 s
  deferral, the carve-out and token-union sites, the drain's sender candidates
  (`drain.rs:722-737`) and its fixed-string rejection path (`:1390-1417`, no generated enum)
  all hold.
- **Commands.** `cargo test -p tests za::za_tests` selects exactly `tests/src/za/mod.rs:49`
  (CI uses it verbatim); `cargo test -p vault -p groupware -p common` requests no feature,
  so the `test_mode`-without-`enterprise` failure does not arise and groupware's unit modules
  build; `-p http@0.16.26 --features test_mode`, `cargo check -p stalwart
  --no-default-features --features rocks` and `cargo fmt --all --check` (CI's style
  command) run as written; `webdav::webdav_tests` with `ZA_KEY_ACCOUNTS=1` likewise.
- **Harness.** The registration slot "after pinning and before leak scans" is real
  (`tests/src/za/mod.rs:154-170`); `DrainOn` is usable from a new module; a new pause is a
  static plus a setter plus a pause point; `assert_is_empty` needs no new exemption (rings,
  ledgers and bundles ride inside the Calendar archive and the event's `X-ZA-KEY`), provided
  the modules delete their calendars and revoke their grants before account destruction;
  the leak scanner visits an account's calendars before its events in the same pass, so
  Task 11's header map fits.
- **Kernels, tried and not broken.** The Task 4 retry: a revoke between attempts (403, the
  recipients' items stand, which spec 4.6 and 6.2 accept), a generation change (DEK rewrapped
  in memory, statuses and pairs kept, no double send), a `reverting` flip (rewrap to the
  account key), calendar and event both changed (409), no lock beyond the lease. The
  boundary: two interleaved renames in either order, a crashed step 2 repaired against a
  racing grant; the recorded id is always at or above every commit whose flag it clears. The
  conversion: no grant before the final scan; an interrupted rollback keeps the ring until
  the last event is back under the account key; a concurrent owner PUT either loses and
  re-seals or is skipped by the per-event assertion; a second conversion resumes a pending
  rollback first. The share loop: invalidation after the commit regardless of step 2; a node
  that missed the broadcast is caught by the use-time checks (the accepted limit). The cache:
  the fresh read and the own-id check precede any hit, and a reconversion's new domain id
  misses by key. The identity split: a group address shared with the owner gets no self
  item (row 4); the carve-out admits only second-run items once keyed by provenance (D2).

## Disposition after Jay's decisions, 2026-10-23

The report above is preserved as the review of `840baab1`. Jay approved following upstream
for D1 and the account-resolution/provenance fix for D2. A source check against `5fa7ebd4`
confirms the four P2s. Spec revision 12 and the accompanying plan now incorporate the
corrections below. This pass changed documentation only; no implementation, build or test ran.

| Review items | Disposition in the revised documents |
|---|---|
| 1 / D1 | Spec 5.6 and 6.2 preserve `send_itip=false` for MOVE overwrite/source unlink. Task 5 checks no scheduling items; Task 9 lists only the five actual fan-out calls. |
| 2 | Task 4 adds separate update/delete pause hooks; spec cases 21–22 name them. Alice's and Bob's revoke races use independent runs, so one-shot slots suffice. |
| 3, 5 | Task 8 exposes a narrow `za_finalize` wrapper. Task 9 runs the second builder before the first public builder call and names the finalize entry it uses. |
| 4 / D2 | Spec 6.2 and Tasks 8–10 classify writer recipients by quiet account resolution, retain second-run provenance, remove those recipients from first-run output and add sub-address fixtures. The three corresponding mutation checks join the ledger. |
| 6 | Task 3 adds `has_shared_access`, inspecting the exact collection entry without membership; candidate lists use `shared_accounts`. See the accessor refinement below. |
| 7, 8, 11 | Tasks 4, 6 and 7 specify loss classification, second-loss exits and decision-level versus chunk-level attempt accounting. Missing Account is terminal even if its vault is also gone. |
| 9, 10 | Task 5 identifies the committed archive by read-back change id, asserts that version, and uses the post-read fallback otherwise. Repair is owner-only, ignores an assertion loss and is not called by `shares` listing; other errors keep their usual handling. |
| 12, 13, 15, 20 | Fixed trace strings, five fan-out sites, captured-trace tests, the JMAP refusal gate and exact SENT-BY parsing helpers/tree fields are named. |
| 14, 17, 18, 19 | Request-local read memo; explicit COPY destination checks; DELETE retries without rebuilding/sending CANCELs; Option-to-vector conversion; scanner key-account checks. |
| 16, 21, 22 | Separate fixture prefixes, precise hook modules/call sites, and revoke-plus-delete cleanup before the old scanner runs. Healthy shared fixtures survive to scanning only after Task 11 lands. |
| N1–N5 | Primitive wrap tests placed in vault; post-verification conflicts avoid failed-auth accounting; lease cleanup on non-fence exits; preference-entry set invariant; batch context restoration. |
| N6–N7 | Deliberately retain Task 10 activation: Task 9 uses direct fixtures and owner HTTP paths, then Task 10 runs every prepared delegated HTTP case. No task reordering. |
| N8–N11 | Ownership/registration paths, `EnumImpl` import, concrete file lists and repair wiring, review focus, destroyed-owner URI 404 and shared-group fixture are explicit. |

Two source-backed refinements qualify the recommendations:

- `has_access` is public but also succeeds through `is_member`
  (`crates/common/src/auth/access_token.rs:779-787`). Substituting it alone for the private
  field would weaken the spec's exact-entry gate. The new narrow accessor avoids that;
  fresh calendar ACL and own-id checks remain authoritative.
- D2 proves a transport divergence. Import is conditional: `drain.rs:584` snapshots against
  the recipient's listed addresses, and `snapshot.rs:217,353-361` requires a local scheduled
  line. The main sub-address fixture therefore retains a listed attendee and compares item
  counts, token-address presence and copies. A sub-address-only fixture checks equal delivery
  and equal receiver rejection. This inherited limit is recorded in spec 6.2's table; incoming
  validation is unchanged. Filtering first-run writer recipients also prevents a refused
  duplicate attempt from dominating the successful self-message's status (`fanout.rs:1031-1063`).

All review items have a documentation disposition. The plan's twelve-task structure remains;
implementation and its specified independent reviews, mutation checks and native gates are
still future work. This does not claim a new independent review of these documentation edits.

## Recheck of spec revision 12 and the revised plan, 2026-10-23

- Reviewed: the uncommitted working tree over HEAD `5fa7ebd4`: the spec's revision 12 diff
  (107 lines) and the plan's diff (301 lines), against the disposition table above and the
  code. Two independent read-only reviewers at the Opus tier (one on the spec diff, one on
  the plan diff against every disposition row), adjudicated by the session's controller. No
  build, test or edit.
- Result: D1 and D2 are applied correctly in substance; every disposition row has plan text
  that implements it; both source-backed refinements hold against the code (`has_access`
  does succeed through membership, `access_token.rs:779-787`; a payload with no listed
  server-scheduled line of the recipient is refused by the drain on both paths,
  `snapshot.rs:217`, `:353-361`). One P2 (the spec's own hook list), seventeen P3, nine
  notes. Two of the P3s are decisions for Jay (D3, D4) because they change what the
  parity contract covers. The plan can start executing once the P2 and D3/D4 are settled;
  the other P3s belong to their owning tasks.

### Decisions for Jay

**D3. Does the forced exception follow account resolution, or stay on the writer's listed
lines?** Revision 12 classifies writer recipients by `za_account_id_from_email`, which
also applies the domain's catch-all fallback and custom rules (`principals.rs:741-754`,
`:786-791`), and applies the same classification to the snapshot override that lifts
RSVP and PARTSTAT. So a DECLINED or `RSVP=FALSE` line on `bob+x@`, or on any unknown
address of a domain whose catch-all is Bob, gets a forced REQUEST when Bob writes and
nothing when Alice writes: a designed divergence that now reaches beyond the writer's own
lines, which case 25's exception run (2840-2844) does not cover. Recommended: keep the
exception's purpose narrow. The override (`force_send`, `rsvp`) applies to the writer's
listed addresses only; the resolution-based classification stays for routing (the kept
messages, the first-run filter, the carve-out). State it in 6.2 and in 11.6's exception
kill. (Spec recheck F9.)

**D4. The writer-recipient definition drops a writer-only group address.** The merged list
holds the addresses of the writer's groups (`build_account_info`, `principals.rs:849-863`).
A group `team@` that Bob belongs to and Alice does not is local under the merged list, so
the first run never addresses it; under revision 12 it is not a writer recipient either
(it resolves to the group's own id, `principals.rs:793-799`), so nothing is kept. When
Alice writes, `team@` is addressed, resolves to a plain group and records `5.3` on her
event (`fanout.rs:1090-1096`, `:538-539`). Revision 11's "merged list minus the owner's"
kept it. Recommended: define writer recipients as the union of the writer's merged-list
addresses that are not local under the owner's list and the addresses that resolve to the
writer's account. That restores revision 11's coverage and adds the sub-addresses; with
it, a second run that returns `Ok` while the first returned `NothingToSend` always keeps
something, so "the second run built" can simply mean its builder returned `Ok`. Add a
`team@` fixture to case 25. (Plan recheck F4, spec recheck F2 and N2.)

### P2

**R1. [P2] The spec's hook list contradicts cases 21 and 22.** Spec 11.2's hook paragraph
(2505-2515) still ends "beside the existing `COMMIT_PAUSE`, which case 22 also places
before an event `DELETE`'s commit", while cases 21-22 (2746, 2749, 2753) now use
`UPDATE_COMMIT_PAUSE` and `DELETE_COMMIT_PAUSE`, which the spec never defines. The plan
defines them (445-448, 464-467). Fix: replace the sentence with the two new one-shot
pauses of the `Pause` shape, keyed by the resource's account: `UPDATE_COMMIT_PAUSE` (an
update PUT after its fan-out and seal, before `update.rs:405`) and `DELETE_COMMIT_PAUSE`
(an event DELETE after its fan-out loop, before `delete.rs:276`), beside `COMMIT_PAUSE`
(a leased create, `update.rs:631`). (Spec recheck F1.)

### P3

**R2. [P3] What "built" means after the first-run filter.** Spec 1819-1830 keys the
second run's finalize on the first run having returned `NothingToSend`; the filter text
(1853-1860) never says the filter leaves the first run's outcome alone. State: an `Ok`
first run stays built and finalized even if the filter empties its recipient list (it
finalized inside `itip_update`/`itip_create`); and, with D4, "the second run built" means
its builder returned `Ok`. The plan's Task 9 step 3 sentence "filtering a second-run
result to no writer recipients is NothingToSend" then needs the same wording. (Spec
recheck F2, plan recheck F4.)

**R3. [P3] Case 25's sub-address fixture cannot see the second new kill.** A first-run
attempt to `bob+x@` left in the output resolves to the sender, returns `NotInternal`,
writes no item and records `5.3`, which outranks the self item's `1.2` per address
(`fanout.rs:1031-1063`; stored on the owner's event, `update.rs:371`). Item counts and
token presence do not change; only the owner's stored `SCHEDULE-STATUS` does, and REQUEST
export strips it, so Bob's copies never show it. Fix, in spec case 25 and plan Task 10
step 5: compare the `SCHEDULE-STATUS` stored on the owner's event for every
writer-recipient line, in both runs, after every step (`schedule_statuses`,
`tests/src/utils/za.rs:813`). (Spec recheck F3, plan recheck F1.)

**R4. [P3] The main sub-address fixture is not well defined.** Spec 2815-2821 says
"repeat the alias steps with an unlisted `bob+x@`", but the alias steps remove or
replace `bob@` on the master, against "with `bob@` also a server-scheduled attendee on
the master". Where `bob+x@` attends is unstated. Fix: give the layout and steps, for
example master `bob@` and carol; override O1 `bob@`, `bob+x@`, carol; steps: create, a
master SUMMARY edit, a DTSTART edit, `bob+x@` removed from O1 (a partial CANCEL carrying
only its line, refused in both runs), O1 deleted (a CANCEL carrying both of Bob's lines,
accepted), the event deleted; compare Bob's inbox counts as multisets (the partials come
in hash order, `organizer.rs:210-221`). (Spec recheck F4.)

**R5. [P3] Stale "writer's addresses" wording.** Spec 1752 (the contract), 1844
("delivered to the writer's addresses only"), 1966 ("one self item per writer address"),
3076 (invariant 38) and 3202 (13.1 item 9). Read literally, 1844 reintroduces D2. Fix:
"writer recipients" in all five. (Spec recheck F5.)

**R6. [P3] The DELETE bullet omits the first-run filter.** Spec 1897-1910 walks the loop
step by step with no filter step, yet the first run's deletion CANCEL names every
non-local recipient over every component (`event_cancel.rs:41-52`), `bob+x@` included.
Also `delete.rs:261` already converts the `Option` (`message.message.into_iter().collect()`).
Fix: "removes the writer recipients from the first run's CANCEL, dropping it if emptied,
before appending the retained second-run CANCEL". (Spec recheck F6.)

**R7. [P3] 11.1 does not list the unit test the third new kill names.** Kill 3003-3005
names "a negative routing unit"; 11.1's fan-out list (2483-2490) has none; the plan has it
(Task 9 step 1). Fix: add to 11.1: an ordinary-provenance item addressed to the sender's
own account (a listed alias or a sub-address) is not delivered even when it resolves to a
key account with a served key; only a retained second-run item is admitted; a lookup
failure yields `5.1`. (Spec recheck F7.)

**R8. [P3] Section 14 lacks two dependencies revision 12 created.** (a) The classification
depends on `account_id_from_email_inner`: the `+` strip on the canonical address before
the lookup (`principals.rs:739-758`), the custom expression, the catch-all fallback
(`:786-791`) and the Account-only result (`:793-799`). (b) D1 depends on upstream passing
`send_itip=false` on the overwrite; if upstream ever set it, `delete_all_za` without keys
would queue a plain iMIP CANCEL on the task queue for a key account (`storage.rs:581-614`),
against the rule that a key account's messages never reach the queue (`update.rs:290-291`).
Fix: add both to "Upstream code the design depends on"; 3373-3375's "filtering for
unlisted writer sub-addresses" is narrower than the rule. (Spec recheck F8.)

**R9. [P3] D1's consequence is recorded nowhere, and case 12 has no overwrite step.** 5.6
says the overwrite "deletes it without scheduling, as upstream does" but not that the
overwritten event's attendees, the writer's own copy included, keep active copies; 6.2's
limits list (2065-2099) does not mention it; case 12 (2695-2702) has no overwrite step
although plan Task 5 hangs its "no pending items" assertion on it; 11.6 has no kill for
D1. Fix: one line in the limits list; a case 12 step ("bob's MOVE that overwrites a
scheduled event in the shared calendar writes no pending item and builds no CANCEL, as
alice's own does"); the kill "the overwrite scheduled". (Spec recheck F10.)

**R10. [P3] Row 6's wording.** Spec 2063 says "a message addressed only to an unlisted
sub-address"; rejection depends on the payload's lines, not the addressing: a full REQUEST
to `bob+x@` alone carries `bob@`'s master line and imports, a partial CANCEL keeps only
its own line (`event_cancel.rs:164-176`) and is refused. Fix: "a message whose payload has
no listed, server-scheduled line of the recipient". (Spec recheck F11.)

**R11. [P3] The combining step has no producer.** Plan 230-231 says only the fork's
second-run helper constructs `DelegatedSelf`, but Task 8's helpers return
`ZaSelfMessages { messages: Vec<ItipMessage<ICalendar>> }` (684-694) while
`ZaFanOutMessage`/`ZaMessageSource` are Task 9's in `fanout.rs` (736-739); three DAV sites
must filter the first run, drop empty messages, append and tag (766-792), so DAV would
construct `DelegatedSelf`. The "request-local recipient-resolution context" (740-745) has
no type name or place in `za_fan_out`'s signature, and the resolver of the writer set has
no name or file. Fix: a groupware resolver in `delegation.rs` returning a named resolution
type; a combinator there (for example
`za_combine_delegated(first, second: Option<ZaSelfMessages>, &resolution) -> Vec<ZaFanOutMessage>`);
`DelegatedSelf` constructible only through a `pub(crate)` constructor; `za_fan_out`'s full
new parameter list in Shared contracts. (Plan recheck F2.)

**R12. [P3] Task 6 cannot tell which assertion of a chunk failed.** Plan 575-578 sends a
lost chunk to Task 7's vault check, but the store reports one unnamed `AssertValueFailed`
and `za_reread_verified` is private to `crates/http/src/api/vault.rs:605`, unreachable
from groupware's `convert.rs`. Fix: on any chunk loss, re-read the calendar and the
chunk's events: ring `reverting`, join the rollback; calendar or events changed, rebuild
the chunk; neither changed, the lost assertion was the vault's or the Account's, so return
a typed error to the HTTP loop, which classifies it as Task 7 does. Say whether per-event
losses count toward the eight attempts, and cap chunk retries. (Plan recheck F3, N-f.)

**R13. [P3] The update path has two builder branches; the plan and the spec name one.**
`update.rs:259-266` calls `itip_update` when the stored event has a schedule tag and
`itip_create` otherwise; plan 767-769 says only "before `itip_update` at `update.rs:259`",
and spec 6.2 pairs "an update" with `organizer_handle_update`. Fix: the second run mirrors
upstream's branch at that site: `za_self_update` when `schedule_tag.is_some()`, otherwise
`za_self_create`, both before `:259`; say so in the spec's second-run bullet too. (Plan
recheck F5.)

**R14. [P3] The provenance tag has no slot in the reconciliation.**
`za_reconcile_sequences(&mut ICalendar, &mut [ItipMessage<ICalendar>], &[String])`
(`sequence.rs:137-141`, called at `fanout.rs:234`) cannot take the tagged type plan 739
describes; `za_decline_all`'s push (`fanout.rs:280`) needs the `Upstream` tag; and
`za_fan_out` already has a `source: ZaSource<'_>` parameter (`fanout.rs:201`), so the new
field should not be called `source`. Fix: either `sequence.rs` joins Task 9's Files or
`fanout.rs` passes a projected slice; name the field `provenance`. (Plan recheck N-b.)

**R15. [P3] The owner's unlisted sub-address as an attendee.** With `alice+y@` attending,
Alice's own write records `5.3` for it (self-exclusion); Bob's delegated write, whose
sender is Bob, writes an item into Alice's inbox. Contrived, but a divergence in the
owner's event state. Fix: on a delegated organizer fan-out, exclude the event account's
inbox as the sender's own is excluded, recording the same status; one sentence in 6.2's
carve-out bullet. (Plan recheck N-e.)

**R16. [P3] Spec 3.6 and the per-request read memo.** Spec 3.6 (715) says the domain id is
"read with the calendar's ring on every use"; the plan's request-local memo for reads
(47-49, 411-415), which the review recommended, is not in the spec. Fix: one sentence in
3.6 or 5.5 allowing a per-request memo for reads, with the fresh read kept for writes.
(Plan recheck N-g.)

**R17. [P3] `seal_calendar_changes` now rejects every change to the entry set.** Plan
369-371. Grant, revoke, sweep and rollback add or remove preference entries, so they must
write entries through another path, which Tasks 6 and 7 do not name. Fix: name the
entry-writing function those mutations use (the ring and ledger writer of Task 1 or 7),
and keep `seal_calendar_changes` for PROPPATCH. (Plan recheck N-h.)

**R18. [P3] The ownership table still under-reports.** `dav/src/common/za.rs` (163) is also
Task 5's (489); `sharing/resolve.rs` (162) also Tasks 4, 5, 6 (430, 489, 543);
`sharing/{mod,format}.rs` (159) also Tasks 6, 7 (543, 594); `convert.rs` (168) also Task
7 (594); `delegation.rs` (171) also Task 9 (728); `groupware/src/calendar/mod.rs` and
`vault/src/keys.rs` have no row (Tasks 1 and 8, 286, 678, 287). Coverage rows §5.6 (1011)
still list Task 9, which no longer touches COPY/MOVE, and §5.7 (1012) lists Task 10, where
only Task 12 step 1 owns it. (Plan recheck F6, N-a.)

### Notes

- **R-N1. The "source-unlink" citations.** Spec 5.6's `:703-712` (move_event's
  cross-account branch) and `:1022-1030` (copy_container's cross-account MOVE) sit behind
  `za_refuse_cross_account` (`copy_move.rs:122`), so key accounts never reach them; an
  in-account MOVE unlinks its source by name (`names.swap_remove`, `:678`) and deletes
  nothing; `:569-578` is the COPY overwrite, refused for a shared calendar anyway. The one
  site that matters is the MOVE overwrite (`:747-756`); the collection overwrite
  `delete_with_events(..., false)` at `:931-943` is not cited. The citations are correct
  for what they show; the wording "source-unlink deletions" is not.
- **R-N2. Section 9's table row 2410** reads as if a grantee's MOVE and PROPPATCH schedule
  "with `SENT-BY`"; add "(MOVE and PROPPATCH send nothing)".
- **R-N3. "Provenance-only" (13.1, 3211)** can be read as "provenance suffices", against
  6.2's "all receive permissions and key checks still apply"; "provenance-keyed" is
  clearer. Invariant 38 does not state the self-inbox rule.
- **R-N4. The revision header (49-53)** omits the first-run removal rule, row 6, the kills,
  invariant 38 and 14, and says the hook names are explicit while 11.2's list is not (R1).
- **R-N5. Case 21's "`COMMIT_PAUSE` for a create or `UPDATE_COMMIT_PAUSE` for an update"**
  is either/or; say whether both variants run.
- **R-N6. Case 22 and delivered items.** Both pause points sit after the fan-out
  (`update.rs:358-374`, `delete.rs:255-274`), so a held write that ends in 403 leaves its
  delivered items standing (spec 4.6); saying so prevents a "no items" assertion.
- **R-N7. `has_shared_access` still passes group-derived grants**, because `access_to` is
  built from the account's own and its groups' ACLs (`access_token.rs:78-122`); "without
  membership" means without the `is_member` shortcut only. With the Impersonate refusal
  and the owner arm, it differs from `has_access` only for a token that is a member of the
  owner principal, which cannot happen for an individual key account: sound defence in
  depth with no observable mutation. Reword so implementers keep the own-id `acls` check
  as the authority.
- **R-N8. Normalization.** The writer set is "normalized" (plan 696) without naming the
  function; `za_normalize_address` is `sanitize_email` plus `to_canonical_address`
  (`vault.rs:243`) while a snapshot's `Email.email` is `sanitize_email` only
  (`scheduling/mod.rs:244-250`); they agree for lowercase ASCII. State that builders
  compare `za_normalize_address(&attendee.email.email)`.
- **R-N9. An ORGANIZER that is the writer's unlisted sub-address** takes the attendee path
  (the gate's ORGANIZER test stays exact membership), gets no second run, and its REPLY is
  dropped as self-inbox: consistent with "no second run on the attendee side", exercised by
  no case.

### Verified in this recheck

Spec: the `copy_move.rs` sites pass `false` at 576, 710, 754 and 1029, and
`DestroyArchive::delete` forwards to `delete_za` without keys (`storage.rs:446-467`) with
the CANCEL gated on `send_itip` (`:581`); `za_account_id_from_email` (`principals.rs:693`)
is the lookup delivery (`fanout.rs:1083`) and the drain's sender check (`drain.rs:644-661`)
use, with sub-addressing enabled by default; the carve-out (`fanout.rs:1086-1089`) precedes
the key-account, permission and served-key checks, so "all checks still apply" is
implementable; status recording is worst-wins per address (`:1043-1063`); `ItipMessage.to`
is a `Vec<String>` of sanitized snapshot addresses, so removing an address and dropping an
emptied message leave payloads untouched; the reconciliation sees the same SEQUENCEs after
filtering, since per-address change sets are identical in both runs; the two provenances
address disjoint sets after the filter, so the fan-out's per-address memo cannot leak a
self-inbox decision; the schedule-tag rule holds; row 6's claim holds on both paths; the
partial CANCEL to a sub-address is one item in both runs; no sentence outside 5.6 still
assumes the overwrite schedules; the header, 13.1 item 9 and 14's `fanout.rs` text agree
with 6.2; the stopping criterion and the normalization list need no change.

Plan: every disposition row has implementing text (1/D1 at 94-96, 172, 523-525, 728-732,
805-807; 2 at 445-448, 464-467; 3 and 5 at 686-687, 766-771; 4/D2 at 102-104, 229-231,
688-699, 705, 716-719, 736-745, 761-763, 785-792, 856-866, 940-941; 6 at 137-143, 165,
380, 392-394, 839-840; 7, 8, 11 at 53-54, 476-477, 575-578, 644-649; 9, 10 at 133-136,
215-216, 420-421, 492-504, 526-531; 12, 13, 15, 20 at 66-69, 665-667, 728-732, 796-799,
808-809, 966, 972-973; 14, 17, 18, 19 at 47-49, 409, 411-415, 472-473, 519-521, 777-778,
903-905; 16, 21, 22 at 274-277, 407-409, 464-467, 531-535, 583-585, 663-665, 906-908,
994-995; N1-N5 at 207-208, 287-289, 369-371, 477-482, 633-636; N6-N7 at 266-272, 764-765,
778-779; N8-N11 at 100-106, 164-165, 172, 174, 380-381, 431, 542-544, 593-595, 644,
671-672, 815, 857, 949). Task 7 checks the Account before the vault, which is sound (when
the owner is destroyed both lose and case 14 expects `account deleted`). `za_finalize`
wraps `itip_finalize` with a matching signature; `ZaBoundaryCommit { change_id }` and
`ArchiveVersion::change_id` exist; the five fan-out sites are exact; both pauses are placed
and keyed as the review asked; the three new kills are in the ledger (51 kills plus the
attendee-side equivalence); the commands, the task order and the fenced-before-published
dependency are intact; the trace strings at plan 66-69 match the spec.

## Revision 13 disposition, 2026-10-23

Jay accepted D3/D4 and authorized this documentation correction pass, with Sol-or-lower
agents. The requested `gpt-6.1-sol` high agent could not start (`agent thread limit reached`);
a retry after Jay resumed the turn returned the same error. The primary made the edits
and checked them against source. No higher-tier substitute,
new independent review, implementation, build or runtime test ran. The recheck above remains
unchanged as the report on revision 12.

The revision 13 contract uses two sets. L is the writer's listed scheduling addresses minus
owner-local addresses, including writer-only groups; R is L union addresses resolving to the
writer, always excluding owner-local addresses. Only L gets the RSVP/PARTSTAT exception;
R determines retention and first-run removal. Normal delivery checks still apply: a retained
group is not redirected to the writer, and self-inbox admission requires both provenance and
writer-account resolution. The event owner's inbox remains excluded on that delegated path.

| Recheck item | Revision 13 disposition |
|---|---|
| D3, D4 | Spec 6.2 defines L/R and the narrow exception; invariant 38 and plan Tasks 8–10 use it. Case 25 adds group and unlisted/custom/catch-all coverage, with matching mutation checks. |
| R1 | Spec 11.2 defines the update and DELETE pauses with their account key and exact placement, separately from leased-create `COMMIT_PAUSE`. |
| R2 | Preserve raw builder outcomes. An `Ok` first run stays finalized after filtering; a successful second run stays built. Test the D4 implication that first-run `NothingToSend` plus second-run success retains an attempt. |
| R3–R4 | Case 25 has a distinct sub-address layout and explicit steps, including re-add before deleting O1. It compares multiset item counts, token presence, copies, and the owner's stored per-line `SCHEDULE-STATUS`; after event deletion it compares absence. |
| R5–R6 | Contract, per-address delivery, invariant and handoff language use R. DELETE explicitly filters its already-collected first-run vector before appending the retained second-run CANCEL. |
| R7–R10 | Units cover provenance, lookup failure and owner-inbox exclusion; section 14 names resolution and no-scheduling dependencies. MOVE's retained attendee copies and overwrite regression/mutation are explicit. Inherited row 6 states a payload-locality rule, not an addressing rule. |
| R11 | `delegation.rs` owns named `ZaDelegationResolution`, `za_resolve_delegation` and `za_combine_delegated`. The combinator alone produces delegated provenance; Shared contracts gives the full fan-out signature, including its optional resolution argument. |
| R12 | Conversion returns typed `ConversionConflict` to HTTP on every opaque assertion loss. HTTP checks fresh vault/Account guards before any resume, even if progress changed. Separate counters bound conversion losses and decision attempts; no groupware call reaches an HTTP-private helper. See the refinement below. |
| R13–R14 | The second run mirrors the existing-event PUT's schedule-tag branch. Fan-out unzips payloads/provenance, reconciles the unchanged payload slice, and re-zips in identical order; the synthesized decline is tagged Upstream. The field is named `provenance`. |
| R15–R17 | Delegated organizer delivery excludes the owner's resolved inbox; spec 3.6 allows a request-local read memo; explicit `za_replace_calendar_entries` assembly serves conversion/grant/revoke/sweep/rollback while the property codec retains its fixed-entry-set invariant. |
| R18 | Ownership rows reflect all touching tasks and module/primitive files. COPY/MOVE and JMAP coverage point to their actual owners. |
| R-N1–R-N4 | MOVE citations distinguish name removal from deletion; the feature table says MOVE/PROPPATCH send nothing; provenance is keyed and checked, not sufficient alone; revision header and invariant describe the current rules. |
| R-N5–R-N9 | Case 21 runs both variants; case 22 retains already-delivered items after a refused commit; the access-entry helper does not replace own-id ACL checks; normalization is named; an unlisted writer-organizer fixture preserves the attendee-side/no-second-run behavior. |

Two implementation details refine the suggested fixes:

- R4's proposed sequence removed `bob+x@` and then expected O1's later deletion to cancel
  both Bob lines. The fixture now re-adds it before that deletion, so the expected payload
  follows from the prior tree.
- R12 cannot establish which assertion failed merely because a calendar/event changed;
  vault or Account state may have changed concurrently. Every conversion loss returns to
  HTTP for terminal guard classification first. A request permits at most eight conversion
  assertion losses (calendar or event), independent of successful chunk count. An unchanged-
  progress loss also consumes a decision attempt; changed progress resumes under fresh state.
  Terminal account/vault errors take precedence over budget exhaustion. Committed counts
  travel with the typed result and are accumulated once across resumes.

The MOVE overwrite check compares both pending items and queued scheduling tasks while the
task consumer is paused, so an accidentally queued plain CANCEL cannot disappear before
the assertion. This implements the no-queue dependency already identified by R8.

All recheck items have a documentation disposition. The twelve-task structure and its future
independent reviews, automated checks and native gates remain. This pass provides source and
document checks only, not runtime acceptance or a new independent review.

## Revision 13 independent re-review, 2026-10-23

Reviewed the current uncommitted revision 13 spec and plan in `release3-mail-spec`, branch
`release4-sharing-spec-r1`, against source at `5fa7ebd4`. Three bounded read-only reviewers
covered calendar transactions (Tasks 1–7), delegated scheduling (Tasks 8–10), and interfaces,
fixtures and qualification (Tasks 11–12). The first two used `gpt-6.1-sol` with `xhigh`
reasoning; the third used `gpt-6-sol` with `high` reasoning. The primary checked the retained
finding against source. No edits, builds, tests or runtime probes ran during that review.

**R19. [P2] Preserve quiet lookup through catch-all resolution.** Task 9 (plan 822–825)
uses `za_account_id_from_email` as a quiet dependency, but its catch-all branch calls the
ordinary `rcpt_id_from_email(catch_all)` (`crates/common/src/cache/principals.rs:786–790`).
That helper calls non-quiet `domain()` and `rcpt_id_from_parts()` (`:360–364`), which can emit
domain-name and local-part cache events (`:254–276`, `:347–352`, and `domain_inner`). The
planned fixed recipient-lookup failure trace does not suppress these successful lookup events.

Configure a local domain's catch-all to resolve to Bob, then make a delegated write with an
unlisted attendee in that domain. Computing R follows the fallback and can trace the catch-all
address's components, even when RSVP/PARTSTAT filters suppress delivery. The delegated sender
check uses the same dependency. This conflicts with plan 66–67 and spec §10 (2482–2486), which
permit ids, levels and counts but no addresses or content.

Correction: give Task 9 ownership of preserving `quiet` through catch-all resolution in
`principals.rs`, while keeping ordinary callers' behavior. Add captured cache-hit and
cache-miss trace fixtures for both R classification and delegated sender verification, using
catch-all address components as canaries.

Result: one P2 finding; no other actionable residual finding. The finding is a source-backed
plan dependency gap, not a claim that the future source fix or regression tests have run.

## Review of the revision 13 plan changes, Task 9 in focus, 2026-10-23

- Reviewed: the committed plan diff `5fa7ebd4..76d01f1d` (414 insertions, 86 deletions),
  against spec revision 13 (uncommitted in the working tree), the revision 13 disposition
  table and R19 above, and the code at HEAD (the code tree is unchanged since the plan's
  snapshot). Two independent read-only reviewers at the Opus tier, one on Tasks 8 and 9
  against the code, one on the rest of the diff against the disposition table; adjudicated
  by the session's controller. No build, test or edit.
- Result: the delegation mechanism of Tasks 8 and 9 matches revision 13's L and R sets, the
  combinator and provenance design is sound, the fan-out's unzip-reconcile-zip is safe (the
  reconciliation mutates in place and changes neither length nor order), and the R19 fix is
  correct and applicable without changing ordinary callers' results or traces. No P1; four
  P2; sixteen P3; one further decision for Jay (D5); fourteen notes, several of them
  implementation pointers worth copying into the tasks. The four P2s are test feasibility
  and interface gaps, not design errors; each is a paragraph to fix before its task starts.

### Decision for Jay

**D5. The RSVP/PARTSTAT override reaches writer-only group lines.** L includes the writer's
group addresses that the owner does not hold, and the override applies to every L entry
(plan 238-241, 786, 799-800; spec 1788-1794). A DECLINED or `RSVP=FALSE` `team@` line
therefore gets a forced message when Bob writes, which is retained (L is inside R) and
records `5.3` on `team@`'s line of the owner's event (`fanout.rs:1082-1096`, `:538-540`),
while Alice's own write sends nothing to that line and records no status. The divergence
sits outside the writer's copy and serves none of the exception's purpose, and case 25's
`team@` fixture uses ordinary RSVP and PARTSTAT, so nothing tests or records it.
Recommended: confine the override to the L entries that resolve to the writer's account,
the writer's own listed aliases; keep writer-only groups in L and R for retention and
removal only. One sentence in 6.2 and in Shared contracts. (Task 9 reviewer F6.)

### P2

**S1. [P2] The gate and SENT-BY need the writer's own address list, and nothing carries it.**
Plan 252-253, 764, 767-773, 822-826, 861-863, 894-895, 925-927; spec 1780-1782 (the gate:
the ORGANIZER "an address of the owner's and not of the writer's") and 1708-1717 (no
SENT-BY "when the property names the writer's own address"). The DAV sites hold the merged
`scheduling_account_info` (`update.rs:128-130`, `delete.rs:79-81`) and step 3 fetches the
owner's; from those two lists one cannot tell whether an owner address is also the
writer's (a group both hold). Tested literally against the merged list, "not of the
writer's" fails for every owner ORGANIZER and the gate never applies; tested as merged
minus owner, a shared-group ORGANIZER counts as the owner's, against spec 1781, and Task
8's "shared group-address gates" test (783) states no expected outcome. Ruling: an address
both accounts hold counts as the owner's for the gate (the write is organizer-side for the
owner) and as the writer's own for SENT-BY (none is added, since the property names an
address the writer holds); state both in the spec's gate sentence, in Task 8's test and
in `za_mark_sent_by`. L is unaffected: merged minus owner equals the writer's own minus
owner. (Task 9 reviewer F1.)

**S2. [P2] "Test lookup failures" cannot be executed.** Plan 829-832, 859-860, 877. No hook
makes `za_account_id_from_email` or `resolve_inner` return `Err` (the existing `5.1` tests
come from write failures: `tests/src/za/fanout.rs:646-721`, `pinning.rs:1930-1946`);
`ZaDelegationResolution`'s fields are crate-private, so the tests crate cannot inject
`Failed`; groupware units have no `Server`; `test_hooks.rs` is owned by Tasks 4-7 and
absent from Task 9's Files. Fix: add a test-mode lookup-failure hook keyed by the sender's
id, consulted by the resolver's lookup and by `resolve_inner`, and add `test_hooks.rs` to
Task 9's Files and the ownership row; assert `5.1` and no self item for a listed L line,
and `5.1` with the first run kept for an unlisted one. (Task 9 reviewer F2.)

**S3. [P2] The MOVE-overwrite check and its kill cannot see the obvious mutation.** Plan 97-99,
564-570, ledger 1101; spec 11.6 (3098), case 12 (2760-2766), 14 (3507-3511). The overwrite
is `DestroyArchive(event).delete(..., false, ...)` (`copy_move.rs:747-756`), a wrapper that
passes no keys and discards the result (`storage.rs:446-467`). Flipping the flag to `true`
on a sealed key-account event changes nothing observable: without keys the tree stays
sealed (`storage.rs:589`), ORGANIZER and ATTENDEE are invisible in a sealed tree
(`seal/policy.rs:21-46`), `itip_snapshot` returns `NoSchedulingInfo` and nothing is queued
(`:594`, `:611`). Every event of a shared calendar is sealed, so case 12's pending-item,
queued-task and active-copy assertions pass with the mutation in place, and spec 14's
"the ordinary `delete_all_za` path without keys queues a plain iMIP message" holds only for
plaintext trees. Fix, in Task 5: the key-account overwrite deletion passes the resolved
domain and the writer's keys with `send_itip=false` (with keys, a built CANCEL can never be
queued in plaintext); make the kill observable at a seam, asserting that the overwrite's
`delete_za` returns no message, or have Task 11 record it as equivalent; reword spec 14.
Name the helpers the check uses: `TASK_CONSUMER_PAUSE` (`test_hooks.rs:26-27`, `:361-367`),
`hold_task_consumer` and `ConsumerHeld` (`tests/src/utils/za.rs:868-900`), `itip_tasks_for`
(`:902-924`), `pending_ids` and `inbox_ids` (`:1269`); the pattern with a plain-account
control is `tests/src/za/fanout.rs:948-1001`; the queued task records the destination
owner's account (`scheduling/itip.rs:457-463`), so count the owner's tasks and Bob's.
(Rest reviewer F1, N3.)

**S4. [P2] Task 6 asks for tests only Task 7's HTTP loop can provide.** Plan 617-620,
632-639 against 692-694, 714-717. "Terminal guard classification wins", "the eighth
conversion loss ends with `conflict`" and the 3,000-event budget test depend on the HTTP
coordinator, the 409 body and both counters, which exist only in Task 7; `za_reread_verified`
is private to `vault.rs:605` until Task 7 exposes it; line 602 forbids a groupware
coordinator. Fix: Task 6 tests the groupware contract (every injected loss, calendar,
event, combined with a vault or Account change, or unchanged, returns `ConversionConflict`
with the right `committed` counts and `progress_changed`, and a resume works from each
state); the precedence test, the eighth-loss 409 and the budget test move to Task 7 step 1.
(Rest reviewer F2.)

### P3

**S5. [P3] Step 1 of Task 9 mixes assertions only the DAV path can meet.** Plan 866-881,
892-893. `schedule_tag_after_self_only_create.is_some()`, `stored_force_send_parameter ==
None`, the owner's stored per-line statuses, stored pairs, and case 9's and 10's DAV flows
need a grantee write, refused until Task 10. Fix: split step 1 into direct-fixture tests,
each with its observable (`ZaFanOutResult.{statuses,pairs}`, `ZaSelfMessages`, `za_finalize`
on a cloned tree, inbox items, drain outcomes), and HTTP definitions that move to Task 10
step 5 under the convention at 296-299; add Task 5 and Task 9 to the activation lists
(297-299, 998), since Bob's MOVE-overwrite run of case 12 also has no activation path.
(Task 9 reviewer F3, rest reviewer F7.)

**S6. [P3] The quiet catch-all test's control needs the right expected events per cache
state.** Plan 889-891. A cold positive domain lookup emits no `domainName` event, only
`domainId` (`principals.rs:84-118`, `:158-172`); the address-bearing events are, cold,
`CacheMiss` `email` with the local part for both addresses (`:270-277`), and, warm,
`CacheHit` `domainName` (`:69-75`), `CacheHit` `emailNegative` for the original local part
(`:346-353`) and `CacheHit` `email` for the target (`:254-261`). A control expecting domain
canaries on a cold run fails. Fix: cold control asserts both local parts; warm control
asserts both domain names and both local parts. Harness pointers: `Traced` is public
(`tests/src/za/mail/mod.rs:327-412`); `traced_types()` is private, use
`EventType::variants()`; the nearest precedent is `tests/src/za/tracing.rs:686-718` with its
control at `:662-668`; set and restore the catch-all with
`registry_update_object(ObjectType::Domain, id, json!({"catchAllAddress": …|null}))`
(`properties_impl.rs:1463`), which invalidates the domain cache; use a Bob alias on a
dedicated canary domain (as `bob_alias`, `tests/src/za/pinning.rs:3448-3480`), never a
catch-all on `example.com`; cold caches via `invalidate_local_caches(&[Domain(..),
Account(bob)])` plus `invalidate_all_local_negative_caches()`; the drain's catch-all
SENT-BY must be planted, since the fan-out always writes the writer's primary address.
Task 10's catch-all and custom-rule variants (1020-1022) need the same dedicated domain
and restoration sentence. (Task 9 reviewer F4, N5; rest reviewer F10.)

**S7. [P3] "Classification still resolves" a suppressed attendee is unobservable.** Plan
885-886. The resolution's fields are crate-private and a suppressed attendee produces no
message, so the quiet assertion can pass without the lookup running. Fix: after the cold
quiet run assert cache population (`test.server.inner.cache.emails_negative` holds the
original address and `emails` the target; public fields, `common/src/lib.rs:199-202`), or
include a non-suppressed catch-all attendee whose self item proves resolution. (Task 9
reviewer F5.)

**S8. [P3] The sender-candidate type has no name.** Plan 861-864; `drain.rs:584-588`,
`:631-662`. Fix: name it (for example `ZaSenderCandidate { named: String, sent_by:
Option<String> }` in `drain.rs`), say that `sender_candidates` and `apply(..)` take it, and
which field feeds the merge and the token match. (Task 9 reviewer F7.)

**S9. [P3] R's removal from the first run is specified twice.** Plan 848-856 (the combinator)
and 929-931 (fan-out step 4). Fix: the combinator alone removes R; the fan-out relies on
the invariant that Upstream and DelegatedSelf attempts have disjoint keys, optionally as a
`debug_assert`. (Task 9 reviewer F8.)

**S10. [P3] The resolver bullet's tree wording.** Plan 825-828; spec 1787-1788 ("either
tree"). "Uses the new tree for the create branch and prior tree for update/delete" reads as
the address source; an update's new attendees must be classified too. Fix: "gate: the
ORGANIZER of the new tree (Create) or the prior tree (Update, Delete); addresses: every
ATTENDEE of every scheduling component of every tree supplied; owner-local addresses
(exact membership) excluded before lookup". Shared contracts' L (238-240) likewise omits
"over the ATTENDEE addresses in either tree". (Task 9 reviewer F9, rest reviewer N4.)

**S11. [P3] Task 9's drain fixtures need `DrainOn`**, named only in Task 10 (1031); under
the suite's drain skip `trigger_drain` does nothing. Fix: name `DrainOn`,
`plant_inbox_item` (`tests/src/utils/za.rs:971-1049`), `schedule_statuses` and `Traced`
among Task 9's helpers. (Task 9 reviewer F10.)

**S12. [P3] The "ordinary-provenance refusal unit" cannot be a groupware unit test.** Plan
876-877; `resolve_inner` needs a `Server`. Fix: factor a pure per-attempt decision
`(provenance, resolved account, writer_id, owner_id) -> admit | NotInternal` and unit-test
it, or make it a za fixture through the public `za_resolve_delegation`,
`ZaFanOutMessage::upstream`, `za_self_create`, `za_combine_delegated` and `za_fan_out`; say
which in 11.1. (Task 9 reviewer F11.)

**S13. [P3] Spec 3.5 contradicts the new conversion exits.** Plan 637-639, 716; spec 3.5
lines 668-671 ("no request that runs to its end leaves a partly converted calendar
behind"). A request ending on the eighth conversion loss or a terminal guard during phase 2
or the rollback leaves `converted=false` or `reverting`: safe and resumable, but the spec
says otherwise. Fix: record the exit states in 3.5 and test that the next grant resumes
from each. (Rest reviewer F3.)

**S14. [P3] "Resume" is not fully specified.** Plan 629-643. Losses in phase 1, in the
rollback's `reverting` ring write and in the final unshared rewrite are not said to return
`ConversionConflict`; the resume dispatch does not cover no ring (phase 1 again with a new
domain id), `converted=true` (skip), or whether the parentage walk runs again across
requests, which 3.5 requires. Fix: one dispatch sentence per ring state; name the non-chunk
batches; add the `EnumImpl` import to Task 6, whose batches carry the Account assertion.
(Rest reviewer F4, N10.)

**S15. [P3] Which layer runs the precheck and the conversion.** Plan 665-671, 701-704.
`ZaShareApplied` carries no conversion counts, yet the response needs `rewrapped` and
`sealed` (spec 911-913), so HTTP must call conversion itself, and step 4 does not say who
runs the maxShares and linked-event precheck or how HTTP learns conversion is needed. Fix:
HTTP runs the precheck through a named groupware helper and calls `za_convert_calendar`
before `za_share_apply`, or the counts join `ZaShareApplied`. (Rest reviewer F5.)

**S16. [P3] Files and ownership gaps after R18.** `sharing/mod.rs` needs `pub mod` lines
from Tasks 3 and 5 (not in their Files, 415-418, 528-531; row 169 says 1, 6, 7); case 12
lives in `sharing.rs` (spec 2786) but Task 5's Files list only `sharing_races.rs` (531);
Task 7 step 3 changes `contacts.rs` (its `za_resolve` returns only the key,
`contacts.rs:387-395`) yet `contacts.rs` is missing from Task 7's Files (656-659) and row
176; row `sharing/{mod,format}.rs` lists Task 7 but Task 7's Files say `{mod,convert}`;
Task 11's mutations document and Task 12's `CLAUDE.md` and root `README.md` have no row.
The "commit only the owned files" rule (274-275) makes these matter. (Rest reviewer F6.)

**S17. [P3] R19 is in the plan but not in the spec.** Plan 110-112, 182, 834-843, 1099;
spec 14 (3497-3506) lists `account_id_from_email_inner` under code the design depends on
"without editing it", while Task 9 edits its catch-all fallback; 6.2 (1789-1792) calls the
lookup quiet, untrue through the catch-all until Task 9 lands; 11.1 and 11.6 have no quiet
catch-all unit or kill; Task 12 step 5's hotspot update omits `principals.rs`. Fix: carry
R19 into the spec (14's hotspot, 6.2's wording, an 11.1 unit, an 11.6 kill), mark the
ledger row as plan-added until then, name `principals.rs` in Task 12 step 5. (Rest
reviewer F8.)

**S18. [P3] The boundary-pause phase type has no name and its setter will not fit the
helper.** Plan 577-580: `BeforeRenameCommit` and `BeforeBoundaryCompletion` are variants of
an unnamed type, and `HookParked::start` accepts only `set: fn(Option<Arc<HookPause>>)`
(`tests/src/utils/za.rs:836-845`). Fix: name the enum and its home, or use two setters of
the `Pause` shape. (Rest reviewer F9.)

**S19. [P3] Behaviour on a resolution id mismatch, and `ZaSelfMessages` visibility.** Plan
859 ("verifying writer/owner ids match") says nothing about a mismatch: use the same
`Unauthorized` error as the keys check (`fanout.rs:220-227`), and trace a reused `Failed`
once, in the resolver. Plan 755-756: if `messages` is public, any crate can pass crafted
messages through the public combinator and have them tagged `DelegatedSelf` (bounded, since
only addresses resolving to the writer reach its inbox); make the fields crate-private with
read accessors, or have the combinator re-filter to R. (Task 9 reviewer N1, N2.)

**S20. [P3] The owner-inbox exclusion is scoped to the delegated-organizer path.** Plan
934-935; spec 1952-1955. An attendee-side delegated write whose ORGANIZER is an unlisted
owner sub-address (`alice+y@`) gets no resolution, so its REPLY reaches Alice's inbox,
where her own write would record `5.3`. Contrived; apply the exclusion whenever
`event_account` differs from the sender, or record it as inherited. (Task 9 reviewer N3.)

### Notes

- **S-N1.** When the first run returns `NothingToSend` on an update, upstream's error arm
  already bumps the tag (`update.rs:317-320`) and traces `ItipMessageError` (`:311-316`);
  say "bump once" when the second run built, and whether that trace is kept.
- **S-N2.** R16's read memo is carried by Global Constraints 47-49, Review Focus 8 and Task
  3, but not by Decisions; and after a read adapter repairs a pending boundary (Task 5 step
  4) the request's memo still holds the pre-repair archive: replace the memo entry with
  `za_complete_boundary`'s returned archive, or accept a conservative refusal.
- **S-N3.** Ring-only rewrites (rename step 1's headers, the rollback's `reverting` ring
  write) have no named path: `seal_calendar_changes` keeps unopened parts and cannot
  rewrite grant headers; name `za_replace_calendar_entries` with an unchanged entry set, or
  a dedicated helper.
- **S-N4.** Review Focus 6 (103-104) cites Tasks 4 and 7; the opaque conversion losses
  belong to Tasks 6 and 7. Case 25's writer-organizer unlisted sub-address fixture (spec
  2932-2934) is in Task 9 step 1 (880-881) but not in Task 10's case 25 list.
- **S-N5.** Committed state: HEAD's plan cites spec revision 13 and the review's
  dispositions, but at HEAD the spec is revision 11 and this file holds only the original
  review and R19; the disposition, recheck and revision 13 sections are uncommitted. Commit
  them together. The closing line "records two independent review passes" (1200-1201)
  overstates: `5fa7ebd4` holds one review by two reviewers; and the R19 plan edits themselves
  had no independent read before this one.
- **S-N6.** Task 9's delegated DAV branch has no test until Task 10, as the plan says
  (915-916); only the owner path, which never passes the gate, runs in Task 9.

### Verified in this review

R19 and its fix: `domain_inner(&self, domain, quiet)` (`principals.rs:57-147`) and
`rcpt_id_from_parts_inner(&self, local_part, domain_id, quiet)` (`:246-358`) exist, private
with public wrappers; the catch-all fallback at `:786-791` calls the non-quiet
`rcpt_id_from_email` (`:360-371`); the planned inner helper only composes the two; ordinary
callers (`mta.rs:211`, `:229`, `expr/functions/asynch.rs:36`, `account_id_from_email` with
`quiet=false`) keep their results and traces, `za_is_key_rcpt` never reaches the catch-all,
and `contacts.rs:388` becomes quiet too. `za_account_id_from_email` (`:693-695`) is quiet by
construction, returns the group's own id for a group, strips a sub-address at `+` or by the
custom rule (`:739-758`), yields an Account id or `None` through the catch-all (`:786-799`),
`None` for an unknown address or domain, `Err` for a lookup error; the fixed reason is
`zero-access: fan-out recipient lookup failed` (`fanout.rs:1068-1080`). The L and R use in
Tasks 8-9 matches spec 1788-1797, R never intersects the owner's list, and the handling of
listed versus unlisted `Failed` matches spec 1960-1963. `za_fan_out`'s new signature (plan
247-251) carries `event_account` and the optional resolution, with a defined value at all
five sites; `delete.rs:261` already collects the `Option`; step 3's order (second run on
the pre-finalize trees, the first run, `za_finalize` when the first returned
`NothingToSend` and the second built, combine, fan-out) is consistent; normalization
matches (`fanout.rs:301` keys by `za_normalize_address`; `to` entries are
`attendee.email.email`). `za_reconcile_sequences` (`sequence.rs:137-199`) only raises
component SEQUENCEs in place; the only later push (`fanout.rs:280`) is deletion-only and
deletion skips reconciliation; `fanout.rs:205-293` neither filters nor reorders; the
attempts loop keeps message and `to` order. Status codes: `5.3` is `ITIP_STATUS_NOT_ATTEMPTED`
and `5.1` is `ITIP_STATUS_DELIVERY_FAILED` (`itip.rs:86`, `:89`); `NotInternal` maps to
`5.3` and `Failed` to `5.1` (`fanout.rs:537-540`); the owner's own send records `5.3` for its
own unlisted sub-address (`:1086-1089`); the carve-out precedes the key-account, permission
and served-key checks (`:1086-1110`); recording is worst-wins per address (`:1032-1063`);
statuses land at `update.rs:371`. Every revision 13 disposition row has implementing text;
R12's three statements are mutually consistent and bounded (every loss takes one of eight
slots; terminal guards are checked after every loss; every vault write raises the revision,
`common/src/auth/vault.rs:157-161`); the ledger holds the spec's 55 entries (54 kills plus
the attendee-side equivalence) with the right detecting cases, plus the plan-only catch-all
row; coverage rows §5.6, §5.7 and §2 point to their owners; the commands, the task order and
the fenced-before-published dependency are intact; the trace strings and the L/R sentence of
Global Constraints match the spec; every interface name has a producer except the
sender-candidate type (S8) and the boundary-phase type (S18).

## Astra whole-plan finding, 2026-10-23

After the whole plan was committed at `76d01f1d`, a fresh `gpt-6-astra` reviewer with
`xhigh` reasoning inspected the complete plan against the working-tree revision 13 spec
and current source. It retained one P2; the primary checked the source scenario. This was
a read-only review with no builds, tests or runtime probes.

**R20. [P2] Fully seal supported plaintext events on cross-domain MOVE.** Task 5 at that
snapshot (plan 563) requires a DEK-only rewrap. A valid legacy event with no encryption
carriers has no DEK: `unseal_event` explicitly accepts it (`seal/event.rs:264–277`), and
`tests/src/za/dav_seal.rs:1148–1199` exercises this supported state. Moving it from an
unshared calendar into an already converted shared calendar never runs Task 6's conversion
branch. Refusal would break the permitted MOVE; plaintext pass-through would violate the
destination's CK-envelope invariant. Task 5 must distinguish already sealed input from
valid no-carrier input, fully seal the latter with a fresh DEK under the destination domain,
and keep partial/malformed carriers as corruption errors. The focused regression plants a
legacy source event and moves it into an already converted shared destination, checking the
envelope, sole parent, raw plaintext absence, owner/grantee reads and no scheduling.

## Focused correction disposition, revision 14, 2026-10-23

Jay accepted D5's scope and authorized a focused documentation correction pass after the
source-backed assessment of S1–S4. The pass also carries the related interface, fixture and
dependency clarifications, R19 and the separately retained Astra finding R20. The plan and
spec corrections preserve L/R routing and the existing authorization and delivery checks.
This disposition records documentation work; it does not claim implementation or runtime
acceptance. Earlier reports above remain historical records of their reviewed snapshots.

| Item | Correction contract |
|---|---|
| D5 | Only directly owned listed writer aliases receive the snapshot RSVP/PARTSTAT lift. Membership-derived group addresses remain in L/R for retention and first-run removal, using their normal predicates. Alias identity does not depend on a delivery lookup succeeding; a failed lookup can still yield an attempted `5.1`. Suppressed group and failed direct-alias fixtures distinguish these cases. |
| S1 | Carry explicit merged, unmerged writer and owner account-info roles. The merged `AccountInfo.account` retains the writer record; the unmerged scheduling list can be reconstructed with `build_account_info`. Shared owner/writer addresses count as owner-local for the organizer gate and writer-own for SENT-BY. Shared-group organizer rejection at the unchanged drain is an explicit inherited limitation, separate from the existing group-attendee row. |
| S2 | Add a deterministic test-mode lookup-error seam scoped by sender and normalized address. Reuse the resolver's recorded `Failed` result in fan-out. Test listed/direct-alias and unlisted failures, `5.1`, no self item and retention of ordinary first-run attempts. One controllable failure boundary suffices; two independently consumed errors are not required. |
| S3 | The keyed MOVE overwrite passes its resolved domain and writer keys with scheduling off. Observe the actual overwrite invocation's returned scheduling message at a test-only seam; passing keys alone does not make queue counts detect a discarded return value. Retain held-consumer queue/pending counts and ordinary controls as additional checks. Limit the spec's no-key queue claim to trees with visible scheduling information. |
| S4, S13–S15 | Task 6 tests typed converter conflicts, committed counts, progress and resume through direct store-backed fixtures. Task 7 owns HTTP guard precedence, both retry counters, eighth-loss `409` and successful large-conversion budget checks. Name the readonly precheck/coordinator handoff, every conversion write that can conflict, and the no-ring, unconverted, reverting and converted resume states. Bounded exits may leave a safe resumable journal. |
| S5, S11–S12 | Separate Task 9's direct builder/fan-out/drain observables from DAV-only source/tag assertions. Name `DrainOn` and the public harness helpers; Server-dependent routing refusal uses a za fixture. Task 10 activates grantee HTTP cases from Tasks 4, 5, 7 and 9. |
| S6–S7 | Use a dedicated restored catch-all domain, public trace capture and explicit cold/warm controls. Cold controls observe local parts; warm controls also observe domain names. Prove suppressed classification actually ran, and plant the drain's catch-all SENT-BY fixture. |
| S8–S10, S19 | Name the sender-candidate type and its named-sender merge/token role; the combinator alone removes R. Distinguish the organizer tree used for the gate from ATTENDEE collection over both trees. Bound self-result construction with private fields/read accessors, reject mismatched resolution identities, and trace reused lookup failures once. |
| S16–S18, R19 | Correct file ownership and module registrations, name compatible independent rename/completion pause setters, and carry quiet catch-all behavior, fixtures, mutation coverage and merge dependencies into the spec and final handoff. |
| S20 | Record the attendee-side unlisted owner-address REPLY behavior as an inherited limitation; this correction does not expand the organizer-path inbox exclusion. |
| R20 | Restrict DEK-only MOVE to already sealed input. Fully seal valid no-carrier legacy input under the destination domain with the existing calendar/event fences and no scheduling. Test against an already converted shared destination; retain corruption refusals for partial carriers. |
| S-N1–S-N4, S-N6 | Preserve the existing single tag bump and upstream trace, refresh a request memo with the boundary-repair result, name the unchanged-entry-set assembly for ring-only writes, and correct activation/ownership references. |

S-N5 needs a factual qualification: the R19 plan edits did receive a focused independent
`gpt-6-sol` high review in this chat before the later Opus review; that reviewer reported
no findings. That narrow result does not dispose of the broader S1–S20 findings. Commit
`5fa7ebd4` contains one review pass by two reviewers, not two separate review passes. The
revision 13 spec and later historical review sections remain pre-existing uncommitted work;
this pass does not stage or commit them.

The plan corrections were written by `gpt-6.1-sol` with `high` reasoning and the spec
corrections by `gpt-6-sol` with `high` reasoning, in separate owned files. The primary
inspected their diffs and integrated the cross-document refinements. A fresh read-only
`gpt-6.1-sol` reviewer with `xhigh` reasoning then checked the combined correction deltas,
interacting clauses and relevant identity, scheduling, MOVE, trace and harness source.
It identified two stale conversion-state statements: invariant 36's same-request rollback
promise and section 3.5's omission of valid no-carrier plaintext from interrupted mixed
states. Both were corrected and rechecked. The final focused review reported no remaining
findings; this is source/document evidence only.

Documentation checks cover whitespace, Markdown fence balance, final newlines, intended
file scope, preservation of the historical review text and the unrelated mail review, and
the unchanged HEAD/index. Product implementation, builds, tests, runtime probes, mutation
execution and native acceptance remain unperformed. The three corrected documents remain
uncommitted.

## Recheck of revision 14, 2026-10-23

- Reviewed: commit `09ba2858` (spec revision 14, the plan and this file, committed
  together; the paragraph above predates that commit) against the "Focused correction
  disposition, revision 14" table, D5, S1-S20, R19, R20 and the code. One independent
  read-only reviewer at the Opus tier, adjudicated by the session's controller. No build,
  test or edit.
- Result: every row of the disposition table has implementing text in the spec and the plan,
  and the four departures the table declares (one lookup-failure boundary; the overwrite
  seam observing the returned message rather than queue counts; S20 recorded rather than
  the exclusion widened; the unmerged writer list rebuilt from the merged record) are sound
  against the code. The mutation ledger and spec 11.6 agree at 56 entries each, the
  catch-all kill now in the spec; the commands, task order, coverage table and Global
  Constraints are intact; every named hook and helper exists. No P1; one P2; six P3; seven
  notes. Nothing architectural remains: the P2 and the P3s are test seams and fixture
  placement. The plan can start executing once the P2 is written into Task 6; the P3s can
  be folded into the same pass or fixed before their owning tasks.

### P2

**V1. [P2] Task 6 cannot inject the conversion losses case 23 requires.** Spec case 23
(2927-2931) requires opaque assertion losses in phase 1, in the calendar and event chunks,
in the `reverting` ring write and in the final unshared rewrite; plan Task 6 step 1
(664-667) repeats it "through direct store-backed fixtures". The only conversion hooks are
`SHARE_PRECHECK_PAUSE`, `SHARE_REWRAP_PAUSE` and `SHARE_REWRAP_FAIL_ONCE` (plan 703-705; spec
2643-2658): the rewrap pause can produce chunk losses; phase 1 loses only if its archive
read precedes the precheck pause, which neither document says; nothing sits between the
read and the commit of the mark-reverting write, the rollback chunks or the final restore;
and the fail-once hook is a cut, not an `AssertValueFailed` (the existing pattern returns
`UnexpectedError`, `crates/dav/src/calendar/mod.rs:267-275`). Fix: name one injection per
write class, a pause slot placed after that write's read or a fail-once hook that returns
`AssertValueFailed`; state which archive each write asserts relative to its pause; add the
hooks to 11.2's list.

### P3

**V2. [P3] The overwrite seam does not show the flipped flag in Bob's fixture.** Spec case 12
(2855-2862) expects `Some(true)` "for this scheduled fixture", Bob's MOVE, and plan 602-607
records `has_inner_message`. The overwrite passes the mover's own unmerged info
(`copy_move.rs:655-658`); when Bob is neither organizer nor attendee the CANCEL builder
fails (`storage.rs:594`, `event_cancel.rs:28`, `snapshot.rs:353-360`) and the return is
`Some(ZaDeleteMessage { message: None })`, so the inner flag stays false under the
mutation. Task 5's owner fixture does kill it. Fix: record `delete_za`'s outer
`Option::is_some()`, true exactly when `send_itip && schedule_tag && future`
(`storage.rs:581-584`), whoever moves; or require the observation in the owner run and
correct the spec's claim. Name the seam's home (`common::za::test_hooks`) where it is
defined, not only in Task 5's Files.

**V3. [P3] R20's "valid no-carrier" versus "partial carriers" has no classifier.**
`is_sealed` checks only that the root's last entry is `X-ZA-KEY` (`seal/event.rs:151-161`);
`unseal_event` treats everything else as plaintext, stray `X-ZA-SEALED` or `X-ZA-EXTRA`
entries included (`:271-278`); `seal_event` has no already-sealed check (`:187-189`). Spec
5.6 (1646-1647) and 3.5 (646) are stricter than the reader, and plan 591-595 names no
classifier while `seal/*` belongs to Task 2. Fix: define one classifier in Task 2 and use it
in conversion phase 2, MOVE and owner reads; or adopt `is_sealed` and define "partial" as
sealed input whose envelope fails to open.

**V4. [P3] R20's regression and the overwrite observation sit in Task 5's implementation
step.** Plan 596-607 are in step 3; step 1 (575-584) lists only "case 12", and its snippet
`assert_eq!(moved_encrypted_payload, original_encrypted_payload)` (581) is false on the
full-seal path. The regression sentence omits sole parent and no scheduling (R20 asked for
both; spec case 12 asserts sole parent but not no scheduling), and nothing says how an
"already converted shared destination" exists before Tasks 6-7 (Task 4 uses staged
shared-form fixtures, 507). Fix: move both tests into step 1 with the full assertion list,
limit the payload-equality snippet to sealed input, name the staged fixture.

**V5. [P3] D5's failed direct-alias fixture is not in case 25.** Case 25 has the suppressed
group (3013-3015) and the exception run (3043-3049) but no failed direct alias; it appears
in 11.1 (2614-2619) and 6.2 (2014-2016) while plan Task 10 (1148-1149) and ledger row 1235
attribute it to case 25, and no 11.6 mutation covers D5's second clause (alias identity
independent of lookup success). Fix: add it to case 25's exception run (run B records
`5.1` and writes no item; run A sends nothing) or re-attribute it to 11.1 and Task 9;
optionally add the mutation.

**V6. [P3] The shared-group ORGANIZER fixture (inherited row 7) has no explicit test owner.**
Task 8 (863-865) hands "omitted SENT-BY and inherited drain rejection" to Task 9's direct
fixtures, which never name it (967-1007); Task 10's "shared and writer-only group fixtures"
(1127) could mean row 4 or row 7. Fix: name both fixtures in Task 9 step 1 and Task 10.

**V7. [P3] The HTTP forms of cases 3, 23 and 24 have no owning step.** Task 6's tests are
now "direct store-backed" (664); case 24's 409 `calendar has linked events`, case 3's grant
counts and Bob's reads, and case 23's "next grant completes" need Task 7's endpoint or Task
10's admission, but Task 7 step 1 (741) lists cases 4-8, 13-14 and 16-20 and the activation
lists (320, 1123) omit Task 6. Fix: give the end-to-end runs to Task 7 step 1 and Bob's
reads to Task 10, or add Task 6 to the activation lists.

### Notes

- **V-N1.** S20 is labelled "inherited" (spec 2197-2201) but only the delegated write
  produces it; the owner's own write records `5.3` by self-exclusion. The stopping
  criterion (3062-3068) admits only what the owner's write also produces. Acceptable as
  Jay's recorded limitation: call it a fork limitation and let the stopping criterion cite
  the limits list.
- **V-N2.** The catch-all cold reset (plan 999) invalidates one domain, but the fixture uses
  distinct source and target domains (994); `Domain(id)` clears only that id
  (`invalidate.rs:392-396`), so the other stays warm and emits a `domainName` hit. Invalidate
  both.
- **V-N3.** Two spec 3.5 clauses lag the plan: the list of conflicting writes (656-657) omits
  the rollback's per-event rewrap chunks, which the plan includes (679-680); the
  "interrupted first grant" paragraph (728-735) still says the next grant "finishes phase 2"
  and ignores `reverting`, against 707-712.
- **V-N4.** The paragraph above this section says the three documents remain uncommitted;
  `09ba2858` holds all three.
- **V-N5.** The revision 14 header (62-67) omits section 12, though invariant 36 (3270-3274)
  and invariant 38's clause (3293-3295) changed.
- **V-N6.** S-N1's wording (plan 1026-1027) covers the update branch only; at an
  existing-event PUT that takes the create branch (`update.rs:259-266`) the error arm's
  `if let Some(schedule_tag)` (`:323-325`) does nothing, so the tag must be set to 1 when
  only the second run built (spec 1967-1975), which plan 1023-1024 states only generically.
- **V-N7.** Task 5 replaces the read memo (621-622), which lives in `PropFindData`
  (`crates/dav/src/common/propfind.rs:104`), a file not in Task 5's Files; fine if Task 3
  exposes a replace API through `common/za.rs`.

### Verified in this recheck

D5 (spec 1834-1844, 2023-2025, 2053-2059, 3293-3295; plan 72-76, 257-264, 828-830,
862-866, 880, 915-919): `AccountInfo.addresses` is flattened (`principals.rs:835-871`), so
the resolver rebuilds the direct aliases from the account record's own addresses, as the
plan says; a failed lookup on a direct alias yields `5.1` with no item (`fanout.rs:537`).
S1 (spec 1743-1746, 1771-1778, 1816-1826, row 7 at 2173; plan 246-253, 847-850, 863-865,
901-903, 910-912, 960-962, 1010-1013): the merged info keeps the writer's record
(`principals.rs:818-819`); groups are their own accounts, so a shared-group ORGANIZER
resolves to the group's id, not `changed_by`, and both runs are refused alike. S2 (spec
2614-2619; plan 925-931, 984-986, 893, row 194): one boundary suffices because the
resolver does every lookup on the delegated path and the fan-out reuses its results
(`fanout.rs:305-315`). S3 (plan 99-102, 600-614, ledger 1236; spec 1651-1656,
3646-3647): with keys the CANCEL is returned and discarded, not queued
(`storage.rs:595-614`), and the seam calls `delete_za` directly, avoiding the discard at
`storage.rs:466`. S4 with S13-S15: `za_share_precheck` and `ZaSharePrecheck {
needs_conversion }` in `sharing/mutations.rs` (plan 729-734, 768-771); all conflicting
writes (679-680); the four resume states (685-687; spec 707-712); bounded exits (spec
705-707, invariant 36); Task 7 owns precedence, the eighth-loss 409 and the budget test
(741-747). S5 with S11-S12 (plan 967-992, 1123, 1153-1155, 320; spec 2611-2614). S6-S7
(plan 993-1007, 1150-1151; spec 2620-2625; the cold and warm expectations match
`principals.rs:68-75`, `:158-172`, `:254-277`, `:346-353`). S8-S10 and S19:
`ZaSenderCandidate` (962-965, 1073; spec 2106-2108); the combinator alone removes R
(948-949, 1052-1054; spec 1930-1931); the gate's tree versus both-tree collection (254,
910-916; spec 1826-1827); private fields with accessors (823-827); a mismatched identity
returns `Unauthorized`; a reused failure is traced once (956-959). S16-S18 and R19:
ownership table 170-198; Files 440, 555-559, 712-716; setters 623-626; the quiet catch-all
at 932-941 and 1287; spec 2018-2020, 2619-2625, 3213-3214, 3625-3636. S20 (spec 2197-2201,
3025-3026; plan 989-990, 1061-1062, 1146-1147). R20 (spec 642-649, 703-704, 1639-1647,
2846-2854; plan 101-102, 591-599, 1154, 1307). S-N1 to S-N4 and S-N6 (plan 1026-1027,
140-142, 484-486, 619-622, 701, 106-108, 1128, 1037-1039; spec 1909-1911, 699-701,
1388-1391). `HookParked` takes the `Pause` shape (`tests/src/utils/za.rs:836-845`);
`DrainOn` (`tests/src/za/drain.rs:54`), `Traced` (`tests/src/za/mail/mod.rs:327`),
`plant_inbox_item` (`:971`) and `schedule_statuses` (`:813`) exist.
