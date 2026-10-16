# Independent Python CalDAV client CI

The `caldav-client` job follows `zero-access`. It builds the non-enterprise RocksDB product without
`test_mode` and runs python-caldav's unmodified calendar and scheduling test classes, then this
fork's stricter client flows, sequentially for ordinary and key accounts.

The third-party suite is pinned to python-caldav **v3.3.1**, commit
`111ae14b875f40d100d5259820f10210f00e20f9`. `tests/caldav_client/requirements.txt` pins its runtime,
build, and external-test dependencies for Python 3.12; embedded servers, Docker services, and the
separately released server-compatibility checker are deliberately not installed. The upstream source
is
[python-caldav](https://github.com/python-caldav/caldav/tree/111ae14b875f40d100d5259820f10210f00e20f9).

Run from the repository root (stable Rust and Python 3.12):

```sh
cargo build --release -p stalwart --no-default-features --features rocks
git clone --no-checkout https://github.com/python-caldav/caldav.git /tmp/python-caldav
git -C /tmp/python-caldav checkout --detach 111ae14b875f40d100d5259820f10210f00e20f9
python3.12 -m venv /tmp/caldav-venv
/tmp/caldav-venv/bin/python -m pip install -r tests/caldav_client/requirements.txt
/tmp/caldav-venv/bin/python -m pip install --no-deps --no-build-isolation /tmp/python-caldav
/tmp/caldav-venv/bin/python -m unittest discover -s tests/caldav_client -p test_runner.py -v
/tmp/caldav-venv/bin/python tests/caldav_client/runner.py \
  --binary target/release/stalwart --caldav-source /tmp/python-caldav
```

Use `--mode plain` or `--mode key` to narrow a diagnostic run. The normal CI command runs both. The
runner attempts both modes even if one fails; each has a fresh server and store. The runner never
uses `.run/` or an existing server: it creates a private temporary RocksDB store, chooses a high
port, generates administrator and account credentials, configures the server through public registry
JMAP, stops/reaps recovery mode, and starts normal mode. Recovery mode briefly binds `[::]` before
the normal loopback-only listener starts. Background application downloads are disabled during
bootstrap to keep this CalDAV fixture self-contained. Key accounts use the public vault setup-token,
setup, and password-rotation endpoints. Credentials, recovery material, and the temporary store are
removed on exit. Logs are removed by default. Optional `--log-dir /absolute/new-directory` retains
only per-mode sanitized server and pytest logs in a new private directory; existing directories,
symlink paths and parent traversal are refused. Known generated passwords, setup tokens, recovery
keys and directly derived credential representations are redacted before log retention or console
output. Both server modes use warning-level tracing. The runner clears `PYTHON_CALDAV_COMMDUMP` and
`PYTHON_CALDAV_DEBUGMODE` so caller settings cannot write request dumps outside the fixture. CI
uploads only the four allowlisted sanitized logs on failure, with three-day retention; fixture
stores, users/config files and vault responses are never exported.

The upstream documented external-server JSON configuration is generated in a private suite
materialized with `git archive` from the pinned commit. Modified tracked files and untracked local
configuration in the source checkout never enter the executed suite.
`PYTHON_CALDAV_TEST_EMBEDDED=0`, `PYTHON_CALDAV_TEST_DOCKER=0`, and `PYTHON_CALDAV_TEST_EXTERNAL=1`
select only the disposable server. Explicit pytest class selectors and a checked-in 72-case manifest
reject missing classes, individual tests, parameter cases, strict flows, reports, or
filtered/deselected runs. The runner clears caller pytest options and third-party plugin
autoloading.

`tests/caldav_client/compatibility.json` is the reviewed exception policy. Every feature change and
expected skip/xfail needs an explanation. The per-mode policies intentionally duplicate the same
records for now, keeping each account mode independently reviewable. The runner rejects unknown
feature names; the plugin fails unlisted skips, xfails, xpasses, missing policy tests, and stale
exceptions that start passing. It does not inherit the upstream `stalwart` profile's broad flags.
Upstream's `testCheckCompatibility` intentionally skips because the optional server tester's
released feature matrix does not cover this pin (see upstream `pyproject.toml`).

Key-account sharing, cross-user free/busy and external iMIP are outside release 2's accepted
contract; local invitation, reply, cancellation and three-account delegation are covered.
Key-account delegation follows the attendee-token visibility rules in invitations design section
3.3. The strict tests assert read-back event data, client-side recurring exceptions and adjacent
instances, attendee `PARTSTAT` after authenticated drains, and organizer-mediated delegation.
Upstream's automatic scheduling invitation test returns early once an event appears, so these
additional assertions are necessary. The 15-second polling limit intentionally stays below the
drain's 30-second retry deferral, so a failed initial drain cannot hide behind a later retry.
Server-side all-day and exception expansion remain known gaps tracked by the precise upstream
expected failures; the mandatory client-side recurrence flow does not cover or compensate for those
gaps. vdirsyncer and native/physical client acceptance remain separate work.

Validation commands and results are recorded in the pull request description; a local run does not
claim that GitHub Actions has executed.

## Compatibility findings

The policy below is based on the pinned source and unmasked diagnostics against the fresh worktree
product. Both modes reproduced six failures, 64 passes, and two skips before the policy. The policy
applies identically in both account modes. These are compatibility limits, not successful product
assertions or native-client acceptance.

| Test/feature | Recorded behavior and boundary | Evidence at the client pin |
| --- | --- | --- |
| `create-calendar.auto=true` | Querying a nonexistent calendar can auto-create it; the test's optional NotFound expectation does not apply. This is an upstream server behavior hint. | `testCreateDeleteCalendar`, line 2049, and upstream `stalwart` hint; diagnostic did not raise NotFound. |
| `get-current-user-principal.has-calendar=true` | Exercise the default-calendar assertion instead of accepting the library's default skip. The fixture has a readable default calendar. | `testGetDefaultCalendar`; strict invitations use the default calendar. |
| `scheduling.auto-schedule=true` | A local invitation creates an attendee event after authenticated polling. Mandatory strict flows assert acceptance, cancellation, and delegation beyond the upstream early return. | `_TestSchedulingBase.testInviteAndRespond`; accepted invitations design sections 6 and 6.6. |
| `scheduling.mailbox.inbox-delivery=true` | Local iTIP request history is readable in the inbox after import. This says nothing about external iMIP. | Strict flow requires matching UID and METHOD:REQUEST history; design section 6.6. |
| `testCheckCompatibility` (skip) | The optional separately released server checker is intentionally omitted upstream because its probes do not match the pinned feature matrix. | `pyproject.toml` test dependencies and `test_caldav.py:1634-1645`. |
| `testPrincipals` (strict xfail) | The upstream test calls deprecated `DAVClient.principals()`, which emits DeprecationWarning under upstream's warnings-as-errors configuration before principal search executes. This is an upstream client-test defect. | `test_caldav.py:2017`, `davclient.py:415`, `pyproject.toml` warning policy. |
| `testAlarm` (strict xfail) | Creating the event succeeds; the server returns zero events for the 07:40–07:55 alarm-range query whose alarm is 07:45, where the test expects one. Alarm-range search remains an inherited gap. | `test_caldav.py:2279`; upstream Stalwart hint disables `search.time-range.alarm`. |
| `testTodoDatesearch` (strict xfail) | The inherited VTODO date query returns four resources where the first count assertion expects five. No broad recurring-todo or DTSTART-less-task flags are imported to hide it. | `test_caldav.py:3621`; upstream Stalwart hints mark recurring todos and DTSTART-less date search fragile. |
| `testRecurringDateSearch` (strict xfail) | Client expansion passes; explicit server expansion retains the original all-day DTSTART year instead of the searched anniversary year. | `test_caldav.py:4391`; diagnostic expected one `DTSTART;VALUE=DATE:2026`, found zero. |
| `testRecurringDateWithExceptionSearch` (strict xfail) | Client exception expansion produces the two correct instances; server expansion returns three including the overridden original. | `test_caldav.py:4469`; upstream Stalwart hint records `search.recurrences.expanded.exception` fragile. |

Every xfail is constrained to the observed call-phase exception type and deepest source file/line;
setup and teardown failures always fail. An expected call failure followed by a teardown failure
still fails. Xfails are strict: an unexpected pass fails CI and requires revisiting the record.
Other failures remain failures. Mandatory strict flows cannot be listed in the exception policy, and
the complete test/report manifest remains required. In particular, recurring exception client
behavior, key-account Carol-before-Alice absence, and Bob's unchanged delegate status/inbox before
an organizer update have no waivers.
