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
    local path bytes sum=0
    for path in "$CARGO_HOME/registry/cache" "$CARGO_HOME/registry/index" "$CARGO_HOME/git/db"; do
        if [[ -d "$path" ]]; then
            if ! bytes=$(du -sb "$path" | cut -s -f1); then
                printf 'Could not measure dependency download path %s\n' "$path" >&2
                return 1
            fi
            # Bound each of the three counts before adding them in Bash.
            if [[ ! "$bytes" =~ ^(0|[1-9][0-9]{0,17})$ ]]; then
                printf 'Invalid byte count for dependency download path %s\n' "$path" >&2
                return 1
            fi
            sum=$((sum + bytes))
        fi
    done
    printf '%s\n' "$sum"
}

cache_budget() {
    # The verified fork quota and GitHub's default are both 10 GB.
    local limit=10000000000 used bytes
    # Reserve headroom for both scheduled lanes because they can save concurrently.
    # Size the reservation from uncompressed inputs and leave room for archive overhead.
    if ! bytes=$(download_paths_bytes); then
        printf '%s\n' 'Cache download size measurement failed. Skip this optional save.'
        return
    fi
    if ! used=$(gh api "repos/$GITHUB_REPOSITORY/actions/cache/usage" \
        --jq '.active_caches_size_in_bytes'); then
        printf '%s\n' 'Cache usage request failed. Skip this optional save.'
        return
    fi
    if [[ ! "$used" =~ ^(0|[1-9][0-9]*)$ ]]; then
        printf '%s\n' 'Cache usage response is invalid. Skip this optional save.'
        return
    fi
    printf 'Cache bytes: %s used of %s, %s for this lane before compression\n' \
        "$used" "$limit" "$bytes" | tee -a "$GITHUB_STEP_SUMMARY"
    # Values longer than the budget are already too large and could overflow Bash arithmetic.
    if (( ${#used} <= ${#limit} && ${#bytes} <= ${#limit} &&
          bytes > 0 && used + 2 * (bytes + 32 * 1024 * 1024) <= limit )); then
        printf '%s\n' 'room=true' >> "$GITHUB_OUTPUT"
    else
        printf '%s\n' 'Insufficient cache room for both download scopes. Skip this optional save.'
    fi
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
        if bytes=$(download_paths_bytes); then
            printf 'Dependency download paths: %s bytes\n' "$bytes" | tee -a "$GITHUB_STEP_SUMMARY"
        else
            printf '%s\n' 'Dependency download path sizes are unavailable.' | tee -a "$GITHUB_STEP_SUMMARY"
        fi
        df -h "$GATES"
        ;;
    cache-budget)
        cache_budget
        ;;
    stage-logs)
        shift
        exec python3 -I "$(dirname "$0")/retention_artifacts.py" stage-logs -- "$@"
        ;;
    *)
        printf 'Unknown retention setup operation: %s\n' "$1" >&2
        exit 2
        ;;
esac
