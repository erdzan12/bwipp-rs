#!/usr/bin/env python3
"""Build `rust/tests/golden_manifest.json` from PORT_STATUS + GOLDEN_COVERAGE.

This is the executable replacement for "trust that GOLDEN_COVERAGE.md
mentions every verified row". The manifest emitted by this script is
consumed by `scripts/check-golden-manifest.sh`, which runs
`cargo test --lib -- --list` and asserts every test function named in
the manifest actually exists in the compiled test binary.

The manifest schema is:

    {
      "version": 1,
      "generated_from": [...],
      "entries": [
        {
          "id": "<catalog id>",
          "status": "verified" | "partial" | "compatibility exception" | "missing",
          "tests": ["module::tests::fn_name", ...],
          "notes": "<optional>"
        },
        ...
      ]
    }

Heuristics:
  * `tests` are extracted from any `mod::name_part::tests::fn_name`
    style identifier that appears inside the GOLDEN_COVERAGE entry
    for the row (or a slash-rollup that expands to the row id).
  * Rows with no extracted tests are tagged with the empty list and
    flagged in the script's stderr — those are gaps the human author
    needs to fill before the manifest checker can prove them.

Run from repo root:
    python3 scripts/build-golden-manifest.py
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PORT_STATUS = REPO / "rust" / "PORT_STATUS.md"
GOLDEN_COVERAGE = REPO / "rust" / "GOLDEN_COVERAGE.md"
MANIFEST = REPO / "rust" / "tests" / "golden_manifest.json"

ROW_RE = re.compile(
    r"^\| `([a-zA-Z0-9_-]+)` \|[^|]+\|[^|]+\|[^|]+\| ([a-z ]+) \|", re.M
)
# Test function references span three shapes:
#   * Tests inside `mod tests`: `code128::tests::matches_bwip_js_raw_sbs`
#   * Tests in `tests/integration.rs`: `integration::alias_ids_route_to_canonical_symbology`
#   * Tests in `tests/wasm.rs`: `wasm::renders_qrcode_svg`
#
# Match any backticked identifier of the form `seg1::seg2[::seg3...]`
# where every segment is a lowercase Rust identifier and there's at
# least one `::`. The post-filter in `is_test_path` drops mentions
# that don't look like test paths (e.g. module references or trait
# paths).
TEST_FN_RE = re.compile(
    r"`(?:crate::)?([a-z_][a-z0-9_]*(?:::[a-z_][a-z0-9_]*)+)`"
)


def is_test_path(s: str) -> bool:
    """Filter the universal `seg::seg::seg` regex to only test paths.

    Heuristic: a Rust test path either contains the literal `::tests::`
    (the canonical in-module-tests case) or starts with one of the
    known integration-test top-level module names (`integration::`,
    `wasm::`). Anything else is probably a type or function reference,
    not a test path."""
    if "::tests::" in s:
        return True
    if s.startswith("integration::") or s.startswith("wasm::"):
        return True
    return False


# Top-level Rust modules in the crate. Test paths starting with one
# of these are already fully qualified; anything else gets a
# `symbology::` prefix to match the cargo `--list` output.
TOPLEVEL_MODS = {
    "encoding",
    "error",
    "options",
    "render",
    "symbology",
    "wasm",
    "util",
    # Integration tests live in `tests/*.rs` and surface under the
    # bare top-level filename in cargo's `--list` output.
    "integration",
}


def canonicalize_test_path(s: str) -> str:
    """Normalize a doc-cited test path to its cargo `--list` form.

    The crate's per-symbology test modules nest under `symbology::`,
    but the markdown docs habitually elide the prefix (e.g. they
    write `code39::tests::matches_bwip_js_raw_sbs` instead of
    `symbology::code39::tests::matches_bwip_js_raw_sbs`). Add the
    prefix when the first segment isn't already a known top-level
    module."""
    first = s.split("::", 1)[0]
    if first in TOPLEVEL_MODS:
        return s
    return f"symbology::{s}"


def expand_slash_rollup(seed: str, pieces: list[str]) -> list[str]:
    """Same expansion rule the existing check-golden-coverage.sh uses."""
    out = [seed]
    for piece in pieces:
        if not piece:
            continue
        if piece.startswith("_"):
            i = seed.rfind("_")
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
            if (
                len(piece) >= 2
                and piece[1:].isdigit()
                and re.search(r"[a-z]\d+$", seed)
            ):
                j = re.search(r"[a-z]\d+$", seed).start()
                out.append(seed[:j] + piece)
                continue
            j = len(seed)
            while j > 0 and seed[j - 1].isalpha():
                j -= 1
            out.append(seed[:j] + piece)
            continue
    return out


def parse_port_status() -> list[dict]:
    """Return [{'id': ..., 'status': ...}] for every PORT_STATUS row."""
    text = PORT_STATUS.read_text()
    out = []
    for m in ROW_RE.finditer(text):
        out.append({"id": m.group(1), "status": m.group(2).strip()})
    return out


def extract_id_to_tests() -> dict[str, set[str]]:
    """Two-pass parse of GOLDEN_COVERAGE.md.

    Pass 1 (sectioning): split the document on `## ` headings into
    sections, then collect each section's ids and tests separately.
    Pass 2: assign every test in a section to every id named in
    that same section. This handles the GS1 Composite case where
    the test bullets precede the id list.

    Within a single line, tests are also attached to the ids named
    on that very line (the table-row case), so a row like
    `| `code39` | Linear | `code39::tests::X` | …` doesn't leak its
    test name to other ids in the same section."""
    text = GOLDEN_COVERAGE.read_text()
    id_to_tests: dict[str, set[str]] = {}
    sections: list[dict] = [{"ids": set(), "tests": set(), "lines": []}]
    for line in text.splitlines():
        if line.startswith("## "):
            sections.append({"ids": set(), "tests": set(), "lines": []})
        sections[-1]["lines"].append(line)

    def line_ids(line: str) -> list[str]:
        ids: list[str] = []
        for m in re.finditer(r"`([a-z][a-zA-Z0-9_/-]+)`", line):
            tok = m.group(1)
            if "/" not in tok:
                ids.append(tok)
                continue
            pieces = tok.split("/")
            ids.extend(expand_slash_rollup(pieces[0], pieces[1:]))
        for m in re.finditer(
            r"`([a-z][a-zA-Z0-9_-]+)`(?:/`([a-z0-9_-]+)`)+", line
        ):
            parts = re.findall(r"`([a-z][a-zA-Z0-9_-]+)`", m.group(0))
            if parts:
                ids.extend(expand_slash_rollup(parts[0], parts[1:]))
        # Drop tokens that look like test function names (contain ::)
        # — only treat short barewords as catalog ids.
        return [i for i in ids if "::" not in i]

    def line_tests(line: str) -> list[str]:
        return [
            canonicalize_test_path(t)
            for t in TEST_FN_RE.findall(line)
            if is_test_path(t)
        ]

    for section in sections:
        # Per-section accumulators.
        section_ids: set[str] = set()
        # Tests that appeared in bullet/narrative lines (no ids on
        # the same line). These pool together for ids that have no
        # line-local attribution — the GS1 Composite case.
        unbound_tests: set[str] = set()
        # Track which ids got at least one line-local attribution.
        bound_ids: set[str] = set()
        for line in section["lines"]:
            lids = line_ids(line)
            ltests = line_tests(line)
            section_ids.update(lids)
            if lids and ltests:
                for tid in lids:
                    id_to_tests.setdefault(tid, set()).update(ltests)
                    bound_ids.add(tid)
            elif ltests and not lids:
                unbound_tests.update(ltests)
        # Distribute unbound section-pool tests ONLY to ids that
        # didn't get a line-local attribution. This avoids leaking
        # the GS1 Composite bullet tests into adjacent table-row
        # sections that happen to be in the same section block (or
        # cross-section bleed when the section split is coarse).
        for tid in section_ids - bound_ids:
            id_to_tests.setdefault(tid, set()).update(unbound_tests)

    return id_to_tests


def classify_oracle(tests: list[str]) -> dict:
    """Derive a coarse oracle-strength classification from the named
    tests. Heuristic: scan for substrings that signal a particular
    proof class.

    Returns a dict with `oracle_type`, `assertion_kind`, and
    `source` fields. The classifier prefers the strongest evidence
    found across the test list (pixs > sbs > codewords > wrapper >
    substrate > alias)."""

    def any_match(substrings):
        return any(any(s in t for s in substrings) for t in tests)

    # Byte-for-byte pixs / matrix oracle.
    has_pixs = any_match([
        "pixs",
        "matches_bwip_js_golden",
        "matches_oracle_full_pipeline",
        "matches_oracle",
        "encode_hello_matches_bwip_js",
        "_pixs_corpus_matches_oracle",
    ])
    # Byte-for-byte sbs / linear oracle.
    has_sbs = any_match([
        "_sbs",
        "sbs_matches_bwipp",
        "sbs_matches_bwip_js",
        "raw_sbs",
        "rm4scc_matches_bwip_js",
        "kix_matches_bwip_js",
        "daft_matches_bwip_js",
        "japanpost",
        "rendered_sbs_matches_bwip_js",
        "two_track_matches_bwip_js",
    ])
    # Byte-for-byte codewords / cws / per-bar oracle.
    has_codewords = any_match([
        "codewords",
        "build_ccs",
        "_cws_",
        "encstrs_match_bwip_js",
        "binval_then_bytes_match",
        "bar_shapes_match_bwip_js",
        "bar_sequence_matches",
        "scores_match",
        "evalfull_scores",
    ])
    # Wrapper composition / delegation proof.
    has_wrapper = any_match([
        "composes_",
        "delegates_to",
        "wraps_with",
        "_renders_with",
        "matches_underlying",
        "matches_encode_for",  # `encode_compact_matches_encode_for_short_input`
        "produces_rect_shape", # `gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai`
        "renders_and_rejects", # `gs1_dl_qrcode_renders_and_rejects_invalid_uri`
        "round_trips_through",
    ])
    # Alias / from_id router proof.
    has_alias = any_match(["alias_ids_route_to_canonical_symbology"])
    # End-to-end render smoke (the weakest acceptable claim).
    has_smoke = any_match(["every_symbology_renders_svg", "every_symbology_renders_png"])
    # Generic "matches_bwip" fallback — broader catch-all for any
    # test that names bwip-js / BWIPP as its oracle without matching
    # the specific keyword patterns above.
    has_generic_bwip = any_match([
        "matches_bwip_js",
        "matches_bwipp",
        "match_bwip_js",
        "match_bwipp",
    ])

    # Strength priority: pixs > sbs > codewords > wrapper > alias > smoke > generic.
    if has_pixs:
        oracle_type = "bwip_js_pixs"
        assertion_kind = "byte_for_byte_matrix"
        source = "bwip-js"
    elif has_sbs:
        oracle_type = "bwip_js_sbs"
        assertion_kind = "byte_for_byte_linear"
        source = "bwip-js"
    elif has_codewords:
        oracle_type = "bwip_js_codewords"
        assertion_kind = "byte_for_byte_codewords"
        source = "bwip-js"
    elif has_wrapper:
        oracle_type = "wrapper_proof"
        assertion_kind = "composition_pinned"
        source = "delegates_to_verified_primary"
    elif has_alias:
        oracle_type = "alias"
        assertion_kind = "from_id_round_trip"
        source = "alias_router_test"
    elif has_generic_bwip:
        oracle_type = "bwip_js_other"
        assertion_kind = "matches_bwip_js"
        source = "bwip-js"
    elif has_smoke:
        oracle_type = "smoke"
        assertion_kind = "renders_without_panic"
        source = "integration_smoke"
    else:
        oracle_type = "unknown"
        assertion_kind = "unknown"
        source = "unknown"
    return {
        "oracle_type": oracle_type,
        "assertion_kind": assertion_kind,
        "source": source,
    }


def main() -> int:
    rows = parse_port_status()
    id_to_tests = extract_id_to_tests()

    entries = []
    gaps = []
    for row in rows:
        tests = sorted(id_to_tests.get(row["id"], set()))
        classification = classify_oracle(tests)
        entry = {
            "id": row["id"],
            "status": row["status"],
            **classification,
        }
        if tests:
            entry["tests"] = tests
        else:
            entry["tests"] = []
            gaps.append(row["id"])
        entries.append(entry)

    manifest = {
        "version": 2,
        "generated_from": [
            "rust/PORT_STATUS.md",
            "rust/GOLDEN_COVERAGE.md",
        ],
        "schema": {
            "oracle_type": [
                "bwip_js_pixs",
                "bwip_js_sbs",
                "bwip_js_codewords",
                "bwip_js_other",
                "wrapper_proof",
                "alias",
                "smoke",
                "unknown",
            ],
            "assertion_kind": [
                "byte_for_byte_matrix",
                "byte_for_byte_linear",
                "byte_for_byte_codewords",
                "matches_bwip_js",
                "composition_pinned",
                "from_id_round_trip",
                "renders_without_panic",
                "unknown",
            ],
        },
        "entries": entries,
    }

    MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {MANIFEST} ({len(entries)} entries)")
    if gaps:
        print(
            f"NOTE: {len(gaps)} rows have no extracted test references "
            "(test discovery only walks GOLDEN_COVERAGE.md; rows "
            "without tests will fall to the alias-coverage integration "
            "test in the manifest checker).",
            file=sys.stderr,
        )
        for gid in gaps[:10]:
            print(f"  - {gid}", file=sys.stderr)
        if len(gaps) > 10:
            print(f"  ... and {len(gaps) - 10} more", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
