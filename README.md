# bwipp-rs

> **bwipp-rs is an independent pure-Rust port of [Barcode Writer in Pure PostScript](https://github.com/bwipp/postscriptbarcode) (BWIPP).**

[![Crates.io](https://img.shields.io/crates/v/bwipp-rs.svg)](https://crates.io/crates/bwipp-rs)
[![Documentation](https://docs.rs/bwipp-rs/badge.svg)](https://docs.rs/bwipp-rs)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-brightgreen.svg)](https://www.rust-lang.org)
[![Unsafe Forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![Live demo](https://img.shields.io/badge/live%20demo-bwipp--rs.rastoder.lu-2ea44f.svg)](https://bwipp-rs.rastoder.lu)

**Try it in your browser → [bwipp-rs.rastoder.lu](https://bwipp-rs.rastoder.lu)** —
generate any of the 169 symbologies live (Rust/WASM by default, with bwip-js as
a side-by-side comparison engine).

bwipp-rs is a pure-Rust reimplementation of BWIPP's barcode encoders —
hand-written from scratch for every symbology except Data Matrix (which
builds on the `datamatrix` crate) and QR (native by default, with the
`qrcode` crate as an optional substrate). The goal is **every
user-facing symbology BWIPP supports** — every monochrome encoder plus the one colour symbology
(`ultracode`) — written in safe Rust (`#![forbid(unsafe_code)]`) with
no Ghostscript, no Node.js, and no external runtime — a single
library + CLI + WASM bundle that runs anywhere Rust runs.

## Crate

The published crate lives in [`rust/`](rust/). See
[`rust/README.md`](rust/README.md) for the full crate-level README
(install, examples, status, verification strength).

```toml
[dependencies]
bwipp-rs = "0.1"
```

```rust
use bwipp::{render_svg, Options, Symbology};

let svg = render_svg(Symbology::QrCode, "Hello", &Options::default())?;
assert!(svg.starts_with("<svg"));
```

## Status snapshot

| Category | Catalog entries | Verified | Partial | Compatibility exception | Missing |
|---|---|---|---|---|---|
| **Total** | **169** | **169** | **0** | **0** | **0** |

The full catalog is reachable through `Symbology::from_id`. **All
169 rows are byte-for-byte verified** against bwip-js / BWIPP for
known test vectors, or composition-pinned as a thin wrapper over a
verified primary encoder. There are no partial rows. See
[`rust/PORT_STATUS.md`](rust/PORT_STATUS.md) for the per-row
verification details.

The QR Code family routes through an in-crate native bwipp-faithful
encoder (`src/symbology/qrcode_native/`) verified on a 48-row
corpus.

See [`rust/PORT_STATUS.md`](rust/PORT_STATUS.md) for the per-symbology
breakdown and [`rust/AUDIT.md`](rust/AUDIT.md) for the
verification-strength matrix.

### Honestly: what's out of scope

Every user-facing BWIPP encoder — including the one colour symbology
`ultracode` — is shipped and verified. The only BWIPP catalog names
NOT exposed as standalone encoders are internal helpers:

| Upstream id | Status                                                                  |
|-------------|-------------------------------------------------------------------------|
| `raw`       | Internal bwip-js dispatch helper, not a standalone encoder.             |
| `symbol`    | Internal bwip-js generic-symbol renderer, not a standalone encoder.     |
| `gs1-cc`    | Internal composite-component glue used by the various `*composite` rows.|

`ultracode` — the catalog's single colour 2D symbology — **is**
implemented and byte-for-byte verified against bwip-js: it routes
through the `Encoded::ColorMatrix` carrier and the colour SVG/PNG
renderers paint each cell from the 6-colour `ULTRACODE_PALETTE`. See
[`rust/PORT_STATUS.md`](rust/PORT_STATUS.md) for the per-row details.

## Repository layout

```
.
├── rust/          ← the bwipp-rs crate (this is the public crate)
│   ├── src/       ← encoders, renderer, public API
│   ├── tests/     ← integration + golden fixtures
│   ├── tools/     ← inventory & oracle-extraction scripts
│   ├── PORT_STATUS.md, AUDIT.md, ROADMAP.md, GOLDEN_COVERAGE.md,
│   │   COMPATIBILITY_EXCEPTIONS.md, PORT_COMPLETENESS.md
│   └── README.md
├── web/           ← Vercel barcode workbench (Next.js + WASM)
├── scripts/       ← CI scripts (ci-local, ci-inventory, ci-rust,
│                   ci-golden, ci-web, check-doc-counts)
├── node-sidecar/  ← oracle harness (bwip-js) used to generate
│                   golden fixtures during development; not at
│                   runtime.
└── README.md      ← this file
```

The bwip-js oracle harness (`node-sidecar/`) is development-only — it is
not loaded at runtime by the published `bwipp-rs` crate, which has zero
non-Rust runtime dependencies. It regenerates the golden fixtures during
encoder ports; the byte-for-byte BWIPP / bwip-js corpus comparison is what
this crate's "verified" status is grounded in.

## CI

```sh
./scripts/ci-local.sh                                   # standard gate
./scripts/bootstrap-ci.sh                               # one-time: install opt-in toolchains/tools
PUBLISH_STRICT=1 mise exec -- ./scripts/ci-local.sh     # strict (publish) gate
```

Runs the full local gate end-to-end: `cargo fmt`, `clippy`,
unit / integration / doctest, `cargo doc` (no warnings),
release build, wasm32 build, raw-pointer wasm build, `cargo publish
--dry-run`, golden-fixture verification, web typecheck + production
build, and the inventory + doc-count consistency checks.

The **strict** gate (`PUBLISH_STRICT=1`) additionally runs the
cargo-fuzz smoke gate and the security gates. Run
[`scripts/bootstrap-ci.sh`](scripts/bootstrap-ci.sh) **once** first — it
idempotently installs the MSRV + pinned-stable + nightly toolchains, the
`rust-src` / `llvm-tools-preview` components, `cargo-fuzz` /
`cargo-audit` / `cargo-deny`, and the `wasm32` target. The gate scripts
never auto-mutate your toolchain; bootstrap is the only step that does.

Hosted GitHub Actions are intentionally `workflow_dispatch`-only
during development; the same checks run inside `ci-local.sh`.

## License

bwipp-rs is dual-licensed under [MIT](rust/LICENSE-MIT) OR
[Apache-2.0](rust/LICENSE-APACHE), at your option. BWIPP itself is
MIT-licensed; this crate is an independent re-implementation, not a
PostScript-to-Rust translation, and carries no upstream code.
