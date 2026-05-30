#!/usr/bin/env bash
# Local MSRV CI: build + lib-test under the declared `rust-version`
# from `rust/Cargo.toml`. Locks the published crate's promised
# minimum supported Rust version against accidental usage of newer
# language / std-lib features.
#
# `rustup` toolchain install is OPT-IN — this gate runs only when
# the pinned toolchain is on the user's machine. The
# `PUBLISH_STRICT=1` mode in `ci-local.sh` upgrades that to a
# required gate: missing toolchain is a strict-mode failure.
#
# Install once with:
#   rustup toolchain install $(grep '^rust-version' rust/Cargo.toml \
#     | head -1 | cut -d '"' -f 2)

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

# Parse the MSRV from Cargo.toml.
MSRV="$(grep '^rust-version' "$REPO_ROOT/rust/Cargo.toml" | head -1 | cut -d '"' -f 2)"
if [ -z "$MSRV" ]; then
    die "no rust-version field in rust/Cargo.toml"
fi

section "MSRV check (rust $MSRV from Cargo.toml)"

if ! command -v rustup >/dev/null 2>&1; then
    if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: rustup not installed; install via https://rustup.rs/"
    fi
    info "rustup not installed — skipping MSRV gate"
    exit 0
fi

if ! rustup toolchain list 2>/dev/null | grep -q "^${MSRV}"; then
    if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
        die "PUBLISH_STRICT=1: rust $MSRV toolchain not installed. Install with: rustup toolchain install ${MSRV}"
    fi
    info "rust $MSRV toolchain not installed — skipping (install with: rustup toolchain install $MSRV)"
    exit 0
fi

cd "$REPO_ROOT/rust"

section "  → cargo +$MSRV build --all-features"
run cargo "+$MSRV" build --all-features --quiet
ok "msrv $MSRV builds clean"

section "  → cargo +$MSRV test --lib"
run cargo "+$MSRV" test --lib --quiet
ok "msrv $MSRV lib tests pass"

ok "ci-msrv.sh complete"
