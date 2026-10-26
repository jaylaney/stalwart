# Calendar sharing client coverage outcome

The existing pinned python-caldav harness now requires 72 ordinary-account cases and 77
key-account cases. The extra five key cases cover shared view/edit CRUD, principal-specific
properties and calendar ETags, revoke/regrant/rename/sync, delegated invitation/update/cancel,
and own-copy/shared-source replies with a sole-attendee decline. No new case has a skip or xfail
exception. The existing 72 case IDs and compatibility policy are unchanged.

## Local validation on 2026-10-25

- Python: isolated CPython 3.12.15 at `/private/tmp/stalwart-coverage-venv/bin/python`.
  Dependencies came from unchanged `tests/caldav_client/requirements.txt` (SHA-256
  `2f864695d6c3d3bf230bec2a7bf1912612b952c4e8aef73acfd723454462bde3`).
- python-caldav: v3.3.1, checkout and runner pin
  `111ae14b875f40d100d5259820f10210f00e20f9`; installed without dependency changes.
- Product: existing non-enterprise RocksDB shipping binary `target/release/stalwart`, SHA-256
  `6438ae439206ebae0d15d1e35680c41baab89d1b819be0927e5074bab80a777d`, matching
  `.superpowers/sdd/2026-10-25-calendar-sharing-client-coverage/shipping-binary.json`.
  Product Rust source did not change since that verified build, so no Rust rebuild was needed.
- Harness: `python -m unittest discover -s tests/caldav_client -p test_runner.py -v` passed
  30 tests, including manifest shape, key-only file selection, missing/extra sharing cases,
  public API error redaction under pytest assertion rewriting, late-result polling rejection,
  exact 401/403 status handling, and the retained failure boundaries.
- Full runner: `python tests/caldav_client/runner.py --binary target/release/stalwart
  --caldav-source /private/tmp/stalwart-coverage-caldav --log-dir NEW_PRIVATE_ABSOLUTE_DIR`
  passed sequentially without test filters. Plain: 66 passed, one skipped, five xfailed (72).
  Key: 71 passed, one skipped, five xfailed (77). All five new sharing flows passed.

The first unprivileged matrix attempt failed before server startup because sandboxed loopback
listener creation returned `EPERM` in both modes. A fresh escalated full run reached the product:
the 72 old cases retained expected outcomes, while the five new flows failed at Bob's calendar
discovery. An isolated public-API/DAV diagnostic showed the grant and Bob's shared calendar were
present. The owner URL used `%40`, while python-caldav returned the same href with a literal `@`;
the new helper had compared encoded paths directly. Comparing decoded paths fixed that harness
error. Subsequent full runs passed. One later plain-mode attempt timed out during disposable
recovery-server readiness before product tests; its key mode passed. The sanitized plain log
showed only recovery-mode startup, and a separate instrumented plain bootstrap reached readiness
after two connection-refused polls. The timeout's cause remains unproven. No deadline or global
state was changed. The final changed-source full run passed both modes. Attempt logs were retained
separately; no failed run was silently replaced. The task evidence report records their exact
locations and commands.

This is client interoperability evidence through public APIs and disposable normal-mode servers.
After regrant, the rename flow advances an owner event change, captures a new sync token,
proves that token is accepted incrementally before rename, then requires it to be refused after
rename. The new sharing polls reject states observed at or after 15 seconds and set a finite
remaining-budget HTTP inactivity timeout for each read, restoring the client's prior setting;
this does not guarantee absolute socket cancellation. The stale `If-Match` test is sequential
and does not replace the deterministic Rust I1 race test.
Native Apple Calendar, iOS, Thunderbird, and physical-device acceptance remain pending. This
local result does not claim GitHub Actions has completed.


## Independent review and CI boundary

The requested Astra xhigh review found no actionable defect in the production PR at
`b489852de60117623a1835508da26e8cec5f96e8`. Its separate coverage review found one Important
rename-token oracle gap and two Minor deadline/error-output gaps. The focused correction and
scoped re-review closed all three, with no new findings. The primary independently reran all
30 harness tests successfully and verified that the integrated Python source matches the reviewed
freeze, the original72 case IDs remain intact, and product source/binary, dependency pins and
compatibility policy are unchanged. These are static-review and local automated results.

The initial [PR CI run](https://github.com/jaylaney/stalwart/actions/runs/40211528604) at
`b489852d` failed key-account JMAP during ACL fixture cleanup (`Store is not empty`,
`tests/src/utils/cleanup.rs:443`), leaving search-index entries for one deleted plain admin mail
fixture. Earlier steps passed core units, both zero-access blob-store variants, ordinary/key
CalDAV and IMAP, and ordinary JMAP. The dependent independent-client job was skipped.
Read-only source triage identified a plausible inherited index/delete ordering counterexample
in paths unchanged between the PR base and that head; the CI log does not establish the actual
interleaving. This remains a failed qualification gate, not a proven environment flake. No mail
indexing change or CI retry was made as part of this coverage task. The updated PR's CI result
is separate and is not claimed green here. Native acceptance remains pending.
