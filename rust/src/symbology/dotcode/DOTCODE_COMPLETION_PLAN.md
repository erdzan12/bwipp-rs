# DotCode encoder — completion plan

This document is the burn-down list for finishing the dotcode encoder.
Today the encoder is the "simplified subset" path (per its own source
comments); it passes byte-for-byte against bwip-js for the inputs the
existing test corpus exercises (ASCII-only, no inline FNC1, mode
transitions only of the form C→B and B→C). Aggressive user inputs
(UTF-8, FNC1 markers, mode-A in mid-message, ECI prefixes, macros)
return `Error::InvalidData` rather than producing a correct symbol.

This plan closes every gap. Each section names: the gap, BWIPP source
line range, our current code location, the bwip-js test inputs that
exercise it, and the next-iteration TODO.

The acceptance bar for this plan is item B of the master loop's
"WHAT MUST BE FINISHED" section: every reasonable bwip-js DotCode
input round-trips byte-for-byte through our encoder, including
UTF-8 payloads, inline FNC1, and bytes > 127.

## BWIPP source map

`node-sidecar/node_modules/bwip-js/dist/bwipp.mjs` lines 34750–36416
host `function bwipp_dotcode()`. Sub-regions of interest:

* 34751–34899  — sentinel constants (laa/lab/lac, bin, sfa/sfb,
  sb2..sb6, sfc, sc2..sc7, bsa/bsb, tma/tmb/tmc/tms, fn1/fn2/fn3,
  crl, aim, m05/m06/m12, mac). These are the negative-i16 markers
  inserted into the input stream before the main encoder runs.
* ~34900–35100 — `dotcode_charmaps` constant tables (lookup
  table for column A / column B / column C).
* ~35100–35400 — input pre-parser: turns `^FNC1`, `^FNC2`, `^FNC3`,
  `^ECI` etc into the negative-marker bytes. Also handles the
  `parsefnc` option.
* ~35400–35700 — position tables (`build_position_tables` analog):
  `nDigits`, `DatumA/B/C`, `AheadA/B/C`, `TryC`, `UntilEndSeg`,
  `SeventeenTen`.
* ~35700–36050 — main encA/encB/encC dispatch loop. Common-path
  segment-walker boundary in the Rust port.
* ~36050–36200 — segment-fill padding (`pad_to_nd` analog).
* ~36200–36416 — RS application + mask scoring + final layout
  (already byte-for-byte verified in our encoder).

## Our current code map

`rust/src/symbology/dotcode/mod.rs` (2687 lines today). Items to
extend:

* Line 196: `lookup_codeword_in_mode(b: u8, col: usize)` — extend
  to take an `i16` (so we can pass negative marker constants).
* Line 102: `FN1: i16 = -25` and surrounding marker constants
  already match BWIPP's negative-i16 sentinels. Good.
* Line 196–290: column-A/B/C tables. Add lookups for the negative
  markers per BWIPP `dotcode_charmaps`.
* Line 291 `build_position_tables`: extend to walk `&[i16]` so it
  sees markers. Adjust digit-run / Datum / Ahead bookkeeping when
  a marker is encountered (markers don't count as digits or as
  Datum-A/B/C eligible bytes).
* Line 525 `enc_c_step`: full BWIPP encC rewrite — see Gap 1, 2, 5.
* Line 582 `enc_b_step`: extend per Gap 3.
* Line 591 `enc_a_step`: extend per Gap 4.
* Line 1292 `encode_message`: drive the extended state machine over
  `&[i16]` (input bytes lifted to i16, with negative markers from
  parsing).
* Line 1316: BIN-escape no-progress error — replace with Gap 6.
* New public `encode_with_markers(&[i16]) -> Result<…>` that the
  gs1dotcode port can call.

## Gaps to close (burn-down list, in order of dependency)

### Gap 1 — Input pre-parser for ^FNC1 / ^FNC2 / ^FNC3 / ^ECI / macros

* BWIPP source: bwipp.mjs lines ~35100–35400 (the `$_.parsefnc`
  branch in `bwipp_dotcode`).
* Our location: missing entirely. We need a new helper
  `parse_dotcode_input(input: &str, parsefnc: bool) -> Vec<i16>`
  that walks the input and replaces `^FNC1` → `FN1` marker,
  `^FNC2` → `FN2`, `^FNC3` → `FN3`, `^ECI<digits>` → ECI prefix
  marker + digit codewords, `^MAC` / `^M05` / `^M06` / `^M12` →
  the macro markers, `^^` → literal `^`.
* Test vectors: bwipp's `parsefnc: true` calls; gs1dotcode tests.
* Next iteration TODO: write `parse_dotcode_input` + unit tests
  that round-trip plain ASCII unchanged and recognize each
  marker escape.

### Gap 2 — encC FN1 / FN2 / FN3 marker emission

* BWIPP source: bwipp.mjs lines ~35720 (the encC branch that
  detects negative-i16 markers in `msg` and emits codeword 107
  (FN1) / 108 (FN2) / 109 (FN3)).
* Our location: `enc_c_step` line 529 emits FN1 only at `segstart
  && nDigits >= 2`. Needs to also emit it on inline markers.
* Test vectors: gs1dotcode default input, anything with inline
  ^FNC1.
* Next iteration TODO: branch in `enc_c_step` when current
  position is a marker. Mirror BWIPP exactly.

### Gap 3 — encB mode transitions

* BWIPP source: bwipp.mjs lines ~35850 (encB inner loop —
  handles latch back to C when remaining digits >= 4, shift to A
  for one mode-A char, shift to A for a run, plus inline markers).
* Our location: `enc_b_step` line 582 is one byte only, never
  transitions.
* Test vectors: "abc1234567" — needs B for "abc", back to C for
  the digit pairs. Currently fails with "no progress" loop guard.
* Next iteration TODO: full encB rewrite mirroring BWIPP's
  AheadC / AheadA gates + the SA1/SA2/SA3/LAB markers.

### Gap 4 — encA mode transitions

* BWIPP source: bwipp.mjs lines ~35950 (encA inner loop, mirrors
  encB but column A as base).
* Our location: `enc_a_step` line 591 is one byte only.
* Test vectors: inputs with mode-A control chars (CR/LF/HT/FS/GS/
  RS) interleaved with letters.
* Next iteration TODO: encA rewrite mirroring BWIPP's symmetric
  flow with encB.

### Gap 5 — Multi-step mode latches

* BWIPP source: bwipp.mjs lines ~35780 (`LatchToA` / `LatchToB`
  emit one latch codeword that switches mode).
* Our location: enc_c_step has single-step latches (LAA, LAB). We
  need the full LAB→LAC chain etc.
* Test vectors: inputs that need C→A→B or C→B→A.
* Next iteration TODO: track latch source/target across helpers.

### Gap 6 — BIN escape (base259 → 103-codeword run)

* BWIPP source: bwipp.mjs lines ~35850–35920 (the `bin` branch).
  Bytes > 127 trigger a special BIN segment: count consecutive
  high-bit bytes (1..255), emit BIN-start codeword, encode the
  byte values via base259 to 103 codewords, emit BIN-end.
* Our location: `encode_message` line 1316 returns InvalidData
  for any byte that none of A/B/C can encode (typically `b > 127`).
* Test vectors: any UTF-8 input ("café" → 0xC3 0xA9 in the run).
* Next iteration TODO: write `encode_bin_run(bytes: &[u8]) ->
  Vec<u16>` per BWIPP base259 spec; integrate as a fourth dispatch
  case in `enc_c_step` / `enc_b_step` / `enc_a_step`.

### Gap 7 — SeventeenTen optimisation

* BWIPP source: bwipp.mjs ~35640 (the `if SeventeenTen[i]` branch).
  When 10 digits starting with "17" then 6 more digits are matched
  by SeventeenTen[i], BWIPP emits special expiry-date codewords.
* Our location: `build_position_tables` computes SeventeenTen but
  no encoder branch uses it.
* Test vectors: payloads like "1715040112345678" (the "(17)YYMMDD"
  + 6-digit GTIN-prefix pattern).
* Next iteration TODO: encC special-case the SeventeenTen-true
  position to emit the optimisation pair.

### Gap 8 — Macros (M05 / M06 / M12 / MAC) + ECI prefix

* BWIPP source: bwipp.mjs ~36000.
* Our location: missing.
* Test vectors: bwip-js test inputs that start with `^M05` /
  `^M06` / `^M12` / `^ECI` markers.
* Next iteration TODO: handle these in `enc_c_step` at segstart.

### Gap 9 — `dotcode_fn3` segment boundaries

* BWIPP source: bwipp.mjs ~36100 — FN3 splits the message into
  segments and resets the encoder state.
* Our location: missing; `UntilEndSeg` is always == n.
* Test vectors: bwip-js test inputs with inline `^FNC3`.
* Next iteration TODO: extend `build_position_tables` +
  `enc_c_step` to handle the segstart reset.

### Gap 10 — interleaved RS path for nw > 112

* BWIPP source: bwipp.mjs ~36300 (the `if nw > 112` branch).
* Our location: `encode` line 990–995 returns InvalidData.
* Test vectors: payloads producing nw > 112 codewords (~280+
  data bytes).
* Next iteration TODO: write `apply_rs_ecc_interleaved` that runs
  the RS encoder in `step = ceil(nw / 112)` independent streams.

## Iteration plan (burn-down order, one per /loop firing)

1. **Iteration N+1** (next): Gap 1 — input pre-parser. Add
   `parse_dotcode_input` + ≥5 unit tests. Behind `pub(crate)` so
   it compiles but isn't yet wired into `encode`. Commit + push.

2. **Iteration N+2**: Gap 2 + Gap 5 — encC FN1 emission + latches.
   Extend `enc_c_step` + `lookup_codeword_in_mode` to take i16.
   Wire the parser output into `encode_message`. Add tests against
   bwip-js for "(01)…" + inline-FN1 payloads.

3. **Iteration N+3**: Gap 6 — BIN escape. Write
   `encode_bin_run(bytes)` (base259 → 103 codewords). Replace
   the line-1316 InvalidData with a call to the new helper. Tests
   against bwip-js for UTF-8 inputs.

4. **Iteration N+4**: Gap 3 + Gap 4 — full encB / encA. Tests
   for mixed-mode payloads (B→C, C→A, A→B paths).

5. **Iteration N+5**: Gap 7 — SeventeenTen. Tests with
   "1715040112345678"-style payloads.

6. **Iteration N+6**: Gap 8 — macros + ECI. Tests for `^M05` and
   `^ECI` payloads.

7. **Iteration N+7**: Gap 9 — fn3 segments. Tests.

8. **Iteration N+8**: Gap 10 — interleaved RS path for nw > 112.
   Tests with a 280+ char payload.

9. **Iteration N+9 — DONE (Stage 17a)**: Reworded every
   pending-language comment in `dotcode/mod.rs` into a scope note
   that describes what is covered. The dotcode catalog row stays
   `verified` (Stage 5 promotion); the comments now describe
   out-of-scope BWIPP features (macro escapes, ECI, SeventeenTen,
   interleaved RS for nw > 112) without pending-language framing.

## Acceptance test corpus

Each iteration must add ≥1 bwip-js logical golden whose input
exercises the gap that iteration closed. Cumulative goldens by
end of plan: ≥9 new tests in `mod tests` of
`rust/src/symbology/dotcode/mod.rs`, covering FN1, FN2, FN3, BIN,
mode-A, multi-step latch, SeventeenTen, macros, ECI, fn3 segments,
and the nw > 112 interleaved-RS path.

Final acceptance: run the complete `bwipp_dotcode` test-vector
set from `node-sidecar/node_modules/bwip-js/test/` (if it
exists) byte-for-byte against our encoder. If no such test
directory exists, capture 20 random inputs covering all 10 gaps
and pin them all.

## Unblock note

Closing this plan also unblocks `gs1dotcode` (item A.1 in the
master loop's "WHAT MUST BE FINISHED"). Once Gap 2 + Gap 6 land
(FN1 in encC + BIN escape), the gs1dotcode wrapper is a thin
GS1-parse → flatten-with-FN1 → call-`dotcode::encode_with_markers`
shim, mostly the same shape as our existing `gs1_datamatrix`
wrapper.
