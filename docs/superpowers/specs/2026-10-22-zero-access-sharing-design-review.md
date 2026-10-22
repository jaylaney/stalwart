# Zero-access sharing design review

Re-reviewed 2026-10-22 in `release3-mail-spec`, on branch `release4-sharing-spec-r1`.

- Spec: revision 7, draft for review.
- Source HEAD: `4c2211d45fa8f9c98153566c510c7a8980144a1a`.
- Spec SHA-256: `e4dd3cec86608ec3720a6878f6958f45aa702e05151bb2244f3f1143663e1cde`.
- Result: two P2 findings below. The previous P1 key-cache finding is addressed in the design.
- Previous review: revision 6 at `52ebb3f7588d24a925cfdf664839893516811573`,
  committed as `b4d5c79d469bcf1f765e73197d7b0c9a50994c33`.
  This report supersedes that finding list; each earlier finding's disposition follows.
- Method: static review of the revision 6-to-7 spec changes against current Rust source.
  Between the committed previous review and this HEAD, only the spec changed. Three independent
  reviewers covered keys/conversion, DAV sync/rename, and scheduling; the primary reviewer
  checked the evidence and adjudicated findings. Reviewers continued on their inherited parent
  route at `xhigh`; the exact parent model identity and capability ceiling were not exposed.
- Validation boundary: no builds, tests, server runs, client probes, or implementation edits.
  “Addressed” means addressed in the design, not implemented or verified at runtime.
  Proposed regression cases have not been executed.

## Disposition of the revision 6 findings

| Previous finding | Revision 7 disposition |
|---|---|
| 1. [P1] Cached CK survives rollback/reconversion under the same identity | Addressed: each conversion draws a fresh key-domain ID, the cache includes it, every lookup reads the current ring's ID, and associated data binds it ([§3.1](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:374); [§3.6](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:686)). The regression preserves one DAV session across rollback and reconversion ([case 24](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2396)). |
| 2. [P2] Rename into another calendar's slug exposes its history | Partially addressed: rename advances a boundary in every ring, which closes the sequential example. Reading that boundary before the committing transaction leaves a race; see current finding 1. |
| 3. [P2] Self-delivery misses ordinary updates and cancellations | Partially addressed: existing messages now reach the writer, including same-SEQUENCE REQUESTs and CANCELs. The builders can suppress the message before fan-out receives it; see current finding 2. |
| 4. [P2] One self item cannot carry tokens for multiple aliases | Addressed: the contract now requires one self item per distinct attending address, preserving the one-pair pending format and address-specific merge ([§6.2](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1658)). |

No additional actionable issue was established in the revised key-domain/associated-data
contract or per-address token rule. Earlier resolved findings are not repeated below.

## 1. [P2] Bind the rename sync boundary to the committing transaction

Location: [§5.2, recording the rename boundary](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1289)
and [§5.6, rename write](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1494).

The new rename boundary is the last calendar change ID read before the commit, rather than
the ID assigned atomically to that commit. Asserting the renamed calendar does not fence
unrelated calendar history written after that read.

A concrete interleaving is:

1. Alice starts renaming granted Q from `/other/` to vacant `/family/`. The request reads
   the last calendar change ID B, then pauses before writing.
2. Another Alice session creates private P at `/family/`, creates and deletes
   `medical-visit.ics` at D > B+1, then deletes P.
3. Q's rename resumes. P's writes changed neither Q's archive nor Alice's vault, and the
   destination is free again, so the calendar assertion passes. The new ring headers record
   B even though the rename's actual commit ID R is greater than D.
4. Bob submits numeric sync position S=B+1. It is newer than Q's grant and recorded rename
   boundaries, so the request passes. Q's current `/family/` prefix admits P's tombstone at
   D, exposing a filename from a private calendar Bob was never granted.

The current destination lookup occurs before the rename
([copy_move.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/copy_move.rs:124)),
and creation permits a currently vacant slug
([mkcol.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/mkcol.rs:63)).
[rename_container](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/copy_move.rs:1085)
updates Q through the index builder, which asserts Q's archive
([index.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/common/src/storage/index.rs:349)).
It does not assert the separately read account-log position.

[get_last_change_id](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/store/src/query/log.rs:192)
is an independent log scan. RocksDB allocates the actual per-account change ID inside the
later write transaction
([rocksdb/write.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/store/src/backend/rocksdb/write.rs:174)).
The token parser accepts numeric positions without proof of issuance
([Urn::parse](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/common/uri.rs:197)),
and vanished lookup returns account-wide paths after that position
([Store::vanished](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/store/src/query/log.rs:136)).

Derive the stored boundary from the rename's transaction-assigned change ID, or use an
equivalent atomic fence that forces retry when intervening relevant changes occur. Merely
moving an unfenced read closer to the commit leaves the same window.

Extend the rename regression with a pause after reading the proposed boundary. Create and
delete a private calendar and event at the destination while paused, finish the rename, and
verify that no token admitted afterward can return the private tombstone.

## 2. [P2] Build self-messages when normal scheduling suppresses the writer's message

Location: [§6.2, reuse of built messages and fallback](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1635)
and [planning handoff, extra recipients placed in fan-out](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2703).

Adding the writer to the fan-out recipient map works only when an appropriate message already
exists. The revised handoff explicitly leaves the upstream builders unchanged, and the only
independent fallback is a REQUEST when the writer's own ATTENDEE line changed. It cannot
fulfill the update, deletion, and removal promises in [§6.3](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1812) in these cases:

1. **Sole-attendee edit.** Alice organizes an event whose only attendee is edit-grantee Bob,
   who already has his personal invitation copy. Bob changes only SUMMARY through Alice's
   shared calendar. Under the merged addresses Bob is local, so the builder finds no recipient
   and returns `NothingToSend`. There is no message to augment, and Bob's ATTENDEE line did
   not change, so the fallback does not run. His personal summary stays stale.
2. **Sole-attendee deletion.** Bob deletes that shared event. The CANCEL builder applies the
   same local-recipient filter and returns `NothingToSend`. The new rule has no CANCEL to
   copy and defines only a REQUEST fallback; Bob's personal copy stays active.
3. **Removing the writer.** Bob removes his own ATTENDEE line while Carol remains invited.
   Upstream's cancellation discovery excludes local Bob, so it emits no CANCEL for him.
   Carol's REQUEST no longer contains Bob. Building a fallback REQUEST from that committed
   tree cannot cancel his copy; with no Bob address, the recipient's snapshot rejects it.

The common filter excludes local attendees
([send_update_messages](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/mod.rs:221)).
The organizer update collects recipients at
[organizer.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:77)
and returns `NothingToSend` when none remain
([empty-message result](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:133)).
The cancellation builder likewise requires a nonempty recipient set
([event_cancel.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:47)).
DELETE can still invoke fan-out with an empty message list; that does not supply the missing
CANCEL
([delete_all_za](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/storage.rs:594);
[DELETE fan-out](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/delete.rs:255)).

For removal, cancellation discovery uses only external attendees
([organizer.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:58)).
The drain validates the incoming snapshot before merging
([drain.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:1061)),
and a tree with no local organizer or attendee is rejected
([snapshot.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/snapshot.rs:353)).

Derive writer-directed scheduling from old and new attendance before recipient filtering,
either within the builders or through an independent self-message builder. Require REQUESTs
for relevant updates even without another recipient, and CANCELs derived from prior attendance
for deletion/removal. Preserve the per-address item and token rule.

Extend case 10 with explicit sole-attendee SUMMARY and DELETE cases, plus removal of the
writer while another attendee remains. Check the personal copy after drain and refresh;
covering the sole-attendee decline alone does not exercise these branches.
