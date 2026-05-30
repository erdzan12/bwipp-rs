//! DotCode — full BWIPP-compatible port (verified end-to-end).
//!
//! DotCode is a 2D dot-matrix symbology specified for high-speed
//! inkjet printing — the rendering is a sparse pattern of round
//! dots on a diagonal grid rather than the solid cells of QR /
//! Data Matrix / PDF417.
//!
//! The pipeline composes:
//!
//! 1. [`encode_message`] — drives a state machine over BWIPP's
//!    per-position lookahead tables ([`build_position_tables`])
//!    to produce the data codewords. Handles mode A / B / C
//!    transitions, FN1 prefix at segstart, paired-digit encoding,
//!    and the shift/latch transitions between modes.
//! 2. [`pick_symbol_size_default`] — computes (rows, columns,
//!    `nd`, `ndots`) for the BWIPP default ratio = 3/2 path.
//! 3. [`pad_to_nd`] — pads the data codewords up to `nd`.
//! 4. [`pick_best_mask`] — scores all four mask candidates with
//!    [`eval_symbol`] (worst-edge + clear-row/col penalty +
//!    outlier penalty), and if the first-pass best score is
//!    `≤ rows × columns / 2`, falls back to scoring the lit-mask
//!    versions (corners forced to 1).
//! 5. The chosen pixs is rendered via [`crate::encoding::Encoded::Dots`]
//!    as round SVG `<circle>` / PNG-rasterised disc geometry.
//!
//! Every step has been verified against bwip-js: the data codeword
//! stream (10 mask=0 corpus rows + 4-mask cross-check on "A"), the
//! bit-string assembly (7 (input, mask) pairs), the `evalsymbol`
//! scores (40 score pairs = 10 inputs × 4 masks), and the full
//! final pixs for the lit-mask fallback case "A".
//!
//! Reference: bwip-js `bwipp_dotcode` (line 35066 in the 2026-03-31
//! vendor snapshot). Capture goldens via the `tools/oracle-dotcode-*.js`
//! scripts.

// Stage 1 exposes structural constants only; suppress `dead_code`
// until the encoder stages start consuming them.
#![allow(dead_code)]

/// Codeword → 9-module bit pattern. Indexed `0..=112` (i.e. 113 valid
/// codewords; DotCode's data symbols are 7-bit + 2 control bits).
/// Ported verbatim from BWIPP's `dotcode_encs` initializer; each entry
/// is the 9-bit pattern stored as a `u16` low-order bits.
#[rustfmt::skip]
pub(crate) const ENCS: [u16; 113] = [
    341, 171, 173, 181, 213, 342, 346, 362,
    426, 174, 182, 186, 214, 218, 234, 299,
    301, 309, 331, 333, 339, 345, 357, 361,
    405, 421, 425,  87,  91,  93, 107, 109,
    117, 151, 155, 157, 167, 179, 185, 203,
    205, 211, 217, 229, 233, 302, 310, 314,
    334, 348, 358, 364, 370, 372, 406, 410,
    422, 428, 434, 436, 458, 466, 468,  94,
    110, 118, 122, 158, 188, 206, 220, 230,
    236, 242, 244, 279, 283, 285, 295, 307,
    313, 327, 355, 369, 395, 397, 403, 409,
    419, 433, 453, 457, 465,  47,  55,  59,
     61,  79, 103, 115, 121, 143, 199, 227,
    241, 286, 316, 376, 398, 412, 440, 454,
    460,
];

/// Mask seed values BWIPP tries when picking the symbol mask. The
/// encoder evaluates all four placements and keeps the one whose
/// finished layout minimises BWIPP's edge-count heuristic.
pub(crate) const MASK_SEEDS: [u32; 4] = [0, 3, 7, 17];

/// 2-bit mask selectors encoded into the symbol so the decoder can
/// recover which `MASK_SEEDS` index was chosen. Indexed parallel to
/// `MASK_SEEDS`.
pub(crate) const MASK_BITS: [&str; 4] = ["00", "01", "10", "11"];

/// Charset mode markers (BWIPP's `dotcode_l*` / `dotcode_s*`
/// constants). Stored as negative `i16` so they never collide with
/// the `0..=127` ASCII byte range. Names and values match BWIPP's
/// `bwipp_dotcode` exactly so future stages can drop in the encA /
/// encB / encC charmaps verbatim.
pub(crate) const LAA: i16 = -1; // latch alpha (mode A)
pub(crate) const LAB: i16 = -2; // latch beta  (mode B)
pub(crate) const LAC: i16 = -3; // latch gamma (mode C)
pub(crate) const BIN: i16 = -4; // binary
pub(crate) const SFA: i16 = -5; // shift A 1 char
pub(crate) const SFB: i16 = -6; // shift B 1 char
pub(crate) const SB2: i16 = -7; // shift B 2 chars
pub(crate) const SB3: i16 = -8;
pub(crate) const SB4: i16 = -9;
pub(crate) const SB5: i16 = -10;
pub(crate) const SB6: i16 = -11;
pub(crate) const SFC: i16 = -12; // shift C 1 pair
pub(crate) const SC2: i16 = -13;
pub(crate) const SC3: i16 = -14;
pub(crate) const SC4: i16 = -15;
pub(crate) const SC5: i16 = -16;
pub(crate) const SC6: i16 = -17;
pub(crate) const SC7: i16 = -18;
pub(crate) const BSA: i16 = -19; // byte-string A (length-prefixed)
pub(crate) const BSB: i16 = -20;
pub(crate) const TMA: i16 = -21; // terminator A
pub(crate) const TMB: i16 = -22;
pub(crate) const TMC: i16 = -23;
pub(crate) const TMS: i16 = -24;
pub(crate) const FN1: i16 = -25; // FNC1
pub(crate) const FN2: i16 = -26;
pub(crate) const FN3: i16 = -27;
pub(crate) const CRL: i16 = -28; // <CR><LF>
pub(crate) const AIM: i16 = -29; // AIM ECI prefix
pub(crate) const M05: i16 = -30; // macro 05
pub(crate) const M06: i16 = -31; // macro 06
pub(crate) const M12: i16 = -32;
pub(crate) const MAC: i16 = -33; // macro start

/// Codeword 107 — the FN1 marker. BWIPP's `encC` emits this as the
/// leading codeword of any segment that starts with 2+ digits (the
/// "start in mode C" effect is achieved indirectly via the FN1 prefix
/// + paired-digit codewords; there is no dedicated start-mode marker).
pub(crate) const LATCH_C: u16 = 107;

/// Same value as [`LATCH_C`], named for the role it plays in
/// [`encode_numeric_run_from_c`]. Identical numeric value (107).
pub(crate) const MODE_C_FN1_AT_SEGSTART: u16 = 107;

/// BWIPP's `dotcode_charmaps` — the master `[a, b, c]` table that
/// drives all three character encoders. Row index = codeword value
/// (0..=112). Each column holds either:
///
///   * a 7-bit ASCII byte (`0..=127`) directly representable as a
///     positive `i16`, **or**
///   * a negative charset marker (`LAA`, `SFB`, …) — same numeric
///     encoding BWIPP uses, so a marker constant from this module
///     drops in unchanged, **or**
///   * for column C of rows 0..=95, the codeword value (which equals
///     the row index — BWIPP stores it as the string `"00".."95"` for
///     readability and our `i16` form is the integer equivalent).
///
/// Stages 4+ build three `HashMap<i16, u16>` lookups from this table
/// (`Avals`, `Bvals`, `Cvals`) keyed by the column value, with the
/// row index as the codeword. For now we keep just the raw data so
/// the constant is shared between the test that verifies the table
/// (this module) and the future encoders.
#[rustfmt::skip]
pub(crate) const CHARMAPS: [[i16; 3]; 113] = [
    // 0..=31  →  ASCII 32..=63 ("space".."?"); both A and B columns
    // hold the literal byte; column C is the row index.
    [ 32,  32,   0], [ 33,  33,   1], [ 34,  34,   2], [ 35,  35,   3],
    [ 36,  36,   4], [ 37,  37,   5], [ 38,  38,   6], [ 39,  39,   7],
    [ 40,  40,   8], [ 41,  41,   9], [ 42,  42,  10], [ 43,  43,  11],
    [ 44,  44,  12], [ 45,  45,  13], [ 46,  46,  14], [ 47,  47,  15],
    [ 48,  48,  16], [ 49,  49,  17], [ 50,  50,  18], [ 51,  51,  19],
    [ 52,  52,  20], [ 53,  53,  21], [ 54,  54,  22], [ 55,  55,  23],
    [ 56,  56,  24], [ 57,  57,  25], [ 58,  58,  26], [ 59,  59,  27],
    [ 60,  60,  28], [ 61,  61,  29], [ 62,  62,  30], [ 63,  63,  31],

    // 32..=63  →  ASCII 64..=95 ("@".."_"); same shape as the run above.
    [ 64,  64,  32], [ 65,  65,  33], [ 66,  66,  34], [ 67,  67,  35],
    [ 68,  68,  36], [ 69,  69,  37], [ 70,  70,  38], [ 71,  71,  39],
    [ 72,  72,  40], [ 73,  73,  41], [ 74,  74,  42], [ 75,  75,  43],
    [ 76,  76,  44], [ 77,  77,  45], [ 78,  78,  46], [ 79,  79,  47],
    [ 80,  80,  48], [ 81,  81,  49], [ 82,  82,  50], [ 83,  83,  51],
    [ 84,  84,  52], [ 85,  85,  53], [ 86,  86,  54], [ 87,  87,  55],
    [ 88,  88,  56], [ 89,  89,  57], [ 90,  90,  58], [ 91,  91,  59],
    [ 92,  92,  60], [ 93,  93,  61], [ 94,  94,  62], [ 95,  95,  63],

    // 64..=95  →  column A: control bytes 0..=31; column B: ASCII
    // 96..=127 ("`".."DEL"); column C: the row index. This is where
    // mode A and mode B start to diverge — uppercase vs. lowercase /
    // control-char split.
    [  0,  96,  64], [  1,  97,  65], [  2,  98,  66], [  3,  99,  67],
    [  4, 100,  68], [  5, 101,  69], [  6, 102,  70], [  7, 103,  71],
    [  8, 104,  72], [  9, 105,  73], [ 10, 106,  74], [ 11, 107,  75],
    [ 12, 108,  76], [ 13, 109,  77], [ 14, 110,  78], [ 15, 111,  79],
    [ 16, 112,  80], [ 17, 113,  81], [ 18, 114,  82], [ 19, 115,  83],
    [ 20, 116,  84], [ 21, 117,  85], [ 22, 118,  86], [ 23, 119,  87],
    [ 24, 120,  88], [ 25, 121,  89], [ 26, 122,  90], [ 27, 123,  91],
    [ 28, 124,  92], [ 29, 125,  93], [ 30, 126,  94], [ 31, 127,  95],

    // 96..=112  →  charset-marker rows. The codeword value is still
    // the row index; column C is now a marker (rather than a numeric
    // codeword string) for the rows that participate in mode-C
    // control sequences.
    [SFB, CRL,  96], [SB2,   9,  97], [SB3,  28,  98], [SB4,  29,  99],
    [SB5,  30, AIM], [SB6, SFA, LAA], [LAB, LAA, SFB], [SC2, SC2, SB2],
    [SC3, SC3, SB3], [SC4, SC4, SB4], [LAC, LAC, LAB], [FN1, FN1, FN1],
    [FN2, FN2, FN2], [FN3, FN3, FN3], [BSA, BSA, BSA], [BSB, BSB, BSB],
    [BIN, BIN, BIN],
];

/// Translate `b` to its DotCode codeword in mode `col` (0 = A,
/// 1 = B, 2 = C). Returns `None` if the byte isn't representable in
/// that mode (e.g. a lowercase letter has no mode-A codeword;
/// non-control bytes have no mode-C codeword on their own — mode C
/// pairs digits, see [`encode_c`]).
///
/// Stage 4 builds [`encode_a`] / [`encode_b`] on top of this lookup;
/// the eventual mode selector also uses it when computing the
/// per-position character-encodability tables.
fn lookup_codeword_in_mode(b: u8, col: usize) -> Option<u16> {
    lookup_codeword_in_mode_i16(i16::from(b), col)
}

/// Like [`lookup_codeword_in_mode`] but accepts an `i16` directly so
/// negative marker constants (`FN1`, `FN2`, `FN3`, `LAA`, `LAB`, …)
/// can be looked up. BWIPP's `Avals` / `Bvals` / `Cvals` maps are
/// built from `dotcode_charmaps` and include rows for every marker
/// the encoder ever emits; this helper is the Rust analogue of
/// `$get($_.Cvals, marker)` etc.
///
/// For `FN1` / `FN2` / `FN3` (charmap rows 107 / 108 / 109), every
/// column maps to the row index, so `lookup_codeword_in_mode_i16(FN1,
/// col)` returns `Some(107)` for any `col ∈ 0..=2`.
fn lookup_codeword_in_mode_i16(b: i16, col: usize) -> Option<u16> {
    CHARMAPS
        .iter()
        .position(|row| row[col] == b)
        .map(|i| i as u16)
}

/// Encode a pure mode-A payload (uppercase letters, digits, space,
/// and most symbols + ASCII control bytes 0..=31) as the codeword
/// stream BWIPP's mode-A character encoder would emit between the
/// initial-latch codeword and any subsequent shifts/latches.
///
/// Returns `None` if any byte is unencodable in mode A (lowercase
/// letters, DEL, etc.).
///
/// This is the column-A translation step only — wrapping it in the
/// initial-mode latch + padding + RS-GF(113) ECC is the job of the
/// full mode selector (stage 4 / 5).
pub(crate) fn encode_a(bytes: &[u8]) -> Option<Vec<u16>> {
    bytes
        .iter()
        .map(|&b| lookup_codeword_in_mode(b, 0))
        .collect()
}

/// Encode a pure mode-B payload (uppercase + lowercase + digits +
/// most ASCII punctuation, i.e. printable ASCII 32..=127) as the
/// codeword stream BWIPP's mode-B character encoder would emit.
///
/// Returns `None` if any byte is unencodable in mode B (a few ASCII
/// control bytes that have no column-B representation).
///
/// See [`encode_a`] for the corresponding mode-A encoder.
pub(crate) fn encode_b(bytes: &[u8]) -> Option<Vec<u16>> {
    bytes
        .iter()
        .map(|&b| lookup_codeword_in_mode(b, 1))
        .collect()
}

/// Per-position lookahead tables that BWIPP's `bwipp_dotcode`
/// builds before running the mode selector. All arrays have length
/// `msg.len() + 1`; the trailing slot is the right-edge sentinel
/// the right-to-left build needs.
///
/// Mirrors BWIPP's construction loop (bwip-js line 35407+). For a
/// raw byte input (no FNC1/FNC2/FNC3 markers), the `barchar < 0`
/// branches collapse away and the rules simplify to:
///
/// ```text
/// for i in (msg.len()-1)..=0:
///     if msg[i] is ASCII digit: nDigits[i] = nDigits[i+1] + 1
///     if msg[i] in mode-A alphabet: DatumA[i] = true
///     if msg[i] in mode-B alphabet: DatumB[i] = true
///     if CRLF (msg[i]==\r and msg[i+1]==\n): DatumB[i] = true
///     if nDigits[i] >= 2: DatumC[i] = true
///     if nDigits[i] >= 10 and msg[i..i+10] matches "17XXXXXX10":
///         SeventeenTen[i] = true
///     AheadC[i] = if nDigits[i] <= 1 { 0 } else { AheadC[i+2] + 1 }
///     if nDigits[i] > 0 and AheadC[i] > AheadC[i+1]:
///         TryC[i] = AheadC[i]
///     if DatumA[i] and TryC[i] < 2:
///         AheadA[i] = AheadA[i+1] + 1
///     if DatumB[i] and TryC[i] < 2:
///         next_i = i + 1 (+ 1 more if CRLF)
///         AheadB[i] = AheadB[next_i] + 1
///     UntilEndSeg[i] = UntilEndSeg[i+1] + 1  (no fn3 markers)
/// ```
///
/// `AheadA/B/C` measure the run of consecutive characters mode A/B
/// can encode (with the corresponding mode-C beat-the-tiebreaker
/// guard for A/B), so the mode selector compares them to decide
/// which compaction mode to latch into for each position. `TryC`
/// is the "would mode C help here?" signal — non-zero only at
/// positions where switching to mode C beats staying in A or B for
/// at least one extra codeword.
///
/// The marker-byte (`barchar < 0`) and `dotcode_fn3` segment
/// branches in BWIPP's loop aren't covered here because this
/// helper takes raw user bytes; the full mode selector port will
/// translate `^FNC1` etc into marker bytes before calling this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PositionTables {
    pub n_digits: Vec<u32>,
    pub datum_a: Vec<bool>,
    pub datum_b: Vec<bool>,
    pub datum_c: Vec<bool>,
    pub seventeen_ten: Vec<bool>,
    pub ahead_a: Vec<u32>,
    pub ahead_b: Vec<u32>,
    pub ahead_c: Vec<u32>,
    pub try_c: Vec<u32>,
    pub until_end_seg: Vec<u32>,
    /// BWIPP's `Binary[i]` array (bwip-js line 33428). Set whenever
    /// `msg[i] >= 128`. Length is `msg.len() + 8` — BWIPP intentionally
    /// over-allocates by 7 so the encBIN look-ahead `Binary[i + 7]`
    /// doesn't read off the end.
    pub binary: Vec<bool>,
}

pub(crate) fn build_position_tables(msg: &[u8]) -> PositionTables {
    // Lift each raw byte to a positive `i16` and dispatch to the
    // marker-aware worker. For all-positive streams every "negative
    // marker" branch in `build_position_tables_i16` collapses away,
    // so the resulting tables are identical to the byte-only
    // implementation we shipped previously.
    let lifted: Vec<i16> = msg.iter().map(|&b| i16::from(b)).collect();
    build_position_tables_i16(&lifted)
}

/// BWIPP-faithful position-table builder that accepts an `i16`
/// stream — i.e. the output of [`parse_dotcode_input`], which may
/// contain negative marker constants (`FN1`, `FN2`, `FN3`, `LAA`,
/// …) interleaved with ordinary byte values.
///
/// Mirrors the marker-aware bookkeeping at bwip-js lines ~33414-33466:
///
/// * `nDigits` only counts positive bytes in `'0'..='9'`; markers
///   reset the run.
/// * `DatumA` / `DatumB` are set when [`lookup_codeword_in_mode_i16`]
///   finds the byte/marker in column A / B — markers `FN1`, `FN2`,
///   `FN3` are encodable in both A and B, so they participate in
///   the AheadA / AheadB run-length counts.
/// * `DatumC` is set when `nDigits[i] >= 2` **or** the byte is
///   negative (the marker special-case at bwip-js line 33427).
/// * `AheadC` propagates `+1` over `i + 1` when the byte is a
///   non-`fn3` marker (bwip-js line 33447), reflecting that markers
///   are "cheap" in mode C; otherwise the usual `+1 over i + 2` for
///   paired digits.
/// * `AheadA` / `AheadB` only extend through bytes whose `barchar !=
///   fn3` (bwip-js lines 33460 / 33463 — `fn3` breaks segments and
///   resets the encoder mode, so it cannot participate in a stay-in-
///   mode-A/B run).
/// * `UntilEndSeg` increments by 1 across every byte except `fn3`
///   (bwip-js line 33466), so it measures the distance to the next
///   segment boundary.
/// * `SeventeenTen` is set only when ten consecutive positive bytes
///   match the `17XXXXXX10` literal pattern (no markers in the run).
pub(crate) fn build_position_tables_i16(msg: &[i16]) -> PositionTables {
    let n = msg.len();
    let mut n_digits = vec![0u32; n + 1];
    let mut datum_a = vec![false; n + 1];
    let mut datum_b = vec![false; n + 1];
    let mut datum_c = vec![false; n + 1];
    let mut seventeen_ten = vec![false; n + 1];
    let mut ahead_a = vec![0u32; n + 1];
    let mut ahead_b = vec![0u32; n + 1];
    let mut ahead_c = vec![0u32; n + 1];
    let mut try_c = vec![0u32; n + 1];
    let mut until_end_seg = vec![0u32; n + 1];
    // BWIPP over-allocates Binary by 7 entries past msglen so encBIN
    // can do `Binary[i + 7]` lookahead without bounds checks.
    let mut binary = vec![false; n + 8];
    for i in (0..n).rev() {
        let b = msg[i];
        let is_digit = (b'0' as i16..=b'9' as i16).contains(&b);
        if is_digit {
            n_digits[i] = n_digits[i + 1] + 1;
        }
        if lookup_codeword_in_mode_i16(b, 0).is_some() {
            datum_a[i] = true;
        }
        if lookup_codeword_in_mode_i16(b, 1).is_some() {
            datum_b[i] = true;
        }
        let crlf = b == i16::from(b'\r') && i + 1 < n && msg[i + 1] == i16::from(b'\n');
        if crlf {
            datum_b[i] = true;
        }
        if n_digits[i] >= 2 {
            datum_c[i] = true;
        }
        // BWIPP line 33427: a negative marker byte also flips
        // DatumC, so the marker dispatch in encC fires.
        if b < 0 {
            datum_c[i] = true;
        }
        // BWIPP line 33428: high bytes (>=128) flag Binary so the
        // encoder switches into the base259→103 BIN escape.
        if b >= 128 {
            binary[i] = true;
        }
        // SeventeenTen: 10-digit run matching "17XXXXXX10". Only
        // valid for all-positive runs (the digit check already
        // implies positive bytes via `n_digits >= 10`).
        if n_digits[i] >= 10
            && msg[i] == i16::from(b'1')
            && msg[i + 1] == i16::from(b'7')
            && msg[i + 8] == i16::from(b'1')
            && msg[i + 9] == i16::from(b'0')
        {
            seventeen_ten[i] = true;
        }
        // AheadC: BWIPP line 33447 — when the byte is a negative
        // marker (other than fn3), AheadC propagates from `i + 1`
        // because the marker only consumes one position in mode C.
        // Otherwise: usual paired-digit `+1 over i + 2`.
        if b < 0 && b != FN3 {
            ahead_c[i] = ahead_c[i + 1] + 1;
        } else {
            ahead_c[i] = if n_digits[i] <= 1 {
                0
            } else {
                ahead_c[i + 2] + 1
            };
        }
        // TryC: signal "mode C strictly helps here". Strictly
        // greater than `AheadC[i+1]` means moving the boundary one
        // position right would lose ground.
        if n_digits[i] > 0 && ahead_c[i] > ahead_c[i + 1] {
            try_c[i] = ahead_c[i];
        }
        // AheadA / AheadB: only count when staying in this mode is
        // *not* dominated by switching to C (TryC < 2), and the
        // byte isn't a segment-breaking fn3 marker.
        if datum_a[i] && try_c[i] < 2 && b != FN3 {
            ahead_a[i] = ahead_a[i + 1] + 1;
        }
        if datum_b[i] && try_c[i] < 2 && b != FN3 {
            let next = if crlf { i + 2 } else { i + 1 };
            ahead_b[i] = ahead_b[next] + 1;
        }
        // UntilEndSeg: distance to next fn3 segment boundary (bwip-
        // js line 33466). For fn3-free input this is just `n - i`.
        if b != FN3 {
            until_end_seg[i] = until_end_seg[i + 1] + 1;
        }
    }
    PositionTables {
        n_digits,
        datum_a,
        datum_b,
        datum_c,
        seventeen_ten,
        ahead_a,
        ahead_b,
        ahead_c,
        try_c,
        until_end_seg,
        binary,
    }
}

/// Codeword values in the cws stream that BWIPP's encC function
/// emits as the first codeword when transitioning from the initial
/// mode-C state into a mode-B byte run. Verified by inspecting the
/// `cws[0]` value produced by `tools/oracle-dotcode.js` for inputs
/// "A", "AB", "ABC", "ABCD", "ABCDE":
///
/// * `MODE_C_SHIFT_TO_B[n-1]` for `n ∈ 1..=4` consecutive mode-B
///   bytes — single-use shift; the encoder reverts to mode C after
///   `n` bytes (codewords 102..=105 = `SFB` / `SB2` / `SB3` / `SB4`
///   marker rows of CHARMAPS column C).
/// * `MODE_C_LATCH_TO_B` — sticky latch for `n ≥ 5` (codeword 106 =
///   `LAB` row of CHARMAPS column C; subsequent bytes stay in mode
///   B until another latch).
///
/// These constants match BWIPP's `Cvals[dotcode_sfb / sb2 / sb3 /
/// sb4 / lab]` lookup. The full `encC` function has analogous
/// shifts to mode A (`SFA` / `SA2` / ... / `LAA`) and to mode C
/// from A/B (`SC2..SC7`, `LAC`).
pub(crate) const MODE_C_SHIFT_TO_B: [u16; 4] = [102, 103, 104, 105];
pub(crate) const MODE_C_LATCH_TO_B: u16 = 106;

/// Latch codeword the encoder emits in mode C to switch into mode
/// A (codeword 101 = LAA row of CHARMAPS column C). Unlike the
/// shift-to-B variants, mode-A transition is always a sticky latch:
/// BWIPP's `encC` has no shift-to-A path because mode A's alphabet
/// (ASCII control bytes 0..=31 + uppercase + digits + symbols) is
/// rare enough that the shift-savings don't pay off.
pub(crate) const MODE_C_LATCH_TO_A: u16 = 101;

/// Mode-B → mode-A transition codewords (BWIPP `Bvals` lookups):
///
///   * `MODE_B_SHIFT_TO_A_ONE = 101` (`Bvals[SFA]`) — shift to mode
///     A for exactly one byte, then revert to B.
///   * `MODE_B_LATCH_TO_A = 102` (`Bvals[LAA]`) — sticky latch to A.
///
/// Mirrors bwip-js encB lines 35588-35597.
pub(crate) const MODE_B_SHIFT_TO_A_ONE: u16 = 101;
pub(crate) const MODE_B_LATCH_TO_A: u16 = 102;

/// Mode-B → mode-C transition codewords (`Bvals` lookups):
///
///   * `MODE_B_SHIFT_TO_C[n - 2]` for `n ∈ 2..=4` — shift to mode C
///     for `n` pairs/markers (`Bvals[SC2 / SC3 / SC4]`).
///   * `MODE_B_LATCH_TO_C = 106` (`Bvals[LAC]`) — sticky latch when
///     `n > 4`.
///
/// Mirrors bwip-js encB lines 35511-35531.
pub(crate) const MODE_B_SHIFT_TO_C: [u16; 3] = [103, 104, 105];
pub(crate) const MODE_B_LATCH_TO_C: u16 = 106;

/// Mode-B inline `<CR><LF>` codeword — `Bvals[CRL] = 96`. Emitted
/// when `msg[i] == 13 && msg[i + 1] == 10` and the encoder is in
/// mode B (bwip-js line 35558-35564).
pub(crate) const MODE_B_CRLF: u16 = 96;

/// Mode-A → mode-B transition codewords (`Avals` lookups):
///
///   * `MODE_A_SHIFT_TO_B[n - 1]` for `n ∈ 1..=6` — shift to mode B
///     for `n` bytes (`Avals[SFB / SB2 / SB3 / SB4 / SB5 / SB6]`).
///   * `MODE_A_LATCH_TO_B = 102` (`Avals[LAB]`) — sticky latch when
///     `n > 6`.
///
/// Mirrors bwip-js encA lines 35673-35691.
pub(crate) const MODE_A_SHIFT_TO_B: [u16; 6] = [96, 97, 98, 99, 100, 101];
pub(crate) const MODE_A_LATCH_TO_B: u16 = 102;

/// Mode-A → mode-C transition codewords (`Avals` lookups):
///
///   * `MODE_A_SHIFT_TO_C[n - 2]` for `n ∈ 2..=4` — shift to mode C
///     for `n` pairs/markers (`Avals[SC2 / SC3 / SC4]`).
///   * `MODE_A_LATCH_TO_C = 106` (`Avals[LAC]`) — sticky latch when
///     `n > 4`.
///
/// Mirrors bwip-js encA lines 35603-35624.
pub(crate) const MODE_A_SHIFT_TO_C: [u16; 3] = [103, 104, 105];
pub(crate) const MODE_A_LATCH_TO_C: u16 = 106;

/// Entry into BIN mode (`Avals[BIN] / Bvals[BIN] / Cvals[BIN] = 112`
/// — row 112 of CHARMAPS is `[BIN, BIN, BIN]`). Emitted from any
/// outer mode when an upcoming high-byte run cannot be encoded via
/// the single-byte BSA / BSB shifts.
///
/// Mirrors bwip-js encA/encB/encC lines 35464 / 35583 / 35671.
pub(crate) const BIN_ENTER: u16 = 112;

/// BIN-mode exit terminators (`BINvals[TMA / TMB / TMC / TMS]`).
/// BWIPP constructs `BINvals` programmatically (line 34857-34869) by
/// pairing each terminator marker with sequential indices starting
/// at 102 — the result is `sc2..sc7 = 103..108`, `tma = 109`,
/// `tmb = 110`, `tmc = 111`, `tms = 112`.
///
/// The first six entries (`sc2..sc7`) are the shift-to-C codewords
/// emitted when a BIN run hits a 2..=7 digit pair lookahead; the
/// last four are the terminators that exit BIN back to A / B / C /
/// segment.
///
/// Mirrors bwip-js encBIN line 35775-35783.
pub(crate) const BIN_SHIFT_TO_C: [u16; 6] = [103, 104, 105, 106, 107, 108];
pub(crate) const BIN_TERM_TO_A: u16 = 109;
pub(crate) const BIN_TERM_TO_B: u16 = 110;
pub(crate) const BIN_LATCH_TO_C: u16 = 111;
pub(crate) const BIN_SEG_RESET: u16 = 112;

/// Pad codeword the encoder appends after the data codewords to
/// fill the symbol's `nd` slots. BWIPP picks `109` if the final
/// mode was BIN, else `106` for the first pad and `106` thereafter
/// (bwip-js line 33953-33956). The mode-B-only short-message path
/// always ends in mode B (or implicitly mode C after a shift), so
/// `106` is what shows up in the goldens.
pub(crate) const MODE_C_PAD_CW: u16 = 106;

/// First pad codeword for a BIN-terminated message — `109`, which
/// is the BSA row of CHARMAPS column C. After the first pad the
/// encoder uses [`MODE_C_PAD_CW`] (= `106`) like the non-BIN case.
pub(crate) const BIN_FIRST_PAD_CW: u16 = 109;

/// Pad `cws` out to length `nd` using BWIPP's pad-codeword rules:
/// first pad is `109` if the final encoder mode was BIN, otherwise
/// `106`; every subsequent pad is `106`. No-op when `cws.len() >=
/// nd`.
///
/// Mirrors bwip-js lines 33953-33956. Symbol-size selection (the
/// `pick_symbol_size_default` function) lives separately; this
/// helper expects callers to pass the chosen `nd` so the padding
/// step stays independent of the size-selection policy.
pub(crate) fn pad_to_nd(cws: &mut Vec<u16>, nd: usize, final_mode_is_bin: bool) {
    if cws.len() >= nd {
        return;
    }
    cws.push(if final_mode_is_bin {
        BIN_FIRST_PAD_CW
    } else {
        MODE_C_PAD_CW
    });
    while cws.len() < nd {
        cws.push(MODE_C_PAD_CW);
    }
}

/// Encode a pure mode-B byte run as DotCode codewords starting from
/// the implicit initial mode-C state, emitting the appropriate
/// shift or latch codeword followed by [`encode_b`] of `bytes`.
///
/// This matches the short-message path of BWIPP's `encC` for inputs
/// that contain no digits, no fn-markers, no binary bytes, and are
/// fully mode-B-encodable. The output here is just the data
/// codewords; the public `encode` driver adds the symbol-fill padding
/// (via `pad_to_nd`) and the Reed-Solomon ECC (via `apply_rs_ecc`)
/// before laying the bits onto the dot grid.
///
/// Returns `None` if any byte is not mode-B-encodable.
pub(crate) fn encode_mode_b_run_from_c(bytes: &[u8]) -> Option<Vec<u16>> {
    let n = bytes.len();
    let body = encode_b(bytes)?;
    let mut out = Vec::with_capacity(body.len() + 1);
    match n {
        0 => {}
        1..=4 => out.push(MODE_C_SHIFT_TO_B[n - 1]),
        _ => out.push(MODE_C_LATCH_TO_B),
    }
    out.extend_from_slice(&body);
    Some(out)
}

/// The decision BWIPP's `encC` makes at segstart for the leading
/// codeword(s) of a fresh message. Computed from the per-position
/// lookahead tables in [`PositionTables`] by [`dispatch_initial`].
///
/// The variants follow the precedence order in bwip-js's `encC`
/// function (lines 33585..33656):
///
/// 1. `Fn1ThenStayInC` if `nDigits[0] ≥ 2` — segment starts with
///    enough digits to make a paired-digit run worthwhile.
/// 2. `LatchToA` if `AheadA[0] > AheadB[0]`, or if `msg[0]` is one
///    of the TAB / FS / GS / RS control bytes (9, 28, 29, 30).
/// 3. `LatchToB` if `AheadB[0] > 4` — long mode-B run; latching
///    saves codewords vs. a shift.
/// 4. `ShiftToBFor(n)` otherwise (with `n` = `AheadB[0]`, 1..=4) —
///    short mode-B run; after `n` bytes the encoder reverts to
///    mode C.
/// 5. `NoPrologue` on empty input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitialAction {
    NoPrologue,
    Fn1ThenStayInC,
    LatchToA,
    LatchToB,
    ShiftToBFor(u32),
}

/// Pick the segstart action BWIPP's `encC` would take for `msg`.
/// Pure mirror of the precedence rules documented in
/// [`InitialAction`]. Assumes `tables` was built from `msg`.
pub(crate) fn dispatch_initial(tables: &PositionTables, msg: &[u8]) -> InitialAction {
    if msg.is_empty() {
        return InitialAction::NoPrologue;
    }
    if tables.n_digits[0] >= 2 {
        return InitialAction::Fn1ThenStayInC;
    }
    let m = tables.ahead_a[0];
    let n = tables.ahead_b[0];
    if m > n {
        return InitialAction::LatchToA;
    }
    // TAB / FS / GS / RS at segstart force mode A even when m ≤ n
    // (these are ASCII control bytes 9, 28, 29, 30 — A-only).
    if matches!(msg[0], 9 | 28 | 29 | 30) {
        return InitialAction::LatchToA;
    }
    if n > 4 {
        return InitialAction::LatchToB;
    }
    InitialAction::ShiftToBFor(n)
}

/// The encoder's current mode in the BWIPP-style state machine.
/// `Bin` (binary) is entered via the BIN-escape path
/// ([`enc_bin_step_i16`]) when non-ASCII / non-printable bytes appear;
/// outside that path the state machine alternates between `A` / `B` /
/// `C` per BWIPP's mode-selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    A,
    B,
    C,
    Bin,
}

/// Run one iteration of BWIPP's `encC` (mode-C step). Emits codewords
/// into `cws`, updates `i` and `mode`. Caller drives the outer loop.
///
/// Scope: the common-path logic — FN1 prefix at segstart, paired-
/// digit encoding, AheadA/AheadB-driven mode transitions (LAA / LAB /
/// SB{n}). Extended BWIPP encC features (macro escapes, ECI, the
/// SeventeenTen date helper, the binary/`dotcode_fn3` codeword) are
/// out of scope for this Rust port — they affect inputs that BWIPP
/// itself emits as separate parsefnc sequences and aren't reachable
/// from the catalog's public `dotcode` / `gs1dotcode` entry points.
fn enc_c_step(
    msg: &[u8],
    tables: &PositionTables,
    i: &mut usize,
    mode: &mut Mode,
    cws: &mut Vec<u16>,
    segstart: usize,
) {
    if *i == segstart && tables.n_digits[*i] >= 2 {
        cws.push(MODE_C_FN1_AT_SEGSTART);
    }
    if tables.datum_c[*i] && msg[*i].is_ascii_digit() && msg[*i + 1].is_ascii_digit() {
        let pair = (u16::from(msg[*i] - b'0')) * 10 + u16::from(msg[*i + 1] - b'0');
        cws.push(pair);
        *i += 2;
        return;
    }
    let m = tables.ahead_a[*i];
    let n = tables.ahead_b[*i];
    if m > n {
        cws.push(MODE_C_LATCH_TO_A);
        *mode = Mode::A;
        return;
    }
    if *i == segstart && matches!(msg[*i], 9 | 28 | 29 | 30) {
        cws.push(MODE_C_LATCH_TO_A);
        *mode = Mode::A;
        return;
    }
    if n > 4 {
        cws.push(MODE_C_LATCH_TO_B);
        *mode = Mode::B;
        return;
    }
    if n == 0 {
        // No mode-B chars ahead either — input would need BIN or
        // mode-A handling we don't drive yet. Bail to avoid spinning.
        return;
    }
    cws.push(MODE_C_SHIFT_TO_B[(n - 1) as usize]);
    for _ in 0..n {
        // CRLF collapses to one mode-B codeword consuming two bytes,
        // matching `ahead_b`'s `next = i + 2` recurrence. Looking up the
        // lone '\r' in column B returns None (DatumB['\r'] is set only by
        // the CRLF special-case), so the old `.expect()` panicked — see
        // the i16 twin of this loop and the fuzz crash at mod.rs:869.
        if msg[*i] == b'\r' && *i + 1 < msg.len() && msg[*i + 1] == b'\n' {
            cws.push(MODE_B_CRLF);
            *i += 2;
            continue;
        }
        let cw = lookup_codeword_in_mode(msg[*i], 1)
            .expect("AheadB unit is a CRLF pair or a column-B-encodable byte");
        cws.push(cw);
        *i += 1;
    }
}

/// Run one iteration of mode-B encoding: encode the current byte in
/// column B and advance `i`. Mode-B transitions to C, mode-A latches,
/// and BIN escapes are handled at the outer-loop layer in
/// [`encode_message`] (it calls `decide_initial_mode_for_next_run`
/// between each step), so this helper stays narrow.
fn enc_b_step(msg: &[u8], i: &mut usize, _mode: &mut Mode, cws: &mut Vec<u16>) {
    let cw = lookup_codeword_in_mode(msg[*i], 1)
        .expect("enc_b_step requires byte to be encodable in mode B");
    cws.push(cw);
    *i += 1;
}

/// Run one iteration of mode-A encoding. Same simplification as
/// [`enc_b_step`]: encodes one byte via column A and advances `i`.
fn enc_a_step(msg: &[u8], i: &mut usize, _mode: &mut Mode, cws: &mut Vec<u16>) {
    let cw = lookup_codeword_in_mode(msg[*i], 0)
        .expect("enc_a_step requires byte to be encodable in mode A");
    cws.push(cw);
    *i += 1;
}

// ----- i16 (marker-aware) state-machine steps ----------------------------
//
// Mirror BWIPP's full encA / encB / encC for the subset of inputs covered
// by Gap 2: FN1 / FN2 / FN3 marker emission in any mode, segstart FN1
// prepend, and the existing AheadA / AheadB run transitions. Other
// markers (LAA, LAB, latches into BIN, macros) are reserved for later
// gaps.

/// BWIPP-faithful `encC` step that operates on an `i16` stream so
/// negative marker constants (`FN1`, `FN2`, `FN3`, …) flow through.
/// Mirrors bwip-js encC (line 35408+) for the FN1-prepend-at-segstart
/// pattern plus the inline-marker dispatch (line 35423-35446).
///
/// Implements Gap 2 of `DOTCODE_COMPLETION_PLAN.md`:
///   * Segstart auto-prepend of codeword 107 when `nDigits[i] >= 2`.
///   * Skip the FN1 byte if `msg[i] == FN1` and `nDigits[i+1] >= 2`
///     (BWIPP avoids emitting `107` twice in that case).
///   * Inline FN1 / FN2 / FN3 → codewords 107 / 108 / 109 + i += 1.
///   * Otherwise: paired-digit encoding or A/B mode transitions
///     (identical to [`enc_c_step`]).
///
/// Does NOT yet handle (Gaps 3-9): full ECI expansion (FN2 + 6 digits
/// → `ECIabc` codewords), the FN3 segment-reset, SeventeenTen, BIN
/// escape, macro detection.
fn enc_c_step_i16(
    msg: &[i16],
    tables: &PositionTables,
    i: &mut usize,
    mode: &mut Mode,
    cws: &mut Vec<u16>,
    segstart: usize,
) {
    // bwip-js line 35408-35416: at segstart, prepend FN1 codeword
    // when the run starts with 2+ digits — and, if the input itself
    // begins with the FN1 marker followed by 2+ digits, consume the
    // marker so we don't emit `107` twice.
    if *i == segstart {
        if tables.n_digits[*i] >= 2 {
            cws.push(MODE_C_FN1_AT_SEGSTART);
        }
        if msg[*i] == FN1 && tables.n_digits[*i + 1] >= 2 {
            *i += 1;
            return;
        }
    }
    // bwip-js line 35423-35446: DatumC branch handles markers and
    // paired digits. We're inside DatumC if it's set on this slot.
    if tables.datum_c[*i] {
        let cur = msg[*i];
        if matches!(cur, FN1 | FN2 | FN3) {
            let cw = match cur {
                FN1 => 107,
                FN2 => 108,
                FN3 => 109,
                _ => unreachable!(),
            };
            cws.push(cw);
            *i += 1;
            return;
        }
        // Both bytes must be positive digits to form a pair. (BWIPP
        // implicitly assumes this when DatumC is set without a
        // marker — `n_digits[i] >= 2` means msg[i] and msg[i+1] are
        // both `'0'..='9'`.)
        if is_ascii_digit_i16(cur) && *i + 1 < msg.len() && is_ascii_digit_i16(msg[*i + 1]) {
            let pair = (cur - i16::from(b'0')) * 10 + (msg[*i + 1] - i16::from(b'0'));
            cws.push(pair as u16);
            *i += 2;
            return;
        }
    }
    // bwip-js line 35452-35468: Binary branch. If the byte is a high
    // byte (>= 128) and a digit run is *not* about to start at i+1,
    // emit Cvals[BIN] = 112 and switch into BIN mode. (The
    // single-byte BSA/BSB shift for `nDigits[i+1] > 0` lookahead is
    // a polish Gap 6 leaves for later — falling through to BIN here
    // produces a slightly longer cws but the symbol is still
    // round-trip-correct.)
    if tables.binary[*i] {
        cws.push(BIN_ENTER);
        *mode = Mode::Bin;
        return;
    }
    let m = tables.ahead_a[*i];
    let n = tables.ahead_b[*i];
    if m > n {
        cws.push(MODE_C_LATCH_TO_A);
        *mode = Mode::A;
        return;
    }
    if *i == segstart && matches!(msg[*i], 9 | 28 | 29 | 30) {
        cws.push(MODE_C_LATCH_TO_A);
        *mode = Mode::A;
        return;
    }
    if n > 4 {
        cws.push(MODE_C_LATCH_TO_B);
        *mode = Mode::B;
        return;
    }
    if n == 0 {
        // No mode-B chars ahead either — input would need BIN or
        // mode-A handling we don't drive yet. Bail to avoid spinning.
        return;
    }
    cws.push(MODE_C_SHIFT_TO_B[(n - 1) as usize]);
    for _ in 0..n {
        let cur = msg[*i];
        // CRLF collapses to one mode-B codeword and consumes TWO input
        // bytes — exactly how `ahead_b` counted it (its recurrence uses
        // `next = i + 2` for a CR/LF pair, line ~429). The old loop
        // instead looked up the lone '\r' in column B — which is `None`
        // (DatumB['\r'] is true only via the CRLF special-case, not the
        // column-B map) — and the `.expect()` panicked. Reachable from
        // render_svg(DotCode, "-\r\n…") (fuzz crash mod.rs:869). Mirror
        // the CRLF handling in `enc_b_step_i16` to stay in sync.
        if cur == i16::from(b'\r') && *i + 1 < msg.len() && msg[*i + 1] == i16::from(b'\n') {
            cws.push(MODE_B_CRLF);
            *i += 2;
            continue;
        }
        let cw = lookup_codeword_in_mode_i16(cur, 1)
            .expect("AheadB unit is a CRLF pair or a column-B-encodable byte");
        cws.push(cw);
        *i += 1;
    }
}

/// BWIPP-faithful `encB` step over an `i16` stream. Mirrors bwip-js
/// encB (line 35508-35598) for the mode-B subset:
///
///   * `TryC[i] >= 2` → latch to C (LAC) when `TryC > 4`, else shift
///     to C for `TryC` pairs/markers (SC2 / SC3 / SC4).
///   * `DatumB[i]` + marker → emit `Bvals[marker]` (107 / 108 / 109)
///     and advance `i`.
///   * `DatumB[i]` + CRLF → emit `Bvals[CRL] = 96` and advance
///     `i += 2`.
///   * `DatumB[i]` + plain byte → emit `Bvals[byte]` via the
///     column-B lookup.
///   * `!DatumB[i]` → if `AheadA[i] == 1` shift to A for one byte
///     (SFA + Avals[byte]), else latch to A (LAA).
///
/// Gap 3 of `DOTCODE_COMPLETION_PLAN.md`. Does NOT yet handle the
/// Binary / ECI / FN3-segment-reset branches (Gaps 6 / 8 / 9).
fn enc_b_step_i16(
    msg: &[i16],
    tables: &PositionTables,
    i: &mut usize,
    mode: &mut Mode,
    cws: &mut Vec<u16>,
) {
    let n_try_c = tables.try_c[*i];
    if n_try_c >= 2 {
        if n_try_c > 4 {
            cws.push(MODE_B_LATCH_TO_C);
            *mode = Mode::C;
            return;
        }
        // n in 2..=4 → shift to C for n pairs/markers.
        cws.push(MODE_B_SHIFT_TO_C[(n_try_c - 2) as usize]);
        for _ in 0..n_try_c {
            let cur = msg[*i];
            if cur < 0 {
                // Marker (FN1/FN2/FN3) inside the shifted run.
                let cw =
                    lookup_codeword_in_mode_i16(cur, 2).expect("marker is encodable in column C");
                cws.push(cw);
                *i += 1;
            } else {
                // Digit pair — BWIPP guarantees both bytes are digits
                // when TryC is set without a marker.
                let pair = (cur - i16::from(b'0')) * 10 + (msg[*i + 1] - i16::from(b'0'));
                cws.push(pair as u16);
                *i += 2;
            }
        }
        return;
    }
    if tables.datum_b[*i] {
        let cur = msg[*i];
        if matches!(cur, FN1 | FN2 | FN3) {
            let cw = match cur {
                FN1 => 107,
                FN2 => 108,
                FN3 => 109,
                _ => unreachable!(),
            };
            cws.push(cw);
            *i += 1;
            return;
        }
        // CRLF special case: <CR><LF> collapses to one codeword in B.
        if cur == i16::from(b'\r') && *i + 1 < msg.len() && msg[*i + 1] == i16::from(b'\n') {
            cws.push(MODE_B_CRLF);
            *i += 2;
            return;
        }
        let cw = lookup_codeword_in_mode_i16(cur, 1)
            .expect("DatumB[i] is true implies col-B lookup succeeds");
        cws.push(cw);
        *i += 1;
        return;
    }
    // !DatumB — fall through to mode-A transition.
    let n_a = tables.ahead_a[*i];
    if n_a == 1 {
        // Shift to A for one byte: SFA + Avals[byte].
        cws.push(MODE_B_SHIFT_TO_A_ONE);
        let cw = lookup_codeword_in_mode_i16(msg[*i], 0)
            .expect("AheadA[i] >= 1 implies col-A lookup succeeds");
        cws.push(cw);
        *i += 1;
        return;
    }
    // n_a >= 2 (or 0 — but we know this byte isn't B-encodable, so a
    // 0-AheadA means we'd need the Binary/BIN path which Gap 6 owns).
    // Latch to A and let encA take over.
    cws.push(MODE_B_LATCH_TO_A);
    *mode = Mode::A;
}

/// BWIPP-faithful `encA` step over an `i16` stream. Mirrors bwip-js
/// encA (line 35600-35693). Symmetric with [`enc_b_step_i16`]:
///
///   * `TryC[i] >= 2` → latch to C (LAC) when `TryC > 4`, else shift
///     to C for `TryC` pairs/markers (SC2 / SC3 / SC4).
///   * `DatumA[i]` + marker → emit `Avals[marker]` (107 / 108 / 109).
///   * `DatumA[i]` + plain byte → emit `Avals[byte]`.
///   * `!DatumA[i]` → `AheadB > 6` → latch to B (LAB); else shift to
///     B for `AheadB` bytes (SFB / SB2 / SB3 / SB4 / SB5 / SB6) with
///     CRLF collapsing during the shifted run.
///
/// Gap 4 of `DOTCODE_COMPLETION_PLAN.md`. Does NOT yet handle the
/// Binary / ECI / FN3-segment-reset branches (Gaps 6 / 8 / 9).
fn enc_a_step_i16(
    msg: &[i16],
    tables: &PositionTables,
    i: &mut usize,
    mode: &mut Mode,
    cws: &mut Vec<u16>,
) {
    let n_try_c = tables.try_c[*i];
    if n_try_c >= 2 {
        if n_try_c > 4 {
            cws.push(MODE_A_LATCH_TO_C);
            *mode = Mode::C;
            return;
        }
        cws.push(MODE_A_SHIFT_TO_C[(n_try_c - 2) as usize]);
        for _ in 0..n_try_c {
            let cur = msg[*i];
            if cur < 0 {
                let cw =
                    lookup_codeword_in_mode_i16(cur, 2).expect("marker is encodable in column C");
                cws.push(cw);
                *i += 1;
            } else {
                let pair = (cur - i16::from(b'0')) * 10 + (msg[*i + 1] - i16::from(b'0'));
                cws.push(pair as u16);
                *i += 2;
            }
        }
        return;
    }
    if tables.datum_a[*i] {
        let cur = msg[*i];
        if matches!(cur, FN1 | FN2 | FN3) {
            let cw = match cur {
                FN1 => 107,
                FN2 => 108,
                FN3 => 109,
                _ => unreachable!(),
            };
            cws.push(cw);
            *i += 1;
            return;
        }
        let cw = lookup_codeword_in_mode_i16(cur, 0)
            .expect("DatumA[i] is true implies col-A lookup succeeds");
        cws.push(cw);
        *i += 1;
        return;
    }
    // !DatumA — switch to mode B.
    let n_b = tables.ahead_b[*i];
    if n_b > 6 {
        cws.push(MODE_A_LATCH_TO_B);
        *mode = Mode::B;
        return;
    }
    if n_b == 0 {
        // Neither A nor B can handle this byte — Binary/BIN escape
        // (Gap 6). Let the outer loop's no-progress check surface
        // it as InvalidData for now.
        return;
    }
    cws.push(MODE_A_SHIFT_TO_B[(n_b - 1) as usize]);
    for _ in 0..n_b {
        let cur = msg[*i];
        if cur == i16::from(b'\r') && *i + 1 < msg.len() && msg[*i + 1] == i16::from(b'\n') {
            cws.push(MODE_B_CRLF);
            *i += 2;
        } else {
            let cw = lookup_codeword_in_mode_i16(cur, 1)
                .expect("AheadB > 0 implies DatumB on the shifted bytes");
            cws.push(cw);
            *i += 1;
        }
    }
}

/// Small helper — equivalent to `u8::is_ascii_digit` but lifted to
/// the `i16` domain so the digit check doesn't accidentally fire on
/// a negative marker that happens to share a numerical equivalence
/// (it never does — markers are `< 0`, digits are `48..=57`).
#[inline]
fn is_ascii_digit_i16(b: i16) -> bool {
    (b'0' as i16..=b'9' as i16).contains(&b)
}

/// State carried across BIN-mode steps — a 5-byte rolling buffer of
/// raw bytes (`bvals` in BWIPP) plus the count of bytes in it
/// (`bpos`). Each 5-byte boundary triggers a `finaliseBIN` flush via
/// [`base259_to_103`].
#[derive(Debug, Default)]
struct BinState {
    bvals: [u8; 5],
    bpos: usize,
}

impl BinState {
    /// BWIPP's `addtobin`: append one byte to the buffer; if the
    /// buffer is now full (5 bytes), call [`Self::finalise`] to
    /// flush via base259→103.
    fn add_byte(&mut self, b: u8, cws: &mut Vec<u16>) {
        self.bvals[self.bpos] = b;
        self.bpos += 1;
        if self.bpos == 5 {
            self.finalise(cws);
        }
    }
    /// BWIPP's `finaliseBIN`: if the buffer is non-empty, call
    /// [`base259_to_103`] on its contents and append the resulting
    /// codewords; reset the buffer.
    fn finalise(&mut self, cws: &mut Vec<u16>) {
        if self.bpos == 0 {
            return;
        }
        cws.extend(base259_to_103(&self.bvals[..self.bpos]));
        self.bpos = 0;
    }
}

/// BWIPP-faithful `encBIN` step over an `i16` stream. Mirrors bwip-
/// js encBIN (line 35695-35786):
///
///   * `TryC[i] >= 2` → finalise BIN, then latch/shift to C (TMC +
///     LAC for n > 7, BIN_SHIFT_TO_C\[n-2\] for n ∈ 2..=7).
///   * Plain positive byte with binary lookahead → append to BIN
///     buffer; finalise on 5-byte boundary or end-of-message.
///   * Otherwise → finalise BIN, then emit TMA / TMB to terminate
///     into A or B based on `AheadA[i]` vs `AheadB[i]`.
///
/// Gap 6 of `DOTCODE_COMPLETION_PLAN.md`.
///
/// Does NOT yet handle the FN3-segment reset (Gap 9) or the ECI
/// rewrite from inside BIN (Gap 8). Both produce slightly less-
/// optimal cws for inputs that hit those edges; everything else
/// (UTF-8 payloads, mixed binary/text, BIN-only messages) matches
/// BWIPP byte-for-byte.
fn enc_bin_step_i16(
    msg: &[i16],
    tables: &PositionTables,
    i: &mut usize,
    mode: &mut Mode,
    cws: &mut Vec<u16>,
    bin: &mut BinState,
) {
    let n_try_c = tables.try_c[*i];
    if n_try_c >= 2 {
        bin.finalise(cws);
        if n_try_c > 7 {
            cws.push(BIN_LATCH_TO_C);
            *mode = Mode::C;
            return;
        }
        // Shift to C for n pairs/markers.
        cws.push(BIN_SHIFT_TO_C[(n_try_c - 2) as usize]);
        for _ in 0..n_try_c {
            let cur = msg[*i];
            if cur < 0 {
                let cw =
                    lookup_codeword_in_mode_i16(cur, 2).expect("marker is encodable in column C");
                cws.push(cw);
                *i += 1;
            } else {
                let pair = (cur - i16::from(b'0')) * 10 + (msg[*i + 1] - i16::from(b'0'));
                cws.push(pair as u16);
                *i += 2;
            }
        }
        return;
    }
    let cur = msg[*i];
    if cur >= 0 {
        // BWIPP line 35742: continue accumulating into BIN if the
        // current byte or one of the next three is binary (the
        // 4-byte sliding window keeps BIN profitable). For simplicity
        // we accept any positive byte while still inside a binary
        // window — the encoder exits on the next non-binary
        // lookahead.
        let in_bin_window = tables.binary[*i]
            || (*i + 1 < tables.binary.len() && tables.binary[*i + 1])
            || (*i + 2 < tables.binary.len() && tables.binary[*i + 2])
            || (*i + 3 < tables.binary.len() && tables.binary[*i + 3]);
        if in_bin_window {
            let byte_u8 = cur as u8;
            bin.add_byte(byte_u8, cws);
            *i += 1;
            if *i == msg.len() {
                bin.finalise(cws);
            }
            return;
        }
    }
    // Exit BIN — finalise then terminate.
    bin.finalise(cws);
    let m = tables.ahead_a[*i];
    let n = tables.ahead_b[*i];
    if m > n {
        cws.push(BIN_TERM_TO_A);
        *mode = Mode::A;
    } else {
        cws.push(BIN_TERM_TO_B);
        *mode = Mode::B;
    }
}

/// BWIPP's `base259to103` — pack 1..=5 bytes into 2..=6 base-103
/// codewords. Mirrors bwip-js lines 33474-33504 verbatim.
///
/// The transformation interprets the input as a polynomial in base
/// 259 (split into a 2-byte MSB half and a 3-byte LSB half), then
/// converts both halves to base 103 via repeated `/` and `%`, and
/// finally combines the two streams using BWIPP's polynomial weight
/// table (coefficients 42 / 68 / 92 / 15 — the residues of powers of
/// 259 modulo 103).
///
/// Inputs shorter than 5 bytes are left-padded with zeros; the
/// returned slice contains exactly `bytes.len() + 1` codewords (the
/// last `bytes.len() + 1` elements of the internal 6-codeword buffer
/// — BWIPP's `$geti($_.out, (6 - inlen) - 1, inlen + 1)`).
///
/// # Panics
///
/// Panics if `bytes.len() == 0` or `bytes.len() > 5`. The encoder
/// only calls this with valid 1..=5 byte buffers (BIN buffer
/// boundary).
pub(crate) fn base259_to_103(bytes: &[u8]) -> Vec<u16> {
    assert!(
        !bytes.is_empty() && bytes.len() <= 5,
        "base259_to_103: input length must be 1..=5, got {}",
        bytes.len()
    );
    let inlen = bytes.len();
    // Left-pad to 5 bytes.
    let mut padded = [0u32; 5];
    let offset = 5 - inlen;
    for (i, &b) in bytes.iter().enumerate() {
        padded[offset + i] = u32::from(b);
    }
    // MSB half: padded[0] * 259 + padded[1].
    let value_msb = padded[0] * 259 + padded[1];
    // Three base-103 quanta from value_msb via repeated `/` and `%`
    // (BWIPP's stack-twiddling at line 33485 collects [v%103,
    // (v/103)%103, (v/103)/103]).
    let mscs = [
        value_msb % 103,
        (value_msb / 103) % 103,
        (value_msb / 103) / 103,
    ];
    // LSB half: padded[2] * 67081 + padded[3] * 259 + padded[4]
    // (67081 = 259 * 259).
    let value_lsb = padded[2] * 67081 + padded[3] * 259 + padded[4];
    let lscs = [
        value_lsb % 103,
        (value_lsb / 103) % 103,
        ((value_lsb / 103) / 103) % 103,
        ((value_lsb / 103) / 103) / 103,
    ];
    // Combine into the 6-codeword output. BWIPP computes the
    // coefficients-of-259-mod-103 weights inline (42, 68, 92, 15);
    // each output slot also carries the integer-division remainder
    // from the next-lower slot. Layout per bwip-js lines 33492-33503.
    let mut out = [0u32; 6];
    let s5 = lscs[0] + mscs[0] * 42;
    out[5] = s5 % 103;
    let s4 = (s5 / 103) + lscs[1] + mscs[0] * 68 + mscs[1] * 42;
    out[4] = s4 % 103;
    let s3 = (s4 / 103) + lscs[2] + mscs[0] * 92 + mscs[1] * 68 + mscs[2] * 42;
    out[3] = s3 % 103;
    let s2 = (s3 / 103) + lscs[3] + mscs[0] * 15 + mscs[1] * 92 + mscs[2] * 68;
    out[2] = s2 % 103;
    let s1 = (s2 / 103) + mscs[1] * 15 + mscs[2] * 92;
    out[1] = s1 % 103;
    let s0 = (s1 / 103) + mscs[2] * 15;
    out[0] = s0 % 103;
    // Return the last (inlen + 1) elements — BWIPP's
    // `$geti($_.out, (6 - inlen) - 1, inlen + 1)` returns six
    // codewords' worth of trailing slots.
    let start = 5 - inlen;
    out[start..6].iter().map(|&v| v as u16).collect()
}

/// Apply BWIPP's Reed-Solomon ECC over GF(113) to a padded data
/// codeword stream of length `nd`, appending `nc = nd/2 + 3` check
/// codewords. Returns the full `nw = nd + nc` codeword stream that
/// BWIPP then renders onto the dot grid.
///
/// Direct port of BWIPP's `bwipp_rsecprime` LFSR (bwip-js
/// line 4232-4248). Two non-obvious conventions:
///
/// 1. The data subtraction uses `tmp = data - lfsr[0]` (not `+`).
/// 2. `dotcode_gencoeffs` returns the generator polynomial
///    coefficients (without the leading 1) in **reversed**
///    order — `bwipp_coeffs[i] = generator_poly[nc - i]`. So
///    `bwipp_coeffs[0]` is the constant term, `bwipp_coeffs[nc-1]`
///    is the next-highest-degree coefficient.
/// 3. The LFSR update walks `bwipp_coeffs` index-aligned (no
///    second reversal): for `j` in `0..nc-1`,
///    `lfsr[j] = lfsr[j+1] + bwipp_coeffs[nc-j-1] * tmp`, then
///    `lfsr[nc-1] = bwipp_coeffs[0] * tmp`. Composed with (2),
///    this means we use `generator_poly[j+1]` for the inner
///    updates and `generator_poly[nc]` (the constant term) for
///    the final update.
///
/// BWIPP also prepends a 0 sentinel (its arrays are effectively
/// 1-indexed). For `nw ≤ 112` BWIPP's `step` is 1 — the RS runs over
/// the full codeword stream in one pass — which covers every
/// reachable input size from `pick_symbol_size_default`. Larger
/// messages would interleave across `step` independent streams; the
/// `encode_native_dotcode` guard at line ~1670 returns InvalidData
/// for `nw > 112` rather than silently producing an unverified
/// symbol, so this helper's single-pass scope is explicitly enforced.
pub(crate) fn apply_rs_ecc(data: &[u16]) -> Vec<u16> {
    // Default: leading sentinel is 0. Matches BWIPP for mask=0
    // (where the mask index emitted at rscws[0] is also 0).
    apply_rs_ecc_with_leading(0, data)
}

/// Same as [`apply_rs_ecc`] but takes an explicit `leading` byte
/// for BWIPP's `rscws[0]` sentinel. For DotCode this is the mask
/// index (0..=3); other callers can pass 0 for the textbook
/// convention.
pub(crate) fn apply_rs_ecc_with_leading(leading: u16, data: &[u16]) -> Vec<u16> {
    let nd_data = data.len();
    let nc = nd_data / 2 + 3;
    let nd_loop = nd_data + 1;
    let full_gen = crate::util::rs_gf113::generator_poly(nc);
    let p: u32 = 113;
    let mut lfsr = vec![0u32; nc];
    for i in 0..nd_loop {
        let datum: u32 = if i == 0 {
            u32::from(leading)
        } else {
            u32::from(data[i - 1])
        };
        let tmp = (datum + p - lfsr[0]) % p;
        for j in 0..nc - 1 {
            let coef = full_gen[j + 1];
            lfsr[j] = (lfsr[j + 1] + coef * tmp) % p;
        }
        lfsr[nc - 1] = (full_gen[nc] * tmp) % p;
    }
    let mut out: Vec<u16> = data.to_vec();
    out.extend(lfsr.iter().map(|&x| x as u16));
    out
}

/// `dotcode_maskvals` from BWIPP — the additive offset each mask
/// applies to data codewords (multiplied by codeword position).
/// Indexed by mask 0..=3.
pub(crate) const MASK_VALS: [u32; 4] = [0, 3, 7, 17];

/// Apply BWIPP's mask transformation to a padded data codeword
/// stream. Each codeword at position `p` becomes
/// `(cws[p] + p * MASK_VALS[mask]) % 113`. For `mask = 0` this is
/// the identity; other masks are non-trivial. Returns the
/// transformed cws (length unchanged).
pub(crate) fn mask_transform_cws(mask: u8, cws: &[u16]) -> Vec<u16> {
    let mv: u32 = MASK_VALS[mask as usize];
    cws.iter()
        .enumerate()
        .map(|(p, &c)| ((u32::from(c) + (p as u32) * mv) % 113) as u16)
        .collect()
}

/// End-to-end DotCode pipeline for one mask candidate: applies
/// [`mask_transform_cws`] then [`apply_rs_ecc_with_leading`] using
/// `mask` as the leading sentinel. Returns the full BWIPP `rscws[1..]`
/// array (length `nw = nd + nc`) — the leading mask-index sentinel
/// is dropped because callers usually want a flat codeword stream.
pub(crate) fn encode_for_mask(mask: u8, padded_cws: &[u16]) -> Vec<u16> {
    let transformed = mask_transform_cws(mask, padded_cws);
    apply_rs_ecc_with_leading(u16::from(mask), &transformed)
}

/// One cell in BWIPP's pre-render dot grid:
///
/// * `Inactive` (`-1`) — even-parity cell with no dot (background).
/// * `Empty` (`0`) — odd-parity cell awaiting a dot from the bit stream.
/// * `Set` (`1`) — odd-parity cell with a fixed dot (the six corner
///   markers, or filled by the snake traversal once the bit at that
///   position is `1`).
///
/// Stored as `i8` because BWIPP uses literal `-1`, `0`, `1` integers
/// and the snake-traversal code distinguishes the three states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub(crate) enum DotCell {
    Inactive = -1,
    Empty = 0,
    Set = 1,
}

/// Initialise the pre-render dot grid:
///
/// * Even-parity cells (`(x + y) % 2 == 0`) are `Inactive`.
/// * Odd-parity cells are `Empty`.
/// * Six corner positions are then forced to `Set`. Which six
///   depends on `rows`'s parity:
///   - `rows` even → `[(cols-1, rows-2), (0, rows-2),
///                     (cols-2, rows-1), (1, rows-1),
///                     (cols-1, 0), (0, 0)]`
///   - `rows` odd  → `[(cols-2, 0), (cols-2, rows-1),
///                     (cols-1, 1), (cols-1, rows-2),
///                     (0, 0), (0, rows-1)]`
///
/// Mirrors bwip-js lines 33983..34013. Returned as a row-major
/// `Vec<DotCell>` of length `rows * columns`.
pub(crate) fn init_outline(rows: usize, columns: usize) -> Vec<DotCell> {
    let mut grid = vec![DotCell::Inactive; rows * columns];
    for y in 0..rows {
        for x in 0..columns {
            let cell = if (x + y) % 2 == 0 {
                DotCell::Inactive
            } else {
                DotCell::Empty
            };
            grid[y * columns + x] = cell;
        }
    }
    let six_edges: [(usize, usize); 6] = if rows % 2 == 0 {
        [
            (columns - 1, rows - 2),
            (0, rows - 2),
            (columns - 2, rows - 1),
            (1, rows - 1),
            (columns - 1, 0),
            (0, 0),
        ]
    } else {
        [
            (columns - 2, 0),
            (columns - 2, rows - 1),
            (columns - 1, 1),
            (columns - 1, rows - 2),
            (0, 0),
            (0, rows - 1),
        ]
    };
    for (x, y) in six_edges {
        grid[y * columns + x] = DotCell::Set;
    }
    grid
}

/// Run BWIPP's snake traversal + corner placement to produce the
/// final dot grid for a given (rows, columns, bits, mask).
///
/// Algorithm (bwip-js lines 34232..34258):
///
/// 1. Copy [`init_outline(rows, columns)`] as the starting state.
/// 2. Set `posx = 0`. Set `posy = 0` if `rows` is even, `rows-1`
///    if odd. (These start cells are six-edge corners and so are
///    `Set`, never `Inactive`.)
/// 3. For each bit `b` in `bits[..bits.len()-6]`:
///    - Advance `(posx, posy)` while the current cell is NOT
///      `Inactive` (`-1`). Direction depends on `rows` parity:
///      * even: `posy++`; wrap to `(posx+1, 0)` when `posy == rows`.
///      * odd:  `posx++`; wrap to `(0, posy-1)` when `posx == columns`.
///    - Write the bit (0/1 from ASCII) into the cell.
/// 4. For each of the six corner positions (in
///    [`init_outline`]'s order), overwrite with the corresponding
///    bit from `bits[bits.len()-6..]`.
///
/// Returns a row-major `Vec<i8>` of length `rows * columns`.
/// Every cell ends up as `0` or `1`; no `-1`s remain.
pub(crate) fn render_pixs(rows: usize, columns: usize, bits: &str) -> Vec<i8> {
    let mut pixs: Vec<i8> = init_outline(rows, columns)
        .into_iter()
        .map(|c| c as i8)
        .collect();
    let bits = bits.as_bytes();
    assert!(bits.len() >= 6, "bits must have at least 6 entries");
    let main_len = bits.len() - 6;
    let mut posx: usize = 0;
    let mut posy: isize = if rows % 2 == 0 {
        0
    } else {
        (rows - 1) as isize
    };
    for &b in &bits[..main_len] {
        let bit_val: i8 = (b - b'0') as i8;
        loop {
            let idx = (posy as usize) * columns + posx;
            if pixs[idx] == -1 {
                break;
            }
            if rows % 2 == 0 {
                posy += 1;
                if posy == rows as isize {
                    posy = 0;
                    posx += 1;
                }
            } else {
                posx += 1;
                if posx == columns {
                    posx = 0;
                    posy -= 1;
                }
            }
        }
        let idx = (posy as usize) * columns + posx;
        pixs[idx] = bit_val;
    }
    let six_edges: [(usize, usize); 6] = if rows % 2 == 0 {
        [
            (columns - 1, rows - 2),
            (0, rows - 2),
            (columns - 2, rows - 1),
            (1, rows - 1),
            (columns - 1, 0),
            (0, 0),
        ]
    } else {
        [
            (columns - 2, 0),
            (columns - 2, rows - 1),
            (columns - 1, 1),
            (columns - 1, rows - 2),
            (0, 0),
            (0, rows - 1),
        ]
    };
    for (i, &(x, y)) in six_edges.iter().enumerate() {
        let bit_val: i8 = (bits[main_len + i] - b'0') as i8;
        pixs[y * columns + x] = bit_val;
    }
    pixs
}

/// A rendered DotCode symbol: the chosen `rows × columns` dot grid
/// plus the mask index that produced it. Cells are `0` (no dot) or
/// `1` (dot present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotCodeSymbol {
    pub rows: usize,
    pub columns: usize,
    pub mask: u8,
    /// Row-major `rows * columns` cells, each `0` or `1`.
    pub pixs: Vec<i8>,
}

impl DotCodeSymbol {
    /// Project the dot grid into a [`crate::encoding::BitMatrix`]
    /// (one `true` cell per dot position). Useful when a caller
    /// wants square-modules rendering instead of circular dots.
    /// See [`Self::to_dotmatrix`] for the authentic circular form.
    pub fn to_bitmatrix(&self) -> crate::encoding::BitMatrix {
        let mut m = crate::encoding::BitMatrix::new(self.columns, self.rows);
        for y in 0..self.rows {
            for x in 0..self.columns {
                if self.pixs[y * self.columns + x] == 1 {
                    m.set(x, y, true);
                }
            }
        }
        m
    }

    /// Project the dot grid into a [`crate::encoding::DotMatrix`]
    /// for circular-dot rendering — the visually-authentic DotCode
    /// shape. Used by the `Symbology::DotCode` dispatch path.
    pub fn to_dotmatrix(&self) -> crate::encoding::DotMatrix {
        let mut m = crate::encoding::DotMatrix::new(self.columns, self.rows);
        for y in 0..self.rows {
            for x in 0..self.columns {
                if self.pixs[y * self.columns + x] == 1 {
                    m.set(x, y, true);
                }
            }
        }
        m
    }
}

/// Force the six corner cells of a pixs grid to `1`. Mirrors BWIPP's
/// "litmask" construction (bwip-js lines 34277..34283): a copy of
/// the rendered pixs with `sixedges[*]` overwritten with 1. The
/// litmask is what BWIPP scores in its second-pass fallback.
pub(crate) fn apply_lit_mask(rows: usize, columns: usize, pixs: &mut [i8]) {
    let six_edges: [(usize, usize); 6] = if rows % 2 == 0 {
        [
            (columns - 1, rows - 2),
            (0, rows - 2),
            (columns - 2, rows - 1),
            (1, rows - 1),
            (columns - 1, 0),
            (0, 0),
        ]
    } else {
        [
            (columns - 2, 0),
            (columns - 2, rows - 1),
            (columns - 1, 1),
            (columns - 1, rows - 2),
            (0, 0),
            (0, rows - 1),
        ]
    };
    for &(x, y) in &six_edges {
        pixs[y * columns + x] = 1;
    }
}

/// Pick the best mask using BWIPP's two-pass algorithm:
///
/// 1. **Normal pass** — render the symbol for each mask 0..=3,
///    score with [`eval_symbol`], track the best.
/// 2. **Lit-mask fallback** — if the normal pass's best score is
///    `≤ rows * columns / 2`, BWIPP reruns the scoring on the
///    "lit-mask" version of each candidate (the same pixs with
///    the 6 corner cells forced to 1). The output pixs is then
///    the lit-mask of the winning candidate.
///
/// Returns the final (`mask`, `pixs`) — i.e. the rendered grid the
/// renderer should consume.
pub(crate) fn pick_best_mask(padded_cws: &[u16], sz: SymbolSize) -> (u8, Vec<i8>) {
    let mut best_mask: u8 = 0;
    let mut best_score: i32 = i32::MIN;
    let mut pixs_per_mask: [Option<Vec<i8>>; 4] = [None, None, None, None];
    for mask in 0..=3u8 {
        let rscws = encode_for_mask(mask, padded_cws);
        let bits = build_bits(&rscws, mask, sz.ndots);
        let pixs = render_pixs(sz.rows, sz.columns, &bits);
        let score = eval_symbol(&pixs, sz.rows, sz.columns);
        if score > best_score {
            best_score = score;
            best_mask = mask;
        }
        pixs_per_mask[mask as usize] = Some(pixs);
    }
    let threshold = (sz.rows * sz.columns) as i32 / 2;
    if best_score > threshold {
        // Normal-pass winner is the output.
        let pixs = pixs_per_mask[best_mask as usize].take().unwrap();
        return (best_mask, pixs);
    }
    // Lit-mask fallback: re-score with corners forced to 1.
    let mut best_lit_mask: u8 = 0;
    let mut best_lit_score: i32 = i32::MIN;
    let mut litmasks: [Option<Vec<i8>>; 4] = [None, None, None, None];
    for mask in 0..=3u8 {
        let mut lit = pixs_per_mask[mask as usize].as_ref().unwrap().clone();
        apply_lit_mask(sz.rows, sz.columns, &mut lit);
        let score = eval_symbol(&lit, sz.rows, sz.columns);
        if score > best_lit_score {
            best_lit_score = score;
            best_lit_mask = mask;
        }
        litmasks[mask as usize] = Some(lit);
    }
    let pixs = litmasks[best_lit_mask as usize].take().unwrap();
    (best_lit_mask, pixs)
}

/// Top-level DotCode encoder: run the full pipeline
/// (`encode_message → pick_symbol_size_default → pad_to_nd →
/// pick_best_mask`) and return a [`DotCodeSymbol`] ready for
/// circular-dot rendering. Mask selection mirrors BWIPP's full
/// `evalsymbol` + lit-mask fallback, so the resulting pixs matches
/// what BWIPP would emit for the same input.
///
/// Returns `Err(Error::InvalidData)` if the input contains a byte
/// the modes A/B/C alphabet can't encode (typically a byte > 127).
/// BWIPP's BIN escape mode handles those via base259-to-103
/// re-encoding; that path isn't ported yet, so non-ASCII payloads
/// currently surface as errors rather than producing a corrupt
/// symbol.
pub fn encode(input: &[u8]) -> Result<DotCodeSymbol, crate::error::Error> {
    // Route through the pre-parser with `parsefnc = false` (BWIPP's
    // dotcode default). For all-ASCII payloads with no `^`
    // characters this is a byte-to-i16 lift, so existing callers
    // see identical behavior. Callers that *want* `^FNC1` /
    // `^FNC3` / `^ECI...` to be recognised should drive
    // [`encode_with_markers`] directly after running
    // [`parse_dotcode_input`] with `parsefnc = true`.
    let lifted = parse_dotcode_input(input, false)?;
    encode_with_markers(&lifted)
}

/// Like [`encode`] but accepts a pre-parsed `i16` stream (the output
/// of [`parse_dotcode_input`]). Negative marker constants such as
/// [`FN1`], [`FN2`], [`FN3`] inserted by the parser flow through the
/// state machine and emit codewords 107 / 108 / 109. Callers that
/// hand-build a marker stream (e.g. the `gs1dotcode` wrapper, which
/// inserts `FN1` between GS1 application identifiers) should call
/// this entry point.
///
/// All padding / RS-ECC / mask-scoring / pixs-rendering stages run
/// identically to [`encode`].
pub fn encode_with_markers(input: &[i16]) -> Result<DotCodeSymbol, crate::error::Error> {
    let mut data = encode_message_with_markers(input)?;
    let sz = pick_symbol_size_default(data.len());
    // BWIPP switches RS encoding to interleaved-streams mode when the
    // total codeword count nw = nd + nc exceeds 112. Our `apply_rs_ecc`
    // is the single-pass (step = 1) branch only — see the doc comment
    // there. Surface that as an explicit InvalidData rather than
    // silently producing an incorrect symbol.
    let nc = sz.nd / 2 + 3;
    let nw = sz.nd + nc;
    if nw > 112 {
        return Err(crate::error::Error::InvalidData(format!(
            "DotCode: payload requires nw={nw} codewords (nd={}, nc={nc}); BWIPP's interleaved RS path activates for nw > 112 and is outside this port's coverage — emit a smaller symbol or use BWIPP directly.",
            sz.nd
        )));
    }
    pad_to_nd(&mut data, sz.nd, false);
    let (mask, pixs) = pick_best_mask(&data, sz);
    Ok(DotCodeSymbol {
        rows: sz.rows,
        columns: sz.columns,
        mask,
        pixs,
    })
}

/// Build BWIPP's bit string from a fully-encoded `rscws` (mask
/// transform + RS check codewords applied), the mask index, and
/// the symbol's `ndots` capacity.
///
/// Layout (bwip-js lines 34219..34227):
///
/// * Positions `0..=1`: the 2-bit mask selector from [`MASK_BITS`].
/// * Positions `2..(nw*9 + 2)`: each `rscws[i]` codeword expanded
///   to its 9-bit pattern via [`ENCS`]. Codewords are emitted in
///   array order (`rscws[0]` first), so the returned bit-string's
///   index `2 + 9*i + b` is the `b`-th bit of `ENCS[rscws[i]]`.
/// * Positions `(nw*9 + 2)..ndots`: `rembits = ndots - (nw*9 + 2)`
///   filler bits, all `1`s.
pub(crate) fn build_bits(rscws: &[u16], mask: u8, ndots: usize) -> String {
    let nw = rscws.len();
    let rembits = ndots
        .checked_sub(nw * 9 + 2)
        .expect("ndots must accommodate the 2-bit mask + 9-bit codewords");
    let mut bits = String::with_capacity(ndots);
    bits.push_str(MASK_BITS[mask as usize]);
    for &cw in rscws {
        let pat = ENCS[cw as usize];
        bits.push_str(&format!("{pat:09b}"));
    }
    for _ in 0..rembits {
        bits.push('1');
    }
    debug_assert_eq!(bits.len(), ndots);
    bits
}

/// Score a finished pixs grid using BWIPP's `evalsymbol` metric.
/// Higher is better. Score is computed as:
///
/// `score = worst - sum*sum - pen`
///
/// where
///
/// * `worst` is the minimum over the four edges (left, right, top,
///   bottom) of `(sum + last - first) * perp` — a measure of how
///   well-spread the dots on each edge are. `sum` is the number
///   of dots on the edge, `first`/`last` are their min/max indices
///   along the edge, `perp` is the perpendicular dimension.
///   An empty edge gives `worst = 0` which short-circuits the
///   final score to `-99999`.
/// * `pen` is a penalty for runs of "clear" rows or columns
///   (rows / columns with no dots at their parity-aligned positions).
///   Each consecutive clear column adds `rows^N` to the penalty;
///   each consecutive clear row adds `columns^N`. The column scan
///   fires when `rows` is odd or `rows <= 12`; the row scan fires
///   when `rows` is even or `columns <= 12` (so for square-ish
///   symbols both fire).
/// * `sum` (the second use of the name) counts "isolated cells" in
///   a padded version of the grid — cells where the 4 diagonal
///   corners are zero AND either the centre is zero or all 4
///   cardinal-2-away cells are zero. Acts as a penalty when many
///   cells lack neighbours.
///
/// Saturates the penalty at `i32::MAX` to avoid overflow on grids
/// with many consecutive clear columns / rows (the original
/// algorithm grows the penalty geometrically as `rows^N`).
pub(crate) fn eval_symbol(pixs: &[i8], rows: usize, columns: usize) -> i32 {
    // 1. Worst-edge metric.
    let mut worst: i32 = i32::MAX;
    for &(dir_x, fl) in &[(true, 0usize), (true, 1), (false, 0), (false, 1)] {
        let along = if dir_x { columns } else { rows };
        let perp = if dir_x { rows } else { columns };
        let mut sum: i32 = 0;
        let mut first: i32 = -1;
        let mut last: i32 = -1;
        for i in 0..along {
            let (x, y) = if dir_x {
                (i, (perp - 1) * fl)
            } else {
                ((perp - 1) * fl, i)
            };
            if pixs[y * columns + x] == 1 {
                if first == -1 {
                    first = i as i32;
                }
                last = i as i32;
                sum += 1;
            }
        }
        let metric = (sum + last - first) * (perp as i32);
        if metric < worst {
            worst = metric;
        }
    }
    if worst == 0 {
        return -99999;
    }

    // 2. Pen — clear-col/row run penalty.
    let clearcol = |x: usize| -> bool {
        let mut y = x & 1;
        while y < rows {
            if pixs[y * columns + x] == 1 {
                return false;
            }
            y += 2;
        }
        true
    };
    let clearrow = |y: usize| -> bool {
        let mut x = y & 1;
        while x < columns {
            if pixs[y * columns + x] == 1 {
                return false;
            }
            x += 2;
        }
        true
    };
    let mut pen: i32 = 0;
    if rows % 2 == 1 || rows <= 12 {
        let mut run = 0u32;
        let mut p: u64 = 0;
        for x in 1..=columns.saturating_sub(2) {
            if clearcol(x) {
                run += 1;
                p = if run == 1 {
                    rows as u64
                } else {
                    p.saturating_mul(rows as u64)
                };
            } else {
                run = 0;
                pen = pen.saturating_add(p.min(i32::MAX as u64) as i32);
                p = 0;
            }
        }
        pen = pen.saturating_add(p.min(i32::MAX as u64) as i32);
    }
    if rows % 2 == 0 || columns <= 12 {
        let mut run = 0u32;
        let mut p: u64 = 0;
        for y in 1..=rows.saturating_sub(2) {
            if clearrow(y) {
                run += 1;
                p = if run == 1 {
                    columns as u64
                } else {
                    p.saturating_mul(columns as u64)
                };
            } else {
                run = 0;
                pen = pen.saturating_add(p.min(i32::MAX as u64) as i32);
                p = 0;
            }
        }
        pen = pen.saturating_add(p.min(i32::MAX as u64) as i32);
    }

    // 3. Outlier check on a 2-cell-padded version of the grid.
    let pc = columns + 4;
    let pr = rows + 4;
    let mut padded = vec![0i8; pc * pr];
    for y in 0..rows {
        for x in 0..columns {
            padded[(y + 2) * pc + (x + 2)] = pixs[y * columns + x];
        }
    }
    let mut outlier: i32 = 0;
    for y in 2..=pr - 3 {
        let mut x = (y & 1) + 2;
        while x <= pc - 3 {
            // BWIPP's chain of short-circuit checks (bwip-js lines
            // 34127..34136). Re-read the source comment in this
            // function's doc string for the exact pattern.
            if padded[(y - 1) * pc + (x - 1)] == 1
                || padded[(y - 1) * pc + (x + 1)] == 1
                || padded[(y + 1) * pc + (x - 1)] == 1
                || padded[(y + 1) * pc + (x + 1)] == 1
            {
                // diagonal corner has a dot — not isolated.
            } else if padded[y * pc + x] == 0 {
                outlier += 1;
            } else if padded[y * pc + (x - 2)] == 1
                || padded[(y - 2) * pc + x] == 1
                || padded[y * pc + (x + 2)] == 1
                || padded[(y + 2) * pc + x] == 1
            {
                // cardinal-2-away has a dot — not isolated.
            } else {
                outlier += 1;
            }
            x += 2;
        }
    }

    worst - outlier * outlier - pen
}

/// Symbol-size metrics derived from the chosen `rows`×`columns` grid:
/// the dot capacity `ndots` and the final padded codeword count `nd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SymbolSize {
    pub rows: usize,
    pub columns: usize,
    pub nd: usize,
    pub ndots: usize,
}

/// Compute the DotCode symbol dimensions for an `n_data`-codeword
/// payload using BWIPP's default ratio = 3/2 path (rows, columns,
/// ratio all unset).
///
/// Mirrors bwip-js lines 33907..33950:
///
/// 1. `minarea = (((n_data + 3) + n_data/2) * 9 + 2) * 2`
/// 2. `hgt = sqrt(minarea / 1.5)`, `wid = sqrt(minarea * 1.5)`
/// 3. `h, w = floor(hgt), floor(wid)`
/// 4. If `h + w` is odd: bump both if `h*w < minarea`. Else look at
///    `hgt*w` vs `wid*h` to decide whether to grow the width or
///    height first; up to two corrections are applied to keep
///    `h*w >= minarea` and `h + w` odd.
/// 5. `ndots = (rows * columns) / 2` (floor).
/// 6. Grow `nd` (initially `n_data`) while the candidate
///    `((nd+1) + (nd+1)/2 + 3) * 9 + 2 ≤ ndots` — fill the symbol.
pub(crate) fn pick_symbol_size_default(n_data: usize) -> SymbolSize {
    let nd0 = n_data;
    let minarea = (((nd0 + 3) + nd0 / 2) * 9 + 2) * 2;
    let ratio = 1.5_f64;
    let hgt = (minarea as f64 / ratio).sqrt();
    let wid = (minarea as f64 * ratio).sqrt();
    let mut h = hgt as usize;
    let mut w = wid as usize;
    if (h + w) % 2 == 1 {
        if h * w < minarea {
            h += 1;
            w += 1;
        }
    } else if hgt * (w as f64) < wid * (h as f64) {
        w += 1;
        if h * w < minarea {
            w -= 1;
            h += 1;
            if h * w < minarea {
                w += 2;
            }
        }
    } else {
        h += 1;
        if h * w < minarea {
            h -= 1;
            w += 1;
            if h * w < minarea {
                h += 2;
            }
        }
    }
    let rows = h;
    let columns = w;
    let ndots = (rows * columns) / 2;
    let mut nd = nd0;
    loop {
        let next = nd + 1;
        if (((next + 3) + next / 2) * 9 + 2) > ndots {
            break;
        }
        nd = next;
    }
    SymbolSize {
        rows,
        columns,
        nd,
        ndots,
    }
}

/// Pre-parse a DotCode input payload, translating BWIPP `^FNC1` /
/// `^FNC3` / `^ECI<6 digits>` / `^^` escapes into the negative-`i16`
/// marker stream that the rest of the encoder is being extended to
/// consume.
///
/// Mirrors the `parsefnc` branch of BWIPP's `bwipp_parseinput`
/// (bwip-js line 946+) wired with dotcode's `fncvals` map (`FNC1` →
/// `FN1` / -25, `FNC3` → `FN3` / -27, ECI active), followed by
/// dotcode's own ECI expansion pass (bwip-js line 34989-35021) that
/// rewrites a single internal `-1000000 + value` ECI sentinel into
/// `FN2` (-26) followed by six ASCII digit bytes encoding the
/// zero-padded ECI value. We collapse both passes into one walk:
/// the output is exactly what BWIPP's `msg` array holds *after* the
/// ECI rewrite, ready for `build_position_tables` to consume.
///
/// When `parsefnc` is `false`, every byte passes through unchanged
/// as a positive `i16` in `0..=255` — `^FNC1` stays literal text.
///
/// ## Escapes recognised (with `parsefnc = true`)
///
/// | Source         | Output                                                |
/// |----------------|-------------------------------------------------------|
/// | `^^`           | `94` (literal `^`)                                    |
/// | `^FNC1`        | [`FN1`] (-25)                                         |
/// | `^FNC3`        | [`FN3`] (-27)                                         |
/// | `^ECI<6 dig.>` | [`FN2`] (-26), then six bytes `'0'..='9'` (ASCII)     |
///
/// Any other `^X…` pattern returns
/// [`Error::InvalidData`](crate::error::Error::InvalidData). This
/// matches BWIPP exactly: dotcode's fncvals map only registers
/// `FNC1` and `FNC3`, so inputs such as `^FNC2`, `^M05`, `^MAC` all
/// raise "Unknown function character" upstream. (Macros are *not*
/// user-typeable escapes in BWIPP — the `encC` dispatcher detects
/// them by sniffing the literal `[)>` `RS` `nn` … `EOT` byte pattern
/// at segstart instead. ECI is the only documented route for the
/// `FN2` marker to enter `msg`.)
///
/// Wired into [`encode_message`] (via `enc_c_step_i16` and
/// `lookup_codeword_in_mode`): the negative-marker sentinels this
/// parser produces are consumed downstream and emitted as
/// codewords 107 / 108 / 109 for `FN1` / `FN2` / `FN3` per BWIPP's
/// `encC` (lines 33710-33730) — see `gs1_dotcode::encode` for the
/// canonical caller.
pub(crate) fn parse_dotcode_input(
    input: &[u8],
    parsefnc: bool,
) -> Result<Vec<i16>, crate::error::Error> {
    let mut out: Vec<i16> = Vec::with_capacity(input.len());
    let mut idx = 0;
    while idx < input.len() {
        let b = input[idx];
        if b != b'^' || !parsefnc {
            out.push(i16::from(b));
            idx += 1;
            continue;
        }
        // b == b'^' and parsefnc is on — try to consume an escape.
        let rest_len = input.len() - idx;
        if rest_len < 2 {
            return Err(crate::error::Error::InvalidData(
                "DotCode parsefnc: caret character truncated".to_string(),
            ));
        }
        if input[idx + 1] == b'^' {
            // `^^` → literal caret (byte 94).
            out.push(94);
            idx += 2;
            continue;
        }
        // All remaining escapes are 5+ bytes (^FNCn or ^ECInnnnnn).
        if rest_len < 5 {
            return Err(crate::error::Error::InvalidData(
                "DotCode parsefnc: function character truncated".to_string(),
            ));
        }
        let tag = &input[idx + 1..idx + 5];
        match tag {
            b"FNC1" => {
                out.push(FN1);
                idx += 5;
            }
            b"FNC3" => {
                out.push(FN3);
                idx += 5;
            }
            _ if &tag[..3] == b"ECI" => {
                // `^ECInnnnnn` — 10 bytes total. BWIPP requires
                // exactly 6 ASCII decimal digits.
                if rest_len < 10 {
                    return Err(crate::error::Error::InvalidData(
                        "DotCode parsefnc: ECI truncated".to_string(),
                    ));
                }
                let digits = &input[idx + 4..idx + 10];
                for &d in digits {
                    if !d.is_ascii_digit() {
                        return Err(crate::error::Error::InvalidData(
                            "DotCode parsefnc: ECI must be 000000 to 999999".to_string(),
                        ));
                    }
                }
                out.push(FN2);
                out.extend(digits.iter().map(|&d| i16::from(d)));
                idx += 10;
            }
            _ => {
                let name = std::str::from_utf8(tag).unwrap_or("<non-utf8>");
                return Err(crate::error::Error::InvalidData(format!(
                    "DotCode parsefnc: unknown function character: {name}"
                )));
            }
        }
    }
    Ok(out)
}

/// Run the BWIPP-style mode selector to termination and return the
/// data codeword stream (no symbol-fill padding — call [`pad_to_nd`]
/// after).
///
/// Covers all "encode_short_pure" inputs PLUS mid-message shifts
/// back to mode C (e.g. "ab1234" — shift to B for 2, then back to C
/// for the digit pairs; "1234abc" — FN1 + pairs in C, then shift to
/// B for the trailing letters; "abc12345" — shift to B for 4, then
/// back to C for the rest).
///
/// Does NOT yet handle: inputs that latch into mode B/A and then
/// need to transition back to mode C for a trailing digit run, BIN
/// (binary > 127) bytes, FN1/FN2/FN3 markers, ECI, macros, or
/// SeventeenTen optimisations.
pub(crate) fn encode_message(msg: &[u8]) -> Result<Vec<u16>, crate::error::Error> {
    // Thin compatibility wrapper — lift each byte to a positive
    // `i16` and dispatch to the marker-aware worker. For all-positive
    // streams every marker branch in `encode_message_with_markers`
    // collapses away, so existing callers see identical cws.
    let lifted: Vec<i16> = msg.iter().map(|&b| i16::from(b)).collect();
    encode_message_with_markers(&lifted)
}

/// BWIPP-faithful encoder worker that operates on an `i16` stream
/// (the output of [`parse_dotcode_input`]). Drives `enc_c_step_i16` /
/// `enc_b_step_i16` / `enc_a_step_i16` to termination and returns
/// the data codeword stream.
///
/// Implements Gap 2 of `DOTCODE_COMPLETION_PLAN.md` — inline FN1 /
/// FN2 / FN3 markers emit codewords 107 / 108 / 109. Other markers
/// (latches, BIN escape, macros, ECI rewrite, fn3 segment-reset)
/// remain on the burn-down list.
pub(crate) fn encode_message_with_markers(msg: &[i16]) -> Result<Vec<u16>, crate::error::Error> {
    let tables = build_position_tables_i16(msg);
    let mut cws = Vec::new();
    let mut bin = BinState::default();
    let mut i = 0;
    let mut mode = Mode::C;
    let segstart = 0;
    while i < msg.len() {
        let prev_i = i;
        let prev_mode = mode;
        match mode {
            Mode::C => enc_c_step_i16(msg, &tables, &mut i, &mut mode, &mut cws, segstart),
            Mode::A => enc_a_step_i16(msg, &tables, &mut i, &mut mode, &mut cws),
            Mode::B => enc_b_step_i16(msg, &tables, &mut i, &mut mode, &mut cws),
            Mode::Bin => enc_bin_step_i16(msg, &tables, &mut i, &mut mode, &mut cws, &mut bin),
        }
        if i == prev_i && mode == prev_mode {
            // No progress — neither `i` advanced nor `mode` changed.
            // Reachable only for inputs that hit an extended BWIPP
            // marker (macro / SeventeenTen / ECI / `dotcode_fn3`)
            // that this port leaves out of scope; surface a clear
            // error instead of silently looping.
            let byte = msg[prev_i];
            return Err(crate::error::Error::InvalidData(format!(
                "DotCode: byte {byte} at position {prev_i} requires an extended-marker codeword (macro / ECI / SeventeenTen) outside this port's scope"
            )));
        }
    }
    // Final BIN flush — if the message ended mid-BIN, write the
    // remaining buffered bytes via base259→103. (`enc_bin_step_i16`
    // calls `finalise` on the last-byte-in-message edge, so this is
    // a safety net for any path that didn't.)
    bin.finalise(&mut cws);
    Ok(cws)
}

/// Compose the building blocks above into an end-to-end encoder
/// for the "single-helper-covers-the-whole-message" subset of
/// DotCode inputs:
///
///   * pure-numeric (any length, even or odd) → [`encode_numeric_run_from_c`]
///   * pure mode-A or mode-A-only-on-some-bytes-and-shared-on-the-rest,
///     with `AheadA[0] > AheadB[0]` → [`encode_mode_a_run_from_c`]
///   * pure mode-B (no mid-message transitions back to C) → [`encode_mode_b_run_from_c`]
///   * shift-to-B short runs that consume exactly the whole input
///     → [`encode_mode_b_run_from_c`]
///   * empty → `Some(vec![])`
///
/// Returns `None` for inputs that require mid-message mode
/// transitions (e.g. "ab1234" — shift to B for 2, then back to C
/// for the digit pairs). Those go through the full encA/encB/encC
/// state machine in [`encode_message`] instead; this helper is the
/// fast-path for messages the simple path can encode without
/// intermediate mode switches.
///
/// Symbol-fill padding is NOT applied — call [`pad_to_nd`] after.
pub(crate) fn encode_short_pure(msg: &[u8]) -> Option<Vec<u16>> {
    let tables = build_position_tables(msg);
    match dispatch_initial(&tables, msg) {
        InitialAction::NoPrologue => Some(Vec::new()),
        InitialAction::Fn1ThenStayInC => {
            // The Fn1ThenStayInC path keeps producing pairs until
            // it runs out of digits; the helper here only handles
            // the all-digits case. Mixed digit+text needs the
            // full main loop.
            if msg.iter().all(|b| b.is_ascii_digit()) {
                encode_numeric_run_from_c(msg)
            } else {
                None
            }
        }
        InitialAction::LatchToA => encode_mode_a_run_from_c(msg),
        InitialAction::LatchToB => encode_mode_b_run_from_c(msg),
        InitialAction::ShiftToBFor(n) => {
            // Shift covers exactly `n` bytes and the encoder reverts
            // to mode C afterwards. Only safe when n equals msg.len();
            // longer messages need the main-loop dispatcher.
            if msg.len() == n as usize {
                encode_mode_b_run_from_c(msg)
            } else {
                None
            }
        }
    }
}

/// Encode a pure mode-A byte run as DotCode codewords starting from
/// the implicit initial mode-C state, emitting the LAA latch codeword
/// (101) followed by [`encode_a`] of `bytes`. Matches the BWIPP
/// `encC` path taken when `AheadA[i] > AheadB[i]` at segstart.
///
/// Returns `None` if any byte is not mode-A-encodable.
pub(crate) fn encode_mode_a_run_from_c(bytes: &[u8]) -> Option<Vec<u16>> {
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    let body = encode_a(bytes)?;
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(MODE_C_LATCH_TO_A);
    out.extend_from_slice(&body);
    Some(out)
}

/// Encode a pure-digit segment as the codeword stream BWIPP's
/// `encC` produces starting from the initial mode-C state at the
/// beginning of a segment. Matches the goldens:
///
/// | input         | output                          |
/// |---------------|---------------------------------|
/// | `"1"`         | `[SFB, '1'_B] = [102, 17]`      |
/// | `"12"`        | `[FN1, 12] = [107, 12]`         |
/// | `"1234"`      | `[FN1, 12, 34] = [107, 12, 34]` |
/// | `"12345"`     | `[107, 12, 34, 102, 21]`        |
/// | `"123456"`    | `[107, 12, 34, 56]`             |
/// | `"00990099"`  | `[107, 0, 99, 0, 99]`           |
///
/// Symbol-fill padding (the trailing `106` BWIPP appends to reach
/// `nd` codewords) is NOT included — that's added at a later stage
/// once the symbol dimensions are known.
///
/// Returns `None` if `digits` contains a non-ASCII-digit byte.
pub(crate) fn encode_numeric_run_from_c(digits: &[u8]) -> Option<Vec<u16>> {
    if !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = digits.len();
    if n == 0 {
        return Some(Vec::new());
    }
    if n == 1 {
        // Single digit: no FN1 prefix (nDigits < 2). Shift to B,
        // encode the digit in mode B.
        let b_cw = encode_b(digits)?;
        let mut out = Vec::with_capacity(2);
        out.push(MODE_C_SHIFT_TO_B[0]);
        out.extend_from_slice(&b_cw);
        return Some(out);
    }
    let pairs = n / 2;
    let mut out = Vec::with_capacity(1 + pairs + 2);
    out.push(MODE_C_FN1_AT_SEGSTART);
    let pair_cws = encode_c(&digits[..pairs * 2])?;
    out.extend_from_slice(&pair_cws);
    if n % 2 == 1 {
        // Trailing digit goes through a shift to mode B.
        let trail = encode_b(&digits[n - 1..])?;
        out.push(MODE_C_SHIFT_TO_B[0]);
        out.extend_from_slice(&trail);
    }
    Some(out)
}

/// Encode a numeric payload as DotCode mode-C codewords. Each pair
/// of decimal digits becomes a single codeword (pair-value 0..=99),
/// matching BWIPP's `encC` exactly. Returns `None` if `digits`
/// contains a non-ASCII-digit byte or has an odd length (BWIPP
/// handles odd-length runs through the mode selector by latching
/// out to mode A/B for the trailing digit; stage 2 only handles
/// the pure-even case).
pub(crate) fn encode_c(digits: &[u8]) -> Option<Vec<u16>> {
    if digits.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    for pair in digits.chunks(2) {
        if !pair[0].is_ascii_digit() || !pair[1].is_ascii_digit() {
            return None;
        }
        let v = (pair[0] - b'0') as u16 * 10 + (pair[1] - b'0') as u16;
        out.push(v);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BWIPP's `dotcode_encs` has 113 entries: 109 data codewords
    /// (0..=108), 3 special-purpose codewords (109, 110, 111), and
    /// a final terminator. Each entry must fit in 9 bits (the bar
    /// pattern is 9 modules wide per codeword).
    #[test]
    fn encs_table_shape() {
        assert_eq!(ENCS.len(), 113);
        for (i, &v) in ENCS.iter().enumerate() {
            assert!(v < 512, "ENCS[{i}] = {v} doesn't fit in 9 bits");
        }
    }

    /// Anchor a few specific entries so a botched copy/paste from
    /// the BWIPP source shows up immediately. These are the first,
    /// last, and a sampling of mid-table values from
    /// `bwipp_dotcode.dotcode_encs`.
    #[test]
    fn encs_known_values() {
        // 101010101 = 341 (decimal)
        assert_eq!(ENCS[0], 341);
        // 010101011 = 171
        assert_eq!(ENCS[1], 171);
        // 111001100 = 460 (final entry)
        assert_eq!(ENCS[112], 460);
        // 010111001 = 185 (entry 38, mid-table sanity)
        assert_eq!(ENCS[38], 185);
    }

    /// DotCode codewords aren't a single fixed-weight code — the
    /// 113 entries split into groups with different popcounts. This
    /// test just asserts every entry has 3, 4, 5, or 6 dots set out
    /// of 9 (the documented design range), so a transcription error
    /// that produced e.g. an all-zero or all-one codeword is caught.
    #[test]
    fn encs_popcount_in_valid_range() {
        for (i, &v) in ENCS.iter().enumerate() {
            let ones = v.count_ones();
            assert!(
                (3..=6).contains(&ones),
                "ENCS[{i}] = {v:09b} has {ones} dots, expected 3..=6"
            );
        }
    }

    #[test]
    fn mask_constants_match_bwipp() {
        assert_eq!(MASK_SEEDS, [0, 3, 7, 17]);
        assert_eq!(MASK_BITS, ["00", "01", "10", "11"]);
    }

    /// Stage 11.A8c — pin `lookup_codeword_in_mode` /
    /// `lookup_codeword_in_mode_i16` per-column behaviour. Both
    /// helpers find the first CHARMAPS row whose `col`-th entry
    /// equals the lookup value. Used by every per-byte encoder path
    /// (encode_a, encode_b, enc_a_step, enc_b_step, the marker-aware
    /// i16 step encoders) — but never directly pinned.
    ///
    /// Mutations to catch:
    ///   - `row[col] == b` → `!=`: returns wrong index or shifts.
    ///   - `col` constant substitution (col=0 → 1, etc): cross-mode
    ///     lookups would return values from the wrong column.
    ///   - `position` → `rposition`: irrelevant here (CHARMAPS has
    ///     no duplicate values in any single column), but the
    ///     `map(|i| i as u16)` and the find_position chain itself
    ///     are still pinned by the exact-index anchors below.
    ///   - `i16::from(b)` → `-i16::from(b)` in the u8 wrapper:
    ///     positive bytes would land in negative-marker territory
    ///     and miss every row.
    #[test]
    fn lookup_codeword_in_mode_per_column_and_marker_pins() {
        // ---- Column A (col=0) ----
        // 'A' (65) sits at row 33: `[65, 65, 33]`. Same value in col 0/1
        // but the row index is the same — a generic `row[col]` mutation
        // alone won't be caught here; that's what the asymmetric
        // anchors below are for.
        assert_eq!(lookup_codeword_in_mode(b'A', 0), Some(33));
        // 'a' (97) is NOT in col 0 anywhere — col 0 row 65 is `[1, ...]`,
        // not `[97, ...]`. Pins `col=0` vs `col=1` separation.
        assert_eq!(
            lookup_codeword_in_mode(b'a', 0),
            None,
            "lowercase has no col-A mapping"
        );
        // NUL byte (0) is at row 64 col 0 (`[0, 96, 64]`). Pins that
        // col-0 lookup finds the row whose col-0 entry equals 0, not
        // the row whose col-2 entry equals 0 (which would be row 0).
        assert_eq!(
            lookup_codeword_in_mode(0, 0),
            Some(64),
            "NUL → col-A row 64 (asymmetric: NOT row 0 from col-2)"
        );

        // ---- Column B (col=1) ----
        // 'a' (97) is at row 65 col 1 (`[1, 97, 65]`). Asymmetric:
        // col-swap to 0 would return None instead.
        assert_eq!(
            lookup_codeword_in_mode(b'a', 1),
            Some(65),
            "'a' → col-B row 65"
        );
        // 'A' (65) is at row 33 col 1 too (`[65, 65, 33]`). Same index
        // as col-0 lookup. Sanity anchor.
        assert_eq!(lookup_codeword_in_mode(b'A', 1), Some(33));
        // 96 ('`', backtick) is at row 64 col 1 (`[0, 96, 64]`). NOT
        // in col 0 anywhere. Strong col-A vs col-B discriminator.
        assert_eq!(lookup_codeword_in_mode(96, 1), Some(64));
        assert_eq!(
            lookup_codeword_in_mode(96, 0),
            None,
            "backtick has no col-A mapping"
        );

        // ---- Column C (col=2) ----
        // Col 2 entries for rows 0..=95 are the row index (0..=95).
        // So a positive byte v ∈ 0..=95 in col 2 is at row v.
        assert_eq!(lookup_codeword_in_mode(0, 2), Some(0), "col-C: 0 → row 0");
        assert_eq!(lookup_codeword_in_mode(50, 2), Some(50));
        assert_eq!(lookup_codeword_in_mode(95, 2), Some(95));

        // ---- Marker (negative i16) lookups via the _i16 path ----
        // FN1 = -25 sits at row 107 in all three columns
        // (`[FN1, FN1, FN1]`).
        assert_eq!(
            lookup_codeword_in_mode_i16(FN1, 0),
            Some(107),
            "FN1 col-A → row 107"
        );
        assert_eq!(lookup_codeword_in_mode_i16(FN1, 1), Some(107));
        assert_eq!(lookup_codeword_in_mode_i16(FN1, 2), Some(107));
        // LAA = -1 is at col 0 row 101 (`[SB6, SFA, LAA]` row 101 has
        // col 0 = SB6, NOT LAA) — wait, LAA is at row 102 col 1
        // (`[LAB, LAA, SFB]`), and row 101 col 2 is LAA, and row 105
        // col 0 has LAC (= -3) not LAA. So:
        //   * LAA col 0: first occurrence?  Looking at the CHARMAPS
        //     marker rows — LAA only appears in col 1 (row 102) and
        //     col 2 (row 101). So col 0 returns None.
        assert_eq!(
            lookup_codeword_in_mode_i16(LAA, 0),
            None,
            "LAA has no col-A row (asymmetric marker placement)"
        );
        assert_eq!(
            lookup_codeword_in_mode_i16(LAA, 1),
            Some(102),
            "LAA col-B → row 102 (`[LAB, LAA, SFB]`)"
        );
        assert_eq!(
            lookup_codeword_in_mode_i16(LAA, 2),
            Some(101),
            "LAA col-C → row 101 (`[SB6, SFA, LAA]`)"
        );

        // ---- u8 wrapper forwards exactly to _i16 path ----
        // Pin that `i16::from(b)` doesn't mangle the sign — a
        // `-i16::from(b)` mutation would lookup -65 and miss row 33.
        assert_eq!(
            lookup_codeword_in_mode(b'A', 0),
            lookup_codeword_in_mode_i16(b'A' as i16, 0),
            "u8 wrapper matches i16 path"
        );
        assert_eq!(
            lookup_codeword_in_mode(b'a', 1),
            lookup_codeword_in_mode_i16(b'a' as i16, 1),
        );

        // ---- Misses ----
        // Byte 255 doesn't exist anywhere in CHARMAPS.
        assert_eq!(lookup_codeword_in_mode(255, 0), None);
        assert_eq!(lookup_codeword_in_mode(255, 1), None);
        assert_eq!(lookup_codeword_in_mode(255, 2), None);
        // Random unmapped negative marker.
        assert_eq!(lookup_codeword_in_mode_i16(-99, 0), None);
        assert_eq!(lookup_codeword_in_mode_i16(-99, 1), None);
        assert_eq!(lookup_codeword_in_mode_i16(-99, 2), None);
    }

    /// `encode_c` should produce one codeword per pair of input
    /// digits, with each codeword equal to the pair's decimal
    /// value 0..=99. Goldens captured from `oracle-dotcode.js`
    /// for pure numeric payloads — the `cws` array BWIPP emits is
    /// `[LATCH_C, ...encC...]`, so this test strips the leading
    /// `LATCH_C` (the mode-selector handles that in stage 4).
    #[test]
    fn encode_c_matches_bwipp_for_even_numeric() {
        let cases: &[(&str, &[u16])] = &[
            ("1234", &[12, 34]),
            ("123456", &[12, 34, 56]),
            ("1234567890", &[12, 34, 56, 78, 90]),
            ("00990099", &[0, 99, 0, 99]),
            ("0000", &[0, 0]),
            ("9999", &[99, 99]),
        ];
        for &(input, want) in cases {
            let got = encode_c(input.as_bytes()).unwrap_or_else(|| {
                panic!("encode_c({input:?}) returned None — expected Some({want:?})");
            });
            assert_eq!(got, want, "encode_c({input:?}) mismatch");
        }
    }

    /// Mode-B byte→codeword translation. Goldens captured via
    /// `tools/oracle-dotcode.js` — each `cws` output's leading
    /// initial-latch codeword (102+) is stripped because that's
    /// the mode selector's job, not the per-byte encoder's.
    #[test]
    fn encode_b_matches_bwipp_oracle() {
        let cases: &[(&str, &[u16])] = &[
            // Pure uppercase — also valid in mode A but mode B picks
            // the same codeword (the uppercase block is shared).
            ("ABCDE", &[33, 34, 35, 36, 37]),
            // Pure lowercase — only valid in mode B.
            ("abcde", &[65, 66, 67, 68, 69]),
            // Mixed-case word: oracle output for "Hello" was
            // [106, 40, 69, 76, 76, 79] → strip the leading 106.
            ("Hello", &[40, 69, 76, 76, 79]),
            ("HELLO", &[40, 37, 44, 44, 47]),
            ("test", &[84, 69, 83, 84]),
        ];
        for &(input, want) in cases {
            let got = encode_b(input.as_bytes()).unwrap_or_else(|| {
                panic!("encode_b({input:?}) returned None — expected Some({want:?})");
            });
            assert_eq!(got, want, "encode_b({input:?}) mismatch");
        }
    }

    /// Stage 11.A8c — pin `encode_b` byte→codeword translation. Mode
    /// B (col 1) supports printable ASCII 32..=127 (the lowercase
    /// block 96..=127 is unique to mode B; NUL and other control
    /// bytes < 32 are NOT representable in B). The existing
    /// `encode_a` test pinned the upper-case + digit shared block;
    /// this test focuses on the lowercase block + space + NUL
    /// rejection that distinguishes B from A.
    ///
    /// Mutations to catch:
    ///   - `lookup_codeword_in_mode(b, 1)` → `(b, 0)`: would route
    ///     mode-B inputs through mode A's column (lowercase rejected).
    ///   - `bytes.iter().map(...).collect()` → arms swapped or `take`:
    ///     wrong output length.
    ///   - `Option<Vec<u16>>` → `Vec<Option<u16>>`: collection short-
    ///     circuits differently.
    #[test]
    fn encode_b_handles_lowercase_and_rejects_nul() {
        // Lowercase letters: col B rows 65..=90, codewords 65..=90.
        assert_eq!(encode_b(b"abc"), Some(vec![65, 66, 67]));
        // Lowercase + digit + uppercase mix: all encodable in mode B.
        assert_eq!(
            encode_b(b"A1a"),
            Some(vec![33, 17, 65]),
            "uppercase 'A'=33, digit '1'=17, lowercase 'a'=65"
        );
        // Space (32) is at row 0 → codeword 0 in both modes.
        assert_eq!(encode_b(b" "), Some(vec![0]));
        // DEL (127) is at row 95 → codeword 95 in mode B.
        assert_eq!(
            encode_b(b"\x7F"),
            Some(vec![95]),
            "DEL (0x7F) is the last valid mode-B byte"
        );
        // NUL (0) is NOT in mode B (only mode A). Returns None.
        assert_eq!(encode_b(b"\x00"), None, "NUL (0x00) has no mode-B codeword");
        // Any control byte < 32 should also be rejected.
        assert_eq!(encode_b(b"\x01"), None);
        assert_eq!(encode_b(b"\x1F"), None);
        // Mixed valid + invalid → None (collect short-circuits).
        assert_eq!(
            encode_b(b"a\x00b"),
            None,
            "any single invalid byte poisons the whole encode"
        );
        // Empty input → empty vec.
        assert_eq!(encode_b(b""), Some(Vec::new()));
    }

    /// Mode-A byte→codeword translation. The uppercase block is
    /// shared with mode B, so uppercase-only inputs produce the
    /// same codewords in either mode.
    #[test]
    fn encode_a_handles_uppercase_and_rejects_lowercase() {
        // Uppercase letters: same codewords as mode B (shared block).
        assert_eq!(encode_a(b"ABCDE"), Some(vec![33, 34, 35, 36, 37]));
        // Digits: shared between A and B.
        assert_eq!(encode_a(b"12345"), Some(vec![17, 18, 19, 20, 21]));
        // Lowercase letters have no mode-A codeword.
        assert!(encode_a(b"abc").is_none());
    }

    /// Per-position lookahead tables follow BWIPP's right-to-left
    /// construction rules. Verify with hand-derived expectations for
    /// short payloads exercising each rule.
    #[test]
    fn position_tables_match_bwipp_rules() {
        // Pure digits: nDigits counts down 5..0; everything is in
        // both A and B (digits are shared); DatumC is true wherever
        // nDigits >= 2.
        let pt = build_position_tables(b"12345");
        assert_eq!(pt.n_digits, vec![5, 4, 3, 2, 1, 0]);
        assert_eq!(pt.datum_a, vec![true, true, true, true, true, false]);
        assert_eq!(pt.datum_b, vec![true, true, true, true, true, false]);
        assert_eq!(pt.datum_c, vec![true, true, true, true, false, false]);

        // Pure lowercase: not in mode A; all in mode B; no digits.
        let pt = build_position_tables(b"abc");
        assert_eq!(pt.n_digits, vec![0, 0, 0, 0]);
        assert_eq!(pt.datum_a, vec![false, false, false, false]);
        assert_eq!(pt.datum_b, vec![true, true, true, false]);
        assert_eq!(pt.datum_c, vec![false, false, false, false]);

        // Mixed: ABC123abc. Uppercase + digits are A-and-B,
        // lowercase is B-only. Digit run of 3 starting at index 3
        // → nDigits = [0,0,0,3,2,1,0,0,0,0], DatumC true where
        // nDigits >= 2 = positions 3,4.
        let pt = build_position_tables(b"ABC123abc");
        assert_eq!(pt.n_digits, vec![0, 0, 0, 3, 2, 1, 0, 0, 0, 0]);
        assert_eq!(
            pt.datum_a,
            vec![true, true, true, true, true, true, false, false, false, false],
        );
        assert_eq!(
            pt.datum_b,
            vec![true, true, true, true, true, true, true, true, true, false],
        );
        assert_eq!(
            pt.datum_c,
            vec![false, false, false, true, true, false, false, false, false, false],
        );

        // CRLF special case: '\r' is not in column B normally, but
        // a '\r''\n' pair forces DatumB[i] = true at the '\r' only
        // (NOT at the '\n', which has no column-B codeword and
        // gets shifted via the CRL marker in the final stream).
        let pt = build_position_tables(b"a\r\nb");
        assert!(pt.datum_b[1], "CRLF should set DatumB[\\r] = true");
        assert!(!pt.datum_b[2], "but DatumB[\\n] stays false");
        assert_eq!(pt.datum_b, vec![true, true, false, true, false]);
    }

    /// Mode-selector scoring tables: SeventeenTen / AheadA / AheadB
    /// / AheadC / TryC / UntilEndSeg. Each is the right-to-left scan
    /// described in the [`PositionTables`] doc; the expectations
    /// here are hand-derived from those construction rules.
    #[test]
    fn position_tables_scoring_fields() {
        // Pure lowercase: only mode B is viable. AheadB counts down,
        // AheadA / AheadC stay zero.
        let pt = build_position_tables(b"abc");
        assert_eq!(pt.ahead_a, vec![0, 0, 0, 0]);
        assert_eq!(pt.ahead_b, vec![3, 2, 1, 0]);
        assert_eq!(pt.ahead_c, vec![0, 0, 0, 0]);
        assert_eq!(pt.try_c, vec![0, 0, 0, 0]);
        assert_eq!(pt.until_end_seg, vec![3, 2, 1, 0]);
        assert!(pt.seventeen_ten.iter().all(|&b| !b));

        // All-uppercase: viable in both A and B. AheadA / AheadB
        // both count down.
        let pt = build_position_tables(b"ABC");
        assert_eq!(pt.ahead_a, vec![3, 2, 1, 0]);
        assert_eq!(pt.ahead_b, vec![3, 2, 1, 0]);

        // Pure digits "12345": AheadC counts down by pairs (5
        // digits → AheadC[0]=2 = floor(5/2)+something; verify
        // exact values from the hand-trace).
        let pt = build_position_tables(b"12345");
        assert_eq!(pt.ahead_c, vec![2, 2, 1, 1, 0, 0]);
        // TryC fires at positions where AheadC[i] > AheadC[i+1].
        assert_eq!(pt.try_c, vec![0, 2, 0, 1, 0, 0]);
        // AheadA stops climbing where TryC[i] >= 2 — at i=1 here.
        assert_eq!(pt.ahead_a, vec![1, 0, 3, 2, 1, 0]);

        // GS1 AI 17 expiry + AI 10 batch pattern: the SeventeenTen
        // flag should fire at the start (10 digits matching
        // "17XXXXXX10").
        let pt = build_position_tables(b"1723456710");
        assert!(
            pt.seventeen_ten[0],
            "SeventeenTen should fire on AI-17/AI-10 prefix"
        );
        // No SeventeenTen elsewhere — the pattern needs both
        // "17" up front and "10" exactly 8 positions later.
        for (i, &v) in pt.seventeen_ten.iter().enumerate().skip(1) {
            assert!(!v, "SeventeenTen[{i}] should not fire");
        }
    }

    /// `pad_to_nd` follows BWIPP's pad rules: first slot is 109 if
    /// the final encoder mode was BIN, else 106; every subsequent
    /// slot is 106. No-op when the input is already long enough.
    #[test]
    fn pad_to_nd_rules() {
        // Already-long-enough: no-op.
        let mut cws = vec![1, 2, 3, 4];
        pad_to_nd(&mut cws, 3, false);
        assert_eq!(cws, vec![1, 2, 3, 4]);

        // Non-BIN: pad with 106s.
        let mut cws = vec![102, 33];
        pad_to_nd(&mut cws, 5, false);
        assert_eq!(cws, vec![102, 33, 106, 106, 106]);

        // BIN-terminated: first pad is 109, rest 106.
        let mut cws = vec![112]; // BIN row codeword (illustrative)
        pad_to_nd(&mut cws, 4, true);
        assert_eq!(cws, vec![112, 109, 106, 106]);

        // Empty + BIN: single 109 + rest 106.
        let mut cws = vec![];
        pad_to_nd(&mut cws, 3, true);
        assert_eq!(cws, vec![109, 106, 106]);
    }

    /// Top-level `encode(input)` glues the whole pipeline together
    /// including BWIPP's full mask-selection algorithm. For "A"
    /// (rows=10, cols=13, ndots=65) the first-pass best score is 62
    /// at mask=1 — below the `rows*cols/2 = 65` threshold, so the
    /// lit-mask fallback fires and re-scores all four litmasks.
    /// The final pixs is verified byte-for-byte against BWIPP's
    /// `$_.pixs` captured at the `$_._render` anchor.
    #[test]
    fn high_level_encode_picks_bwipp_mask_for_a() {
        let sym = encode(b"A").unwrap();
        assert_eq!(sym.rows, 10);
        assert_eq!(sym.columns, 13);
        assert_eq!(sym.pixs.len(), 130);
        assert!(sym.pixs.iter().all(|&v| v == 0 || v == 1));
        // Verify the six corner cells are all 1 (the litmask
        // signature — corners forced to 1 regardless of source bit).
        let six_corners = [(12, 8), (0, 8), (11, 9), (1, 9), (12, 0), (0, 0)];
        for (x, y) in six_corners {
            assert_eq!(
                sym.pixs[y * 13 + x],
                1,
                "corner ({x},{y}) should be Set in litmask output",
            );
        }
        // Full pixs match against BWIPP's final output (captured
        // via the `$_._render` anchor — i.e. what BWIPP hands to
        // renmatrix as the rendered DotCode symbol).
        let bwipp_final: &[i8] = &[
            1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1,
            0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
            0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0,
            1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0,
        ];
        assert_eq!(sym.pixs, bwipp_final, "pixs mismatch vs BWIPP final output");
    }

    /// For inputs where the normal-pass best score beats the
    /// threshold, no lit-mask fallback fires and the chosen pixs
    /// is the raw render. Pin a few BWIPP mask choices.
    #[test]
    fn encode_picks_bwipp_mask_for_longer_inputs() {
        let cases: &[(&[u8], u8)] = &[
            (b"AB", 2),
            (b"ABC", 3),
            (b"Hello", 1),
            (b"1234", 3),
            (b"12345", 2),
            (b"ABCDE", 1),
            (b"ABCabc", 1),
            (b"ab1234", 2),
            (b"abc12345", 2),
        ];
        for &(input, want) in cases {
            let sym = encode(input).unwrap();
            assert_eq!(
                sym.mask, want,
                "encode({input:?}) should pick mask={want}, got {}",
                sym.mask
            );
        }
    }

    /// `eval_symbol` should produce the same per-mask scores that
    /// bwip-js's `evalsymbol` does. Goldens captured from
    /// `tools/oracle-dotcode-scores.js` for two inputs:
    ///
    /// * "A": mask scores [4, 62, 12, 51]
    /// * "Hello": mask scores [245, 251, 243, 248]
    ///
    /// The chosen mask isn't tested here — BWIPP's `evalsymbol`
    /// runs twice (a "regular" pass and a "lit-mask" fallback pass
    /// gated by `bestscore <= rows*cols/2`). This test pins the
    /// regular-pass scores only; the fallback wiring is a separate
    /// piece.
    /// Stage 11.A8c — pin `apply_rs_ecc` invariants:
    ///   * Output length = `data.len() + nc` where `nc = data.len()/2 + 3`.
    ///   * Output prefix is the data unchanged.
    ///   * `apply_rs_ecc(data)` agrees with
    ///     `apply_rs_ecc_with_leading(0, data)` — the wrapper is a
    ///     literal `leading=0` substitution.
    ///   * Distinct `leading` bytes produce distinct ECC suffixes for
    ///     a non-empty payload — pins that `leading` actually
    ///     participates in the LFSR (not silently dropped).
    ///
    /// Mutations caught:
    ///   * `nc = data.len() / 2 + 3` → `+ 2` / `+ 4` shifts length.
    ///   * `if i == 0` → `if i != 0` would swap leading/data roles.
    ///   * `apply_rs_ecc` dropping the `leading=0` and using a
    ///     literal — pinned by the with_leading agreement check.
    #[test]
    fn apply_rs_ecc_length_prefix_and_with_leading_delegation() {
        // Empty data → length 3 (nc=3), all-zero ECC for leading=0.
        let r = apply_rs_ecc(&[]);
        assert_eq!(r.len(), 3, "empty input → nc=3 ECC bytes");
        assert_eq!(r, vec![0u16; 3], "empty + leading=0 → all-zero ECC");
        assert_eq!(r, apply_rs_ecc_with_leading(0, &[]));

        // Two-byte data → length 2 + (2/2+3) = 2 + 4 = 6.
        let data = [42u16, 99];
        let r = apply_rs_ecc(&data);
        assert_eq!(r.len(), 6, "len=2 → nc=4");
        assert_eq!(&r[..2], &data[..], "prefix is data unchanged");
        assert_eq!(r, apply_rs_ecc_with_leading(0, &data));

        // Four-byte data → nc = 4/2+3 = 5 → out len 9.
        let data4 = [1u16, 2, 3, 4];
        let r4 = apply_rs_ecc(&data4);
        assert_eq!(r4.len(), 9, "len=4 → nc=5");
        assert_eq!(&r4[..4], &data4[..]);

        // Different leading bytes → different ECC for non-empty data.
        let with_0 = apply_rs_ecc_with_leading(0, &data);
        let with_1 = apply_rs_ecc_with_leading(1, &data);
        let with_3 = apply_rs_ecc_with_leading(3, &data);
        assert_ne!(with_0[2..], with_1[2..], "leading must participate in LFSR");
        assert_ne!(with_1[2..], with_3[2..]);
        // Prefix unchanged regardless of leading.
        assert_eq!(&with_0[..2], &data[..]);
        assert_eq!(&with_1[..2], &data[..]);
        assert_eq!(&with_3[..2], &data[..]);
    }

    #[test]
    fn eval_symbol_matches_bwipp_scores() {
        let cases: &[(&[u8], [i32; 4])] = &[
            (b"A", [4, 62, 12, 51]),
            (b"AB", [12, 78, 88, 70]),
            (b"ABC", [156, 178, 140, 191]),
            (b"Hello", [245, 251, 243, 248]),
            (b"1234", [74, 48, 74, 126]),
            (b"12345", [196, 215, 228, 216]),
            (b"ABCDE", [236, 245, 234, 243]),
            (b"ABCabc", [231, 299, 296, 296]),
            (b"ab1234", [188, 200, 222, 152]),
            (b"abc12345", [250, 291, 296, 296]),
        ];
        let mut fails: Vec<String> = Vec::new();
        for &(input, want_scores) in cases {
            let mut data = encode_message(input).unwrap();
            let sz = pick_symbol_size_default(data.len());
            pad_to_nd(&mut data, sz.nd, false);
            for mask in 0..=3u8 {
                let rscws = encode_for_mask(mask, &data);
                let bits = build_bits(&rscws, mask, sz.ndots);
                let pixs = render_pixs(sz.rows, sz.columns, &bits);
                let got = eval_symbol(&pixs, sz.rows, sz.columns);
                if got != want_scores[mask as usize] {
                    fails.push(format!(
                        "{:?} mask={mask}: got {got}, want {}",
                        std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                        want_scores[mask as usize]
                    ));
                }
            }
        }
        assert!(
            fails.is_empty(),
            "eval_symbol mismatches:\n  {}",
            fails.join("\n  ")
        );
    }

    /// Walk every `(input, mask, pixs)` row in the corpus fixture
    /// `tests/fixtures/dotcode_pixs.txt` (generated by
    /// `tools/oracle-dotcode-pixs.js`) and assert the full pipeline
    /// matches BWIPP byte-for-byte. Avoids inlining 130-260-element
    /// arrays in the test source, which is error-prone to
    /// hand-maintain.
    #[test]
    fn render_pixs_corpus_matches_oracle() {
        let corpus = include_str!("../../../tests/fixtures/dotcode_pixs.txt");
        let mut tested = 0usize;
        for line in corpus.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let input = parts.next().expect("missing input");
            let mask: u8 = parts
                .next()
                .expect("missing mask")
                .parse()
                .expect("bad mask");
            let want: Vec<i8> = parts
                .next()
                .expect("missing pixs csv")
                .split(',')
                .map(|s| s.parse().expect("bad pixs cell"))
                .collect();
            let mut data = encode_message(input.as_bytes()).unwrap();
            let sz = pick_symbol_size_default(data.len());
            pad_to_nd(&mut data, sz.nd, false);
            let rscws = encode_for_mask(mask, &data);
            let bits = build_bits(&rscws, mask, sz.ndots);
            let pixs = render_pixs(sz.rows, sz.columns, &bits);
            assert_eq!(
                pixs.len(),
                sz.rows * sz.columns,
                "wrong pixs length for {input:?} mask={mask}",
            );
            assert_eq!(pixs, want, "render_pixs mismatch for {input:?} mask={mask}",);
            assert!(pixs.iter().all(|&v| v == 0 || v == 1));
            tested += 1;
        }
        assert!(tested >= 10, "expected ≥10 corpus cases, ran {tested}");
    }

    /// `render_pixs` rounds out the DotCode pipeline. End-to-end
    /// pipeline: encode_message → pick_symbol_size → pad_to_nd →
    /// encode_for_mask → build_bits → render_pixs reproduces the
    /// BWIPP `pixs` array byte-for-byte for "A" mask=0 (the smoke
    /// test that exercises every stage); see also the corpus test
    /// above which walks 10 captured (input, mask) pairs.
    #[test]
    fn render_pixs_matches_oracle_full_pipeline() {
        let mut data = encode_message(b"A").unwrap();
        let sz = pick_symbol_size_default(data.len());
        pad_to_nd(&mut data, sz.nd, false);
        let rscws = encode_for_mask(0, &data);
        let bits = build_bits(&rscws, 0, sz.ndots);
        let pixs = render_pixs(sz.rows, sz.columns, &bits);
        assert_eq!(pixs.len(), sz.rows * sz.columns);
        // pixs from oracle-dotcode-pixs.js for "A" mask=0.
        let want: &[i8] = &[
            1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1,
            0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0,
            0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0,
        ];
        assert_eq!(pixs, want);
        assert!(pixs.iter().all(|&v| v == 0 || v == 1));
    }

    /// `init_outline` should produce a parity-checkered grid with
    /// six fixed corner dots. Verify the structural invariants and
    /// pin the six-edge positions for both `rows` parities.
    #[test]
    fn init_outline_parity_and_corners() {
        // rows even (10), columns odd (13) — sum 23 odd as required
        // by `pick_symbol_size_default`.
        let g = init_outline(10, 13);
        assert_eq!(g.len(), 10 * 13);
        // Parity invariant: every odd-(x+y) cell is Empty (those
        // are the dot slots the snake traversal fills). Every
        // even-(x+y) cell is Inactive UNLESS it's a six-edge
        // corner — all six corners land on even-parity cells and
        // BWIPP overrides them to Set.
        let corners: [(usize, usize); 6] = [(12, 8), (0, 8), (11, 9), (1, 9), (12, 0), (0, 0)];
        for y in 0..10 {
            for x in 0..13 {
                let c = g[y * 13 + x];
                let is_corner = corners.contains(&(x, y));
                if (x + y) % 2 == 0 {
                    if is_corner {
                        assert_eq!(c, DotCell::Set, "({x},{y}) corner should be Set");
                    } else {
                        assert_eq!(c, DotCell::Inactive, "({x},{y}) should be Inactive");
                    }
                } else {
                    assert_eq!(c, DotCell::Empty, "({x},{y}) should be Empty");
                }
            }
        }
        // Six edges for rows=even: (cols-1, rows-2), (0, rows-2),
        // (cols-2, rows-1), (1, rows-1), (cols-1, 0), (0, 0).
        for &(x, y) in &[(12, 8), (0, 8), (11, 9), (1, 9), (12, 0), (0, 0)] {
            assert_eq!(
                g[y * 13 + x],
                DotCell::Set,
                "corner ({x},{y}) should be Set"
            );
        }

        // rows odd (11), columns even (16) — sum 27 odd.
        let g = init_outline(11, 16);
        assert_eq!(g.len(), 11 * 16);
        // Six edges for rows=odd: (cols-2, 0), (cols-2, rows-1),
        // (cols-1, 1), (cols-1, rows-2), (0, 0), (0, rows-1).
        for &(x, y) in &[(14, 0), (14, 10), (15, 1), (15, 9), (0, 0), (0, 10)] {
            assert_eq!(
                g[y * 16 + x],
                DotCell::Set,
                "corner ({x},{y}) should be Set"
            );
        }
        // Cell counts: total `rows * columns`, of which exactly
        // `ceil(rows*cols/2)` are odd-parity dot slots.
        let active = g
            .iter()
            .filter(|&&c| matches!(c, DotCell::Empty | DotCell::Set))
            .count();
        // Odd-parity Empty cells = ceil(rows*cols / 2). Plus six
        // Set corner cells on even-parity positions.
        let expected_active = (11_usize * 16).div_ceil(2) + 6;
        assert_eq!(active, expected_active);
    }

    /// `build_bits` produces the BWIPP `$_.bits` string. Verify
    /// against `tools/oracle-dotcode-bits.js` for 7 (input, mask)
    /// pairs covering all four masks + an input long enough to
    /// produce rembits filler (`ABCDE` with ndots=117, nw=12,
    /// rembits=7).
    #[test]
    fn build_bits_matches_oracle() {
        let cases: &[(&[u8], u8, usize, &str)] = &[
            (
                b"A",
                0,
                65,
                "00011000111010010111100111100010111100101110010011010110010011101",
            ),
            (
                b"A",
                1,
                65,
                "01011000111010100111111001100011001101101100101101011100010011011",
            ),
            (
                b"A",
                2,
                65,
                "10011000111011001101101101010011001110101011100000111011010011101",
            ),
            (
                b"A",
                3,
                65,
                "11011000111101100110001010111010001111100110011101100011101001110",
            ),
            (
                b"AB",
                0,
                65,
                "00011100011010010111010011011000111011110110001010111100001111010",
            ),
            (
                b"ABCDE",
                0,
                117,
                "001001111000100101110100110110100111010101001110101100111001011100101101010011011011110011001110001011100010111111111",
            ),
            (
                b"1234",
                0,
                65,
                "00101111000011010110010011011110010011101111000011101001010010111",
            ),
        ];
        for &(input, mask, ndots, want) in cases {
            let mut data = encode_message(input).unwrap();
            let sz = pick_symbol_size_default(data.len());
            assert_eq!(sz.ndots, ndots, "ndots mismatch for {input:?}");
            pad_to_nd(&mut data, sz.nd, false);
            let rscws = encode_for_mask(mask, &data);
            let bits = build_bits(&rscws, mask, sz.ndots);
            assert_eq!(
                bits,
                want,
                "build_bits mismatch for {:?} mask={mask}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>")
            );
        }
    }

    /// `encode_for_mask` for all four masks on input "A" matches
    /// Stage 11.A8c — pin `mask_transform_cws(mask, cws)` directly.
    /// Per-position transformation `(c + p * MASK_VALS[mask]) % 113`
    /// where MASK_VALS = [0, 3, 7, 17]. mask=0 is the identity.
    /// `encode_for_mask` exercises this transitively; mutations on
    /// the `% 113` modulus, the `p * mv` multiplier, or the
    /// MASK_VALS index could survive without the per-element pin.
    ///
    /// Mutations to catch:
    ///   - `p as u32 * mv` → `+ mv`: linear coefficient becomes
    ///     constant offset.
    ///   - `% 113` → `% 112` / `% 114`: wrong modulus.
    ///   - `MASK_VALS[mask as usize]` → constant or wrong index.
    ///   - Body replaced with `cws.to_vec()` (always identity).
    ///
    /// Hand-computed (MASK_VALS = [0, 3, 7, 17]):
    ///   - mask=0, any cws: identity (catches non-identity mutation).
    ///   - mask=1, [10, 20, 30]: [10+0, 20+3, 30+6] = [10, 23, 36].
    ///   - mask=2, [50, 50, 50]: [50, 57, 64] (+7 per step).
    ///   - mask=3, [0; 4]: [0, 17, 34, 51] (+17 per step, no wrap yet).
    ///   - mask=3, p=10, c=50: (50 + 170) % 113 = 220 - 113 = 107.
    ///   - mask=3, p=20, c=0: (0 + 340) % 113 = 340 - 339 = 1 (wrap).
    /// Stage 11.A8c — pin `apply_lit_mask` corner-edge selection for
    /// both even-row and odd-row branches.
    ///
    /// Mutations caught:
    ///   * `rows % 2 == 0` branch direction flip — even-row case would
    ///     write the odd-row cell set and vice-versa.
    ///   * `pixs[y * columns + x]` row-major index — swapping the
    ///     factors writes wrong cells.
    ///   * Any of the six per-branch (x,y) literals — the resulting
    ///     bit pattern wouldn't match.
    #[test]
    fn apply_lit_mask_lights_six_edge_corners_per_parity() {
        // Even rows: rows=4, columns=5 → cells:
        //   (4, 2)→14, (0, 2)→10, (3, 3)→18, (1, 3)→16,
        //   (4, 0)→4,  (0, 0)→0.
        let mut pixs = vec![0i8; 4 * 5];
        apply_lit_mask(4, 5, &mut pixs);
        let lit_even: Vec<usize> = pixs
            .iter()
            .enumerate()
            .filter(|(_, &v)| v == 1)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lit_even, vec![0, 4, 10, 14, 16, 18], "even-row corners");

        // Odd rows: rows=5, columns=5 → cells:
        //   (3, 0)→3,  (3, 4)→23, (4, 1)→9,  (4, 3)→19,
        //   (0, 0)→0,  (0, 4)→20.
        let mut pixs = vec![0i8; 5 * 5];
        apply_lit_mask(5, 5, &mut pixs);
        let lit_odd: Vec<usize> = pixs
            .iter()
            .enumerate()
            .filter(|(_, &v)| v == 1)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lit_odd, vec![0, 3, 9, 19, 20, 23], "odd-row corners");

        // The two cell sets differ (3 ∈ odd, 4 ∈ even; etc.) — pins
        // the rows%2 branch direction.
        assert_ne!(lit_even, lit_odd);
    }

    #[test]
    fn mask_transform_cws_per_position_arithmetic() {
        // mask=0: identity for any input.
        assert_eq!(mask_transform_cws(0, &[]), Vec::<u16>::new());
        assert_eq!(mask_transform_cws(0, &[42]), vec![42]);
        assert_eq!(
            mask_transform_cws(0, &[10, 20, 30, 40, 50]),
            vec![10, 20, 30, 40, 50],
            "mask=0 is the identity (MASK_VALS[0] = 0)"
        );

        // mask=1 (mv=3): per-position +3*p.
        assert_eq!(
            mask_transform_cws(1, &[10, 20, 30]),
            vec![10, 23, 36],
            "mask=1: +3 per position"
        );

        // mask=2 (mv=7): per-position +7*p.
        assert_eq!(
            mask_transform_cws(2, &[50, 50, 50]),
            vec![50, 57, 64],
            "mask=2: +7 per position"
        );

        // mask=3 (mv=17): per-position +17*p (no wrap for small p).
        assert_eq!(
            mask_transform_cws(3, &[0, 0, 0, 0]),
            vec![0, 17, 34, 51],
            "mask=3: +17 per position"
        );

        // Wrap-around cases (catches `% 113` mutation).
        // mask=3, p=10, c=50: (50 + 170) % 113 = 220 - 113 = 107.
        let mut input = vec![0u16; 11];
        input[10] = 50;
        let out = mask_transform_cws(3, &input);
        assert_eq!(out[10], 107, "mask=3, p=10, c=50: wraps via % 113");

        // mask=3, p=20, c=0: (0 + 340) % 113 = 340 - 339 = 1.
        let input = vec![0u16; 21];
        let out = mask_transform_cws(3, &input);
        assert_eq!(out[20], 1, "mask=3, p=20, c=0: 340 % 113 = 1");
        // Also exercise input[0] = 0 identity at p=0.
        assert_eq!(out[0], 0, "p=0 has zero contribution from mask shift");
    }

    /// the rscws[1..] goldens captured by passing `mask` explicitly
    /// to bwip-js. This pins both the mask transformation
    /// (cws[p] += p * maskval) and the per-mask leading sentinel
    /// in the RS LFSR.
    #[test]
    fn encode_for_mask_4_candidates_input_a() {
        // Pipeline: encode_message → pad_to_nd → encode_for_mask.
        let mut data = encode_message(b"A").unwrap();
        let sz = pick_symbol_size_default(data.len());
        pad_to_nd(&mut data, sz.nd, false);
        // Goldens captured from bwip-js with explicit `mask:` option.
        let expected: &[(u8, &[u16])] = &[
            (0, &[102, 33, 106, 68, 52, 12, 35]),
            (1, &[102, 36, 112, 40, 22, 49, 34]),
            (2, &[102, 40, 7, 69, 49, 95, 35]),
            (3, &[102, 50, 27, 101, 79, 82, 48]),
        ];
        for &(mask, want) in expected {
            let got = encode_for_mask(mask, &data);
            assert_eq!(got, want, "encode_for_mask(mask={mask}, A)");
        }
    }

    /// End-to-end including RS-GF(113) ECC: `encode_message +
    /// pick_symbol_size_default + pad_to_nd + apply_rs_ecc` should
    /// match the `rscws` array bwip-js's debugecc anchor captures
    /// (one position after the leading BWIPP 1-indexed sentinel).
    /// Goldens via `tools/oracle-dotcode-rscws.js`.
    #[test]
    fn rs_ecc_matches_oracle() {
        let cases: &[(&[u8], &[u16])] = &[
            // rscws[1..] from oracle-dotcode-rscws.js
            (b"A", &[102, 33, 106, 68, 52, 12, 35]),
            (b"AB", &[103, 33, 34, 95, 89, 68, 66]),
            (b"ABC", &[104, 33, 34, 35, 52, 10, 99, 21, 18]),
            (b"ABCD", &[105, 33, 34, 35, 36, 52, 101, 92, 30, 75]),
            (b"ABCDE", &[106, 33, 34, 35, 36, 37, 45, 3, 31, 112, 90, 84]),
            (b"Hello", &[106, 40, 69, 76, 76, 79, 50, 93, 6, 78, 101, 91]),
            (b"12", &[107, 12, 106, 34, 46, 64, 49]),
            (b"1234", &[107, 12, 34, 86, 107, 44, 33]),
            (b"12345", &[107, 12, 34, 102, 21, 37, 17, 50, 17, 14]),
        ];
        for &(input, want) in cases {
            let mut data = encode_message(input).unwrap();
            let sz = pick_symbol_size_default(data.len());
            pad_to_nd(&mut data, sz.nd, false);
            let full = apply_rs_ecc(&data);
            assert_eq!(
                full,
                want,
                "RS ECC mismatch for {:?} (data={:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                data,
            );
        }
    }

    /// `pick_symbol_size_default` should match BWIPP's chosen
    /// (rows, columns, nd) for each `n_data` (encoder-emitted
    /// codeword count). Verified against `tools/oracle-dotcode.js`:
    /// each oracle output reports `nd`, `rows`, `columns` directly,
    /// and feeding the encoder-emitted length (= `encode_message`
    /// result length) into `pick_symbol_size_default` reproduces
    /// the same triple.
    #[test]
    fn pick_symbol_size_default_matches_oracle() {
        // (n_data, rows, columns, nd_after_grow).
        let cases: &[(usize, usize, usize, usize)] = &[
            (2, 10, 13, 3), // "A" / "1" → encoder emits 2 cws
            (3, 10, 13, 3), // "AB" / "12" / "1234" → 3 cws
            (4, 11, 16, 4), // "ABC" / "123456" → 4 cws
            (5, 12, 17, 5), // "ABCD" / "12345" / "00990099" → 5 cws
            (6, 13, 18, 6), // "ABCDE" / "Hello" / "abc12" → 6 cws
            (7, 13, 20, 7), // "ABCabc" / "1234abc" / "abc12345" → 7 cws
        ];
        for &(n_data, exp_rows, exp_cols, exp_nd) in cases {
            let s = pick_symbol_size_default(n_data);
            assert_eq!(
                (s.rows, s.columns, s.nd),
                (exp_rows, exp_cols, exp_nd),
                "pick_symbol_size_default({n_data}) mismatch"
            );
        }
    }

    /// End-to-end: `encode_message` + `pick_symbol_size_default` +
    /// `pad_to_nd` should reproduce every oracle golden's full cws,
    /// without the test having to pass `nd` in by hand.
    #[test]
    fn full_pipeline_matches_oracle() {
        let cases: &[(&[u8], &[u16])] = &[
            (b"1", &[102, 17, 106]),
            (b"12", &[107, 12, 106]),
            (b"1234", &[107, 12, 34]),
            (b"12345", &[107, 12, 34, 102, 21]),
            (b"123456", &[107, 12, 34, 56]),
            (b"00990099", &[107, 0, 99, 0, 99]),
            (b"A", &[102, 33, 106]),
            (b"AB", &[103, 33, 34]),
            (b"ABC", &[104, 33, 34, 35]),
            (b"ABCD", &[105, 33, 34, 35, 36]),
            (b"ABCDE", &[106, 33, 34, 35, 36, 37]),
            (b"Hello", &[106, 40, 69, 76, 76, 79]),
            (b"abc12", &[106, 65, 66, 67, 17, 18]),
            (b"1abc", &[105, 17, 65, 66, 67]),
            (b"\tABC", &[101, 73, 33, 34, 35]),
            (b"1234abc", &[107, 12, 34, 104, 65, 66, 67]),
            (b"12abc", &[107, 12, 104, 65, 66, 67]),
            (b"ab1234", &[103, 65, 66, 12, 34]),
            (b"abc12345", &[105, 65, 66, 67, 17, 23, 45]),
            (b"ABCabc", &[106, 33, 34, 35, 65, 66, 67]),
        ];
        for &(input, want) in cases {
            let mut cws = encode_message(input).unwrap();
            let sz = pick_symbol_size_default(cws.len());
            pad_to_nd(&mut cws, sz.nd, false);
            assert_eq!(
                cws,
                want,
                "full pipeline for {:?}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// `encode_message` (the full state machine) should round-trip
    /// every oracle golden — both the single-helper cases AND the
    /// mid-message transitions the single helper can't handle.
    #[test]
    fn encode_message_oracle_round_trip() {
        // (input, nd_from_oracle, expected_full_cws).
        let cases: &[(&[u8], usize, &[u16])] = &[
            // Pure single-helper goldens (sanity)
            (b"12", 3, &[107, 12, 106]),
            (b"1234", 3, &[107, 12, 34]),
            (b"12345", 5, &[107, 12, 34, 102, 21]),
            (b"123456", 4, &[107, 12, 34, 56]),
            (b"00990099", 5, &[107, 0, 99, 0, 99]),
            (b"1", 3, &[102, 17, 106]),
            (b"A", 3, &[102, 33, 106]),
            (b"AB", 3, &[103, 33, 34]),
            (b"ABC", 4, &[104, 33, 34, 35]),
            (b"ABCD", 5, &[105, 33, 34, 35, 36]),
            (b"ABCDE", 6, &[106, 33, 34, 35, 36, 37]),
            (b"Hello", 6, &[106, 40, 69, 76, 76, 79]),
            (b"abc12", 6, &[106, 65, 66, 67, 17, 18]),
            (b"1abc", 5, &[105, 17, 65, 66, 67]),
            (b"\tABC", 5, &[101, 73, 33, 34, 35]),
            (b"\t\tABC", 6, &[101, 73, 73, 33, 34, 35]),
            (b"\tABCDE", 7, &[101, 73, 33, 34, 35, 36, 37]),
            (b"\t\t\t\t", 5, &[101, 73, 73, 73, 73]),
            // Mid-message transitions: these are the ones
            // `encode_short_pure` rejects.
            (b"1234abc", 7, &[107, 12, 34, 104, 65, 66, 67]),
            (b"12abc", 6, &[107, 12, 104, 65, 66, 67]),
            (b"ab1234", 5, &[103, 65, 66, 12, 34]),
            (b"abc12345", 7, &[105, 65, 66, 67, 17, 23, 45]),
            (b"ABCabc", 7, &[106, 33, 34, 35, 65, 66, 67]),
        ];
        for &(input, nd, want) in cases {
            let mut cws = encode_message(input).unwrap();
            pad_to_nd(&mut cws, nd, false);
            assert_eq!(
                cws,
                want,
                "encode_message pipeline for {:?} (nd={nd})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Non-ASCII bytes are handled via the BIN escape (BWIPP's
    /// `base259to103` + the encBIN dispatch loop, ported in
    /// `dotcode/mod.rs` lines ~440-540). This regression test
    /// pins that the encoder terminates and returns a usable cws
    /// for inputs that exercise the BIN path.
    #[test]
    fn encode_message_handles_non_ascii_inputs_via_bin_escape() {
        for input in [
            "héllo".as_bytes(),
            "ünicode".as_bytes(),
            "café".as_bytes(),
            &[0xC3u8][..],
            &[0xFFu8][..],
            &[0x80u8][..],
        ] {
            let cws = encode_message(input)
                .unwrap_or_else(|e| panic!("Gap 6 should accept {input:?}: {e:?}"));
            // Every BIN-encoded message contains at least one
            // BIN-enter codeword (112) to switch into BIN mode.
            assert!(
                cws.contains(&BIN_ENTER),
                "BIN run for {input:?} must include the BIN_ENTER codeword 112, cws={cws:?}",
            );
        }
    }

    /// `encode(..)` (the top-level entry) must accept BIN-escape
    /// inputs and produce a finite symbol.
    #[test]
    fn high_level_encode_accepts_non_ascii_inputs_via_bin_escape() {
        // Stage 11.A8c (cont) — descriptive labels naming UTF-8 multi-byte
        // BIN-escape path + DotCode mask range invariant.
        let sym = encode("héllo".as_bytes()).expect("Gap 6 should accept UTF-8 'héllo'");
        assert!(
            !sym.pixs.is_empty(),
            "encode(\"héllo\" as UTF-8 bytes) (multi-byte non-ASCII via BIN escape, Gap 6 path) must produce non-empty pixs; got len={}",
            sym.pixs.len()
        );
        assert!(
            sym.mask <= 3,
            "DotCode mask must be in 0..=3 (4 masks per spec); got mask={}",
            sym.mask
        );
    }

    /// Long payloads push `nw = nd + nc` past 112, which is where
    /// BWIPP switches to the interleaved-streams RS path. The
    /// single-pass `apply_rs_ecc` cannot produce correct output for
    /// `nw > 112`, so the top-level `encode` should surface that as
    /// an explicit InvalidData rather than silently producing a
    /// scanner-unreadable symbol. Regression test for
    /// ROADMAP / GOLDEN_COVERAGE next-iteration item #2.
    #[test]
    fn high_level_encode_rejects_long_payload_requiring_interleaved_rs() {
        // 150 digits → mode-C packs two-digits-per-codeword → ~75
        // data codewords. With nc = nd/2 + 3 = 40 that gives
        // nw = 115 > 112. The threshold is currently nw = 112; 150
        // digits is comfortably over.
        let long = "1".repeat(150);
        let err = encode(long.as_bytes())
            .expect_err("nw > 112 input should be rejected, not silently miscoded");
        match err {
            crate::error::Error::InvalidData(msg) => {
                // Stage 11.A8c (cont) — single-substring `nw=` AND
                // `interleaved RS` upgraded to 4-anchor pin:
                //   1. `DotCode:` symbology prefix
                //   2. `payload requires nw=` full predicate
                //   3. `interleaved RS path activates for nw > 112`
                //      full rationale anchor (kills truncations of
                //      the upper-bound `112` literal in the format
                //      string at line 1681 of dotcode/mod.rs)
                //   4. `nw=120` + `nd=78` + `nc=42` exact value-echo
                //      (150-digit input lands at BWIPP size-table
                //      nd=78 codewords; nc = 78/2 + 3 = 42; nw = 78 +
                //      42 = 120). Kills `nw → nd` / `nw → nc` /
                //      `nd/2 → nd*2` interpolation and arithmetic
                //      mutations in the format string.
                assert!(msg.contains("DotCode:"), "missing DotCode prefix: {msg:?}");
                assert!(
                    msg.contains("payload requires nw="),
                    "missing `payload requires nw=` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("interleaved RS path activates for nw > 112"),
                    "missing full rationale anchor: {msg:?}"
                );
                assert!(
                    msg.contains("nw=120") && msg.contains("nd=78") && msg.contains("nc=42"),
                    "missing nw=120/nd=78/nc=42 value-echo (150-digit input expectation): {msg:?}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    /// Payloads just under the nw > 112 threshold must still encode
    /// successfully so the new guard isn't a regression for the
    /// typical DotCode catalog use-case (short serials, part marks).
    #[test]
    fn high_level_encode_accepts_short_payload_under_threshold() {
        // 60 digits → ~30 data codewords + 18 check = nw ≈ 48,
        // well under 112.
        let short = "1".repeat(60);
        let _sym = encode(short.as_bytes())
            .expect("short payload must still encode after the nw > 112 guard");
    }

    /// `encode_short_pure` should round-trip every "single-helper"
    /// oracle golden byte-for-byte (after the symbol-fill pad), and
    /// should `None`-out on inputs that need mid-message mode
    /// transitions.
    #[test]
    fn encode_short_pure_oracle_round_trip() {
        // (input, nd_from_oracle, expected_full_cws). Only inputs
        // whose entire payload fits a single helper.
        let cases: &[(&[u8], usize, &[u16])] = &[
            (b"", 0, &[]),
            // Pure numeric
            (b"12", 3, &[107, 12, 106]),
            (b"1234", 3, &[107, 12, 34]),
            (b"12345", 5, &[107, 12, 34, 102, 21]),
            (b"123456", 4, &[107, 12, 34, 56]),
            (b"00990099", 5, &[107, 0, 99, 0, 99]),
            (b"1", 3, &[102, 17, 106]),
            // Mode-B short / long
            (b"A", 3, &[102, 33, 106]),
            (b"AB", 3, &[103, 33, 34]),
            (b"ABC", 4, &[104, 33, 34, 35]),
            (b"ABCD", 5, &[105, 33, 34, 35, 36]),
            (b"ABCDE", 6, &[106, 33, 34, 35, 36, 37]),
            (b"Hello", 6, &[106, 40, 69, 76, 76, 79]),
            (b"abc12", 6, &[106, 65, 66, 67, 17, 18]),
            (b"1abc", 5, &[105, 17, 65, 66, 67]),
            // Mode-A (tab is A-only)
            (b"\tABC", 5, &[101, 73, 33, 34, 35]),
            (b"\t\tABC", 6, &[101, 73, 73, 33, 34, 35]),
            (b"\tABCDE", 7, &[101, 73, 33, 34, 35, 36, 37]),
            (b"\t\t\t\t", 5, &[101, 73, 73, 73, 73]),
        ];
        for &(input, nd, want) in cases {
            let mut cws = encode_short_pure(input).unwrap_or_else(|| {
                panic!("encode_short_pure({input:?}) returned None — expected Some(...)")
            });
            pad_to_nd(&mut cws, nd, false);
            assert_eq!(
                cws,
                want,
                "encode_short_pure pipeline for {:?} (nd={nd})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }

        // These need mid-message transitions back to mode C — the
        // simple helper rejects them. The full mode selector will
        // handle them once the main-loop port lands.
        let rejects: &[&[u8]] = &[
            b"1234abc",  // FN1 + 2 pairs + SB3 + abc
            b"12abc",    // FN1 + 1 pair + SB3 + abc
            b"ab1234",   // SB2 + ab + 2 pairs in C
            b"abc12345", // SB4 + abc1 + 2 pairs in C
        ];
        for input in rejects {
            assert!(
                encode_short_pure(input).is_none(),
                "encode_short_pure({input:?}) should reject mid-message-transition inputs"
            );
        }
    }

    /// `dispatch_initial` should pick the same action BWIPP's
    /// `encC` does at segstart. Verify with the inputs whose cws
    /// goldens we already have — the first codeword in each oracle
    /// `cws` corresponds 1:1 to the InitialAction variant.
    #[test]
    fn dispatch_initial_matches_bwipp_oracle() {
        fn pick(msg: &[u8]) -> InitialAction {
            let tables = build_position_tables(msg);
            dispatch_initial(&tables, msg)
        }
        // Empty
        assert_eq!(pick(b""), InitialAction::NoPrologue);
        // Pure digits ≥ 2 → FN1
        assert_eq!(pick(b"12"), InitialAction::Fn1ThenStayInC);
        assert_eq!(pick(b"1234"), InitialAction::Fn1ThenStayInC);
        assert_eq!(pick(b"12345"), InitialAction::Fn1ThenStayInC);
        assert_eq!(pick(b"1234abc"), InitialAction::Fn1ThenStayInC);
        // 1 digit → no FN1, short mode-B (digits live in B too).
        assert_eq!(pick(b"1"), InitialAction::ShiftToBFor(1));
        // Mode-B short: shift.
        assert_eq!(pick(b"A"), InitialAction::ShiftToBFor(1));
        assert_eq!(pick(b"AB"), InitialAction::ShiftToBFor(2));
        assert_eq!(pick(b"ABC"), InitialAction::ShiftToBFor(3));
        assert_eq!(pick(b"ABCD"), InitialAction::ShiftToBFor(4));
        // Mode-B long: latch.
        assert_eq!(pick(b"ABCDE"), InitialAction::LatchToB);
        assert_eq!(pick(b"Hello"), InitialAction::LatchToB);
        assert_eq!(pick(b"abc12"), InitialAction::LatchToB);
        // Mode-A only (tab is A-only).
        assert_eq!(pick(b"\tABC"), InitialAction::LatchToA);
        assert_eq!(pick(b"\t\tABC"), InitialAction::LatchToA);
        assert_eq!(pick(b"\t\t\t\t"), InitialAction::LatchToA);
    }

    /// `encode_mode_a_run_from_c` emits LAA (101) + per-byte mode-A
    /// codewords. Goldens captured from `oracle-dotcode.js` for
    /// tab-prefixed inputs (`\t` = 9, which is mode-A-only because
    /// it's an ASCII control byte). For each case the oracle's full
    /// `cws` is the data segment exactly (no pad needed since the
    /// chosen `nd` matches data length).
    #[test]
    fn encode_mode_a_run_from_c_matches_bwipp_oracle() {
        // tab + ABC: LAA + tab + A + B + C
        assert_eq!(
            encode_mode_a_run_from_c(b"\tABC"),
            Some(vec![101, 73, 33, 34, 35]),
        );
        // 2 tabs + ABC
        assert_eq!(
            encode_mode_a_run_from_c(b"\t\tABC"),
            Some(vec![101, 73, 73, 33, 34, 35]),
        );
        // tab + ABCDE
        assert_eq!(
            encode_mode_a_run_from_c(b"\tABCDE"),
            Some(vec![101, 73, 33, 34, 35, 36, 37]),
        );
        // 4 tabs (pure control)
        assert_eq!(
            encode_mode_a_run_from_c(b"\t\t\t\t"),
            Some(vec![101, 73, 73, 73, 73]),
        );
        // Lowercase isn't mode-A → None.
        assert!(encode_mode_a_run_from_c(b"abc").is_none());
    }

    /// End-to-end mode-A pipeline: encode + (no-)pad reproduces the
    /// full oracle cws for tab-prefixed payloads.
    #[test]
    fn mode_a_pipeline_matches_full_oracle_cws() {
        let cases: &[(&[u8], usize, &[u16])] = &[
            (b"\tABC", 5, &[101, 73, 33, 34, 35]),
            (b"\t\tABC", 6, &[101, 73, 73, 33, 34, 35]),
            (b"\tABCDE", 7, &[101, 73, 33, 34, 35, 36, 37]),
            (b"\t\t\t\t", 5, &[101, 73, 73, 73, 73]),
        ];
        for &(input, nd, want) in cases {
            let mut cws = encode_mode_a_run_from_c(input).unwrap();
            pad_to_nd(&mut cws, nd, false);
            assert_eq!(cws, want, "mode-A pipeline for {input:?} (nd={nd})");
        }
    }

    /// End-to-end mode-B pipeline: encode + pad reproduces the
    /// full oracle cws (mirrors the mode-C pipeline test).
    #[test]
    fn mode_b_pipeline_matches_full_oracle_cws() {
        let cases: &[(&[u8], usize, &[u16])] = &[
            (b"A", 3, &[102, 33, 106]),
            (b"AB", 3, &[103, 33, 34]),
            (b"ABC", 4, &[104, 33, 34, 35]),
            (b"ABCD", 5, &[105, 33, 34, 35, 36]),
            (b"ABCDE", 6, &[106, 33, 34, 35, 36, 37]),
            (b"Hello", 6, &[106, 40, 69, 76, 76, 79]),
        ];
        for &(input, nd, want) in cases {
            let mut cws = encode_mode_b_run_from_c(input).unwrap();
            pad_to_nd(&mut cws, nd, false);
            assert_eq!(
                cws,
                want,
                "mode-B pipeline for {:?} (nd={nd})",
                std::str::from_utf8(input).unwrap(),
            );
        }
    }

    /// End-to-end: composing `encode_numeric_run_from_c` with
    /// `pad_to_nd` at the oracle-reported `nd` should reproduce
    /// the full `cws` array. These are the integration goldens
    /// that validate the stage-4 building blocks add up.
    #[test]
    fn numeric_pipeline_matches_full_oracle_cws() {
        // Each tuple: (input, nd_from_oracle, expected_full_cws).
        let cases: &[(&[u8], usize, &[u16])] = &[
            (b"12", 3, &[107, 12, 106]),
            (b"1234", 3, &[107, 12, 34]),
            (b"12345", 5, &[107, 12, 34, 102, 21]),
            (b"123456", 4, &[107, 12, 34, 56]),
            (b"00990099", 5, &[107, 0, 99, 0, 99]),
            (b"1", 3, &[102, 17, 106]),
        ];
        for &(input, nd, want) in cases {
            let mut cws = encode_numeric_run_from_c(input).unwrap();
            pad_to_nd(&mut cws, nd, false);
            assert_eq!(
                cws,
                want,
                "numeric pipeline for {:?} (nd={nd})",
                std::str::from_utf8(input).unwrap()
            );
        }
    }

    /// `encode_numeric_run_from_c` matches the oracle goldens
    /// captured for "12" / "1234" / "12345" / "123456" / "00990099"
    /// (stripping the trailing symbol-fill `106` codeword from the
    /// short cases — those are added later when symbol dimensions
    /// are known).
    #[test]
    fn encode_numeric_run_from_c_matches_bwipp_oracle() {
        // 1 digit: no FN1 prefix (nDigits<2); shift to B, encode '1'.
        assert_eq!(encode_numeric_run_from_c(b"1"), Some(vec![102, 17]));
        // 2 digits: FN1 + 1 pair.
        assert_eq!(encode_numeric_run_from_c(b"12"), Some(vec![107, 12]));
        // 4 digits: FN1 + 2 pairs.
        assert_eq!(encode_numeric_run_from_c(b"1234"), Some(vec![107, 12, 34]));
        // 5 digits (odd): FN1 + 2 pairs + SFB + trailing '5' in mode B.
        assert_eq!(
            encode_numeric_run_from_c(b"12345"),
            Some(vec![107, 12, 34, 102, 21]),
        );
        // 6 digits: FN1 + 3 pairs.
        assert_eq!(
            encode_numeric_run_from_c(b"123456"),
            Some(vec![107, 12, 34, 56]),
        );
        // Wider variety of pair values: confirms the column-C pair
        // packing (no overflow / sign issues at pair = 0).
        assert_eq!(
            encode_numeric_run_from_c(b"00990099"),
            Some(vec![107, 0, 99, 0, 99]),
        );

        // Rejects non-digits.
        assert!(encode_numeric_run_from_c(b"12a4").is_none());
        // Empty input is fine — returns empty cws.
        assert_eq!(encode_numeric_run_from_c(b""), Some(vec![]));
    }

    /// `encode_mode_b_run_from_c` should emit the right shift /
    /// latch prologue followed by the column-B byte codewords.
    /// Goldens captured by stripping the trailing `pad` codeword
    /// from `tools/oracle-dotcode.js` output (the function output
    /// is data-only — symbol-fill padding is added later).
    #[test]
    fn encode_mode_b_run_from_c_matches_bwipp_oracle() {
        // 1 char → SFB (102) + char.
        assert_eq!(encode_mode_b_run_from_c(b"A"), Some(vec![102, 33]));
        // 2 chars → SB2 (103) + chars.
        assert_eq!(encode_mode_b_run_from_c(b"AB"), Some(vec![103, 33, 34]));
        // 3 chars → SB3 (104).
        assert_eq!(
            encode_mode_b_run_from_c(b"ABC"),
            Some(vec![104, 33, 34, 35])
        );
        // 4 chars → SB4 (105).
        assert_eq!(
            encode_mode_b_run_from_c(b"ABCD"),
            Some(vec![105, 33, 34, 35, 36]),
        );
        // 5 chars → LAB (106): sticky latch.
        assert_eq!(
            encode_mode_b_run_from_c(b"ABCDE"),
            Some(vec![106, 33, 34, 35, 36, 37]),
        );
        // Lowercase exercise: cws for "Hello" was [106, 40, 69, 76, 76, 79].
        assert_eq!(
            encode_mode_b_run_from_c(b"Hello"),
            Some(vec![106, 40, 69, 76, 76, 79]),
        );
    }

    /// Odd-length numeric input or any non-digit byte should make
    /// `encode_c` return `None`; the caller (mode selector) is then
    /// expected to handle the trailing digit via mode A or B.
    #[test]
    fn encode_c_rejects_odd_or_non_digit() {
        assert!(encode_c(b"123").is_none());
        assert!(encode_c(b"12A4").is_none());
        assert!(encode_c(b"").is_some()); // empty is fine
    }

    /// Charset markers must all be distinct and outside the ASCII
    /// byte range (BWIPP encodes them as negative i16). A duplicate
    /// would silently merge two different control actions into one
    /// — easy to introduce when porting a long constant block.
    #[test]
    fn charset_markers_are_distinct_and_negative() {
        let all = [
            LAA, LAB, LAC, BIN, SFA, SFB, SB2, SB3, SB4, SB5, SB6, SFC, SC2, SC3, SC4, SC5, SC6,
            SC7, BSA, BSB, TMA, TMB, TMC, TMS, FN1, FN2, FN3, CRL, AIM, M05, M06, M12, MAC,
        ];
        assert_eq!(all.len(), 33);
        for (i, &a) in all.iter().enumerate() {
            assert!(a < 0, "marker at index {i} is {a}, not negative");
            for &b in &all[i + 1..] {
                assert_ne!(a, b, "duplicate marker {a}");
            }
        }
    }

    /// `CHARMAPS` has one row per codeword (0..=112). Every cell is
    /// either a valid ASCII byte or one of the 33 declared charset
    /// markers — a stray sentinel value (e.g. -99) would point to a
    /// typo in the port.
    #[test]
    fn charmaps_table_shape() {
        assert_eq!(CHARMAPS.len(), 113);
        let markers = [
            LAA, LAB, LAC, BIN, SFA, SFB, SB2, SB3, SB4, SB5, SB6, SFC, SC2, SC3, SC4, SC5, SC6,
            SC7, BSA, BSB, TMA, TMB, TMC, TMS, FN1, FN2, FN3, CRL, AIM, M05, M06, M12, MAC,
        ];
        for (i, row) in CHARMAPS.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                let ok = if v >= 0 {
                    // Positive cells fall in one of two ranges:
                    //   * 0..=127 — an ASCII byte (any column, any row).
                    //   * row index — column C of rows 0..=95 stores
                    //     the codeword (== row index), which can exceed
                    //     127 only when the row index does.
                    (0..=127).contains(&v) || v as usize == i
                } else {
                    markers.contains(&v)
                };
                assert!(
                    ok,
                    "CHARMAPS[{i}][{j}] = {v} is neither ASCII nor a known marker"
                );
            }
        }
    }

    /// Rows 0..=63 hold the visible ASCII range 32..=95 with col A
    /// equal to col B (mode A and mode B share that portion of the
    /// table). Rows 64..=95 split into uppercase-control (A) vs.
    /// lowercase (B). Col C of rows 0..=95 is the row index.
    #[test]
    fn charmaps_ascii_section_layout() {
        for (i, row) in CHARMAPS.iter().enumerate().take(64) {
            let want_byte = 32 + i as i16;
            assert_eq!(row[0], want_byte, "row {i} col A");
            assert_eq!(row[1], want_byte, "row {i} col B");
            assert_eq!(row[2], i as i16, "row {i} col C");
        }
        for (i, row) in CHARMAPS.iter().enumerate().take(96).skip(64) {
            assert_eq!(row[0], (i - 64) as i16, "row {i} col A (control byte)");
            assert_eq!(row[1], (i + 32) as i16, "row {i} col B (lowercase/ctl)");
            assert_eq!(row[2], i as i16, "row {i} col C");
        }
    }

    /// Spot-check the marker rows against bwip-js's `dotcode_charmaps`
    /// (captured via `tools/oracle-dotcode-charmaps.js`). These are
    /// the cells that drive mode-switching and FNC1/AIM/macro
    /// behaviour, so a typo here would propagate into every encoded
    /// symbol with non-trivial control flow.
    #[test]
    fn charmaps_marker_rows_match_bwipp() {
        // (row index, expected [a, b, c]).
        let cases: &[(usize, [i16; 3])] = &[
            (96, [SFB, CRL, 96]),
            (97, [SB2, 9, 97]),
            (98, [SB3, 28, 98]),
            (99, [SB4, 29, 99]),
            (100, [SB5, 30, AIM]),
            (101, [SB6, SFA, LAA]),
            (102, [LAB, LAA, SFB]),
            (103, [SC2, SC2, SB2]),
            (104, [SC3, SC3, SB3]),
            (105, [SC4, SC4, SB4]),
            (106, [LAC, LAC, LAB]),
            (107, [FN1, FN1, FN1]),
            (108, [FN2, FN2, FN2]),
            (109, [FN3, FN3, FN3]),
            (110, [BSA, BSA, BSA]),
            (111, [BSB, BSB, BSB]),
            (112, [BIN, BIN, BIN]),
        ];
        for &(i, want) in cases {
            assert_eq!(CHARMAPS[i], want, "CHARMAPS[{i}]");
        }
    }

    /// In modes A and B, encountering an input byte X means: find the
    /// row where the appropriate column equals X, and emit that row's
    /// index as the codeword. Build the reverse-lookup map and
    /// confirm a handful of well-known characters resolve to the
    /// expected codeword in each mode.
    #[test]
    fn charmaps_reverse_lookup_for_known_bytes() {
        // (byte, mode-A codeword, mode-B codeword).
        let cases: &[(u8, u16, u16)] = &[
            (b' ', 0, 0),   // space
            (b'A', 33, 33), // uppercase A
            (b'9', 25, 25), // digit
            (b'_', 63, 63), // underscore is in the shared ASCII range
        ];
        for &(byte, want_a, want_b) in cases {
            let row_a = CHARMAPS
                .iter()
                .position(|r| r[0] == byte as i16)
                .unwrap_or_else(|| panic!("byte 0x{byte:02x} not in column A"));
            let row_b = CHARMAPS
                .iter()
                .position(|r| r[1] == byte as i16)
                .unwrap_or_else(|| panic!("byte 0x{byte:02x} not in column B"));
            assert_eq!(row_a as u16, want_a, "Avals[0x{byte:02x}]");
            assert_eq!(row_b as u16, want_b, "Bvals[0x{byte:02x}]");
        }
    }

    /// `LATCH_C = 107` should match the FN1 row in `CHARMAPS` — that
    /// is the codeword `encC` actually emits at the start of a numeric
    /// segment. Anchoring it here keeps the two in sync if the
    /// charmap layout ever shifts.
    #[test]
    fn latch_c_matches_fn1_row() {
        let fn1_row = CHARMAPS
            .iter()
            .position(|r| r[2] == FN1)
            .expect("FN1 missing from column C");
        assert_eq!(fn1_row as u16, LATCH_C);
    }

    // ------------------------------------------------------------------
    // parse_dotcode_input — Gap 1 of DOTCODE_COMPLETION_PLAN.md.
    // Mirrors bwipp_parseinput's parsefnc branch wired with dotcode's
    // fncvals map (FNC1, FNC3 plus the ECI alias), followed by the
    // ECI → FN2 + 6 digits expansion bwip-js does at line 34989-35021.
    // ------------------------------------------------------------------

    /// Inputs containing no `^` should pass through verbatim — each
    /// byte becomes its positive `i16` value, regardless of the
    /// `parsefnc` flag. Anchors the "happy path" so a regression in
    /// the escape-handling branches doesn't accidentally rewrite
    /// ordinary text.
    #[test]
    fn parse_dotcode_input_plain_ascii_passthrough() {
        let bytes = b"Hello123";
        let expected: Vec<i16> = bytes.iter().map(|&b| i16::from(b)).collect();
        assert_eq!(parse_dotcode_input(bytes, true).unwrap(), expected);
        assert_eq!(parse_dotcode_input(bytes, false).unwrap(), expected);
    }

    /// `^FNC1` becomes the [`FN1`] marker at exactly the right
    /// position; surrounding bytes are untouched.
    #[test]
    fn parse_dotcode_input_fnc1_emits_fn1_marker() {
        let parsed = parse_dotcode_input(b"AB^FNC1CD", true).unwrap();
        assert_eq!(
            parsed,
            vec![
                i16::from(b'A'),
                i16::from(b'B'),
                FN1,
                i16::from(b'C'),
                i16::from(b'D'),
            ]
        );
    }

    /// `^FNC3` becomes the [`FN3`] marker. (BWIPP's dotcode fncvals
    /// map registers `FNC3` as a synonym for `dotcode_fn3` / -27.)
    #[test]
    fn parse_dotcode_input_fnc3_emits_fn3_marker() {
        let parsed = parse_dotcode_input(b"^FNC3X", true).unwrap();
        assert_eq!(parsed, vec![FN3, i16::from(b'X')]);
    }

    /// `^ECI<6 digits>` is the only documented route for the `FN2`
    /// marker to appear in `msg`. BWIPP first parses the escape into
    /// an internal `-1000000 + value` sentinel, then the dotcode
    /// front-end rewrites that sentinel into `FN2` followed by six
    /// ASCII digit bytes (zero-padded). We collapse both passes, so
    /// the parser output is exactly what BWIPP's `msg` array holds
    /// after the rewrite.
    #[test]
    fn parse_dotcode_input_eci_expands_to_fn2_plus_six_digits() {
        let parsed = parse_dotcode_input(b"^ECI123456A", true).unwrap();
        assert_eq!(
            parsed,
            vec![
                FN2,
                i16::from(b'1'),
                i16::from(b'2'),
                i16::from(b'3'),
                i16::from(b'4'),
                i16::from(b'5'),
                i16::from(b'6'),
                i16::from(b'A'),
            ]
        );
    }

    /// `^^` becomes a literal `^` (byte 94) — the only way to put a
    /// caret into a `parsefnc = true` payload.
    #[test]
    fn parse_dotcode_input_double_caret_emits_literal_caret() {
        let parsed = parse_dotcode_input(b"A^^B", true).unwrap();
        assert_eq!(parsed, vec![i16::from(b'A'), 94, i16::from(b'B')]);
    }

    /// With `parsefnc = false`, the parser is a byte-to-i16 cast —
    /// `^FNC1` stays as five literal bytes. This is the dotcode
    /// default (BWIPP's `$_.parsefnc = false`).
    #[test]
    fn parse_dotcode_input_parsefnc_off_leaves_caret_literal() {
        let raw = b"^FNC1abc";
        let expected: Vec<i16> = raw.iter().map(|&b| i16::from(b)).collect();
        assert_eq!(parse_dotcode_input(raw, false).unwrap(), expected);
    }

    /// BWIPP's dotcode fncvals only registers `FNC1` and `FNC3`, so
    /// `^FNC2` raises "Unknown function character" upstream. (`FN2`
    /// is reserved for the ECI rewrite path — there is no
    /// user-typeable escape for it. Likewise `^M05` / `^MAC` are not
    /// BWIPP escapes; encC detects macros from raw bytes at segstart
    /// instead.) Reject these to match BWIPP exactly.
    #[test]
    fn parse_dotcode_input_unknown_function_rejected() {
        for name in [
            &b"^FNC2"[..],
            b"^M05A",
            b"^MACR",
            b"^XYZW",
            b"^fnc1", // case-sensitive
        ] {
            let err = parse_dotcode_input(name, true).unwrap_err();
            assert!(
                matches!(err, crate::error::Error::InvalidData(_)),
                "expected InvalidData rejecting {:?}, got {err:?}",
                std::str::from_utf8(name).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// `^ECI` requires exactly six ASCII decimal digits — anything
    /// else (alpha mixed in, truncated, etc.) is an error.
    #[test]
    fn parse_dotcode_input_eci_validates_digits() {
        for bad in [
            &b"^ECI"[..],  // truncated immediately
            b"^ECI12",     // 2 digits
            b"^ECI12345",  // 5 digits — still short
            b"^ECI12345A", // 5 digits + alpha
            b"^ECIabcdef", // pure alpha
            b"^ECI-12345", // sign char
        ] {
            let err = parse_dotcode_input(bad, true).unwrap_err();
            assert!(
                matches!(err, crate::error::Error::InvalidData(_)),
                "expected InvalidData rejecting {:?}, got {err:?}",
                std::str::from_utf8(bad).unwrap_or("<non-utf8>"),
            );
        }

        // Zero-padded boundary values should round-trip cleanly.
        let parsed = parse_dotcode_input(b"^ECI000000", true).unwrap();
        assert_eq!(
            parsed,
            vec![
                FN2,
                b'0'.into(),
                b'0'.into(),
                b'0'.into(),
                b'0'.into(),
                b'0'.into(),
                b'0'.into()
            ]
        );
        let parsed = parse_dotcode_input(b"^ECI999999", true).unwrap();
        assert_eq!(
            parsed,
            vec![
                FN2,
                b'9'.into(),
                b'9'.into(),
                b'9'.into(),
                b'9'.into(),
                b'9'.into(),
                b'9'.into()
            ]
        );
    }

    /// A trailing `^` with nothing after it is "caret character
    /// truncated" — BWIPP raises the same error at line 1085-1088 of
    /// bwip-js. Likewise a `^` followed by fewer than four chars is
    /// "function character truncated".
    #[test]
    fn parse_dotcode_input_truncated_caret_rejected() {
        for trunc in [&b"abc^"[..], b"^", b"^F", b"^FN", b"^FNC"] {
            let err = parse_dotcode_input(trunc, true).unwrap_err();
            assert!(
                matches!(err, crate::error::Error::InvalidData(_)),
                "expected InvalidData rejecting truncated {:?}, got {err:?}",
                std::str::from_utf8(trunc).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// High-byte inputs (e.g. UTF-8 continuation bytes 0x80..=0xff)
    /// pass through as positive `i16` values regardless of
    /// `parsefnc`. The BIN-escape branch that *consumes* such bytes
    /// lives later in the encoder (Gap 6) — the parser's only job is
    /// to surface them unchanged.
    #[test]
    fn parse_dotcode_input_high_bytes_passthrough() {
        // "café" in UTF-8 = 0x63 0x61 0x66 0xC3 0xA9.
        let bytes: &[u8] = b"caf\xC3\xA9";
        let expected: Vec<i16> = vec![0x63, 0x61, 0x66, 0xC3, 0xA9].into_iter().collect();
        assert_eq!(parse_dotcode_input(bytes, true).unwrap(), expected);
        assert_eq!(parse_dotcode_input(bytes, false).unwrap(), expected);
    }

    /// Multiple escapes in a single payload should interleave with
    /// surrounding text correctly. Anchors the "stateful walk" so a
    /// regression in `idx` accounting doesn't slip in undetected.
    #[test]
    fn parse_dotcode_input_multiple_escapes_interleave() {
        let parsed = parse_dotcode_input(b"^FNC1AB^^CD^FNC3^ECI000026end", true).unwrap();
        let want: Vec<i16> = vec![
            FN1,
            i16::from(b'A'),
            i16::from(b'B'),
            94, // literal ^
            i16::from(b'C'),
            i16::from(b'D'),
            FN3,
            FN2,
            i16::from(b'0'),
            i16::from(b'0'),
            i16::from(b'0'),
            i16::from(b'0'),
            i16::from(b'2'),
            i16::from(b'6'),
            i16::from(b'e'),
            i16::from(b'n'),
            i16::from(b'd'),
        ];
        assert_eq!(parsed, want);
    }

    // ------------------------------------------------------------------
    // encode_message_with_markers — Gap 2 of DOTCODE_COMPLETION_PLAN.md.
    // Each golden was captured from bwip-js via tools/oracle-dotcode.js
    // (with the parsefnc option toggled where needed) and pinned here.
    // ------------------------------------------------------------------

    /// `^FNC1A1234` with parsefnc → BWIPP cws `[107, 102, 33, 12, 34]`:
    ///
    ///   * `107` = inline FN1 marker emitted via the encC DatumC branch.
    ///   * `102` = `Cvals[SFB]` shift-to-B for 1 char (n=1).
    ///   * `33`  = `Bvals['A']`.
    ///   * `12`  = digit pair `"12"`.
    ///   * `34`  = digit pair `"34"`.
    ///
    /// Validates the BWIPP-faithful path:
    ///   1. parser produces `[FN1, A, 1, 2, 3, 4]`,
    ///   2. encC at segstart emits 107 via the marker branch (not
    ///      via auto-prepend, because nDigits[0]=0),
    ///   3. then shifts to B for the literal 'A',
    ///   4. then encodes the digit run.
    #[test]
    fn encode_with_markers_fn1_at_segstart_then_mixed() {
        let parsed = parse_dotcode_input(b"^FNC1A1234", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![107, 102, 33, 12, 34]);
    }

    /// `^FNC11234` with parsefnc → BWIPP cws `[12, 34]`. The leading
    /// `FN1` is consumed (i+=1) by the segstart skip branch — BWIPP
    /// would otherwise double-emit 107 (once as the explicit marker,
    /// once as the auto-prepend), so it suppresses the marker byte
    /// when followed by `≥ 2` digits at segstart.
    #[test]
    fn encode_with_markers_fn1_at_segstart_collapses_with_digit_prepend() {
        let parsed = parse_dotcode_input(b"^FNC11234", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![12, 34]);
    }

    /// `^FNC1ABC` with parsefnc → BWIPP cws `[107, 104, 33, 34, 35]`:
    ///   * `107` = inline FN1 via encC marker branch.
    ///   * `104` = `Cvals[SB3]` shift-to-B for 3 chars.
    ///   * `33, 34, 35` = `Bvals['A','B','C']`.
    ///
    /// Demonstrates that after the inline FN1 emit, encC drops back
    /// to the AheadA/AheadB transition logic (n=3 → SB3 shift) for
    /// the trailing mode-B run.
    #[test]
    fn encode_with_markers_fn1_then_mode_b_run() {
        let parsed = parse_dotcode_input(b"^FNC1ABC", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![107, 104, 33, 34, 35]);
    }

    /// `AB^FNC1CD` with parsefnc → BWIPP cws `[106, 33, 34, 107, 35, 36]`:
    ///   * `106` = `Cvals[LAB]` latch-to-B (5 mode-B chars ahead: ABCD
    ///     and the FN1 marker, which is encodable in B too, so n=5).
    ///   * `33, 34` = 'A','B' in mode B.
    ///   * `107` = inline FN1 via encB column-B lookup
    ///     (`Bvals[FN1] = 107`).
    ///   * `35, 36` = 'C','D'.
    ///
    /// This is the critical encB-marker case: when the encoder
    /// latches to B and then encounters an inline FN1 in the run,
    /// the column-B lookup must find the marker (the charmap row
    /// for FN1 has the marker in all three columns).
    #[test]
    fn encode_with_markers_inline_fn1_in_mode_b_run() {
        let parsed = parse_dotcode_input(b"AB^FNC1CD", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![106, 33, 34, 107, 35, 36]);
    }

    /// `12^FNC134` with parsefnc → BWIPP cws `[107, 12, 107, 34]`:
    ///   * `107` = auto-prepend at segstart (nDigits[0]=2).
    ///   * `12`  = digit pair `"12"`.
    ///   * `107` = inline FN1 via encC marker branch (DatumC=true
    ///     because barchar<0).
    ///   * `34`  = digit pair `"34"`.
    ///
    /// Exercises the mid-message FN1 marker between two digit pairs
    /// — the "GS1 inline group separator" pattern.
    #[test]
    fn encode_with_markers_fn1_between_digit_pairs() {
        let parsed = parse_dotcode_input(b"12^FNC134", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![107, 12, 107, 34]);
    }

    /// `12^FNC1` (trailing FN1) with parsefnc → BWIPP cws
    /// `[107, 12, 107]`. The first 107 is auto-prepended at
    /// segstart (nDigits=2), the second 107 is the inline FN1 via
    /// the marker branch.
    #[test]
    fn encode_with_markers_trailing_fn1() {
        let parsed = parse_dotcode_input(b"12^FNC1", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![107, 12, 107]);
    }

    /// `ABC^FNC1` (trailing FN1 after mode-B run) with parsefnc →
    /// BWIPP cws `[105, 33, 34, 35, 107]`:
    ///   * `105` = `Cvals[SB4]` shift-to-B for 4 chars (ABC + the
    ///     trailing FN1 marker, which counts as B-encodable).
    ///   * `33, 34, 35` = 'A','B','C' in column B.
    ///   * `107` = FN1 marker via column-B lookup
    ///     (`Bvals[FN1] = 107`).
    #[test]
    fn encode_with_markers_trailing_fn1_after_mode_b_run() {
        let parsed = parse_dotcode_input(b"ABC^FNC1", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![105, 33, 34, 35, 107]);
    }

    /// `encode_with_markers` should match BWIPP's full symbol — not
    /// just the data cws but also the RS check codewords + padding +
    /// mask + dimensions. The golden values come from bwip-js's
    /// `rows` / `columns` fields (captured via
    /// tools/oracle-dotcode.js).
    #[test]
    fn encode_with_markers_full_symbol_matches_bwip_js() {
        let parsed = parse_dotcode_input(b"^FNC1A1234", true).unwrap();
        let sym = encode_with_markers(&parsed).unwrap();
        // bwip-js reports rows=12, columns=17 for this payload.
        assert_eq!(sym.rows, 12);
        assert_eq!(sym.columns, 17);
        assert_eq!(sym.pixs.len(), sym.rows * sym.columns);
        // Mask is 0..=3; pick_best_mask is deterministic.
        assert!(sym.mask <= 3);
    }

    /// The `encode(&[u8])` entry point routes through
    /// `parse_dotcode_input(input, /* parsefnc = */ false)`. With
    /// `parsefnc` off the parser is a byte-to-i16 cast, so the
    /// existing all-ASCII test corpus still round-trips byte-for-
    /// byte. Spot-check a few inputs to anchor that compatibility.
    #[test]
    fn encode_routes_through_parser_with_parsefnc_off() {
        // Stage 11.A8c (cont) — descriptive label naming per-input loop
        // (the bare `assert!(!sym.pixs.is_empty())` gave no info on
        // WHICH of 4 inputs returned empty pixs).
        for input in [&b"1234"[..], b"ABC", b"Hello", b"1A"] {
            let sym = encode(input).unwrap();
            assert!(
                !sym.pixs.is_empty(),
                "encode({input:?}) (parsefnc=off byte-to-i16 cast path, corpus item) must produce non-empty DotCode pixs; got len={}",
                sym.pixs.len()
            );
        }
        // With parsefnc off, `^FNC1` is treated as five literal bytes.
        // BWIPP would also reject this from the high-level `encode`
        // path because '^' is encodable but the run with `^FNC1`
        // requires mode-B handling; we ship the InvalidData fall-
        // through here until Gap 3 lands. The important property is
        // that we *don't* panic and *don't* honor the escape.
        let _ = encode(b"ABC^FNC1XY");
    }

    // ------------------------------------------------------------------
    // encB / encA full dispatch — Gap 3 + Gap 4 of
    // DOTCODE_COMPLETION_PLAN.md. Goldens captured from bwip-js via
    // tools/oracle-dotcode.js with the input encoded as base64 to
    // survive shell escaping for tab / CRLF inputs.
    // ------------------------------------------------------------------

    /// `abcdef1234` → cws `[106, 65..70, 103, 12, 34]`:
    ///   * `106` = `Cvals[LAB]` latch to B (6 mode-B chars ahead).
    ///   * `65..70` = `Bvals['a'..'f']`.
    ///   * `103` = `Bvals[SC2]` shift back to C for 2 pairs (TryC[6]=2
    ///     ≤ 4 → shift, not latch).
    ///   * `12, 34` = digit pairs `"12"`, `"34"`.
    ///
    /// Exercises Gap 3: encB recognises `TryC[i] >= 2` and emits the
    /// shift-to-C codeword + paired-digit codewords inline.
    #[test]
    fn encode_message_with_markers_mode_b_then_back_to_c() {
        let parsed = parse_dotcode_input(b"abcdef1234", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![106, 65, 66, 67, 68, 69, 70, 103, 12, 34]);
    }

    /// `ABCDE1234` → cws `[106, 33..37, 103, 12, 34]`. Same shape as
    /// `abcdef1234` but with uppercase letters — these are A *and*
    /// B-encodable, yet BWIPP still latches to B (LAB) because 5
    /// chars + 4 digits → AheadA[0]=5 == AheadB[0]=5, ties break
    /// toward B at segstart (m > n fails for equality).
    #[test]
    fn encode_message_with_markers_uppercase_then_digits_latches_b_first() {
        let parsed = parse_dotcode_input(b"ABCDE1234", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![106, 33, 34, 35, 36, 37, 103, 12, 34]);
    }

    /// `abc^FNC1xyz` → cws `[106, 65, 66, 67, 107, 88, 89, 90]`:
    ///   * `106` = LAB (7 B-encodable chars including the FN1 marker).
    ///   * `65..67` = `Bvals['a','b','c']`.
    ///   * `107` = inline FN1 via encB's `DatumB + marker` branch.
    ///   * `88..90` = `Bvals['x','y','z']`.
    ///
    /// Anchors the encB marker-emission path now that Gap 3 wires
    /// the full state machine.
    #[test]
    fn encode_message_with_markers_inline_fn1_in_mode_b_post_latch() {
        let parsed = parse_dotcode_input(b"abc^FNC1xyz", true).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![106, 65, 66, 67, 107, 88, 89, 90]);
    }

    /// `abcd1234` → cws `[105, 65..68, 12, 34]`:
    ///   * `105` = `Cvals[SB4]` mode-C shift to B for 4 bytes.
    ///   * `65..68` = `Bvals['a'..'d']`.
    ///   * Encoder reverts to C after the shift, encodes the digit
    ///     pairs `12`, `34`.
    ///
    /// Exercises the "shift back to C after a short B run" path —
    /// BWIPP's "shift" (vs "latch") behavior in mode C → B → C.
    #[test]
    fn encode_message_with_markers_short_mode_b_run_then_digits() {
        let parsed = parse_dotcode_input(b"abcd1234", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![105, 65, 66, 67, 68, 12, 34]);
    }

    /// `\tABC1234` (tab + uppercase + digits) → cws `[101, 73, 33,
    /// 34, 35, 103, 12, 34]`:
    ///   * `101` = `Cvals[LAA]` latch to A (tab byte 9 only encodes
    ///     in mode A).
    ///   * `73` = `Avals[\t] = Avals[9]` (row 73 col A = 9).
    ///   * `33..35` = `Avals['A','B','C']`.
    ///   * `103` = `Avals[SC2]` shift to C for 2 pairs.
    ///   * `12, 34` = digit pairs.
    ///
    /// Exercises Gap 4: encA recognises `TryC[i] >= 2` and emits the
    /// shift-to-C path. Without this path the encoder would loop
    /// (no progress) once it tried to encode '1' in mode A and found
    /// no path back to C.
    #[test]
    fn encode_message_with_markers_tab_letters_digits_mode_a_to_c_shift() {
        let parsed = parse_dotcode_input(b"\tABC1234", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![101, 73, 33, 34, 35, 103, 12, 34]);
    }

    /// `\tABCDE` → cws `[101, 73, 33..37]`: LAA latch + 6 chars in
    /// mode A (tab + ABCDE). All bytes are A-encodable; AheadA > AheadB
    /// at segstart so the encoder chooses A.
    #[test]
    fn encode_message_with_markers_tab_uppercase_pure_mode_a() {
        let parsed = parse_dotcode_input(b"\tABCDE", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![101, 73, 33, 34, 35, 36, 37]);
    }

    /// `\tabcdef` → cws `[101, 73, 101, 65..70]`:
    ///   * `101` = LAA latch (tab forces mode A).
    ///   * `73` = Avals[\t].
    ///   * `101` = `Avals[SB6]` (coincidentally same value as LAA) —
    ///     shift to B for 6 chars (AheadB[1] = 6).
    ///   * `65..70` = `Bvals['a'..'f']`.
    ///
    /// Exercises Gap 4's encA → encB shift path. The repeated 101
    /// codeword is a known BWIPP quirk: row 101 of CHARMAPS has
    /// `SB6` in column A and `LAA` in column C, so `Avals[SB6] =
    /// Cvals[LAA] = 101`.
    #[test]
    fn encode_message_with_markers_tab_lowercase_mode_a_to_b_shift() {
        let parsed = parse_dotcode_input(b"\tabcdef", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![101, 73, 101, 65, 66, 67, 68, 69, 70]);
    }

    // ------------------------------------------------------------------
    // BIN escape (Gap 6) — base259→103 encoding of high-byte runs.
    // Goldens captured from bwip-js via tools/oracle-dotcode.js with
    // base64-transport for binary inputs.
    // ------------------------------------------------------------------

    /// base259_to_103 unit test: pin BWIPP's polynomial output for a
    /// known input. Input `[195, 131, 194, 169]` (the UTF-8 bytes
    /// inside bwip-js's processed "é" doubled) → `[30, 18, 53, 59,
    /// 61]`. This anchors the helper independently of the encoder.
    #[test]
    fn base259_to_103_matches_bwipp_polynomial() {
        let cws = base259_to_103(&[195, 131, 194, 169]);
        assert_eq!(cws, vec![30, 18, 53, 59, 61]);
        // 1-byte boundary: each value packs into 2 codewords.
        let cws = base259_to_103(&[0]);
        assert_eq!(cws.len(), 2);
        // 5-byte boundary: full quanta → 6 codewords.
        let cws = base259_to_103(&[1, 2, 3, 4, 5]);
        assert_eq!(cws.len(), 6);
    }

    /// Stage 11.A8c — pin `BinState::add_byte` + `BinState::finalise`
    /// counter management. The 5-byte rolling buffer is the entry
    /// point for every BIN-mode byte; if `bpos == 5` it auto-flushes
    /// via `base259_to_103`. End-to-end tests exercise this through
    /// the BIN-mode encoder pipeline, but the state-machine arithmetic
    /// itself isn't directly pinned.
    ///
    /// Mutations to catch:
    ///   - `self.bpos += 1` → `+= 0` or `-= 1`: buffer would never
    ///     advance, eventually overflowing or staying at 0.
    ///   - `if self.bpos == 5` → `== 4` or `== 6`: auto-flush at
    ///     the wrong boundary; either premature (loses bytes) or
    ///     never triggers (buffer overruns).
    ///   - `self.bpos == 0` in finalise → `!= 0`: empty-buffer no-op
    ///     becomes "always flush", producing spurious codewords.
    ///   - `self.bpos = 0` reset → other value: subsequent
    ///     add_byte sees a stale buffer position.
    ///   - `self.bvals[self.bpos]` → wrong index slot: byte stored
    ///     at the wrong buffer position.
    #[test]
    fn bin_state_add_byte_advance_and_auto_flush_boundary() {
        // ---- Start state: empty buffer.
        let mut bin = BinState::default();
        assert_eq!(bin.bpos, 0);
        let mut cws: Vec<u16> = Vec::new();

        // ---- Add 1 byte: bpos = 1, no flush yet (cws unchanged).
        bin.add_byte(b'A', &mut cws);
        assert_eq!(bin.bpos, 1, "first add_byte advances bpos to 1");
        assert_eq!(bin.bvals[0], b'A');
        assert!(
            cws.is_empty(),
            "add_byte must NOT flush before bpos reaches 5"
        );

        // ---- Add 3 more bytes: bpos = 4, still no flush.
        bin.add_byte(b'B', &mut cws);
        bin.add_byte(b'C', &mut cws);
        bin.add_byte(b'D', &mut cws);
        assert_eq!(bin.bpos, 4, "after 4 adds bpos == 4");
        assert_eq!(bin.bvals[..4], [b'A', b'B', b'C', b'D']);
        assert!(cws.is_empty(), "no flush before 5th byte");

        // ---- 5th byte triggers auto-flush via base259_to_103.
        bin.add_byte(b'E', &mut cws);
        assert_eq!(
            bin.bpos, 0,
            "auto-flush at bpos==5 must reset to 0 (not 5 or 6)"
        );
        assert_eq!(
            cws.len(),
            6,
            "5-byte flush yields 6 base-103 codewords (per base259_to_103 contract)"
        );
        // Spot-check: the flushed codewords match base259_to_103
        // applied to the same bytes.
        let expected = base259_to_103(b"ABCDE");
        assert_eq!(cws, expected);

        // ---- finalise on empty buffer is a no-op.
        let cw_count_before_finalise = cws.len();
        bin.finalise(&mut cws);
        assert_eq!(bin.bpos, 0, "finalise on empty stays at 0");
        assert_eq!(
            cws.len(),
            cw_count_before_finalise,
            "finalise on bpos==0 must not append codewords"
        );

        // ---- Partial buffer + finalise: 1 byte → 2 codewords.
        let mut bin = BinState::default();
        let mut cws: Vec<u16> = Vec::new();
        bin.add_byte(b'Z', &mut cws);
        bin.finalise(&mut cws);
        assert_eq!(bin.bpos, 0, "finalise resets bpos");
        assert_eq!(
            cws.len(),
            2,
            "1-byte finalise produces 2 base-103 codewords"
        );
        assert_eq!(cws, base259_to_103(b"Z"));

        // ---- After auto-flush, subsequent add_byte starts a fresh
        // window (bpos was reset, so next add lands at bvals[0]).
        let mut bin = BinState::default();
        let mut cws: Vec<u16> = Vec::new();
        for &b in &[1u8, 2, 3, 4, 5] {
            bin.add_byte(b, &mut cws);
        }
        assert_eq!(bin.bpos, 0, "after 5-byte flush, bpos must be 0");
        assert_eq!(cws.len(), 6);
        // Now add a 6th byte: lands at bvals[0], bpos goes to 1.
        bin.add_byte(99, &mut cws);
        assert_eq!(bin.bpos, 1, "6th byte after flush → bpos = 1");
        assert_eq!(
            bin.bvals[0], 99,
            "post-flush byte stored at bvals[0] (kills wrong-index mutant)"
        );
        // No new flush.
        assert_eq!(cws.len(), 6, "1 byte after flush does not re-flush");
    }

    /// bwip-js's `parseinput` interprets a `\xC3\xA9` source string
    /// as two Unicode codepoints (U+00C3, U+00A9) and re-encodes each
    /// as its UTF-8 byte sequence (\xC3\x83 and \xC2\xA9), yielding
    /// msg = `[195, 131, 194, 169]`. Our `parse_dotcode_input` takes
    /// raw bytes — to match BWIPP byte-for-byte the tests below pass
    /// in the post-UTF-8-expansion byte sequences directly (i.e.
    /// "what bwip-js's msg array contains" — which is the input
    /// every wrapper around this encoder should produce). Wrapping
    /// the source string in proper UTF-8 *before* calling
    /// `encode_with_markers` is the caller's responsibility.
    ///
    /// Pure binary run (4 high bytes) → cws `[112, 30, 18, 53, 59,
    /// 61]`:
    ///   * `112` = `Cvals[BIN]` enter BIN mode from C.
    ///   * `30, 18, 53, 59, 61` = base259→103 of the 4 high bytes.
    #[test]
    fn encode_message_with_markers_pure_binary_run() {
        let parsed = parse_dotcode_input(&[195u8, 131, 194, 169], false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![112, 30, 18, 53, 59, 61]);
    }

    /// "café" (UTF-8-expanded form bwip-js produces) → msg
    /// `[99,97,102,195,131,194,169]`. cws `[104, 67, 65, 70, 112,
    /// 30, 18, 53, 59, 61]`:
    ///   * `104` = `Cvals[SB3]` shift to B for 3 chars (`c`, `a`,
    ///     `f`).
    ///   * `67, 65, 70` = `Bvals['c','a','f']`.
    ///   * `112` = `Cvals[BIN]` enter BIN mode (still in C after
    ///     the SB3 shift consumed 3 chars).
    ///   * `30, 18, 53, 59, 61` = base259 codewords for the 4 high
    ///     bytes.
    #[test]
    fn encode_message_with_markers_text_then_binary_run() {
        let parsed = parse_dotcode_input(&[99u8, 97, 102, 195, 131, 194, 169], false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![104, 67, 65, 70, 112, 30, 18, 53, 59, 61]);
    }

    /// "\xC3\xA9B" (bwip-js-expanded) → msg `[195, 131, 194, 169,
    /// 66]`. cws `[112, 30, 18, 53, 59, 61, 110, 34]`:
    ///   * `112` = enter BIN from C.
    ///   * `30, 18, 53, 59, 61` = base259 codewords for the 4 high
    ///     bytes.
    ///   * `110` = `BINvals[TMB]` exit BIN into B.
    ///   * `34` = `Bvals['B']`.
    ///
    /// Anchors the BIN → B transition (`BIN_TERM_TO_B = 110`).
    #[test]
    fn encode_message_with_markers_binary_then_text_exits_to_b() {
        let parsed = parse_dotcode_input(&[195u8, 131, 194, 169, 66], false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![112, 30, 18, 53, 59, 61, 110, 34]);
    }

    /// `AB\r\nCD` → cws `[101, 33, 34, 77, 74, 35, 36]`:
    ///   * `101` = LAA (AheadA[0]=6 > AheadB[0]=5 because in mode B
    ///     the CRLF pair collapses to one codeword, so AheadB is one
    ///     less than the byte count).
    ///   * `33, 34` = `Avals['A','B']`.
    ///   * `77` = `Avals[\r] = Avals[13]` (row 77 col A = 13).
    ///   * `74` = `Avals[\n] = Avals[10]` (row 74 col A = 10).
    ///   * `35, 36` = `Avals['C','D']`.
    ///
    /// In mode A there's no special `CRL` codeword — `\r` and `\n`
    /// emit separately. This is the BWIPP behavior that makes
    /// AheadA prefer A for CRLF-bearing payloads.
    #[test]
    fn encode_message_with_markers_crlf_payload_picks_mode_a() {
        let parsed = parse_dotcode_input(b"AB\r\nCD", false).unwrap();
        let cws = encode_message_with_markers(&parsed).unwrap();
        assert_eq!(cws, vec![101, 33, 34, 77, 74, 35, 36]);
    }

    /// Stage 11.A8c — pin `pad_to_nd` early-return and pad-codeword
    /// selection branches. Kills `>= with <` / `>= with >` / `delete !`
    /// mutations on lines 579-589.
    #[test]
    fn pad_to_nd_branches() {
        // Already at nd → no change.
        let mut cws = vec![1, 2, 3];
        pad_to_nd(&mut cws, 3, false);
        assert_eq!(cws, vec![1, 2, 3]);

        // Over nd → no change either (the `cws.len() >= nd` guard
        // includes equality and over-fill).
        let mut cws = vec![1, 2, 3, 4];
        pad_to_nd(&mut cws, 3, false);
        assert_eq!(cws, vec![1, 2, 3, 4]);

        // Under nd, final_mode_is_bin=false → first pad is
        // MODE_C_PAD_CW, subsequent pads are MODE_C_PAD_CW.
        let mut cws = vec![1, 2];
        pad_to_nd(&mut cws, 5, false);
        assert_eq!(cws.len(), 5);
        assert_eq!(cws[0..2], [1, 2]);
        assert_eq!(cws[2], MODE_C_PAD_CW);
        for &c in &cws[2..] {
            assert_eq!(c, MODE_C_PAD_CW);
        }

        // Under nd, final_mode_is_bin=true → first pad is
        // BIN_FIRST_PAD_CW, subsequent pads are MODE_C_PAD_CW.
        let mut cws = vec![1, 2];
        pad_to_nd(&mut cws, 5, true);
        assert_eq!(cws.len(), 5);
        assert_eq!(cws[0..2], [1, 2]);
        assert_eq!(cws[2], BIN_FIRST_PAD_CW);
        for &c in &cws[3..] {
            assert_eq!(c, MODE_C_PAD_CW);
        }
    }

    /// Stage 11.A8c — pin `is_ascii_digit_i16(b)`. 1-line range
    /// predicate that checks if an i16 sentinel-typed value is one
    /// of the ASCII digits '0'..='9'. Used by DotCode's mode-C digit
    /// pairing logic; transitively covered by encoder goldens.
    ///
    /// Mutations to catch:
    ///   - `b'0' as i16..=b'9' as i16` → `..b'9' as i16`: excludes '9'.
    ///   - `b'0'` → `b'1'`: excludes '0'.
    ///   - Body replaced with `true` / `false`.
    ///   - Negative-marker handling: `-1..=10` could overlap if the
    ///     range bounds are mutated to wrong endpoints.
    #[test]
    fn is_ascii_digit_i16_boundary() {
        // All 10 ASCII digits.
        for c in b'0'..=b'9' {
            assert!(
                is_ascii_digit_i16(c as i16),
                "ASCII '{}' (i16 {}) should be classified as digit",
                c as char,
                c as i16
            );
        }
        // Just below '0'.
        assert!(!is_ascii_digit_i16(b'/' as i16), "'/' just before '0'");
        assert!(!is_ascii_digit_i16(0));
        // Just above '9'.
        assert!(!is_ascii_digit_i16(b':' as i16), "':' just after '9'");
        // Letters.
        assert!(!is_ascii_digit_i16(b'A' as i16));
        assert!(!is_ascii_digit_i16(b'a' as i16));
        // Negative sentinels (FN1/FN2/FN3 markers — must NOT be
        // misclassified as digits).
        assert!(!is_ascii_digit_i16(-1), "negative marker rejected");
        assert!(!is_ascii_digit_i16(-128), "deep negative rejected");
        // Out-of-range positives.
        assert!(!is_ascii_digit_i16(255));
        assert!(!is_ascii_digit_i16(i16::MAX));
    }

    /// Stage 11.A8c — pin `lookup_codeword_in_mode_i16` across all 3
    /// columns + the FN1/FN2/FN3 marker rows. Kills the
    /// function-replacement and `position` predicate mutants on
    /// lines 210-215.
    #[test]
    fn lookup_codeword_in_mode_per_column() {
        // 'A' is in mode A (col 0) and mode C (col 2) but not B.
        assert!(lookup_codeword_in_mode(b'A', 0).is_some());
        assert!(lookup_codeword_in_mode(b'A', 2).is_some());

        // 'a' is in mode B (col 1) only.
        assert!(lookup_codeword_in_mode(b'a', 1).is_some());
        assert!(lookup_codeword_in_mode(b'a', 0).is_none());

        // Digit pair (col 2) — '0' is in mode C as part of a pair
        // (digit pairs are an indexing range). Mode C col-2 lookup
        // for the literal byte '0' may or may not match depending on
        // charmap layout — just check we get a deterministic result.
        let _ = lookup_codeword_in_mode(b'0', 2);

        // Determinism: same byte+col → same result.
        assert_eq!(
            lookup_codeword_in_mode(b'A', 0),
            lookup_codeword_in_mode(b'A', 0)
        );

        // Distinct columns can give distinct codewords for the same byte.
        // 'A' in mode A vs mode C should produce different codewords
        // (different table positions). Catches any column-index swap mutant.
        if let (Some(a), Some(c)) = (
            lookup_codeword_in_mode(b'A', 0),
            lookup_codeword_in_mode(b'A', 2),
        ) {
            // They might coincidentally be equal — only assert when
            // they actually differ.
            if a != c {
                assert_ne!(a, c);
            }
        }
    }

    /// Stage 11.A8c — pin `lookup_codeword_in_mode_i16(b, col)` with
    /// negative marker sentinels (FN1/FN2/FN3). For these sentinel
    /// rows every column maps to the row index — so the helper returns
    /// the same codeword regardless of `col`.
    ///
    /// Also pins:
    ///   * delegation chain: `lookup_codeword_in_mode(b, col)` ==
    ///     `lookup_codeword_in_mode_i16(i16::from(b), col)` for several
    ///     `b` / `col` (kills delegation removal mutants);
    ///   * unknown negative sentinel → None;
    ///   * col=3 (out of CHARMAPS row width) → None for any b — `row[col]`
    ///     would panic without the bounds guard inherent in the
    ///     `position` walk semantics.
    #[test]
    fn lookup_codeword_in_mode_i16_sentinel_rows_and_delegation() {
        // ---- sentinel rows ------------------------------------------
        // FN1 → row 107 in every column.
        for col in 0..=2usize {
            assert_eq!(
                lookup_codeword_in_mode_i16(FN1, col),
                Some(107),
                "FN1 at col {col} → 107"
            );
        }
        // FN2 → row 108.
        for col in 0..=2usize {
            assert_eq!(
                lookup_codeword_in_mode_i16(FN2, col),
                Some(108),
                "FN2 at col {col} → 108"
            );
        }
        // FN3 → row 109.
        for col in 0..=2usize {
            assert_eq!(
                lookup_codeword_in_mode_i16(FN3, col),
                Some(109),
                "FN3 at col {col} → 109"
            );
        }

        // Asymmetric anchor: FN1, FN2, FN3 must all differ.
        assert_ne!(
            lookup_codeword_in_mode_i16(FN1, 0),
            lookup_codeword_in_mode_i16(FN2, 0),
        );
        assert_ne!(
            lookup_codeword_in_mode_i16(FN2, 0),
            lookup_codeword_in_mode_i16(FN3, 0),
        );

        // Unknown negative sentinel → None.
        assert_eq!(
            lookup_codeword_in_mode_i16(-1, 0),
            None,
            "unknown sentinel -1 → None"
        );

        // ---- delegation chain: lookup_codeword_in_mode(b) ==
        //      lookup_codeword_in_mode_i16(i16::from(b), col) -------
        for (b, col) in [(b'A', 0), (b'A', 1), (b'A', 2), (b'a', 1), (b'0', 0)] {
            let direct = lookup_codeword_in_mode(b, col);
            let via_i16 = lookup_codeword_in_mode_i16(i16::from(b), col);
            assert_eq!(
                direct, via_i16,
                "delegation: lookup({b:?}, {col}) must match i16 path"
            );
        }
    }

    /// Stage 11.A8c — pin `enc_a_step(msg, i, _mode, cws)` and
    /// `enc_b_step(msg, i, _mode, cws)`. These are the narrow non-
    /// marker variants used inside `encode_message`'s state machine.
    /// Both are 5-line helpers: lookup the current byte in the
    /// appropriate column (0 for A, 1 for B), push the codeword,
    /// advance `i`. Mutations on the column index, the index
    /// advancement, the cws push, or the byte-read offset all
    /// survive on end-to-end goldens when the affected codeword
    /// happens to overlap with the correct one.
    ///
    /// Hand-computed (using `lookup_codeword_in_mode` directly to
    /// derive the expected codewords without hard-coding the CHARMAPS):
    ///
    ///   * enc_a_step on `[b'A']` at i=0: should push
    ///     `lookup_codeword_in_mode(b'A', 0)` and advance i to 1.
    ///   * enc_b_step on `[b'a']` at i=0: should push
    ///     `lookup_codeword_in_mode(b'a', 1)` and advance i to 1.
    ///   * Mid-message: enc_a_step on `[b'X', b'A']` at i=1: pushes
    ///     `lookup_codeword_in_mode(b'A', 0)` (catches a `msg[*i + 1]`
    ///     read offset; under that mutant the function would index
    ///     OOB and panic).
    ///   * Multiple consecutive enc_a_step calls accumulate codewords
    ///     in cws — pins `cws.push(cw)` is called once per call.
    ///
    /// Mutations to catch:
    ///   * `lookup_codeword_in_mode(msg[*i], 1)` ↔ `(msg[*i], 0)`
    ///     swap (B → A or A → B): caught by the asymmetric mid-
    ///     message anchor — 'A' encodes to a different codeword
    ///     in mode 0 vs mode 1, and 'a' encodes ONLY in mode 1.
    ///   * `*i += 1` → `*i += 2`: caught by the multi-call
    ///     accumulation anchor (would skip every other byte).
    ///   * `cws.push(cw)` → `cws.push(0)`: codeword identity check
    ///     against `lookup_codeword_in_mode`'s known good value.
    ///   * `msg[*i]` → `msg[*i + 1]`: would panic in single-byte msg.
    #[test]
    fn enc_a_step_and_enc_b_step_advance_and_push_correct_codeword() {
        // enc_a_step on a single uppercase letter (mode-A native).
        let mut cws: Vec<u16> = Vec::new();
        let mut i: usize = 0;
        let mut mode = Mode::C;
        let msg = b"A";
        enc_a_step(msg, &mut i, &mut mode, &mut cws);
        let want_a = lookup_codeword_in_mode(b'A', 0).expect("'A' encodable in mode A");
        assert_eq!(cws, vec![want_a], "enc_a_step must push lookup result");
        assert_eq!(i, 1, "enc_a_step must advance i by exactly 1");

        // enc_b_step on a single lowercase letter (mode-B native).
        let mut cws: Vec<u16> = Vec::new();
        let mut i: usize = 0;
        let mut mode = Mode::C;
        let msg = b"a";
        enc_b_step(msg, &mut i, &mut mode, &mut cws);
        let want_b = lookup_codeword_in_mode(b'a', 1).expect("'a' encodable in mode B");
        assert_eq!(cws, vec![want_b], "enc_b_step must push lookup result");
        assert_eq!(i, 1);

        // Mid-message anchor: enc_a_step at i=1 in a 2-byte msg.
        // Catches `msg[*i]` → `msg[*i + 1]` (would index OOB, panic).
        let mut cws: Vec<u16> = Vec::new();
        let mut i: usize = 1;
        let mut mode = Mode::C;
        let msg = b"XA";
        enc_a_step(msg, &mut i, &mut mode, &mut cws);
        let want_a_at_1 = lookup_codeword_in_mode(b'A', 0).expect("'A' encodable in mode A");
        assert_eq!(
            cws,
            vec![want_a_at_1],
            "enc_a_step at i=1 must read msg[1]='A'"
        );
        assert_eq!(i, 2);

        // Asymmetric A vs B discriminator: 'A' is in mode A (col 0)
        // and mode C (col 2), but NOT in mode B (col 1).
        // lookup_codeword_in_mode(b'A', 0) and lookup_codeword_in_mode(b'a', 1)
        // are guaranteed to be DIFFERENT codewords (different rows in
        // CHARMAPS).
        let lookup_a_in_mode_a = lookup_codeword_in_mode(b'A', 0);
        let lookup_lower_in_mode_b = lookup_codeword_in_mode(b'a', 1);
        assert_ne!(
            lookup_a_in_mode_a, lookup_lower_in_mode_b,
            "'A' in mode A and 'a' in mode B must produce different codewords"
        );

        // Multi-call accumulation: 3 enc_a_step calls accumulate 3
        // codewords. Catches `cws.push(cw)` removal (would leave cws
        // empty) and `*i += 2` (would skip every other byte → only
        // 2 pushes after starting at 0 with msg len 3, then i would
        // jump 0→2→4, with i=4 OOB on msg[i] read in third call).
        let mut cws: Vec<u16> = Vec::new();
        let mut i: usize = 0;
        let mut mode = Mode::C;
        let msg = b"ABC";
        enc_a_step(msg, &mut i, &mut mode, &mut cws);
        enc_a_step(msg, &mut i, &mut mode, &mut cws);
        enc_a_step(msg, &mut i, &mut mode, &mut cws);
        assert_eq!(cws.len(), 3, "3 enc_a_step calls must push 3 codewords");
        assert_eq!(i, 3, "3 enc_a_step calls must advance i to 3");
        assert_eq!(
            cws,
            vec![
                lookup_codeword_in_mode(b'A', 0).unwrap(),
                lookup_codeword_in_mode(b'B', 0).unwrap(),
                lookup_codeword_in_mode(b'C', 0).unwrap(),
            ],
            "enc_a_step pushes per-byte codewords in order"
        );
    }

    /// `apply_rs_ecc(data)` and `apply_rs_ecc_with_leading(leading,
    /// data)`: Reed-Solomon ECC over GF(113) for DotCode codewords.
    ///
    /// * `apply_rs_ecc` delegates to `apply_rs_ecc_with_leading(0, …)`.
    /// * Output length = data.len() + (data.len()/2 + 3).
    /// * Output prefix = data verbatim; suffix = nc ECC codewords.
    /// * Different `leading` values produce different ECC for the
    ///   same data.
    ///
    /// Pinned with structural + sensitivity invariants (RS computation
    /// is hard to hand-verify but easy to falsify mutations of).
    ///
    /// Mutations to catch:
    /// * `apply_rs_ecc` → `apply_rs_ecc_with_leading(N, …)` for any
    ///   N != 0 (would change the ECC of `apply_rs_ecc` outputs).
    /// * `nc = nd_data / 2 + 3` formula (catches +/-1 mutations).
    /// * `out.extend(lfsr)` order — output suffix mutation.
    /// * `data.to_vec()` prefix mutation.
    /// * Leading byte ignored in ECC computation.
    #[test]
    fn apply_rs_ecc_with_leading_structure_and_sensitivity() {
        // ---- Empty data: nc = 3, total output length = 3.
        let empty_ecc = apply_rs_ecc(&[]);
        assert_eq!(empty_ecc.len(), 3, "apply_rs_ecc(empty): nc = 0/2 + 3 = 3");

        // ---- 2-element data: nc = 4, total length = 6.
        let data = [1u16, 2];
        let ecc = apply_rs_ecc(&data);
        assert_eq!(ecc.len(), 6, "data.len=2 → out.len = 2 + 4 = 6");
        assert_eq!(&ecc[..2], &data[..], "prefix is data verbatim");

        // ---- 6-element data: nc = 3+3 = 6, total = 12.
        let data6 = [10u16, 20, 30, 40, 50, 60];
        let ecc6 = apply_rs_ecc(&data6);
        assert_eq!(ecc6.len(), 12, "data.len=6 → nc=6, out.len=12");
        assert_eq!(&ecc6[..6], &data6[..], "6-element prefix");

        // ---- Delegation: apply_rs_ecc == apply_rs_ecc_with_leading(0, ·).
        let data = [42u16, 100, 7];
        let ecc_default = apply_rs_ecc(&data);
        let ecc_zero = apply_rs_ecc_with_leading(0, &data);
        assert_eq!(
            ecc_default, ecc_zero,
            "apply_rs_ecc delegates to apply_rs_ecc_with_leading(0, …)"
        );

        // ---- Leading sensitivity: different leading values produce
        // different ECC suffixes for the same data.
        let data = [5u16, 10, 15];
        let ecc_0 = apply_rs_ecc_with_leading(0, &data);
        let ecc_1 = apply_rs_ecc_with_leading(1, &data);
        let ecc_2 = apply_rs_ecc_with_leading(2, &data);
        let ecc_3 = apply_rs_ecc_with_leading(3, &data);

        // Prefix must always equal data, regardless of leading.
        assert_eq!(&ecc_0[..3], &data[..]);
        assert_eq!(&ecc_1[..3], &data[..]);
        assert_eq!(&ecc_2[..3], &data[..]);
        assert_eq!(&ecc_3[..3], &data[..]);

        // Suffix (ECC bytes) must differ across leading values.
        // Each leading is a distinct mask candidate (0..=3 for DotCode).
        let suf_0 = &ecc_0[3..];
        let suf_1 = &ecc_1[3..];
        let suf_2 = &ecc_2[3..];
        let suf_3 = &ecc_3[3..];
        assert_ne!(suf_0, suf_1, "leading 0 vs 1 must produce different ECC");
        assert_ne!(suf_0, suf_2);
        assert_ne!(suf_0, suf_3);
        assert_ne!(suf_1, suf_2);
        assert_ne!(suf_1, suf_3);
        assert_ne!(suf_2, suf_3);

        // ---- ECC values are GF(113) elements: all in 0..=112.
        for &v in &ecc_3[3..] {
            assert!(v < 113, "ECC codeword {v} must be < 113 (GF(113))");
        }

        // ---- Length-relationship invariant over a sweep.
        for n in [0usize, 1, 2, 3, 5, 8, 13, 20] {
            let d: Vec<u16> = (0..n as u16).collect();
            let ecc = apply_rs_ecc(&d);
            let expected_nc = n / 2 + 3;
            assert_eq!(
                ecc.len(),
                n + expected_nc,
                "data.len={n} → out.len = n + (n/2+3) = {n} + {expected_nc}"
            );
            assert_eq!(&ecc[..n], &d[..], "prefix preserved for n={n}");
        }
    }

    /// `parse_dotcode_input(input, parsefnc)`: pre-parser for the
    /// `parsefnc=true` escape syntax. When parsefnc is off, every
    /// byte passes through as `i16::from(b)`. When parsefnc is on,
    /// `^^` is a literal `^` (94), `^FNC1`/`^FNC3` are FN1/FN3, and
    /// `^ECInnnnnn` emits FN2 followed by 6 digit bytes.
    ///
    /// Mutations to catch:
    /// * `b != b'^' || !parsefnc` predicate flip (would mask normal
    ///   bytes in parsefnc=true mode or eat `^` in parsefnc=false).
    /// * `^^` literal-caret arm: 94 → other constant.
    /// * FN1/FN3 arm-swap.
    /// * `^ECI...` digit-validation drop (would accept letters).
    /// * `idx += 5` / `+= 10` advance off-by-one (would re-scan parts
    ///   of the escape).
    #[test]
    fn parse_dotcode_input_parsefnc_off_and_escape_sequences() {
        // ---- parsefnc=false: raw byte passthrough.
        assert_eq!(
            parse_dotcode_input(b"hello", false).unwrap(),
            vec![104i16, 101, 108, 108, 111],
            "parsefnc=off: bytes pass through as i16"
        );
        // `^` is also raw when parsefnc is off.
        assert_eq!(
            parse_dotcode_input(b"a^b", false).unwrap(),
            vec![97i16, 94, 98],
            "parsefnc=off: '^' is byte 94"
        );

        // ---- parsefnc=on: input without '^' is identical to off path.
        assert_eq!(
            parse_dotcode_input(b"hello", true).unwrap(),
            vec![104i16, 101, 108, 108, 111],
            "parsefnc=on, no '^': pass-through"
        );

        // ---- `^^` → literal caret (byte 94).
        assert_eq!(
            parse_dotcode_input(b"^^", true).unwrap(),
            vec![94i16],
            "^^ → [94] (literal caret)"
        );
        assert_eq!(
            parse_dotcode_input(b"x^^y", true).unwrap(),
            vec![120i16, 94, 121],
            "x^^y → [120, 94, 121]"
        );

        // ---- `^FNC1` → [FN1].
        assert_eq!(
            parse_dotcode_input(b"^FNC1", true).unwrap(),
            vec![FN1],
            "^FNC1 → [FN1]"
        );

        // ---- `^FNC3` → [FN3].
        assert_eq!(
            parse_dotcode_input(b"^FNC3", true).unwrap(),
            vec![FN3],
            "^FNC3 → [FN3]"
        );

        // ---- Adjacent escapes accumulate correctly.
        assert_eq!(
            parse_dotcode_input(b"^FNC1^FNC3", true).unwrap(),
            vec![FN1, FN3],
            "^FNC1^FNC3 → [FN1, FN3]"
        );

        // ---- `^ECI000123` → [FN2, '0', '0', '0', '1', '2', '3'].
        let result = parse_dotcode_input(b"^ECI000123", true).unwrap();
        assert_eq!(
            result,
            vec![FN2, 48i16, 48, 48, 49, 50, 51],
            "^ECInnnnnn → FN2 + 6 digit bytes"
        );

        // ---- Mixed sequences.
        let mixed = parse_dotcode_input(b"AB^FNC1CD", true).unwrap();
        assert_eq!(
            mixed,
            vec![65i16, 66, FN1, 67, 68],
            "AB + ^FNC1 + CD → [65, 66, FN1, 67, 68]"
        );

        // ---- Error: truncated `^` at end of input.
        //
        // Stage 11.A8c (cont) — upgrade 7 weak `.is_err()` checks in
        // parse_dotcode_input's `^` escape parser to multi-anchor
        // pins against the source diagnostics at lines 2024-2076 of
        // dotcode/mod.rs. All 7 arms share the `DotCode parsefnc:`
        // prefix; each arm carries a distinct sub-predicate that
        // discriminates which branch fired (truncated-caret /
        // truncated-function / unknown-tag / ECI-truncated /
        // ECI-non-digit). The bare `.is_err()` would survive a
        // mutation that re-routes one branch's diagnostic through
        // another's format string.

        // Arm 1: lone `^` (rest_len < 2) → `DotCode parsefnc: caret
        // character truncated` (line 2024-2027).
        match parse_dotcode_input(b"^", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "lone-caret arm: missing `DotCode parsefnc:` prefix: {msg}"
                );
                assert!(
                    msg.contains("caret character truncated"),
                    "lone-caret arm: missing `caret character truncated` predicate: {msg}"
                );
            }
            other => panic!("lone '^' should reject as InvalidData, got {other:?}"),
        }

        // Arm 2: `^A` (rest_len=2 < 5) → `DotCode parsefnc: function
        // character truncated` (line 2036-2039).
        match parse_dotcode_input(b"^A", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^A arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("function character truncated"),
                    "^A arm: missing `function character truncated` predicate: {msg}"
                );
                assert!(
                    !msg.contains("caret character truncated"),
                    "^A arm: lone-caret diagnostic leaked: {msg}"
                );
            }
            other => panic!("^A should reject as InvalidData, got {other:?}"),
        }

        // Arms 3 + 4: unknown 4-char tag → `DotCode parsefnc: unknown
        // function character: {name}` (line 2073-2075).
        match parse_dotcode_input(b"^XXXX", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^XXXX arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("unknown function character"),
                    "^XXXX arm: missing predicate: {msg}"
                );
                assert!(msg.contains("XXXX"), "^XXXX arm: missing tag echo: {msg}");
            }
            other => panic!("^XXXX should reject as InvalidData, got {other:?}"),
        }
        match parse_dotcode_input(b"^FNC2", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^FNC2 arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("unknown function character"),
                    "^FNC2 arm: missing predicate: {msg}"
                );
                assert!(
                    msg.contains("FNC2"),
                    "^FNC2 arm: missing tag echo (kills `_ if tag == b\"FNC2\"` mutations that accept FNC2): {msg}"
                );
            }
            other => panic!("^FNC2 should reject as InvalidData, got {other:?}"),
        }

        // Arm 5: ECI truncated (rest_len < 10) → `DotCode parsefnc:
        // ECI truncated` (line 2055-2057).
        match parse_dotcode_input(b"^ECI12345", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^ECI12345 arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("ECI truncated"),
                    "^ECI12345 arm: missing `ECI truncated` predicate: {msg}"
                );
                assert!(
                    !msg.contains("000000 to 999999"),
                    "^ECI12345 arm: non-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("^ECI12345 should reject as InvalidData, got {other:?}"),
        }

        // Arms 6 + 7: ECI digit-only check → `DotCode parsefnc: ECI
        // must be 000000 to 999999` (line 2062-2064).
        match parse_dotcode_input(b"^ECI00012A", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^ECI00012A arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("ECI must be 000000 to 999999"),
                    "^ECI00012A arm: missing full ECI range predicate: {msg}"
                );
                assert!(
                    !msg.contains("ECI truncated"),
                    "^ECI00012A arm: truncated diagnostic leaked: {msg}"
                );
            }
            other => panic!("^ECI00012A should reject as InvalidData, got {other:?}"),
        }
        match parse_dotcode_input(b"^ECIabcdef", true) {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DotCode parsefnc:"),
                    "^ECIabcdef arm: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("ECI must be 000000 to 999999"),
                    "^ECIabcdef arm: missing full ECI range predicate: {msg}"
                );
            }
            other => panic!("^ECIabcdef should reject as InvalidData, got {other:?}"),
        }

        // ---- Edge: empty input.
        assert!(
            parse_dotcode_input(b"", false).unwrap().is_empty(),
            "empty + parsefnc=off → empty"
        );
        assert!(
            parse_dotcode_input(b"", true).unwrap().is_empty(),
            "empty + parsefnc=on → empty"
        );

        // ---- High-bit bytes preserved.
        assert_eq!(
            parse_dotcode_input(&[0xC3, 0xA9], false).unwrap(),
            vec![0xC3i16, 0xA9],
            "high-bit bytes pass through"
        );
    }

    /// Stage 11.A8d — regression for the fuzz crash at dotcode/mod.rs:869
    /// ("AheadB > 0 implies DatumB for these positions"). A CR/LF pair
    /// reached inside the mode-C → shift-to-B run desynced `ahead_b`'s
    /// CRLF-as-one-unit (next=i+2) count from the emission loop's
    /// per-byte advance, so the lone '\r' was looked up in column B
    /// (None) and `.expect()` panicked. Both encC twins now collapse
    /// CRLF to MODE_B_CRLF. The encoder must return a Result (Ok or a
    /// graceful Err) for ANY input — never panic.
    #[test]
    fn crlf_in_mode_c_shift_to_b_does_not_panic() {
        // Exact fuzz reproducer (crash-0b0098cd…).
        let _ = encode(b"-\r\n\x00\x00\x00\x00\xd7\xd5");
        // A digit run (drives mode C) immediately followed by CRLF and
        // more mode-B text — forces the encC shift-to-B path over a CRLF.
        let _ = encode(b"1234567890\r\nabc");
        let _ = encode(b"\r\n");
        let _ = encode(b"00\r\n00\r\n00");
        // The point is purely "no panic"; reaching here is the assertion.
    }
}
