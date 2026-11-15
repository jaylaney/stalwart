#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only

set -euo pipefail

install_tools() {
    local lane="$1"
    local asset=cargo-llvm-cov-x86_64-unknown-linux-gnu.tar.gz
    rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal --no-self-update
    test "$(rustc -vV | sed -n 's/^host: //p')" = "$RETENTION_TARGET"
    if [[ "$lane" == instrumented ]]; then
        rustup component add --toolchain "$RUSTUP_TOOLCHAIN" llvm-tools-preview
        command -v zstd
        mkdir -p "$GATES/bin"
        curl --fail --silent --show-error --location --retry 2 --max-time 90 \
            "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v0.9.1/$asset" \
            --output "$GATES/$asset"
        printf '%s  %s\n' \
            b3f68e625481fed9b16444174f3fa5ebcdbde4a1878803a35eabe2dcefcdc41a \
            "$GATES/$asset" | sha256sum --check --strict
        tar -xzf "$GATES/$asset" -C "$GATES/bin" cargo-llvm-cov
        printf '%s\n' "$GATES/bin" >> "$GITHUB_PATH"
    fi

    printf 'Runner image: %s %s\n' "${ImageOS:-unknown}" "${ImageVersion:-unknown}"
    rustc -vV
    cargo -V
    cc --version
    ld --version
    ldd --version
    if [[ "$lane" == instrumented ]]; then
        PATH="$GATES/bin:$PATH" cargo llvm-cov --version
        "$(rustc --print target-libdir)/../bin/llvm-cov" --version
        "$(rustc --print target-libdir)/../bin/llvm-profdata" --version
    fi
}

within_budget() {
    local remaining
    remaining=$((RETENTION_DEADLINE_EPOCH - $(date +%s)))
    if (( remaining <= 0 )); then
        printf '%s\n' 'Retention work deadline already reached' >&2
        return 124
    fi
    timeout --kill-after=30s "$remaining" "$@"
}

download_paths_bytes() {
    local path sum=0
    for path in "$CARGO_HOME/registry/cache" "$CARGO_HOME/registry/index" "$CARGO_HOME/git/db"; do
        if [[ -d "$path" ]]; then
            sum=$((sum + $(du -sb "$path" | cut -f1)))
        fi
    done
    printf '%s\n' "$sum"
}

cache_budget() {
    local used limit bytes
    # Reserve headroom for both scheduled lanes because they can save concurrently.
    # Size the reservation from uncompressed inputs and leave room for archive overhead.
    bytes=$(download_paths_bytes)
    if ! used=$(gh api "repos/$GITHUB_REPOSITORY/actions/cache/usage" \
        --jq '.active_caches_size_in_bytes') ||
       ! limit=$(gh api "repos/$GITHUB_REPOSITORY/actions/cache/storage-limit" \
        --jq '.max_cache_size_gb') ||
       [[ ! "$used" =~ ^[0-9]+$ || ! "$limit" =~ ^[1-9][0-9]*$ ]]; then
        printf '%s\n' 'Cache usage or limit unavailable. Skip this optional save.'
        return
    fi
    limit=$((limit * 1000 * 1000 * 1000))
    printf 'Cache bytes: %s used of %s, %s for this lane before compression\n' \
        "$used" "$limit" "$bytes" | tee -a "$GITHUB_STEP_SUMMARY"
    if (( bytes > 0 && used + 2 * (bytes + 32 * 1024 * 1024) <= limit )); then
        printf '%s\n' 'room=true' >> "$GITHUB_OUTPUT"
    else
        printf '%s\n' 'Insufficient cache room for both download scopes. Skip this optional save.'
    fi
}

redact() {
    sed -E \
        -e 's/(Basic|Bearer) [A-Za-z0-9._~+\/=-]+/\1 [REDACTED]/Ig' \
        -e 's/((password|secret|token)[A-Za-z_]*["\x27]?[=:] *["\x27]?)[^"\x27 ,}[:space:]]+/\1[REDACTED]/Ig' \
        "$1" > "$2"
}

stage_logs() {
    local destination="$1" file
    shift
    mkdir -p "$destination"
    for file in "$@"; do
        [[ -f "$file" ]] || continue
        redact "$file" "$destination/$(basename "$file")"
    done
}

case "$1" in
    install-tools)
        install_tools "$2"
        ;;
    within-budget)
        shift
        within_budget "$@"
        ;;
    fetch)
        time within_budget cargo fetch --locked --target "$RETENTION_TARGET"
        time within_budget cargo metadata --offline --locked --format-version 1 \
            --filter-platform "$RETENTION_TARGET" > /dev/null
        printf 'Dependency download paths: %s bytes\n' "$(download_paths_bytes)" \
            | tee -a "$GITHUB_STEP_SUMMARY"
        df -h "$GATES"
        ;;
    cache-budget)
        cache_budget
        ;;
    stage-logs)
        shift
        stage_logs "$@"
        ;;
    *)
        printf 'Unknown retention setup operation: %s\n' "$1" >&2
        exit 2
        ;;
esac
