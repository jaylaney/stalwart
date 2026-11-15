#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Stage sanitized retention evidence and validate one workflow run's selected artifacts."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture_listings
from capture_listings import run_identity
import ci_steps
from coverage_net import STEPS, canonical_json, write_json

SCHEMA = 1
LISTINGS = "listings"
PRODUCERS = (*ci_steps.COVERAGE_SHARDS, LISTINGS)
INDEXED_PROFILE_MAGIC = b"\xfflprofi\x81"
AUTH = re.compile(rb"((?:Basic|Bearer) )[A-Za-z0-9._~+/=-]+", re.IGNORECASE)
ASSIGNMENT = re.compile(rb'''((?:password|secret|token)[A-Za-z_]*["']?[=:] *["']?)[^"' ,}\s]+''',
                        re.IGNORECASE)
SHA256 = re.compile(r"[a-f0-9]{64}")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_keys(items):
    result = dict(items)
    require(len(result) == len(items), "duplicate JSON key")
    return result


def load(path):
    require(path.is_file() and not path.is_symlink(), f"missing regular file: {path.name}")
    data = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_keys)
    require(isinstance(data, dict), f"{path.name} is not a JSON object")
    return data


def fresh(directory):
    require(not directory.is_symlink()
            and (not directory.exists() or (directory.is_dir() and not any(directory.iterdir()))),
            "evidence output directory must be fresh")
    directory.mkdir(parents=True, exist_ok=True)


def file_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def redacted(path, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    with path.open("rb") as source, destination.open("wb") as output:
        for line in source:
            output.write(ASSIGNMENT.sub(rb"\1[REDACTED]", AUTH.sub(rb"\1[REDACTED]", line)))


def transfer(source, out, names, *, redact=False):
    files = {}
    for name in names:
        path, destination = source / name, out / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        require(path.is_file() and not path.is_symlink(), f"missing regular file: {name}")
        if redact:
            redacted(path, destination)
        else:
            shutil.copyfile(path, destination)
        files[name] = file_digest(destination)
    return files


def positive_id(value):
    return isinstance(value, str) and re.fullmatch(r"[1-9][0-9]*", value) is not None


def unit_features():
    features = " ".join(os.environ.get(ci_steps.UNIT_FEATURES, "").split())
    require(features, "UNIT_TEST_FEATURES is required for listing validation")
    return features


def build_identity(path, run, snapshot):
    require(path is not None and positive_id(snapshot), "build manifest and snapshot ID required")
    build = load(path)
    require(build.get("schema") == SCHEMA and build.get("commit") == run["commit"]
            and build.get("run_id") == run["run_id"],
            "build manifest does not match current run and commit")
    attempt = build.get("build_attempt")
    require(type(attempt) is int and 0 < attempt <= run["producer_attempt"],
            "invalid build attempt")
    objects = build.get("objects")
    require(isinstance(objects, list) and objects
            and hashlib.sha256(canonical_json(objects)).hexdigest()
            == build.get("object_set_sha256"),
            "invalid build coverage object set")
    require(isinstance(build.get("identity"), dict) and build["identity"],
            "build manifest omits toolchain identity")
    return build


def diagnostic_names(producer, root):
    if producer not in ci_steps.COVERAGE_SHARDS:
        return set()
    names = set()
    net = root / "steps.tsv"
    if net.is_file() and not net.is_symlink():
        names.add(net.name)
    logs = root / "logs"
    if logs.is_dir() and not logs.is_symlink():
        steps = "|".join(map(re.escape, ci_steps.COVERAGE_SHARDS[producer]))
        pattern = re.compile(rf"(?:{steps})(?:\.attempt[12])?(?:\.(?:export|profdata-merge))?\.log")
        names.update(f"logs/{p.name}" for p in logs.iterdir()
                     if p.is_file() and not p.is_symlink() and pattern.fullmatch(p.name))
    return names


def valid_status(value, labels):
    return type(value) is int or isinstance(value, str) and value in labels


def coverage_paths(shard, ledger, status):
    entries = ledger.get("steps")
    require(isinstance(entries, dict) and set(entries) == set(ci_steps.COVERAGE_SHARDS[shard]),
            "incorrect shard step ownership")
    found, missing_profiles, missing_exports = set(), [], []
    for step, entry in entries.items():
        require(isinstance(entry, dict) and set(entry) == {"status", "attempts", "profile", "export"}
                and type(entry.get("profile")) is bool
                and entry.get("status") in ("success", "failure", "not-run")
                and entry.get("export") in ("success", "failure", "not-attempted"),
                f"{step}: invalid step outcome")
        attempts = entry.get("attempts")
        require(isinstance(attempts, list) and len(attempts) <= 2, f"{step}: invalid attempts")
        for index, attempt in enumerate(attempts, start=1):
            require(isinstance(attempt, dict)
                    and set(attempt) == {"attempt", "status", "names", "profile"}
                    and type(attempt.get("attempt")) is int
                    and attempt["attempt"] == index
                    and valid_status(attempt.get("status"), ("timeout", "deadline"))
                    and isinstance(attempt.get("names"), list)
                    and all(isinstance(name, str) and re.fullmatch(r"[A-Za-z0-9_:]+", name)
                            for name in attempt["names"])
                    and attempt.get("profile") in ("collected", "merge-error", "no-data"),
                    f"{step}: invalid attempt {index}")
        if attempts:
            first = attempts[0]
            retry = first["status"] not in (0, "deadline")
            if STEPS[step]["rerun"] == "by-name":
                retry = retry and bool(first["names"])
            require(len(attempts) == 1 + int(retry), f"{step}: invalid retry sequence")
        collected = any(a["profile"] == "collected" for a in attempts)
        require(not entry["profile"] or collected, f"{step}: profile without attempt evidence")
        require(entry["export"] != "success" or entry["profile"], f"{step}: export without profile")
        if entry["status"] == "not-run":
            require(not attempts and not entry["profile"] and entry["export"] == "not-attempted",
                    f"{step}: unexecuted step has evidence")
        success = bool(attempts and attempts[-1]["status"] == 0
                       and attempts[-1]["profile"] == "collected"
                       and all(a["profile"] != "merge-error" for a in attempts)
                       and entry["export"] == "success")
        require((entry["status"] == "success") == success
                and (status != "success" or success), f"{step}: inconsistent coverage result")
        if entry["profile"]:
            found.add(f"profdata/{step}.profdata")
        else:
            missing_profiles.append(step)
        if entry["export"] == "success":
            found.add(f"steps/{step}.json")
        else:
            missing_exports.append(step)
    return found, missing_profiles, missing_exports


def listing_paths(ledger, features, contract, status):
    require(set(ledger) == {"schema", "commit", "run_id", "producer_attempt",
                            "identity", "unit_features", "captures"}
            and ledger.get("schema") == SCHEMA and ledger.get("unit_features") == features,
            "listing ledger does not match the unit feature contract")
    identity = ledger.get("identity")
    require(isinstance(identity, dict)
            and set(identity) <= {"cargo", "rustc", "ImageOS", "ImageVersion", "CARGO_TARGET_DIR"}
            and all(isinstance(value, str) for value in identity.values())
            and all(isinstance(identity.get(tool), str) and identity[tool]
                    for tool in ("cargo", "rustc")),
            "listing build identity is missing")
    entries = ledger.get("captures")
    require(isinstance(entries, dict) and set(entries) == set(contract),
            "incorrect listing configuration ownership")
    found, missing = set(), []
    for name, spec in contract.items():
        outcomes = entries[name]
        require(isinstance(outcomes, list) and len(outcomes) == len(spec["sections"])
                and all(valid_status(value, ("timeout", "deadline", "not-run"))
                        for value in outcomes),
                f"{name}: listing section status is invalid")
        require(status != "success" or all(value == 0 for value in outcomes),
                f"{name}: successful producer reports a failed listing")
        if any(value != "not-run" for value in outcomes):
            found.add(name)
        if any(value != 0 for value in outcomes):
            missing.append(name)
    return found, missing


def file_set(root):
    files = set()
    for path in root.rglob("*"):
        require(not path.is_symlink(), "symbolic link in evidence payload")
        if path.is_dir():
            continue
        require(path.is_file(), "non-regular file in evidence payload")
        files.add(path.relative_to(root).as_posix())
    return files


def source_files(root, directories):
    files = set()
    for name in directories:
        directory = root / name
        if directory.exists() or directory.is_symlink():
            require(directory.is_dir() and not directory.is_symlink(),
                    f"{name}: expected a regular directory")
            files.update(f"{name}/{child}" for child in file_set(directory))
    return files


def validate_coverage(root, names):
    for name in names:
        path = root / name
        if name.startswith("profdata/"):
            with path.open("rb") as source:
                header = source.read(56)
            require(len(header) == 56 and header.startswith(INDEXED_PROFILE_MAGIC)
                    and 56 <= int.from_bytes(header[32:40], "little") < path.stat().st_size,
                    f"{name}: invalid indexed LLVM profile")
        else:
            report = load(path)
            data = report.get("data")
            require(report.get("type") == "llvm.coverage.json.export"
                    and isinstance(report.get("version"), str)
                    and isinstance(data, list) and len(data) == 1
                    and isinstance(data[0], dict)
                    and isinstance(data[0].get("files"), list)
                    and isinstance(data[0].get("functions"), list)
                    and isinstance(data[0].get("totals"), dict),
                    f"{name}: malformed region-level LLVM export")


def validate_listing(path, name, spec, statuses):
    lines = path.read_text(encoding="utf-8").splitlines()
    marker = ("# config:", "# env:", "## SELECTED", "## UNFILTERED", "## command:", "## exit:")
    actual = [line for line in lines if line.startswith(marker)]
    expected = [f'# config: {spec["label"]}', f'# env: {spec["environment"]}']
    for (title, cmd), status in zip(spec["sections"], statuses):
        expected.extend([title, "## command: " + " ".join(cmd), f"## exit: {status}"])
    require(actual == expected, f"{name}: listing text contradicts its capture contract")


def validate_ledger(receipt, producer, run, build, snapshot, features, contract):
    require(receipt.get("schema") == SCHEMA and receipt.get("producer") == producer
            and receipt.get("kind") == ("listings" if producer == LISTINGS else "shard"),
            f"{producer}: receipt schema or producer identity differs")
    require(receipt.get("commit") == run["commit"] and receipt.get("run_id") == run["run_id"],
            f"{producer}: receipt does not match this run or commit")
    attempt = receipt.get("producer_attempt")
    require(type(attempt) is int and 0 < attempt <= run["producer_attempt"],
            f"{producer}: invalid producer attempt")
    status = receipt.get("status")
    require(status in ("success", "failure") and not receipt.get("integrity_error"),
            f"{producer}: skipped or invalid producer")
    ledger = receipt.get("ledger")
    require(isinstance(ledger, dict), f"{producer}: missing execution ledger")
    for key in ("commit", "run_id", "producer_attempt"):
        require(type(ledger.get(key)) is type(receipt[key]) and ledger[key] == receipt[key],
                f"{producer}: ledger differs in {key}")
    if producer == LISTINGS:
        require(receipt.get("contract_sha256") == hashlib.sha256(canonical_json(contract)).hexdigest(),
                "listing capture contract changed")
        found, missing = listing_paths(ledger, features, contract, status)
        if build is not None:
            identity = ledger["identity"]
            require(all(build["identity"].get(tool) == identity[tool]
                        for tool in ("cargo", "rustc")),
                    "listing build toolchain differs")
        return found, [], [], missing
    require(build is not None and receipt.get("snapshot_artifact_id") == snapshot
            and receipt.get("build_attempt") == build["build_attempt"]
            and receipt.get("object_set_sha256") == build["object_set_sha256"]
            and attempt >= build["build_attempt"],
            f"{producer}: selected build snapshot differs")
    require(set(ledger) == {"schema", "shard", "compatible", "commit", "run_id", "producer_attempt",
                            "build_attempt", "object_set_sha256", "steps"}
            and ledger.get("schema") == SCHEMA and ledger.get("shard") == producer
            and ledger.get("compatible") is True,
            f"{producer}: no compatible coverage run")
    for key in ("build_attempt", "object_set_sha256"):
        require(type(ledger.get(key)) is type(receipt[key]) and ledger[key] == receipt[key],
                f"{producer}: ledger differs in {key}")
    found, missing_profiles, missing_exports = coverage_paths(producer, ledger, status)
    return found, missing_profiles, missing_exports, []


def stage(kind, producer, status, source, out, manifest=None, snapshot=None):
    require((kind == "listings" and producer == LISTINGS)
            or (kind == "shard" and producer in ci_steps.COVERAGE_SHARDS), "unknown producer")
    require(status in ("success", "failure", "skipped"), "unknown producer status")
    fresh(out)
    run = run_identity()
    receipt = dict(schema=SCHEMA, kind=kind, producer=producer, status=status, **run)
    # Keep the safe subset of diagnostics even if a producer failed before its ledger settled.
    receipt["files"] = transfer(source, out, diagnostic_names(producer, source), redact=True)
    try:
        build, features, contract = None, None, None
        candidate = dict(receipt)
        if kind == "listings":
            features = unit_features()
            contract = capture_listings.capture_contract(features)
            candidate.update(ledger=load(source / "listing.json"),
                           contract_sha256=hashlib.sha256(canonical_json(contract)).hexdigest())
        else:
            build = build_identity(manifest, run, snapshot)
            candidate.update(ledger=load(source / "shard.json"), snapshot_artifact_id=snapshot,
                           build_attempt=build["build_attempt"],
                           object_set_sha256=build["object_set_sha256"])
        names, _, _, _ = validate_ledger(candidate, producer, run, build, snapshot, features,
                                           contract)
        if kind == "listings":
            require(source_files(source, ("list", "ign")) == names,
                    "listing files disagree with section outcomes")
        else:
            # The driver retains indexed per-attempt data locally. Only the merged
            # profile of each step belongs in the artifact.
            separate_attempts = {
                f"profdata/{step}.attempt{attempt}.profdata"
                for step in ci_steps.COVERAGE_SHARDS[producer] for attempt in (1, 2)
            }
            require(source_files(source, ("steps", "profdata")) - separate_attempts == names,
                    "coverage files disagree with step outcomes")
            validate_coverage(source, names)
        files = transfer(source, out, names, redact=(kind == "listings"))
        receipt = dict(candidate, files={**receipt["files"], **files})
    except (OSError, ValueError):
        for kind_dir in ("steps", "profdata", "list", "ign"):
            path = out / kind_dir
            if path.exists():
                shutil.rmtree(path)
        receipt["integrity_error"] = True
        print(f"{producer}: producer evidence failed validation", file=sys.stderr)
    write_json(out / "receipt.json", receipt)
    return 0


def validate_producer(path, producer, run, build, snapshot, features, contract):
    require(path.is_dir() and not path.is_symlink(), f"{producer}: missing selected artifact")
    receipt = load(path / "receipt.json")
    names, missing_profiles, missing_exports, missing_captures = validate_ledger(
        receipt, producer, run, build, snapshot, features, contract
    )
    expected = names | diagnostic_names(producer, path)
    files = receipt.get("files")
    require(isinstance(files, dict) and set(files) == expected
            and file_set(path) == expected | {"receipt.json"},
            f"{producer}: unexpected or missing files")
    for name, digest in files.items():
        require(isinstance(digest, str) and SHA256.fullmatch(digest)
                and file_digest(path / name) == digest, f"{producer}/{name}: content hash differs")
    if producer == LISTINGS:
        for name in names:
            validate_listing(path / name, name, contract[name], receipt["ledger"]["captures"][name])
    else:
        validate_coverage(path, names)
    return names, receipt["status"], missing_profiles, missing_exports, missing_captures


def collect(inputs, out, manifest, snapshot):
    try:
        fresh(out)
    except (OSError, ValueError) as error:
        print(f"collection: {error}", file=sys.stderr)
        return 2
    errors = []
    statuses = {}
    names_by_producer = {}
    missing_profiles, missing_exports, missing_captures = [], [], []
    destination = out / "collection.json"
    try:
        run = run_identity()
        build = build_identity(manifest, run, snapshot)
        features = unit_features()
        contract = capture_listings.capture_contract(features)
        require(inputs.is_dir() and not inputs.is_symlink(), "selected artifacts directory missing")
        if any(path.name not in PRODUCERS for path in inputs.iterdir()):
            errors.append("unexpected selected producer artifact directory")
        for name in PRODUCERS:
            try:
                files, status, profiles, exports, listings = validate_producer(
                    inputs / name, name, run, build, snapshot, features, contract
                )
            except (OSError, ValueError) as error:
                errors.append(str(error))
                continue
            names_by_producer[name] = files
            statuses[name] = status
            missing_profiles.extend(profiles)
            missing_exports.extend(exports)
            missing_captures.extend(listings)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        errors.append(str(error))
    summary = dict(schema=SCHEMA, status="integrity-error", producers=statuses, errors=errors,
                   missing_profiles=sorted(missing_profiles), missing_exports=sorted(missing_exports),
                   missing_steps=sorted(set(missing_profiles) | set(missing_exports)),
                   missing_captures=sorted(missing_captures))
    if errors:
        for error in errors:
            print(f"collection: {error}", file=sys.stderr)
        write_json(destination, summary)
        return 2

    try:
        for directory in ("net/steps", "net/profdata", "listings/list", "listings/ign"):
            (out / directory).mkdir(parents=True)
        for name, paths in names_by_producer.items():
            transfer(inputs / name, out / ("listings" if name == LISTINGS else "net"), paths)
    except (OSError, ValueError) as error:
        for folder in ("net", "listings"):
            shutil.rmtree(out / folder, ignore_errors=True)
        errors.append(str(error))
        print(f"collection: {error}", file=sys.stderr)
        write_json(destination, summary)
        return 2
    failed = any(status == "failure" for status in statuses.values())
    summary["status"] = "failure" if failed else "success"
    write_json(destination, summary)
    if failed:
        print("collection: validated producer failure", file=sys.stderr)
        return 1
    return 0


def register_evidence_commands(commands):
    producer = commands.add_parser("stage")
    producer.add_argument("--kind", choices=("shard", "listings"), required=True)
    producer.add_argument("--producer", choices=PRODUCERS, required=True)
    producer.add_argument("--status", choices=("success", "failure", "skipped"), required=True)
    producer.add_argument("--source", type=Path, required=True)
    producer.add_argument("--out", type=Path, required=True)
    producer.add_argument("--build-manifest", type=Path)
    producer.add_argument("--snapshot-artifact-id")
    producer.set_defaults(handler=lambda args: stage(
        args.kind, args.producer, args.status, args.source, args.out,
        args.build_manifest, args.snapshot_artifact_id,
    ))

    report = commands.add_parser("collect")
    report.add_argument("--inputs", type=Path, required=True)
    report.add_argument("--out", type=Path, required=True)
    report.add_argument("--build-manifest", type=Path, required=True)
    report.add_argument("--snapshot-artifact-id", required=True)
    report.set_defaults(handler=lambda args: collect(
        args.inputs, args.out, args.build_manifest, args.snapshot_artifact_id,
    ))
