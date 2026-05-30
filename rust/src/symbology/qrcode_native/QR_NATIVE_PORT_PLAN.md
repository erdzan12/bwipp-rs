# qrcode_native port plan (Path 3 of QR_PATH_D_PLAN)

This file is the detailed sub-plan for the from-scratch QR encoder.
The parent `crate::symbology::QR_PATH_D_PLAN` argued for Path 3
as the cleanest end-state. This file enumerates the stages.

## High-level algorithm

The full QR encoder body in BWIPP is `bwipp_qrcode` at bwip-js
`src/bwipp.js` lines 25521-28528 (3003 LOC of JS). It handles all
three formats — full / micro / rmqr — via internal dispatch on
`$_.format`. Major phases:

1. **Input parsing + segment splitting**. ECI prefix handling.
   Per-segment mode selection (Numeric / Alphanumeric / Byte /
   Kanji / Mixed). Output: a list of `(mode, data)` segments.
2. **Version + EC level selection**. Walk per-format version tables
   (40 versions for Full × 4 EC levels, 4 for Micro × 4 EC, 32 for
   rMQR × 2 EC). Pick smallest version whose data capacity covers
   the segment stream after mode-indicator + char-count-indicator
   prefixes.
3. **Bit-stream serialization**. Pack the segments into a bit stream
   per the version's character-count-indicator widths. Append a
   terminator + byte-align + pad-byte sequence to fill `dcws`.
4. **Reed-Solomon ECC**. GF(256) primitive polynomial 285 (= 0x11D,
   distinct from codeone's 301). Apply per the version's interleaved
   block layout (e.g. Version 7H = 4 blocks × 11 data + 3 blocks ×
   12 data, each with 26 EC codewords).
5. **Matrix placement**. Module layout per the format:
   * Full QR: 3 corner finders + alignment patterns from the
     version's alignment table + 2 timing patterns + dark module +
     format-info + version-info (V7+) + data spiral.
   * Micro: 1 corner finder + timing + format-info + data spiral.
   * rMQR: 1 corner finder + 4 sub-finders + alignment-pattern row
     + format-info bands + data spiral with format-specific orientation.
6. **Mask scoring + selection**. 8 masks for Full, 4 for Micro, 4
   for rMQR. ISO score = N1 + N2 + N3 + N4 penalties. **BWIPP's
   tiebreak rule**: lower mask index wins when two masks tie.
7. **Render**. Return BitMatrix of `rows × cols`.

## Module structure

```
rust/src/symbology/qrcode_native/
    mod.rs                  — public entry + Format enum (Stage 1 ✓)
    QR_NATIVE_PORT_PLAN.md  — this file (Stage 1 ✓)
    metrics.rs              — version tables, alignment-pattern coords,
                              format-info bit patterns (Stage 2)
    segments.rs             — mode detection, char-count widths,
                              bit-stream packing (Stage 3-4)
    rs.rs                   — GF(256) tables (poly 285), interleaved
                              block ECC application (Stage 5)
    placement.rs            — module-grid layout: finders, alignment,
                              timing, data spiral, format/version info
                              (Stage 6)
    mask.rs                 — 8/4/4 mask functions, ISO scoring,
                              BWIPP-tiebreak selection (Stage 7)
```

## Stage breakdown

### Stage 1 ✓ (this commit)

* Module skeleton + `Format` enum.
* Stub `encode()` returning InvalidData.
* This plan document.
* `mod qrcode_native;` declaration in `crate::symbology` (private —
  no `Symbology` variant yet; existing 9 catalog rows untouched).
* Push gate green.

### Stage 2 — Foundation constants

* Full QR version metrics (40 versions × 4 EC): module size, data
  codeword capacity, EC codeword count, block-count + block-size
  table, alignment-pattern coordinate table, char-count-indicator
  widths per (version, mode).
* Micro QR (M1..=M4 × 4 EC).
* rMQR metrics (32 variants × 2 EC).
* GF(256) primitive polynomial 285 (= 0x11D) — distinct from codeone's 301.
* Format-info encoding masks (BCH 15,5 code with mask 0x5412 for QR,
  0x4445 for Micro).
* Version-info encoding (BCH 18,6 for V7+ Full QR).

Estimated 1 commit (table extraction is mechanical).

### Stage 3 — Mode encoders

* Numeric: 3-digit groups → 10 bits each; tails of 2 → 7 bits,
  1 → 4 bits.
* Alphanumeric: pairs of {0-9 A-Z $%*+-./: SPACE} → 11 bits;
  tails of 1 → 6 bits.
* Byte: 8 bits per byte; ISO-8859-1 by default, FNC4 escape for
  non-Latin-1 high bytes (Full QR ECI mode).
* Kanji: shift-JIS encoding → 13 bits per code point. (Deferred?
  Probably feasible to skip until later.)
* ECI prefix: mode indicator 0111 + variable-length ECI assignment.

Estimated 2 commits.

### Stage 4 — Mode selector + segment splitter

* BWIPP's optimization: pick a mode-segment partition that
  minimizes bit count for the chosen version. This is a
  shortest-path problem on the mode-state DAG (one state per
  position × mode).
* Mode-transition cost = mode-indicator bits + char-count-indicator
  bits for the new mode.
* Output: list of `(mode, byte_range)` segments + total bit count.

Estimated 1-2 commits.

### Stage 5 — Reed-Solomon

* Build GF(256) log/antilog tables for poly 285. Reuse codeone's
  `Gf256Tables` shape but with different poly.
* Generator-polynomial coefficient generation per ECC length.
* Interleaved block-ECC application: split the bit-stream-derived
  codeword stream into N blocks per the version's block table,
  compute ECC for each, then de-interleave back into a single
  output stream.

Estimated 1 commit.

### Stage 6 — Matrix placement

* Finder pattern at (0,0), (0,size-7), (size-7,0) for Full QR
  + alignment patterns at coordinates from the version's table.
* Micro: single finder at (0,0). Timing pattern along right + bottom.
* rMQR: corner finders (1 main + 4 sub) and a 1-row alignment band.
* Timing patterns (Full: row 6 + col 6; Micro: row 0 + col 0; rMQR:
  spec-specific).
* Dark module (Full QR only) at fixed position.
* Format-info: 15-bit BCH-coded EC-level + mask-index at format-info
  positions. Position varies per format.
* Version-info (Full QR V7+ only): 18-bit BCH-coded version at
  positions (size-11..=size-9, 0..=5) and mirrored.
* Data placement: snake-traversal through unmasked cells skipping
  function patterns. Different orientation for rMQR.

Estimated 2-3 commits (rMQR placement is its own commit).

### Stage 7 — Masks + scoring

* 8 mask functions for Full QR (`(r+c)%2 == 0`, etc.).
* 4 for Micro (BWIPP-specific subset).
* 4 for rMQR (ISO/IEC 23941 spec).
* ISO score: N1 (run penalty) + N2 (2×2 block penalty) + N3
  (finder-lookalike penalty 1:1:3:1:1) + N4 (dark-module imbalance).
* **BWIPP tiebreak**: when two masks score equally, lower mask index
  wins. Critical for the compat-exception that motivated this whole
  project.
* Auto-mask + manual-mask paths.

Estimated 1-2 commits.

### Stage 8 — Wire `Symbology::QrCodeNative` + first goldens

* Add `Symbology::QrCodeNative` variant (private route — no public
  catalog id yet, gated behind a build feature or kept as a Stage-9
  cutover handle).
* Capture ≥3 bwip-js pixs goldens per format: Full (V1, V7+, V40),
  Micro (M1, M4), rMQR (a small + a tall variant).
* Cover each EC level + boundary version-step.

Estimated 2 commits.

### Stage 9 — Cutover

* Switch `qrcode_::encode` to delegate to `qrcode_native::encode`
  internally. Add a build feature flag to allow opting back into
  the upstream crate during the transition period if regressions
  surface.
* Cutover `swissqrcode`, `gs1qrcode`, `gs1dlqrcode`,
  `hibc_lic_qrcode`, `hibc_pas_qrcode`, `microqrcode` to the native
  path. The wrappers themselves don't change — just the underlying
  encoder behind `qrcode_::encode`.
* Update PORT_STATUS / GOLDEN_COVERAGE: graduate the 8 compat-
  exception rows to `verified`.
* Drop `qrcode = "0.14"` from `Cargo.toml` once goldens for all 8
  rows confirm byte-for-byte BWIPP match.
* Refresh CHANGELOG status snapshot.

Estimated 1-2 commits (cutover + dep removal).

### Stage 10 — rMQR final promotion

* Wire `Symbology::RectangularMicroQrCode` variant.
* Pin ≥3 pixs goldens for rMQR (different heights + widths).
* Update inventory: rectangularmicroqrcode missing → implemented.
* Refresh PORT_STATUS / GOLDEN_COVERAGE / CHANGELOG.
* Add web/src/lib/catalog.ts row.
* Run final 3 scout passes.
* Run STOP CONDITION check — at this point the inventory should be
  missing=0, partial=0, compat-exception=0 (or empty pending bucket).

Estimated 1 commit.

## Risk register

* **Kanji mode**: BWIPP supports it, but it requires Shift-JIS
  tables (~12 KB of mapping data). Risk: scope creep. Mitigation:
  ship without Kanji in Stage 9 cutover, document as a future
  enhancement if no catalog row uses it. Verify via grep across
  the 9 wrapper modules that none currently emit Kanji.
* **Version-info BCH**: Full QR V7+ uses 18-bit BCH(18,6) format-info.
  Risk: bit-order bugs hard to debug. Mitigation: pin Stage 6 with
  per-bit-position goldens for V7, V10, V25, V40 cases.
* **rMQR mask coverage**: 32 variants × 4 masks × scoring = many
  branches to test. Risk: undetected edge cases. Mitigation: in
  Stage 10, pin pixs goldens for at least the 4 corners of the
  variant space (smallest, tallest, widest, default).
* **GF(256) poly conflict**: codeone uses 301; QR uses 285. Two
  different `Gf256Tables` types in the crate. Risk: cross-pollution
  via shared helpers. Mitigation: keep them in module-local
  namespaces (`codeone::Gf256Tables` and `qrcode_native::Gf256Tables`),
  never share the OnceLock cache.

## Next iteration TODO (Stage 2)

1. Add `pub(crate) const FULL_QR_METRICS: [VersionMetric; 40]`.
2. Add `pub(crate) const MICRO_QR_METRICS: [VersionMetric; 4]`.
3. Add `pub(crate) const RMQR_METRICS: [RmqrVariant; 32]`.
4. Add the alignment-pattern coordinate table (per Full-QR version).
5. Add the format-info encoding table (BCH 15,5 for QR, 15,5
   different mask for Micro, 18,6 for V7+ version-info).
6. Anchor each table with unit-tested known values.

Estimated 1 commit. Foundation constants only — no encoder logic.
