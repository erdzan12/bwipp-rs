#!/usr/bin/env bash
# Local web CI: build the Rust/WASM bundle that `web/` depends on, then
# typecheck + production-build the Next.js app.

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

require cargo "install via https://rustup.rs/"
require npm "install Node 18+"

WEB_DIR="$REPO_ROOT/web"
WASM_CRATE="$REPO_ROOT/rust/wasm"

[ -d "$WEB_DIR" ] || die "$WEB_DIR not found"
[ -f "$WASM_CRATE/Cargo.toml" ] || die "$WASM_CRATE/Cargo.toml not found"

section "wasm staleness guard (host-independent): committed bundle vs current source"
# We TRACK web/public/wasm/bwipp_wasm.wasm so the Vercel deploy needs no Rust
# toolchain. The wasm BINARY is not byte-reproducible across OS/arch, so we do
# NOT byte-diff it (that would false-fail on any host other than the one that
# built the committed copy). Instead we compare a fingerprint of the SOURCE that
# determines the bundle (scripts/wasm-srcsha.sh: rust/src + rust/wasm/src +
# manifests + the wasm crate lockfile) against the committed
# bwipp_wasm.wasm.srcsha256 sidecar. If source changed without refreshing the
# bundle, this fails — on ANY machine. Refresh with
# `npm --prefix web run build:wasm` (or scripts/build-wasm.sh), which rewrites
# the bundle AND the sidecar; commit both. (wasm *correctness* is verified
# separately by the wasm-pack tests in ci-golden.sh, built fresh from source;
# the wasm32 crate also builds in ci-rust.sh.)
WASM_BUNDLE="$WEB_DIR/public/wasm/bwipp_wasm.wasm"
WASM_SHA_FILE="${WASM_BUNDLE}.srcsha256"
[ -f "$WASM_BUNDLE" ] || die "committed wasm bundle missing: $WASM_BUNDLE (run scripts/build-wasm.sh)"
[ -f "$WASM_SHA_FILE" ] || die "missing wasm source fingerprint: $WASM_SHA_FILE (run scripts/build-wasm.sh and commit it)"
_cur_sha="$("$HERE/wasm-srcsha.sh")"
_committed_sha="$(cat "$WASM_SHA_FILE")"
if [ "$_cur_sha" != "$_committed_sha" ]; then
    die "web/public/wasm/bwipp_wasm.wasm is STALE: wasm source changed but the committed bundle was not refreshed (current source fingerprint $_cur_sha != committed $_committed_sha). Run 'npm --prefix web run build:wasm' (or scripts/build-wasm.sh) and commit the refreshed bwipp_wasm.wasm + .srcsha256."
fi
ok "committed wasm matches current source (size: $(wc -c < "$WASM_BUNDLE") bytes, fingerprint ${_cur_sha})"

cd "$WEB_DIR"

section "npm install (only if node_modules is missing)"
if [ ! -d node_modules ]; then
    run mexec npm install --no-audit --no-fund
else
    info "node_modules present; skipping npm install"
fi

section "Next.js typecheck"
run mexec npm run typecheck
ok "typecheck clean"

section "Next.js production build"
run mexec npm run build
ok "next build"

# Playwright browser tests. Strict mode (PUBLISH_STRICT=1) requires
# the suite to run — PLAYWRIGHT_SKIP=1 is rejected and missing devDep
# is fatal. Normal mode keeps the soft-skip behaviour so a contributor
# without Chromium installed can still get local CI green.
#
# Hosted GitHub Actions stays `workflow_dispatch`-only per project
# policy, so this step is local CI only.
if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
    if [ "${PLAYWRIGHT_SKIP:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: PLAYWRIGHT_SKIP=1 is rejected. Browser tests must run."
    fi
    if [ ! -d "$WEB_DIR/node_modules/@playwright/test" ]; then
        die "PUBLISH_STRICT=1: @playwright/test devDep missing. Run \`npm install\` in web/."
    fi
    section "Playwright browser tests (desktop + mobile, strict)"
    run mexec npx playwright install chromium >/dev/null 2>&1 \
        || die "PUBLISH_STRICT=1: playwright install failed; cannot proceed without Chromium."
    run mexec npx playwright test
    ok "playwright suite green (desktop-chromium + mobile-chromium)"
elif [ "${PLAYWRIGHT_SKIP:-0}" = "1" ]; then
    info "PLAYWRIGHT_SKIP=1 — skipping browser tests"
elif [ ! -d "$WEB_DIR/node_modules/@playwright/test" ]; then
    info "@playwright/test devDep absent — skipping browser tests (run \`npm install\` to enable)"
else
    section "Playwright browser tests (desktop + mobile)"
    # Auto-install Chromium binary if it isn't already cached. The CLI
    # short-circuits on cache hit.
    run mexec npx playwright install chromium >/dev/null 2>&1 \
        || info "playwright install returned non-zero; continuing — already-cached install is fine"
    run mexec npx playwright test
    ok "playwright suite green (desktop-chromium + mobile-chromium)"
fi

ok "Web CI passed"
