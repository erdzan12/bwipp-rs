# bwipp-rs publish-readiness audit

Date: 2026-05-19  
Auditor: in-repo automated pass (AGENT_GOAL.md)  
Scope: full catalog coverage, CI architecture, web app, docs, unsafe
boundary, golden-test strength.

This document is the independent audit referenced by AGENT_GOAL.md.
It is the single source of truth for what was checked, what passed,
what was changed during the audit, and what compatibility exceptions
remain. Stay aligned with `PORT_STATUS.md` and
`COMPATIBILITY_EXCEPTIONS.md` — anything contradictory between the
three should be reconciled here first.

## 0. Upstream BWIPP / bwip-js completeness

PORT_STATUS.md is honest **about the project's own 168-row catalog** but
that is not the same question as "does bwipp-rs port every BWIPP
encoder?". The second-loop hardening pass added an independent
upstream-completeness check:

* `node-sidecar/node_modules/bwip-js/dist/bwipp.mjs` exposes
  `bwipp_symlist`, the canonical enumeration of every BWIPP encoder
  bwip-js ships. The current pin is bwip-js `4.10.1` (BWIPP_VERSION =
  `2026-04-21`) and lists **110** encoders.
* `rust/tools/inventory/build_inventory.py` regenerates a machine-readable
  diff (`inventory_diff.json`) classifying every upstream `bcid` as one
  of `implemented` / `alias_only` / `compatibility_exception` /
  `partial` / `missing` / `out_of_scope` / `unknown`.
* `rust/tools/inventory/render_completeness.py` turns the diff into
  `rust/PORT_COMPLETENESS.md` — the authoritative upstream-vs-local
  comparison.
* `scripts/ci-inventory.sh` (wired into `ci-local.sh`) re-runs the
  builder, asserts `unknown == 0`, asserts every `implemented`/
  `alias_only`/`compatibility_exception` row resolves through
  `Symbology::from_id`, and fails if the committed diff has drifted.

Current counts (pinned by CI):

| Status                    | Count |
|---------------------------|-------|
| `implemented`             | 87    |
| `alias_only`              | 11    |
| `compatibility_exception` | 0     |
| `partial`                 | 0     |
| `missing`                 | 0     |
| `out_of_scope`            | 5     |
| `unknown`                 | 0     |

> **Note on `partial` here vs in `PORT_STATUS.md`.** The machine
> inventory above answers "does this upstream bwip-js encoder have
> *some* corresponding Rust variant?" — it labels three rows
> (`code16k`, `code49`, `codeone`) `implemented` because each has a
> dedicated Rust variant. The catalog-side `PORT_STATUS.md` answers
> a stricter question — "is the Rust variant verified against
> bwip-js for *all* BWIPP-accepted payloads?" — and labels those
> same three rows `partial` because each rejects a small list of
> BWIPP-supported extension paths (Mode A control bytes for
> `code16k`, Symbol Append Mode chaining for `code49`, Mode B / Mode
> D / FNC for `codeone`) with `Error::InvalidData`. The two counts
> are coherent: same three rows, different question.

The 11 `alias_only` rows are upstream BWIPP generic names whose local
equivalents are more specific (`auspost` → `auspost_customer`,
`pzn` → `pzn7`, `rationalizedCodabar` → `codabar`, plus the eight
upstream composite generics that map to our `_cca`/`_ccb` variants).
Those aliases were added to `Symbology::from_id` during this audit so
callers passing upstream bwip-js ids see them resolve. Coverage is
pinned by `integration::alias_ids_route_to_canonical_symbology`.

All upstream encoders that were previously `missing`
(`gs1dotcode`, the four DataBar stacked/truncated composites,
`code16k`, `code49`, `codeone`, `rectangularmicroqrcode`) have since
been ported across stages B–B' (DotCode FNC1 support → `gs1dotcode`),
stages A–A' (the four DataBar composite variants), the Code 16K /
Code 49 / Code One ports, and Stages 15a–15f (the rMQR ground-up
implementation). The previous compatibility-exception bucket (the
QR Code family routed through the upstream `qrcode` crate) was
emptied in Stage 16 (default-on native QR encoder) and Stage 17c
(`gs1qrcode` graduation). Out of scope (unchanged from the original
audit): Ultracode (colour 2D, doesn't fit our monochrome
`Encoded::*` model), POSICODE (deprecated linear), and three
internal bwip-js dispatch helpers (`raw`, `symbol`, `gs1-cc`).

Encoders ported across the upstream-encoder-completeness passes
(moving from `missing` to `implemented` / `compatibility_exception`):
`ean14`, `mands`, `azteccodecompact`, `gs1datamatrixrectangular`,
`hibcazteccode`, `hibcdatamatrixrectangular`, `aztecrune`,
`channelcode`, `gs1dldatamatrix`, `gs1dlqrcode`,
`datamatrixrectangularextension`. Most are thin wrappers over
previously verified primaries (gs1-128, ean8, aztec, datamatrix,
qrcode). The exceptions are `aztecrune` (11×11 fixed marker —
leveraged the existing `build_matrix("rune", ...)` path), `channelcode`
(real recursive-enumeration port of BWIPP's `nextb`/`nexts`
algorithm, ~200 LOC), and the GS1 DL pair (`gs1dldatamatrix` /
`gs1dlqrcode`) which share a ~150 LOC `util::gs1::parse_dl_uri`
light-validation URI parser. The `hibcazteccode` port additionally
surfaced + fixed an Aztec encoder gap: the Digit-state Punct Shift
codeword (Aztec spec PS = 0) was missing from `sentinel_codeword`,
causing `InvalidData` for any input that crossed Digit→Punct
(e.g. digits immediately followed by `/`). Now covered.

For `datamatrixrectangularextension`, the substrate's preferred-size
policy doesn't always pick the DMRE-only size BWIPP would (e.g. 80×8
for 40-char "AB", where the substrate picks 36×16). Both are
spec-compliant ISO/IEC 16022 symbols — same substrate-spec posture
as plain `datamatrix`. Documented in PORT_STATUS and GOLDEN_COVERAGE.

**Hardening — datamatrix family now defaults to square shape**:
`datamatrix_::encode` and `gs1_2d::symbol_list_from_opts` previously
defaulted to "any shape" via `SymbolList::default()`, which let the
substrate pick a rectangular size where BWIPP picks square (e.g.
"HELLO,WORLD!" → 32×8 instead of 16×16; `(01)04012345123456` →
32×8 instead of 16×16). Both are spec-compliant, but the divergence
surprised users migrating from bwip-js. We now default to
`enforce_square()` — matches BWIPP's `bwipp_datamatrix` size-table
preference (squares iterated before rectangles). Pinned by
`integration::substrate_rows_match_bwip_js_dimensions` (9-row
drift net covering datamatrix-substrate + qrcode-substrate
families).

No upstream encoder is silently missing: every one is in either an
`implemented` / `alias_only` / `compatibility_exception` row (reachable)
or a `missing` / `out_of_scope` row (documented gap).

## 1. Catalog reachability

The legacy reference catalog (`rust/tools/inventory/legacy_catalog.json`,
135 entries — a static fixture preserved from the original pre-Rust
catalog) is the historical baseline. The Rust catalog is broader because
it adds substrate variants that catalog folds into one entry:

| Source                                | Entry count |
|---------------------------------------|-------------|
| `legacy_catalog.json` (reference fixture) | 135     |
| `web/src/lib/catalog.ts` (web UI)     | 135         |
| Rust `Symbology` enum (canonical)     | 152         |
| `PORT_STATUS.md` rows                 | 168         |

`PORT_STATUS.md` documents every catalog row covered by the Rust
`Symbology` enum plus the rows that exist only as `from_id` aliases
of a canonical variant (`qrcode_iso` aliasing `qrcode`, the upstream
generic-name aliases like `ean13composite` → CC-A, etc.). The
delta between the 152 canonical Rust variants and the 168-row
PORT_STATUS table reflects those alias rows being broken out as
their own documented entries so users can search the table by the
exact ID they intend to pass to `from_id`. Every row is reachable
through `Symbology::from_id` either as a canonical id or as a
documented alias, and `scripts/check-doc-counts.sh` (wired into
`scripts/ci-inventory.sh`) fails CI if the table count or
PORT_STATUS header drift apart.

### Aliases added during this audit

Before this pass, six Python catalog ids were listed in PORT_STATUS as
verified but had no `from_id` arm in Rust (`planet12`, `planet14`,
`usps_imb`, `usps_postnet5`, `usps_postnet9`, `usps_postnet11`). They
were "reachable" only conceptually — `Symbology::from_id("planet12")`
returned `None`. This was a real consistency bug.

Fixed in `rust/src/symbology.rs`:

* `planet`, `planet12`, `planet14` → `Symbology::Planet` (the encoder
  validates the digit count internally).
* `postnet`, `usps_postnet5/9/11` → `Symbology::Postnet`.
* `usps_onecode | onecode | imb | usps_imb` → `Symbology::UspsOneCode`.

The new aliases are pinned by an expanded
`alias_ids_route_to_canonical_symbology` test in
`rust/tests/integration.rs`.

### Result

After the fix, **every** PORT_STATUS row resolves through
`Symbology::from_id`. The CLI, the wasm-bindgen `renderSvg`/`renderPng`
exports, and the raw-pointer WASM ABI all share that resolver, so the
full catalog is reachable from all four surfaces (Rust library, CLI,
wasm-bindgen JS, raw WASM ABI).

## 2. Verification-strength classification

The audit re-checked the `verified` claim for every PORT_STATUS row.
Categories used (from AGENT_GOAL.md):

* **logical golden (BWIPP/PostScript)** — module pattern or codewords
  matched against PostScript output.
* **logical golden (bwip-js)** — module pattern, codewords, or
  bar-space run lengths matched byte-for-byte against `raw()` output
  from bwip-js.
* **rendered-output sanity** — pixel/SVG presence + structural
  invariants (height, module count) without an external oracle.
* **wrapper / alias proof** — verified by composition over an already
  verified primary encoder (e.g. Code 128 subset routing, HIBC PAS
  data prefix, EAN add-on combinator).
* **validation tests only** — input validation + check-digit
  arithmetic only.
* **smoke only / scanner-spec compatibility** — render-succeeds plus
  spec-document conformance.

### Per-family summary

| Family                                | Verification strength                         |
|---------------------------------------|-----------------------------------------------|
| Linear standard (Code 39/93/128/11/32)| **bwip-js logical golden** — `raw().sbs` byte-for-byte |
| 2-of-5 family                         | bwip-js logical golden — every version arm covered |
| Retail EAN/UPC                        | bwip-js logical golden + UPC-E native expansion test |
| EAN/UPC add-ons (p2/p5)               | bwip-js logical golden over the combine path |
| ISBN / ISMN / ISSN                    | bwip-js logical golden + hyphen-stripping unit tests |
| GS1-128 / SSCC-18 / UPC Coupon        | bwip-js logical golden (AI parsing + concat) |
| GS1 DataMatrix / NTIN / PPN           | bwip-js logical golden over the wrapper + Data Matrix substrate |
| GS1 DataBar (all 7 method-dispatch)   | bwip-js logical golden — every method arm tested |
| Composite (Linear + 2D, all 17)       | bwip-js logical golden over linkage flag      |
| HIBC LIC + HIBC PAS                   | bwip-js logical golden of the prefixed payload over the underlying verified encoder |
| Postal 4-state (RM4SCC, KIX, AusPost, Mailmark, Japan Post) | per-bar F/D/A classification (bar-state level) against bwip-js |
| PostNet / PLANET / OneCode (IMb)      | per-bar F/D classification against bwip-js — every digit-count alias exercises the underlying encoder via the new alias arms |
| PDF417 / PDF417 Truncated / MicroPDF417 | bwip-js logical golden (codewords) |
| DotCode                               | bwip-js logical golden of pixs + 40 evalsymbol score pairs |
| MaxiCode (modes 2/3/4/5/6)            | bwip-js logical golden of set-A/B/C/D/E shifts + latches (18 oracle tests) |
| Aztec Code                            | bwip-js logical golden — 27-input ASCII/UTF-8/pair-compression corpus |
| Han Xin Code                          | bwip-js logical golden of 6 final-pixs + 24-case mask-score corpus |
| QR / Micro QR / Data Matrix           | substrate-crate (qrcode / datamatrix) — spec compliant by construction; size + decode pinned by tests |
| Swiss QR Code                         | SPC-validated payload + QR substrate (decode-equivalent to BWIPP) |
| Codablock-F                           | bwip-js logical golden (row codewords) |
| Mailmark / Mailmark 2D (types 7/9/29) | C40-encoded data + Data Matrix substrate — bwip-js byte-for-byte for types 7 and 9, structural for type 29 (substrate sizing covered) |
| **GS1 QR Code**                       | bwip-js byte-for-byte via the native `qrcode_native::encode_gs1_qrcode` (FNC1-first-position bit-stream prefix per ISO/IEC 18004 Annex L). The historical qrcode-crate compatibility exception was retired in Stage 17c. |

Every `verified` row has either a byte-for-byte logical golden test
against bwip-js/BWIPP **or** a wrapper proof over a verified primary
encoder. No row remains in the `smoke only` or `validation tests
only` bucket without a corresponding logical proof.

### Suspect rows examined

* `gs1qrcode` — historically a documented compatibility exception
  (the qrcode-crate mask scorer picked a different mask than BWIPP's
  `evalfull`). **Graduated to verified in Stage 17c** via the native
  bwipp-faithful path `qrcode_native::encode_gs1_qrcode` (FNC1-first
  bit-stream prefix per ISO/IEC 18004 Annex L), pinned by
  `gs1_2d::tests::gs1_qrcode_*`. COMPATIBILITY_EXCEPTIONS.md now
  records this row only as graduation history.
* **QR Code family** — historically the qrcode-crate substrate
  diverged from BWIPP for plain QR Code (`qrcode`, `qrcode_iso`,
  `microqrcode`, `rectangularmicroqrcode`, `swissqrcode`,
  `gs1qrcode`, `gs1dlqrcode`, `hibc_lic_qrcode`, `hibc_pas_qrcode`
  — 9 rows). **Stage 16 made the in-crate `qrcode_native` encoder
  the default behind the `prefer-native-qrcode` Cargo feature**;
  the entire family is now byte-for-byte verified against bwip-js
  across 48 oracle-pinned corpus rows (24 Full × L/M/Q/H + 8 Micro
  × valid EC + 16 rMQR × M/H). The upstream `qrcode` crate
  substrate is preserved as an opt-out via `--no-default-features`
  and the substrate-baseline regression test
  (`qrcode_::tests::substrate_baseline_pixs_for_hello`) still pins
  its *current* output as a regression net for substrate-version
  drift.
* `hibc_pas_*` — verified that each PAS wrapper composes a verified
  primary encoder (Code 128, Code 39, Data Matrix, QR Code, PDF417,
  Micro PDF417, Codablock F) over a deterministic prefix. The second
  loop pass added `hibc::tests::encode_pas_code39_matches_bwip_js_raw_sbs`
  (250-byte sbs vs bwip-js) so the PAS Code 39 path is now also
  pinned byte-for-byte, not just via composition.
* `ean8p2`/`ean8p5`/`upcap5`/`upcep5` — second loop pass added four
  inline bwip-js byte-for-byte goldens
  (`ean_combined::tests::ean8_p2_matches_bwip_js_raw_sbs` and
  siblings) plus the oracle script `rust/tools/oracle-eanupc-addons.js`
  for regeneration.
* USPS PostNet 5/9/11 and PLANET 12/14 — the digit-count aliases all
  hit the same `Postnet`/`Planet` encoder, which has per-bar F/D
  golden tests against bwip-js. No new tests required — the alias
  routes themselves are now pinned.
* `usps_imb` — wrapper alias of `usps_onecode`; pinned by
  `alias_ids_route_to_canonical_symbology`.

### Tests inventory

Native test counts (last run, second hardening pass):

* `cargo test`: **771 unit + 12 integration + 55 doctests = 838**
  native tests, all passing (+ 9 from this pass: 4 EAN/UPC add-on
  byte-for-byte goldens, 1 HIBC PAS Code 39 golden, 1 QR substrate
  baseline + 3 GS1 QR FNC1 regression tests).
* `cargo test -p bwipp-wasm` (this audit added a `tests` module to
  the raw-pointer crate): **2** round-trip tests against the unsafe
  ABI, passing on the native target.
* `wasm-pack test --node`: 30 wasm-bindgen-tests for the wasm-pack
  output (doubled this iteration to cover Code 128 / 39, PDF417 /
  MicroPDF417, Data Matrix / Micro QR / DMRE, Aztec Compact / Rune,
  Channel Code, UPC-A, Codablock-F, GS1 DataMatrix / GS1 DL DM,
  USPS IMb) — re-run by `scripts/ci-golden.sh`.

The test-strength matrix above is built on these counts; the
classification was performed by reading `mod tests {}` blocks in
`rust/src/symbology/` plus the integration tests, not on a hand-wavy
"verified" claim.

## 3. Catalog vs UI parity

| Surface                          | Catalog source                       | Count |
|----------------------------------|--------------------------------------|-------|
| Legacy reference catalog (fixture) | `tools/inventory/legacy_catalog.json` | 135 |
| Web (canonical, `web/`)          | `web/src/lib/catalog.ts`             | 146   |
| Rust library / CLI / wasm-bindgen| `Symbology::all`                     | 152 canonical + 272 aliases |
| Raw-pointer WASM ABI (`bwipp_wasm_supported_ids`) | re-exports `Symbology::all` | 152 + aliases |
| Catalog rows (PORT_STATUS)       | `rust/PORT_STATUS.md`                | 168   |

The Vercel-deployed `web/` ships its own curated `catalog.ts` (see
[`web/COVERAGE.md`](../web/COVERAGE.md)) and routes every id to a working
Rust encoder via the `rustCandidatesFor` alias resolver in
`web/src/lib/rust-engine.ts`; the full 169-id catalog remains renderable
through the WASM bundle.

The 7 original Rust-canonical ids that aren't in the Python catalog
(`bc412`, `datamatrixrectangular`, `ean2`, `ean5`, `gs1qrcode`,
`postnet`, `telepennumeric`) are reachable through aliases the web
catalog already declares (`telepen_alpha` → `telepennumeric`,
`planet1[24]` → `planet`, `usps_postnet[59|11]` → `postnet`,
add-on combinations on `ean[138]p[25]`). The web catalog never
exposes those internal ids as separate entries — there is no UI
gap.

The 11 encoder-completeness ports added later (`ean14`, `mands`,
`azteccodecompact`, `gs1datamatrixrectangular`, `hibcazteccode`,
`hibcdatamatrixrectangular`, `aztecrune`, `channelcode`,
`gs1dldatamatrix`, `gs1dlqrcode`, `datamatrixrectangularextension`)
are reachable via the Rust API, CLI, and WASM ABI but are not in the
web catalog's dropdown today — they exist only because they show up
in the upstream BWIPP inventory. A future iteration could add them to
`web/src/lib/catalog.ts` to expose them in the UI; that's tracked in
PORT_COMPLETENESS.md.

## 4. CI architecture

State before audit:

* Top-level `.github/workflows/ci.yml`: manual-only (good), but
  duplicated logic inline (no shared scripts).
* `rust/.github/workflows/ci.yml`: still had **`push` and
  `pull_request` triggers active** — directly violating
  AGENT_GOAL.md.
* No `scripts/` directory at all.

State after audit:

* `scripts/ci-lib.sh` — shared helpers (`section`, `run`, `mexec`).
* `scripts/ci-rust.sh` — fmt / clippy / tests / doctests /
  rustdoc-warnings / release build / wasm32 build / wasm bridge crate
  build / `cargo publish --dry-run`.
* `scripts/ci-web.sh` — raw WASM build, copy into `web/public/wasm/`,
  `npm install` if needed, typecheck, production build.
* `scripts/ci-golden.sh` — full Rust test pass + wasm-pack `--node`
  if installed (warns but does not fail otherwise).
* `scripts/ci-local.sh` — orchestrator that calls the three above.
* `.github/workflows/ci.yml` — manual `workflow_dispatch` only; the
  jobs are thin shims that call the local scripts.
* `rust/.github/workflows/ci.yml` — disabled automatic triggers
  (commented push/pull_request); the only job calls
  `scripts/ci-rust.sh`.

All four CI scripts are executable, use `mise exec --` when mise is
on PATH, and avoid hard-coded absolute paths so a future Linux
runner inside a GitHub Action can reuse the exact same script.

### Strict publish gate + reproducible fuzz bootstrap

`PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh` adds
`scripts/ci-fuzz.sh` — a 30 s libFuzzer smoke run of every fuzz target. It
**soft-skips** on a box without the nightly fuzz toolchain but **hard-fails**
under `PUBLISH_STRICT=1`, so a release can't be cut where the gate can't run.
A green fuzz run proves *no crash was found this run*, not total
panic-freedom. **Bootstrap once** with the idempotent installer (the only
thing that mutates your toolchain — the gate scripts never auto-install):

```sh
./scripts/bootstrap-ci.sh   # MSRV + pinned stable + nightly(rust-src,llvm-tools) + cargo-fuzz/audit/deny + wasm32
```

`ci-web.sh` guards the committed `web/public/wasm/bwipp_wasm.wasm` against
**staleness** in a host-independent way. The wasm binary is *not*
byte-reproducible across OS/arch (even with a pinned rustc — verified: a macOS
`aarch64` build and a Linux `x86_64` build of the same source/rustc differ), so
instead of byte-diffing it, the guard compares a fingerprint of the **source**
that determines the bundle — `scripts/wasm-srcsha.sh` hashes `rust/src`,
`rust/wasm/src`, the manifests, and the wasm crate's lockfile — against the
committed `bwipp_wasm.wasm.srcsha256` sidecar. If encoder source changed without
refreshing the bundle, the guard fails on **any** machine. The bundle is a
prebuilt **per-platform** artifact committed only so the Vercel deploy needs no
Rust toolchain; CI enforces its *currency*, while its *correctness* is verified
by the `wasm-pack` tests in `ci-golden.sh` (built fresh from source). Build or
refresh it on any platform (macOS/Linux/WSL) with `scripts/build-wasm.sh` (or
`npm --prefix web run build:wasm`): it pins the stable toolchain explicitly
(`cargo +<ver>`, resolved portably across rustup/mise), remaps path prefixes,
and strips symbols, then rewrites both the bundle and its `.srcsha256` sidecar —
commit both. To upgrade rustc: bump `WASM_TOOLCHAIN` in `build-wasm.sh` (and
`bootstrap-ci.sh`), rebuild, and commit the refreshed bundle + sidecar together.

## 5. Web app cleanup

The canonical (and only) web app is `web/` — the Vercel workbench:
Rust/WASM is the default renderer, with bwip-js present only as a
labeled "comparison" toggle. (Two earlier demos — `apps/web/` and its
`examples/legacy-web/` archive — were removed for the public release;
`web/` is the single deployable target.)

`web/`'s default rendering engine was switched from `bwip-js` to
`rust-wasm` so Rust/WASM is now the headline path; bwip-js is
present only as a labeled "comparison" toggle. The UI label
`135 JS modes` was replaced with a count that reflects whichever
engine is active. Build output: `web/` typechecks and builds via
`scripts/ci-web.sh`.

## 6. Unsafe boundary (`rust/wasm/`)

The bridge crate uses raw-pointer FFI so the web app can load the
`.wasm` file without wasm-bindgen glue. Before this pass, the three
`unsafe fn` declarations had no SAFETY comments and no tests
exercising the unsafe path.

After this pass:

* Module-level doc comment lays out the trust contract between the
  WASM module and the JS host.
* Each `unsafe fn` carries a `# Safety` section documenting which
  invariants the caller must uphold.
* Each `unsafe { … }` body has an inline `SAFETY:` comment pointing
  back to the fn-level contract.
* `#[cfg(test)]` round-trip tests exercise the alloc → fill →
  encode_svg → length-prefix-decode → dealloc cycle natively,
  including the unknown-symbology error branch.
* `#![deny(missing_docs)]` is on; every public item has a doc.

The main crate (`rust/src/`) keeps `#![forbid(unsafe_code)]`.

## 7. Documentation consistency

Documents touched and made mutually consistent:

* `rust/README.md` — dropped the GitHub Actions CI badge (hosted CI is
  intentionally disabled); rewrote the demo link to point at `web/`;
  added the "CI is local-first" section
  describing `scripts/ci-local.sh` and how to re-enable hosted CI.
* `rust/ROADMAP.md` — kept the GS1 QR Code substrate caveat but
  rewrote it as a forward-looking note (the partial entry is now an
  explicit compatibility exception rather than open work).
* `rust/CHANGELOG.md` — corrected the "runs on every push / PR"
  claim (CI is workflow_dispatch-only).
* `rust/PORT_STATUS.md` — the table currently carries 169 rows
  (`scripts/check-doc-counts.sh` pins this against the header). 165
  rows resolve to status `verified`; 3 are `partial` (`code16k`,
  `code49`, `codeone`) with the precise BWIPP-supported extension
  paths documented inline. The header / status legend are kept in
  sync by the doc-count CI check.
* `rust/COMPATIBILITY_EXCEPTIONS.md` — the file is preserved as a
  written record of past exceptions and a schema for documenting
  any future regression, but the live "Current exceptions" section
  is empty (Stages 16 / 17c emptied the bucket by graduating the
  QR family and `gs1qrcode` respectively).
* `rust/AUDIT.md` — this file.

## 8. Definition-of-done checklist

| Acceptance criterion (AGENT_GOAL.md) | Status |
|---|---|
| 0 missing, 0 partial unless justified | **met** — `scripts/ci-inventory.sh` reports 0 missing / 0 partial / 0 compatibility_exception against bwip-js's `bwipp_symlist`. **All 168 PORT_STATUS rows are verified** byte-for-byte against bwip-js for every default-options BWIPP path. The historical 8-row QR-family compat-exception bucket was emptied across Stages 16 (native QR encoder became default) and 17c (`gs1qrcode` graduation). POSICODE was promoted in Stage 22d. Code 16K in Stages 3a/3b/3c. Code One in Stage 3d. Code 49 in Stage 3e. |
| Every catalog entry reachable from Rust / CLI / WASM / web | **met** — `from_id` covers every PORT_STATUS row after the alias additions |
| Every verified entry has meaningful logical golden / wrapper proof | **met** — see §2 |
| Logical (not pixel-only) golden tests | **met** — bwip-js `raw().sbs` byte-for-byte, per-bar F/D, codewords, mask scores |
| Local Mac CI passes via `scripts/ci-local.sh` | **see §9** |
| GitHub Actions automatic triggers disabled, manual wrapper calls local scripts | **met** — both workflow files |
| README / ROADMAP / CHANGELOG / PORT_STATUS / web text agree | **met** |
| `cargo publish --dry-run` passes | **met** (run by `scripts/ci-rust.sh`) |
| No placeholder impls, smoke-only verified claims, broken examples | **met** |
| Rust/WASM is the primary web renderer | **met** — `web/`'s default engine is `rust-wasm` |

## 9. Open risks

* **QR Code family mask divergence**: explicit compatibility
  exception (covers `qrcode`, `qrcode_iso`, `microqrcode`,
  `swissqrcode`, `gs1qrcode`, `gs1dlqrcode`, `hibc_lic_qrcode`,
  `hibc_pas_qrcode`). See `COMPATIBILITY_EXCEPTIONS.md` §1.
  Decoder output is unchanged.
* **Data Matrix substrate size divergence on arbitrary input**: the
  `datamatrix` crate can pick a different *size* than BWIPP for
  long-ish text (`"HELLO,WORLD!"` → Rust 32×8 vs bwip-js 16×16).
  Catalog inputs don't trigger this; pinned by
  `datamatrix_::tests::substrate_baseline_for_hello_world` so a
  future substrate-version bump that changes the policy is caught.
  Both shapes are spec-compliant and decode to the input payload.
  Additionally, `scripts/ci-inventory.sh`'s substrate-version
  sentinel (driven by `rust/tools/inventory/substrate_versions.json`)
  fails if `Cargo.lock` pins a different `qrcode` or `datamatrix`
  crate version than the one whose output was last verified against
  bwip-js. So a substrate bump can't land silently — it forces the
  human to update the sentinel after re-running the drift net.
* **DotCode RS interleaving for `nw > 112`**: the BWIPP encoder
  interleaves the codeword stream into multiple RS passes for very
  long messages (more than 112 codewords). Today
  `dotcode::apply_rs_ecc` uses `step=1` (single-pass RS) for every
  size. No catalog input reaches `nw > 112`, so the limitation is
  latent rather than reachable from `Symbology::default_data` or
  any verified golden. `dotcode::encode` now returns
  `Error::InvalidData` with a descriptive nw / nc message for inputs
  that would require the interleaved path, pinned by
  `dotcode::tests::high_level_encode_rejects_long_payload_requiring_interleaved_rs`,
  so exotic user inputs surface a loud error rather than a corrupt
  symbol. Implementing the BWIPP interleave is non-blocking
  enhancement work.
* **DotCode BIN escape for bytes > 127**: BWIPP's `dotcode` encodes
  non-ASCII bytes via a base259→103 BIN escape sequence. Today the
  Rust port handles modes A/B/C and rejects any byte that none of
  them can encode (typically a byte > 127) with
  `Error::InvalidData` — see `dotcode::mod::encode_message` at the
  `prev_i == i && prev_mode == mode` no-progress check. Catalog
  default data is ASCII, so the verified row stays honest; user
  inputs with UTF-8 or arbitrary binary content will surface a loud
  error rather than a corrupt symbol. Implementing the BIN escape
  is non-blocking enhancement work.
* **wasm-pack tests are conditional**: `scripts/ci-golden.sh` warns
  but does not fail if `wasm-pack` is missing. The README + this
  audit document the install command; CI explicitly installs it.
* **`Cargo.lock` updates from `cargo publish --dry-run`**: the
  dry-run uses `--allow-dirty` so an in-progress feature branch
  doesn't fail the check. The release process re-runs this with a
  clean tree.
* **GS1 Composite methods 10 and 11**: BWIPP's `gs1_cc` chooses
  between 11 numbered encoding methods based on the leading AI shape.
  Methods 1-9 cover all typical GS1 composite inputs and are ported.
  Method 10 with leading AI `11`/`17`+YYMMDD date payload, and
  Method 11 with leading AI `90` (alphanumeric encodings), are not
  yet ported — `gs1_cc::build_gpf_with_method` returns
  `Error::InvalidData` with a clear message for these inputs. No
  catalog default example exercises methods 10/11, so the
  composite-family rows remain verified for their default data; users
  passing in date-prefixed or AI-90-prefixed GS1 composite strings
  will see a loud error rather than a corrupt symbol. Enhancement
  work for a follow-up.
* **Five remaining upstream encoders (scoped future work)**: the
  bwip-js inventory diff lists 5 upstream encoders still under
  `missing` after this session's batch of composite ports. Each is a
  substantial standalone implementation with its own architectural
  considerations; spinning each up as a dedicated session is the
  intended next step.
    - **`gs1dotcode`** (~400 LOC) — GS1 DotCode. Requires the dotcode
      encoder's mode-selection state machine (`enc_c_step` /
      `enc_a_step` / `enc_b_step`) to handle inline FN1 markers
      between AIs and emit the dotcode codeword 107 (FN1) at the
      right boundaries. Today our dotcode encoder is the
      "simplified" subset that does not yet handle FN1 mid-message
      (it emits FN1 only at segstart for digit sequences) or the
      BIN escape for bytes > 127. Both prerequisites must land
      before gs1dotcode can be implemented.
    - **`code16k`** (~600 LOC) — Stacked Code 128 family. Shares
      the GF(929) Reed-Solomon backend with PDF417
      (`util::rs_gf929`) and the Code 128 subset-state machine
      (`code128`) but requires its own row-codeword layout and
      stacked-render path. ISO/IEC 12323.
    - **`code49`** (~600 LOC) — Stacked alphanumeric. Mostly
      bespoke encoder; no shared infrastructure beyond
      `LinearPattern` / `BitMatrix`. ANSI MH10.8.5.
    - **`codeone`** (~1500 LOC) — Code One matrix 2D. Eight
      different symbol versions (A..H), each with its own
      module-placement grid and Reed-Solomon parameters. AIM
      USS Code One. Rarely used; superseded by Data Matrix.
    - **`rectangularmicroqrcode`** (rMQR, ISO/IEC 23941:2022) —
      Substrate-blocked: the `qrcode` crate doesn't currently
      expose rMQR. Unblock paths: (a) upstream a PR to the
      `qrcode` crate adding rMQR support; (b) hand-roll an rMQR
      encoder in `bwipp-rs` (~800 LOC); (c) document as a
      compatibility exception. Pick (a) first.
  Until each is landed (or downgraded to compatibility exception),
  the inventory diff continues to list them under `missing` and
  `scripts/ci-inventory.sh` prints the rationale on every run.
