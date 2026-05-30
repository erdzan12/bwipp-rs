# QR-family Path D plan — rMQR + BWIPP-mask-tiebreak resolution

This document captures the investigation done in iteration N for the
final outstanding inventory items:

* 1 row in the `missing` bucket: `rectangularmicroqrcode` (rMQR per
  ISO/IEC 23941:2022, the only item A.5 of the master-loop backlog).
* 8 rows in the QR-family compatibility-exception bucket: `qrcode`,
  `qrcode_iso`, `microqrcode`, `swissqrcode`, `gs1qrcode`,
  `gs1dlqrcode`, `hibc_lic_qrcode`, `hibc_pas_qrcode`. All route
  through the upstream [`qrcode` crate](https://crates.io/crates/qrcode)
  whose mask-selection tie-breaks differ from BWIPP.

Per the master-loop STOP CONDITION, *any one* of three paths
resolves the family:

* **Path 1** — upstream PR to `qrcode` adding (a) a
  `mask-tiebreak=bwipp` mode and (b) rMQR support. Compatibility
  exception graduates to "awaiting upstream PR #N" with the PR URL
  serving as the acceptance.
* **Path 2** — vendor a fork under `rust/vendor/qrcode/` with the
  BWIPP tiebreak + rMQR patches. 8 compat-exception rows + 1 missing
  row all promote to verified.
* **Path 3** — write a from-scratch QR encoder under
  `rust/src/symbology/qrcode_native/`. Decommissions the `qrcode`
  crate dependency. All 9 rows promote to verified.

## Investigation findings

### bwip-js / BWIPP source shape

```
function bwipp_qrcode()             { ... 3003 LOC ... }
function bwipp_microqrcode()        { 24 LOC wrapper, sets format="micro" }
function bwipp_rectangularmicroqrcode() { 24 LOC wrapper, sets format="rmqr" }
```

The two micro variants are thin wrappers around the monolithic
`bwipp_qrcode`. Format dispatch happens inside via
`$_.format in {"full", "micro", "rmqr"}` (bwip-js line 26628 +
onwards). This is the same pattern bwipp uses for the rest of the QR
family (`swissqrcode`, `gs1qrcode`, etc. — all build options and call
`bwipp_qrcode`).

Implications:

* **rMQR cannot be a small "delta" encoder** — its layout, alignment
  patterns, mask-evaluation rules, and version-selection table are
  all driven by code paths inside the 3003-LOC `bwipp_qrcode`. A
  rMQR-only port that doesn't ship a full QR encoder would mostly
  duplicate that body.
* **All 9 QR-family rows depend on the same encoder**. Shipping a
  native encoder graduates all 9 rows in one move; vendoring a fork
  with both fixes does the same.

### Existing wrappers in bwipp-rs

* `rust/src/symbology/qrcode_.rs` — 277 LOC. Wraps the `qrcode = "0.14"`
  crate. Handles input parsing, EC-level selection, optional
  version-string parsing for QR/micro forms, and BitMatrix
  conversion. Currently doesn't support rMQR (the upstream crate
  doesn't either).
* `rust/src/symbology/swiss_qr.rs`, `gs1_2d.rs`, `hibc.rs` — wrappers
  that build a payload then delegate to `qrcode_::encode` (or its
  variants).

### Upstream `qrcode` crate state

* Repository: <https://github.com/kennytm/qr-rust>.
* Last published version: 0.14.x (May 2024 era).
* **No rMQR support**. Codebase covers QR (Versions 1-40) and
  Micro QR (M1-M4) only.
* **No BWIPP-tiebreak option**. The crate uses a standard ISO mask
  scoring algorithm; BWIPP's tiebreak (lower mask index when scores
  tie) isn't exposed.
* Maintenance status: occasional commits in 2023-2024 but slow
  cadence. A PR adding rMQR (a 32-variant brand-new shape) would
  likely take months even if accepted.

### Path 1 (upstream PR) — assessment

Pros:

* Lowest long-term cost if upstream accepts.
* Both fixes live in well-maintained crate.

Cons:

* **rMQR is a substantial feature** — 32 distinct symbol versions
  with bespoke alignment pattern tables, function-info encoding, and
  spec-distinct mask-evaluation rules. Not a drop-in 50-line patch.
* Mask-tiebreak option would also need design review (probably an
  enum variant or feature flag).
* Master-loop STOP CONDITION says "PR open" satisfies the path, but
  the catalog rows remain in the compat-exception bucket until the
  PR lands.
* If upstream rejects or stalls, we'd need to re-pivot.

### Path 2 (vendored fork) — assessment

Pros:

* Immediate resolution: we control the fork, no waiting on upstream.
* Both fixes can land in one fork commit each.
* The 9 catalog rows promote to verified once the fork is wired.

Cons:

* **Ongoing maintenance burden**: every upstream release requires
  manual reconciliation.
* Adds `rust/vendor/qrcode/` to the repo footprint (probably 5K-10K
  LOC of vendored code).
* Doesn't help the upstream ecosystem.

### Path 3 (native from-scratch) — assessment

Pros:

* Cleanest end-state: zero external QR dependency.
* Full control over BWIPP-compatibility.
* Reuses infrastructure already built (GF(256) RS tables from
  `codeone::gf256_*`, BitMatrix encoder, lookup-style mode
  selectors).
* Decommissions `qrcode` crate dependency — one fewer pin in
  `substrate_versions.json`.

Cons:

* **~1500-2500 LOC of from-scratch work** (BWIPP source is 3003 LOC
  of dense JS; Rust port is typically ~70% of JS size for this kind
  of code, plus all the constants tables).
* Estimated 12-18 iterations following the same Stage 1-N template
  used for code16k / code49 / codeone.
* High up-front cost; only pays off when the family migrates over.

## Recommendation

**Path 3 (native from-scratch QR encoder)** is the recommended
trajectory. Reasoning:

1. **It actually finishes the job.** Paths 1 and 2 either depend on
   external review (Path 1) or create a permanent maintenance burden
   (Path 2). Path 3 leaves a clean, fully verified, BWIPP-faithful
   end-state with no compat exceptions remaining.

2. **Infrastructure reuse**. The just-shipped codeone port already
   has GF(256) RS tables (poly 301 — different from QR's poly 285
   but the framework is reusable). The lookup-style cost-based
   mode selector pattern from codeone applies directly. The
   BitMatrix + placement plumbing is identical.

3. **All 9 catalog rows promote in one project**. Each iteration
   builds toward removing 9 compat-exceptions + 1 missing row
   simultaneously.

4. **Aligns with the master-loop philosophy**: "never stop until
   done" with "documented compatibility exception with upstream PR"
   as the *only* acceptable alternative to byte-for-byte verified.
   Path 1 just defers the problem.

## Stage breakdown (Path 3)

| Stage | Deliverable | Estimated iterations |
|-------|-------------|----------------------|
| 1     | `rust/src/symbology/qrcode_native/mod.rs` skeleton + PORT_PLAN.md + module wiring (private). | 1 |
| 2     | Foundation: version metrics for QR (40 versions × 4 EC levels), micro (M1-M4 × 4 EC), rMQR (32 variants × 2 EC). Alignment-pattern tables, format-info bit patterns. | 1-2 |
| 3     | Mode encoders: Numeric, Alphanumeric, Byte, Kanji (deferred?), ECI prefix. Mirrors BWIPP's `numeric`, `alphanumeric`, `eightbit`, `kanji`, `eci` sub-encoders. | 2 |
| 4     | Mode selector: BWIPP's per-segment cost-optimization (similar to codeone's lookup but with different cost tables). | 1-2 |
| 5     | Reed-Solomon: GF(256) primitive poly 285 (different from codeone's 301). Interleaved block ECC application per the QR spec's per-version table. Reuse `gf256_gen_coeffs` style helper. | 1 |
| 6     | Matrix placement: finder patterns (3 corners + 1 for rMQR), timing rows, alignment patterns, dark module, format info, version info (≥7 only), data placement spiral. | 2-3 |
| 7     | Mask functions (8 for QR, 4 for Micro, 4 for rMQR) + ISO scoring + BWIPP tiebreak (lower mask index wins ties). | 1-2 |
| 8     | Wire `Symbology::QrCodeNative` (or replace `Symbology::QrCode`'s dispatch) + ≥3 pixs goldens per format + ≥1 golden for each EC level / version-edge case. | 2 |
| 9     | Cutover: switch swissqrcode, gs1qrcode, gs1dlqrcode, hibc_lic_qrcode, hibc_pas_qrcode wrappers from `qrcode_::encode` to the native encoder. Update PORT_STATUS / GOLDEN_COVERAGE / inventory / COMPATIBILITY_EXCEPTIONS / web catalog. Drop the `qrcode` crate dependency. | 1-2 |
| 10    | rMQR-specific: confirm all 32 variants render. Wire `Symbology::RectangularMicroQrCode`. Promote inventory's last `missing` row → implemented. | 1 |

Total: **12-15 iterations**.

## Interim fallback

If Path 3 reveals an unexpected blocker partway through (e.g., a
patent issue with kanji mode, or a spec interpretation conflict), we
can pivot to Path 2 mid-project: vendor a forked qrcode crate that
adds rMQR + BWIPP-tiebreak, ship that, and revisit Path 3 later. The
PORT_PLAN's stage 9 cutover is the natural switchover point.

## Next iteration TODO (Path 3 Stage 1)

1. Create `rust/src/symbology/qrcode_native/` directory.
2. Add `mod qrcode_native;` to `rust/src/symbology.rs` (private —
   no `Symbology` variant yet).
3. Write `rust/src/symbology/qrcode_native/mod.rs` with module-level
   rustdoc referencing the BWIPP source line ranges + a stub
   `pub(crate) fn encode(input: &[u8]) -> Result<BitMatrix, Error>`
   returning InvalidData.
4. Write `rust/src/symbology/qrcode_native/QR_NATIVE_PORT_PLAN.md`
   listing version metrics ranges + per-stage task lists.
5. Run push gate. Commit + push.

Stage 1 must compile + tests pass + no behavior change to the
existing 9 catalog rows (still routing through the upstream
`qrcode` crate via `qrcode_.rs`).
