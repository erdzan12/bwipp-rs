# Changelog

All notable changes to bwipp-rs are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [SemVer](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-05-30

### Fixed
- Constrained the `image` dependency to `>=0.25, <0.25.10`. image 0.25.10
  raised its MSRV to rust 1.88, which broke building bwipp-rs on its declared
  MSRV of rust 1.85; pinning to 0.25.9 keeps the crate buildable on 1.85 as
  documented.

## [0.1.0] - 2026-05-30

Initial public release.

bwipp-rs is an independent, pure-Rust port of BWIPP (Barcode Writer in Pure
PostScript). It renders barcodes to SVG and PNG with no Ghostscript, Node.js,
or other external runtime — a single library + CLI + WebAssembly bundle.

### Added
- 169 user-facing symbology IDs reachable through `Symbology::from_id`,
  spanning linear, retail / EAN / UPC, GS1, postal 4-state, 2D matrix,
  stacked, healthcare (HIBC), and colour (Ultracode) families. See
  [`PORT_STATUS.md`](PORT_STATUS.md) for the per-symbology verification table.
- SVG and PNG renderers, a `bwipp` CLI, and a `wasm32-unknown-unknown`
  binding exposing `renderSvg` / `renderPng` / `listSymbologies`.
- `#![forbid(unsafe_code)]` in the core library.

[0.1.1]: https://github.com/erdzan12/bwipp-rs/releases/tag/v0.1.1
[0.1.0]: https://github.com/erdzan12/bwipp-rs/releases/tag/v0.1.0
