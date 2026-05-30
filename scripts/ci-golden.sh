#!/usr/bin/env bash
# Local golden-fixture verification. The bulk of golden tests live inside
# the Rust crate as `#[test]` cases that assert against `tests/fixtures/`
# and inline oracle constants. This script:
#
#   * Re-runs the Rust test suite while emitting human-friendly names for
#     every test, so reviewers can confirm the verified-claim coverage.
#   * Builds and runs the wasm-bindgen integration tests under Node via
#     wasm-pack when available, so the JS-facing API is also verified.
#
# Designed to be re-runnable both locally and from GitHub Actions later.

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

require cargo "install via https://rustup.rs/"

cd "$REPO_ROOT/rust"

section "Logical golden tests (rust crate)"
run mexec cargo test --quiet --all-features
ok "rust golden suite passed"

section "Catalog reachability"
run mexec cargo test --quiet --all-features -- catalog
ok "catalog reachable end-to-end"

if command -v wasm-pack >/dev/null 2>&1; then
    section "WASM logical tests (wasm-pack test --node)"
    if ! mexec rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
        info "installing wasm32-unknown-unknown target via rustup"
        run mexec rustup target add wasm32-unknown-unknown
    fi
    run mexec wasm-pack test --node -- --no-default-features --features wasm
    ok "wasm logical tests passed"
elif [ "${PUBLISH_STRICT:-0}" = "1" ]; then
    die "PUBLISH_STRICT=1: wasm-pack is required but not on PATH. Install with: cargo install --locked wasm-pack"
else
    warn "wasm-pack not on PATH; skipping wasm-bindgen tests"
    info "install with: cargo install --locked wasm-pack"
fi

ok "Golden verification passed"
