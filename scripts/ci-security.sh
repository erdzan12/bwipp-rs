#!/usr/bin/env bash
# Local security CI: cargo audit on both Rust lockfiles.
#
# `cargo audit` is OPT-IN — this gate runs only when the binary is on
# PATH, so contributors and CI environments without it installed
# don't see a hard failure. The `PUBLISH_STRICT=1` mode in
# ci-local.sh upgrades that to a required gate: missing
# `cargo audit` is a strict-mode failure.
#
# Install once with: `cargo install cargo-audit --locked`

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

section "cargo audit (security advisories)"

if ! command -v cargo-audit >/dev/null 2>&1; then
    if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: cargo-audit not installed. Install with: cargo install cargo-audit --locked"
    fi
    info "cargo-audit not installed — skipping (install with: cargo install cargo-audit --locked)"
    info "(PUBLISH_STRICT=1 would require it.)"
    exit 0
fi

cd "$REPO_ROOT/rust"
section "  → rust/Cargo.lock"
run cargo audit
ok "rust/Cargo.lock: no advisories"

cd "$REPO_ROOT/rust/wasm"
section "  → rust/wasm/Cargo.lock"
run cargo audit
ok "rust/wasm/Cargo.lock: no advisories"

# cargo deny check — licenses, sources, advisories, version bans.
# Same opt-in / strict-mode-required policy as cargo-audit.
section "cargo deny check (license + source + duplicates)"
if ! command -v cargo-deny >/dev/null 2>&1; then
    if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: cargo-deny not installed. Install with: cargo install cargo-deny --locked"
    fi
    info "cargo-deny not installed — skipping (install with: cargo install cargo-deny --locked)"
else
    cd "$REPO_ROOT/rust"
    run cargo deny --manifest-path Cargo.toml check
    ok "cargo deny: licenses + sources + advisories + bans clean"
fi

ok "ci-security.sh complete"
