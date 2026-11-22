# Release 6 code map (JMAP calendars and browser sign-in)

Reference inventories for release 6, moved out of `CLAUDE.md` after the release merged (pull
request #90). `CLAUDE.md` keeps the release's invariants and points here. Read this before
touching the release's code, before merging an upstream release, and before adding a test
fixture to the zero-access suite. The design is
`docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md`; the record of the
release is `docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md`.

## Where the code lives

New at release 6 (JMAP calendars and browser sign-in, issue #22; spec
`docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md`, revision 7; plan 17,
`docs/superpowers/plans/2026-11-14-zero-access-17-jmap-calendars.md`, an index; its interface
sheet and group files are in the history at `04145d586`; outcome note
`docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md`). Vault:
`crates/vault/src/tokens.rs` (the token record and its entries, the suffix split,
reconciliation, eviction, refresh classification, the hand-off value), `keys.rs` (the token and
hand-off derivations, labels `za/v1/token/{wrap,tag,next}` and `za/v1/handoff`, and their
associated data), `record.rs` (`za_session_binding`, D19's fingerprint of the credentials),
`lib.rs` (the `tokens` module), `Cargo.toml` and the workspace's `Cargo.lock` (the `encodify`
dependency, the base64 codec of the token secrets). `crates/types/src/field.rs`: `PrincipalField::ZeroAccessTokens = 153`. Common:
`crates/common/src/za/tokens.rs` (the record's storage, issue, refresh, open, forget, the sweep,
the hand-off's store entry, `za_split_bearer`, the one splitter of a Bearer value, and the
refresh precheck), `za/calendar.rs` (the keyless calendar refusal, `ZA_CALENDAR_KEYLESS` in
`za/mod.rs`), `za/test_hooks.rs` (the token hooks), `auth/vault.rs` (field 153 cleared in
`vault_batch_write` when the binding changes), `auth/access_token.rs` (a key account's credential
version is the binding), `auth/oauth/token.rs` (the suffix strip in `decode_access_token`, unit
tests in `token_restored_tests.rs`), `auth/oauth/grant.rs` (`delete_oauth_grant` removes a key
account's entries). Groupware, `crates/groupware/src/calendar/pipeline/` (moved out of `dav`):
`mod.rs` (`ZaScheduling`, `ZaWritten`, `ZaDeleted`), `error.rs` (`ZaPipelineError`,
`za_itip_fails_write`), `admission.rs` (the admission gate, the writer's keys, the roles),
`context.rs` (read and write contexts and views), `uid.rs` (the UID lease, its uniqueness check,
the lost-create fence, the create's second commit), `schedule.rs` (the scheduling status and
assembly, the second run, the delete's send condition), `create.rs`, `update.rs` (with
placement), `placement.rs` (`ZaPlacementPlan`, its contexts and refusals, the event container's
reseal), `delete.rs` (the event delete, the whole-event destroy, the calendar delete); beside it
`calendar/storage.rs` (`CalendarEvent::enqueue_full_at`, the full update without its commit
point), `za_alerts.rs` (default alerts by value, D11), `za_text.rs` (`ZaEventTextIndex`, D12),
`sharing/resolve.rs` (`ZaSyncBoundary::refuses`, D15), `seal/collection.rs` and
`sharing/convert.rs` (the colour in the personal bundle, D14). Dav: `common/za.rs`,
`calendar/{mod,update,delete,copy_move}.rs` and `common/propfind/{za,get}.rs` call the pipeline;
`From<ZaPipelineError> for DavError`. Http: `crates/http/src/auth/za_bearer.rs` (keys for a
Bearer token that carries a secret), `auth/oauth/za_token.rs` (the code's hand-off, the keyed
issue, the keyed refresh), `auth/authenticate.rs` (the Bearer arm and its refusal at a stale
generation), `auth/oauth/auth.rs` (the password step's keys, the hand-off at the code and the
consent token), `auth/oauth/token.rs` (the code exchange, the refresh grant, the untraced granted
answer), `api/vault.rs` (`za_totp` draws a fresh salt through `za_set_password` on every
second-factor change), `request.rs` (the three traced request URLs formatted through
`traced_url`). Http-proto: `crates/http-proto/src/request.rs` (`traced_url`, which masks a `code`
query parameter, applied by `traced_header` to `referer`). Common, again:
`crates/common/src/cache/invalidate.rs` (the `OAuthGrant` arm also evicts the account's resident
keys). Jmap-proto: `types/state.rs` (D29's transient `z` codec and expected-state handling)
and `request/parser.rs` (refusal of a `z` outside the calendar changes methods), with their
`*_restored_tests.rs`. Jmap: `api/za_calendar.rs` (the admission of calendar methods, the method
table `za_calendar_method`, the fixed texts), `calendar_event/za_read.rs` (`ZaEventReader`, the
per-request reader of unsealed event views, used by `CalendarEvent/get`, `/query` and `/set`),
`calendar_event/query/za.rs` (the text leaves resolved in memory), `calendar_event/set/za.rs`
(the key writer of `CalendarEvent/set`), `calendar/set/za.rs` (the key writer of
`Calendar/set`), `changes/za_calendar.rs` (the sync boundary and the original origin of a paged
key-calendar sync), `changes/{get,query}.rs` (admission, the D29 arms, `ChangesPage`),
`changes/za.rs` and `participant_identity/changes.rs` (a `z` refused), `changes/mod.rs` (the
`za_calendar` module), and one branch or call
each in `api/{request,session,mod}.rs`, `calendar_event/{get,query,set,mod}.rs`,
`calendar/{get,set}.rs`, `participant_identity/set.rs`, `principal/get.rs`,
`api/metadata/za.rs` (a unit test). Services: `task_manager/tasks/maintenance.rs` (the token
sweep after the lease sweep). `crates/{dav,jmap}/Cargo.toml`: `test_mode` enables
`groupware/test_mode`. Tests: `tests/src/utils/oauth.rs` (declared in `tests/src/utils/mod.rs`),
`tests/src/za/tokens.rs`,
`tests/src/za/jmap_calendar/{mod,pipeline,events,calendars,query,scheduling,sharing,refusals,oracle}.rs`,
their calls in `tests/src/za/mod.rs`, `tests/src/za/leak.rs` (the token-record rule,
`secret_hits`, `leak::test_jmap_calendars`), `tests/src/za/imip/oracle.rs` (the comparison
helpers the JMAP oracle reuses, made `pub(crate)`: `Kind`, `Tally` with its fields and `same`,
`without_schedule_status`, `mail_key`, `mail_parts` and `raw_header`),
`tests/src/za/dav_seal.rs` (three helpers' visibility), `tests/src/za/sweep.rs` (it calls the
lifted `run_purge_maintenance`), the field-153 exemption in
`tests/src/utils/{cleanup,storage}.rs`, the release's changed assertions in
`tests/src/za/{gating,websocket,app_password,sharing,setup,password}.rs` and
`tests/src/jmap/za_variants.rs`, a comment on the calendar block in `tests/src/jmap/mod.rs`, and
23 cohort entries in `tests/qualification/dispositions.json`. CI: `.github/workflows/test.yml`
(the key-mode JMAP step's comment, and its one rerun from `bea8a39fb`) and
`scripts/tests/ci_steps.py` (that step's `rerun="same"`). The pull request's review (PR #90) added: the
one-transaction flag `BatchBuilder::za_one_transaction` (`crates/store/src/write/batch.rs:1282`;
the field at `write/mod.rs:552`, the no-op branch at the head of `commit_point()`,
`batch.rs:681`) and its size predicate `za_exceeds_one_chunk` (`batch.rs:1292`, against
`MAX_COMMIT_BYTES`, with the reserve its caller passes); `ZaChildFence`
(`crates/groupware/src/calendar/pipeline/delete.rs:520`, with `operation_class` at `:531`, the one
definition of the fence's operation, `reserve` at `:537`, its counted size, `current` at `:555`
and the merge operation `assert_in` at `:586`) and the store predicate in `za_delete_calendar`
(`delete.rs:392`: not on FoundationDB, a read replica or the ephemeral store); from the pull
request's second review, `za_calendar_members` (`delete.rs:479`: a calendar's members from the
snapshot's stored records, used by the build and both comparisons) and, from its third review,
`za_calendar_has_member` (`delete.rs:493`: whether any stored record names the calendar,
stopping at the first; the owner's `onDestroyRemoveEvents` check in
`crates/jmap/src/calendar/set/za.rs` calls it only when the flag is not set), both through the one
definition of membership `za_record_names_calendar` (`delete.rs:502`), and the build's
unwritten-child count (`ZaCalendarBuild` in `crates/groupware/src/calendar/storage.rs:481`,
returned by `delete_with_events_za`, refused at `delete.rs:439`); `ZaPipelineError::CalendarTooLarge` and `ZaBusy::CalendarChanged`
(`pipeline/error.rs:29`, `:54`); `za_event_state` (`crates/jmap/src/calendar_event/za_read.rs:28`,
the event state of a caller who is not the owner, used by `/get`, `/query` and `/set`); the
guarded raise in `za_calendar_changes` (`crates/jmap/src/changes/za_calendar.rs:77-133`);
`za_drain_keys` (`crates/common/src/za/calendar.rs:17`) and `za_call_admitted`
(`crates/jmap/src/api/za_calendar.rs:191`); `Server::za_evict_deleted_grant`
(`crates/common/src/za/tokens.rs:576`); `za_insert_keys_fenced`
(`crates/http/src/auth/authenticate.rs:356`); `za_bearer_entry` (`authenticate.rs:334`, from the
second review: at publication it reads the cache clock, opens the token again and lowers a keyed
entry's deadline to the fresh remaining lifetime less one second, or publishes nothing for an
expired token); and the store unit test
`crates/store/src/write/za_change_id_fence_tests.rs` (declared at the end of `write/mod.rs`). The
reference client's manual checklist is `docs/zero-access/client-jmap.md`.

## Test hooks and helpers

Release 6 adds the token hooks `TOKEN_HANDOFF_WRITE_PAUSE` (D6's hand-off write), `TOKEN_EXCHANGE_PAUSE`, `TOKEN_REFRESH_READ_PAUSE`, `TOKEN_REFRESH_WRITE_PAUSE`, `TOKEN_REFRESH_WRITE_FAIL_ONCE` and `TOKEN_FORGET_FAIL_ONCE` (`common::za::test_hooks`, each keyed by account id); the pull request's review (PR #90) adds `TOKEN_FORGET_PAUSE` (at the head of `za_token_forget_grant`, before `TOKEN_FORGET_FAIL_ONCE` and any read, keyed by the account: a grant's deletion parked in its token cleanup), `CALENDAR_DELETE_FENCE_PAUSE` (in a key calendar's delete, after the second child-list comparison has added the change-id fence and immediately before the commit, keyed by the owner; `pipeline::delete::za_commit_delete`), `UNFENCED_CALENDAR_DELETE` (while it names an account, that account's key calendar delete takes the form a store without the fence's contract keeps: commit points, no unwritten-child refusal, no size refusal, no fence; set it through the `UnfencedCalendarDelete` guard, whose drop clears it), and from the second review `CALENDAR_DELETE_BUILT_PAUSE` (one `test_mode` line in `za_delete_calendar` after the batch is built and before the first comparison, the unwritten-child refusal and the size check, keyed by the owner, one-shot) and `TOKEN_VERSION_CHECK_PAUSE` (at the head of the fork's `za_refuse_unversioned_token`, `crates/common/src/auth/vault.rs:590`, keyed by the account, one-shot: that function runs for an access token's authentication, between its decode and its grant read, and for every validation of a grant-bound token, the refresh grant, introspection and `/api` token validation, so an armed pause parks whichever call for that account comes first), all in `common::za::test_hooks`, and in `crates/store/src/za_test.rs` `COMMIT_BYTES_OVERRIDE` (the byte count above which a named account's key calendar delete is refused, in place of `MAX_COMMIT_BYTES`, read in `BatchBuilder::za_exceeds_one_chunk`; set it through `common::za::test_hooks::CommitBytesOverride`, whose drop clears it); `COMMIT_PAUSE`, `COMMIT_FAIL_ONCE`, the key `UPDATE_COMMIT_PAUSE`, `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_PAUSE` and `DELETE_COMMIT_FAIL_ONCE` of the key paths now fire from `groupware::calendar::pipeline` (`create.rs`, `uid.rs`, `update.rs`, `delete.rs`), on CalDAV and JMAP writes; `DELETE_COMMIT_PAUSE` also still fires from `dav::calendar::copy_move` for a MOVE's key overwrite delete, and `UPDATE_COMMIT_PAUSE` also at the `dav` commits of `calendar/update.rs`' upstream path and of `calendar/proppatch.rs`; `PROPFIND_ADMITTED_PAUSE` stays in `dav` and does not fire on JMAP. Its helpers: `tests/src/utils/oauth.rs` (`za_oauth_client`, `za_sign_in`, `za_authorize`, `za_confirm_consent`, `za_exchange`, `za_refresh`, `za_revoke_token`, the Bearer callers `za_bearer_jmap`, `za_bearer_session`, `za_bearer_dav`, the record readers `token_record`, `raw_token_record`, `set_raw_token_record`, `handoff_exists`, `run_purge_maintenance`, `oauth_expiry`, and from PR #90 `za_keyed_grant`, a keyed grant with the scopes given, its record entry issued and its access token sealed after it with a server built from the current configuration, so an `oauth_expiry` guard reaches the signed expiry; `tokens.rs`' `keyed_grant` calls it with `OAuthScopes::ALL`), `tests/src/za/jmap_calendar/mod.rs` (`pair`, `key_account`, `make_calendar`, `share`, `put_event`, `session_token`, `mark_boundary_pending`, `owner_boundary`, `finish`), and in `tests/src/utils/za.rs` `Keyless::call_using`, `jscalendar_of` and `normalize_jscal`, and from the second review `HookParked::relay` (it arms a later pause point of the same parked request, releases the first and waits up to a given limit for the request to reach the later one) with its guard `RelayedPause` (`#[must_use]`; its drop clears the slot and releases the request, also on a failed assertion); `leak::secret_hits` searches the store for a secret in raw, base64 and hex forms.

## What release 6 costs every account

With these, release 6 costs ordinary accounts one cached account read at a code exchange, at a refresh and at a grant's deletion, one in-memory delete at a consent confirmation that carries no secret, one read of field 153 per account in every maintenance purge run (the token sweep), a pass over the key cache on any account's grant deletion (the `OAuthGrant` arm of `crates/common/src/cache/invalidate.rs` evicts the account's resident keys), and cached classification reads in each calendar method, where spec 10 says one: the classification is read twice in `Calendar/set` and `CalendarEvent/set` (admission, then the handler's head) and three times in a calendar `/changes` (R28-8, R30-3, R22-1 in the plan 17 outcome note); D29 adds state-codec fields and context checks to calendar `/changes` and `/queryChanges` parsing, with no global account lookup and no additional ordinary-account store I/O (spec 10). The pull request's review (PR #90) adds, for every account: one boolean test per `commit_point()` call of every batch (the one-transaction flag); a deleted grant's sessions evicted twice on the deleting node (in place by `za_evict_deleted_grant`, then through the queued invalidation) and on a cluster broadcast twice, so the global revocation epoch advances at two instants and an authentication of any account in flight at either one is served but not cached, a revocation of N grants makes N + 1 cache scans and broadcasts where upstream made one of each, and on a cluster each deletion awaits its broadcast before its cleanup; one more cached account read where a deletion's re-read finds the grant already gone; and, per `OAuthGrant` invalidation, the account's keys removed after the epoch advances (the same work, later). The drain's scope rule and the search pass's condition are reached only with keys; the grantee's event state, the effective privacy, the calendar delete's fence and size refusal and the guarded raise are on key-account paths (spec 10). The second review adds, for an ordinary account, a few boolean tests per listed child of a calendar collection delete, with no read and no write; the member list, the unwritten-child refusal, the every-name removal, the size check's reserve and the keyed entry's lowering are on key-account paths, and the keyed `COPY`'s condition is in the fork's keyed handler (spec 10).

## Merge hotspots

Release 6 added merge hotspots (release 6 spec 11; the plan 17 outcome note's "Merge hotspots"): `crates/http/src/auth/oauth/auth.rs` (`authenticate_login` takes keys; the hand-off at `issue_auth_code`, `request_consent` and `confirm_consent`; `approve_device` drops the keys); `crates/http/src/auth/oauth/token.rs` (the code exchange, the refresh grant, `token_response` marking a granted answer untraced); `crates/common/src/auth/oauth/token.rs` (the suffix strip in `decode_access_token`) and `auth/oauth/grant.rs` (the entry removal in `delete_oauth_grant`); `crates/http/src/auth/authenticate.rs` again (the Bearer arm's keys and its refusal at a stale generation, D23; `crates/common/src/auth/authentication.rs` is not touched); `crates/common/src/auth/vault.rs` (the record's deletion in `vault_batch_write`) and `auth/access_token.rs` (a key account's credential version through `za_session_binding`); `crates/types/src/field.rs` (field 153); `crates/services/src/task_manager/tasks/maintenance.rs` (the token sweep after the lease sweep); `crates/jmap/src/api/request.rs` (admission, the drain hook), `api/session.rs`, `principal/get.rs`, `changes/get.rs` and `changes/query.rs`; one `mod` line and one branch at the head of the two `/set` handlers, `crates/jmap/src/calendar/set.rs` and `calendar_event/set.rs`, whose key writers are child modules (`set/za.rs`) calling upstream's private helpers; replacement points in `calendar/get.rs` and `calendar_event/get.rs`; two points in `calendar_event/query.rs` (before the search store is asked, and where the expanded loop reads the stored content); a refusal in `participant_identity/set.rs`; the `dav` key branches, now calls into `groupware` (`crates/dav/src/calendar/` and `crates/dav/src/common/za.rs`); `crates/dav/Cargo.toml` and `crates/jmap/Cargo.toml` (`test_mode` enables `groupware/test_mode`); and D29's `crates/jmap-proto/src/types/state.rs` (the `z` codec and `ExpectedState` handling), `request/parser.rs` (method context), `crates/jmap/src/changes/{get,query,za}.rs` and `participant_identity/changes.rs`, with `changes/page.rs` left upstream's and its output wrapped only for key calendars. Upstream's `changes/get.rs` body after the object match moved unchanged into the private trait `ChangesPage` (`get.rs:293`, implemented at `:304`) so the generic path and the key-calendar adapter both call it, within D29's exception (R22-4). `crates/groupware/src/calendar/storage.rs` gained `CalendarEvent::enqueue_full_at` (`update_full_at`'s body without its commit point, for one placement batch). Plan 17's final review added: `crates/http-proto/src/request.rs` (`traced_url`, and `traced_header` passing a `referer` through it) and the three `Url = traced_url(..)` sites of `crates/http/src/request.rs` (`:1038`, `:1048`, `:1057`); the `OAuthGrant` arm of `crates/common/src/cache/invalidate.rs` (it also evicts the account's resident keys); `za_totp` in `crates/http/src/api/vault.rs` (the fresh salt through `za_set_password`); and the compile-time equality of `CODE_LEN` and upstream's `DEVICE_CODE_LEN` beside `is_server_code` (`crates/http/src/auth/oauth/token.rs:701`), which stops the build if upstream changes the code length. The pull request's review (PR #90) added: `crates/store/src/write/{batch,mod}.rs` (the `za_one_transaction` field, its setter and `za_exceeds_one_chunk`, a short branch at the head of `commit_point()` before the test-mode split, `batch.rs:681`, and the declaration of `za_change_id_fence_tests.rs` at the end of `mod.rs`); `crates/dav/src/request.rs` again (the drain takes its keys through `za_drain_keys`, `:673`, one expression and an import) and `crates/dav/src/calendar/mod.rs` (the `CalendarTooLarge` arm of `From<ZaPipelineError> for DavError`, 403); `crates/jmap/src/api/request.rs` again (`za_call_admitted` in the drain's condition, `:211`, and the search pre-pass's, `:219`); `crates/common/src/auth/oauth/grant.rs` again (one `za_evict_deleted_grant` call in `delete_oauth_grant`'s success arm before `za_forget_deleted_grant`, `:390`, and the entries' removal on the exit whose re-read finds the grant gone, `:415`); `crates/common/src/cache/invalidate.rs` again (the `OAuthGrant` arm collects the account ids in `revoked_key_accounts`, `:596`, `:707`, and their keys are removed after `grant_revocations.evict` has advanced the epoch, `:755`); `crates/http/src/auth/authenticate.rs` again (upstream's "Cache credentials" block moved above the key lookup, `:211`, so the entry's deadline is fixed at verification; the keyed arm's two inserts guarded by a liveness filter, `:298`; the keys inserted through `za_insert_keys_fenced`); and `crates/jmap/src/calendar_event/{get,query}.rs` (the state through `za_event_state`; in `get.rs` the fork's `ZaEventReader::new` line moved above the response and upstream's `JmapCacheState` import line removed, a new merge point). The pull request's second review added: the keyed `COPY`'s condition in `za_copy_move_stored_event` (`crates/dav/src/calendar/copy_move.rs:2090`, `source != target || !is_move`, so a `COPY` inside one calendar runs the UID check, as upstream's handler does); the result struct of `delete_with_events_za` (`ZaCalendarBuild`, `crates/groupware/src/calendar/storage.rs:481`; upstream's `delete_with_events` drops it) and the `all_names` argument of `delete_from_calendar_za` (`:918`, true only for a key account's collection delete, which removes every name under the calendar), both in upstream's file; and `za_bearer_entry` (`crates/http/src/auth/authenticate.rs:334`) beside the keyed arm, called at `:294` for keys opened from a Bearer token.

## Silent dependencies

Release 6 silent dependencies to re-read on upstream changes (spec 11). The refresh steps follow `StoredOAuthGrant::rotation` (two accepted generations, a deterministic refresh token on retry, deletion on replay), and 4.5 relies on upstream's retry returning without a registry write and on no lock held from the rotation to the record's write; the reconciliation reads `OAuthGrant.generation` as upstream's rotation means it. The suffix relies on upstream's token alphabet excluding `~`, on the code and consent token being 40 alphanumeric characters used as the store key, and on `TOKEN_HEADER` (`crates/common/src/auth/oauth/token.rs:23`) opening every token the server issues and no other issuer's: `za_split_bearer` (`crates/common/src/za/tokens.rs:48`) splits only values that begin with it (D26). The hand-off relies on the password step issuing the code or consent token in the request that verified the password. D19 relies on every vault write passing through `vault_batch_write` and on the credential version being computed in one place; the four inputs of the binding must stay the only facts of the vault record that decide a login, and every use of the rule goes through `za_session_binding`: `crates/common/src/auth/access_token.rs:185` (the credential version), `crates/common/src/auth/vault.rs:233` (the record cleared when the binding changes), `crates/common/src/za/tokens.rs:242` (`za_handoff_put`), `:338` (`za_token_issue`), `:401` (`za_token_refresh`), `:487` (`za_token_open_access`) and `crates/http/src/auth/oauth/auth.rs:913` (`confirm_consent`). 4.6 relies on the HTTP layer being the only reader of the token record (`authenticate_with_keys` is shared with the IMAP and POP3 logins). The JMAP adapter relies on upstream's `/set` handlers producing the final tree before any write and on `export_icalendar` being the only conversion; no `preferences_mut` call on a key account's event may be reached (upstream's patcher makes one for `useDefaultAlerts`, which the key branch takes out first), and the key branch skips the per-user split and never reaches the `resolve(.., false)` sites. The key writer of `CalendarEvent/set` (`za_calendar_event_set`, `calendar_event/set/za.rs`) repeats the head and the update grouping of upstream's handler and calls upstream's checks in upstream's order (D17); `za_calendar_set` (`calendar/set/za.rs`) repeats the head of `calendar/set.rs`; a check or step upstream adds, moves or merges must be given a place. Both key writers also copy checks that are inline in upstream's handlers, with nothing to call, so a merge does not flag a change to them (plan 17's final review FC1-10, FC2-11). In `CalendarEvent/set`'s writer: the calendar-existence check and its text, the add and remove ACL checks and their texts, the calendar limit, the UID generation loop, the created and updated stamps, the user-data-only rule and its SEQUENCE increment, the UID-unchanged check with its three texts, the restricted grantee's ACL choice, the size and quota checks, the destroy's remove check (restructured), and the "modified" and recipients texts. In `Calendar/set`'s writer: a create's quota and shared-account refusals, an update's `notFound` ACL test, the destroy's ACL and child-privacy tests, the default-calendar rule, the server-set block and three description strings. Fork insertions in upstream files larger than one call or one short branch (FA-9, FC2-12): `issue_login_entry` in `crates/http/src/auth/oauth/auth.rs`, which merges the bodies of upstream's `request_consent` and `issue_auth_code`, and the inline block in `confirm_consent`; the precheck match inline in `refresh_token_grant` (`crates/http/src/auth/oauth/token.rs`), and `is_server_code`, which repeats the code-shape rule of upstream's `take_pending_consent`; the grantee filter and the bundle-opening block of `crates/jmap/src/calendar/get.rs`; the view block inside `crates/jmap/src/calendar_event/get.rs`; the two blocks of `calendar_event/query.rs`; `changes/query.rs`'s key branch, which repeats upstream's `assert_has_access`; the rewritten calendar arms of `crates/jmap/src/api/request.rs` (each calls `za_serve_calendar_read` or `za_admit_calendar_method`); and `ChangesPage` (above). `za_itip_fails_write` (`crates/groupware/src/calendar/pipeline/error.rs:90`) follows `ItipPrecondition::failed_precondition` and names every `ItipError` variant, so a new one stops the build. D12's expanded form relies on `InstanceTextFilter` covering every text condition of `CalendarEventFilter`; its unexpanded form relies on `index_document` (`crates/groupware/src/calendar/index.rs`) being the only builder of a calendar search document and on release 7's content part (`crates/store/src/za_search/content.rs`) using the store's indexer and planner (`crates/store/src/search/leaf.rs`); the two-account query case is the guard. The query branch relies on no key account's event carrying `EVENT_HIDE_ATTENDEES`, which the key writer refuses; upstream's check for such events sends an attendee condition to the search store. D11's alarm text copies two private rules of upstream's `crates/groupware/src/calendar/alerts.rs`: the email recipient of `DefaultAlertsResolver::recipient` (`alerts.rs:472-489`, copied at `za_alerts.rs:50-60`) and the `DefaultAlertsView::ICalendar` description, the component's SUMMARY or "Reminder" (`alerts.rs:714`, `DEFAULT_ALARM_DESCRIPTION` at `:33`; copied at `za_alerts.rs:23` and `:104-110`, R29-7). D14 relies on the personal bundle's rkyv layout keeping its colour field; a binary built before release 6 refuses a coloured bundle, so a binary rollback below release 6 or a mixed-version cluster leaves a coloured shared calendar unreadable (R27-1). D29 relies on the versioned origin codec, on the method and expected-state context checks in the parser, on the origin staying unchanged while upstream's range and count advance (`changes/page.rs`) and across early returns, on the key owner's legacy `r` chain staying `r` until it completes, on `/queryChanges` forwarding the state unchanged, and on every `State` consumer keeping `z` within its key-calendar scope; a change to the codec, request-slot parsing, expected-state checks, early returns or pagination needs Task 22's collision, legacy-state and context-error cases and Task 26's `query_changes_paged_origin`. `tests/src/za/jmap_calendar/refusals.rs` pins the 404 detail of `crates/jmap-proto/src/error/request.rs:238` (R37-16). The oracle (`tests/src/za/jmap_calendar/oracle.rs`) compares through upstream's JSCalendar converter in both directions, so a converter change changes what it compares. The calendar variants of `tests/src/jmap/za_variants.rs` mirror upstream's bodies line for line (R44), so an upstream change to `tests/src/jmap/calendar/{event,identity,acl}.rs` or `tests/src/jmap/principal/availability.rs` needs a re-read of its variant. Spec 1.3's facts about the reference client are facts about one release of it. The key calendar delete's fence (PR #90, spec 5.3) relies on the store allocating a batch's change ids inside its transaction before its merge functions run, and on an error from a merge function aborting the whole transaction: pinned for RocksDB and SQLite by `crates/store/src/write/za_change_id_fence_tests.rs`, held by reading for PostgreSQL and MySQL; the ephemeral store keeps earlier writes on such an error, and the test asserts that too. The fence mirrors release 4's `za_move_overwrite_container` (`crates/dav/src/calendar/copy_move.rs:2338`, the same three stores excluded at `:2366-2368`). The one transaction relies on every commit point of `delete_with_events_za` going through `commit_point()`; an explicit `add_commit_point()` still splits. The guarded raise in `changes/za_calendar.rs` reads the log with `Store::changes`, the reader upstream's page takes its item state from (`item_change_id`, `crates/jmap/src/changes/page.rs:315`), so a change to either needs a re-read of the other. From the second review: `za_bearer_entry` takes the cache clock's now from upstream's `HttpAuthEntries::expires_in(0)` (`crates/common/src/cache/http_auth.rs`), which upstream gives no `now()` of its own; if upstream changed that rule the lowering would fail closed (no keyed Bearer entry published) and `keyed_entry_ends_before_signed_expiry` would show it. `za_record_names_calendar` reads each record's names, and `za_calendar_members` and `za_calendar_has_member` iterate the snapshot's resource records through it (`resources.iter_run(false)`), so a change to the calendar snapshot's record layout needs a re-read of all three.

## Test fixtures

Release 6 reserves the `token-*` and `jmapcal-*` families, all at example.com with password
`STRONG`. A `jmap_calendar` module's accounts are `jmapcal-<module>` and `jmapcal-<module>-2`, both
key accounts (`pair` in `tests/src/za/jmap_calendar/mod.rs`); a third key account takes a role
suffix (`-reader`, `-noperm`; `key_account`); an ordinary account takes `-plain`; outside parties
are sink addresses `jmapcal-<module>-<role>@remote.test`; OAuth client ids are `za-token-*` and
`za-jmapcal-*`; deliberate authentication failures use `X-Forwarded-For` addresses in
`10.0.60.x` (`10.0.60.20` for the D27 ban case, which lowers the ban rate through
`set_auth_ban_rate`, `pub(crate)` in `tests/src/za/setup.rs`). Allocations, by module: `tokens` creates the key accounts `token-owner`, `token-mfa`,
`token-cap`, `token-consent`, `token-race` and `token-factor` (`second_factor_change_ends_sessions`), the ordinary `token-plain` (destroyed by the
module), and the key accounts `token-removed` (`classification_failure_removes_the_handoff`) and
`token-gone` (`account_destruction_removes_the_record`), each destroyed by its case; its OAuth
clients are `za-token-app` (with an `Application`) and `za-token-consent`. `jmap_calendar::pipeline`
creates `jmapcal-pipeline` and `-2` (outside `jmapcal-pipeline-att@remote.test`); `events`
`jmapcal-events` and `-2` (outside `jmapcal-events-out@remote.test`); `calendars`
`jmapcal-calendars` and `-2` (outside `jmapcal-calendars-out@remote.test`); `query`
`jmapcal-query` and `-2` and the ordinary `jmapcal-query-plain` (destroyed by the module; outside
`jmapcal-query-arthur@remote.test` and `jmapcal-query-olive@remote.test`); `scheduling`
`jmapcal-scheduling`, `-2` and `jmapcal-scheduling-noperm` (key, `CalendarSchedulingSend`
disabled), outside `jmapcal-scheduling-out@remote.test`, the OAuth client `za-jmapcal-app` (with an
`Application`), and
`jmapcal-scheduling-nobody@example.com`, a local address with no entry, not an account; `sharing`
`jmapcal-sharing`, `-2` and `jmapcal-sharing-reader`, the ordinary `jmapcal-sharing-plain` and
`jmapcal-sharing-plain-from` (destroyed by the module; the second is outside the naming rule,
R22-8), outside `jmapcal-sharing-org@remote.test`, and the calendar slugs `jmapcal-two-kept`,
`jmapcal-two-revoked` and `jmapcal-unshared` (`revoked_and_unshared_calendars_hidden`); `refusals` `jmapcal-refusals` (alias
`jmapcal-refusals-alias@example.com`), `jmapcal-refusals-2` and `jmapcal-refusals-impersonate`
(key, holds `Impersonate`; Task 37's fix round); `oracle` the key accounts `jmapcal-oracle-cd`,
`jmapcal-oracle-jm`, `jmapcal-oracle-cd-inv` and `jmapcal-oracle-jm-inv`, the ordinary
`jmapcal-oracle-plain` (destroyed by the module), outside `jmapcal-oracle-cd-out@remote.test` and
`jmapcal-oracle-jm-out@remote.test`; `leak::test_jmap_calendars` `jmapcal-leak` and
`jmapcal-leak-2`, the outside canary `jmapcal-leak-addr-canary@remote.test`, the OAuth client
`za-jmapcal-leak` and canary strings `jmapleak-*`. The OAuth `state` values `za-token-sign-in`
and `za-token-sign-in-race` are not client ids. Calendar slugs and UIDs inside a module use
`jmapcal-<name>` without creating accounts; check the module before reusing one. The cases the
pull request's review (PR #90) added create no account and no OAuth client; by module:
`jmap_calendar::calendars` the calendar slugs `jmapcal-destroy-whole`, `jmapcal-destroy-fence`,
`jmapcal-destroy-crossed` and `jmapcal-destroy-large`, with the UIDs `jmapcal-cal-whole-1` to
`-3`, `jmapcal-cal-fence-invited`, `jmapcal-cal-fence-late`, `jmapcal-cal-crossed`,
`jmapcal-cal-crossing`, `jmapcal-cal-crossing-dav` and `jmapcal-cal-large`; `pipeline` the slugs
`pipe-fence`, `pipe-late` and `pipe-late-2`, with the UIDs `jmapcal-pipe-fence-late`,
`jmapcal-pipe-late-destroy` and `jmapcal-pipe-late-place`; `events` the UID
`jmapcal-events-rec-links`; `scheduling` the UID `jmapcal-sched-scope` and three keyed grants of
`jmapcal-scheduling-2` through `za_keyed_grant`, scoped to mail, contacts and calendars;
`sharing` the slugs `jmapcal-sharing-granted` and `jmapcal-sharing-unshared-later`, with the UIDs
`jmapcal-sharing-quiet`, `jmapcal-sharing-a1`, `-a2`, `-b1`, `-x0`, `-y1`, `-y2`, `-veiled` and
`-open`; `imip::outbound` the slug `imip-coll-chunked` with the UIDs `imip-out-k3` and
`imip-out-k4` (`chunked_collection_delete_unfenced`); `mail::index`'s `case_scope_excludes_mail`
runs on `keymail48` with two keyed grants, scoped to calendars and to mail; `tokens`' three new
cases run on `token-owner`, `deleted_grant_evicted_before_cleanup` through `za-token-app`. The
second review's waves create no account and no OAuth client either. Wave C: `tokens`'
`keyed_entry_ends_before_signed_expiry` and `expired_second_decode_publishes_nothing` run on
`token-owner`; `jmap_calendar::calendars` adds the calendar slugs `jmapcal-destroy-moved-x`,
`-moved-a`, `-moved-b`, `jmapcal-copy-same`, `jmapcal-destroy-twice`, `jmapcal-destroy-twice-dav`,
`jmapcal-destroy-twice-kept`, `jmapcal-destroy-shadowed`, `jmapcal-destroy-shadowed-dav` and
`jmapcal-destroy-hidden`, the UIDs `jmapcal-cal-moved-invite`, `jmapcal-cal-moved`,
`jmapcal-cal-copy-one`, `-copy-two`, `jmapcal-cal-twice`, `-twice-dav`, `-twice-kept`,
`jmapcal-cal-shadow-1`, `-2`, `jmapcal-cal-shadow-dav-1`, `-2`, `jmapcal-cal-hidden-1` and `-2`,
and the names `second.ics`, `kept.ics`, `shadowed.ics` and `hidden.ics`. Wave D (coverage):
`events` the UIDs `jmapcal-events-many-cals`, `-many-parts`, `-many-kws`, `-metadata-cap`,
`-too-large`, `-limits`, `-quota`, `-quota-count`, `-quota-disk`, `-quota-after`, `-privacy`,
`-failed-1`, `-failed-2` and `-unreadable`, an event titled `jmapcal generated uid` (its UID is
generated), the keywords `jmapcal-kw` and `jmapcal-kw-{i}`, the update id
`jmapcal-not-an-id!` (also in `calendars`), the outside addresses `jmapcal-events-out-1` to
`-21@remote.test` (refused before any mail) and a third series under the existing
`jmapcal-events-rec` UID; `calendars` the calendar names `jmapcal-writer-a` to `-g`, the slug
`jmapcal-grantee-viewed`, the default-alert ids `jmapcal-t1` and `jmapcal-t2` and the metadata
note `jmapcal canary`; `sharing` the
slug `jmapcal-sharing-viewed`, the UIDs `jmapcal-sharing-classified`, `-confidential`, `-public`,
`-mine` and `-in-viewed`, the resource `jmapcal-sharing-copy.ics` and the forwarded address
`10.0.60.12`; `tokens` the hand-off keys `p`x40 and `q`x40 (never stored); `pinning` (outside
the release 6 family, which owns the `10.0.60.x` range) the UIDs `pin-read-failure` and
`pin-lookup-failure` and the forwarded address `10.0.60.11`; and `plain@example.com` is used with
its existing password against the contacts and share endpoints.
