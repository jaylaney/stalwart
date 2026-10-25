# PR 58 follow-up review disposition

Review input: seven new threads in review [5645121295](https://github.com/jaylaney/stalwart/pull/58#pullrequestreview-5645121295), against `ee7ebbe4e616fd09a117a801f22a59d60fe91f14`. The preceding disposition remains historical evidence for the earlier 17 threads. The predecessor's GitHub Actions run 40222683663 completed successfully; these new cases require additional regressions.

This pass follows the [correction plan](2026-10-25-calendar-sharing-followup-corrections.md). The seven-comment implementation, focused reviews, whole-branch Astra review/fix and amended-source Rust/harness checks below passed. Shipping-binary client qualification remains pending. Keep the PR in draft.

| Thread | Adjudication | Required correction |
| --- | --- | --- |
| [4372296680](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296680) | Accepted. Original source CAS protects metadata, but a failed multi-commit overwrite MOVE can leave a prefix that a repeat request destroys. | One bounded atomic overwrite-MOVE transaction; oversized requests refuse before mutations. Preserve original conditions and prove safe identical retry. |
| [4372296686](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296686) | Accepted. Metadata-only calendar CAS does not detect a child added after enumeration. A last-minute re-read still has a race. | Fresh membership scan after sampling the account change counter; transactional counter predicate plus calendar/event assertions in the same atomic write. Prove both creator/deleter commit orders. |
| [4372296690](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296690) | Accepted. Encoded-prefix collisions can disclose private tombstone hrefs. Current collision scans miss removed/renamed colliders; versioned token spelling does not prove safe history. | Additive calendar-identity tombstone log; authorize by fresh identity and that identity's current prefix. Relevant identity-free or inconsistent history requires full resync. Preserve existing archive and original path-log formats. |
| [4372296693](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296693) | Accepted. Two ordinary paths still perform unnecessary key-context preparation, and DELETE keeps an unused retry archive. | Gate on existing writer keys at the present call sites; preserve validation order. |
| [4372296697](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296697) | Accepted. CLAUDE omits the new review suite and account family. | Document actual suite order and `sharing-review-*` allocation. |
| [4372296701](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296701) | Accepted. Raw archive restoration skips reversal of an indexed quota delta. | Fresh broken snapshot plus indexed restore; explicit quota-baseline regression before cleanup. |
| [4372296703](https://github.com/jaylaney/stalwart/pull/58#discussion_r4372296703) | Accepted. Introduced prose/diagnostics contain missing spaces and misplaced MOVE guidance. | Narrow human-readable copy pass; place and revise MOVE guidance beside collection-operation behavior. |

The bounded atomic route deliberately replaces the earlier accepted committed-prefix behavior for key-account collection overwrite MOVE. COPY, event MOVE, ordinary-account operations and absent-destination calendar rename keep their existing routes. Conflicts caused by unrelated indexed writes in the same account may conservatively return 409. Identity sidecars add log storage and a scan; legacy relevant history can require a full resync. These are compatibility costs, not claims about native-client retry behavior or cross-store runtime qualification.

Independent source review additionally found that FoundationDB's inherited read-version floor regression and independently chosen SQL replicas can make membership enumeration older than the sampled counter. Ephemeral applies batches without rollback. This route refuses FoundationDB, SQLReadReplica and Ephemeral with 409 before mutation; generic backend redesign is outside this correction. These are source-backed dispositions, with optional-backend runtime qualification unavailable. Direct transactional primary stores retain the route; runtime qualification here is RocksDB.

The sync review also found a retention publication gap: deleting both history streams before publishing the truncation marker could acknowledge an incomplete response. Calendar retention now awaits its cutoff marker before deleting any of the three ranges. Other collections keep their existing ordering. The deterministic old-order regression confirmed a healthy token and an actually removed deletion, then observed the incorrect 207 response. The combined post-fix ZA run passed that control.

## Fresh verification

All commands used the required rustup PATH, stable Rust 1.99.0 and Cargo 1.99.0. Integration commands used RocksDB and `RUST_MIN_STACK=16777216`, sequentially. After the key-erasure fix, the 1,781 compile/runtime inputs were frozen at SHA-256 `8ef2fca8d7b42ac405a0972f50cf8059d6575d048e5ce0b789e04e573e9a964c`, including both new regression modules. Each command verified unchanged inputs before and after execution; documentation/outcome updates are outside this compile/runtime identity. The earlier complete passing net at `486369fc...` remains historical evidence; the results below are the repeated amended-source checks.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo test -p vault -p groupware -p common -p store -p dav` | Passed: vault 45, groupware 149, common 121, store 5, DAV 1; no failures. |
| `cargo test -p http@0.16.26 --features test_mode` | Passed: six tests, including standalone feature handling. |
| Whole `za::za_tests` | Passed: 393.32 seconds; includes MOVE, identity/collision/history, retention, ordinary-owner, rotation and repaired quota controls. |
| Whole `webdav::webdav_tests`, plain then key | Passed; full command durations 22.43 / 22.37 seconds. |
| Pinned Python harness unit tests | Passed: 30 tests. |
| Shipping release build and full client matrix | First build intentionally interrupted for the key-erasure fix; no new artifact qualified yet. |
| Fresh Astra xhigh whole-branch review | Completed at `f28143ec`; its sole Important key-erasure finding was fixed and closed by one scoped re-review, with no new actionable finding. |

The first primary ZA run failed a new ordinary-client fixture that took its token before PUT and DELETE; the ordinary change reader correctly coalesced both operations. The corrected control first proves the client observed the event, then deletes it and checks the legacy raw bytes and matching identity record. Its post-fix full run is the passing result above. Earlier inert/log-ID, decimal-token, property-404 and raw-reader activation fixture failures are retained in local evidence and excluded as meaningful negative controls. The meaningful quota, destructive-MOVE/orphan, legacy disclosure and retention-gap negative controls were separately observed before their fixes.

Scoped reviewers used `gpt-6-sol/high` for the hygiene changes and `gpt-6.1-sol/xhigh` for the MOVE/identity contracts and fix reviews; implementers used Sol 6.1 or lower. All confirmed focused findings are addressed. The conditional JMAP duplicate-parent alias concern has no established ordinary-account producer path and remains unconfirmed; no broad rewrite was made. Optional backend feature/runtime checks, native clients, multi-node qualification, deployment and merge are not established by these local results. Earlier mutation and 3,000-event measurements remain historical evidence at their recorded source revisions.

## Whole-branch review follow-up

Fresh `gpt-6-astra/xhigh` reviewed the full range from `2440b674` to `f28143ec` without inheriting implementer reports or prior findings. It confirmed that rotating a decoded owner ring grows a full `Vec<[u8; 32]>`; a relocating allocation can release its old CK bytes unwiped, beyond the reach of the ring's eventual Drop. The bounded fix allocates an empty Zeroizing replacement, copies within its final capacity, wipes the owned original initialized/spare storage and swaps without allocation. Stored fields, wire format and key order are unchanged. The first release build was stopped before qualification so the final client run uses the corrected artifact.

The safe pre-release observer's negative control detected omission of the explicit wipe (two intended failures). Three focused tests then passed, covering decoded exact-capacity growth, ordered keys/archive compatibility, spare capacity and controlled unwind. The observer is test-only and reads owned initialized storage; it does not probe released or uninitialized memory. The full groupware suite passed 149 tests. Astra checked the exact two Rust-file hashes and zeroize implementation, closed the finding in the five-file scoped diff and found no new actionable defect. The full local net was then repeated at the amended-source identity above.

The review also noted the previously accepted T6-C/T6-M conversion granularity: at most 100 payload snapshots per logical group, with separately acknowledged event commits. The spec now states that existing ruling accurately; conversion behavior was not redesigned. This clarification does not qualify additional backend or native behavior.
