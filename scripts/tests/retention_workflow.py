# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Fail-closed contract for the retention workflow and its reusable shard job."""

import textwrap

SHARD_WORKFLOW_NAME = "test-retention-shard.yml"
RUNNER = "ubuntu-24.04"
RUST_TOOLCHAIN = "1.99.1"
TARGET = "x86_64-unknown-linux-gnu"
WHEN = "github.event_name == 'workflow_dispatch' || github.event_name == 'schedule'"
AFTER_FAILURE = "!cancelled()"
REPORT_WHEN = f"{AFTER_FAILURE} && needs.style.result == 'success' && ({WHEN})"
ROOT = "/mnt/gates"
CARGO_HOME = "/mnt/retention-ci/.cargo"
ARTIFACT_PATH = "${{ env.GATES }}"
DEADLINE_ARG = '--deadline-epoch "$RETENTION_DEADLINE_EPOCH"'
COVERAGE = "python3 -I scripts/tests/coverage_net.py"
ARTIFACTS = "python3 -I scripts/tests/retention_artifacts.py"
LISTINGS = "python3 -I scripts/tests/capture_listings.py"
SETUP = "bash scripts/tests/retention_ci_setup.sh"
BOUNDED = f"{SETUP} within-budget"
CACHE_PATHS = "\n".join(f"{CARGO_HOME}/{path}" for path in ("registry/cache", "registry/index", "git/db"))
SHARD_TIMEOUTS = {"short": "170", "za-rocks": "355", "za-filesystem": "355"}
SNAPSHOT_ID = "${{ needs.gates-build.outputs.snapshot-artifact-id }}"
RECEIPT_ID = "${{ steps.receipt.outputs.artifact-id }}"
REPORTS = "$GATES/reports"
REPORT_INPUTS_OK = "(steps.collect.outputs.code == '0' || steps.collect.outputs.code == '1')"
READ_ONLY = {"contents": "read"}
READ_ACTIONS = {**READ_ONLY, "actions": "read"}
TRIGGERS = {
    "workflow_dispatch": None,
    "push": {"branches": "[main]", "paths-ignore": ["docs/**", "**.md"]},
    "pull_request": {"branches": "[main]", "paths-ignore": ["docs/**", "**.md"]},
    "schedule": [{"cron": "0 7 * * 1"}],
}


def expression(value):
    return "${{ " + value + " }}"


def shell(name, step_id, command, *, condition=None, env=None, timeout=None, optional=False):
    result = {"name": name, "id": step_id, "shell": "bash",
              "run": textwrap.dedent(command).strip()}
    if condition:
        result["if"] = expression(condition)
    if env:
        result["env"] = env
    if timeout:
        result["timeout-minutes"] = timeout
    if optional:
        result["continue-on-error"] = "true"
    return result


def action(name, uses, inputs, *, step_id=None, condition=None, timeout=None, optional=False):
    result = {"name": name, "uses": uses, "with": inputs}
    if step_id:
        result["id"] = step_id
    if condition:
        result["if"] = expression(condition)
    if timeout:
        result["timeout-minutes"] = timeout
    if optional:
        result["continue-on-error"] = "true"
    return result


def upload(name, artifact_name, paths, retention, timeout, **options):
    inputs = {"name": artifact_name, "path": paths, "retention-days": retention,
              "if-no-files-found": "ignore" if retention == "7" else "error"}
    if options.pop("compressed", False):
        inputs["compression-level"] = "0"
    return action(name, "actions/upload-artifact@v7.0.1", inputs, timeout=timeout, **options)


def download(name, artifact_id, folder, **options):
    return action(name, "actions/download-artifact@v8", {
        "artifact-ids": artifact_id, "path": f"{ROOT}/{folder}", "digest-mismatch": "error",
    }, **options)


def job_env(base_env, timeout, lane, toolchain):
    return {
        **base_env,
        "RUSTUP_TOOLCHAIN": toolchain,
        "RETENTION_TIMEOUT_MINUTES": timeout,
        "RETENTION_TARGET": TARGET,
        "CARGO_TARGET_DIR": "/mnt/listing-target" if lane == "ordinary" else "/mnt/target",
        "CARGO_HOME": CARGO_HOME,
        "CARGO_TERM_COLOR": "never",
        "GIT_OPTIONAL_LOCKS": "0",
        "GATES": ROOT,
        "TMPDIR": f"{ROOT}/tmp",
    }


def prepare_steps(lane):
    tool_label = "coverage tools" if lane == "instrumented" else "tools"
    return [
        shell("Set the work deadline and prepare retention directories", "prepare", r"""
            # Leave the final 12 minutes for settlement, diagnostics and bounded uploads.
            printf 'RETENTION_DEADLINE_EPOCH=%s\n' \
              "$(( $(date +%s) + (RETENTION_TIMEOUT_MINUTES - 12) * 60 ))" >> "$GITHUB_ENV"
            sudo mkdir -p "$GATES" "$CARGO_TARGET_DIR" "$CARGO_HOME" "$TMPDIR"
            sudo chown "$(id -u):$(id -g)" "$GATES" "$CARGO_TARGET_DIR" "$CARGO_HOME" "$TMPDIR"
            df -h "$GATES"
        """),
        action("Checkout", "actions/checkout@v7", {"persist-credentials": "false"}),
        shell(f"Install pinned {tool_label}", "tools", f"{SETUP} install-tools {lane}"),
        shell("Normalize checkout mtimes", "normalize", f"{ARTIFACTS} normalize-checkout"),
    ]


def cache_steps(lane, *, save=False):
    config = {"path": CACHE_PATHS, "key": expression("steps.cache-key.outputs.key")}
    result = [
        shell(f"Compute {lane} dependency download key", "cache-key", f"""
            key=$({ARTIFACTS} cache-key --lane {lane} \\
              --target-triple "$RETENTION_TARGET")
            test -n "$key"
            printf 'key=%s\\n' "$key" >> "$GITHUB_OUTPUT"
        """, optional=True),
        action(f"Restore {lane} dependency downloads", "actions/cache/restore@v6.1.0", config,
               step_id="cache-restore", timeout="5", optional=True,
               condition="steps.cache-key.outcome == 'success' && steps.cache-key.outputs.key != ''"),
        shell("Fetch and materialize dependencies", "fetch", r"""
            printf 'Dependency cache %s (exact hit: %s)\n' \
              "${CACHE_RESULT:-skipped}" "${CACHE_HIT:-false}" | tee -a "$GITHUB_STEP_SUMMARY"
            bash scripts/tests/retention_ci_setup.sh fetch
        """, env={"CACHE_HIT": expression("steps.cache-restore.outputs.cache-hit"),
                  "CACHE_RESULT": expression("steps.cache-restore.outcome")}),
    ]
    if save:
        result.extend([
            shell("Check room for scheduled dependency cache saves", "cache-budget",
                  f"{SETUP} cache-budget", timeout="2", optional=True,
                  condition=("github.event_name == 'schedule' && steps.cache-key.outcome == 'success'"
                             " && steps.cache-restore.outputs.cache-hit != 'true'"),
                  env={"GH_TOKEN": expression("github.token")}),
            action(f"Save {lane} dependency downloads", "actions/cache/save@v6.1.0", config,
                   step_id="cache-save", timeout="6", optional=True,
                   condition="github.event_name == 'schedule' && steps.cache-budget.outputs.room == 'true'"),
        ])
    return result


def unpack_step(condition=None):
    return shell("Restore the instrumented build", "unpack", rf"""
        time {BOUNDED} \
          python3 -I scripts/tests/retention_artifacts.py unpack-build \
          --archive "$GATES/snapshot/snapshot.tar.zst" --target "$CARGO_TARGET_DIR" \
          --manifest-out "$GATES/build.json"
        printf 'Expanded build bytes: %s\n' "$(du -sb "$CARGO_TARGET_DIR" | cut -f1)" \
          | tee -a "$GITHUB_STEP_SUMMARY"
        df -h "$GATES"
    """, condition=condition)


def producer_jobs(base_env):
    build = {
        "name": "Build Retention Coverage Target",
        "needs": "style",
        "if": WHEN,
        "runs-on": RUNNER,
        "timeout-minutes": "120",
        "permissions": READ_ACTIONS,
        "outputs": {"snapshot-artifact-id": expression("steps.snapshot.outputs.artifact-id")},
        "env": job_env(base_env, "120", "instrumented", RUST_TOOLCHAIN),
        "steps": [
            *prepare_steps("instrumented"),
            *cache_steps("instrumented", save=True),
            shell("Build instrumented targets", "build", f"""
                time {COVERAGE} --build-only --out "$GATES/build" \\
                  {DEADLINE_ARG}
            """),
            shell("Pack the build snapshot", "pack", r"""
                remaining=$((RETENTION_DEADLINE_EPOCH - $(date +%s)))
                test "$remaining" -gt 0
                time timeout --kill-after=30s "$remaining" \
                  python3 -I scripts/tests/retention_artifacts.py pack-build \
                  --manifest "$GATES/build/build.json" --target "$CARGO_TARGET_DIR" \
                  --out "$GATES/snapshot.tar.zst"
                printf 'Build snapshot compressed bytes: %s; target bytes: %s\n' \
                  "$(stat -c '%s' "$GATES/snapshot.tar.zst")" \
                  "$(du -sb "$CARGO_TARGET_DIR" | cut -f1)" | tee -a "$GITHUB_STEP_SUMMARY"
                df -h "$GATES"
            """),
            upload("Upload the build snapshot", "retention-build-${{ github.run_attempt }}",
                   f"{ARTIFACT_PATH}/snapshot.tar.zst", "1", "8", step_id="snapshot",
                   compressed=True),
            shell("Stage sanitized build diagnostics", "build-diagnostics", f"""
                {SETUP} stage-logs "$GATES/build-diagnostics" \\
                  "$GATES/build/steps.tsv" "$GATES"/build/logs/*.log
            """, condition=AFTER_FAILURE, timeout="1"),
            upload("Retain build diagnostics", "retention-build-diagnostics-${{ github.run_attempt }}",
                   f"{ARTIFACT_PATH}/build-diagnostics/", "7", "2",
                   condition=f"{AFTER_FAILURE} && steps.build-diagnostics.outcome == 'success'"),
        ],
    }
    listings = {
        "name": "Capture Retention Test Listings",
        "needs": "style",
        "if": WHEN,
        "runs-on": RUNNER,
        "timeout-minutes": "100",
        "permissions": READ_ACTIONS,
        "outputs": {"receipt-artifact-id": RECEIPT_ID},
        "env": job_env(base_env, "100", "ordinary", RUST_TOOLCHAIN),
        "steps": [
            *prepare_steps("ordinary"),
            *cache_steps("ordinary", save=True),
            shell("Capture all CI step listings", "capture", f"""
                time {LISTINGS} --out "$GATES/listings" \\
                  {DEADLINE_ARG}
            """),
            shell("Stage listing evidence", "stage", f"""
                {ARTIFACTS} stage --kind listings \\
                  --producer listings --status "$CAPTURE_OUTCOME" \\
                  --source "$GATES/listings" --out "$GATES/evidence"
            """, condition=AFTER_FAILURE, timeout="2",
                  env={"CAPTURE_OUTCOME": expression("steps.capture.outcome")}),
            upload("Upload listing evidence", "retention-listings-${{ github.run_attempt }}",
                   f"{ARTIFACT_PATH}/evidence/", "1", "4", step_id="receipt",
                   condition=f"{AFTER_FAILURE} && steps.stage.outcome == 'success'"),
            upload("Retain sanitized listing receipt",
                   "retention-listings-diagnostics-${{ github.run_attempt }}",
                   f"{ARTIFACT_PATH}/evidence/receipt.json", "7", "2",
                   condition=f"{AFTER_FAILURE} && steps.stage.outcome == 'success'"),
        ],
    }
    return {"gates-build": build, "gates-listings": listings}


def shard_callee(base_env):
    input_id = expression("inputs.snapshot-artifact-id")
    shard_id = expression("inputs.shard-id")
    toolchain = expression("inputs.rust-toolchain")
    timeout = expression("inputs.timeout-minutes")
    return {
        "name": "Retention Coverage Shard",
        "on": {"workflow_call": {
            "inputs": {
                "shard-id": {"required": "true", "type": "string"},
                "snapshot-artifact-id": {"required": "true", "type": "string"},
                "rust-toolchain": {"required": "true", "type": "string"},
                "timeout-minutes": {"required": "true", "type": "number"},
            },
            "outputs": {
                "receipt-artifact-id": {"value": expression("jobs.shard.outputs.receipt-artifact-id")},
            },
        }},
        "permissions": READ_ONLY,
        "jobs": {"shard": {
            "name": "Run coverage shard",
            "runs-on": RUNNER,
            "timeout-minutes": timeout,
            "outputs": {"receipt-artifact-id": RECEIPT_ID},
            "env": job_env(base_env, timeout, "instrumented", toolchain),
            "steps": [
                *prepare_steps("instrumented"),
                shell("Require a selected build snapshot ID", "artifact-id", r"""
                    remaining=$((RETENTION_DEADLINE_EPOCH - $(date +%s)))
                    if (( remaining < 21 * 60 )); then
                      printf '%s\n' 'Insufficient work time for the build snapshot download' >&2
                      exit 1
                    fi
                    [[ "$SNAPSHOT_ID" =~ ^[1-9][0-9]*$ ]]
                """, env={"SNAPSHOT_ID": input_id}),
                download("Download the selected build snapshot", input_id, "snapshot", timeout="20"),
                unpack_step(),
                *cache_steps("instrumented"),
                shell("Run coverage shard", "coverage", f"""
                    time {COVERAGE} --shard "$SHARD_ID" \\
                      --out "$GATES/net" --build-manifest "$GATES/build.json" \\
                      {DEADLINE_ARG} --diagnostics-reserve-seconds 300
                """, env={"SHARD_ID": shard_id}),
                shell("Stage validated coverage evidence", "stage", f"""
                    {ARTIFACTS} stage --kind shard \\
                      --producer "$SHARD_ID" --status "$SHARD_OUTCOME" \\
                      --source "$GATES/net" --out "$GATES/evidence" \\
                      --build-manifest "$GATES/build.json" --snapshot-artifact-id "$SNAPSHOT_ID"
                    printf 'Staged shard bytes: %s\\n' "$(du -sb "$GATES/evidence" | cut -f1)" \\
                      | tee -a "$GITHUB_STEP_SUMMARY"
                """, condition=AFTER_FAILURE, timeout="3",
                      env={"SHARD_ID": shard_id, "SNAPSHOT_ID": input_id,
                           "SHARD_OUTCOME": expression("steps.coverage.outcome")}),
                upload("Upload coverage evidence", "retention-${{ inputs.shard-id }}-${{ github.run_attempt }}",
                       f"{ARTIFACT_PATH}/evidence/", "1", "5", step_id="receipt",
                       condition=f"{AFTER_FAILURE} && steps.stage.outcome == 'success'"),
                upload("Retain sanitized shard diagnostics",
                       "retention-${{ inputs.shard-id }}-diagnostics-${{ github.run_attempt }}",
                       f"{ARTIFACT_PATH}/evidence/receipt.json\n"
                       f"{ARTIFACT_PATH}/evidence/logs/\n{ARTIFACT_PATH}/evidence/steps.tsv",
                       "7", "2", condition=f"{AFTER_FAILURE} && steps.stage.outcome == 'success'"),
            ],
        }},
    }


def reporter_job(base_env):
    slots = ("short", "za-rocks", "za-filesystem", "listings")
    artifact_ids = {
        slot: expression(f"needs.gates-{slot}.outputs.receipt-artifact-id") for slot in slots
    }
    artifact_env = {f"{slot.upper().replace('-', '_')}_ID": value
                    for slot, value in artifact_ids.items()}
    id_calls = "\n".join(f'check_id {slot} "${slot.upper().replace("-", "_")}_ID"'
                         for slot in slots)
    outcomes = ("BUILD", "SHORT", "ZA_ROCKS", "ZA_FILESYSTEM", "LISTINGS", "COLLECT", "MERGE",
                "FUNCTIONS", "INVENTORY", "COVERAGE")
    outcome_env = {
        f"{name}_OUTCOME": expression(
            f"needs.gates-{name.lower().replace('_', '-')}.result" if index < 5
            else f"steps.{name.lower()}.outcome")
        for index, name in enumerate(outcomes)
    }
    download_checks = " && ".join(f"steps.download-{slot}.outcome == 'success'" for slot in slots)
    lines = f"""
        remaining=$((RETENTION_DEADLINE_EPOCH - $(date +%s)))
        if (( remaining < 41 * 60 )); then
          printf '%s\\n' 'Insufficient work time for the selected artifact downloads' >&2
          exit 1
        fi
        check_id() {{
          if [[ "$2" =~ ^[1-9][0-9]*$ ]]; then
            printf '%s=true\\n' "$1" >> "$GITHUB_OUTPUT"
          else
            printf 'Missing or invalid %s artifact ID. Re-run all jobs if artifacts expired.\\n' "$1"
            bad=1
          fi
        }}
        bad=0
        check_id snapshot "$SNAPSHOT_ID"
        {textwrap.indent(id_calls, '        ').lstrip()}
        exit "$bad"
    """
    return {
        "name": "Test Retention Gates",
        "needs": ["style", "gates-build", *(f"gates-{slot}" for slot in slots)],
        "if": expression(REPORT_WHEN),
        "runs-on": RUNNER,
        "timeout-minutes": "100",
        "env": job_env(base_env, "100", "instrumented", RUST_TOOLCHAIN),
        "steps": [
            *prepare_steps("instrumented"),
            shell("Validate selected artifact IDs", "artifact-ids", lines,
                  condition=AFTER_FAILURE, env={"SNAPSHOT_ID": SNAPSHOT_ID, **artifact_env}),
            *(download(f"Download {slot} evidence", artifact_ids[slot], f"inputs/{slot}",
                       condition=f"{AFTER_FAILURE} && steps.artifact-ids.outputs.{slot} == 'true'",
                       step_id=f"download-{slot}", timeout="5")
              for slot in slots),
            download("Download the selected build snapshot", SNAPSHOT_ID, "snapshot",
                     condition=f"{AFTER_FAILURE} && steps.artifact-ids.outputs.snapshot == 'true'",
                     step_id="snapshot-download", timeout="20"),
            unpack_step(f"{AFTER_FAILURE} && steps.snapshot-download.outcome == 'success' &&"
                        " steps.normalize.outcome == 'success'"),
            *cache_steps("instrumented"),
            shell("Collect retention evidence", "collect", f"""
                status=0
                time {BOUNDED} \\
                  {ARTIFACTS} collect \\
                  --inputs "$GATES/inputs" --snapshot-artifact-id "$SNAPSHOT_ID" \\
                  --build-manifest "$GATES/build.json" --out "{REPORTS}" || status=$?
                printf 'code=%s\\n' "$status" >> "$GITHUB_OUTPUT"
                exit "$status"
            """, condition=f"{AFTER_FAILURE} && steps.unpack.outcome == 'success' && {download_checks}",
                  env={"SNAPSHOT_ID": SNAPSHOT_ID}),
            shell("Prepare sources for reports", "sources", f"""
                mkdir -p "{REPORTS}/src"
                {BOUNDED} git archive HEAD tests crates | \\
                  {BOUNDED} tar -x -C "{REPORTS}/src"
            """, condition=f"{AFTER_FAILURE} && {REPORT_INPUTS_OK}"),
            shell("Merge validated profiles", "merge", f"""
                status=0
                time {COVERAGE} --merge-only \\
                  --out "{REPORTS}/net" --build-manifest "$GATES/build.json" \\
                  {DEADLINE_ARG} || status=$?
                if [[ -s "{REPORTS}/net/merged.json" ]]; then
                  printf 'available=true\\n' >> "$GITHUB_OUTPUT"
                fi
                exit "$status"
            """, condition=f"{AFTER_FAILURE} && {REPORT_INPUTS_OK} && steps.fetch.outcome == 'success'"),
            shell("Executed functions", "functions", f"""
                {BOUNDED} \\
                  python3 -I tests/qualification/tools/strict_rule.py functions \\
                  --exports "{REPORTS}/net/steps" --sources "{REPORTS}/src" \\
                  --out "{REPORTS}/functions.json" 2>&1 | tee "{REPORTS}/functions.log"
            """, condition=f"{AFTER_FAILURE} && steps.sources.outcome == 'success'"),
            shell("Inventory gate", "inventory", f"""
                {BOUNDED} \\
                  python3 -I scripts/tests/check_test_inventory.py \\
                  --baseline tests/qualification/baseline-0.16.27.json \\
                  --dispositions tests/qualification/dispositions.json \\
                  --listing "{REPORTS}/listings/list" --ignored-listing "{REPORTS}/listings/ign" \\
                  --executed "{REPORTS}/functions.json" 2>&1 | tee "{REPORTS}/inventory.out"
            """, condition=f"{AFTER_FAILURE} && steps.functions.outcome == 'success'"),
            shell("Coverage gate", "coverage", f"""
                {BOUNDED} \\
                  python3 -I scripts/tests/check_coverage.py \\
                  --baseline tests/qualification/baseline-0.16.27.json \\
                  --dispositions tests/qualification/dispositions.json \\
                  --merged "{REPORTS}/net/merged.json" --sources "{REPORTS}/src" \\
                  --out "{REPORTS}/coverage.json" 2>&1 | tee "{REPORTS}/coverage.out"
            """, condition=f"{AFTER_FAILURE} && steps.sources.outcome == 'success' &&"
                            " steps.merge.outputs.available == 'true'"),
            shell("Sanitize gate diagnostics", "sanitize", r"""
                bash scripts/tests/retention_ci_setup.sh stage-logs "$RUNNER_TEMP/gates-receipt" \
                  "$GATES/reports/inventory.out" "$GATES/reports/coverage.out" \
                  "$GATES/reports/functions.log" "$GATES/reports/collection.json" \
                  "$GATES/reports/net/logs/merged.export.log" "$GATES/reports/net/merge.json"
                grep -h "^## exit\|^# config" "$GATES"/reports/listings/list/*.txt \
                  "$GATES"/reports/listings/ign/*.txt \
                  > "$RUNNER_TEMP/gates-receipt/listing-exits.txt" 2>/dev/null || true
            """, condition=AFTER_FAILURE, timeout="2"),
            upload("Retain sanitized gate diagnostics", "gates-receipt-${{ github.run_attempt }}",
                   "${{ runner.temp }}/gates-receipt/", "7", "4",
                   condition=f"{AFTER_FAILURE} && steps.sanitize.outcome == 'success'"),
            shell("Require a complete passing retention run", "qualify", f"""
                status=0
                for outcome in {' '.join(outcomes)}; do
                  key="${{outcome}}_OUTCOME"
                  printf '%s: %s\\n' "$outcome" "${{!key}}" | tee -a "$GITHUB_STEP_SUMMARY"
                  if [[ "${{!key}}" != success ]]; then
                    status=1
                  fi
                done
                exit "$status"
            """, condition=AFTER_FAILURE, env=outcome_env),
        ],
    }


def compare(actual, expected, where):
    """Compare structured YAML, allowing only outer whitespace in shell bodies."""
    if isinstance(actual, dict) and isinstance(expected, dict):
        for extra in sorted(actual.keys() - expected.keys()):
            yield f"{where}.{extra}: unexpected"
        for key, value in expected.items():
            if key not in actual:
                yield f"{where}.{key}: missing"
            else:
                yield from compare(actual[key], value, f"{where}.{key}")
    elif isinstance(actual, list) and where.endswith(".steps"):
        job = where.removesuffix(".steps")
        actual_labels = [step.get("id", step.get("name")) if isinstance(step, dict) else None
                         for step in actual]
        expected_labels = [step.get("id", step["name"]) for step in expected]
        if actual_labels != expected_labels:
            yield f"{job}: unexpected step order or set, expected {expected_labels!r}"
        for key, step in zip(expected_labels, expected):
            if actual_labels.count(key) == 1:
                yield from compare(actual[actual_labels.index(key)], step, f"{job}, step {key}")
    elif where.endswith(".run") or where.endswith(".path"):
        if not isinstance(actual, str) or actual.strip() != expected.strip():
            yield f"{where}: differs from the retention contract"
    elif actual != expected:
        yield f"{where}: differs from the retention contract"


def check(workflow, callee, base_env, shard_names):
    problems = list(compare(workflow.get("on"), TRIGGERS, "test.yml: on"))
    expected = producer_jobs(base_env)
    if set(shard_names) != set(SHARD_TIMEOUTS):
        problems.append("test.yml: coverage shard partition and workflow callers disagree")
    for shard in shard_names:
        if shard not in SHARD_TIMEOUTS:
            continue
        expected[f"gates-{shard}"] = {
            "name": f"Retention Coverage ({shard})",
            "needs": "gates-build",
            "if": WHEN,
            "uses": f"./.github/workflows/{SHARD_WORKFLOW_NAME}",
            "with": {
                "shard-id": shard,
                "snapshot-artifact-id": SNAPSHOT_ID,
                "rust-toolchain": RUST_TOOLCHAIN,
                "timeout-minutes": SHARD_TIMEOUTS[shard],
            },
        }
    expected["gates"] = reporter_job(base_env)
    jobs = workflow["jobs"]
    for name, spec in expected.items():
        where = f"job {name}"
        if name not in jobs:
            problems.append(f"{where}: missing")
        else:
            problems.extend(compare(jobs[name], spec, where))
    for name in sorted(name for name in jobs if name.startswith("gates-") and name not in expected):
        problems.append(f"job {name}: unexpected retention producer")
    if callee is None:
        problems.append(f"{SHARD_WORKFLOW_NAME}: missing")
    else:
        for key, spec in shard_callee(base_env).items():
            actual = callee.get(key)
            if key == "jobs":
                if not isinstance(actual, dict) or set(actual) != {"shard"}:
                    problems.append(f"{SHARD_WORKFLOW_NAME}: expected exactly one shard job")
                else:
                    problems.extend(compare(actual["shard"], spec["shard"], "job shard"))
            else:
                problems.extend(compare(actual, spec, f"{SHARD_WORKFLOW_NAME}: {key}"))
        for key in sorted(callee.keys() - {"name", "on", "permissions", "jobs"}):
            problems.append(f"{SHARD_WORKFLOW_NAME}: unexpected {key}")
    return problems
