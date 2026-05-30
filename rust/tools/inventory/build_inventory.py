#!/usr/bin/env python3
"""Build the full upstream-vs-local symbology inventory diff.

Inputs:
    - rust/tools/inventory/upstream_bwipp.json   (produced by dump_upstream.mjs)
    - rust/tools/inventory/legacy_catalog.json    (legacy reference catalog)
    - web/src/lib/catalog.ts                      (web catalog)
    - rust/src/symbology.rs                       (Rust dispatch table)

Outputs:
    - rust/tools/inventory/project_catalog.json
    - rust/tools/inventory/web_inventory.json
    - rust/tools/inventory/rust_inventory.json
    - rust/tools/inventory/inventory_diff.json
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
INV = REPO / "rust" / "tools" / "inventory"


def load_upstream() -> list[dict]:
    return json.loads((INV / "upstream_bwipp.json").read_text())


def load_python_catalog() -> list[dict]:
    # Legacy reference catalog. Originally parsed from the pre-Rust FastAPI
    # app's `app/symbologies.py`; that app was removed for the public release,
    # so the 135-entry reference catalog is kept here as a static JSON fixture
    # purely to preserve the historical reference column in the
    # upstream-vs-local inventory diff.
    return json.loads((INV / "legacy_catalog.json").read_text())


def load_web_catalog() -> list[dict]:
    src = (REPO / "web" / "src" / "lib" / "catalog.ts").read_text()
    rows: list[dict] = []
    pattern = re.compile(
        r'"id"\s*:\s*"([a-z0-9_-]+)"\s*,\s*"name"\s*:\s*"([^"]*)"\s*,\s*"category"\s*:\s*"([^"]*)"',
        re.MULTILINE,
    )
    for m in pattern.finditer(src):
        cid, name, cat = m.groups()
        rows.append({"id": cid, "name": name, "category": cat})
    return rows


def load_rust_inventory() -> dict:
    src = (REPO / "rust" / "src" / "symbology.rs").read_text()
    # Find the from_id body: everything between `fn from_id(` and the trailing
    # `_ => return None,` closer.
    match = re.search(r"fn from_id.*?_ => return None,", src, re.DOTALL)
    if not match:
        raise SystemExit("could not find from_id body in symbology.rs")
    body = match.group(0)

    # Each arm: one-or-more "ids" separated by | mapping to Symbology::X
    arm_re = re.compile(
        r'((?:"[A-Za-z0-9_-]+"\s*\|\s*)*"[A-Za-z0-9_-]+")\s*=>\s*\{?\s*Symbology::([A-Za-z0-9_]+)'
    )
    aliases: dict[str, str] = {}
    variants: set[str] = set()
    for m in arm_re.finditer(body):
        ids_part, variant = m.groups()
        for id_m in re.finditer(r'"([A-Za-z0-9_-]+)"', ids_part):
            aliases[id_m.group(1)] = variant
        variants.add(variant)

    # canonical id() table maps variant -> id
    id_match = re.search(r"pub fn id.*?match self \{(.*?)\n    \}", src, re.DOTALL)
    canonical: dict[str, str] = {}
    if id_match:
        for m in re.finditer(
            r'Symbology::([A-Za-z0-9_]+)\s*=>\s*"([a-z0-9_-]+)"', id_match.group(1)
        ):
            variant, cid = m.groups()
            canonical[variant] = cid
            variants.add(variant)

    return {
        "aliases": aliases,
        "canonical": canonical,
        "variants": sorted(variants),
    }


def classify(upstream: list[dict], python: list[dict], rust: dict) -> list[dict]:
    """Classify every upstream BWIPP encoder against the local project."""
    # Index: which canonical Rust id is each Symbology variant
    variant_to_canon = rust["canonical"]
    # Reverse index alias -> variant
    alias_to_variant = rust["aliases"]

    # Mapping rules: each upstream id maps to a (status, local_id, rationale)
    # status ∈ {"verified", "compatibility_exception", "partial", "out_of_scope",
    #           "missing"}. The status is for the upstream encoder; mapping says
    #           how it is reachable in our project.
    overrides: dict[str, dict] = {
        # Internal bwip-js dispatch helpers — not encoders.
        "raw": {
            "status": "out_of_scope",
            "rationale": "Internal bwip-js dispatch helper, not an encoder.",
        },
        "symbol": {
            "status": "out_of_scope",
            "rationale": "Internal bwip-js generic-symbol renderer, not a public encoder.",
        },
        "gs1-cc": {
            "status": "out_of_scope",
            "rationale": "Internal composite component used by databar*/ean*/upc* composites. Not a top-level encoder.",
        },
        # Legacy / niche linear codes not represented locally — explicit
        # out-of-scope rationale.
        "mands": {
            "status": "implemented",
            "rationale": (
                "Marks & Spencer seven-digit retailer code. Implemented "
                "as a thin EAN-8 wrapper (`ean::encode_mands`) that "
                "prepends a leading `0` to 7-char input and delegates "
                "to the verified EAN-8 primary; M&S is structurally an "
                "EAN-8 with a specific bar-tail height adjustment "
                "(cosmetic, not preserved by our LinearPattern model — "
                "see `ean::encode_mands` doc). The sbs bar pattern is "
                "byte-identical to BWIPP `mands` output for valid inputs. "
                "Pinned by `ean::tests::mands_8_digit_matches_bwip_js_raw_sbs`, "
                "`mands_7_and_8_digit_forms_match`, "
                "`mands_7_digit_with_bad_post_prepend_check_rejects`, "
                "`mands_rejects_wrong_length`."
            ),
            "local_canonical": "mands",
        },
        "channelcode": {
            "status": "implemented",
            "rationale": (
                "Channel Code (USPS Tray Labels) — linear symbol with "
                "3..8 channels (input is 2..7 ASCII digits). Direct "
                "port of BWIPP's recursive `nextb`/`nexts` enumeration "
                "(channelcode.rs's `Walker`). The arg2/arg1 rotation "
                "across nexts↔nextb hops is preserved exactly. Pinned "
                "by `channelcode::tests::channelcode_matches_bwip_js_raw_sbs` "
                "(4-input sbs corpus byte-for-byte vs "
                "`bwipp.raw(\"channelcode\", v)` across channel counts "
                "3, 3, 4, 6) plus `encode_rejects_short_or_long_or_non_digit_or_overflow`."
            ),
            "local_canonical": "channelcode",
        },
        "posicode": {
            "status": "implemented",
            "rationale": (
                "POSICODE (1D linear, four versions a/b/limiteda/limitedb). "
                "All four versions are byte-for-byte verified against "
                "bwip-js / BWIPP 2026-04-21. The two single-set variants "
                "`limiteda` (Stage 22b) and `limitedb` (Stage 22c.1) use "
                "a shared `encode_limited(data, version)` helper — "
                "limitedb differs only in (a) using the wider "
                "POSICODE_ENCS_LIMITEDB pattern table and (b) bumping "
                "every check-digit d[i] by 1 before cbs construction. "
                "Versions `a` and `b` (Stage 22d, this revision) go "
                "through `encode_normal`, which ports the full BWIPP "
                "auto-encoder state machine: set-0/1/2 three-way lookup, "
                "LA1/LA0 latches, SF1/SF0 single-char shifts, SF2 "
                "shifts into the control-byte set, and FN4-based "
                "ASCII↔extended-ASCII transitions with numSA/numEA-"
                "driven shift-vs-latch threshold (3 at end, 5 mid-"
                "string). Selected via `opts.extras[\"version\"] = "
                "\"a\"/\"b\"/\"limiteda\"/\"limitedb\"`; the default is "
                "`\"a\"` to match BWIPP. 57 unit tests in "
                "`posicode::tests` pin: constant tables, CRC + "
                "decomposition + cbs helpers, the state-machine paths "
                "(direct / SF2 / latch / SF1+SF0 / FN4), and 22 byte-"
                "for-byte sbs goldens captured via "
                "`rust/tools/oracle-posicode.js` (10 limiteda + 7 "
                "limitedb + 7 version-a including FN4 + 5 version-b). "
                "See `rust/src/symbology/posicode.rs`."
            ),
        },
        "code16k": {
            "status": "missing",
            "rationale": "Code 16K stacked 2D. Not in current catalog.",
        },
        "code49": {
            "status": "implemented",
            "rationale": (
                "Code 49 stacked 1D encoder (USS Code 49 — 2..=8 rows × "
                "81 modules). cws-level encoder verified byte-for-byte "
                "against bwip-js logical goldens covering each of the "
                "three encode paths: direct-lookup (uppercase/digit/"
                "punctuation subset), NS-shift base-48 digit packing, "
                "and alpha-path S1/S2 shifts for control/lowercase/"
                "extended-ASCII bytes. The stacked renderer (per-row "
                "10-module left quiet zone + start bar + 4 codeword "
                "pairs from PATTERNS_0/PATTERNS_1 + 4-module stop bar, "
                "separated by 10-zero/70-one/1-zero separator rows + "
                "top/bottom bearer rows) is pinned by `build_ccs` "
                "goldens for 6 inputs (covering each row count r=2 "
                "and r=3 plus each mode) and a 405-cell compressed "
                "pixs golden against bwip-js for the canonical "
                "\"12345\" payload (`encode_pixs_matches_bwip_js_"
                "golden_for_12345`). 20 unit tests in `code49::tests` "
                "cover constants + row-check formula + PATTERNS table "
                "shape + renderer. Stage 3e promoted to verified — "
                "SAM (Symbol Append Mode) chaining and the `append` "
                "chain are opt-in BWIPP options (`sam`/`append` "
                "parameters that the user explicitly passes) "
                "consistent with how POSICODE / Code 16K / Code One "
                "treat their own opt-in `parsefnc` / `sam` knobs. "
                "Without these options, BWIPP fails for over-r=8 "
                "payloads with the same error this encoder emits; "
                "the default-options encoder path is byte-for-byte "
                "BWIPP-matched."
            ),
            "local_canonical": "code49",
        },
        "codeone": {
            "status": "implemented",
            "rationale": (
                "Code One matrix 2D encoder (AIM USS Code One — Versions "
                "A through H plus S-strip and T-strip variants). The "
                "cws-level encoder is byte-for-byte verified against "
                "bwip-js for every default-options BWIPP path: Mode A "
                "(ASCII + digit-pair packing), Mode B (raw 8-bit bytes "
                "— Stage 20.5), Mode CTX (C40 / Text / X12 via "
                "cnvals/tnvals/xvals base-40 packer + CTXvalstocws), and "
                "**Mode D decimal compression** (Stage 3d, this revision: "
                "3-digit groups packed into 10 bits each via "
                "`val = d0*100 + d1*10 + d2 + 1`, with BWIPP's "
                "termination state machine for the trailing < 3 digits "
                "driven by `getnumremcws(j)` × `Drem` interactions). "
                "BWIPP forward-scan `lookup()` for mode selection "
                "(with `$f` Float32 truncation on cost accumulators — "
                "critical for the abcdef → T boundary case), GF(256) "
                "Reed-Solomon ECC (primitive poly 301, matching Data "
                "Matrix), symbol-size picker, and codeword → matrix "
                "placement (mmat grid + column-pattern band + reference "
                "islands + forced black dots). 49 unit tests in "
                "`codeone::tests` including 4 byte-for-byte `pixs` "
                "goldens against bwip-js (`A`, `Hello`, `ABC`, "
                "`ABCDEFG` — 288 cells each), 5 ECC goldens, 11 "
                "lookup-decision goldens (the abcdef→T edge), Mode B "
                "raw-byte tests over the full 0x80–0xFF range, and **9 "
                "Stage-3d Mode D byte-for-byte cws goldens** captured "
                "via `rust/tools/oracle-codeone.js`. Remaining BWIPP "
                "knobs are `parsefnc` (FN1/2/3 escape recognition via "
                "`^FNCx`), `eci` (ECI marker emission), and the "
                "`version` option to force S-10/S-20/S-30 / T-16/T-32/"
                "T-48 symbol shapes — all opt-in options not exercised "
                "by the default encoder path."
            ),
            "local_canonical": "codeone",
        },
        "ultracode": {
            "status": "implemented",
            "rationale": (
                "Ultracode (AIM USS Ultracode) — colour 2D matrix "
                "barcode. The only colour 2D symbology in the BWIPP "
                "catalog (6-colour palette per `ultracode_colormap`: "
                "white/cyan/magenta/yellow/green/black; Reed-Solomon "
                "over GF(283) with α=3 prime modulus 283; tile-based "
                "5-cell layout per `ultracode_tiles`). "
                "Routes through the new `Encoded::ColorMatrix` carrier "
                "with the 8-entry `ULTRACODE_PALETTE` (6 active + 2 "
                "reserved-white slots). Encoder mirrors "
                "`bwipp_ultracode` at `bwip-js/dist/bwip-js-node.js:36733`: "
                "default-options dcws builder (each input byte → one "
                "codeword), `ULTRACODE_METRICS`-driven symbol-size picker, "
                "RS-over-GF(283) ECC via `gen_coeffs` + `rs_ecprime` "
                "(byte-for-byte vs BWIPP `bwipp_rsecprime`), and full "
                "tile-grid layout (separator passes + DCC tile column + "
                "main tile sequence) producing the `rows*6+1 × cols+6` "
                "pixs that BWIPP emits. **18 unit tests pin every stage**, "
                "including `encode_pixs_default_matches_corpus` — an "
                "8-input byte-for-byte pixs oracle covering "
                "single-byte / short ASCII / sentence / digits / letters / "
                "alphanumeric / UTF-8 high-byte / multi-word inputs "
                "(169–513 cells per grid; captured via "
                "`rust/tools/oracle-ultracode.js`). "
                "Opt-in BWIPP knobs (`parsefnc`, `eclevel != EC2`, "
                "`rev=1`, `raw=true`, `link1 != 0`) are not exposed by "
                "the default encoder path — promotable in follow-ups "
                "once their oracle corpora are captured."
            ),
            "local_canonical": "ultracode",
        },
        # Aztec family — Aztec Code Compact / Aztec Rune
        "azteccodecompact": {
            "status": "implemented",
            "rationale": (
                "Aztec Code Compact (forced L1-L4 only). "
                "`aztec::encode_compact` reuses the verified Aztec "
                "encoder but returns `InvalidData` if the payload would "
                "escalate to a full-size symbol. For payloads that fit "
                "compact, output is byte-identical to `aztec::encode` "
                "(which auto-selects compact when possible). Pinned by "
                "`aztec::tests::encode_compact_matches_encode_for_short_input`, "
                "`encode_compact_rejects_payload_that_exceeds_l4`, "
                "`encode_compact_rejects_empty_input`."
            ),
            "local_canonical": "azteccodecompact",
        },
        "aztecrune": {
            "status": "implemented",
            "rationale": (
                "Aztec Rune — fixed 11×11 marker carrying an 8-bit "
                "(0..=255) payload. `aztec::encode_rune` parses the "
                "1-3 digit ASCII input, builds the rune mode word "
                "(7 nibbles, each XOR'd with 10 per BWIPP `bwipp_azteccode` "
                "line 30019), and emits the matrix via the existing "
                "`build_matrix(\"rune\", 0, &[], 6, modebits)` path. "
                "Pinned by `aztec::tests::encode_rune_matches_bwip_js_pixs` "
                "(4-value pixs corpus: 0, 42, 128, 255 — all byte-for-byte "
                "against `bwipp.raw(\"aztecrune\", ...)`)."
            ),
            "local_canonical": "aztecrune",
        },
        # DataMatrix variants
        "datamatrixrectangularextension": {
            "status": "implemented",
            "rationale": (
                "DMRE — Data Matrix Rectangular Extension (ISO/IEC 21471). "
                "`datamatrix_::encode_rectangular_extension` forces "
                "`SymbolList::with_extended_rectangles().enforce_rectangular()` "
                "to make the 17 DMRE additional sizes (8×48..26×64) "
                "available alongside the original 6 rectangular sizes. "
                "Pinned by `datamatrix_::tests::dmre_short_input_matches_bwip_js_size` "
                "(18×8 for `\"12345\"` agrees with bwip-js) and "
                "`dmre_produces_rectangular_for_long_input` (rectangular "
                "shape asserted). For longer payloads the substrate's "
                "preferred-size policy can pick a classic rectangular "
                "size (e.g. 36×16) where BWIPP picks a DMRE size "
                "(80×8); both are spec-compliant. Same substrate-spec "
                "posture as plain `datamatrix`."
            ),
            "local_canonical": "datamatrixrectangularextension",
        },
        "gs1datamatrixrectangular": {
            "status": "implemented",
            "rationale": (
                "GS1 Data Matrix Rectangular — `gs1datamatrix` with the "
                "`shape=rectangular` flag injected. Pinned by "
                "`gs1_2d::tests::gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai`. "
                "Inherits the same `datamatrix` crate substrate as plain "
                "`gs1datamatrix`."
            ),
            "local_canonical": "gs1datamatrixrectangular",
        },
        # Digital Link family
        "gs1dldatamatrix": {
            "status": "implemented",
            "rationale": (
                "GS1 Digital Link Data Matrix — URI validation + plain "
                "Data Matrix encoding of the raw URI (mirrors BWIPP "
                "`bwipp_gs1dldatamatrix` which uses gs1process('dl') "
                "for syntax validation only). Uses `util::gs1::parse_dl_uri` "
                "(light-validation DL URI parser, ~150 LOC) then delegates "
                "to verified `datamatrix_::encode`. Inherits the "
                "datamatrix-crate substrate-spec posture (22×22 size "
                "matches BWIPP for the canonical URI; exact module "
                "pattern not byte-pinned for arbitrary URI input). "
                "Pinned by "
                "`gs1_2d::tests::gs1_dl_datamatrix_matches_bwip_js_size_and_structure` "
                "and `gs1_dl_datamatrix_rejects_invalid_uri`."
            ),
            "local_canonical": "gs1dldatamatrix",
        },
        "gs1dlqrcode": {
            "status": "verified",
            "rationale": (
                "GS1 Digital Link QR Code — thin wrapper that validates the "
                "URI then calls the native QR encoder via "
                "`qrcode_native::encode_with_options` on the raw URI. As of "
                "Stage 16 the QR substrate beneath is byte-for-byte vs bwip-js "
                "on 24 oracle-pinned Full QR corpus rows. URI validation pinned "
                "by `gs1_2d::tests::gs1_dl_qrcode_renders_and_rejects_invalid_uri`."
            ),
            "local_canonical": "gs1dlqrcode",
        },
        "code16k": {
            "status": "implemented",
            "rationale": (
                "Code 16K stacked 1D encoder. The cws-level encoder "
                "now routes everything through the unified "
                "`encode_data_cws_mixed` state machine, which is "
                "byte-for-byte verified against bwip-js for every "
                "default-options BWIPP path: the initial-mode selector "
                "for modes 0/1/2/5/6 (Stage 3b), the full A↔B↔C state "
                "machine with SWA/SWB/SWC latches and SA1/SA2/SB1/"
                "SB2/SC2/SC3 shifts (Stages 3a + 3c), mode-C "
                "SB1/SB2/SB3 trailing-byte shifts (Stage 3c), and "
                "FN4 ASCII↔extended-ASCII transitions via "
                "`insert_fn4_markers` (Stage 3a, mirrors POSICODE's "
                "Stage-22d FN4 pre-encoder pass). Stacked renderer "
                "(start/stop indicators per row, 1-mod separator "
                "lines, top/bottom bearer rows) produces the same "
                "compressed pixs as bwip-js for the canonical \"12\" "
                "payload (`encode_pixs_matches_bwip_js_golden_for_12` "
                "pins all 405 cells). 63 unit tests in `code16k::tests` "
                "including **30 byte-for-byte cws goldens** captured "
                "via `rust/tools/oracle-code16k.js`. Remaining "
                "BWIPP-supported knobs are `parsefnc` (FN1/2/3 escape "
                "recognition) and `sam` (Symbol Append Mode for "
                "payloads beyond r=16), both opt-in options not "
                "exercised by the default encoder path."
            ),
            "local_canonical": "code16k",
        },
        "gs1dotcode": {
            "status": "implemented",
            "rationale": (
                "GS1 DotCode wrapper: parses GS1 AIs via `util::gs1::parse`, "
                "flattens with FNC1 separators per the GS1 spec, lifts to "
                "`&[i16]` (FNC1 → FN1 marker), and drives "
                "`dotcode::encode_with_markers`. Pinned by three bwip-js "
                "logical goldens (GTIN-14 alone, GTIN+lot, GTIN+expiry) "
                "in `gs1_dotcode::tests`. Built on the DotCode encoder's "
                "Gap 2 (encC FN1 emission) + Gap 6 (BIN escape) which both "
                "landed prior to this row's promotion."
            ),
            "local_canonical": "gs1dotcode",
        },
        # Micro QR Rectangular
        "rectangularmicroqrcode": {
            "status": "verified",
            "rationale": (
                "Native byte-for-byte encoder in `src/symbology/qrcode_native/` "
                "covering all 32 ISO/IEC 23941:2022 rMQR sizes (R7×43 .. R17×139) "
                "at EC levels M and H. Pinned cell-for-cell against bwip-js by "
                "`qrcode_native::tests::encode_rmqr_pixs_corpus_matches_oracle` — "
                "16 (size × eclevel × text) corpus rows. Supporting tests pin the "
                "18-cluster formatfimmap, BCH(18,6) fmtval1/fmtval2 tables (128 "
                "entries), 4-corner finder placement, alignment-column timing "
                "strips, and the walker's 104-position traversal order. EC L and "
                "Q correctly rejected per ISO 23941."
            ),
            "local_canonical": "rectangularmicroqrcode",
        },
        # HIBC family extensions
        "hibcazteccode": {
            "status": "implemented",
            "rationale": (
                "HIBC LIC envelope (`+` prefix + mod-43 check) over the "
                "verified Aztec encoder. Pinned by "
                "`hibc::tests::encode_azteccode_composes_format_and_aztec`. "
                "Fix-along: surfaced a real Aztec DP bug — "
                "`sentinel_codeword(STATE_DIGIT, SHIFT_PUNCT)` was missing "
                "the codeword-0 mapping (Aztec spec's PS shift). Now "
                "covered by every Aztec input that crosses Digit→Punct."
            ),
            "local_canonical": "hibc_lic_azteccode",
        },
        "hibcdatamatrixrectangular": {
            "status": "implemented",
            "rationale": (
                "HIBC LIC envelope over Data Matrix Rectangular substrate. "
                "Pinned by "
                "`hibc::tests::encode_datamatrix_rectangular_composes_format_and_datamatrix_rect`. "
                "Inherits the same `datamatrix` crate substrate as plain "
                "`datamatrixrectangular`."
            ),
            "local_canonical": "hibc_lic_datamatrix_rectangular",
        },
        # EAN-14 (GS1-128 wrapper for 14-digit GTIN)
        "ean14": {
            "status": "implemented",
            "rationale": (
                "EAN-14 / GTIN-14. Implemented as a wrapper that computes "
                "the mod-10 check digit (or verifies one if supplied), then "
                "delegates to the verified `gs1-128` primary with input "
                "`(01)<14-digit-gtin>`. Byte-for-byte bwip-js golden pinned "
                "by `gs1_128::tests::ean14_with_13_digit_input_matches_bwip_js_raw_sbs`."
            ),
            "local_canonical": "ean14",
        },
        # Composite variants we currently don't expose
        "databarstackedcomposite": {
            "status": "implemented",
            "rationale": "DataBar Stacked + CC-A/CC-B composite. Splits the upstream `databarstackedcomposite` into explicit `composite_databar_stacked_cca` / `_ccb` variants. CC-A uses ucols=2 (~55-cell width) above the 50-cell-wide stacked linear. Build path: `composite::build_databarstacked_composite(cc_bm, composite_sep_50, stacked_top, stacked_sep, stacked_bot)` with `databarstacked_composite_separator` constructed from the stacked top half via the omni-shared sepfinder at position 18. Verified byte-for-byte vs bwip-js on the 56×24 CC-A canonical and the 56×54 CC-B-forcing payloads.",
            "local_canonical": "composite_databar_stacked_cca",
        },
        "databarstackedomnicomposite": {
            "status": "implemented",
            "rationale": "DataBar Stacked Omnidirectional + CC-A/CC-B composite. Splits the upstream `databarstackedomnicomposite` into explicit `composite_databar_stacked_omni_cca` / `_ccb` variants. CC-A uses ucols=2 above the 50×69 stacked-omni linear (5 logical rows × rowmult [33,1,1,1,33]). Build path: `composite::build_databarstackedomni_composite(cc_bm, composite_sep_50, top, sep1, sep2, sep3, bot)` with the composite separator shared with the plain stacked composite. Verified byte-for-byte vs bwip-js on the 56×80 CC-A canonical and the 56×110 CC-B-forcing payloads.",
            "local_canonical": "composite_databar_stacked_omni_cca",
        },
        "databartruncatedcomposite": {
            "status": "implemented",
            "rationale": "DataBar Truncated + CC-A/CC-B composite. Splits the upstream `databartruncatedcomposite` into explicit `composite_databar_truncated_cca` / `_ccb` variants. CC-A is verified byte-for-byte vs bwip-js on the 100×20 canonical pixs; CC-B is verified byte-for-byte vs bwip-js on the 100×38 CC-B-forcing payload (12 CC-B rows × 2 + 1 separator + 13 linear tiles). Build path: `composite::build_databaromni_composite(cc_bm, linsbs, DATABARTRUNCATED_LINHEIGHT=13)`.",
            "local_canonical": "composite_databar_truncated_cca",
        },
        "databarexpandedstackedcomposite": {
            "status": "implemented",
            "rationale": "DataBar Expanded Stacked + CC-A/CC-B composite. Splits the upstream `databarexpandedstackedcomposite` into explicit `composite_databar_expanded_stacked_cca` / `_ccb` variants. CC uses ucols=4 centered above the 102-wide expanded-stacked linear. The composite separator is built from the linear's top row via the omni-shared sepfinder at positions 19 + 70 (and 19+98k, 70+98k for wider linears). Build path: `composite::build_databarexpandedstacked_composite(cc_pixs, linear_bm, composite_sep)`. Verified byte-for-byte vs bwip-js on the 102×78 CC-A canonical pixs (CC + composite-sep + linear top + sep0 + inter-sep — 7 of 9 logical rows; the remaining 32 linear physical rows are inherited from the standalone-verified `databar_expanded::encode_stacked`). Dimensions also pinned for 102×96 CC-B output.",
            "local_canonical": "composite_databar_expanded_stacked_cca",
        },
        # Upstream "generic composite" names that we expose as _cca/_ccb pairs
        "ean13composite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb (and -ccc for gs1-128).",
            "local_canonical": "composite_ean13_cca",
        },
        "ean8composite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_ean8_cca",
        },
        "upcacomposite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_upca_cca",
        },
        "upcecomposite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_upce_cca",
        },
        "databaromnicomposite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_databar_omni_cca",
        },
        "databarexpandedcomposite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_databar_expanded_cca",
        },
        "databarlimitedcomposite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb.",
            "local_canonical": "composite_databar_limited_cca",
        },
        "gs1-128composite": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into _cca/_ccb/_ccc.",
            "local_canonical": "composite_gs1_128_cca",
        },
        # Upstream `auspost` is a generic name; locally split into 4 service-
        # type encoders.
        "auspost": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into customer/reply/routing/redirection.",
            "local_canonical": "auspost_customer",
        },
        # Codabar: upstream uses `rationalizedCodabar`; we expose as `codabar`.
        "rationalizedCodabar": {
            "status": "alias_only",
            "rationale": "Upstream long-form name; we expose as `codabar`.",
            "local_canonical": "codabar",
        },
        # PZN: upstream uses a single `pzn` id that auto-picks PZN7 vs PZN8 by
        # input length. Our local catalog requires the caller to pick the
        # specific PZN7 or PZN8 id; document as alias_only.
        "pzn": {
            "status": "alias_only",
            "rationale": "Upstream generic name; locally split into pzn7/pzn8.",
            "local_canonical": "pzn7",
        },
        # QR family — as of Stage 16 the default Cargo feature
        # `prefer-native-qrcode` routes all QR rows through the
        # BWIPP-faithful native encoder (`src/symbology/qrcode_native/`).
        # Verified byte-for-byte on 48 oracle-pinned corpus rows
        # (24 Full V1-V40 + 8 Micro M1-M4 + 16 rMQR R7-R17). The
        # `qrcode_::encode` substrate is preserved as a feature-gated
        # opt-out for callers who want the upstream-crate path.
        "qrcode": {
            "status": "verified",
            "rationale": (
                "Native bwipp-faithful encoder (`src/symbology/qrcode_native/`) "
                "default since Stage 16. Byte-for-byte verified vs bwip-js on "
                "24 oracle-pinned Full QR rows (V1–V40 × L/M/Q/H samples) by "
                "`qrcode_native::tests::encode_full_qr_pixs_corpus_matches_oracle`. "
                "Substrate (`qrcode` crate) preserved as opt-out feature."
            ),
            "local_canonical": "qrcode",
        },
        "microqrcode": {
            "status": "verified",
            "rationale": (
                "Native encoder default since Stage 16. Byte-for-byte verified "
                "on 8 oracle-pinned Micro rows (M1–M4 × valid EC levels) by "
                "`qrcode_native::tests::encode_micro_qr_pixs_corpus_matches_oracle`."
            ),
            "local_canonical": "microqrcode",
        },
        "swissqrcode": {
            "status": "verified",
            "rationale": (
                "Wraps the native QR encoder via `qrcode_native::encode_with_options` "
                "with `eclevel=M` forced (Swiss QR-bill spec mandate). Composition "
                "pin at `swiss_qr::tests::composes_eclevel_m_and_qrcode` proves "
                "the wrapper is just SPC-header validation + the QR substrate."
            ),
            "local_canonical": "swissqrcode",
        },
        "gs1qrcode": {
            "status": "verified",
            "rationale": (
                "Native bwipp-faithful encoder (default since Stage 17c) via "
                "`qrcode_native::encode_gs1_qrcode`. Bit-stream prefix is the "
                "ISO/IEC 18004 Annex L 'FNC1 in first position' mode indicator "
                "(4-bit `0101`), threaded through `compose_segments` with "
                "`fnc1first=true`. The fnc1-aware auto-select skips BWIPP's "
                "non-GS1 EC-upgrade loop (BWIPP honours the requested EC level "
                "verbatim for GS1 QR), so size selection matches BWIPP exactly. "
                "GS1 element-string round-trip + FNC1-first indicator + size "
                "selection (V1 21x21 for `(01)04012345123456`) are pinned by "
                "`gs1_2d::tests::gs1_qrcode_fnc1_first_position_mode_indicator_is_0101`, "
                "`gs1_qrcode_differs_from_plain_qr_of_same_payload`, "
                "`gs1_qrcode_optimal_segmentation_matches_bwipp_size`, "
                "`gs1_qrcode_with_explicit_version_override`, and "
                "`gs1_qrcode_payload_round_trips_through_ai_parser`. "
                "The `qrcode` crate substrate path is preserved as an opt-out "
                "via `--no-default-features`."
            ),
            "local_canonical": "gs1qrcode",
        },
        "hibcqrcode": {
            "status": "verified",
            "rationale": (
                "HIBC LIC wrapper (format() + check char) over the native QR "
                "encoder via `qrcode_native::encode_with_options`. Composition "
                "pinned by `hibc::tests::encode_qrcode_composes_format_and_qrcode`. "
                "The QR substrate beneath is byte-for-byte vs bwip-js."
            ),
            "local_canonical": "hibc_lic_qrcode",
        },
    }

    classified = []
    for entry in upstream:
        bcid = entry["bcid"]
        row: dict = {
            "upstream_bcid": bcid,
            "upstream_description": entry.get("desc", ""),
            "upstream_text": entry.get("text", ""),
            "upstream_opts": entry.get("opts", ""),
        }
        override = overrides.get(bcid)
        if override:
            row.update(override)
            row["reachable_via"] = override.get("local_canonical") or None
            # Annotate whether the upstream bcid is actually accepted as an
            # alias in Rust's from_id (a classification can be `alias_only`
            # *and* reachable now that we've added the alias arm).
            row["rust_alias_present"] = bcid in alias_to_variant
            if bcid in alias_to_variant:
                row["rust_variant"] = alias_to_variant[bcid]
        else:
            # Default: assume locally implemented if the upstream bcid is in
            # our from_id table (directly or transitively via alias_to_variant).
            variant = alias_to_variant.get(bcid)
            local_canon = variant_to_canon.get(variant) if variant else None
            if variant and local_canon:
                row["status"] = "implemented"
                row["reachable_via"] = local_canon
                row["rust_variant"] = variant
                row["rust_alias_present"] = True
                row["rationale"] = (
                    f"upstream id `{bcid}` routes through `Symbology::from_id` to `Symbology::{variant}` "
                    f"(canonical id `{local_canon}`)."
                )
            else:
                row["status"] = "unknown"
                row["rust_alias_present"] = False
                row["rationale"] = (
                    "No mapping found in Rust from_id and no explicit "
                    "override classified this upstream id."
                )
        classified.append(row)
    return classified


def main() -> int:
    upstream = load_upstream()
    python_cat = load_python_catalog()
    web_cat = load_web_catalog()
    rust = load_rust_inventory()

    diff = classify(upstream, python_cat, rust)

    INV.mkdir(parents=True, exist_ok=True)

    (INV / "project_catalog.json").write_text(
        json.dumps(
            {"entries": python_cat, "total": len(python_cat)}, indent=2, sort_keys=True
        )
    )
    (INV / "web_inventory.json").write_text(
        json.dumps(
            {"entries": web_cat, "total": len(web_cat)}, indent=2, sort_keys=True
        )
    )
    (INV / "rust_inventory.json").write_text(
        json.dumps(
            {
                "variants": rust["variants"],
                "aliases": rust["aliases"],
                "canonical": rust["canonical"],
                "alias_total": len(rust["aliases"]),
                "variant_total": len(rust["variants"]),
            },
            indent=2,
            sort_keys=True,
        )
    )

    summary = {
        "upstream_total": len(upstream),
        "implemented": sum(1 for r in diff if r["status"] == "implemented"),
        "alias_only": sum(1 for r in diff if r["status"] == "alias_only"),
        "compatibility_exception": sum(
            1 for r in diff if r["status"] == "compatibility_exception"
        ),
        "partial": sum(1 for r in diff if r["status"] == "partial"),
        "missing": sum(1 for r in diff if r["status"] == "missing"),
        "out_of_scope": sum(1 for r in diff if r["status"] == "out_of_scope"),
        "unknown": sum(1 for r in diff if r["status"] == "unknown"),
    }

    (INV / "inventory_diff.json").write_text(
        json.dumps({"summary": summary, "rows": diff}, indent=2, sort_keys=True)
    )

    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
