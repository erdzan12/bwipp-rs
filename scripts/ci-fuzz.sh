#!/usr/bin/env bash
# Local fuzz CI: a short coverage-guided smoke pass over every
# cargo-fuzz target in rust/fuzz.
#
# cargo-fuzz requires a nightly toolchain (libFuzzer + sanitizers). It
# is OPT-IN: this gate runs only when BOTH the nightly toolchain and
# the `cargo-fuzz` binary are present, so contributors without them
# don't hit a hard failure. `PUBLISH_STRICT=1` (from ci-local.sh)
# upgrades that to a required gate — a missing prerequisite is then a
# strict-mode failure.
#
# Install once with:
#   rustup install nightly
#   rustup component add --toolchain nightly rust-src llvm-tools-preview
#   cargo +nightly install cargo-fuzz
#
# Each target is fuzzed for FUZZ_SECONDS (default 30) under a per-run
# RSS cap of FUZZ_RSS_MB (default 2048) — libFuzzer's own
# `-rss_limit_mb` aborts a target that blows the cap, so no external
# cgroup is needed and the gate stays portable across CI environments.
# Up to FUZZ_JOBS (default 4) targets run concurrently. Any crash,
# panic, sanitizer finding, OOM, or timeout fails the gate and the
# reproducer is left under rust/fuzz/artifacts/<target>/.
#
# Knobs (env): FUZZ_SECONDS, FUZZ_RSS_MB, FUZZ_JOBS, FUZZ_ONLY (space-
# separated subset of target names).

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

section "cargo fuzz (libFuzzer smoke pass)"

FUZZ_SECONDS="${FUZZ_SECONDS:-30}"
FUZZ_RSS_MB="${FUZZ_RSS_MB:-2048}"
FUZZ_JOBS="${FUZZ_JOBS:-4}"

# --- prerequisite detection ------------------------------------------
missing=""
if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
    missing="nightly toolchain"
elif ! rustup component list --toolchain nightly 2>/dev/null \
        | grep -q '^rust-src.*(installed)'; then
    missing="rust-src (nightly component)"
elif ! command -v cargo-fuzz >/dev/null 2>&1; then
    missing="cargo-fuzz"
fi

if [ -n "$missing" ]; then
    if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: $missing not available. Install with:
       rustup install nightly
       rustup component add --toolchain nightly rust-src llvm-tools-preview
       cargo +nightly install cargo-fuzz"
    fi
    info "$missing not available — skipping fuzz smoke (PUBLISH_STRICT=1 would require it)."
    exit 0
fi

cd "$REPO_ROOT/rust"

# --- enumerate targets ------------------------------------------------
# Portable across bash 3.2 (stock macOS) and bash 4+; `mapfile`/`readarray`
# are bash 4+ only and abort with "command not found" on macOS's default shell.
ALL_TARGETS=()
while IFS= read -r _fuzz_target; do
    [ -n "$_fuzz_target" ] && ALL_TARGETS+=("$_fuzz_target")
done < <(cargo +nightly fuzz list 2>/dev/null)
if [ "${#ALL_TARGETS[@]}" -eq 0 ]; then
    die "no fuzz targets found under rust/fuzz — expected fuzz_target_1 + per-encoder targets"
fi

# Optional subset filter.
if [ -n "${FUZZ_ONLY:-}" ]; then
    TARGETS=()
    for t in "${ALL_TARGETS[@]}"; do
        for want in $FUZZ_ONLY; do
            [ "$t" = "$want" ] && TARGETS+=("$t")
        done
    done
else
    TARGETS=("${ALL_TARGETS[@]}")
fi

info "fuzzing ${#TARGETS[@]} target(s) — ${FUZZ_SECONDS}s each, RSS cap ${FUZZ_RSS_MB}MB, up to ${FUZZ_JOBS} concurrent"

# --- build all targets once (shared lib compiled once) ----------------
section "  → cargo +nightly fuzz build"
run cargo +nightly fuzz build

# --- run pass with bounded concurrency --------------------------------
STATUS_DIR="$(mktemp -d)"
trap 'rm -rf "$STATUS_DIR"' EXIT

run_one() {
    local target="$1"
    local log="$STATUS_DIR/$target.log"
    if cargo +nightly fuzz run "$target" -- \
            -max_total_time="$FUZZ_SECONDS" \
            -rss_limit_mb="$FUZZ_RSS_MB" \
            >"$log" 2>&1; then
        echo "ok" >"$STATUS_DIR/$target.status"
    else
        echo "FAIL" >"$STATUS_DIR/$target.status"
    fi
}

active=0
for target in "${TARGETS[@]}"; do
    run_one "$target" &
    active=$((active + 1))
    if [ "$active" -ge "$FUZZ_JOBS" ]; then
        wait -n 2>/dev/null || true
        active=$((active - 1))
    fi
done
wait

# --- collect results --------------------------------------------------
failed=()
for target in "${TARGETS[@]}"; do
    st="$(cat "$STATUS_DIR/$target.status" 2>/dev/null || echo MISSING)"
    if [ "$st" = "ok" ]; then
        runs="$(grep -oE 'Done [0-9]+ runs' "$STATUS_DIR/$target.log" 2>/dev/null | tail -1 || true)"
        ok "  $target — clean (${runs:-completed})"
    else
        failed+=("$target")
        warn "  $target — FAILED:"
        tail -20 "$STATUS_DIR/$target.log" 2>/dev/null | sed 's/^/      /' || true
    fi
done

if [ "${#failed[@]}" -gt 0 ]; then
    die "fuzz smoke FAILED for: ${failed[*]}
       reproducers under rust/fuzz/artifacts/<target>/ — reproduce with:
       cargo +nightly fuzz run <target> rust/fuzz/artifacts/<target>/<crash-file>"
fi

ok "all ${#TARGETS[@]} fuzz target(s) survived ${FUZZ_SECONDS}s smoke"
