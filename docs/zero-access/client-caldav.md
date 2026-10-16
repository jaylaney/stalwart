# Independent Python CalDAV client CI

The `caldav-client` job follows `zero-access`. It builds the non-enterprise RocksDB product
without `test_mode` and runs python-caldav's unmodified calendar and scheduling test classes,
then this fork's stricter client flows, sequentially for ordinary and key accounts.

The third-party suite is pinned to python-caldav **v3.3.1**, commit
`111ae14b875f40d100d5259820f10210f00e20f9`. `tests/caldav_client/requirements.txt` pins its
runtime, build, and external-test dependencies for Python 3.12; embedded servers, Docker services,
and the separately released server-compatibility checker are deliberately not installed.
The upstream source is [python-caldav](https://github.com/python-caldav/caldav/tree/111ae14b875f40d100d5259820f10210f00e20f9).

Run from the repository root (stable Rust and Python 3.12):

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH" # this Mac only
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

Use `--mode plain` or `--mode key` to narrow a diagnostic run. The normal CI command runs both.
The runner never uses `.run/` or an existing server: it creates a private temporary RocksDB
store, chooses a high port, generates administrator and account credentials, configures the
server through public registry JMAP, stops/reaps recovery mode, and starts normal mode.
Key accounts use the public vault setup-token, setup, and password-rotation endpoints.
Credentials, recovery material, logs, and the temporary store are removed on exit; no fixture
store or secret config is uploaded as a workflow artifact.

The upstream documented external-server JSON configuration is generated in a private suite
materialized with `git archive` from the pinned commit. Modified tracked files and untracked
local configuration in the source checkout never enter the executed suite. `PYTHON_CALDAV_TEST_EMBEDDED=0`, `PYTHON_CALDAV_TEST_DOCKER=0`, and
`PYTHON_CALDAV_TEST_EXTERNAL=1` select only the disposable server. Explicit pytest class
selectors and a checked-in 72-case manifest reject missing classes, individual tests, parameter
cases, strict flows, reports, or filtered/deselected runs. The runner clears caller pytest options
and third-party plugin autoloading.

`tests/caldav_client/compatibility.json` is the reviewed exception policy. Every feature
change and expected skip/xfail needs an explanation. The runner rejects unknown feature names;
the plugin fails unlisted skips, xfails, xpasses, missing policy tests, and stale exceptions that
start passing. It does not inherit the upstream `stalwart` profile's broad flags. Upstream's
`testCheckCompatibility` intentionally skips because the optional server tester's released
feature matrix does not cover this pin (see upstream `pyproject.toml`).

Key-account sharing, cross-user free/busy and external iMIP are outside release 2's accepted
contract; local invitation, reply, cancellation and three-account delegation are covered. The
strict tests assert read-back event data, recurring exceptions and adjacent instances, attendee
`PARTSTAT` after authenticated drains, and organizer-mediated delegation. Upstream's automatic
scheduling invitation test returns early once an event appears, so these additional assertions
are necessary. vdirsyncer and native/physical client acceptance remain separate work.

Local validation results and any compatibility findings are recorded with the implementation
report; a local run does not claim that GitHub Actions has executed.

## Compatibility findings

The policy below is based on the pinned source and unmasked diagnostics against the fresh
worktree product. Both modes reproduced six failures, 64 passes, and two skips before the
policy. The policy applies identically in both account modes.
These are compatibility limits, not successful product assertions or native-client acceptance.

| Test/feature | Recorded behavior and boundary | Evidence at the client pin |
| --- | --- | --- |
| `create-calendar.auto=true` | Querying a nonexistent calendar can auto-create it; the test's optional NotFound expectation does not apply. This is an upstream server behavior hint. | `testCreateDeleteCalendar`, line 2049, and upstream `stalwart` hint; diagnostic did not raise NotFound. |
| `get-current-user-principal.has-calendar=true` | Exercise the default-calendar assertion instead of accepting the library's default skip. The fixture has a readable default calendar. | `testGetDefaultCalendar`; strict invitations use the default calendar. |
| `scheduling.auto-schedule=true` | A local invitation creates an attendee event after authenticated polling. Mandatory strict flows assert acceptance, cancellation, and delegation beyond the upstream early return. | `_TestSchedulingBase.testInviteAndRespond`; accepted invitations design sections 6 and 7. |
| `scheduling.mailbox.inbox-delivery=true` | Local iTIP request history is readable in the inbox after import. This says nothing about external iMIP. | Strict flow requires matching UID and METHOD:REQUEST history; design section 6.6. |
| `testCheckCompatibility` (skip) | The optional separately released server checker is intentionally omitted upstream because its probes do not match the pinned feature matrix. | `pyproject.toml` test dependencies and `test_caldav.py:1634-1645`. |
| `testPrincipals` (strict xfail) | The upstream test calls deprecated `DAVClient.principals()`, which emits DeprecationWarning under upstream's warnings-as-errors configuration before principal search executes. This is an upstream client-test defect. | `test_caldav.py:2017`, `davclient.py:415`, `pyproject.toml` warning policy. |
| `testAlarm` (strict xfail) | Creating the event succeeds; the server returns zero events for the 07:40–07:55 alarm-range query whose alarm is 07:45, where the test expects one. Alarm-range search remains an inherited gap. | `test_caldav.py:2279`; upstream Stalwart hint disables `search.time-range.alarm`. |
| `testTodoDatesearch` (strict xfail) | The inherited VTODO date query returns four resources where the first count assertion expects five. No broad recurring-todo or DTSTART-less-task flags are imported to hide it. | `test_caldav.py:3621`; upstream Stalwart hints mark recurring todos and DTSTART-less date search fragile. |
| `testRecurringDateSearch` (strict xfail) | Client expansion passes; explicit server expansion retains the original all-day DTSTART year instead of the searched anniversary year. | `test_caldav.py:4391`; diagnostic expected one `DTSTART;VALUE=DATE:2026`, found zero. |
| `testRecurringDateWithExceptionSearch` (strict xfail) | Client exception expansion produces the two correct instances; server expansion returns three including the overridden original. | `test_caldav.py:4469`; upstream Stalwart hint records `search.recurrences.expanded.exception` fragile. |

Every xfail is constrained to the observed call-phase exception type and deepest source
file/line; setup and teardown failures always fail. An expected call failure followed by a
teardown failure still fails. Xfails are strict: an unexpected pass
fails CI and requires revisiting the record. Other failures remain failures. Mandatory strict
flows cannot be listed in the exception policy, and the complete test/report manifest remains
required. In particular, recurring exception client behavior, key-account Carol-before-Alice
absence, and Bob's unchanged delegate status/inbox before an organizer update have no waivers.

## Local verification

On 2026-10-16, the documented runner was executed from the issue-37 worktree against its
fresh non-enterprise RocksDB release product. All 72 cases were collected and reported per
mode: **66 passed, 1 intentional skip, 5 fingerprinted expected failures** for ordinary
accounts (6.19s) and key accounts (4.33s). All three mandatory strict flows passed in both
modes. The ten offline harness regression tests also passed, including real pytest cases
that reject setup/teardown masking, a different call-site assertion, and unexpected success.
Fresh unmasked runs reproduced the findings above before policy application. These are local
product/client results; GitHub Actions and native/physical clients have not been run here.

After PR #36 merged, the worktree was advanced to merged `main` at `5575dc93` and
revalidated on 2026-10-16. The non-enterprise RocksDB release rebuild and formatting check
passed. Both live client modes again reported **66 passed, 1 intentional skip, and 5
fingerprinted expected failures** (5.97s ordinary, 4.57s key); all strict flows passed.
The ten offline harness checks, dependency check, and workflow/manifest checks also passed.
The compatibility policy and test code required no changes for the merged base.
