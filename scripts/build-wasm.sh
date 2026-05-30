#!/usr/bin/env bash
# Build the raw-pointer WASM bundle and copy it into web/public/wasm/. This is
# the SINGLE source of truth for how the committed bundle is produced —
# web/package.json's `build:wasm` calls it, and scripts/ci-web.sh verifies the
# committed bundle is current via its source fingerprint (see below).
#
# Runs on ANY platform (macOS / Linux / WSL). The build pins an explicit stable
# rustc (`$WASM_TOOLCHAIN`) so the embedded /rustc/<hash>/ std paths + codegen
# are fixed, with --remap-path-prefix (no /home/<user>/… leaks) and
# `strip = "symbols"` (rust/wasm/Cargo.toml). We pin HERE rather than in a
# repo-wide rust-toolchain.toml so bare `cargo` keeps using the contributor's
# own toolchain (no footgun).
#
# NOTE: the resulting binary is reproducible on a GIVEN host, but NOT
# byte-for-byte across OS/arch — so staleness is guarded by a SOURCE fingerprint
# (web/public/wasm/bwipp_wasm.wasm.srcsha256), not a binary diff. On rebuild this
# script rewrites that sidecar too; commit the bundle + sidecar together.
#
# To upgrade rustc: bump WASM_TOOLCHAIN (and scripts/bootstrap-ci.sh), rebuild,
# and commit the refreshed bundle + sidecar. Install the pinned toolchain +
# target via scripts/bootstrap-ci.sh.
set -euo pipefail

# The stable toolchain the committed bundle is built with. Keep in sync with
# scripts/bootstrap-ci.sh and rust/Cargo.toml's MSRV note.
WASM_TOOLCHAIN="${WASM_TOOLCHAIN:-1.95.0}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
WASM_CRATE="$REPO_ROOT/rust/wasm"
WEB_WASM="$REPO_ROOT/web/public/wasm/bwipp_wasm.wasm"

# Resolve a cargo invocation that runs EXACTLY $WASM_TOOLCHAIN, portably:
#   - rustup `+ver` override (native rustup toolchain), or
#   - mise-provided `+ver`, or
#   - a default cargo that already IS that version (e.g. mise pinning
#     rust=$WASM_TOOLCHAIN on macOS, where rustup's mise-linked `+ver` proxy
#     can't launch cargo).
# This is what lets the wasm build run on macOS + Linux + WSL alike.
_wcargo_kind=""
if cargo "+${WASM_TOOLCHAIN}" --version >/dev/null 2>&1; then
    _wcargo_kind="rustup-plus"
elif command -v mise >/dev/null 2>&1 && mise exec -- cargo "+${WASM_TOOLCHAIN}" --version >/dev/null 2>&1; then
    _wcargo_kind="mise-plus"
elif command -v mise >/dev/null 2>&1 && mise exec -- cargo --version 2>/dev/null | grep -qF "${WASM_TOOLCHAIN}"; then
    _wcargo_kind="mise-default"
elif cargo --version 2>/dev/null | grep -qF "${WASM_TOOLCHAIN}"; then
    _wcargo_kind="bare-default"
else
    echo "error: no available cargo provides rustc ${WASM_TOOLCHAIN}." >&2
    echo "       run scripts/bootstrap-ci.sh (installs the pinned toolchain)," >&2
    echo "       or set WASM_TOOLCHAIN=<a version you have>." >&2
    exit 1
fi

wcargo() {
    case "$_wcargo_kind" in
        rustup-plus)  cargo "+${WASM_TOOLCHAIN}" "$@" ;;
        mise-plus)    mise exec -- cargo "+${WASM_TOOLCHAIN}" "$@" ;;
        mise-default) mise exec -- cargo "$@" ;;
        bare-default) cargo "$@" ;;
    esac
}

# Best-effort: ensure the wasm target exists for whichever toolchain we resolved.
case "$_wcargo_kind" in
    rustup-plus)  rustup target add --toolchain "${WASM_TOOLCHAIN}" wasm32-unknown-unknown 2>/dev/null || true ;;
    mise-plus)    mise exec -- rustup target add --toolchain "${WASM_TOOLCHAIN}" wasm32-unknown-unknown 2>/dev/null || true ;;
    mise-default) mise exec -- rustup target add wasm32-unknown-unknown 2>/dev/null || true ;;
    bare-default) rustup target add wasm32-unknown-unknown 2>/dev/null || true ;;
esac

# Stable virtual path prefixes (machine-independent).
export RUSTFLAGS="--remap-path-prefix=${REPO_ROOT}=/bwipp-rs --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"

wcargo build --release \
    --target wasm32-unknown-unknown \
    --manifest-path "$WASM_CRATE/Cargo.toml" "$@"

mkdir -p "$(dirname "$WEB_WASM")"
cp "$WASM_CRATE/target/wasm32-unknown-unknown/release/bwipp_wasm.wasm" "$WEB_WASM"

# Refresh the host-independent source fingerprint that ci-web.sh's staleness
# guard checks. Commit this sidecar alongside the bundle.
"$HERE/wasm-srcsha.sh" > "${WEB_WASM}.srcsha256"

echo "wasm built with rustc ${WASM_TOOLCHAIN} → $WEB_WASM ($(wc -c < "$WEB_WASM") bytes)"
echo "source fingerprint → ${WEB_WASM}.srcsha256 ($(cat "${WEB_WASM}.srcsha256"))"
