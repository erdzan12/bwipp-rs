#!/usr/bin/env bash
# Golden-coverage consistency checker.
#
# Every PORT_STATUS.md row with status `verified` claims that the
# corresponding Rust encoder is byte-for-byte verified against
# bwip-js / BWIPP (or composition-pinned over a verified primary).
# That claim is only credible if there's a corresponding entry in
# GOLDEN_COVERAGE.md naming the actual test that pins the proof.
#
# This script extracts every catalog id mentioned in
# GOLDEN_COVERAGE.md (including the slash- and alt-backtick rollup
# patterns the doc uses, e.g. `code128a/b/c`,
# `composite_databar_omni_cca/_ccb`, `` `ean13p2`/`p5` ``,
# `auspost_customer/redirection/reply/routing`) and asserts that
# every PORT_STATUS verified row is present in the resulting set.
#
# Partial rows are exempt: they are explicitly *not* claiming
# full verification, so a per-row golden mention is encouraged but
# not required by this gate. (They still appear in
# GOLDEN_COVERAGE.md tagged `(partial)` for human reference.)
#
# Run from repo root:
#   ./scripts/check-golden-coverage.sh
# Or via the umbrella CI:
#   ./scripts/ci-inventory.sh   (this script runs as a late step)

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

cd "$REPO_ROOT"

section "scripts/check-golden-coverage: cross-reference PORT_STATUS verified rows ↔ GOLDEN_COVERAGE.md"

python3 <<'PY'
import re
import sys

# ---------------------------------------------------------------------------
# Step 1: parse PORT_STATUS rows and bucket by status.
# ---------------------------------------------------------------------------
ps_text = open('rust/PORT_STATUS.md').read()
row_re = re.compile(
    r'^\| `([a-zA-Z0-9_-]+)` \|[^|]+\|[^|]+\|[^|]+\| ([a-z ]+) \|',
    re.M,
)
verified_ids = sorted(
    {i for i, s in row_re.findall(ps_text) if s.strip() == 'verified'}
)
partial_ids = sorted(
    {i for i, s in row_re.findall(ps_text) if s.strip() == 'partial'}
)

# ---------------------------------------------------------------------------
# Step 2: extract every id mentioned in GOLDEN_COVERAGE.md.
#
# The doc uses three flavours of id mention:
#   1. Plain backticked token:    `code39`
#   2. In-backtick slash rollup:  `code128a/b/c`,
#      `composite_databar_omni_cca/_ccb`,
#      `usps_postnet5/9/11`, `planet12/14`,
#      `auspost_customer/redirection/reply/routing`
#   3. Adjacent backtick groups joined by `/`:
#      `ean13p2`/`p5`, `ean8p2`/`p5`, `upcap2`/`p5`, `upcep2`/`p5`
#
# Substitution rule for the slash rollups (the seed is the first
# piece; subsequent pieces substitute *some* trailing portion of
# the seed):
#   * If piece starts with `_`: substitute from seed's LAST `_`.
#   * Else if `piece[0]` occurs as a character of `seed`:
#     substitute from the LAST such occurrence.
#   * Else if `piece[0]` is a digit: substitute the trailing
#     digit-run of seed.
#   * Else if `piece[0]` is alpha: substitute the trailing
#     alpha-run of seed.
# ---------------------------------------------------------------------------
gc_text = open('rust/GOLDEN_COVERAGE.md').read()


def expand_slash_rollup(seed: str, pieces: list[str]) -> list[str]:
    """Return [seed, seed+alt1_substitution, seed+alt2_substitution, ...].

    Substitution rule, in priority order:
      1. piece starts with `_`        → substitute from seed's LAST `_`.
      2. piece starts with digit      → substitute seed's trailing digit-run.
      3. piece is letter+digit-pattern → substitute seed's trailing
         letter-then-digit-run (handles `ean13p2`/`p5`).
      4. piece starts with letter     → substitute seed's trailing
         letter-run (handles `code128a`/`b/c`,
         `auspost_customer`/`redirection`).
    """
    out = [seed]
    for piece in pieces:
        if not piece:
            continue
        if piece.startswith('_'):
            i = seed.rfind('_')
            if i < 0:
                continue
            out.append(seed[:i] + piece)
            continue
        first = piece[0]
        if first.isdigit():
            j = len(seed)
            while j > 0 and seed[j - 1].isdigit():
                j -= 1
            out.append(seed[:j] + piece)
            continue
        if first.isalpha():
            # Rule 3: piece pattern is letter+digits (e.g. `p5`,
            # `p10`). Strip seed's trailing letter+digit suffix
            # (e.g. `p2` in `ean13p2`).
            if (
                len(piece) >= 2
                and piece[1:].isdigit()
                and re.search(r'[a-z]\d+$', seed)
            ):
                j = re.search(r'[a-z]\d+$', seed).start()
                out.append(seed[:j] + piece)
                continue
            # Rule 4: substitute trailing letter-run.
            j = len(seed)
            while j > 0 and seed[j - 1].isalpha():
                j -= 1
            out.append(seed[:j] + piece)
            continue
    return out


covered = set()

# 1. In-backtick slash rollups (and plain backticked tokens — when
#    there's no slash, expand_slash_rollup just returns [seed]).
for m in re.finditer(r'`([a-z][a-zA-Z0-9_/-]+)`', gc_text):
    raw = m.group(1)
    if '/' not in raw:
        covered.add(raw)
        continue
    pieces = raw.split('/')
    for expanded in expand_slash_rollup(pieces[0], pieces[1:]):
        covered.add(expanded)

# 2. Adjacent backtick groups joined by `/`: `seedX`/`altX`
for m in re.finditer(
    r'`([a-z][a-zA-Z0-9_-]+)`(?:/`([a-z0-9_-]+)`)+',
    gc_text,
):
    raw_text = m.group(0)
    parts = [p for p in re.findall(r'`([a-z][a-zA-Z0-9_-]+)`', raw_text)]
    if not parts:
        continue
    for expanded in expand_slash_rollup(parts[0], parts[1:]):
        covered.add(expanded)

# 3. Plain backticked-ids fallback (catches anything the slash regex
#    rejected — defensively expand).
for m in re.finditer(r'`([a-z][a-zA-Z0-9_-]+)`', gc_text):
    covered.add(m.group(1))

# ---------------------------------------------------------------------------
# Step 3: assert every verified row is covered, and report any gaps.
# ---------------------------------------------------------------------------
missing = [v for v in verified_ids if v not in covered]

print(f'PORT_STATUS verified rows: {len(verified_ids)}')
print(f'PORT_STATUS partial rows:  {len(partial_ids)} (exempt from this gate)')
print(f'GOLDEN_COVERAGE id mentions (after rollup expansion): {len(covered)}')

if missing:
    print()
    print('ERROR: the following PORT_STATUS verified rows have NO corresponding')
    print('       entry in GOLDEN_COVERAGE.md (either by literal id or by')
    print('       slash-rollup expansion):')
    for m in missing:
        print(f'  - {m}')
    print()
    print('Fix: add a row (or rollup) to GOLDEN_COVERAGE.md that names the')
    print('     concrete test pinning the verification for the row above,')
    print('     OR downgrade the row from `verified` to `partial` in')
    print('     PORT_STATUS.md if no such test exists.')
    sys.exit(1)

print()
print('All verified PORT_STATUS rows have GOLDEN_COVERAGE entries.')
PY

ok "golden coverage checker passed"
