# Calendar sharing client coverage design

The user approved extending the existing Python CalDAV harness and opening a draft PR.
This is additional client interoperability coverage for release4, with no new product behavior.

## Approved scope

Add five mandatory key-account flows against the normal shipping binary:

1. View/edit grants: own-credential discovery/read, denied view writes, successful edit CRUD
   with owner readback, and absence of an unrelated owner-private calendar.
2. Properties/ETags: principal-specific displayname, shared Apple colour, a competing
   completed edit followed by stale If-Match412/B preservation, plus fresh/unconditional controls.
3. Revoke/regrant/rename/sync: fresh denied access after revoke, successful regrant, changed
   calendar href after owner rename, old-token refusal and fresh sync with no private sibling data.
4. Delegated invitation/update/cancel: Bob writes Alice's shared calendar, Carol receives
   Alice ORGANIZER with Bob SENT-BY, then observes the update and cancellation.
5. Replies: Bob replies from his own invitation copy and from the shared source; a sole-
   attendee decline without SCHEDULE-FORCE-SEND reaches Bob's distinct own copy.

Public contacts lookup/pin and sharing endpoints establish grants. CalDAV behavior is exercised
through the pinned python-caldav client, using its transport for DAV operations the high-level
API does not expose. No direct store mutation, owner-session substitution or test_mode hook is used.

## Approach

Extend the pinned client harness rather than adding a second framework or duplicating Rust
internal race tests. That approach was proposed and accepted by the user. The existing Rust
pause regression retains the precise I1 interleaving proof; client tests verify public contracts.
Native Apple Calendar/iOS/Thunderbird observations remain separate pending gates.

Primary implementation choices: put the five flows in focused test_sharing.py, keep all72
existing cases in both modes, and use explicit per-mode collection manifests:72 plain/77 key.
The runner selects the additional file only for key mode. All collected cases are mandatory;
no ordinary early-return placeholders, sharing skip/xfail, or compatibility-policy broadening.
The existing strict flows, upstream pin, dependency pins and compatibility exceptions stay intact.

## Invariants and evidence

- Calendar-specific polling distinguishes Bob's own copy from the shared same-UID source.
- Every operation uses its actual principal's credentials. Use public primary-password APIs
  for pin/grant/revoke; preserve credential redaction and private fixture cleanup.
- Keep the existing15-second polling bound below the30-second drain retry deferral.
- Client runs use separate disposable stores/processes in plain/key order and never .run
  or a pre-existing server. Missing/extra cases, filtering and incomplete reports remain failures.
- Sequential stale-ETag coverage is not claimed as deterministic concurrency reproduction.
- Existing policy failures remain failures; diagnosis precedes any retry or product change.

Validation: meaningful harness-unit RED/GREEN, all harness tests, then full pinned upstream+
strict client suites on both account modes using the verified shipping binary. Retain exact
commands, counts, binary/source/pin identities and sanitized diagnostics; independently review
all coverage changes before committing/pushing them to the draft PR.
