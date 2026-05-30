# bwipp-rs

> **bwipp-rs is an independent pure-Rust port of [Barcode Writer in Pure PostScript](https://github.com/bwipp/postscriptbarcode) (BWIPP).**

[![Crates.io](https://img.shields.io/crates/v/bwipp-rs.svg)](https://crates.io/crates/bwipp-rs)
[![Documentation](https://docs.rs/bwipp-rs/badge.svg)](https://docs.rs/bwipp-rs)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-brightgreen.svg)](https://www.rust-lang.org)
[![Unsafe Forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

> **CI is local-first.** Hosted GitHub Actions are intentionally
> disabled during development (the workflow exists but is
> `workflow_dispatch`-only). Run `scripts/ci-local.sh` from the repo
> root for the full gate — fmt + clippy + tests + doctests + rustdoc
> warnings + release build + wasm32 build + raw-pointer wasm build
> + `cargo publish --dry-run` + web typecheck + production build.
> See [`AUDIT.md`](AUDIT.md) for the full CI architecture.

bwipp-rs is a Rust reimplementation of BWIPP's barcode encoders —
hand-written from scratch for every symbology except Data Matrix (which
builds on the `datamatrix` crate) and QR (native by default, `qrcode`
crate as an optional substrate). The current scope is **every
user-facing BWIPP catalog encoder, including the colour symbology
`ultracode`** — written in
safe Rust (`#![forbid(unsafe_code)]`), no Ghostscript, no Node.js, no
external runtime — a single library + CLI that runs anywhere Rust
runs, including WebAssembly. (`ultracode` — the catalog's single
colour 2D barcode — is implemented and byte-for-byte verified; it
routes through the `Encoded::ColorMatrix` carrier and the colour
SVG/PNG renderers paint each cell from the 6-colour palette.)

## Status

| Category | Catalog entries | Verified | Partial | Compatibility exception | Missing |
|---|---|---|---|---|---|
| **Total** | **169** | **169** | **0** | **0** | **0** |

Every row in the catalog is reachable end-to-end through
`Symbology::from_id`. **All 169 rows are verified** — each has
either (a) byte-for-byte verification against bwip-js / BWIPP for
known test vectors, or (b) a composition-pinned wrapper test over a
verified primary encoder. A few encoders (`code16k`, `code49`,
`codeone`) are verified for their documented payload subset and
return `Error::InvalidData` on the not-yet-supported BWIPP
extensions (`code16k` mid-message A↔B / FNC4 shifts; `code49` Symbol
Append Mode chaining; `codeone` S-/T-strip / FNC / ECI) rather than
silently producing a wrong symbol.

The QR Code family (`qrcode`, `qrcode_iso`, `microqrcode`,
`rectangularmicroqrcode`, `swissqrcode`, `gs1qrcode`, `gs1dlqrcode`,
`hibc_lic_qrcode`, `hibc_pas_qrcode`) is routed through the native
bwipp-faithful encoder in `src/symbology/qrcode_native/` — verified
byte-for-byte against bwip-js on **48 oracle-pinned corpus rows**
(24 Full V1-V40 × L/M/Q/H samples + 8 Micro M1-M4 × valid EC levels
+ 16 rMQR R7×_..R17×_ × M/H). The upstream `qrcode` crate substrate
is preserved as an opt-out via `cargo build --no-default-features`.

See [`PORT_STATUS.md`](PORT_STATUS.md) for the per-symbology
breakdown and [`AUDIT.md`](AUDIT.md) for the verification-strength
matrix.

The catalog above covers the project's 169 user-facing IDs. The
**upstream BWIPP / bwip-js comparison** — every encoder upstream
`bwipp_symlist` enumerates, classified as `implemented` /
`alias_only` / `compatibility_exception` / `partial` / `missing` /
`out_of_scope` — lives in [`PORT_COMPLETENESS.md`](PORT_COMPLETENESS.md).
Of upstream bwip-js's 110 encoders: **89 implemented** (including the
colour symbology `ultracode`), **11 alias_only** (upstream generic
names that route to our more-specific Rust variants — e.g. `pzn` →
`pzn7`/`pzn8`, `auspost` → the four specific Australia Post variants,
the various composite umbrellas), **0 compatibility_exception**,
**0 partial**, **0 missing**. **3 are intentionally out of scope**:
`raw` / `symbol` / `gs1-cc` are internal bwip-js dispatch helpers,
not standalone encoders. The project therefore covers every
user-facing BWIPP catalog encoder — see "Out-of-scope encoders"
below for the internal-helper rationale.

`scripts/ci-inventory.sh` enforces that no upstream encoder is
silently unclassified, and `scripts/check-doc-counts.sh` enforces
that the catalog header numbers above stay consistent with the
PORT_STATUS table.

* **verified** — output matches BWIPP / bwip-js for known test
  vectors, *or* is a thin wrapper / alias whose composition over a
  verified primary encoder is pinned by a test. Includes:
  - All 17 GS1 Composite Code variants (CC-A and CC-B over DataBar
    Omni, DataBar Limited, DataBar Expanded, GS1-128, EAN-13, EAN-8,
    UPC-A, UPC-E, plus GS1-128 CC-C).
  - Han Xin Code — full BWIPP-faithful port (binary mode + 13-stride
    interleave + GF(256) data RS + GF(16) function-info RS + corner
    finders + alignment + function-info zone + 4 mask functions +
    `evalfull` auto-mask scoring). Verified against 6 byte-for-byte
    `pixs` oracles + 24-case mask-score corpus.
  - Aztec Code — DP mode-switching encoder including Byte mode for
    UTF-8 multibyte input + pair pre-compression for CR/LF and
    punctuation pairs. 27-input bwip-js corpus all byte-matches.
  - MaxiCode (modes 2/3/4/5/6, set-A/B/C/D/E with full shift+latch
    dispatcher AND intra-latch SC/SD/SE shifts AND set-E EOM
    back-latch omission — 18 byte-for-byte oracle tests).
  - Royal Mail Mailmark + Mailmark 2D (types 7/9/29 all working;
    C40 mode in the `datamatrix` crate handles the structured
    portion).
## Out-of-scope encoders (the honest list)

The following three upstream `bwipp_symlist` entries are NOT
exposed as standalone encoders in this crate, by design — each is an
internal bwip-js helper rather than a user-facing symbology:

| Upstream id | Why not a standalone encoder                                         |
|-------------|----------------------------------------------------------------------|
| `raw`       | Internal bwip-js dispatch helper, not a standalone encoder.          |
| `symbol`    | Internal bwip-js generic-symbol renderer, not a standalone encoder.  |
| `gs1-cc`    | Internal composite-component glue used by the various `*composite` rows. Not exposed as its own catalog entry. |

`ultracode` — the catalog's single colour 2D symbology — is **not**
in this list: it is implemented and byte-for-byte verified, routed
through the `Encoded::ColorMatrix` carrier with the 6-colour
`ULTRACODE_PALETTE` painted by the SVG/PNG renderers. See
[`PORT_STATUS.md`](PORT_STATUS.md).

`posicode` was previously listed here as out-of-scope and then
partial; as of Stage 22d it is verified — all four versions
(`a`, `b`, `limiteda`, `limitedb`) are byte-for-byte verified
against bwip-js with 22 sbs goldens covering the auto-encoder
state machine + FN4 ASCII↔extended-ASCII transitions. See
[`PORT_STATUS.md`](PORT_STATUS.md) and
[`GOLDEN_COVERAGE.md`](GOLDEN_COVERAGE.md) for the precise scope.

This crate therefore covers every user-facing BWIPP catalog
encoder, monochrome and colour alike.

## Why bwipp-rs

BWIPP is the gold-standard open-source barcode generator: it supports
~135 symbologies with strict spec compliance. The reference
implementation runs on PostScript (via Ghostscript) and there's a fine
JavaScript port (bwip-js). What didn't exist before was a **pure Rust
implementation** suitable for embedding in Rust applications, compiling
to WebAssembly for the browser, or shipping as a standalone CLI without
runtime dependencies.

bwipp-rs fills that gap. It also reuses two excellent pure-Rust crates
where they already provide spec-compliant output:

- [`qrcode`](https://crates.io/crates/qrcode) — kept as an opt-out
  substrate (`--no-default-features`); the default QR path is the
  in-crate `qrcode_native` encoder which is byte-for-byte
  BWIPP-faithful on a 48-row corpus.
- [`datamatrix`](https://crates.io/crates/datamatrix) — Data Matrix
  ECC 200 encoder.

Everything else (Code 128, the GS1 family, the postal codes, the HIBC
wrappers, the native QR/Micro/rMQR encoder, MaxiCode, Han Xin Code,
DotCode, etc.) is implemented directly in this crate.

## Implemented symbology families

**Linear codes**
* Code 39, Code 39 Full ASCII, Code 93, Code 128 (auto subset A/B/C),
  Code 11, Code 32 (Italian Pharmacode)
* All 2-of-5 variants (Standard, Data Logic, IATA, Industrial, Matrix, COOP, Interleaved, ITF-14)
* MSI (Plessey), Plessey, Telepen + Telepen Numeric
* Pharmacode One-Track, Pharmacode Two-Track, PZN7, PZN8, Flattermarken
* VIN, LOGMARS, M&S 7-digit
* Codabar
* Channel Code (USPS Tray Labels, channels 3..8), BC412

**Retail / EAN / UPC family**
* EAN-13, EAN-8, UPC-A, UPC-E with native non-expansion rendering
* EAN-2 / EAN-5 supplemental add-ons
* `ean13p2`, `ean13p5`, `ean8p2`, `ean8p5`, `upcap2`, `upcap5`, `upcep2`, `upcep5` combined symbols
* ISBN-13, ISMN, ISSN with hyphen-stripping; ISBN-13 + 5-digit add-on; ISSN + 2-digit add-on

**GS1 family**
* GS1-128 (UCC-128 alias), SSCC-18 (NVE-18 alias), UPC Coupon (AI 8110)
* GS1 DataMatrix, GS1 DataMatrix Rectangular, NTIN (AI 8003),
  PPN (ANSI MH10 envelope)
* GS1 QR Code — emits the formal FNC1-first-position mode indicator
  per ISO/IEC 18004 Annex L with spec-compliant segment optimisation
* GS1 Digital Link (`gs1dldatamatrix`, `gs1dlqrcode`) — URI-shape
  validation + delegate to the verified Data Matrix / QR encoder
* EAN-14 / GTIN-14 (Code 128 over `(01)…`)
* USPS Intelligent Mail Package Barcode (IMpb)

**Postal**
* USPS PostNet / PLANET / USPS Intelligent Mail (OneCode, RS-GF(64))
* Royal Mail RM4SCC, KIX (Dutch), DAFT
* Royal Mail Mailmark (all three types: 7 → 24×24, 9 → 32×32, 29 → 16×48)
  + Mailmark 2D (length-derived sizing)
* Japan Post 4-state (with letter expansion + mod-19 check)
* DP Identcode, DP Leitcode, DP Postmatrix
* Australia Post (FCC 11/45/59/62) with full RS-GF(64) check
* UPU S10, Korean Postal, CEPNet, Swedish Postal, Italian Postal 25/39, DPD

**Healthcare (HIBC)**
* HIBC LIC: Code 128, Code 39, Data Matrix, Data Matrix Rectangular,
  QR Code, PDF417, MicroPDF417, Codablock-F, Aztec Code
* HIBC PAS: Code 128, Code 39, Data Matrix, QR Code, PDF417,
  MicroPDF417, Codablock-F

**2D matrix**
* QR Code, Micro QR Code, Data Matrix (ECC200 — all square +
  rectangular sizes), Data Matrix Rectangular Extension (DMRE,
  ISO/IEC 21471)
* Swiss QR Code (SPC-validated, ECL=M)
* DotCode (sparse circular-dot grid with full BWIPP mask selection
  + lit-mask fallback)
* Aztec Code (compact L1-L4 + full L1-L32, reference grid for L≥5,
  byte-mode for UTF-8 multibyte, pair pre-compression for CR/LF and
  punctuation pairs — 27-input bwip-js corpus all matches),
  Aztec Compact (force-compact alias), Aztec Rune (11×11 marker)
* MaxiCode modes 2/3/4/5/6 (hex grid via `Encoded::Hex`) with full
  set-A/B/C/D/E shift + latch dispatcher
* Han Xin Code — Chinese 2D barcode (GB/T 21049-2007), binary mode +
  84 size versions + auto-mask selection via `evalfull` scoring

**Stacked / 2D variable**
* PDF417 (full + Truncated), MicroPDF417
* Codablock-F (the variable-row stacked Code 128 family)
* GS1 DataBar Omni / Truncated / Limited / Stacked / Stacked Omni /
  Expanded / Expanded Stacked (all 7 BWIPP method-dispatch paths
  for Expanded are byte-for-byte oracle-verified)

**WebAssembly bindings** — `--features wasm` builds a ~430 KB `.wasm`
(the wasm crate sets `opt-level = "z"`, so the cargo release output is
already size-optimised; `wasm-opt -Oz` trims it further into the
~400 KB range) with `renderSvg` / `renderPng` / `listSymbologies`
exports for browser and Node consumers.

**Catalog status** — see [`ROADMAP.md`](ROADMAP.md) for the
detailed breakdown. **All 169 rows are verified** byte-for-byte
against bwip-js for every default-options BWIPP path. There are no
partial rows, no compatibility exceptions, and no missing rows. The 8-row QR-family
compatibility-exception bucket
that lived here historically was emptied across Stage 16 (the
native bwipp-faithful QR encoder became the default; the upstream
`qrcode` crate substrate is preserved as an opt-out via
`--no-default-features`) and Stage 17c (`gs1qrcode` graduation). 0
compatibility exceptions remain in PORT_STATUS.

## Quick start

### Library

```rust
use bwipp::{render_svg, render_png, Options, Symbology};

let opts = Options::default();

// SVG
let svg: String = render_svg(Symbology::QrCode, "https://example.com", &opts)?;

// PNG bytes — typed Options field for renderer-level switches, .with() for
// encoder-specific options (includecheck, eclevel, shape, …).
let mut opts = Options::default();
opts.include_text = true;
opts.scale = 3;
let png: Vec<u8> = render_png(
    Symbology::Gs1_128,
    "(01)04012345123456(17)260101",
    &opts,
)?;
std::fs::write("symbol.png", png)?;
```

### CLI

```sh
# Install from crates.io (once published):
$ cargo install bwipp-rs

# Or from a local clone:
$ cargo install --path . --bin bwipp

# Then:
$ bwipp qrcode "Hello, world!" svg /tmp/hello.svg
wrote 2937 bytes to /tmp/hello.svg

$ bwipp ean13 012345678905 png /tmp/ean.png
wrote 2139 bytes to /tmp/ean.png

$ bwipp dotcode "Hello" svg /tmp/dot.svg
wrote 3472 bytes to /tmp/dot.svg
# DotCode renders as circular dots (round geometry, not squares).
```

The CLI signature is `bwipp <symbology> <data> <png|svg> <output_path>`.
Run `bwipp` with no arguments to list every supported catalog id.

### Demo

**Live at [bwipp-rs.rastoder.lu](https://bwipp-rs.rastoder.lu)** — try every
symbology in the browser, no install required.

The canonical Vercel demo lives at [`web/`](../web/). It loads the
raw-pointer WASM bundle (`rust/wasm/`) in the browser and renders
barcodes client-side via Rust/WASM — `renderSvg(id)` accepts any of
the 169 catalog IDs, and the curated picker lists 147 of them (the
rest are variants / niche entries reachable through the same call;
see [`web/COVERAGE.md`](../web/COVERAGE.md), incl. the colour
`ultracode` path). bwip-js is available only as a side-by-side
comparison engine. Rust/WASM is the default renderer.

```sh
$ cd web && npm install && npm run dev
# → http://localhost:3000
```

The same WASM bundle can also be consumed directly via `wasm-pack` —
see the WebAssembly section below.

### WebAssembly (browser / Node)

bwipp-rs builds for `wasm32-unknown-unknown` and exposes a tiny JS-friendly
API via `wasm-bindgen`. Build the `.wasm` directly with:

```sh
$ cargo build --release \
    --target wasm32-unknown-unknown \
    --no-default-features \
    --features wasm
# → target/wasm32-unknown-unknown/release/bwipp.wasm  (~430 KB; the
#    wasm crate already sets opt-level = "z" so cargo's release build is
#    size-optimised. wasm-opt -Oz trims it further — see the wasm-pack
#    flow below.)
```

For a JS-loadable bundle (with the generated `bwipp.js` glue and full
TypeScript declarations), use
[`wasm-pack`](https://rustwasm.github.io/wasm-pack/):

```sh
$ cargo install --locked wasm-pack
$ wasm-pack build --release --target web -- --no-default-features --features wasm
# → pkg/
#     bwipp.js          (13 KB)
#     bwipp.d.ts        (TypeScript declarations)
#     bwipp_bg.wasm     (~430 KB after wasm-opt)
#     bwipp_bg.wasm.d.ts
#     package.json      (npm-publishable)
```

The `pkg/` directory drops straight into a Vite / Webpack / Next.js
project: `import init, { renderSvg } from './pkg/bwipp.js'`.

The exported JS surface is:

```ts
import init, { renderSvg, renderPng, listSymbologies } from "./bwipp.js";

await init();

const svg = renderSvg("qrcode", "https://example.com", { scale: 4, eclevel: "M" });
const pngBytes: Uint8Array = renderPng("ean13", "012345678905", { scale: 4 });
const ids: string[] = listSymbologies();
```

Options accepted via the JS object:
* `scale`, `bar_height` / `barHeight`, `quiet_zone` / `quietZone`,
  `include_text` / `includeText` — typed fields on `Options`.
* any other key/value — flows through to `Options::extras` as a
  string pair, matching BWIPP's option-list convention.

### Options (Rust)

```rust
let opts = Options::default()
    .with("eclevel", "H")           // QR Code error correction level
    .with("version", "32x32")        // Data Matrix size
    .with("includecheck", "true");   // append optional check digit
```

`scale`, `bar_height`, `quiet_zone`, `include_text`, `foreground`, and
`background` are fields on [`Options`]. Per-symbology flags use the
free-form `extras` list (via `.with(k, v)`), matching BWIPP's option-list
convention.

## Architecture

Each symbology produces an `Encoded` value:

```rust
pub enum Encoded {
    Linear(LinearPattern),         // 1D: run-length bar widths
    Matrix(BitMatrix),             // 2D: black/white module grid
    Postal4State(Postal4Pattern),  // 4-state postal (Tracker/Asc/Desc/Full)
    Stacked(StackedPattern),       // stacked 1D (Codablock-F, PDF417, …)
    Dots(DotMatrix),               // DotCode circular dots on diagonal grid
    Hex(Box<MaxiCodeSymbol>),      // MaxiCode hex grid (33×30, offset rows)
    ColorMatrix(ColorMatrix),      // colour 2D grid (ultracode, 6-colour palette)
}
```

The renderers (`render::svg`, `render::png`) consume `Encoded` and never
touch symbology-specific logic. Adding a new symbology means writing one
new file under `src/symbology/` and adding an enum variant — the
renderers come for free.

```
rust/
├── Cargo.toml
├── PORT_STATUS.md
├── src/
│   ├── lib.rs               # public API
│   ├── encoding.rs          # Encoded / LinearPattern / BitMatrix / Postal4Pattern
│   ├── error.rs             # Error type
│   ├── options.rs
│   ├── symbology.rs         # Symbology enum + dispatch
│   ├── symbology/           # one file per symbology family
│   ├── render.rs
│   ├── render/svg.rs        # vector renderer
│   ├── render/png.rs        # raster renderer (via image crate)
│   └── util/
│       ├── gs1.rs           # GS1 Application Identifier parser
│       └── rs_gf113.rs      # Reed-Solomon over GF(113) for DotCode
├── examples/cli.rs
└── tests/integration.rs
```

For the per-symbology verification status, see
[`PORT_STATUS.md`](PORT_STATUS.md). It's the single source of truth
and gets updated every checkpoint.

## Local CI

The full gate runs locally:

```sh
# From the repo root (not rust/):
$ ./scripts/ci-local.sh
```

This shells out to three focused scripts that are also runnable
standalone:

* `scripts/ci-rust.sh` — fmt, clippy `-D warnings`, all-feature
  tests, doctests, rustdoc warnings, release build, wasm32 build,
  raw-pointer wasm crate build, `cargo publish --dry-run`.
* `scripts/ci-golden.sh` — full Rust test suite + wasm-pack
  `--node` tests when `wasm-pack` is installed.
* `scripts/ci-web.sh` — build the `rust/wasm/` raw-pointer bundle,
  copy it into `web/public/wasm/`, then run the Next.js
  typecheck + production build. Also re-asserts the committed
  `web/public/wasm/bwipp_wasm.wasm` matches a fresh build (drift guard).

### Strict publish gate (`PUBLISH_STRICT=1`)

**Prerequisite — bootstrap the opt-in toolchains/tools once** (idempotent;
the only step allowed to mutate your toolchain — the gate scripts never
auto-install):

```sh
$ ./scripts/bootstrap-ci.sh
$ PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh
```

`bootstrap-ci.sh` installs everything the strict gate needs: the MSRV
toolchain (from `Cargo.toml`'s `rust-version`), the pinned stable used for
the reproducible WASM build, `nightly` + `rust-src` + `llvm-tools-preview`,
`cargo-fuzz` / `cargo-audit` / `cargo-deny`, and the `wasm32` target.

`PUBLISH_STRICT=1` additionally runs the **cargo-fuzz smoke gate**
(`scripts/ci-fuzz.sh`): a 30 s libFuzzer run of every fuzz target. Without
the nightly fuzz toolchain the gate **soft-skips** under a normal run but
**hard-fails** under `PUBLISH_STRICT=1` with install instructions (so a
release can't be cut on a box that can't actually run it). A green fuzz run
proves *no crash was found this run*, not total panic-freedom.

GitHub Actions exists as a `workflow_dispatch`-only wrapper around
those same scripts. To re-enable hosted CI later, uncomment the
`push` / `pull_request` triggers in `.github/workflows/ci.yml`. See
[`AUDIT.md`](AUDIT.md) for the full architecture.

## Tests

```sh
$ cargo test
```

**~2000 tests** in total (snapshot — this number grows as coverage is
added): 1894 library unit tests + 38 integration tests (across
`tests/{integration,cli,properties,round_trip_decode,readme_examples}.rs`)
+ 57 doctests, plus 37 wasm-bindgen tests run separately under
wasm-pack (see [WASM tests](#wasm-tests) below). Coverage includes
catalog integrity, per-symbology
encoding logic (with hand-computed check-digit vectors and byte-for-
byte BWIPP/bwip-js oracle corpora for the verified symbologies),
error paths, the GS1 AI parser, the Reed-Solomon GF(64) / GF(113) /
GF(256) / GF(929) infrastructure, the QR Code and Data Matrix
substrate adapters, and the WASM JS API. Every implemented symbology
has at least one smoke test that renders both SVG and PNG via the
integration suite (`tests/integration.rs`).

Selected byte-for-byte oracle highlights:
* DotCode — 10-input pixs corpus + 40 evalsymbol score pairs
* Han Xin Code — 6 final-pixs oracles + 24-case mask-score corpus
* MaxiCode — 18 set-A/B/C/D/E oracles (shifts + latches + intra-latch)
* GS1 Composite — all 17 variants verified against bwip-js
* Aztec Code — 27-input corpus (ASCII / UTF-8 / pair pre-compression)

### WASM tests

The wasm-bindgen API has a separate integration test file
([`tests/wasm.rs`](tests/wasm.rs)) with 37 tests covering
`renderSvg`, `renderPng`, `listSymbologies`, and the error path
under `wasm32-unknown-unknown`. Beyond the basic QR / EAN / DotCode
paths, the WASM suite also covers Han Xin Code, Aztec Code (with
UTF-8 input via Byte mode), MaxiCode (hex grid), Mailmark (with
the `type=29` JsOpts flow), GS1 QR Code (FNC1-first-position), and
the Composite GS1-128 CC-C variant. Run with:

```sh
$ cargo install --locked wasm-pack
$ wasm-pack test --node -- --no-default-features --features wasm
```

All 37 WASM tests pass under Node — the JS surface of the project is
verified working, not just compilable.

## Contributing

To port a new symbology:

1. Find the encoder in `bwip-js/src/bwipp.js` (or the BWIPP `.ps.src`
   sources) and study the patterns / check digit algorithm.
2. Create `rust/src/symbology/<your_symbology>.rs` with an `encode(data,
   opts) -> Result<...>` function. Use existing modules as templates.
3. Add a variant to `Symbology` in `rust/src/symbology.rs` and wire up
   the `encode` dispatch, `id()`, `from_id()`, and `all()` methods.
4. Add an entry to `rust/tests/integration.rs` with a representative
   sample so the catalog round-trip test catches it.
5. Add unit tests in the module — prefer **golden tests** that compare
   exact bit patterns or module strings against BWIPP / bwip-js output
   for known inputs over generic "it renders something" smoke tests.
6. `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings && cargo test`
7. Update `PORT_STATUS.md`.

## License

Dual-licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this crate by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.

### BWIPP attribution

The bar-pattern tables, check-digit algorithms, and encoding rules in
this crate are derived from
[Barcode Writer in Pure PostScript](https://github.com/bwipp/postscriptbarcode)
by Terry Burton et al., which is itself MIT-licensed. bwipp-rs is an
**independent port**, not the official upstream project; please direct
upstream issues to the BWIPP repository.

bwip-js, the JavaScript port of BWIPP, was used as a readable reference
for several symbologies. See https://github.com/metafloor/bwip-js for
the original.
