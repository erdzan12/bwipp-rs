# bwipp-rs roadmap

Status at this snapshot: **169 verified, 0 partial, 0 compatibility
exceptions, 0 missing** out of 169 catalog entries — **every row
in the catalog is byte-for-byte verified against bwip-js for every
default-options BWIPP path**. POSICODE was promoted in Stage 22d,
Code 16K in Stages 3a/3b/3c, Code One in Stage 3d, and Code 49 in
Stage 3e (SAM treated as opt-in, consistent with how other rows
treat `parsefnc`/`sam`). WASM target compiles to a ~430 KB `.wasm`
with a small JS-friendly API. See
[`PORT_STATUS.md`](PORT_STATUS.md) for the per-symbology breakdown
and [`AUDIT.md`](AUDIT.md) for the verification-strength matrix.

The full catalog is reachable end-to-end. There are no remaining
compatibility exceptions: the QR Code family graduation landed
across Stages 16 (default-on native QR encoder), 15a–15f (rMQR
ground-up port), and 17c (gs1qrcode FNC1-first native path). The
native QR encoder is byte-for-byte verified against bwip-js on a
48-row corpus (24 Full V1–V40 × L/M/Q/H samples + 8 Micro M1–M4 × valid
EC levels + 16 rMQR R7×_..R17×_ × M/H). The historical QR-family
compat-exception entries in [`COMPATIBILITY_EXCEPTIONS.md`](COMPATIBILITY_EXCEPTIONS.md)
are kept only as history; the bucket is empty.

The roadmap below tracks the next iteration of hardening — public-
release surface polish, broader corpus coverage, and durable
upstream-tracking work — rather than missing implementations.

## Newly verified — historical context

### MaxiCode (1200 LOC, verified)
Fixed-size hexagonal symbol (33 rows of 30 modules). Modes 2–6 all
wired and byte-verified vs bwip-js. Reed-Solomon over GF(2⁶) with
per-half secondary check (k=20 for modes 2/3/4/6, k=28 for mode 5).
Hexagonal renderer uses `Encoded::Hex(MaxiCodeSymbol)`.

### Aztec Code (1500 LOC, verified)
Compact L1-L4 and full L1-L32 implemented; bull's-eye finder, mode
message, Reed-Solomon over GF(2⁴/2⁶/2¹²) by size, and 36 fixed-size
variants. DP encoder includes Byte mode for UTF-8 multibyte input
and BWIPP-style pair pre-compression (CR/LF, ". ", ", ", ": ").
Verified against a 27-input bwip-js corpus.

### Han Xin Code (1900 LOC, verified)
Chinese 2D code (GB/T 21049-2007). Binary mode + 13-stride codeword
interleave + GF(256) data RS (poly 355) + GF(16) function-info RS
(poly 19) + 4 corner finders + alignment cleanup + 68-cell
function-info zone + 4 mask functions + `evalfull` auto-mask scoring
(N1 + N3 finder-lookalike penalties). 84 size versions (23×23 to
189×189). Verified against 6 byte-for-byte `pixs` oracles spanning
v1/v2 and all 4 masks, plus a 24-case mask-score corpus.

### Composite codes (17 of 17 verified)
Linear symbology + CC-A / CC-B / CC-C 2D companion. Each composite
combines an existing linear encoder with one of:

* CC-A — small MicroPDF417 variant (we have MicroPDF417 verified)
* CC-B — MicroPDF417 (verified)
* CC-C — PDF417 (verified, used only with GS1-128 since the linear
  half is wide enough to accommodate full PDF417)

The 17 verified composite entries decompose into:
* 8 EAN/UPC composites (4 carriers × CC-A + CC-B)
* 3 GS1-128 composites (CC-A, CC-B, CC-C)
* 6 DataBar composites (Omni / Expanded / Limited × CC-A + CC-B)

`composite_gs1_128_ccc` is pinned by
`composite::tests::encode_gs1_128_ccc_dimensions_match_bwip_js`,
`encode_gs1_128_ccc_matches_bwip_js_separator_and_linear`, and
`encode_gs1_128_ccc_matches_bwip_js_cc_row_0_first_cells`
against the canonical `(01)04012345123456|(99)1234567`
input — 154×49 pixs match byte-for-byte.

## Long tail

### Substrate-level QR / DataMatrix improvements
The `qrcode` and `datamatrix` crates we delegate to don't always
match BWIPP byte-for-byte for arbitrary inputs because their
mode-selectors run different heuristics. The produced symbols are
valid and decode to the same payload, but BWIPP-byte-for-byte
matching would need either:

* upstream PRs adding "match BWIPP's mode selector" options, or
* a pure-Rust port of the QR / DataMatrix encoders.

Either is a significantly larger commit than any individual encoder
port and would touch every symbology that delegates to these
substrates (QR family + DataMatrix family + their HIBC / GS1 / Swiss
QR / NTIN / PPN / DP Postmatrix / Mailmark wrappers).

### Upstream BWIPP encoders — coverage status

For the **complete upstream comparison** see
[`PORT_COMPLETENESS.md`](PORT_COMPLETENESS.md). Of bwip-js's 110
encoders the project's machine inventory currently reports:

* **88 implemented** with their own Rust variant in `Symbology`.
* **11 alias_only** — upstream-generic names that route to our
  more-specific Rust variants via `Symbology::from_id`. Examples:
  `pzn` → `pzn7` / `pzn8`, `auspost` → the four specific Australia
  Post variants, `ean13composite` → CC-A / CC-B sub-variants, etc.
  Every `alias_only` route is exercised by `Symbology::from_id` tests.
* **0 compatibility_exception**, **0 partial**, **0 missing**
  after Stage 22d promoted POSICODE to verified.

### Delivered: ultracode (the former out-of-scope encoder)

`ultracode` — the catalog's single colour 2D symbology — was the one
encoder this project formerly listed as out of scope. It is now
**delivered and verified** (Stage 4). The `Encoded` model now carries
colour as well as monochrome output: it gained a `ColorMatrix`
variant (full set:
`Linear` / `Matrix` / `Postal4State` / `Stacked` / `Dots` / `Hex` /
`ColorMatrix`), and the SVG/PNG renderers paint each cell from the
6-colour `ULTRACODE_PALETTE`. The encoder (RS over GF(283), tile-grid
layout) is byte-for-byte verified against bwip-js. See
[`PORT_STATUS.md`](PORT_STATUS.md).

### Intentionally out of scope

Recorded in [`PORT_COMPLETENESS.md`](PORT_COMPLETENESS.md) with
rationale:

* `raw` / `symbol` / `gs1-cc` — internal bwip-js dispatch helpers,
  not standalone encoders.

`posicode` was previously listed here as out-of-scope and then
partial; as of Stage 22d it is **verified** — all four versions
(`a`, `b`, `limiteda`, `limitedb`) are byte-for-byte verified
against bwip-js. The auto-encoder state machine (set selection /
LA1+LA0 latches / SF0+SF1+SF2 shifts / FN4 ASCII↔extended-ASCII
transitions) plus the single-set limited variants are all pinned
by 22 sbs goldens captured from bwip-js 4.10.1.

This project covers every user-facing BWIPP catalog encoder,
monochrome and colour alike.

## Cross-cutting work

### wasm-bindgen-test coverage
`tests/wasm.rs` covers 37 paths (run via `wasm-pack test --node --
--no-default-features --features wasm`):

- `listSymbologies` non-emptiness
- `renderSvg` / `renderPng` for QR Code, DotCode
- `renderSvg` for EAN-13, UPC-A, Code 128, Code 39, DataBar Expanded,
  DataBar Expanded Stacked, Han Xin Code, Aztec Code (with UTF-8
  multibyte input), Aztec Compact, Aztec Rune, MaxiCode (hex grid
  via Encoded::Hex), PDF417, MicroPDF417, Codablock-F, Data Matrix,
  Micro QR Code, DMRE, Channel Code, GS1 DataMatrix, GS1 DL
  DataMatrix, USPS IMb, Mailmark with the `type=29` option flow
  (exercises the JsOpts path), GS1 QR Code (FNC1-first-position),
  Composite GS1-128 CC-C (the most complex composite path)
- Error path for unknown symbology

All 30 compile under `wasm32-unknown-unknown --features wasm`. The
wasm-pack pass is already wired into `scripts/ci-golden.sh`.

### docs.rs documentation polish
The crate compiles with `#![deny(missing_docs)]` so all public items
are documented, and ~35 of the top-traffic `pub fn encode*` entry
points now carry compilable `# Example` blocks alongside the crate-
level error-handling walkthrough (55 doc tests in total). The
remaining polish work is module-level `//!` overview examples on the
deeper-tail encoders (book_codes, identleitcode, flattermarken,
ean_combined, postal_misc helpers) — nice-to-have rather than blocker.

### crates.io publish
The `Cargo.toml` metadata is in shape (keywords, categories,
repository, documentation, readme, license). The remaining gate is a
clean test run and confidence in the verified-vs-substrate-caveat
split (PORT_STATUS now documents both honestly).

## Suggested next iteration

The full catalog is reachable end-to-end. Catalog is at
**169 verified / 0 partial / 0 compatibility exceptions / 0 missing**
after the QR-family graduation pipeline (Stages 15a–17c) landed
and POSICODE was promoted to verified in Stage 22d (full BWIPP
auto-encoder for versions `a`/`b` joined the Stages 22b + 22c.1
single-set encoders).
Most of the historical roadmap items (Mailmark Type 29, MaxiCode
set-C/D/E, Aztec byte-state, Han Xin Code, GS1-128 CC-C, the entire
QR family) are verified.

Suggested next directions, in roughly decreasing order of impact:

1. **Re-enable hosted GitHub Actions** after the project goes public.
   The workflow files at `.github/workflows/ci.yml` and
   `rust/.github/workflows/ci.yml` are already future-ready — they
   call the same `scripts/ci-*.sh` files. Uncomment the `push` and
   `pull_request` triggers when ready.
2. **Upstream a BWIPP-compatible mask scorer to the `qrcode`
   crate** to graduate the `gs1qrcode` row from compatibility
   exception back to verified. Lowest-cost option if maintainers
   accept it; would also tighten Micro QR / Swiss QR / HIBC QR.
3. **Expand wasm-pack browser coverage** — at 30 tests across the
   major linear / 2D / composite / GS1 / HIBC / DataBar / postal
   families, but every test is a structural "renders a `<svg>`" smoke;
   the JS surface still doesn't get the byte-for-byte logical-golden
   treatment the native crate enjoys. Promoting at least the simpler
   linear (Code 39 / EAN / UPC) wasm tests to compare `raw().sbs`
   against `bwipp::render_svg` parsed bars is the next step.
