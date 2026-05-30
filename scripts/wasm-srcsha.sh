#!/usr/bin/env bash
# Print a host-independent fingerprint of the sources that determine the
# prebuilt WASM bundle (web/public/wasm/bwipp_wasm.wasm).
#
# Used by:
#   scripts/build-wasm.sh  -> writes it to bwipp_wasm.wasm.srcsha256 on rebuild
#   scripts/ci-web.sh      -> compares it to the committed sidecar (staleness guard)
#
# It hashes SOURCE TEXT, not the compiled binary, so the result is identical on
# every OS/arch. (The wasm binary itself is NOT byte-reproducible across hosts,
# so a binary diff would false-fail off the build host; a source fingerprint
# does not.) Deliberately bash 3.2-compatible (stock macOS): no `mapfile`, no
# GNU-only `sort -z`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"

sha256_stdin() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum
    else
        shasum -a 256
    fi
}

{
    # Every Rust source compiled into the wasm: the library + the wasm wrapper.
    # Newline-delimited + LC_ALL=C sort gives a deterministic order; these are
    # plain module paths (no spaces/newlines), so this is safe and portable.
    find "$REPO_ROOT/rust/src" "$REPO_ROOT/rust/wasm/src" -type f -name '*.rs' \
        | LC_ALL=C sort \
        | while IFS= read -r f; do
            printf '=== %s ===\n' "${f#"$REPO_ROOT"/}"
            cat "$f"
        done
    # Manifests (feature selection) + the wasm crate's lockfile (pins every
    # compiled dependency version).
    for m in rust/Cargo.toml rust/wasm/Cargo.toml rust/wasm/Cargo.lock; do
        printf '=== %s ===\n' "$m"
        cat "$REPO_ROOT/$m"
    done
} | sha256_stdin | awk '{print $1}'
