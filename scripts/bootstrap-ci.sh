#!/usr/bin/env bash
# One-command, idempotent bootstrap of every opt-in toolchain/tool the
# *strict* local CI gate needs. Run this once on a fresh clone, then:
#
#     PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh
#
# Safe to re-run: every step is a no-op if already satisfied. This script
# is the ONLY place CI may mutate your toolchain — the gate scripts
# themselves never auto-install (ci-fuzz.sh fails with instructions under
# PUBLISH_STRICT rather than silently changing your machine).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"

# Pinned stable toolchain used to build the reproducible WASM bundle.
# Keep in sync with scripts/build-wasm.sh (WASM_TOOLCHAIN).
WASM_TOOLCHAIN="1.95.0"

if ! command -v rustup >/dev/null 2>&1; then
    echo "error: rustup not found. Install Rust from https://rustup.rs first." >&2
    exit 1
fi

# MSRV toolchain, parsed from rust/Cargo.toml's rust-version.
MSRV="$(grep -E '^rust-version' "$REPO_ROOT/rust/Cargo.toml" | head -1 | sed -E 's/.*"([0-9.]+)".*/\1/')"
MSRV="${MSRV:-1.85}"

step() { echo ">>> $*"; }

step "MSRV toolchain ($MSRV) — for scripts/ci-msrv.sh"
rustup toolchain install "$MSRV" --profile minimal --no-self-update 2>/dev/null || rustup toolchain install "$MSRV"

step "pinned stable ($WASM_TOOLCHAIN) + wasm32 target — for the reproducible WASM build"
rustup toolchain install "$WASM_TOOLCHAIN" --profile minimal --no-self-update 2>/dev/null || rustup toolchain install "$WASM_TOOLCHAIN"
rustup target add --toolchain "$WASM_TOOLCHAIN" wasm32-unknown-unknown
# Also add wasm32 to the default toolchain so plain `cargo build --target
# wasm32-...` works for ad-hoc use.
rustup target add wasm32-unknown-unknown 2>/dev/null || true

step "nightly + rust-src + llvm-tools-preview — for the cargo-fuzz gate"
rustup toolchain install nightly --profile minimal --no-self-update 2>/dev/null || rustup toolchain install nightly
rustup component add --toolchain nightly rust-src llvm-tools-preview

step "cargo-fuzz / cargo-audit / cargo-deny — for the fuzz + security gates"
for tool in cargo-fuzz cargo-audit cargo-deny; do
    if command -v "$tool" >/dev/null 2>&1; then
        echo "    $tool already installed"
    else
        echo "    installing $tool"
        cargo install --locked "$tool"
    fi
done

echo
echo "bootstrap complete. Now run the strict gate:"
echo "    PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh"
