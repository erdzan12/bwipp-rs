#!/usr/bin/env bash
# Verify that the upstream BWIPP / bwip-js encoder inventory in
# `rust/tools/inventory/inventory_diff.json` is:
#
#   1. Up-to-date — regenerating from upstream bwip-js + the local Rust
#      source produces the same bytes that are committed.
#   2. Complete — no upstream encoder has status `unknown`.
#   3. Reachable — every `implemented` / `alias_only` /
#      `compatibility_exception` row resolves through `Symbology::from_id`
#      (`rust_alias_present: true`).
#
# Also regenerates `rust/PORT_COMPLETENESS.md` and verifies it matches the
# committed version.
#
# Source-of-truth for "is the upstream comparison still honest?"

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

cd "$REPO_ROOT"

section "ci-inventory: regenerate upstream BWIPP / bwip-js inventory"

INVDIR="rust/tools/inventory"
DIFF="$INVDIR/inventory_diff.json"
PORT="rust/PORT_COMPLETENESS.md"

if [[ ! -d "node-sidecar/node_modules/bwip-js" ]]; then
  die "node-sidecar/node_modules/bwip-js missing — run 'npm install' in node-sidecar/ first"
fi

info "regenerating $INVDIR/upstream_bwipp.json from upstream bwip-js"
node --input-type=module -e \
  "import { bwipp_symlist } from '$REPO_ROOT/node-sidecar/node_modules/bwip-js/dist/bwipp.mjs'; process.stdout.write(JSON.stringify(bwipp_symlist, null, 2));" \
  > "$INVDIR/upstream_bwipp.json"

# Stash existing copies; we re-render and diff.
if [[ -f "$DIFF" ]]; then
  cp "$DIFF" "/tmp/.bwipprs-inventory-diff.before"
else
  : > "/tmp/.bwipprs-inventory-diff.before"
fi
if [[ -f "$PORT" ]]; then
  cp "$PORT" "/tmp/.bwipprs-port-completeness.before"
else
  : > "/tmp/.bwipprs-port-completeness.before"
fi

python3 "$INVDIR/build_inventory.py" >/dev/null
python3 "$INVDIR/render_completeness.py" >/dev/null

if ! diff -q "/tmp/.bwipprs-inventory-diff.before" "$DIFF" >/dev/null; then
  die "$DIFF is out of date — re-run python3 $INVDIR/build_inventory.py and commit."
  diff "/tmp/.bwipprs-inventory-diff.before" "$DIFF" || true
  exit 1
fi
ok "inventory_diff.json is current"

if ! diff -q "/tmp/.bwipprs-port-completeness.before" "$PORT" >/dev/null; then
  die "$PORT is out of date — re-run python3 $INVDIR/render_completeness.py and commit."
  diff "/tmp/.bwipprs-port-completeness.before" "$PORT" || true
  exit 1
fi
ok "PORT_COMPLETENESS.md is current"

section "ci-inventory: classification invariants"

# Hard invariant: no upstream encoder is unclassified.
unknown=$(python3 -c "import json,sys; d=json.load(open('$DIFF')); print(d['summary'].get('unknown', 0))")
if [[ "$unknown" != "0" ]]; then
  die "inventory has $unknown unknown upstream encoders — classify them in build_inventory.py"
  exit 1
fi
ok "no upstream encoder is unclassified"

# Hard invariant: every implemented / alias_only / compat-exception row is
# actually accepted by Rust's Symbology::from_id.
unreachable=$(python3 -c "
import json
d = json.load(open('$DIFF'))
bad = [r['upstream_bcid'] for r in d['rows']
       if r['status'] in ('implemented', 'alias_only', 'compatibility_exception')
       and not r.get('rust_alias_present')]
print('\n'.join(bad))
")
if [[ -n "$unreachable" ]]; then
  die "the following upstream encoder ids are classified as reachable but missing from Symbology::from_id:"
  echo "$unreachable"
  exit 1
fi
ok "every reachable upstream id is accepted by Symbology::from_id"

# Soft invariant: print the missing & out_of_scope buckets so a human sees
# exactly which upstream encoders are not yet ported. The script does not
# fail on those — they are classified, just not implemented — but printing
# them keeps the gap visible on every CI run.
section "ci-inventory: missing / out-of-scope upstream encoders"
python3 -c "
import json
d = json.load(open('$DIFF'))
for s in ('missing', 'out_of_scope'):
    bucket = [r for r in d['rows'] if r['status'] == s]
    if not bucket:
        continue
    print(f'  {s} ({len(bucket)}):')
    for r in sorted(bucket, key=lambda x: x['upstream_bcid']):
        print(f'    - {r[\"upstream_bcid\"]}: {r[\"rationale\"]}')
"

section "ci-inventory: PORT_STATUS / GOLDEN_COVERAGE / COMPATIBILITY_EXCEPTIONS coverage"

# Hard invariant: every catalog row in PORT_STATUS.md (status = verified
# or compatibility exception) must be reachable in GOLDEN_COVERAGE.md
# either directly by id or via a documented family-notation row
# (e.g. `usps_postnet5/9/11`, `hibc_pas_*`, etc). Compatibility-exception
# rows must additionally appear in COMPATIBILITY_EXCEPTIONS.md.
python3 - <<'PY'
import re, sys
from pathlib import Path

ps = Path("rust/PORT_STATUS.md").read_text()
gc = Path("rust/GOLDEN_COVERAGE.md").read_text()
ex = Path("rust/COMPATIBILITY_EXCEPTIONS.md").read_text()

row_re = re.compile(r'^\| `([a-z0-9_-]+)` \|[^|]+\|[^|]+\|[^|]+\| ([a-z ]+) \|',
                    re.MULTILINE)
rows = [(m.group(1), m.group(2).strip()) for m in row_re.finditer(ps)]

def covered(cid: str, doc: str) -> bool:
    # Direct id mention?
    if re.search(rf"`{re.escape(cid)}`", doc):
        return True
    # Wildcard family: `hibc_pas_*`
    if re.search(r"`hibc_pas_\*`", doc) and cid.startswith("hibc_pas_"):
        return True
    # Slash-separated family list inside backticks. Try a handful of
    # head-suffix splitting strategies — the doc uses several shapes:
    #   - `code128a/b/c`                          (trailing single letter)
    #   - `usps_postnet5/9/11`, `planet12/14`     (trailing digit run)
    #   - `ean13p2/p5`, `upcap2/p5`               (trailing `pN`)
    #   - `auspost_customer/redirection/reply/...`(trailing `_word`)
    #   - `composite_..._cca/_ccb` (slash-prefixed suffix continues from head)
    # (stripper_regex, glue) — when `stripper` removes a suffix from
    # the head, prepend `glue` before the new continuation part. The
    # `auspost_customer` family strips `customer` (keeping the
    # underscore), then glues `_` onto each new leaf. The `code128a`
    # family strips a single letter, no glue.
    suffix_strippers = [
        # Lookbehind keeps the underscore in the stem, so no glue.
        (re.compile(r"(?<=_)[a-z][a-z0-9_]*$"), ""),
        (re.compile(r"p\d+$"), ""),                    # pN
        (re.compile(r"\d+$"), ""),                     # digit run
        (re.compile(r"[a-z]$"), ""),                   # single letter
    ]

    # Family forms covered:
    #   1. one backtick group with slashes:    `code128a/b/c`
    #   2. two adjacent backtick groups:       `ean13p2`/`p5`
    # Normalise both into a flat list of "leaves" sharing one head.
    one_backtick = re.compile(r"`([^`]+/[^`]+)`")
    two_backticks = re.compile(r"`([^`]+)`/`([^`]+)`")

    families: list[list[str]] = []
    for m in one_backtick.finditer(doc):
        families.append(m.group(1).split("/"))
    for m in two_backticks.finditer(doc):
        families.append([m.group(1), m.group(2)])

    for parts in families:
        head = parts[0]
        if cid == head:
            return True
        for p in parts[1:]:
            if p.startswith("_"):
                stripped = re.sub(r"_[a-z][a-z0-9]*$", "", head)
                if cid == stripped + p:
                    return True
            else:
                for stripper, glue in suffix_strippers:
                    stripped = stripper.sub("", head)
                    if stripped == head:
                        continue
                    if cid == stripped + glue + p:
                        return True
    return False

missing_in_gc = []
missing_in_ex = []
for cid, status in rows:
    if status in ("verified",):
        if not covered(cid, gc):
            missing_in_gc.append(cid)
    elif "exception" in status or "compat" in status:
        if not covered(cid, gc):
            missing_in_gc.append(cid)
        if not covered(cid, ex):
            missing_in_ex.append(cid)

if missing_in_gc:
    print("FAIL: PORT_STATUS rows not covered by GOLDEN_COVERAGE.md "
          "(directly or via family notation):", file=sys.stderr)
    for cid in missing_in_gc:
        print(f"  - {cid}", file=sys.stderr)
    sys.exit(1)
if missing_in_ex:
    print("FAIL: compatibility-exception rows not in "
          "COMPATIBILITY_EXCEPTIONS.md:", file=sys.stderr)
    for cid in missing_in_ex:
        print(f"  - {cid}", file=sys.stderr)
    sys.exit(1)
print(f"  PORT_STATUS rows checked: {len(rows)}")
print("  every verified / exception row appears in GOLDEN_COVERAGE.md")
print("  every exception row appears in COMPATIBILITY_EXCEPTIONS.md")
PY
ok "PORT_STATUS / GOLDEN_COVERAGE / COMPATIBILITY_EXCEPTIONS in agreement"

section "ci-inventory: substrate-version sentinel"

# Hard invariant: Cargo.lock pins of the QR Code and Data Matrix
# substrate crates must match the verified-against-bwip-js versions
# recorded in `rust/tools/inventory/substrate_versions.json`. A bump
# is fine in principle, but it must be done deliberately so a human
# can verify the substrate-dim drift net + qrcode/datamatrix baseline
# tests still match bwip-js for the new substrate output.
python3 - <<'PY'
import json, re, sys
from pathlib import Path

snap_path = Path("rust/tools/inventory/substrate_versions.json")
lock_path = Path("rust/Cargo.lock")
snap = json.loads(snap_path.read_text())["substrates"]
lock = lock_path.read_text()

# Extract concrete locked versions for the crates we pin.
locked = {}
for m in re.finditer(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock):
    locked[m.group(1)] = m.group(2)

drift = []
for crate, info in snap.items():
    want = info["version"]
    got = locked.get(crate)
    if got != want:
        drift.append((crate, want, got))

if drift:
    print("FAIL: substrate-version drift detected:", file=sys.stderr)
    for crate, want, got in drift:
        print(f"  {crate}: Cargo.lock pins {got}, sentinel expects {want}", file=sys.stderr)
    print("", file=sys.stderr)
    print("If the bump is intentional, run the substrate-dim drift net + qrcode/", file=sys.stderr)
    print("datamatrix baseline tests against bwip-js, then update", file=sys.stderr)
    print(f"  {snap_path}", file=sys.stderr)
    print("  - bump `version` for the affected crate", file=sys.stderr)
    print("  - bump `verified_against_bwip_js_at` to today's date", file=sys.stderr)
    sys.exit(1)

for crate, info in snap.items():
    print(f"  {crate} = {info['version']} (matches Cargo.lock)")
PY
ok "substrate versions match sentinel snapshot"

section "scripts/ci-inventory: doc-count consistency"
"$HERE/check-doc-counts.sh"

section "scripts/ci-inventory: golden coverage cross-reference"
"$HERE/check-golden-coverage.sh"

section "scripts/ci-inventory: golden manifest executable proof"
"$HERE/check-golden-manifest.sh"

section "scripts/ci-inventory: ignored-test allowlist"
"$HERE/check-ignored-tests.sh"

ok "ci-inventory.sh complete"
