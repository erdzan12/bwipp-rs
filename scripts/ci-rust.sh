#!/usr/bin/env bash
# Local Rust CI: fmt, clippy, tests, doctests, doc build, wasm32 build,
# release build, publish dry-run.
#
# Designed to be re-callable from GitHub Actions later via a thin wrapper.

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

require cargo "install via https://rustup.rs/"

cd "$REPO_ROOT/rust"

section "cargo fmt --check"
run mexec cargo fmt --all -- --check
ok "fmt clean"

section "cargo clippy --all-targets --all-features -- -D warnings"
run mexec cargo clippy --all-targets --all-features -- -D warnings
ok "clippy clean"

section "cargo test (default features)"
run mexec cargo test --all-targets --quiet
ok "unit + integration tests"

section "cargo test (all features)"
run mexec cargo test --all-targets --all-features --quiet
ok "all-feature tests"

section "cargo test --doc"
run mexec cargo test --doc --quiet
ok "doctests"

section "cargo doc (deny warnings, docs.rs flags)"
# `--cfg docsrs` mirrors the docs.rs build profile (see
# `[package.metadata.docs.rs]` in `rust/Cargo.toml`). Catching
# rustdoc warnings here means docs.rs won't surprise us at publish time.
RUSTDOCFLAGS="--cfg docsrs -D warnings" run mexec cargo doc --no-deps --all-features --quiet
ok "rustdoc clean"

section "cargo build --release"
run mexec cargo build --release --quiet
ok "release build"

section "cargo build --target wasm32-unknown-unknown --no-default-features --features wasm"
if ! mexec rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
    info "installing wasm32-unknown-unknown target via rustup"
    run mexec rustup target add wasm32-unknown-unknown
fi
run mexec cargo build --target wasm32-unknown-unknown --no-default-features --features wasm --quiet
ok "wasm32 build"

section "raw-pointer WASM crate"
(
    cd wasm
    run mexec cargo build --release --target wasm32-unknown-unknown --quiet
)
ok "raw-pointer wasm build"

section "cargo publish --dry-run"
# In strict mode (PUBLISH_STRICT=1) we drop `--allow-dirty` and
# require a clean committed worktree — that's what `cargo publish`
# would enforce for real. In the normal local-dev pass, the dry-run
# stays `--allow-dirty` so an in-progress audit doesn't fail before
# the user has even had a chance to stage their changes.
if [ "${PUBLISH_STRICT:-0}" = "1" ]; then
    # If the worktree is dirty, surface a clear error before cargo
    # gives a less-actionable message.
    if [ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]; then
        die "PUBLISH_STRICT=1: working tree is dirty. Commit or stash before running strict CI."
    fi
    run mexec cargo publish --dry-run
else
    run mexec cargo publish --dry-run --allow-dirty
fi
ok "publish dry-run"

# wasm-pack tests are optional in this script (slow + require wasm-pack).
# Use scripts/ci-golden.sh for the full wasm-bindgen test suite.

ok "Rust CI passed"
