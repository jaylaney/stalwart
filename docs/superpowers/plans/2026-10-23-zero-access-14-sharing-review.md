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
