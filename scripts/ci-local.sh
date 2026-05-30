#!/usr/bin/env bash
# Local Mac CI orchestrator. Runs every gate the project ships behind:
#
#   1. Rust formatter, clippy, tests (lib/integration/doc), doc warnings,
#      release build, wasm32 build, raw-pointer wasm crate build, and
#      cargo publish --dry-run.
#   2. Golden-fixture verification (mostly inline cargo tests, plus
#      wasm-bindgen tests via wasm-pack when installed).
#   3. Web app: Rust/WASM bundle build + Next.js typecheck + production
#      build.
#
# Source-of-truth for "the project still works locally". GitHub Actions,
# when re-enabled, calls into this same script.
#
# Set `PUBLISH_STRICT=1` to run the full publish-readiness gate (no
# `--allow-dirty`, wasm-pack required, Playwright required,
# `PLAYWRIGHT_SKIP=1` rejected). Use this before tagging a release:
#
#     PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh
#
# In the default mode (PUBLISH_STRICT unset or != "1"), wasm-pack and
# Playwright are best-effort and soft-skip if absent; cargo publish
# dry-run runs with `--allow-dirty` to tolerate in-progress edits.

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

cd "$REPO_ROOT"

# Tell require() in nested ci-*.sh scripts that the user-invoked
# top-level entry point is this file, not whichever nested script
# happens to be running when the require() fires. The hint that
# falls out of require() then reads:
#
#   Rerun via: mise exec -- ./scripts/ci-local.sh
#
# rather than the previous, less-actionable
#
#   Rerun via: mise exec -- ./scripts/ci-rust.sh
export CI_ENTRY_SCRIPT="${BASH_SOURCE[0]}"

START_TS=$(date +%s)

if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
    info "PUBLISH_STRICT=1 — strict publish-readiness gate enabled."
fi

section "scripts/ci-rust.sh"
"$HERE/ci-rust.sh"

section "scripts/ci-msrv.sh"
"$HERE/ci-msrv.sh"

section "scripts/ci-inventory.sh"
"$HERE/ci-inventory.sh"

section "scripts/ci-security.sh"
"$HERE/ci-security.sh"

section "scripts/ci-golden.sh"
"$HERE/ci-golden.sh"

section "scripts/ci-fuzz.sh"
"$HERE/ci-fuzz.sh"

section "scripts/ci-web.sh"
"$HERE/ci-web.sh"

END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))

ok "All local CI passed (${ELAPSED}s)"
