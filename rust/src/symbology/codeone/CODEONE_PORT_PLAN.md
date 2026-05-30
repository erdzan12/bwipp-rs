# Code One port plan

USS Code One — matrix 2D barcode. BWIPP source:
`node-sidecar/node_modules/bwip-js/src/bwipp.js` lines
31458-33350 (1893 LOC of JS).

## Algorithm overview

Code One symbols come in **11 sizes** across three families:

| Family   | Sizes                              | Modules                  | Codewords (data + ECC) | RS field |
|----------|------------------------------------|--------------------------|------------------------|----------|
| Matrix   | A, B, C, D, E, F, G, H             | 16×18 .. 148×134         | 10..=1480 + 10..=560   | GF(256), poly 301 |
| Strip S  | S-10, S-20, S-30                   | 8×11 .. 8×31             | 4 / 8 / 12 + same      | GF(32), poly 37   |
| Strip T  | T-16, T-32, T-48                   | 16×17 .. 16×49           | 10 / 24 / 38 + 10/16/22 | GF(256), poly 301 |

Six data-encoding modes can switch mid-stream:

| BWIPP id          | Mode | Coverage                              | Density               |
|-------------------|------|---------------------------------------|------------------------|
| `codeone_a` = 0   | A    | ASCII (default)                       | 1 byte → 1 cw         |
| `codeone_c` = 1   | C    | C40: digits + uppercase + punctuation | 3 chars → 2 cws       |
| `codeone_t` = 2   | T    | Text: digits + lowercase + punctuation | 3 chars → 2 cws       |
| `codeone_x` = 3   | X    | X12 / EDI                             | 3 chars → 2 cws       |
| `codeone_d` = 4   | D    | Decimal (numeric compression)         | 3 digits ≈ 10 bits    |
| `codeone_b` = 5   | B    | Byte / raw binary                     | 1 byte → 1 cw         |

C / T / X all dispatch through the same `encCTX` core (BWIPP line
32691) with different value tables.

Pipeline (bwip-js order):

1. **Input pre-parser** — split into FNC1 / FNC2 / FNC3 markers +
   data bytes. ECI prefixes handled.
2. **Mode selector** — walk the input, pick A / C / T / X / D / B per
   run, emit mode-switch codewords as needed.
3. **Mode encoders** — `encA` (32607), `encCTX` (32691), `encD` (32872),
   `encB` (32975) — each emits 8-bit codewords (or 5-bit half-cws for
   S-strip) into `$_.cws`.
4. **Symbol-size selection** — walk
   `codeone_nonstypemetrics` / `codeone_stypemetrics`, pick the smallest
   version whose `dcw` accommodates `cws.len()`, pad with the dedicated
   PAD codeword (line 30797: `codeone_unlcw = 255` for matrix, mode-D
   pad-fill otherwise).
5. **Reed-Solomon ECC** — split `cws` into `rsbl` interleaved blocks,
   each appended with `rscw / rsbl` check codewords via
   `bwipp_rsecbinary` (uses `codeone_rsprod` GF tables).
6. **Codeword → matrix placement** — each 8-bit cw becomes a 4-bit top
   + 4-bit bot pair; matrix written row-pair-by-row-pair through the
   `dcol` data area into `mmat`. S-strip uses 5-bit splits (3+2 / 2+3).
7. **Symbol composition** — render `mmat` into the final `pixs` grid:
   * Apply `codeone_cpatmap` column-pattern repeats (`"121343"` for A,
     etc.).
   * Apply `artifact` start/separator/finder masks from the column
     pattern digit (49-58 ASCII = artifact index 0-9).
   * Insert row-indicator marker bits.
   * Insert reference-island pattern (`risl` / `riso` / `risi` from
     metrics).
   * Insert fixed black dots from `codeone_blackdotmap`.

## BWIPP source line ranges (cheat sheet)

| Phase                                | Lines                  |
|--------------------------------------|------------------------|
| Constants tables (versions, metrics) | 31464-31474           |
| Marker codeword constants            | 31475-31497           |
| Encoder-function dispatch table      | 31498                 |
| Column-pattern / black-dot maps      | 31499-31528           |
| RS parameters table                  | 31529                 |
| `avals` (Mode-A value map)           | 31530-31570           |
| `cnvals` / `c1vals` / `c2vals` / `c3vals` (CTX value maps) | 31571-31627 |
| Combined CTX value table             | 31628+                |
| `encA` (Mode A encoder)              | 32607-32690           |
| `encCTX` (Modes C/T/X encoder)       | 32691-32871           |
| `encD` (Mode D / Decimal encoder)    | 32872-32974           |
| `encB` (Mode B / Byte encoder)       | 32975-33050+          |
| Mode-selector loop                   | ~33060-33099          |
| RS ECC apply                         | 33100-33135           |
| Codeword → matrix placement          | 33136-33190           |
| Pixs / artifact / reference-island   | 33196-33320           |
| Renderer dispatch (`renmatrix`)      | 33321-33347           |

## Constants tables to extract

* `codeone_versionopts` — selectable versions for the `version` option.
* `codeone_stypemetrics` — `[id, rows, cols, dcol, dcw, ecw, rsbl,
  ...]` for S-strip sizes.
* `codeone_nonstypemetrics` — same shape for A..H + T-strip sizes.
* `codeone_stypevals` — base value table for S-strip codeword packing.
* `codeone_cpatmap` — column-pattern strings (e.g. `"121343"`).
* `codeone_blackdotmap` — list of (row, col) pairs to set black.
* `codeone_rsparams` — `[[GF size, poly]]` per version family.
* `avals`, `cnvals`, `c1vals`, `c2vals`, `c3vals` — encoder value
  lookup maps (ASCII / C40 / Text / X12 / Decimal).

## Stage breakdown

Each stage = at least one commit pushed to `origin/main`. Each
commit must compile, clippy clean, tests pass.

* **Stage 1 (this iteration)**: module skeleton + PORT_PLAN.md.
  No `Symbology` variant. `encode` returns `InvalidData`. ✅
* **Stage 2 — foundation**: extract the constants tables verbatim.
  Module-level rustdoc with the algorithm overview. Stub helper
  function signatures (`encode_mode_a`, `encode_mode_ctx`,
  `encode_mode_d`, `encode_mode_b`, `pick_version`,
  `apply_reed_solomon`, `place_into_matrix`, `render_pixs`) with
  `unimplemented!()` bodies. Unit tests asserting table shapes
  (lengths, first / last entries, GF-poly anchors).
* **Stage 3 — mode encoders (part 1)**: `encode_mode_a` (ASCII)
  + `encode_mode_b` (byte) — the two single-byte-per-cw modes.
  Each gets ≥3 bwip-js logical-cws goldens via a patched oracle.
* **Stage 4 — mode encoders (part 2)**: `encode_mode_ctx` for
  C / T / X (3 chars → 2 cws). Plus the
  trailing-bytes-need-shift dance. Goldens.
* **Stage 5 — mode encoder D**: decimal compression (3 digits ≈
  10 bits). Goldens for clean / odd-tail / single-tail cases.
* **Stage 6 — mode selector + symbol-size pick**: top-level
  `encode_cws(input)` returning `(cws, version)`. Walks input,
  switches modes as the rules require, picks the smallest fitting
  version.
* **Stage 7 — Reed-Solomon**: interleaved-block GF(256) + GF(32) RS.
  Reuse `crate::util::rs_gf256` if usable; otherwise vendor a
  small GF helper. Block-split + interleaved-output pinned by
  goldens against bwip-js `$_.cws` (post-RS).
* **Stage 8 — matrix placement**: `place_into_matrix` puts each
  cw's 4+4 nibbles into the `dcol`-wide data area. For S-strip,
  the 5+5 / 3+2-2+3 split. Pinned against bwip-js `$_.mmat`.
* **Stage 9 — renderer**: `render_pixs` composes the final
  `pixs` with column patterns + artifact masks + reference
  islands + black-dot map. Returns `BitMatrix`.
* **Stage 10 — promotion**: ≥3 bwip-js byte-for-byte `pixs`
  goldens (canonical / smallest / boundary), wire as
  `Symbology::CodeOne`, update PORT_STATUS / GOLDEN_COVERAGE /
  inventory / web catalog. Run scout passes. Push.

Estimated total iterations: 6-10 (stage 7 may need 2 commits;
stage 9 may need 2-3 for placement / renderer split).

## Stage 5b debug — RESOLVED

**Root cause**: BWIPP's `$f` helper (bwip-js line 631) truncates
fractional values to **Float32 precision** via `Float32Array`. The
lookup() cost accumulators are stored as f64 BUT every update goes
through `$f(...)` which clamps to f32 first. Plain f64 sums diverge
at the boundary — for "abcdef" the f64 `tc` ends at 5.0000002 (ceil
6) while f32 `tc` ends at exactly 5.0 (ceil 5). That difference
flips the mid-scan T-check at k=5 from "skip" to "fire".

**Fix**: added `ff(v: f64) -> f64` helper that mirrors `$f`. Every
fractional add in lookup() now goes through `ff(...)`. Integer-only
accumulators (`ac` via ceil-snap on non-digit chars, `bc` via `+1` /
`+3`) skip `ff` per BWIPP's `(v|0) == v` short-circuit.

Stage 5a's 10-case golden corpus extended to 11 (added "abcdef" → T).
Plus a `lookup_at_nonzero_position` test exercising mid-string
decisions ("ABCabcdef" at i=3 → T).

## Stage 6 TODO (next iteration)

Mode CTX encoder — BWIPP `encCTX` (bwip-js lines 32691-32871):

1. Implement `encode_ctx_step(ctx)` that consumes 3 chars + emits 2
   cws via CTXvalstocws (3 base-40 nibbles → uint16 → split into 2
   bytes).
2. Handle the per-mode value-table dispatch: cnvals (C), tnvals (T),
   xvals (X), with shift-table lookups (c1vals/c2vals/c3vals or
   t1vals/t2vals/t3vals) for chars NOT in the base table.
3. Wire the dispatcher loop `encode_message(input)` that selects
   encA/encCTX/encD/encB based on `ctx.mode` and runs until i==msglen.
4. Capture end-to-end goldens for inputs that mode-switch
   (uppercase → C, lowercase → T, mixed).

## Historical iteration TODO

### Stage 2 — foundation tables. Specifically:

1. Add `pub(crate) const METRICS_NONSTYPE: [...]` mirroring
   `codeone_nonstypemetrics` (8 matrix + 3 T-strip rows).
2. Add `pub(crate) const METRICS_STYPE: [...]` mirroring
   `codeone_stypemetrics` (3 S-strip rows).
3. Add marker-codeword constants (FNC1, FNC2, FNC3, LC, LB, LX,
   LT, LD, UNL, FNC4, SFT1-3, ECI, PAD, FNC1LD).
4. Add mode constants (MODE_A=0, MODE_C=1, MODE_T=2, MODE_X=3,
   MODE_D=4, MODE_B=5).
5. Add `pub(crate) const CPATMAP: ...` from
   `codeone_cpatmap` (10 entries).
6. Add `pub(crate) const BLACKDOTMAP: ...` from
   `codeone_blackdotmap` (per-version (row, col) coordinate lists
   — H has 6 pairs, etc.).
7. Add `pub(crate) const STYPEVALS: ...` (S-strip codeword base
   values — 18 entries).
8. Add `pub(crate) const RSPARAMS: ...` from `codeone_rsparams`
   ([[], [], [], [], [], [32, 37], [], [], [256, 301]]).
9. Unit tests asserting shape + a few known anchor values for
   each table.
10. Commit + push.

Stage 2 must be one commit; subsequent stages can split into
multiple commits as needed.
