#!/usr/bin/env python3
"""Generate rust/PORT_COMPLETENESS.md from inventory_diff.json.

This is the authoritative *upstream BWIPP / bwip-js* comparison. It
classifies every upstream encoder as one of:

    - implemented            (locally implemented + verified against
                              bwip-js or BWIPP)
    - alias_only             (upstream generic name; locally exposed as a
                              more specific id; the upstream alias is
                              wired through `Symbology::from_id`)
    - compatibility_exception (implemented but bit-pattern diverges; see
                              rust/COMPATIBILITY_EXCEPTIONS.md)
    - partial                (implemented with documented gaps)
    - missing                (not implemented yet)
    - out_of_scope           (intentionally not implemented; documented
                              rationale)

Counts come from rust/tools/inventory/inventory_diff.json. Run
`python3 rust/tools/inventory/build_inventory.py` first to refresh that
file from upstream bwip-js + the in-tree symbology source.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
INV = REPO / "rust" / "tools" / "inventory"
OUT = REPO / "rust" / "PORT_COMPLETENESS.md"

STATUS_LABEL = {
    "implemented": "implemented",
    "alias_only": "alias_only",
    "compatibility_exception": "compatibility_exception",
    "partial": "partial",
    "missing": "missing",
    "out_of_scope": "out_of_scope",
    "unknown": "unknown",
}

STATUS_ORDER = [
    "implemented",
    "alias_only",
    "compatibility_exception",
    "partial",
    "missing",
    "out_of_scope",
    "unknown",
]


def render() -> int:
    data = json.loads((INV / "inventory_diff.json").read_text())
    summary = data["summary"]
    rows = data["rows"]

    upstream_version = None
    bwipp_version = None
    pkg_path = REPO / "node-sidecar" / "node_modules" / "bwip-js" / "package.json"
    if pkg_path.exists():
        pkg = json.loads(pkg_path.read_text())
        upstream_version = pkg.get("version")
    bwipp_path = (
        REPO / "node-sidecar" / "node_modules" / "bwip-js" / "dist" / "bwipp.mjs"
    )
    if bwipp_path.exists():
        text = bwipp_path.read_text()
        import re

        m = re.search(r"BWIPP_VERSION\s*=\s*'([^']+)'", text)
        if m:
            bwipp_version = m.group(1)

    md: list[str] = []
    md.append("# bwipp-rs upstream port completeness")
    md.append("")
    md.append(
        "This document is the **upstream BWIPP / bwip-js comparison** — the answer "
        "to the question \"is bwipp-rs a complete port of BWIPP?\". For every "
        "encoder upstream bwip-js exposes via `bwipp_symlist`, we record:"
    )
    md.append("")
    md.append("- whether it is implemented locally (and how to reach it),")
    md.append("- whether it is reachable via the upstream BWIPP id through `Symbology::from_id`,")
    md.append("- the exact verification status (verified / compatibility exception / partial / missing / out of scope),")
    md.append("- the rationale for any encoder that is not byte-for-byte implemented.")
    md.append("")
    # Derive the cross-reference numbers so the prose can't drift from the
    # generated table / PORT_STATUS (this is what used to read a stale
    # "168 / 90 / 4"). PORT_STATUS catalog counts come from its table rows.
    import re as _re

    _ps_text = (REPO / "rust" / "PORT_STATUS.md").read_text()
    _ps_rows = _re.findall(
        r"^\| `[a-zA-Z0-9_-]+` \|[^|]+\|[^|]+\|[^|]+\| ([a-z ]+) \|",
        _ps_text,
        _re.M,
    )
    cat_total = len(_ps_rows)
    cat_verified = sum(1 for s in _ps_rows if s.strip() == "verified")
    cat_partial = sum(1 for s in _ps_rows if s.strip() == "partial")
    n_impl = summary.get("implemented", 0)
    n_alias = summary.get("alias_only", 0)
    n_partial = summary.get("partial", 0)
    n_missing = summary.get("missing", 0)
    n_oos = summary.get("out_of_scope", 0)
    n_total = summary["upstream_total"]
    md.append(
        "If you need the **catalog-internal** port-status table (the project's own "
        f"**{cat_total}-row** symbology catalog reachable via `Symbology::from_id`), see "
        "[`PORT_STATUS.md`](PORT_STATUS.md). The two documents serve different "
        "audiences:"
    )
    md.append("")
    md.append(
        "* **PORT_STATUS** is for users picking a symbology by id; it tracks "
        f"per-row verified / partial / compatibility-exception status ({cat_verified} "
        f"verified + {cat_partial} partial as of this revision)."
    )
    md.append(
        "* **PORT_COMPLETENESS** (this document) is for evaluating coverage of "
        f"the upstream BWIPP / bwip-js encoder set ({n_impl} implemented + {n_alias} "
        f"alias-only + {n_partial} partial + {n_missing} missing + {n_oos} intentionally "
        f"out-of-scope out of {n_total} upstream encoders)."
    )
    md.append("")
    md.append("**Sources of truth**")
    md.append("")
    if upstream_version or bwipp_version:
        md.append(
            "- Upstream: bwip-js"
            + (f" `{upstream_version}`" if upstream_version else "")
            + (
                f" (BWIPP_VERSION = `{bwipp_version}`)"
                if bwipp_version
                else ""
            )
            + " — `bwipp_symlist` enumerates the canonical encoder set."
        )
    md.append(
        "- Local: `rust/src/symbology.rs` "
        "(`Symbology::from_id`, `Symbology::all`, `id()` table)."
    )
    md.append(
        "- Machine-readable diff: "
        "[`tools/inventory/inventory_diff.json`](tools/inventory/inventory_diff.json) "
        "(regenerate via `python3 rust/tools/inventory/build_inventory.py`)."
    )
    md.append("")

    md.append("## Summary")
    md.append("")
    md.append(f"Upstream encoders enumerated: **{summary['upstream_total']}**")
    md.append("")
    md.append("| Status | Count |")
    md.append("|---|---|")
    for s in STATUS_ORDER:
        cnt = summary.get(s, 0)
        md.append(f"| `{STATUS_LABEL[s]}` | {cnt} |")
    md.append("")
    md.append("Acceptance check (no upstream encoder is left unclassified):")
    md.append("")
    md.append(f"- `unknown == 0` → {'PASS' if summary.get('unknown', 0) == 0 else 'FAIL'}")
    md.append("")

    # Per-status sections
    for status in STATUS_ORDER:
        bucket = [r for r in rows if r["status"] == status]
        if not bucket:
            continue
        md.append(f"## {STATUS_LABEL[status].replace('_', ' ').title()} ({len(bucket)})")
        md.append("")
        md.append("| Upstream `bcid` | Local id / reachable via | Rationale |")
        md.append("|---|---|---|")
        for r in sorted(bucket, key=lambda x: x["upstream_bcid"]):
            local = r.get("reachable_via") or "—"
            rationale = (r.get("rationale") or "").replace("|", "\\|")
            md.append(f"| `{r['upstream_bcid']}` | `{local}` | {rationale} |")
        md.append("")

    md.append("---")
    md.append("")
    md.append("## How this is enforced")
    md.append("")
    md.append(
        "`scripts/ci-inventory.sh` regenerates `inventory_diff.json` from upstream "
        "bwip-js + the local Rust source, and fails if any of these invariants "
        "break:"
    )
    md.append("")
    md.append("1. **No `unknown` rows.** Every upstream `bcid` must be explicitly "
              "classified by `rust/tools/inventory/build_inventory.py`.")
    md.append("2. **Every `implemented` / `alias_only` / `compatibility_exception` "
              "row resolves through `Symbology::from_id`.** That field is "
              "`rust_alias_present: true` in the diff.")
    md.append("3. **The diff is up-to-date.** `scripts/ci-inventory.sh` re-runs the "
              "builder and diffs the output against the committed "
              "`inventory_diff.json`; CI fails on drift.")
    md.append("")
    md.append("Run `python3 rust/tools/inventory/build_inventory.py && "
              "python3 rust/tools/inventory/render_completeness.py` after every "
              "change to `rust/src/symbology.rs` or to the upstream bwip-js pin.")
    md.append("")

    OUT.write_text("\n".join(md))
    print(f"wrote {OUT.relative_to(REPO)} ({len(md)} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(render())
