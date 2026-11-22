#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only

"""Package and restore the retention workflow's pre-test Cargo target."""

import argparse
import base64
import binascii
from contextlib import suppress
from decimal import Decimal
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import posixpath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile


COV_TARGET = "llvm-cov-target"
BUILD_MANIFEST = "build.json"
COPY_SIZE = 1024 * 1024
NANOSECONDS = 10**9
SOURCE_TABLE = re.compile(
    rb"""^\s*(?:\[\s*)?["']?(?:source|registries|registry|net|patch|paths)["']?(?=\s*[.\]=])"""
)
TOML_TABLE = re.compile(rb"^\s*\[")
CONFIG_INCLUDE = re.compile(rb"""^\s*(?:\[\[?\s*)?["']?include["']?(?=\s*[.\]=])""", re.M)
SOURCE_ENV = re.compile(
    r"CARGO_(?:(?:SOURCE|REGISTRIES)_.+_(?:INDEX|REGISTRY|REPLACE_WITH|PROTOCOL)"
    r"|REGISTRY_(?:INDEX|DEFAULT)|NET_GIT_FETCH_WITH_CLI)"
)

BASIC_TOKEN = re.compile(rb"\bBasic[ \t]+([A-Za-z0-9._~+/=-]+)", re.IGNORECASE)
CREDENTIAL_SHAPE = re.compile(
    rb"Bearer [A-Za-z0-9._~+/=-]+"
    rb"|(?:password|secret|token)[A-Za-z_]*[\"']?[=:] *[\"']?[^\"' ,}\s]+",
    re.IGNORECASE,
)
SOURCE_SUFFIXES = {".rs", ".h", ".c", ".cc", ".cpp", ".hpp"}
SOURCE_BEARER = re.compile(
    rb"\bauthorization[ \t\"']*[:=][ \t\"']*Bearer[ \t]+[A-Za-z0-9._~+/=-]+",
    re.IGNORECASE,
)
QUOTED_ASSIGNMENT = re.compile(
    rb"\b([A-Za-z_][A-Za-z0-9_]*)[\"']?[ \t]*"
    rb"(?:=|:(?:[^\r\n\";={}]{0,160}=)?)[ \t]*(?:[bcr]{1,2}#*)?[\"'][^\r\n\"']"
)
SENSITIVE_IDENTIFIER = re.compile(
    rb"(?i:(?:^|_)(?:password|secret|token)(?:$|_))"
    rb"|(?<=[a-z0-9])(?:Password|Secret|Token)(?=$|[A-Z_])"
    rb"|(?:^|_)(?:password|secret|token)(?=[A-Z])"
)
SOURCE_TOKEN = re.compile(
    rb'(?P<raw>r(?P<hash>#*)")|(?P<string>")'
    rb"|(?P<char>'(?:\\.|[^'\\\r\n])')|(?P<line>//)|(?P<block>/\*)"
)
STRING_END = re.compile(rb'(?:\\.|[^"\\\r\n])*"')
BLOCK_TOKEN = re.compile(rb"/\*|\*/")
BUILD_SCRIPT_PRODUCT = re.compile(
    r"build-script-[A-Za-z0-9_][A-Za-z0-9_-]*"
    r"|build_script_[A-Za-z0-9_]+-[0-9a-f]{16}(?:\.d)?"
)


def git(repo, *args):
    return subprocess.check_output(
        ["git", *args], cwd=repo, env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"}
    )


def normalize_checkout(repo):
    """Set the mtimes watched by the release build without changing their bytes."""
    if git(repo, "status", "--porcelain", "--untracked-files=no").strip():
        raise ValueError("the build checkout has tracked changes")
    timestamp = int(git(repo, "show", "-s", "--format=%ct")) * NANOSECONDS
    tracked = git(repo, "ls-files", "-z").split(b"\x00")
    paths = [repo / os.fsdecode(path) for path in tracked if path]
    for name in ("HEAD", "index", "refs", "packed-refs"):
        raw_path = git(repo, "rev-parse", "--path-format=absolute", "--git-path", name)
        path = Path(os.fsdecode(raw_path.rstrip(b"\n")))
        if path.exists():
            paths.append(path)
            if name == "refs":
                paths.extend(path.rglob("*"))
    for path in paths:
        os.utime(path, ns=(timestamp, timestamp), follow_symlinks=False)


def source_configuration(data):
    """Select source settings from standalone Cargo config files used by CI."""
    if CONFIG_INCLUDE.search(data):
        raise ValueError("Cargo config includes are unsupported for retention download cache keys")
    selected = []
    in_source_table = False
    any_table = False
    for line in data.splitlines():
        if TOML_TABLE.match(line):
            in_source_table = bool(SOURCE_TABLE.match(line))
            any_table = True
        if in_source_table or (not any_table and SOURCE_TABLE.match(line)):
            selected.append(line)
    return b"\n".join(selected)


def cache_key(repo, lane, triple):
    hasher = hashlib.sha256()
    hasher.update((f"{platform.system()}\0{platform.machine()}\0{triple}\0").encode())
    hasher.update((repo / "Cargo.lock").read_bytes())
    cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    profiles = [*(path / ".cargo" for path in (repo, *repo.parents)), cargo_home]
    for directory in dict.fromkeys(profiles):
        # Cargo uses the legacy file when both names are present.
        config = directory / "config"
        if not config.is_file():
            config = directory / "config.toml"
        if config.is_file():
            source = source_configuration(config.read_bytes())
            if source:
                hasher.update(b"\0" + source)
    for name, value in sorted(os.environ.items()):
        if SOURCE_ENV.fullmatch(name):
            hasher.update(os.fsencode(f"\0{name}={value}"))
    return f"retention-download-v1-{lane}-{hasher.hexdigest()}"


def allowed(parts):
    """Permit the Cargo test build's debug tree without incremental or test state."""
    if not parts or parts[0] != COV_TARGET:
        return False
    if any(
        part.startswith("_step_")
        or part.endswith((".profraw", ".profdata", ".log"))
        or part in {"logs", "steps", "receipts", "test-stores", "stores", ".run"}
        for part in parts
    ):
        return False
    if len(parts) == 1:
        return True
    if parts[1] != "debug":
        return len(parts) == 2 and parts[1] in {".rustc_info.json", "CACHEDIR.TAG"}
    if len(parts) == 2:
        return True
    group = parts[2]
    if group == ".fingerprint":
        # Cargo can replay rustc diagnostics from output-<target> files, but it
        # does not need them to decide whether the compiled unit is fresh.
        return len(parts) <= 4 or not parts[4].startswith("output-")
    if group in {"deps", "examples"}:
        return True
    if group == "build":
        return (
            len(parts) <= 4
            or parts[4] == "out"
            or len(parts) == 5 and (
                parts[4] in {"output", "root-output", "invoked.timestamp"}
                or BUILD_SCRIPT_PRODUCT.fullmatch(parts[4]) is not None
            )
        )
    # Named Cargo bin outputs and their dep-info at the profile root, without subdirectories.
    return len(parts) == 3 and re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]*(?:\.d)?", group) is not None


def is_text_state(path):
    return (
        path.name in {"output", "root-output"}
        or path.suffix in SOURCE_SUFFIXES
        or path.suffix in {".json", ".toml", ".d", ".txt", ".cmake"}
    )


def source_code_lines(lines, nested_blocks):
    """Yield code and literal fragments while tracking comments and strings across lines."""
    depth = 0
    string_end = None
    for line in lines:
        offset = 0
        code = []
        literals = []
        while offset < len(line):
            if string_end is not None:
                closing = string_end.match(line, offset)
                end = closing.end() if closing else len(line)
                fragment = line[offset:end]
                code.append(fragment)
                literals.append(fragment)
                offset = end
                if closing:
                    string_end = None
                continue
            if depth:
                end = BLOCK_TOKEN.search(line, offset)
                if end is None:
                    break
                if end[0] == b"*/":
                    depth -= 1
                elif nested_blocks:
                    depth += 1
                offset = end.end()
                continue
            token = SOURCE_TOKEN.search(line, offset)
            if token is None:
                code.append(line[offset:])
                break
            code.append(line[offset:token.start()])
            offset = token.end()
            if token.lastgroup == "line":
                break
            if token.lastgroup == "block":
                depth = 1
                code.append(b" ")
            else:
                code.append(token[0])
                if token["raw"] is not None:
                    string_end = re.compile(rb'[^\r\n]*?"' + token["hash"])
                elif token["string"] is not None:
                    string_end = STRING_END
        yield line, b"".join(code), literals


def has_credential_shape(line, code=None, literals=()):
    if code is not None:
        # Generated source commonly contains token type declarations and integer
        # constants. Look for sensitive names assigned nonempty string literals.
        if any(CREDENTIAL_SHAPE.search(literal) for literal in literals) or SOURCE_BEARER.search(line):
            return True
        if any(
            SENSITIVE_IDENTIFIER.search(assignment[1])
            for assignment in QUOTED_ASSIGNMENT.finditer(code)
        ):
            return True
    elif CREDENTIAL_SHAPE.search(line):
        return True
    for candidate in BASIC_TOKEN.finditer(line):
        token = candidate[1]
        unpadded = token.rstrip(b"=")
        try:
            user_password = base64.b64decode(unpadded + b"=" * (-len(unpadded) % 4), validate=True)
        except binascii.Error:
            continue
        canonical = base64.b64encode(user_password)
        if b":" in user_password and token in (canonical, canonical.rstrip(b"=")):
            return True
    return False


def credential_match(path, credential_values):
    if is_text_state(path):
        with path.open("rb") as source:
            if path.suffix in SOURCE_SUFFIXES:
                lines = source_code_lines(source, nested_blocks=path.suffix == ".rs")
                found = any(has_credential_shape(line, code, literals) for line, code, literals in lines)
            else:
                found = any(has_credential_shape(line) for line in source)
            if found:
                return "credential pattern"
    if credential_values:
        overlap = max(map(len, credential_values)) - 1
        with path.open("rb") as source:
            tail = b""
            for chunk in iter(lambda: source.read(COPY_SIZE), b""):
                data = tail + chunk
                if any(value in data for value in credential_values):
                    return "literal credential"
                tail = data[-overlap:] if overlap else b""
    return None


def check_snapshot_credentials(files, credential_values):
    limit = 20
    count = 0
    findings = []
    for path, name in files:
        match = credential_match(path, credential_values)
        if match is None:
            continue
        count += 1
        if len(findings) < limit:
            label = os.fsencode(name)
            for value in credential_values:
                label = label.replace(value, b"[redacted]")
            findings.append(f"{match} in {os.fsdecode(label)}")
    if count > limit:
        findings.append(f"{count - limit} additional flagged files omitted")
    if findings:
        raise ValueError("\n".join(findings))


def check_cargo_state(tree):
    debug = tree / COV_TARGET / "debug"
    required = (debug / ".fingerprint", debug / "deps", debug / "build")
    if not all(path.is_dir() and not path.is_symlink() for path in required):
        raise ValueError("missing Cargo debug, fingerprint, dependency or build-script state")
    found_output = False
    for output_dir in (debug / "build").iterdir():
        if not output_dir.is_dir() or output_dir.is_symlink():
            continue
        output = output_dir / "output"
        root_output = output_dir / "root-output"
        out = output_dir / "out"
        if any(path.exists() or path.is_symlink() for path in (output, root_output, out)):
            if not (
                output.is_file() and not output.is_symlink()
                and root_output.is_file() and not root_output.is_symlink()
                and out.is_dir() and not out.is_symlink()
            ):
                raise ValueError(f"incomplete Cargo build-script state for {output_dir.name}")
            found_output = True
    if not found_output:
        raise ValueError("missing Cargo build-script output")


def relative_path(name):
    path = PurePosixPath(name)
    if (
        name != path.as_posix()
        or path.is_absolute()
        or ".." in path.parts
        or not path.parts
        or "\\" in name
        or any(ord(char) < 32 for char in name)
    ):
        raise ValueError("invalid archive member path")
    return path


def link_destination(member, entry):
    if member.islnk():
        return relative_path(member.linkname)
    target = PurePosixPath(member.linkname)
    if target.is_absolute() or not target.parts or "\\" in member.linkname:
        raise ValueError(f"invalid symlink in {entry}")
    return relative_path(posixpath.normpath(str(entry.parent / target)))


def build_members(target, path):
    relative = relative_path(path.relative_to(target).as_posix())
    if not allowed(relative.parts):
        return
    mode = path.lstat().st_mode
    if stat.S_ISDIR(mode) and len(relative.parts) == 3 and relative.parts[2] not in {
        ".fingerprint", "build", "deps", "examples",
    }:
        return
    yield path, relative, stat.S_ISREG(mode)
    if stat.S_ISDIR(mode):
        for child in sorted(path.iterdir()):
            yield from build_members(target, child)


def add_target(archive, path, relative):
    info = archive.gettarinfo(str(path), relative.as_posix())
    if info is None or not (info.isfile() or info.isdir() or info.issym() or info.islnk()):
        raise ValueError(f"unsupported build artifact at {relative}")
    if info.issym() or info.islnk():
        dest = link_destination(info, relative)
        if not allowed(dest.parts):
            raise ValueError(f"link outside build state at {relative}")
    info.pax_headers["mtime"] = format(Decimal(path.lstat().st_mtime_ns).scaleb(-9), "f")
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mode &= 0o777
    if info.isfile():
        with path.open("rb") as source:
            archive.addfile(info, source)
    else:
        archive.addfile(info)


def inspect_manifest(manifest):
    if manifest.is_symlink() or not manifest.is_file():
        raise ValueError("build manifest must be a regular file")
    if not isinstance(json.loads(manifest.read_bytes()), dict):
        raise ValueError("invalid build manifest")


def require_external_output(path, target, label):
    output = path.resolve()
    target = target.resolve()
    if output == target or target in output.parents:
        raise ValueError(f"{label} must be outside the Cargo target")


def pack_build(manifest, target, destination, credentials):
    require_external_output(destination, target, "snapshot archive")
    inspect_manifest(manifest)
    check_cargo_state(target)
    if destination.exists() or destination.is_symlink():
        raise ValueError("snapshot archive already exists")
    members = list(build_members(target, target / COV_TARGET))
    files = [(manifest, BUILD_MANIFEST)]
    files.extend((path, relative.as_posix()) for path, relative, regular in members if regular)
    check_snapshot_credentials(files, credentials)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as scratch:
        pending = Path(scratch) / "snapshot.tar.zst"
        with pending.open("wb") as compressed:
            with subprocess.Popen(
                ["zstd", "-q", "-3", "-T2", "-c"], stdin=subprocess.PIPE, stdout=compressed
            ) as encoder:
                try:
                    with tarfile.open(
                        fileobj=encoder.stdin, mode="w|", format=tarfile.PAX_FORMAT,
                        bufsize=COPY_SIZE, copybufsize=COPY_SIZE,
                    ) as archive:
                        archive.add(str(manifest), arcname=BUILD_MANIFEST, recursive=False)
                        for path, relative, _regular in members:
                            add_target(archive, path, relative)
                except BaseException:
                    encoder.kill()
                    # A broken pipe while flushing after kill must not hide the pack error.
                    with suppress(BrokenPipeError):
                        encoder.stdin.close()
                    raise
                encoder.stdin.close()
                if encoder.wait() != 0:
                    raise ValueError("snapshot compression failed")
        os.replace(pending, destination)


def extract_snapshot(archive, directory):
    """Extract only supported members. A symlink can never be an entry's parent."""
    seen = {}
    directories = []
    symlinks = []
    for member in archive:
        name = relative_path(member.name)
        if name in seen:
            raise ValueError(f"duplicate snapshot entry {name}")
        is_manifest = name == PurePosixPath(BUILD_MANIFEST)
        if (is_manifest and not member.isfile()) or (not is_manifest and not allowed(name.parts)):
            raise ValueError(f"unexpected snapshot entry {name}")
        if len(name.parts) > 1 and (
            name.parent not in seen or not seen[name.parent].isdir()
        ):
            raise ValueError(f"invalid parent of snapshot entry {name}")
        path = directory / name
        mode = member.mode & 0o777
        mtime = int(Decimal(member.pax_headers.get("mtime", str(member.mtime))) * NANOSECONDS)
        if member.isdir():
            path.mkdir()
            directories.append((path, mode, mtime))
        elif member.isfile():
            with archive.extractfile(member) as source, path.open("xb") as target:
                shutil.copyfileobj(source, target, COPY_SIZE)
            path.chmod(mode)
            os.utime(path, ns=(mtime, mtime))
        elif member.islnk():
            target = link_destination(member, name)
            if target not in seen or not seen[target].isfile() or str(target) == BUILD_MANIFEST:
                raise ValueError(f"invalid hardlink in {name}")
            os.link(directory / target, path)
        elif member.issym():
            target = link_destination(member, name)
            if not allowed(target.parts):
                raise ValueError(f"symlink outside build state in {name}")
            path.symlink_to(member.linkname)
            os.utime(path, ns=(mtime, mtime), follow_symlinks=False)
            symlinks.append(path)
        else:
            raise ValueError(f"unsupported snapshot entry {name}")
        seen[name] = member
    target_root = directory.resolve() / COV_TARGET
    for path in symlinks:
        if target_root not in path.resolve(strict=True).parents:
            raise ValueError("snapshot symlink resolves outside the Cargo target")
    for path, mode, mtime in reversed(directories):
        path.chmod(mode)
        os.utime(path, ns=(mtime, mtime))
    inspect_manifest(directory / BUILD_MANIFEST)
    check_cargo_state(directory)


def decompress_snapshot(archive, staged):
    with subprocess.Popen(["zstd", "-q", "-d", "-c", "--", str(archive)], stdout=subprocess.PIPE) as decoder:
        try:
            with tarfile.open(fileobj=decoder.stdout, mode="r|", bufsize=COPY_SIZE) as entries:
                extract_snapshot(entries, staged)
            while decoder.stdout.read(COPY_SIZE):
                pass
        except BaseException:
            decoder.kill()
            raise
        finally:
            decoder.stdout.close()
        if decoder.wait() != 0:
            raise ValueError("snapshot decompression failed")


def unpack_build(archive, target, manifest_out):
    require_external_output(manifest_out, target, "manifest output")
    create_target = not target.exists()
    if target.is_symlink() or (not create_target and not (target.is_dir() and not any(target.iterdir()))):
        raise ValueError("snapshot target must be a fresh directory")
    if manifest_out.exists() or manifest_out.is_symlink():
        raise ValueError("manifest output already exists")
    if create_target:
        target.mkdir(parents=True)
    try:
        # The runner owns the target, but its parent (typically /mnt) may be read-only.
        with tempfile.TemporaryDirectory(dir=target) as scratch:
            staged = Path(scratch)
            decompress_snapshot(archive, staged)
            manifest_out.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(dir=manifest_out.parent) as manifest_staging:
                staged_manifest = Path(manifest_staging) / BUILD_MANIFEST
                shutil.copyfile(staged / BUILD_MANIFEST, staged_manifest)
                published_build = target / COV_TARGET
                os.replace(staged / COV_TARGET, published_build)
                try:
                    os.replace(staged_manifest, manifest_out)
                except BaseException:
                    os.replace(published_build, staged / COV_TARGET)
                    raise
    except BaseException:
        if create_target and not any(target.iterdir()):
            target.rmdir()
        raise


def register_snapshot_commands(commands):
    normalize = commands.add_parser("normalize-checkout")
    normalize.add_argument("--repo", type=Path, default=Path.cwd())
    normalize.set_defaults(handler=lambda args: normalize_checkout(args.repo.resolve()))
    key = commands.add_parser("cache-key")
    key.add_argument("--repo", type=Path, default=Path.cwd())
    key.add_argument("--lane", choices=("instrumented", "ordinary"), required=True)
    key.add_argument("--target-triple", required=True)
    key.set_defaults(handler=lambda args: print(cache_key(args.repo.resolve(), args.lane, args.target_triple)))
    pack = commands.add_parser("pack-build")
    pack.add_argument("--manifest", type=Path, required=True)
    pack.add_argument("--target", type=Path, required=True)
    pack.add_argument("--out", type=Path, required=True)
    pack.add_argument("--credential-env", action="append", default=[])
    pack.set_defaults(handler=lambda args: pack_build(
        args.manifest, args.target, args.out,
        [os.fsencode(os.environ[name]) for name in args.credential_env if os.environ.get(name)],
    ))
    unpack = commands.add_parser("unpack-build")
    unpack.add_argument("--archive", type=Path, required=True)
    unpack.add_argument("--target", type=Path, required=True)
    unpack.add_argument("--manifest-out", type=Path, required=True)
    unpack.set_defaults(handler=lambda args: unpack_build(args.archive, args.target, args.manifest_out))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(required=True)
    register_snapshot_commands(commands)
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from retention_evidence import register_evidence_commands

    register_evidence_commands(commands)
    args = parser.parse_args(argv)
    try:
        return args.handler(args) or 0
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"retention artifacts: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
