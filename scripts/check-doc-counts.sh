#!/usr/bin/env bash
# Doc / inventory consistency checker.
#
# bwipp-rs has two sources of truth that can drift apart:
#
#   1. PORT_STATUS.md — a hand-written per-row catalog. Its header
#      claims aggregate counts (verified / partial / compat-exception /
#      missing).
#   2. tools/inventory/inventory_diff.json — machine-generated from
#      bwip-js's `bwipp_symlist` + this crate's `Symbology` enum.
#
# Plus the same aggregates are repeated in rust/README.md and (in
# narrative form) in AUDIT.md / ROADMAP.md / GOLDEN_COVERAGE.md /
# COMPATIBILITY_EXCEPTIONS.md.
#
# This script fails CI if any of the doc-side counts disagree with
# what the table actually contains. Add a new row to PORT_STATUS.md
# without updating the header → fail. Lower the verified count without
# changing the table → fail.
#
# Run from repo root:
#   ./scripts/check-doc-counts.sh
# Or via the umbrella CI:
#   ./scripts/ci-inventory.sh   (this script runs as the last step)

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

cd "$REPO_ROOT"

section "scripts/check-doc-counts: parse PORT_STATUS.md table"

# Pull every body row of the form `| `id` | name | source | local | status | …`
# and group by the `status` column.
PARSED="$(
    python3 <<'PY'
import re, json, sys
from collections import Counter

text = open('rust/PORT_STATUS.md').read()
# Skip the legend / scope note tables (they start with column headers
# like `| Status |` or `| Category |` and have no backticked id). Only
# count rows whose first column is a backticked identifier.
pattern = re.compile(
    r'^\| \`([a-zA-Z0-9_-]+)\` \|[^|]+\|[^|]+\|[^|]+\| ([a-z ]+) \|',
    re.M,
)
rows = pattern.findall(text)
counts = Counter(status for _id, status in rows)
result = {
    "total": sum(counts.values()),
    "by_status": dict(counts),
}
print(json.dumps(result))
PY
)"

TOTAL=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["total"])')
VERIFIED=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["by_status"].get("verified", 0))')
PARTIAL=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["by_status"].get("partial", 0))')
COMPAT=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["by_status"].get("compatibility exception", 0))')
MISSING=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["by_status"].get("missing", 0))')
ALIAS=$(echo "$PARSED" | python3 -c 'import json,sys; print(json.load(sys.stdin)["by_status"].get("alias", 0))')

info "PORT_STATUS.md table parsed:"
info "  total                       = $TOTAL"
info "  verified                    = $VERIFIED"
info "  partial                     = $PARTIAL"
info "  compatibility exception     = $COMPAT"
info "  missing                     = $MISSING"
info "  alias                       = $ALIAS"

# Helper: assert that $1 (file) contains $2 (string verbatim).
expect_contains() {
    local file="$1"
    local needle="$2"
    if ! grep -qF "$needle" "$file"; then
        die "$file is missing expected string: \"$needle\""
    fi
}

# Helper: assert that $1 (file) does NOT contain $2 (string verbatim).
expect_absent() {
    local file="$1"
    local needle="$2"
    if grep -qF "$needle" "$file"; then
        die "$file still contains stale string: \"$needle\""
    fi
}

section "scripts/check-doc-counts: PORT_STATUS.md header"
# The header must repeat the table's actual counts. Drift = fail.
expect_contains rust/PORT_STATUS.md "Catalog total: **${TOTAL}** entries"
expect_contains rust/PORT_STATUS.md \
    "Verified: **${VERIFIED}** | Partial: **${PARTIAL}** | Compatibility exceptions: **${COMPAT}** | Missing: **${MISSING}**"

section "scripts/check-doc-counts: rust/README.md status row"
expect_contains rust/README.md \
    "| **Total** | **${TOTAL}** | **${VERIFIED}** | **${PARTIAL}** | **${COMPAT}** | **${MISSING}** |"

section "scripts/check-doc-counts: catalog count consistent across all docs"
# The catalog total (${TOTAL}) must read identically in every doc that
# states it — README.md, rust/README.md, rust/ROADMAP.md, rust/PORT_STATUS.md.
# (PORT_STATUS header is asserted above.) This is what would have caught the
# 168-vs-169 drift between README/ROADMAP and PORT_STATUS.
expect_contains README.md \
    "| **Total** | **${TOTAL}** | **${VERIFIED}** | **${PARTIAL}** | **${COMPAT}** | **${MISSING}** |"
expect_contains rust/README.md "${TOTAL} user-facing IDs"
expect_contains rust/ROADMAP.md "out of ${TOTAL} catalog entries"

section "scripts/check-doc-counts: web demo catalog vs documented coverage"
# The web workbench picker (web/src/lib/catalog.ts) is a CURATED subset of
# the ${TOTAL}-id Rust catalog. web/COVERAGE.md documents the exact count +
# delta. Fail if the picker drifts from the documented numbers (this is what
# would catch someone adding/removing a web entry without updating the doc).
if [ -f web/src/lib/catalog.ts ] && [ -f web/COVERAGE.md ]; then
    WEB_CATALOG_COUNT=$(grep -cE '^[[:space:]]+"id":' web/src/lib/catalog.ts)
    WEB_DELTA=$((TOTAL - WEB_CATALOG_COUNT))
    info "  web picker entries          = $WEB_CATALOG_COUNT (rust catalog = $TOTAL, delta = $WEB_DELTA)"
    if [ "$WEB_CATALOG_COUNT" -gt "$TOTAL" ]; then
        die "web catalog ($WEB_CATALOG_COUNT) exceeds the rust catalog ($TOTAL) — a web id has no rust counterpart"
    fi
    expect_contains web/COVERAGE.md "lists **${WEB_CATALOG_COUNT}** of them"
    expect_contains web/COVERAGE.md "## The ${WEB_DELTA} IDs not in the curated picker"
    # ultracode (the colour symbology) must stay in the web picker.
    if ! grep -qE '"id":[[:space:]]*"ultracode"' web/src/lib/catalog.ts; then
        die "ultracode (the colour symbology) is missing from the web catalog"
    fi
    ok "web catalog count ($WEB_CATALOG_COUNT) consistent with web/COVERAGE.md"
else
    info "web catalog or COVERAGE.md absent — skipping web cross-check"
fi

section "scripts/check-doc-counts: stale-string blocklist"
# Any of these strings indicate a previously-accurate doc that has
# drifted out of agreement with the live numbers. Adding a new
# accurate header should NOT use any of these literals.
STALE_STRINGS=(
    "Catalog total: **154**"
    "Catalog total: **162**"
    "Verified: **146**"
    "Compatibility exceptions: **8**"
    "Verified: **154** | Partial: **0** | Compatibility exceptions: **8**"
    "9 upstream encoders are not yet ported"
    "Missing: **9**"
    "Missing: **1**"
    # Stage-19B audit: the CHANGELOG header used to read
    # "154 verified / 0 partial / 8 documented compatibility
    # exceptions ... out of 162 entries". Live numbers are
    # 169 verified / 0 partial / 0 compat / 0 missing out of 169.
    "154 verified / 0 partial / 8 documented"
    "out of 162 entries"
    "All 154 catalog entries are reachable"
    # Stage-19B audit: PORT_STATUS once said the QR family was
    # "8 rows" but listed 9 ids. Live header now says 9.
    "QR Code family (8 rows:"
    # Stage-19B audit: PORT_COMPLETENESS once said "154-row
    # symbology catalog from app/symbologies.py". Live doc
    # describes the 168-row Rust catalog.
    "154-row symbology catalog"
    # Stage-19B audit: rust/README used to enumerate 8
    # QR-family compatibility-exception ids as pending work.
    # The bucket is empty after Stages 16 + 17c.
    "the 8 non-verified rows form a"
    # Stage-20.4 audit: do not overclaim "every symbology BWIPP
    # supports" while `ultracode` remains intentionally
    # out-of-scope and `posicode` is only partial. The honest
    # phrasing is "every monochrome user-facing BWIPP catalog
    # encoder except `ultracode`, with `posicode` partial".
    "every symbology BWIPP supports"
    # Stage-18d/g audit: PORT_STATUS used to claim that the
    # stacked-renderer, composite-renderer, dot-renderer,
    # RS-GF(64), and WASM-bindings cross-cutting layers were
    # "not yet started". They have long since landed and the
    # doc has been rewritten; fail loudly if any of those
    # strings reappear.
    "Stacked / multi-row model** - not yet started"
    "Composite linear + 2D model** - not yet started"
    "Dot/circular renderer (DotCode)** - not yet started"
    "Reed-Solomon over GF(64)** (USPS OneCode + Australia Post) - not yet started"
    "WASM bindings** - not yet started"
    # Stage-22 audit (POSICODE went out-of-scope → partial → verified):
    # Pre-Stage-22 claims: 167 rows, 3 partials, posicode out-of-scope.
    # Stage-22a-22c.1 mid-state: 168 rows, 4 partial (posicode partial).
    # Stage-22d final state: 168 rows, 3 partial, posicode VERIFIED.
    # Stage 3a/b/c final state: 168 rows, 2 partial (code16k VERIFIED).
    # Stage 3d final state: 168 rows, 1 partial (codeone VERIFIED).
    # Catch every stale phrasing so future regressions fail loudly.
    # (NB: bare "167 rows" / "167-row" are now ambiguous between the
    # legitimate "167 verified" claim and the stale "167 total" one,
    # so the catch-all goes by total-catalog phrasing instead.)
    "Catalog total: **167**"
    "167 user-facing IDs"
    "167 user-facing ids"
    "**167-row**"
    "167 catalog entries"
    "167 catalog rows"
    "All 167 catalog entries"
    "out of 167 catalog"
    "164 verified + 3 partial"
    "164 verified, 3 partial"
    "164 verified / 3 partial"
    "164 verified + 4 partial"
    "164 verified, 4 partial"
    "164 verified / 4 partial"
    "165 verified + 3 partial"
    "165 verified, 3 partial"
    "165 verified / 3 partial"
    "The 4 partial rows"
    "4 partial / 0 compatibility exceptions"
    "3 partial / 0 compatibility exceptions"
    "5 intentionally out-of-scope"
    "5 are intentionally out of scope"
    # Stage-22d: POSICODE is now fully verified; old partial /
    # out-of-scope / deprecated framings must not return.
    "posicode is a deprecated linear"
    "posicode chosen not to ship"
    "chosen not to ship"
    "Two upstream encoders (\`posicode\`, \`ultracode\`)"
    "except \`posicode\` and \`ultracode\`"
    "posicode is partial"
    "with \`posicode\` partial"
    # Stage-22: don't let stale Stage-22a-only / Stage-22b-only /
    # Stage-22c.1-only wording linger after Stage 22d lands.
    "Stage 22a only"
    "Stage 22b only"
    "Stage 22c.1 (this revision)"
    # Stage 3a/b/c: code16k is now verified. Old "code16k partial"
    # phrasings must not return.
    "code16k\` is partial"
    "code16k (partial)"
    # Stage 3d: codeone is now verified too. Old "codeone partial"
    # phrasings must not return.
    "codeone\` is partial"
    "codeone (partial)"
    "166 verified + 2 partial"
    "166 verified, 2 partial"
    "166 verified / 2 partial"
    # Stage 3e: code49 is now verified too. Old "code49 partial"
    # phrasings must not return.
    "code49\` is partial"
    "code49 (partial)"
    "167 verified + 1 partial"
    "167 verified, 1 partial"
    "167 verified / 1 partial"
    "The 1 partial row"
    "1 partial / 0"
    # Stage-A8d docs audit: ultracode is shipped + byte-for-byte verified
    # (Encoded::ColorMatrix + colour renderers), and the catalog total is
    # 169. The old "ultracode out of scope / monochrome-only" framing and
    # the 168 catalog count must never come back.
    "monochrome-only"
    "168 user-facing IDs"
    "168 catalog entries"
    "out of 168 catalog"
    "out of 168 entries"
    "except \`ultracode\`"
    "remain the answer"
    "Tracked as Stage 4 work"
    # Stage-A8d docs audit: stale test counts (real run: 1894 lib unit +
    # 38 integration + 57 doctest + 37 wasm-bindgen). The old self-
    # contradictory wasm counts (15 vs 30) must not return.
    "844 tests"
    "762 unit"
    "15 wasm-bindgen"
    "All 30 WASM tests"
)
for s in "${STALE_STRINGS[@]}"; do
    for f in rust/README.md rust/PORT_STATUS.md rust/AUDIT.md rust/ROADMAP.md rust/COMPATIBILITY_EXCEPTIONS.md rust/GOLDEN_COVERAGE.md README.md; do
        if [ -f "$f" ]; then
            expect_absent "$f" "$s"
        fi
    done
done

section "scripts/check-doc-counts: machine-inventory consistency"
# Cross-check: build_inventory.py reports the same compat-exception
# count as PORT_STATUS.md's verified table (modulo aliasing rules).
# The inventory uses bwip-js's upstream ID set, which is smaller than
# our catalog (it doesn't include split aliases like pzn7/pzn8), so
# we don't expect equal totals — only that BOTH sides report 0
# missing and that the compat-exception buckets are consistent.
INV_MISSING=$(python3 -c '
import json
d = json.load(open("rust/tools/inventory/inventory_diff.json"))
print(d["summary"]["missing"])
')
if [ "$INV_MISSING" != "0" ]; then
    die "inventory_diff.json reports $INV_MISSING missing upstream encoders, but PORT_STATUS claims none"
fi

INV_COMPAT=$(python3 -c '
import json
d = json.load(open("rust/tools/inventory/inventory_diff.json"))
print(d["summary"]["compatibility_exception"])
')
# PORT_STATUS may have its own catalog-only compat-exception rows
# (e.g. a row that exists in the local catalog but not in upstream
# bwip-js). Allow PORT_STATUS_COMPAT >= INV_COMPAT.
if [ "$COMPAT" -lt "$INV_COMPAT" ]; then
    die "inventory_diff.json reports $INV_COMPAT compat-exception upstream encoders but PORT_STATUS.md only counts $COMPAT"
fi

ok "doc counts in agreement with PORT_STATUS.md table + machine inventory"
