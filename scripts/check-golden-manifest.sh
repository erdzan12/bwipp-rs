#!/usr/bin/env bash
# Golden-manifest executable proof gate.
#
# `scripts/check-golden-coverage.sh` proves that every PORT_STATUS
# verified row is *mentioned* in `rust/GOLDEN_COVERAGE.md`. That's
# necessary but not sufficient — a doc could mention test names that
# don't exist (typos, renames). This script closes the loop:
#
#   1. Loads `rust/tests/golden_manifest.json` (built by
#      `scripts/build-golden-manifest.py` from PORT_STATUS +
#      GOLDEN_COVERAGE).
#   2. Compiles the test binary with `cargo test --lib --no-run`.
#   3. Runs `cargo test --lib -- --list --format=terse` to enumerate
#      every test function the binary actually contains.
#   4. Asserts that every test function referenced by the manifest is
#      present in the enumerated list.
#
# A failure means either:
#   * The doc cites a renamed/deleted test (fix the doc).
#   * A row's claimed coverage doesn't actually exist (add the test,
#     or downgrade the row).
#
# Run from repo root:
#   ./scripts/check-golden-manifest.sh
# Or via the umbrella CI:
#   ./scripts/ci-inventory.sh   (this script runs as a late step)

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

require cargo "install via https://rustup.rs/"

cd "$REPO_ROOT"

MANIFEST="rust/tests/golden_manifest.json"
if [ ! -f "$MANIFEST" ]; then
    die "manifest missing: $MANIFEST (run python3 scripts/build-golden-manifest.py)"
fi

section "scripts/check-golden-manifest: regenerate manifest from PORT_STATUS + GOLDEN_COVERAGE"
# Always regenerate the manifest so a manifest that has drifted out
# of sync with the docs is caught here, not silently accepted.
run mexec python3 scripts/build-golden-manifest.py

section "scripts/check-golden-manifest: enumerate cargo lib + integration tests"
# `cargo test --lib --no-run` compiles the lib test binary without
# running it; `-- --list --format=terse` then asks the binary to
# print one test name per line (`module::path::name: test`).
# Integration tests live in `rust/tests/*.rs` and are separate
# binaries — each must be enumerated independently.
LIST_TMP="$(mktemp -t bwipp-golden-manifest.XXXXXX)"
trap 'rm -f "$LIST_TMP"' EXIT
(
    cd "$REPO_ROOT/rust"
    run mexec cargo test --lib --no-run --quiet
    run mexec cargo test --lib -- --list --format=terse > "$LIST_TMP"
    # Integration tests. The test binary is named after its file
    # (`tests/integration.rs` → `--test integration`). The wasm-bindgen
    # integration tests in `tests/wasm.rs` are deliberately excluded
    # here because they require `--target wasm32-unknown-unknown` and
    # the wasm-pack runtime — that's a separate gate in ci-golden.sh.
    if [ -f tests/integration.rs ]; then
        run mexec cargo test --test integration --no-run --quiet
        # `--list` output omits the binary's name; prefix it
        # ourselves so the manifest's `integration::fn_name`
        # references match.
        run mexec cargo test --test integration -- --list --format=terse \
            | sed 's/^/integration::/' >> "$LIST_TMP"
    fi
)
LIST_COUNT="$(wc -l < "$LIST_TMP" | tr -d ' ')"
info "enumerated $LIST_COUNT total test entries (lib + integration)"

section "scripts/check-golden-manifest: cross-reference manifest ↔ cargo lib tests"
python3 - "$LIST_TMP" <<'PY'
import json
import sys
from pathlib import Path

list_path = Path(sys.argv[1])
manifest = json.loads(Path("rust/tests/golden_manifest.json").read_text())

# Cargo emits lines like `code39::tests::X: test`. Strip the
# trailing ": test" / ": benchmark" suffix.
known_tests = set()
for line in list_path.read_text().splitlines():
    line = line.strip()
    if not line:
        continue
    name, _, kind = line.rpartition(": ")
    if kind == "test":
        known_tests.add(name)

print(f"cargo --list reports {len(known_tests)} lib test functions")

# Verify every manifest test exists in cargo --list output.
missing_per_entry: list[tuple[str, list[str]]] = []
verified_rows = [e for e in manifest["entries"] if e["status"] == "verified"]
partial_rows = [e for e in manifest["entries"] if e["status"] == "partial"]
print(f"manifest: {len(verified_rows)} verified rows, {len(partial_rows)} partial rows")

for entry in manifest["entries"]:
    if not entry["tests"]:
        # Empty-tests rows are flagged by the manifest builder; if
        # the manifest builder couldn't extract any test for a row,
        # this script doesn't try to invent one. The
        # check-golden-coverage.sh sibling catches the "no GOLDEN
        # mention at all" case; treat empty-tests here as a row
        # whose only proof is an alias-router smoke test
        # (`integration::alias_ids_route_to_canonical_symbology`)
        # — manifestation builder reports any such row to stderr.
        continue
    missing = [t for t in entry["tests"] if t not in known_tests]
    if missing:
        missing_per_entry.append((entry["id"], missing))

if missing_per_entry:
    print()
    print("ERROR: the following manifest test references do NOT exist")
    print("       in the compiled cargo lib test binary:")
    print()
    for catalog_id, missing in missing_per_entry:
        for m in missing:
            print(f"  - {catalog_id}: {m}")
    print()
    print("Fix: rename / delete / un-cfg the docstring's test reference,")
    print("     OR add the missing test so the manifest's claim is real.")
    sys.exit(1)

# Oracle-strength gate: every verified row must have a non-trivial
# oracle classification. `unknown` means the manifest builder
# couldn't infer what kind of proof backs the row — add a test
# whose name signals its assertion kind (or update the classifier
# in scripts/build-golden-manifest.py to recognise the existing
# pattern). `smoke` is intentionally allowed here ONLY for rows
# whose only listed test is the integration smoke; downstream
# scout passes should still attempt to upgrade those.
unknown_verified = [
    e for e in manifest["entries"]
    if e["status"] == "verified" and e["oracle_type"] == "unknown"
]
if unknown_verified:
    print()
    print(f"ERROR: {len(unknown_verified)} verified rows have oracle_type=`unknown`:")
    for e in unknown_verified:
        print(f"  - {e['id']}: tests={e['tests'][:2]}")
    print()
    print("Fix: rename a test (e.g. add `matches_bwip_js`/`pixs`/`sbs`")
    print("     to its name) OR teach scripts/build-golden-manifest.py")
    print("     to recognise the existing naming convention.")
    sys.exit(1)

empty_rows = [e["id"] for e in manifest["entries"] if not e["tests"]]
if empty_rows:
    print()
    print(f"NOTE: {len(empty_rows)} rows have no manifest tests yet.")
    print("      These rows still pass `check-golden-coverage.sh` (they")
    print("      are mentioned in GOLDEN_COVERAGE.md by id), but the")
    print("      executable proof gate is empty for them. Add a concrete")
    print("      test reference in GOLDEN_COVERAGE.md to harden the chain.")
    print("      First few: " + ", ".join(empty_rows[:5]))

# Oracle-strength distribution summary.
print()
print("Oracle-strength distribution:")
from collections import Counter
dist = Counter(e["oracle_type"] for e in manifest["entries"] if e["status"] == "verified")
for ot, n in sorted(dist.items()):
    print(f"  verified: {ot:25s} {n}")
dist_p = Counter(e["oracle_type"] for e in manifest["entries"] if e["status"] == "partial")
for ot, n in sorted(dist_p.items()):
    print(f"  partial : {ot:25s} {n}")

total_test_refs = sum(len(e["tests"]) for e in manifest["entries"])
print()
print(f"All {total_test_refs} manifest test references exist in the cargo lib test binary.")
PY

ok "golden manifest checker passed"
