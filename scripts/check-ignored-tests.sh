#!/usr/bin/env bash
# Ignored-test allowlist gate.
#
# `#[ignore]` tests inside production test modules are a classic
# silent-regression vector: a test that used to catch a bug gets
# `#[ignore]`d during development and never gets un-ignored. This
# script enumerates all currently-ignored lib tests and asserts:
#
#   1. They all match the allowlist of acceptable name patterns
#      (currently: `debug_*` exploratory tests + a small explicit
#      list of named exceptions).
#   2. The total count hasn't grown beyond the recorded snapshot.
#
# To intentionally add an ignored test (e.g. for a long-running
# debug aid), update the allowlist below; to remove one, update
# the count.
#
# Run from repo root:
#   ./scripts/check-ignored-tests.sh
# Or via the umbrella CI:
#   ./scripts/ci-inventory.sh

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

require cargo "install via https://rustup.rs/"

cd "$REPO_ROOT"

section "scripts/check-ignored-tests: enumerate #[ignore]'d lib tests"

LIST_TMP="$(mktemp -t bwipp-ignored.XXXXXX)"
trap 'rm -f "$LIST_TMP"' EXIT

(
    cd "$REPO_ROOT/rust"
    run mexec cargo test --lib --no-run --quiet
    run mexec cargo test --lib -- --list --format=terse --ignored \
        2>&1 \
        | grep ": test$" > "$LIST_TMP" || true
)

IGNORED_COUNT="$(wc -l < "$LIST_TMP" | tr -d ' ')"
info "$IGNORED_COUNT ignored test(s) found"

section "scripts/check-ignored-tests: cross-reference against allowlist"

python3 - "$LIST_TMP" <<'PY'
import re
import sys
from pathlib import Path

list_path = Path(sys.argv[1])
ignored = []
for line in list_path.read_text().splitlines():
    line = line.strip()
    if not line:
        continue
    name, _, _ = line.rpartition(": ")
    ignored.append(name)

# Allowlist patterns: every ignored test must match one of these.
# These are intentionally narrow — anything that doesn't match is
# a regression candidate.
ALLOWED_PATTERNS = [
    # Exploratory developer-only debug aids inside qrcode_native.
    # These are not production regression tests; they emit diagnostic
    # output for the QR encoder debugging work. Re-running them
    # requires `cargo test -- --ignored debug_`.
    re.compile(r"^symbology::qrcode_native::tests::debug_"),
    # rMQR-specific diff aids (paired with debug_*).
    re.compile(r"^symbology::qrcode_native::tests::rmqr_diff_"),
    # Code128 debug dump aid — manual diagnostics, not a regression test.
    re.compile(r"^symbology::code128::tests::dump_"),
    # Stage 11.A8c-L pre-drafted killer-test fingerprint placeholders for
    # composite/aztec/ultracode/code49. They contain `eprintln!("CAP ...")` capture
    # lines and assert against zero-init constants; they are kept
    # #[ignore]'d until a future loop iteration captures the real fingerprint
    # values (cargo test --include-ignored <name> -- --nocapture) and
    # hardcodes them. Then the #[ignore] is removed.
    re.compile(r"^symbology::(aztec|composite|ultracode|gs1_cc|code49|maxicode|code16k|databar_expanded|databar)::tests::.*_fingerprint_pinned_pending$"),
]

# Snapshot count. The allowlist gate also enforces an upper bound:
# adding new ignored tests requires bumping this number explicitly
# in `scripts/check-ignored-tests.sh`. This catches the case where
# a regression test gets `#[ignore]`d but happens to match an
# already-allowed prefix.
MAX_IGNORED = 37

violations = []
for name in ignored:
    if not any(p.match(name) for p in ALLOWED_PATTERNS):
        violations.append(name)

if violations:
    print()
    print(f"ERROR: {len(violations)} ignored test(s) outside the allowlist:")
    for v in violations:
        print(f"  - {v}")
    print()
    print("Either:")
    print("  * Remove the `#[ignore]` attribute (preferred — the test")
    print("    should pass).")
    print("  * Add an explicit allow-pattern in `scripts/check-ignored-tests.sh`")
    print("    documenting *why* this test is intentionally ignored.")
    sys.exit(1)

if len(ignored) > MAX_IGNORED:
    print()
    print(f"ERROR: ignored-test count {len(ignored)} exceeds the cap")
    print(f"       {MAX_IGNORED}. Either remove an `#[ignore]` or bump")
    print("       MAX_IGNORED in scripts/check-ignored-tests.sh after")
    print("       documenting why the count is growing.")
    sys.exit(1)

print(f"All {len(ignored)} ignored tests are in the allowlist (cap: {MAX_IGNORED}).")
PY

ok "ignored-test allowlist gate passed"
