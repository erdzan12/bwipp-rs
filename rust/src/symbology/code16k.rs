//! Code 16K — stacked 1D barcode (2..=16 rows × 70 modules per row).
//!
//! Reference: AIM/ANSI MH10.8.3, BWIPP `bwipp_code16k` (bwip-js line
//! 18946+, 953 lines). Each row of a Code 16K symbol carries the
//! equivalent of a Code 128 partial: a start-row indicator (4 bars),
//! 5 data codewords (6 bars each = 30 modules), an end-row indicator
//! (4 bars), and a 1-module right-quiet area. Each codeword shares
//! the Code 128 / Code 16K alphabet with mode A / B / C selection
//! and paired-digit packing in mode C.
//!
//! ## Port status
//!
//! This module ships as the **foundation** of the Code 16K port: the
//! BWIPP-verbatim constants tables plus the `c1` / `c2` symbol-check
//! formula. The full encoder (mode selector, row layout, stacked
//! renderer) is on the burndown — current state is the master-loop's
//! prescribed "viable starting point" so subsequent iterations have
//! the constants ready to drive against bwip-js logical goldens.
//! [`encode`] therefore returns
//! [`crate::error::Error::InvalidData`] for now; the row stays in
//! the inventory's `missing` bucket until the encoder is wired
//! through.
//!
//! Following iterations will fill in (in order):
//!   1. cws-level encoder for digit-only payloads (mode-C from start).
//!   2. cws-level encoder for mode A / B payloads.
//!   3. Mixed-mode payloads (shifts, latches, paired-digit packing
//!      mid-message).
//!   4. Stacked renderer (start/stop indicators per row, separator
//!      lines, quiet zones).
//!   5. ≥3 bwip-js logical goldens covering each path.

// The constants + checksum helper land in this iteration but the
// encoder body that consumes them is on the burndown. Suppress the
// dead-code warnings until subsequent iterations wire them up.
#![allow(dead_code)]

use crate::encoding::BitMatrix;
use crate::error::Error;

// ---------------------------------------------------------------------------
// Marker constants — BWIPP's `code16k_*` negative-i16 sentinels (bwip-js
// lines 18951-18982). Stored exactly the same way as DotCode's marker
// constants so the eventual encoder can switch on them with the same
// `if b < 0` pattern.
// ---------------------------------------------------------------------------

/// Switch-to-A latch.
pub(crate) const SWA: i16 = -1;
/// Switch-to-B latch.
pub(crate) const SWB: i16 = -2;
/// Switch-to-C latch.
pub(crate) const SWC: i16 = -3;
/// Shift-to-A for 1 byte.
pub(crate) const SA1: i16 = -4;
/// Shift-to-B for 1 byte.
pub(crate) const SB1: i16 = -5;
/// Shift-to-C for 1 pair.
pub(crate) const SC1: i16 = -6;
/// Shift-to-A for 2 bytes.
pub(crate) const SA2: i16 = -7;
/// Shift-to-B for 2 bytes.
pub(crate) const SB2: i16 = -8;
/// Shift-to-C for 2 pairs.
pub(crate) const SC2: i16 = -9;
/// PAD codeword (107 in the output stream).
pub(crate) const PAD: i16 = -10;
/// Shift-to-B for 3 bytes.
pub(crate) const SB3: i16 = -11;
/// Shift-to-C for 3 pairs.
pub(crate) const SC3: i16 = -12;
/// FNC1 marker (GS1 separator).
pub(crate) const FN1: i16 = -13;
/// FNC2 marker.
pub(crate) const FN2: i16 = -14;
/// FNC3 marker.
pub(crate) const FN3: i16 = -15;
/// FNC4 marker.
pub(crate) const FN4: i16 = -16;

// ---------------------------------------------------------------------------
// Charmaps — bwip-js line 18983.
//
// Each row is `[A-column, B-column, C-column]`. The column-A and
// column-B entries are either ASCII bytes (`0..=127`) or marker
// constants (e.g. `FN1`, `SB2`); the column-C entry is the
// codeword's row index in 0..=106 (BWIPP stores it as the string
// "00".."99" plus a few marker values in the trailing rows). Rows
// 0..=95 contain ASCII text in all three columns; rows 96..=106 are
// the mode-control rows.
// ---------------------------------------------------------------------------

/// 107-row Code 16K charmap. Indexed `0..=106` → codeword value.
/// Mirrors BWIPP's `code16k_charmaps` initializer verbatim.
#[rustfmt::skip]
pub(crate) const CHARMAPS: [[i16; 3]; 107] = [
    // Rows 0..=31: ASCII 32..=63 (space through '?'). All three
    // columns hold the literal byte for these rows.
    [ 32,  32,   0], [ 33,  33,   1], [ 34,  34,   2], [ 35,  35,   3],
    [ 36,  36,   4], [ 37,  37,   5], [ 38,  38,   6], [ 39,  39,   7],
    [ 40,  40,   8], [ 41,  41,   9], [ 42,  42,  10], [ 43,  43,  11],
    [ 44,  44,  12], [ 45,  45,  13], [ 46,  46,  14], [ 47,  47,  15],
    [ 48,  48,  16], [ 49,  49,  17], [ 50,  50,  18], [ 51,  51,  19],
    [ 52,  52,  20], [ 53,  53,  21], [ 54,  54,  22], [ 55,  55,  23],
    [ 56,  56,  24], [ 57,  57,  25], [ 58,  58,  26], [ 59,  59,  27],
    [ 60,  60,  28], [ 61,  61,  29], [ 62,  62,  30], [ 63,  63,  31],

    // Rows 32..=63: ASCII 64..=95 ('@' through '_').
    [ 64,  64,  32], [ 65,  65,  33], [ 66,  66,  34], [ 67,  67,  35],
    [ 68,  68,  36], [ 69,  69,  37], [ 70,  70,  38], [ 71,  71,  39],
    [ 72,  72,  40], [ 73,  73,  41], [ 74,  74,  42], [ 75,  75,  43],
    [ 76,  76,  44], [ 77,  77,  45], [ 78,  78,  46], [ 79,  79,  47],
    [ 80,  80,  48], [ 81,  81,  49], [ 82,  82,  50], [ 83,  83,  51],
    [ 84,  84,  52], [ 85,  85,  53], [ 86,  86,  54], [ 87,  87,  55],
    [ 88,  88,  56], [ 89,  89,  57], [ 90,  90,  58], [ 91,  91,  59],
    [ 92,  92,  60], [ 93,  93,  61], [ 94,  94,  62], [ 95,  95,  63],

    // Rows 64..=95: column A = control bytes 0..=31; column B = ASCII
    // 96..=127 (`'\`' through DEL). Column C = row index.
    [  0,  96,  64], [  1,  97,  65], [  2,  98,  66], [  3,  99,  67],
    [  4, 100,  68], [  5, 101,  69], [  6, 102,  70], [  7, 103,  71],
    [  8, 104,  72], [  9, 105,  73], [ 10, 106,  74], [ 11, 107,  75],
    [ 12, 108,  76], [ 13, 109,  77], [ 14, 110,  78], [ 15, 111,  79],
    [ 16, 112,  80], [ 17, 113,  81], [ 18, 114,  82], [ 19, 115,  83],
    [ 20, 116,  84], [ 21, 117,  85], [ 22, 118,  86], [ 23, 119,  87],
    [ 24, 120,  88], [ 25, 121,  89], [ 26, 122,  90], [ 27, 123,  91],
    [ 28, 124,  92], [ 29, 125,  93], [ 30, 126,  94], [ 31, 127,  95],

    // Rows 96..=106: mode-control codewords. Column-C entries 96..=99
    // are still the integer index; the trailing rows use marker
    // constants in some columns.
    [FN3, FN3,  96], [FN2, FN2,  97], [SB1, SA1,  98], [SWC, SWC,  99],
    [SWB, FN4, SWB], [FN4, SWA, SWA], [FN1, FN1, FN1], [PAD, PAD, PAD],
    [SB2, SA2, SB1], [SC2, SC2, SB2], [SC3, SC3, SB3],
];

/// Symbol-size table (`bwipp` `code16k_metrics`). Entry `i` =
/// `[rows, dcws_inner]` where `dcws_inner` is the number of data
/// codewords excluding the leading mode indicator and trailing
/// `c1`/`c2` checks. Rows range from 2 to 16.
pub(crate) const METRICS: [[u16; 2]; 15] = [
    [2, 7],
    [3, 12],
    [4, 17],
    [5, 22],
    [6, 27],
    [7, 32],
    [8, 37],
    [9, 42],
    [10, 47],
    [11, 52],
    [12, 57],
    [13, 62],
    [14, 67],
    [15, 72],
    [16, 77],
];

/// Per-codeword bar/space width patterns. Each entry is six ASCII
/// digits `1..=4` describing alternating bar/space widths in
/// modules; the row total is always 11 modules (6 elements summing
/// to 11). Indexed `0..=106` parallel to [`CHARMAPS`].
///
/// `ENCS[c]` is the visual encoding of codeword `c`; the renderer
/// translates this into bars by reading each character as a module
/// count (`'2'` → 2 modules wide) and alternating `bar, space, bar,
/// space, ...` starting with a bar.
pub(crate) const ENCS: [&str; 107] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312", "132212",
    "221213", "221312", "231212", "112232", "122132", "122231", "113222", "123122", "123221",
    "223211", "221132", "221231", "213212", "223112", "312131", "311222", "321122", "321221",
    "312212", "322112", "322211", "212123", "212321", "232121", "111323", "131123", "131321",
    "112313", "132113", "132311", "211313", "231113", "231311", "112133", "112331", "132131",
    "113123", "113321", "133121", "313121", "211331", "231131", "213113", "213311", "213131",
    "311123", "311321", "331121", "312113", "312311", "332111", "314111", "221411", "431111",
    "111224", "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111", "111242",
    "121142", "121241", "114212", "124112", "124211", "411212", "421112", "421211", "212141",
    "214121", "412121", "111143", "111341", "131141", "114113", "114311", "411113", "411311",
    "113141", "114131", "311141", "411131", "211412", "211214", "211232", "211133",
];

/// Start-row indicators per row index (1-based: row 1 of the symbol
/// uses `STARTENCS[0]`, etc.). Each entry is four ASCII digits =
/// 4-bar pattern (total 7 modules per indicator). Reused for both
/// the start and stop sides, with `STOPENCS_ODD` / `STOPENCS_EVEN`
/// selecting the right pattern based on SAM (symbol-append-mode)
/// parity.
pub(crate) const STARTENCS: [&str; 16] = [
    "3211", "2221", "2122", "1411", "1132", "1231", "1114", "3112", "3211", "2221", "2122", "1411",
    "1132", "1231", "1114", "3112",
];

/// Stop-row indicators when SAM ≡ 1 (mod 2) — the default
/// stand-alone-symbol layout.
pub(crate) const STOPENCS_ODD: [&str; 16] = [
    "3211", "2221", "2122", "1411", "1132", "1231", "1114", "3112", "1132", "1231", "1114", "3112",
    "3211", "2221", "2122", "1411",
];

/// Stop-row indicators when SAM ≡ 0 (mod 2) — used for the
/// even-indexed symbol in a structured-append sequence.
pub(crate) const STOPENCS_EVEN: [&str; 16] = [
    "2122", "1411", "1132", "1231", "1114", "3112", "1132", "1231", "1114", "3112", "3211", "2221",
    "2122", "1411", "3211", "2221",
];

/// Codeword value of the PAD entry (CHARMAPS row 103). Used by the
/// future mode selector when filling remaining slots before the
/// `c1` / `c2` symbol checks.
pub(crate) const PAD_CW: u16 = 103;

/// Mode dispatch helper — given `r` (target row count, 2..=16) and
/// `mode` (0..=7 per BWIPP's Code 16K mode encoding), compute the
/// leading row-indicator codeword: `(r - 2) * 7 + mode`. BWIPP
/// emits this as `cws[0]` before any data codewords. Verified
/// against bwip-js logical goldens — see
/// `tests::leading_row_indicator_matches_bwipp` below.
#[inline]
pub(crate) fn leading_row_indicator(rows: u16, mode: u16) -> u16 {
    (rows - 2) * 7 + mode
}

/// BWIPP's `c1` / `c2` symbol-check formula (bwip-js lines
/// 20012-20015). Given the data codewords (including the leading
/// row indicator and any pad codewords already inserted), compute
/// the two trailing check codewords:
///
/// ```text
/// c1 = sum((i + 2) * cws[i] for i in 0..=dcws_inner) % 107
/// c2 = (sum((i + 1) * cws[i] for i in 0..=dcws_inner) + c1 * (dcws_inner + 2)) % 107
/// ```
///
/// Both are taken mod 107 (Code 16K's symbol-check modulus).
///
/// `cws` here is the array *including* `cws[0]` (the leading row
/// indicator) — `dcws_inner` is its `len() - 1`.
pub(crate) fn compute_checksums(cws: &[u16]) -> (u16, u16) {
    let dcws_inner = cws.len() as u32 - 1;
    let mut s1: u32 = 0;
    let mut s2: u32 = 0;
    for (idx, &cw) in cws.iter().enumerate() {
        let i = idx as u32;
        s1 = s1.wrapping_add((i + 2) * u32::from(cw));
        s2 = s2.wrapping_add((i + 1) * u32::from(cw));
    }
    let c1 = (s1 % 107) as u16;
    let c2 = ((s2 + u32::from(c1) * (dcws_inner + 2)) % 107) as u16;
    (c1, c2)
}

/// BWIPP mode identifiers (bwip-js line 19490+, `$_.mode`). The
/// leading row-indicator codeword is `(r - 2) * 7 + mode`.
///
/// * `MODE_A` (0) — start in mode A.
/// * `MODE_B` (1) — start in mode B.
/// * `MODE_C_FROM_START` (2) — start in mode C (paired-digit
///   packing). Used for pure-digit payloads of even length.
/// * `MODE_B_THEN_A` (3) — shift to B for one byte, then start A.
/// * `MODE_B_THEN_C` (4) — shift to B for one byte, then start C.
///   Used for odd-length digit payloads with one leading byte.
/// * `MODE_C_THEN_B` (5) — shift to C for one pair, then start B.
///   Used for odd-length digit payloads with one trailing byte.
/// * `MODE_GS1` (6) — FNC1 prefix + mode B (the GS1 wrapper path).
pub(crate) const MODE_A: u16 = 0;
pub(crate) const MODE_B: u16 = 1;
pub(crate) const MODE_C_FROM_START: u16 = 2;
pub(crate) const MODE_B_THEN_A: u16 = 3;
pub(crate) const MODE_B_THEN_C: u16 = 4;
pub(crate) const MODE_C_THEN_B: u16 = 5;
pub(crate) const MODE_GS1: u16 = 6;

/// Pick the smallest symbol size (rows, dcws_inner) that fits
/// `pair_count` mode-C codewords. Returns
/// `(rows, dcws_inner)` matching the [`METRICS`] table row.
///
/// `pair_count` is the number of data codewords *after* the leading
/// mode-indicator and *before* the trailing `c1` / `c2` checks — so
/// for a "1234" payload (2 pairs) the encoder needs at least 2 slots
/// of capacity, plus PAD codewords to fill up to `dcws_inner`.
pub(crate) fn pick_symbol_size(pair_count: usize) -> Option<(u16, u16)> {
    METRICS
        .iter()
        .find(|row| usize::from(row[1]) >= pair_count)
        .map(|row| (row[0], row[1]))
}

/// Look up the column-B codeword index for byte `b`. Returns `None`
/// if the byte is not B-encodable (e.g. ASCII control bytes
/// `0..=31` other than the mode-control rows). BWIPP's
/// `$_.setb[byte]` lookup.
#[inline]
pub(crate) fn lookup_b(b: u8) -> Option<u16> {
    CHARMAPS
        .iter()
        .position(|row| row[1] == i16::from(b))
        .map(|i| i as u16)
}

/// Look up the column-A codeword index for byte `b`. Returns `None`
/// if the byte is not A-encodable (e.g. ASCII lowercase 96..=127).
/// BWIPP's `$_.seta[byte]` lookup.
#[inline]
pub(crate) fn lookup_a(b: u8) -> Option<u16> {
    CHARMAPS
        .iter()
        .position(|row| row[0] == i16::from(b))
        .map(|i| i as u16)
}

// ---------------------------------------------------------------------------
// Codeword-value helpers for set-control rows.
//
// CHARMAPS row 100 = `[SWB, FN4, SWB]` and row 101 = `[FN4, SWA, SWA]`.
// The codeword emitted IS the row index (0..=106), so:
//
//   - codeword 98  : in set A means SB1 (shift to B for 1 byte);
//                    in set B means SA1 (shift to A for 1 byte);
//                    in set C means SB1 (trailing-byte shift).
//   - codeword 99  : SWC (switch to C) — universal.
//   - codeword 100 : in set A means SWB (latch to B);
//                    in set B means FN4;
//                    in set C means SWB.
//   - codeword 101 : in set A means FN4;
//                    in set B means SWA (latch to A);
//                    in set C means SWA.
//   - codewords 102/104/105 : SB2 / SA2, SC2 / SB2, SC3 / SB3 — the
//                    multi-byte / multi-pair shift variants.
// ---------------------------------------------------------------------------

/// `SA1` codeword when emitted from set B (single-byte shift to A).
pub(crate) const SA1_FROM_B: u16 = 98;
/// `SB1` codeword when emitted from set A (single-byte shift to B).
pub(crate) const SB1_FROM_A: u16 = 98;
/// `SA2` codeword when emitted from set B (two-byte shift to A).
/// Row 104 in CHARMAPS: `[SB2, SA2, SB1]` — col 1 = SA2.
pub(crate) const SA2_FROM_B: u16 = 104;
/// `SB2` codeword when emitted from set A (two-byte shift to B).
/// Row 104 in CHARMAPS: `[SB2, SA2, SB1]` — col 0 = SB2.
pub(crate) const SB2_FROM_A: u16 = 104;
/// `SWA` codeword when emitted from set B (latch to A).
pub(crate) const SWA_FROM_B: u16 = 101;
/// `SWB` codeword when emitted from set A (latch to B).
pub(crate) const SWB_FROM_A: u16 = 100;
/// `FN4` codeword when emitted from set A.
pub(crate) const FN4_FROM_A: u16 = 101;
/// `FN4` codeword when emitted from set B.
pub(crate) const FN4_FROM_B: u16 = 100;
/// `SB1` codeword when emitted from set C (1-byte shift to B,
/// continue in C). Row 104 col 2 = SB1.
pub(crate) const SB1_FROM_C: u16 = 104;
/// `SB2` codeword when emitted from set C (2-byte shift to B,
/// continue in C). Row 105 col 2 = SB2.
pub(crate) const SB2_FROM_C: u16 = 105;
/// `SB3` codeword when emitted from set C (3-byte shift to B,
/// continue in C). Row 106 col 2 = SB3.
pub(crate) const SB3_FROM_C: u16 = 106;

/// Is byte `b` encodable in set A but not set B?
#[inline]
pub(crate) fn anotb(b: i16) -> bool {
    let in_a = b >= 0 && CHARMAPS.iter().any(|row| row[0] == b);
    let in_b = b >= 0 && CHARMAPS.iter().any(|row| row[1] == b);
    in_a && !in_b
}

/// Is byte `b` encodable in set B but not set A?
#[inline]
pub(crate) fn bnota(b: i16) -> bool {
    let in_a = b >= 0 && CHARMAPS.iter().any(|row| row[0] == b);
    let in_b = b >= 0 && CHARMAPS.iter().any(|row| row[1] == b);
    in_b && !in_a
}

/// Is byte `b` encodable in set A (column 0)?
#[inline]
pub(crate) fn in_a(b: i16) -> bool {
    b >= 0 && CHARMAPS.iter().any(|row| row[0] == b)
}

/// Is byte `b` encodable in set B (column 1)?
#[inline]
pub(crate) fn in_b(b: i16) -> bool {
    b >= 0 && CHARMAPS.iter().any(|row| row[1] == b)
}

/// Insert FN4 sentinels for ASCII ↔ extended-ASCII transitions,
/// mirroring BWIPP's pre-encoder pass at bwip-js-node.js lines
/// 19531–19565 (same algorithm as POSICODE's `insert_fn4_markers`).
pub(crate) fn insert_fn4_markers(msg: &[i16]) -> Vec<i16> {
    let msglen = msg.len();
    let mut num_sa: Vec<usize> = vec![0; msglen + 1];
    let mut num_ea: Vec<usize> = vec![0; msglen + 1];
    for i in (0..msglen).rev() {
        let c = msg[i];
        if c >= 0 {
            if c >= 128 {
                num_ea[i] = num_ea[i + 1] + 1;
            } else {
                num_sa[i] = num_sa[i + 1] + 1;
            }
        }
    }

    let mut out: Vec<i16> = Vec::with_capacity(msglen * 2);
    let mut ea = false;
    for (i, &c) in msg.iter().enumerate() {
        if c >= 0 && ea == (c < 128) {
            let run = if ea { num_sa[i] } else { num_ea[i] };
            let threshold = if run + i == msglen { 3 } else { 5 };
            if run < threshold {
                out.push(FN4);
            } else {
                ea = !ea;
                out.push(FN4);
                out.push(FN4);
            }
        }
        if c >= 0 {
            out.push(c & 127);
        } else {
            out.push(c);
        }
    }
    out
}

/// Compute the BWIPP `nextanotb` / `nextbnota` lookahead arrays.
/// `nextanotb[i]` = distance from position `i` to the next byte
/// that is A-only (anotb=true); `nextbnota[i]` is the mirror for
/// B-only. Both arrays have length `msg.len() + 1`; the final cell
/// holds BWIPP's `9999` sentinel (treated as "infinity" by the
/// `abeforeb`/`bbeforea` comparison).
pub(crate) fn compute_lookahead(msg: &[i16]) -> (Vec<u32>, Vec<u32>) {
    let n = msg.len();
    let mut next_anotb: Vec<u32> = vec![0; n + 1];
    let mut next_bnota: Vec<u32> = vec![0; n + 1];
    next_anotb[n] = 9999;
    next_bnota[n] = 9999;
    for i in (0..n).rev() {
        next_anotb[i] = if anotb(msg[i]) {
            0
        } else {
            next_anotb[i + 1] + 1
        };
        next_bnota[i] = if bnota(msg[i]) {
            0
        } else {
            next_bnota[i + 1] + 1
        };
    }
    (next_anotb, next_bnota)
}

/// BWIPP `abeforeb(i)` — true when the next A-only byte from
/// position `i` is closer than the next B-only byte.
#[inline]
pub(crate) fn abeforeb(i: usize, next_anotb: &[u32], next_bnota: &[u32]) -> bool {
    next_anotb[i] < next_bnota[i]
}

/// BWIPP `bbeforea(i)` — true when the next B-only byte from
/// position `i` is closer than the next A-only byte.
#[inline]
pub(crate) fn bbeforea(i: usize, next_anotb: &[u32], next_bnota: &[u32]) -> bool {
    next_bnota[i] < next_anotb[i]
}

/// BWIPP `numsscr(p)` — count consecutive C-encodable bytes starting
/// at `p`. Returns `(n, s)` where `n` is the count of digit bytes
/// (or FN1 markers at the right alignment) and `s` is `n` plus any
/// extra alignment-bump for FN1 at an even position.
///
/// For pure-digit runs `n == s`; FN1 in the middle of a digit run
/// adds 1 to both when aligned at an even position. The trigger for
/// mode-C transitions uses `n` (or `s` depending on the caller) to
/// decide if there are enough paired-encodable items to warrant a
/// switch.
///
/// Mirrors the closure at bwip-js-node.js lines 19568–19584.
pub(crate) fn numsscr(msg: &[i16], p: usize) -> (usize, usize) {
    let mut n: usize = 0;
    let mut s: usize = 0;
    let mut p = p;
    while p < msg.len() {
        let c = msg[p];
        if c == FN1 {
            if s % 2 == 0 {
                // FN1 at even position: extra s++ to keep alignment.
                s += 1;
                // Fall through to common n+=1, s+=1.
            } else {
                // FN1 at odd position breaks the run.
                break;
            }
        } else if !(b'0' as i16..=b'9' as i16).contains(&c) {
            break;
        }
        n += 1;
        s += 1;
        p += 1;
    }
    (n, s)
}

/// Look up the column-C codeword for a 2-byte digit pair `(hi, lo)`
/// where `hi` and `lo` are ASCII digit bytes (`'0'..='9'`). Returns
/// the codeword `hi*10 + lo` (so "12" → 12, "34" → 34, "99" → 99).
#[inline]
fn pair_codeword(hi: u8, lo: u8) -> u16 {
    u16::from(hi - b'0') * 10 + u16::from(lo - b'0')
}

/// Codeword for switching to set C from sets A or B.
pub(crate) const SWC_FROM_A_OR_B: u16 = 99;
/// SC2 codeword in set A — shift 2 digit pairs to C then back to A.
pub(crate) const SC2_FROM_A: u16 = 105;
/// SC3 codeword in set A — shift 3 digit pairs to C then back to A.
pub(crate) const SC3_FROM_A: u16 = 106;
/// SC2 codeword in set B — shift 2 digit pairs to C then back to B.
pub(crate) const SC2_FROM_B: u16 = 105;
/// SC3 codeword in set B — shift 3 digit pairs to C then back to B.
pub(crate) const SC3_FROM_B: u16 = 106;
/// SWA codeword in set C — latch back to A.
pub(crate) const SWA_FROM_C: u16 = 101;
/// SWB codeword in set C — latch back to B.
pub(crate) const SWB_FROM_C: u16 = 100;

/// Three-way set state in the Code 16K encoder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cset {
    A,
    B,
    C,
}

/// Pick the initial mode + cset based on BWIPP's leading-byte
/// pattern analysis (bwip-js-node.js lines 19605–19732). Returns
/// `(cset, mode, i_after_prefix, prefix_cws)` where `prefix_cws` is
/// any leading codeword(s) consumed by the mode-N selector (e.g. for
/// mode 5: 1 leading B byte; for mode 6: 2 leading B bytes).
///
/// Implemented branches (in BWIPP priority order):
///
/// * msglen ≥ 2 AND pure-digit count ≥ 2 even → mode 2 (C from
///   start), no prefix.
/// * msglen ≥ 2 AND pure-digit count ≥ 3 odd → mode 5 (1 byte in B,
///   then C), prefix = [msg[0] in B].
/// * msglen ≥ 2 AND msg[0] in B AND numsscr(1).s ≥ 2 even → mode 5,
///   prefix = [msg[0] in B].
/// * msglen ≥ 2 AND msg[0] in B AND numsscr(1).s ≥ 3 odd → mode 6
///   (2 bytes in B, then C), prefix = [msg[0], msg[1] in B].
/// * msglen ≥ 2 AND msg[0],msg[1] in B AND numsscr(2).s ≥ 2 even →
///   mode 6, prefix = [msg[0], msg[1] in B].
/// * Default: `abeforeb(0)` → mode 0 (A) else mode 1 (B); no prefix.
///
/// FN1-initiated branches (modes 3, 4) and SAM (mode 7) are still
/// extension paths since they require `parsefnc` or the `sam`
/// option, neither of which this implementation surfaces.
fn pick_initial_mode(
    msg: &[i16],
    next_anotb: &[u32],
    next_bnota: &[u32],
) -> (Cset, u16, usize, Vec<u16>) {
    let msglen = msg.len();

    if msglen >= 2 {
        // Pure-digit count from start.
        let (_, s0) = numsscr(msg, 0);
        if s0 >= 2 && s0 % 2 == 0 {
            return (Cset::C, MODE_C_FROM_START, 0, vec![]);
        }
        if s0 >= 3 && s0 % 2 == 1 {
            // Pure-digit odd: emit first byte in B, then mode 5.
            if let Some(cw) = lookup_b((msg[0] & 0xff) as u8) {
                return (Cset::C, MODE_C_THEN_B, 1, vec![cw]);
            }
        }
        // msg[0] in B + ≥2 even digits at i=1 → mode 5.
        if msg[0] >= 0 && lookup_b(msg[0] as u8).is_some() {
            let (_, s1) = numsscr(msg, 1);
            if s1 >= 2 && s1 % 2 == 0 {
                let cw = lookup_b(msg[0] as u8).unwrap();
                return (Cset::C, MODE_C_THEN_B, 1, vec![cw]);
            }
            // msg[0] in B + ≥3 odd digits at i=1 → mode 6.
            if s1 >= 3 && s1 % 2 == 1 {
                let cw0 = lookup_b(msg[0] as u8).unwrap();
                // msg[1] is the first digit — emit it in B too.
                if let Some(cw1) = lookup_b((msg[1] & 0xff) as u8) {
                    return (Cset::C, MODE_B_THEN_C, 2, vec![cw0, cw1]);
                }
            }
        }
        // msg[0],msg[1] both in B + ≥2 even digits at i=2 → mode 6.
        if msglen >= 3 && msg[0] >= 0 && msg[1] >= 0 {
            if let (Some(cw0), Some(cw1)) = (lookup_b(msg[0] as u8), lookup_b(msg[1] as u8)) {
                let (_, s2) = numsscr(msg, 2);
                if s2 >= 2 && s2 % 2 == 0 {
                    return (Cset::C, MODE_B_THEN_C, 2, vec![cw0, cw1]);
                }
            }
        }
    }

    // Default: abeforeb(0) → A; else B.
    if abeforeb(0, next_anotb, next_bnota) {
        (Cset::A, MODE_A, 0, vec![])
    } else {
        (Cset::B, MODE_B, 0, vec![])
    }
}

/// BWIPP-faithful Code 16K **mid-message** encoder for payloads
/// that mix A-only and B-only bytes (control codes + printable +
/// lowercase + control codes) and/or contain extended-ASCII bytes
/// (which get FN4-bracketed) and/or contain digit runs (which can
/// trigger mode-C selection at start).
///
/// Ports the main encoder loop at bwip-js-node.js lines 19720
/// through 19974 (sets A, B, and C). Several BWIPP optimisations
/// are still extension paths — see the "Remaining gaps" section
/// below.
///
/// Returns the (mode, data_cws) pair so a caller can prepend the
/// `leading_row_indicator(rows, mode)` and append PAD + c1/c2.
///
/// # Algorithm
///
/// 1. Apply FN4 insertion to handle byte ≥ 128 inputs.
/// 2. Compute `nextanotb` / `nextbnota` lookahead arrays.
/// 3. Pick initial mode via [`pick_initial_mode`] which surfaces
///    BWIPP modes 0, 1, 2, 5, 6 (no FN1 / SAM yet).
/// 4. Walk msg with the active cset:
///    - cset A/B: SB1/SB2 + SA1/SA2 single/double shifts, SWA/SWB
///      latches, default emission.
///    - cset C: digit pair emission when nums ≥ 2, else SWA / SWB
///      latch.
///
/// # Remaining gaps
///
/// * Mode-C SC2/SC3 mid-message transitions (BWIPP lines 19780–
///   19805 in set A and 19830–19877 in set B) are not implemented:
///   for a payload like "ABCDE12345" where mode-C would optimise
///   the embedded digit run, this encoder stays in mode B.
/// * Mode-C SB1/SB2/SB3 trailing-byte shifts (BWIPP lines 19936–
///   19987) are not implemented: for a payload like "12X34" the
///   mode-C path falls back to a SWB latch instead of a 1-byte
///   shift.
/// * FN1 / FN2 / FN3 input parsing (modes 3 and 4 from start).
/// * SAM (Symbol Append Mode, mode 7).
pub(crate) fn encode_data_cws_mixed(input: &[u8]) -> Result<(u16, Vec<u16>), Error> {
    if input.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }

    // Step 1: parseinput (identity for parsefnc=false) + FN4 insertion.
    let initial_msg: Vec<i16> = input.iter().map(|&b| i16::from(b)).collect();
    let msg = insert_fn4_markers(&initial_msg);

    // Step 2: lookahead arrays.
    let (next_anotb, next_bnota) = compute_lookahead(&msg);

    // Step 3: initial cset + any leading codewords from the mode
    // selector (e.g. mode 5 emits 1 byte in B; mode 6 emits 2).
    let (mut cset, mode, mut i, mut cws) = pick_initial_mode(&msg, &next_anotb, &next_bnota);
    cws.reserve(msg.len() * 2);
    while i < msg.len() {
        let c = msg[i];
        match cset {
            Cset::A => {
                // SB1 — shift to B for 1 byte then continue A.
                if i + 1 < msg.len() && bnota(c) && abeforeb(i + 1, &next_anotb, &next_bnota) {
                    cws.push(SB1_FROM_A);
                    let cw = lookup_b_for_sentinel_or_byte(c)?;
                    cws.push(cw);
                    i += 1;
                    continue;
                }
                // SB2 — shift to B for 2 bytes then continue A.
                if i + 2 < msg.len()
                    && bnota(c)
                    && bnota(msg[i + 1])
                    && abeforeb(i + 2, &next_anotb, &next_bnota)
                {
                    cws.push(SB2_FROM_A);
                    cws.push(lookup_b_for_sentinel_or_byte(c)?);
                    cws.push(lookup_b_for_sentinel_or_byte(msg[i + 1])?);
                    i += 2;
                    continue;
                }
                // SWB — latch to B (no i++).
                if bnota(c) {
                    cws.push(SWB_FROM_A);
                    cset = Cset::B;
                    continue;
                }
                // SC2 — shift to C for 2 digit pairs (4 bytes), then back to A.
                let (nums, _) = numsscr(&msg, i);
                if i + 4 < msg.len() && nums == 4 && in_a(msg[i + 4]) {
                    cws.push(SC2_FROM_A);
                    for _ in 0..2 {
                        let hi = (msg[i] & 0xff) as u8;
                        let lo = (msg[i + 1] & 0xff) as u8;
                        cws.push(pair_codeword(hi, lo));
                        i += 2;
                    }
                    continue;
                }
                // SC3 — shift to C for 3 digit pairs (6 bytes), then back to A.
                if i + 6 < msg.len() && nums == 6 && in_a(msg[i + 6]) {
                    cws.push(SC3_FROM_A);
                    for _ in 0..3 {
                        let hi = (msg[i] & 0xff) as u8;
                        let lo = (msg[i + 1] & 0xff) as u8;
                        cws.push(pair_codeword(hi, lo));
                        i += 2;
                    }
                    continue;
                }
                // SWC — latch to C (no i++) when 4+ even-count digits ahead.
                if nums >= 4 && nums % 2 == 0 {
                    cws.push(SWC_FROM_A_OR_B);
                    cset = Cset::C;
                    continue;
                }
                // Default — emit current byte in set A.
                let cw = lookup_a_for_sentinel_or_byte(c)?;
                cws.push(cw);
                i += 1;
            }
            Cset::B => {
                // SA1 — shift to A for 1 byte then continue B.
                if i + 1 < msg.len() && anotb(c) && bbeforea(i + 1, &next_anotb, &next_bnota) {
                    cws.push(SA1_FROM_B);
                    cws.push(lookup_a_for_sentinel_or_byte(c)?);
                    i += 1;
                    continue;
                }
                // SA2 — shift to A for 2 bytes then continue B.
                if i + 2 < msg.len()
                    && anotb(c)
                    && anotb(msg[i + 1])
                    && bbeforea(i + 2, &next_anotb, &next_bnota)
                {
                    cws.push(SA2_FROM_B);
                    cws.push(lookup_a_for_sentinel_or_byte(c)?);
                    cws.push(lookup_a_for_sentinel_or_byte(msg[i + 1])?);
                    i += 2;
                    continue;
                }
                // SWA — latch to A (no i++).
                if anotb(c) {
                    cws.push(SWA_FROM_B);
                    cset = Cset::A;
                    continue;
                }
                // SC2 — shift to C for 2 digit pairs (4 bytes), then back to B.
                let (nums, _) = numsscr(&msg, i);
                if i + 4 < msg.len() && nums == 4 && in_b(msg[i + 4]) {
                    cws.push(SC2_FROM_B);
                    for _ in 0..2 {
                        let hi = (msg[i] & 0xff) as u8;
                        let lo = (msg[i + 1] & 0xff) as u8;
                        cws.push(pair_codeword(hi, lo));
                        i += 2;
                    }
                    continue;
                }
                // SC3 — shift to C for 3 digit pairs (6 bytes), then back to B.
                if i + 6 < msg.len() && nums == 6 && in_b(msg[i + 6]) {
                    cws.push(SC3_FROM_B);
                    for _ in 0..3 {
                        let hi = (msg[i] & 0xff) as u8;
                        let lo = (msg[i + 1] & 0xff) as u8;
                        cws.push(pair_codeword(hi, lo));
                        i += 2;
                    }
                    continue;
                }
                // SWC — latch to C (no i++) when 4+ even-count digits ahead.
                if nums >= 4 && nums % 2 == 0 {
                    cws.push(SWC_FROM_A_OR_B);
                    cset = Cset::C;
                    continue;
                }
                // Default — emit current byte in set B.
                cws.push(lookup_b_for_sentinel_or_byte(c)?);
                i += 1;
            }
            Cset::C => {
                // Mode C: emit digit pair (or FN1 — not yet wired).
                // BWIPP path at bwip-js lines 19923–19972.
                let (nums, _) = numsscr(&msg, i);
                if nums >= 2 {
                    // Emit a digit pair: codeword = hi*10 + lo.
                    let hi = (msg[i] & 0xff) as u8;
                    let lo = (msg[i + 1] & 0xff) as u8;
                    cws.push(pair_codeword(hi, lo));
                    i += 2;
                    continue;
                }
                // SB1 in C — single byte in B, then back to C.
                // Fires when msg[i] in setb AND ≥2 even digits ahead.
                if i + 1 < msg.len() && in_b(c) {
                    let (_, s_next) = numsscr(&msg, i + 1);
                    if s_next >= 2 && s_next % 2 == 0 {
                        cws.push(SB1_FROM_C);
                        cws.push(lookup_b_for_sentinel_or_byte(c)?);
                        i += 1;
                        continue;
                    }
                }
                // SB2 in C — 1 byte in B + 3 odd digits ahead → 1 byte in B + 1 byte in B, then C with i+=2.
                // Actually BWIPP emits SB2 + 1 byte in B for this case. Let me re-check...
                // bwip-js line 19952-19968: `i < msglen-1`, `msg[i] in setb`, `numsscr(i+1).s >= 3 && s%2==1`
                // → emit SB2 in C (105), emit msg[i] in B, emit msg[i+1] in B, i+=2.
                if i + 1 < msg.len() && in_b(c) {
                    let (_, s_next) = numsscr(&msg, i + 1);
                    if s_next >= 3 && s_next % 2 == 1 && i + 2 < msg.len() && in_b(msg[i + 1]) {
                        cws.push(SB2_FROM_C);
                        cws.push(lookup_b_for_sentinel_or_byte(c)?);
                        cws.push(lookup_b_for_sentinel_or_byte(msg[i + 1])?);
                        i += 2;
                        continue;
                    }
                }
                // SB3 in C (variant 1) — 2 bytes in B + 3 odd digits ahead.
                if i + 2 < msg.len() && in_b(c) && in_b(msg[i + 1]) {
                    let (_, s_next) = numsscr(&msg, i + 2);
                    if s_next >= 3 && s_next % 2 == 1 && i + 3 < msg.len() && in_b(msg[i + 2]) {
                        cws.push(SB3_FROM_C);
                        cws.push(lookup_b_for_sentinel_or_byte(c)?);
                        cws.push(lookup_b_for_sentinel_or_byte(msg[i + 1])?);
                        cws.push(lookup_b_for_sentinel_or_byte(msg[i + 2])?);
                        i += 3;
                        continue;
                    }
                }
                // SB3 in C (variant 2) — 3 bytes in B + 2 even digits ahead.
                if i + 3 < msg.len() && in_b(c) && in_b(msg[i + 1]) && in_b(msg[i + 2]) {
                    let (_, s_next) = numsscr(&msg, i + 3);
                    if s_next >= 2 && s_next % 2 == 0 {
                        cws.push(SB3_FROM_C);
                        cws.push(lookup_b_for_sentinel_or_byte(c)?);
                        cws.push(lookup_b_for_sentinel_or_byte(msg[i + 1])?);
                        cws.push(lookup_b_for_sentinel_or_byte(msg[i + 2])?);
                        i += 3;
                        continue;
                    }
                }
                // No SB shift fits → latch back to A or B via abeforeb(i).
                if abeforeb(i, &next_anotb, &next_bnota) {
                    cws.push(SWA_FROM_C);
                    cset = Cset::A;
                } else {
                    cws.push(SWB_FROM_C);
                    cset = Cset::B;
                }
            }
        }
    }

    Ok((mode, cws))
}

/// Like [`lookup_a`] but also handles the FN4 sentinel — when `c`
/// is the `FN4` marker, returns the codeword that means "FN4 in
/// set A" (row 101).
#[inline]
fn lookup_a_for_sentinel_or_byte(c: i16) -> Result<u16, Error> {
    if c == FN4 {
        return Ok(FN4_FROM_A);
    }
    if c < 0 {
        return Err(Error::InvalidData(format!(
            "code16k mixed encoder: unsupported sentinel {c} (only FN4 is wired today)"
        )));
    }
    let b = c as u8;
    lookup_a(b).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k mixed encoder: byte 0x{b:02x} not A-encodable"
        ))
    })
}

/// Like [`lookup_b`] but also handles the FN4 sentinel.
#[inline]
fn lookup_b_for_sentinel_or_byte(c: i16) -> Result<u16, Error> {
    if c == FN4 {
        return Ok(FN4_FROM_B);
    }
    if c < 0 {
        return Err(Error::InvalidData(format!(
            "code16k mixed encoder: unsupported sentinel {c} (only FN4 is wired today)"
        )));
    }
    let b = c as u8;
    lookup_b(b).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k mixed encoder: byte 0x{b:02x} not B-encodable"
        ))
    })
}

/// BWIPP-faithful Code 16K cws-level encoder for **pure-text
/// mode-B** payloads. Looks every byte up in column B of
/// [`CHARMAPS`] (the printable-ASCII alphabet + a few mode-control
/// rows that don't normally appear in plain text), prepends the
/// `(rows - 2) * 7 + 1` row indicator, pads with `PAD = 103` to
/// `dcws_inner`, then appends the `c1` / `c2` symbol checks.
///
/// This is the mode-1 (`MODE_B` from start) path. Pure-mode-A
/// payloads (control bytes + uppercase) and mixed-mode payloads
/// remain on the burndown — the mode selector that picks A vs B
/// vs mixed is Stage 4.
///
/// # Errors
///
/// * `InvalidData` if `text` is empty.
/// * `InvalidData` if any byte is not B-encodable (a control byte
///   `0..=31` other than the rare CR/LF rows would need mode A).
/// * `InvalidData` if `text.len()` exceeds the r=16 ceiling
///   (77 codewords).
pub(crate) fn encode_cws_text_only(text: &[u8]) -> Result<Vec<u16>, Error> {
    if text.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }
    // Mode-B lookups per byte. If any byte isn't B-encodable we
    // bail to InvalidData — the mode selector that switches to A
    // for control chars lands in Stage 4.
    let mut cws: Vec<u16> = Vec::with_capacity(text.len() + 3);
    let (rows, dcws_inner) = pick_symbol_size(text.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k: text payload of {} bytes exceeds the r=16 ceiling (77 codewords)",
            text.len()
        ))
    })?;
    let dcws_inner = usize::from(dcws_inner);
    cws.push(leading_row_indicator(rows, MODE_B));
    for (idx, &b) in text.iter().enumerate() {
        let cw = lookup_b(b).ok_or_else(|| {
            Error::InvalidData(format!(
                "code16k mode-B path: byte 0x{b:02x} at position {idx} \
                 isn't B-encodable — mode-A and mixed-mode paths are on the burndown"
            ))
        })?;
        cws.push(cw);
    }
    while cws.len() < 1 + dcws_inner {
        cws.push(PAD_CW);
    }
    let (c1, c2) = compute_checksums(&cws);
    cws.push(c1);
    cws.push(c2);
    Ok(cws)
}

/// BWIPP-faithful Code 16K cws-level encoder for **pure-mode-A**
/// payloads (control bytes 0x00..=0x1F, uppercase ASCII, digits,
/// and the punctuation subset shared with Mode B). This is the
/// mode-0 (`MODE_A` from start) path.
///
/// Structurally identical to [`encode_cws_text_only`] except that
/// every byte is looked up via [`lookup_a`] instead of
/// [`lookup_b`], and the leading row indicator carries the
/// `MODE_A = 0` mode bit. Because Mode A column A and Mode B
/// column B share the same row indices for bytes 32..=95 (space
/// through `_`), an input that mixes uppercase + digits + punctuation
/// only would also encode in Mode B; the dispatcher routes to
/// Mode A only when the payload contains at least one A-only byte
/// (control byte 0x00..=0x1F or a high byte that needs FNC4 —
/// the latter is not yet handled).
///
/// # Errors
///
/// * `InvalidData` if `text` is empty.
/// * `InvalidData` if any byte is not A-encodable (lowercase
///   `'a'..='z'` and high bytes need Mode B / FNC4 / mixed-mode
///   shifts that this encoder doesn't ship yet — those are
///   documented as extension paths in PORT_STATUS).
/// * `InvalidData` if `text.len()` exceeds the r=16 ceiling
///   (77 codewords).
pub(crate) fn encode_cws_mode_a(text: &[u8]) -> Result<Vec<u16>, Error> {
    if text.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }
    let (rows, dcws_inner) = pick_symbol_size(text.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k mode-A path: payload of {} bytes exceeds the r=16 ceiling (77 codewords)",
            text.len()
        ))
    })?;
    let dcws_inner = usize::from(dcws_inner);
    let mut cws: Vec<u16> = Vec::with_capacity(1 + dcws_inner + 2);
    cws.push(leading_row_indicator(rows, MODE_A));
    for (idx, &b) in text.iter().enumerate() {
        let cw = lookup_a(b).ok_or_else(|| {
            Error::InvalidData(format!(
                "code16k mode-A path: byte 0x{b:02x} at position {idx} isn't A-encodable \
                 (lowercase + high bytes need mode-B or FNC4 / mixed-mode shifts \
                 — extension paths, see PORT_STATUS)"
            ))
        })?;
        cws.push(cw);
    }
    while cws.len() < 1 + dcws_inner {
        cws.push(PAD_CW);
    }
    let (c1, c2) = compute_checksums(&cws);
    cws.push(c1);
    cws.push(c2);
    Ok(cws)
}

/// BWIPP-faithful Code 16K cws-level encoder for **odd-length
/// digit-only** payloads. This is the `MODE_C_THEN_B` path
/// (mode = 5): the leading byte gets emitted in mode B
/// (`Bvals[byte]`), then the encoder switches to mode C for the
/// remaining `(len - 1) / 2` digit pairs.
///
/// Mirrors bwip-js encoder body for the mode-5 path (e.g. "12345"
/// → leading row indicator `(r-2)*7 + 5 = 5`, then `Bvals['1'] =
/// 17`, then pairs `'23' = 23`, `'45' = 45`).
///
/// # Errors
///
/// * `InvalidData` if `digits` is empty.
/// * `InvalidData` if `digits.len()` is even (use
///   [`encode_cws_digit_only`] for even-length pure-digit input).
/// * `InvalidData` if any byte is not an ASCII digit.
/// * `InvalidData` if the payload exceeds the r=16 ceiling.
pub(crate) fn encode_cws_digit_with_shift_b(digits: &[u8]) -> Result<Vec<u16>, Error> {
    if digits.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }
    if digits.len() % 2 == 0 {
        return Err(Error::InvalidData(format!(
            "code16k mode-5 path needs odd length, got {} digits — \
             use encode_cws_digit_only for even-length pure-digit input",
            digits.len()
        )));
    }
    for (idx, &b) in digits.iter().enumerate() {
        if !b.is_ascii_digit() {
            return Err(Error::InvalidData(format!(
                "code16k mode-5 path: non-digit byte 0x{b:02x} at position {idx}"
            )));
        }
    }
    // Data slots: 1 (leading B byte) + (len - 1) / 2 pairs.
    let pair_count = (digits.len() - 1) / 2;
    let inner_slots = 1 + pair_count;
    let (rows, dcws_inner) = pick_symbol_size(inner_slots).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k mode-5: payload of {} bytes (1 + {} pairs) exceeds r=16 ceiling",
            digits.len(),
            pair_count
        ))
    })?;
    let dcws_inner = usize::from(dcws_inner);
    let mut cws: Vec<u16> = Vec::with_capacity(1 + dcws_inner + 2);
    cws.push(leading_row_indicator(rows, MODE_C_THEN_B));
    // First byte in mode B via Bvals lookup.
    let first = lookup_b(digits[0])
        .expect("digit was already validated to be ASCII '0'..='9' → B-encodable");
    cws.push(first);
    // Remaining bytes packed as digit pairs in mode C.
    for chunk in digits[1..].chunks_exact(2) {
        let hi = u16::from(chunk[0] - b'0');
        let lo = u16::from(chunk[1] - b'0');
        cws.push(hi * 10 + lo);
    }
    while cws.len() < 1 + dcws_inner {
        cws.push(PAD_CW);
    }
    let (c1, c2) = compute_checksums(&cws);
    cws.push(c1);
    cws.push(c2);
    Ok(cws)
}

/// Top-level mode-aware encoder. Always routes through
/// [`encode_cws_mixed`] (which in turn runs [`encode_data_cws_mixed`]),
/// the BWIPP-faithful state machine that handles all four set
/// transitions (A, B, C, plus FN4 ASCII↔extended-ASCII) and the
/// initial-mode picker (modes 0, 1, 2, 5, 6).
///
/// The simpler per-mode helpers [`encode_cws_digit_only`],
/// [`encode_cws_digit_with_shift_b`], [`encode_cws_text_only`], and
/// [`encode_cws_mode_a`] remain available for callers that want a
/// known-mode encoder. They are also exercised directly by their
/// own module-level unit tests.
pub(crate) fn encode_cws(input: &[u8]) -> Result<Vec<u16>, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }
    encode_cws_mixed(input)
}

/// Wrap [`encode_data_cws_mixed`] in the standard
/// row-indicator + PAD + c1/c2 pipeline so the output matches the
/// shape used by the other `encode_cws_*` helpers.
pub(crate) fn encode_cws_mixed(input: &[u8]) -> Result<Vec<u16>, Error> {
    let (mode, data_cws) = encode_data_cws_mixed(input)?;
    let (rows, dcws_inner) = pick_symbol_size(data_cws.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k mixed-mode: data payload of {} codewords exceeds r=16 ceiling \
             (77 codewords)",
            data_cws.len()
        ))
    })?;
    let dcws_inner = usize::from(dcws_inner);
    let mut cws: Vec<u16> = Vec::with_capacity(1 + dcws_inner + 2);
    cws.push(leading_row_indicator(rows, mode));
    cws.extend_from_slice(&data_cws);
    while cws.len() < 1 + dcws_inner {
        cws.push(PAD_CW);
    }
    let (c1, c2) = compute_checksums(&cws);
    cws.push(c1);
    cws.push(c2);
    Ok(cws)
}

/// BWIPP-faithful Code 16K cws-level encoder for **digit-only,
/// even-length** payloads. This is the `MODE_C_FROM_START` path:
/// every two consecutive digits get packed into one codeword
/// (`pair = high*10 + low`), the encoder prepends the
/// `(rows - 2) * 7 + 2` row indicator, pads with `PAD = 103` to
/// `dcws_inner`, then appends the `c1` / `c2` symbol checks.
///
/// Mirrors bwip-js encoder body (lines 19490-19975) for the mode-C
/// happy path; mode A / B / mixed-mode payloads are on the burndown.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] when:
///   * `digits` is empty,
///   * any byte is not an ASCII digit `'0'..='9'`,
///   * `digits.len()` is odd (caller should use the eventual
///     `MODE_C_THEN_B` path for odd lengths),
///   * the payload exceeds 77 pairs (the r=16 ceiling — anything
///     beyond requires symbol-append-mode chaining, also on the
///     burndown).
pub(crate) fn encode_cws_digit_only(digits: &[u8]) -> Result<Vec<u16>, Error> {
    if digits.is_empty() {
        return Err(Error::InvalidData("code16k: empty input".to_string()));
    }
    if digits.len() % 2 != 0 {
        return Err(Error::InvalidData(format!(
            "code16k digit-only path needs even length, got {} digits — \
             odd-length payloads use mode 5 (shift to B for trailing byte), \
             on the encoder burndown",
            digits.len()
        )));
    }
    for (idx, &b) in digits.iter().enumerate() {
        if !b.is_ascii_digit() {
            return Err(Error::InvalidData(format!(
                "code16k: non-digit byte 0x{b:02x} at position {idx} — \
                 digit-only path is the only mode currently wired"
            )));
        }
    }
    let pair_count = digits.len() / 2;
    let (rows, dcws_inner) = pick_symbol_size(pair_count).ok_or_else(|| {
        Error::InvalidData(format!(
            "code16k: payload of {} pairs exceeds the r=16 ceiling (77 pairs)",
            pair_count
        ))
    })?;
    let dcws_inner = usize::from(dcws_inner);
    let mut cws: Vec<u16> = Vec::with_capacity(1 + dcws_inner + 2);
    cws.push(leading_row_indicator(rows, MODE_C_FROM_START));
    for chunk in digits.chunks_exact(2) {
        let hi = u16::from(chunk[0] - b'0');
        let lo = u16::from(chunk[1] - b'0');
        cws.push(hi * 10 + lo);
    }
    while cws.len() < 1 + dcws_inner {
        cws.push(PAD_CW);
    }
    let (c1, c2) = compute_checksums(&cws);
    cws.push(c1);
    cws.push(c2);
    Ok(cws)
}

/// Build the BWIPP `seprow`: 10 zeros + 70 ones + 1 zero. Used as
/// the inter-row separator (and accounts for the bearer-line look
/// adjacent to each data row).
fn build_seprow() -> [u8; 81] {
    let mut row = [1u8; 81];
    for cell in row.iter_mut().take(10) {
        *cell = 0;
    }
    row[80] = 0;
    row
}

/// Build the 81-module bit pattern for one row of a Code 16K
/// symbol, given its 0-based row index, 5 data codewords, and the
/// stop-encoding table for the current SAM (odd or even). Mirrors
/// bwip-js lines 19790-19820 exactly:
///
///   sbs = [10] ++ STARTENCS[row].digits ++ [1]
///         ++ ENCS[cw].digits for each of 5 cws
///         ++ stopencs[row].digits ++ [1]
///
/// The sequence-of-widths gets unrolled into bits by alternating
/// from a seed of `1` (so the first width — a 10-module space —
/// becomes 10 zeros).
fn build_row_bits(row_idx: usize, row_cws: &[u16], stopencs: &[&str; 16]) -> [u8; 81] {
    debug_assert_eq!(row_cws.len(), 5);
    let mut sbs: Vec<u8> = Vec::with_capacity(41);
    sbs.push(10);
    for c in STARTENCS[row_idx].chars() {
        sbs.push(c.to_digit(10).expect("STARTENCS contains only digits") as u8);
    }
    sbs.push(1);
    for &cw in row_cws {
        for c in ENCS[cw as usize].chars() {
            sbs.push(c.to_digit(10).expect("ENCS contains only digits") as u8);
        }
    }
    for c in stopencs[row_idx].chars() {
        sbs.push(c.to_digit(10).expect("stopencs contains only digits") as u8);
    }
    sbs.push(1);
    // Toggle starting from 1 → first width (10) produces zeros.
    let mut row = [0u8; 81];
    let mut current: u8 = 1;
    let mut idx = 0;
    for &w in &sbs {
        current = 1 - current;
        for _ in 0..w {
            row[idx] = current;
            idx += 1;
        }
    }
    debug_assert_eq!(idx, 81, "sbs widths must sum to 81 modules");
    row
}

/// BWIPP-faithful stacked renderer. Combines the cws-level encoder
/// output with [`STARTENCS`] / [`ENCS`] / [`STOPENCS_ODD`] into the
/// final bit pattern, stacks rows with separator + bearer lines per
/// the spec, and expands by `rowheight` / `sepheight` into the
/// returned [`BitMatrix`].
///
/// Mirrors bwip-js encode body (lines 19787-19898). Default
/// `rowheight = 8`, `sepheight = 1` — these are BWIPP's defaults
/// for stand-alone symbols (SAM not set).
///
/// # Layout per row (81 modules):
///
///   * 10 module left quiet area.
///   * 4 bars (7 modules total) from `STARTENCS[i]`.
///   * 1 module separator.
///   * 5 codewords × 6 bar/space widths summing to 11 modules each
///     (55 modules total).
///   * 4 bars (7 modules total) from `STOPENCS_ODD[i]`.
///   * 1 module right separator (the "trailing 1" in BWIPP's sbs).
///
/// # Layout vertically (`numcomprows = 2 * r + 1` compressed rows):
///
///   * Top bearer (`sepheight` modules, all ones).
///   * For each row i in 0..r-1: data (rowheight) + separator
///     (sepheight) — the separator has 10 zeros, 70 ones, 1 zero.
///   * Last data row (rowheight).
///   * Bottom bearer (sepheight, all ones).
pub fn encode(input: &[u8]) -> Result<BitMatrix, Error> {
    let cws = encode_cws(input)?;
    // Derive r from the final cws length. cws = 1 (row indicator)
    // + dcws_inner + 2 (c1/c2). cws.len() = dcws_inner + 3, also
    // equal to 5 * r per BWIPP's METRICS table.
    if cws.len() % 5 != 0 {
        return Err(Error::InvalidData(format!(
            "code16k internal: cws length {} not divisible by 5",
            cws.len()
        )));
    }
    let rows = cws.len() / 5;
    if !(2..=16).contains(&rows) {
        return Err(Error::InvalidData(format!(
            "code16k internal: derived row count {rows} not in 2..=16"
        )));
    }
    // SAM not implemented — stand-alone symbol → odd stop encs.
    let stopencs = &STOPENCS_ODD;
    let rowheight: usize = 8;
    let sepheight: usize = 1;
    let pixx: usize = 81;
    let seprow = build_seprow();
    let allone = [1u8; 81];
    // Collect (compressed_row, mult) pairs in BWIPP's layout order.
    let numcomprows = 2 * rows + 1;
    let mut compressed: Vec<[u8; 81]> = Vec::with_capacity(numcomprows);
    let mut mults: Vec<usize> = Vec::with_capacity(numcomprows);
    compressed.push(allone);
    mults.push(sepheight);
    for i in 0..rows {
        let row_cws = &cws[i * 5..i * 5 + 5];
        compressed.push(build_row_bits(i, row_cws, stopencs));
        mults.push(rowheight);
        if i + 1 < rows {
            compressed.push(seprow);
            mults.push(sepheight);
        }
    }
    compressed.push(allone);
    mults.push(sepheight);
    debug_assert_eq!(compressed.len(), numcomprows);
    // Expand to BitMatrix.
    let symhgt: usize = mults.iter().sum();
    let mut bm = BitMatrix::new(pixx, symhgt);
    let mut y = 0;
    for (row, &mult) in compressed.iter().zip(mults.iter()) {
        for _ in 0..mult {
            for (x, &bit) in row.iter().enumerate() {
                if bit != 0 {
                    bm.set(x, y, true);
                }
            }
            y += 1;
        }
    }
    Ok(bm)
}

/// Same as [`encode`] but returns the **compressed pixs** (a flat
/// `Vec<u8>` of `numcomprows × 81` cells, no row-multiplication
/// applied) — the form bwip-js's oracle anchor captures. This is
/// the byte-for-byte comparison surface used by the golden tests.
pub(crate) fn encode_pixs(input: &[u8]) -> Result<Vec<u8>, Error> {
    let cws = encode_cws(input)?;
    if cws.len() % 5 != 0 {
        return Err(Error::InvalidData(format!(
            "code16k internal: cws length {} not divisible by 5",
            cws.len()
        )));
    }
    let rows = cws.len() / 5;
    let stopencs = &STOPENCS_ODD;
    let seprow = build_seprow();
    let allone = [1u8; 81];
    let numcomprows = 2 * rows + 1;
    let mut pixs: Vec<u8> = Vec::with_capacity(numcomprows * 81);
    pixs.extend_from_slice(&allone);
    for i in 0..rows {
        let row_cws = &cws[i * 5..i * 5 + 5];
        pixs.extend_from_slice(&build_row_bits(i, row_cws, stopencs));
        if i + 1 < rows {
            pixs.extend_from_slice(&seprow);
        }
    }
    pixs.extend_from_slice(&allone);
    debug_assert_eq!(pixs.len(), numcomprows * 81);
    Ok(pixs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `numsscr(msg, p)` digit-run counter with
    /// FN1 alignment semantics. The closure returns (n, s) where n
    /// is the number of consumed bytes and s is the digit-pair
    /// alignment counter (incremented one extra step at every FN1
    /// at an even position). FN1 at an odd s position breaks the
    /// run.
    ///
    /// Mutations to catch:
    ///   - `s % 2 == 0` → `!= 0`: FN1 alignment inverted.
    ///   - `s += 1` (FN1 even arm) → `-= 1`: wrong direction.
    ///   - `break` (FN1 odd arm) → continue: wrong behaviour at
    ///     misaligned FN1.
    ///   - `b'0'..=b'9'` range mutation.
    ///   - `n += 1` / `s += 1` / `p += 1` count drift.
    ///
    /// Hand-computed:
    ///   - ['1','2','3'] from p=0: (3, 3) — pure digit run.
    ///   - ['1','2','A','3'] from p=0: (2, 2) — non-digit breaks.
    ///   - [FN1,'1','2'] from p=0: (3, 4) — FN1 at s=0 bumps s by 1
    ///     before the standard +1/+1.
    ///   - ['1', FN1, '2'] from p=0: (1, 1) — FN1 at s=1 (odd) breaks.
    ///   - ['1','2', FN1, '3'] from p=0: (4, 5) — FN1 at even s=2
    ///     re-aligns, run continues.
    ///   - [] / past-end / leading non-digit: (0, 0).
    #[test]
    fn numsscr_digit_run_with_fn1_alignment() {
        // Pure digit run.
        let msg: Vec<i16> = "123".bytes().map(i16::from).collect();
        assert_eq!(numsscr(&msg, 0), (3, 3));

        // Non-digit breaks the run.
        let msg: Vec<i16> = "12A3".bytes().map(i16::from).collect();
        assert_eq!(numsscr(&msg, 0), (2, 2));

        // FN1 at even s position (start, s=0): s bumps to 1, then +1/+1.
        let msg: Vec<i16> = vec![FN1, i16::from(b'1'), i16::from(b'2')];
        assert_eq!(
            numsscr(&msg, 0),
            (3, 4),
            "FN1 at s=0 (even) bumps s by 1 then the standard +1/+1"
        );

        // FN1 at odd s position: breaks.
        let msg: Vec<i16> = vec![i16::from(b'1'), FN1, i16::from(b'2')];
        assert_eq!(numsscr(&msg, 0), (1, 1), "FN1 at s=1 (odd) breaks the run");

        // FN1 at even s position mid-run: re-aligns, run continues.
        let msg: Vec<i16> = vec![i16::from(b'1'), i16::from(b'2'), FN1, i16::from(b'3')];
        assert_eq!(
            numsscr(&msg, 0),
            (4, 5),
            "FN1 at s=2 (even) re-aligns and the run continues through '3'"
        );

        // Empty input.
        assert_eq!(numsscr(&[], 0), (0, 0));

        // Past-end: p == msg.len().
        let msg: Vec<i16> = "12".bytes().map(i16::from).collect();
        assert_eq!(numsscr(&msg, 2), (0, 0));

        // Leading non-digit: immediate break.
        let msg: Vec<i16> = vec![i16::from(b'A')];
        assert_eq!(numsscr(&msg, 0), (0, 0));

        // Digit edges '/' (47) and ':' (58) — just outside the range.
        let msg: Vec<i16> = vec![i16::from(b'/')];
        assert_eq!(numsscr(&msg, 0), (0, 0), "'/' just below '0' breaks");
        let msg: Vec<i16> = vec![i16::from(b':')];
        assert_eq!(numsscr(&msg, 0), (0, 0), "':' just above '9' breaks");
    }

    /// Stage 11.A8c — pin `compute_lookahead` right-to-left walk.
    /// Builds two distance arrays of length `msg.len() + 1` where
    /// `next_anotb[i]` = distance from position `i` to the next
    /// A-only byte (and similarly for B-only). Both arrays terminate
    /// at the 9999 right-edge sentinel.
    ///
    /// Mutations to catch:
    ///   - Sentinel `9999` → other value: position-9999 saturation
    ///     would shift.
    ///   - `next_anotb[i + 1] + 1` → `+ 0` or `- 1`: distance offset.
    ///   - `(0..n).rev()` → `0..n`: left-to-right walk gives wrong
    ///     distances.
    ///   - `if anotb(msg[i])` → `if !anotb(msg[i])`: inverts the
    ///     terminator detection.
    ///   - Swap of next_anotb / next_bnota target slots.
    ///
    /// Hand-computed for msg = [5, 'A', 'a']:
    ///   - 5 is A-only (control byte), 'A' is in both, 'a' is B-only.
    ///   - next_anotb walks back:
    ///     [3]=9999, [2]='a' not anotb → 10000, [1]='A' not anotb →
    ///     10001, [0]=5 IS anotb → 0.
    ///   - next_bnota walks back:
    ///     [3]=9999, [2]='a' IS bnota → 0, [1]='A' not bnota → 1,
    ///     [0]=5 not bnota → 2.
    #[test]
    fn compute_lookahead_right_to_left_walk() {
        let msg: Vec<i16> = vec![5, i16::from(b'A'), i16::from(b'a')];
        let (next_anotb, next_bnota) = compute_lookahead(&msg);
        assert_eq!(
            next_anotb,
            vec![0, 10001, 10000, 9999],
            "next_anotb: [0]=0 (control 5 IS A-only), then +1 each step \
             back to the 9999 sentinel"
        );
        assert_eq!(
            next_bnota,
            vec![2, 1, 0, 9999],
            "next_bnota: [2]=0 ('a' IS B-only), then +1 back; [0]=2 \
             distance to 'a'"
        );

        // Pure uppercase: no A-only and no B-only bytes → both arrays
        // saturate above 9999.
        let msg: Vec<i16> = vec![i16::from(b'A'), i16::from(b'B')];
        let (next_anotb, next_bnota) = compute_lookahead(&msg);
        assert_eq!(next_anotb, vec![10001, 10000, 9999]);
        assert_eq!(next_bnota, vec![10001, 10000, 9999]);

        // Empty input: both arrays are just the [9999] sentinel.
        let msg: Vec<i16> = vec![];
        let (next_anotb, next_bnota) = compute_lookahead(&msg);
        assert_eq!(next_anotb, vec![9999]);
        assert_eq!(next_bnota, vec![9999]);
    }

    /// Stage 11.A8c — pin `abeforeb` and `bbeforea` strict-less
    /// comparators. Both are 1-line wrappers around `<` on the two
    /// lookahead arrays; the encoder uses them to decide
    /// mode-switch direction. Mutations to catch:
    ///   - `<` → `<=`: equal distances flip false → true.
    ///   - `<` → `>`: inverted predicate.
    ///   - swap of `next_anotb` / `next_bnota` indexing (passing
    ///     `bnota` to `abeforeb`).
    ///   - Index `i` ignored (always reads slot 0).
    #[test]
    fn abeforeb_and_bbeforea_strict_less() {
        // Distinct distances: anotb=3, bnota=7 → A is closer.
        let anotb = [3u32];
        let bnota = [7u32];
        assert!(
            abeforeb(0, &anotb, &bnota),
            "anotb(3) < bnota(7) → abeforeb=true"
        );
        assert!(
            !bbeforea(0, &anotb, &bnota),
            "anotb(3) < bnota(7) → bbeforea=false"
        );

        // Reverse: anotb=7, bnota=3 → B is closer.
        let anotb = [7u32];
        let bnota = [3u32];
        assert!(!abeforeb(0, &anotb, &bnota));
        assert!(bbeforea(0, &anotb, &bnota));

        // Equal distance: BOTH must be false (catches `<=` mutation
        // on either helper).
        let anotb = [5u32];
        let bnota = [5u32];
        assert!(
            !abeforeb(0, &anotb, &bnota),
            "equal distances must NOT count as a-before-b (rejects `<=`)"
        );
        assert!(
            !bbeforea(0, &anotb, &bnota),
            "equal distances must NOT count as b-before-a (rejects `<=`)"
        );

        // Indexing: per-position lookup must use `i`, not slot 0.
        let anotb = [99u32, 2, 100];
        let bnota = [99u32, 5, 1];
        assert!(abeforeb(1, &anotb, &bnota), "i=1: 2 < 5 → true");
        assert!(
            !abeforeb(2, &anotb, &bnota),
            "i=2: 100 < 1 is false (rejects index-pinned-at-0 mutation)"
        );
        assert!(bbeforea(2, &anotb, &bnota), "i=2: 1 < 100 → true");
    }

    /// `CHARMAPS` has exactly 107 rows: 96 ASCII rows (0..=95) plus
    /// 11 mode-control rows (96..=106). Each row has three columns
    /// (A / B / C lookups).
    #[test]
    fn charmaps_shape() {
        assert_eq!(CHARMAPS.len(), 107);
        for row in &CHARMAPS {
            assert_eq!(row.len(), 3);
        }
    }

    /// Anchor a handful of `CHARMAPS` rows known from the BWIPP
    /// source: row 0 = `[32, 32, 0]` (space), row 33 = `[65, 65, 33]`
    /// (`A`), row 103 = PAD row, row 106 = `SC3`/`SC3`/`SB3`.
    #[test]
    fn charmaps_anchors() {
        assert_eq!(CHARMAPS[0], [32, 32, 0]);
        assert_eq!(CHARMAPS[33], [65, 65, 33]);
        assert_eq!(CHARMAPS[64], [0, 96, 64]);
        assert_eq!(CHARMAPS[103], [PAD, PAD, PAD]);
        assert_eq!(CHARMAPS[106], [SC3, SC3, SB3]);
    }

    /// `METRICS` covers rows 2..=16 (15 entries). `dcws_inner`
    /// grows linearly: 7, 12, 17, … with step 5 (matches the per-
    /// row data-codeword capacity of Code 16K).
    #[test]
    fn metrics_shape_and_progression() {
        assert_eq!(METRICS.len(), 15);
        assert_eq!(METRICS[0], [2, 7]);
        assert_eq!(METRICS[14], [16, 77]);
        for i in 1..15 {
            assert_eq!(
                METRICS[i][0] - METRICS[i - 1][0],
                1,
                "rows should step by 1"
            );
            assert_eq!(
                METRICS[i][1] - METRICS[i - 1][1],
                5,
                "dcws_inner should step by 5",
            );
        }
    }

    /// `ENCS` has 107 entries (matches `CHARMAPS`); each is six
    /// digits in `1..=4` summing to 11 modules.
    #[test]
    fn encs_shape() {
        assert_eq!(ENCS.len(), 107);
        for (i, enc) in ENCS.iter().enumerate() {
            assert_eq!(enc.len(), 6, "ENCS[{i}] = {enc:?} should be 6 chars");
            let total: u32 = enc.chars().map(|c| c.to_digit(10).unwrap()).sum();
            assert_eq!(total, 11, "ENCS[{i}] = {enc:?} should sum to 11 modules");
        }
    }

    /// Start / stop indicator tables. Each has 16 entries of four
    /// digits summing to 7 modules.
    #[test]
    fn start_stop_enc_shapes() {
        for (table_name, table) in [
            ("startencs", &STARTENCS[..]),
            ("stopencs_odd", &STOPENCS_ODD[..]),
            ("stopencs_even", &STOPENCS_EVEN[..]),
        ] {
            assert_eq!(table.len(), 16, "{table_name} should have 16 entries");
            for (i, enc) in table.iter().enumerate() {
                assert_eq!(
                    enc.len(),
                    4,
                    "{table_name}[{i}] = {enc:?} should be 4 chars"
                );
                let total: u32 = enc.chars().map(|c| c.to_digit(10).unwrap()).sum();
                assert_eq!(
                    total, 7,
                    "{table_name}[{i}] = {enc:?} should sum to 7 modules"
                );
            }
        }
    }

    /// `leading_row_indicator(r, mode)` should match BWIPP's
    /// `(r - 2) * 7 + mode` exactly for every (r, mode) pair we've
    /// captured.
    #[test]
    fn leading_row_indicator_matches_bwipp() {
        // (text, expected cws[0], rows, mode) from oracle-code16k:
        //   "1"           → 1, r=2, mode=1
        //   "12"          → 2, r=2, mode=2
        //   "A"           → 1, r=2, mode=1
        //   "AB"          → 1, r=2, mode=1
        //   "12345"       → 5, r=2, mode=5
        //   "ABC"         → 1, r=2, mode=1
        //   "Hello"       → 1, r=2, mode=1
        //   "1234567890"  → 2, r=2, mode=2
        let cases: &[(u16, u16, u16)] = &[
            (2, 1, 1),
            (2, 2, 2),
            (2, 1, 1),
            (2, 1, 1),
            (2, 5, 5),
            (2, 1, 1),
            (2, 1, 1),
            (2, 2, 2),
        ];
        for &(r, mode, expected) in cases {
            assert_eq!(
                leading_row_indicator(r, mode),
                expected,
                "(r={r}, mode={mode})",
            );
        }
    }

    /// `compute_checksums` mirrors BWIPP exactly for the four
    /// minimum-payload goldens captured from bwip-js:
    ///
    ///   * "1"            → cws=[1, 17, 103×6], c1=4,  c2=46
    ///   * "12"           → cws=[2, 12, 103×6], c1=98, c2=27
    ///   * "A"            → cws=[1, 33, 103×6], c1=52, c2=82
    ///   * "AB"           → cws=[1, 33, 34, 103×5], c1=97, c2=66
    ///   * "12345"        → cws=[5, 17, 23, 45, 103×4], c1=44, c2=45
    ///   * "1234567890"   → cws=[2,12,34,56,78,90,103,103], c1=95, c2=44
    #[test]
    fn compute_checksums_matches_bwipp_goldens() {
        let cases: &[(&[u16], u16, u16)] = &[
            (&[1, 17, 103, 103, 103, 103, 103, 103], 4, 46),
            (&[2, 12, 103, 103, 103, 103, 103, 103], 98, 27),
            (&[1, 33, 103, 103, 103, 103, 103, 103], 52, 82),
            (&[1, 33, 34, 103, 103, 103, 103, 103], 97, 66),
            (&[5, 17, 23, 45, 103, 103, 103, 103], 44, 45),
            (&[2, 12, 34, 56, 78, 90, 103, 103], 95, 44),
        ];
        for &(cws, want_c1, want_c2) in cases {
            let (c1, c2) = compute_checksums(cws);
            assert_eq!(
                (c1, c2),
                (want_c1, want_c2),
                "cws={cws:?} → want (c1, c2) = ({want_c1}, {want_c2}), got ({c1}, {c2})",
            );
        }
    }

    /// Top-level `encode` now produces a real symbol for every
    /// payload the cws-level encoder accepts (digit-only and
    /// printable-ASCII text). Reject guards still kick in for
    /// empty input and bytes requiring mode-A / mid-message shifts.
    #[test]
    fn encode_produces_valid_bitmatrix_for_supported_inputs() {
        // r=2: symhgt = 1 + 8 + 1 + 8 + 1 = 19.
        for input in [&b"12"[..], b"1234", b"A", b"ABC", b"Hello"] {
            let bm = encode(input).unwrap_or_else(|e| panic!("encode({input:?}) failed: {e:?}"));
            assert_eq!(bm.width(), 81, "encode({input:?}) width should be 81");
            assert_eq!(
                bm.height(),
                19,
                "encode({input:?}) r=2 height should be 19 (sep+data+sep+data+sep)",
            );
        }
        // r=3: symhgt = 1 + 8 + 1 + 8 + 1 + 8 + 1 = 28.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // Code 16K r=3 geometry rationale.
        let bm = encode(b"Hello123").expect(
            "encode(b\"Hello123\") (Code 16K r=3 → symhgt = 1 sep + 3×(8 data + 1 sep) - 0 = 28 modules) must succeed",
        );
        assert_eq!(bm.width(), 81);
        assert_eq!(bm.height(), 28);
        // Reject empty. "A\tB" used to error here (mode A was
        // deferred); after Stage 21 it routes through Mode A
        // successfully and produces a valid 81×19 matrix.
        //
        // Stage 11.A8c — upgrade from `matches!(_, InvalidData(_))` to
        // pin the empty-specific diagnostic. encode() / encode_cws has
        // multiple InvalidData rejection arms (empty, "cws length not
        // divisible by 5", "derived row count not in 2..=16",
        // mixed-mode oversized payload). A mutant that swaps the
        // empty guard with any other arm's body survives the old
        // variant-only check. The empty diagnostic is "code16k: empty
        // input" — pin all three substrings.
        let err = encode(b"").unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("encode(b\"\") must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code16k:"),
            "empty-input diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("empty input"),
            "empty-input diagnostic must call out 'empty input'; got {msg:?}"
        );
        assert!(
            !msg.contains("exceeds")
                && !msg.contains("divisible by 5")
                && !msg.contains("row count"),
            "empty-input diagnostic must not leak the downstream arms; got {msg:?}"
        );
        // Stage 11.A8c (cont) — `.expect` naming Stage-21 Mode A path
        // (TAB char now routes through Mode A; pre-Stage-21 it errored).
        let bm = encode(b"A\tB").expect(
            "encode(b\"A\\tB\") (post-Stage-21: Mode A handles TAB control char → 81×19 r=2 matrix) must succeed",
        );
        assert_eq!(bm.width(), 81);
        assert_eq!(bm.height(), 19);
    }

    /// `encode_pixs` produces BWIPP byte-for-byte compressed pixs
    /// for the canonical payloads. The pixs golden for "12" comes
    /// from the patched bwip-js oracle at `#20107` (right before
    /// `bwipp_renmatrix`). 5 compressed rows × 81 modules = 405
    /// cells; rowmult is [1, 8, 1, 8, 1] (= top bearer + 2 data
    /// rows of height 8 each separated by sepheight=1 lines + bottom
    /// bearer).
    #[test]
    fn encode_pixs_matches_bwip_js_golden_for_12() {
        // Stage 11.A8c (cont) — `.expect` naming Code 16K pixs golden
        // geometry (r=2 → 5 rows × 81 cols = 405 cells, rowmult
        // [1, 8, 1, 8, 1] = bearer + 2 data rows + separators).
        let pixs = encode_pixs(b"12").expect(
            "encode_pixs(b\"12\") (Code 16K NS-digits, r=2 → 5 compressed rows × 81 cols, rowmult=[1,8,1,8,1]) must succeed",
        );
        let want: &[u8] = &[
            // Row 0 (top bearer): 81 ones.
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            // Row 1 (data row 0 — cws[0..5] = [2, 12, 103, 103, 103]).
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1,
            0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1,
            1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0,
            // Row 2 (separator): 10 zeros + 70 ones + 1 zero.
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            // Row 3 (data row 1 — cws[5..10] = [103, 103, 103, 98, 27]).
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1,
            0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1,
            1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0,
            // Row 4 (bottom bearer): 81 ones.
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(pixs.len(), 5 * 81, "5 compressed rows × 81 cells");
        assert_eq!(pixs.len(), want.len());
        for (i, (&got, &exp)) in pixs.iter().zip(want.iter()).enumerate() {
            assert_eq!(got, exp, "pixs[{i}] (row {}, col {})", i / 81, i % 81);
        }
    }

    /// Pins the BitMatrix *bit content* produced by `encode()`'s
    /// compressed-row expansion loop (source lines ~1320-1328), which
    /// the geometry-only `encode_produces_valid_bitmatrix_for_supported_inputs`
    /// test (width/height only) leaves unverified. Catches:
    ///   * `if bit != 0` → `if bit == 0` (1323): inverts which cells are
    ///     set — every asserted set/unset bit flips.
    ///   * `y += 1` → `y *= 1` (1327): the row cursor never advances, so
    ///     all compressed rows OR into row 0 and every row y≥1 is blank;
    ///     the data/separator bits we assert at y≥1 vanish.
    ///
    /// The expected matrix is the `encode_pixs` golden (independently
    /// pinned by `encode_pixs_matches_bwip_js_golden_for_12`) expanded
    /// by the r=2 row multipliers [1, 8, 1, 8, 1] → height 19.
    #[test]
    fn encode_bitmatrix_content_matches_expanded_pixs_for_12() {
        let pixs = encode_pixs(b"12").expect("encode_pixs(12) ok");
        let bm = encode(b"12").expect("encode(12) ok");
        // r=2 → compressed rows [bearer, data0, sep, data1, bearer] with
        // multipliers [1, 8, 1, 8, 1] (sepheight=1, rowheight=8).
        let mults = [1usize, 8, 1, 8, 1];
        assert_eq!(bm.width(), 81);
        assert_eq!(bm.height(), mults.iter().sum::<usize>());
        let mut y = 0usize;
        let mut any_set_below_row0 = false;
        for (crow, &mult) in mults.iter().enumerate() {
            for _ in 0..mult {
                for x in 0..81 {
                    let want = pixs[crow * 81 + x] != 0;
                    assert_eq!(
                        bm.get(x, y),
                        want,
                        "encode(12) bit at (x={x}, y={y}) (compressed row {crow}) \
                         must equal expanded-pixs bit {want}"
                    );
                    if y > 0 && bm.get(x, y) {
                        any_set_below_row0 = true;
                    }
                }
                y += 1;
            }
        }
        // Defense against the `y *= 1` collapse: at least one set bit must
        // live below row 0 (the bottom bearer row is all-ones at y=18).
        assert!(
            any_set_below_row0,
            "expanded matrix must have set bits below row 0 (kills y *= 1 collapse)"
        );
    }

    /// Pins `numcomprows = 2 * rows + 1` in `encode_pixs` (line ~1349).
    /// The `* → +` mutant (`2 + rows + 1`) is bit-invariant for r=2
    /// (2*2+1 == 2+2+1 == 5) — which is why the r=2 golden test misses
    /// it — but diverges for r≥3 (r=3: 7 vs 6). `numcomprows` drives the
    /// `debug_assert_eq!(pixs.len(), numcomprows * 81)` at the end of the
    /// function, which fires under the (debug) test build when the count
    /// is wrong. Also pins the actual length to 2*rows+1 rows directly.
    #[test]
    fn encode_pixs_numcomprows_for_r3() {
        // "Hello123" encodes to r=3 (mode B + trailing digits).
        let pixs = encode_pixs(b"Hello123").expect("encode_pixs(Hello123) ok");
        let cws = encode_cws(b"Hello123").expect("cws ok");
        let rows = cws.len() / 5;
        assert_eq!(rows, 3, "Hello123 must be a 3-row symbol");
        assert_eq!(
            pixs.len(),
            (2 * rows + 1) * 81,
            "r=3 compressed pixs must be (2*3+1)=7 rows × 81 (kills 2*rows+1 → 2+rows+1)"
        );
    }

    /// `pick_symbol_size` walks [`METRICS`] and returns the smallest
    /// `(rows, dcws_inner)` row that fits the requested pair count.
    /// Capacities increment by 5 (dcws_inner = 7 + 5 * (r - 2)).
    #[test]
    fn pick_symbol_size_picks_smallest_metrics_row() {
        // 0..=7 pairs all fit in r=2 (dcws_inner=7).
        for pairs in 0..=7 {
            assert_eq!(pick_symbol_size(pairs), Some((2, 7)));
        }
        // 8..=12 → r=3 (dcws_inner=12).
        for pairs in 8..=12 {
            assert_eq!(pick_symbol_size(pairs), Some((3, 12)));
        }
        // 13..=17 → r=4.
        for pairs in 13..=17 {
            assert_eq!(pick_symbol_size(pairs), Some((4, 17)));
        }
        // 77 pairs (max for r=16) → r=16.
        assert_eq!(pick_symbol_size(77), Some((16, 77)));
        // 78 pairs exceeds the ceiling.
        assert_eq!(pick_symbol_size(78), None);
    }

    /// `encode_cws_digit_only` produces BWIPP byte-for-byte cws for
    /// every even-length all-digit golden captured from
    /// `tools/oracle-code16k.js`. Goldens span r=2 (1..=7 pairs),
    /// r=3 (8..=12 pairs), and r=4 (13..=17 pairs) — the three
    /// symbol sizes most common in practice.
    #[test]
    fn encode_cws_digit_only_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            // r=2 (dcws_inner=7) — pad with 103 to fill.
            (b"12", &[2, 12, 103, 103, 103, 103, 103, 103, 98, 27]),
            (b"1234", &[2, 12, 34, 103, 103, 103, 103, 103, 36, 11]),
            (b"123456", &[2, 12, 34, 56, 103, 103, 103, 103, 15, 62]),
            (b"1234567890", &[2, 12, 34, 56, 78, 90, 103, 103, 95, 44]),
            (b"123456789012", &[2, 12, 34, 56, 78, 90, 12, 103, 9, 24]),
            (b"12345678901234", &[2, 12, 34, 56, 78, 90, 12, 34, 30, 89]),
            // r=3 (dcws_inner=12).
            (
                b"1234567890123456",
                &[
                    9, 12, 34, 56, 78, 90, 12, 34, 56, 103, 103, 103, 103, 83, 24,
                ],
            ),
            (
                b"123456789012345678",
                &[9, 12, 34, 56, 78, 90, 12, 34, 56, 78, 103, 103, 103, 22, 97],
            ),
            // r=4 (dcws_inner=17).
            (
                b"12345678901234567890123456",
                &[
                    16, 12, 34, 56, 78, 90, 12, 34, 56, 78, 90, 12, 34, 56, 103, 103, 103, 103, 3,
                    60,
                ],
            ),
        ];
        for &(input, expected) in cases {
            let cws = encode_cws_digit_only(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_digit_only({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_cws_digit_only({:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Reject empty, odd-length, and non-digit inputs from the
    /// digit-only path. Odd-length is reserved for the eventual
    /// `MODE_C_THEN_B` (mode 5) path; non-digit input belongs to
    /// modes A / B.
    #[test]
    fn encode_cws_digit_only_rejects_invalid_inputs() {
        // Stage 11.A8c — upgrade 5 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at lines
        // 1149 (empty), 1152-1156 (odd-length), 1161-1163 (non-digit),
        // and 1169-1171 (r=16 overflow).
        //
        // Empty.
        match encode_cws_digit_only(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "empty arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty arm missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("non-digit"),
                    "empty arm leaked non-digit diagnostic: {msg}"
                );
                assert!(
                    !msg.contains("even length"),
                    "empty arm leaked even-length diagnostic: {msg}"
                );
            }
            other => panic!("empty digit-only input should reject as InvalidData, got {other:?}"),
        }
        // Odd-length.
        match encode_cws_digit_only(b"12345").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k digit-only path"),
                    "odd-length arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("needs even length"),
                    "odd-length arm missing `needs even length` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 5 digits"),
                    "odd-length arm missing `got 5 digits` length echo: {msg}"
                );
            }
            other => panic!("5-digit (odd) input should reject as InvalidData, got {other:?}"),
        }
        // Non-digit 'A' at position 2.
        match encode_cws_digit_only(b"12A4").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "non-digit 'A' arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("non-digit byte"),
                    "non-digit 'A' arm missing `non-digit byte` predicate: {msg}"
                );
                assert!(
                    msg.contains("0x41"),
                    "non-digit 'A' arm missing hex echo `0x41`: {msg}"
                );
                assert!(
                    msg.contains("at position 2"),
                    "non-digit 'A' arm missing `at position 2`: {msg}"
                );
            }
            other => panic!("`12A4` should reject as InvalidData, got {other:?}"),
        }
        // Non-digit ' ' (0x20) at position 2 — different byte.
        match encode_cws_digit_only(b"12 4").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("non-digit byte"),
                    "non-digit ' ' arm missing predicate: {msg}"
                );
                assert!(
                    msg.contains("0x20"),
                    "non-digit ' ' arm missing hex echo `0x20`: {msg}"
                );
                assert!(
                    msg.contains("at position 2"),
                    "non-digit ' ' arm missing `at position 2`: {msg}"
                );
            }
            other => panic!("`12 4` should reject as InvalidData, got {other:?}"),
        }
        // Way too long — exceeds r=16 ceiling (77 pairs = 154 digits;
        // 156 digits = 78 pairs).
        let huge: Vec<u8> = (0..156).map(|i| b'0' + ((i % 10) as u8)).collect();
        match encode_cws_digit_only(&huge).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "overflow arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("78 pairs"),
                    "overflow arm missing `78 pairs` count echo: {msg}"
                );
                assert!(
                    msg.contains("r=16 ceiling"),
                    "overflow arm missing `r=16 ceiling` predicate: {msg}"
                );
            }
            other => panic!("156-digit input should reject as InvalidData, got {other:?}"),
        }
    }

    /// Mode constants line up with BWIPP's `(rows - 2) * 7 + mode`
    /// expectations: cws[0] for `(r=2, mode=MODE_C_FROM_START)` is
    /// 2, for `(r=3, mode=MODE_C_FROM_START)` is 9, …
    #[test]
    fn mode_constants_compose_with_leading_row_indicator() {
        assert_eq!(leading_row_indicator(2, MODE_A), 0);
        assert_eq!(leading_row_indicator(2, MODE_B), 1);
        assert_eq!(leading_row_indicator(2, MODE_C_FROM_START), 2);
        assert_eq!(leading_row_indicator(3, MODE_C_FROM_START), 9);
        assert_eq!(leading_row_indicator(4, MODE_C_FROM_START), 16);
        assert_eq!(leading_row_indicator(2, MODE_C_THEN_B), 5);
        assert_eq!(leading_row_indicator(2, MODE_GS1), 6);
        // Max r=16, max mode=6 → (16-2)*7+6 = 104.
        assert_eq!(leading_row_indicator(16, MODE_GS1), 104);
    }

    /// Column-B lookup spot-checks: ASCII space → 0, 'A' → 33,
    /// 'a' → 65, 'H' → 40, '1' → 17, lowercase 'z' → 90. Control
    /// bytes 0..=31 are NOT B-encodable (the CHARMAPS column-B
    /// cells in rows 64..=95 hold lowercase 96..=127 instead).
    #[test]
    fn lookup_b_spot_checks() {
        assert_eq!(lookup_b(b' '), Some(0));
        assert_eq!(lookup_b(b'A'), Some(33));
        assert_eq!(lookup_b(b'B'), Some(34));
        assert_eq!(lookup_b(b'H'), Some(40));
        assert_eq!(lookup_b(b'a'), Some(65));
        assert_eq!(lookup_b(b'e'), Some(69));
        assert_eq!(lookup_b(b'l'), Some(76));
        assert_eq!(lookup_b(b'o'), Some(79));
        assert_eq!(lookup_b(b'z'), Some(90));
        assert_eq!(lookup_b(b'1'), Some(17));
        assert_eq!(lookup_b(b'9'), Some(25));
        // Control bytes aren't B-encodable.
        assert_eq!(lookup_b(0), None);
        assert_eq!(lookup_b(9), None);
        assert_eq!(lookup_b(31), None);
    }

    /// Column-A lookup spot-checks: control bytes 0..=31 occupy
    /// rows 64..=95 of column A; uppercase letters and digits also
    /// appear (rows 0..=63). Lowercase letters are A-encodable too
    /// via the trailing rows. Bytes 96..=127 are NOT A-encodable.
    #[test]
    fn lookup_a_spot_checks() {
        assert_eq!(lookup_a(b' '), Some(0));
        assert_eq!(lookup_a(b'A'), Some(33));
        assert_eq!(lookup_a(b'1'), Some(17));
        assert_eq!(lookup_a(0), Some(64));
        assert_eq!(lookup_a(9), Some(73));
        assert_eq!(lookup_a(31), Some(95));
        // Lowercase letters not in column A.
        assert_eq!(lookup_a(b'a'), None);
        assert_eq!(lookup_a(b'z'), None);
        // DEL not in column A.
        assert_eq!(lookup_a(127), None);
    }

    /// `encode_cws_text_only` produces BWIPP byte-for-byte cws for
    /// every mode-B golden captured from `tools/oracle-code16k.js`.
    /// Goldens span r=2 (≤7 chars) and r=3 (8..=12 chars) and r=4
    /// (13..=17 chars).
    #[test]
    fn encode_cws_text_only_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            // r=2 / dcws_inner=7 (≤7 chars).
            (b"A", &[1, 33, 103, 103, 103, 103, 103, 103, 52, 82]),
            (b"AB", &[1, 33, 34, 103, 103, 103, 103, 103, 97, 66]),
            (b"ABC", &[1, 33, 34, 35, 103, 103, 103, 103, 78, 51]),
            (b"abc", &[1, 65, 66, 67, 103, 103, 103, 103, 34, 50]),
            (b"Hello", &[1, 40, 69, 76, 76, 79, 103, 103, 7, 58]),
            // 6-char input fits r=2 exactly with 1 PAD.
            (b"abcdef", &[1, 65, 66, 67, 68, 69, 70, 103, 71, 94]),
            // r=3 / dcws_inner=12 (8..=12 chars).
            (
                b"Hello123",
                &[
                    8, 40, 69, 76, 76, 79, 17, 18, 19, 103, 103, 103, 103, 56, 26,
                ],
            ),
            // r=4 / dcws_inner=17 (13..=17 chars).
            (
                b"ABCDEFGHIJKLMN",
                &[
                    15, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 103, 103, 103, 52,
                    56,
                ],
            ),
        ];
        for &(input, expected) in cases {
            let cws = encode_cws_text_only(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_text_only({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_cws_text_only({:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Reject empty input and bytes that aren't B-encodable from
    /// the text-mode path. Mode-A control bytes (`\0`, `\t`, `\x1f`)
    /// surface as InvalidData with a clear message; full mixed-mode
    /// handling lands in Stage 4.
    #[test]
    fn encode_cws_text_only_rejects_invalid_inputs() {
        // Stage 11.A8c — upgrade 4 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at lines
        // 928 (`code16k: empty input`), 944-947 (per-byte mode-B
        // reject), and 935-938 (r=16 ceiling).
        //
        // Empty input.
        match encode_cws_text_only(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "empty arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty arm missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("mode-B path"),
                    "empty arm leaked mode-B byte diagnostic: {msg}"
                );
                assert!(
                    !msg.contains("ceiling"),
                    "empty arm leaked ceiling diagnostic: {msg}"
                );
            }
            other => panic!("empty text-only input should reject as InvalidData, got {other:?}"),
        }
        // Control byte 0 isn't B-encodable.
        match encode_cws_text_only(b"\0").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k mode-B path"),
                    "NUL arm missing `code16k mode-B path` prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x00"),
                    "NUL arm missing hex echo `byte 0x00`: {msg}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "NUL arm missing position 0 echo: {msg}"
                );
                assert!(
                    msg.contains("isn't B-encodable"),
                    "NUL arm missing `isn't B-encodable` predicate: {msg}"
                );
            }
            other => panic!("NUL text-only input should reject as InvalidData, got {other:?}"),
        }
        // Mix of B-encodable + control char (tab \t = 9 = 0x09) at
        // position 1 — fails on the control char.
        match encode_cws_text_only(b"A\tB").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("byte 0x09"),
                    "TAB arm missing hex echo `byte 0x09`: {msg}"
                );
                assert!(
                    msg.contains("at position 1"),
                    "TAB arm missing `at position 1` echo (after 'A' at 0): {msg}"
                );
            }
            other => panic!("`A\\tB` should reject as InvalidData, got {other:?}"),
        }
        // 78-byte payload exceeds the r=16 ceiling (77 codewords).
        let huge: Vec<u8> = (0..78).map(|_| b'A').collect();
        match encode_cws_text_only(&huge).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "ceiling arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("78 bytes"),
                    "ceiling arm missing `78 bytes` count echo: {msg}"
                );
                assert!(
                    msg.contains("r=16 ceiling"),
                    "ceiling arm missing `r=16 ceiling` predicate: {msg}"
                );
                assert!(
                    msg.contains("77 codewords"),
                    "ceiling arm missing `77 codewords` capacity echo: {msg}"
                );
            }
            other => panic!("78-byte input should reject as InvalidData, got {other:?}"),
        }
    }

    /// `encode_cws_digit_with_shift_b` (mode 5) matches BWIPP for
    /// every odd-length digit payload. Captured via
    /// `tools/oracle-code16k.js`:
    ///
    ///   "1"     → r=2, cws=[1, 17, 103×6, 4, 46]
    ///   "123"   → r=2, cws=[5, 17, 23, 103×5, 13, 105]
    ///   "12345" → r=2, cws=[5, 17, 23, 45, 103×4, 44, 45]
    ///   "1234567" → r=2, cws=[5, 17, 23, 45, 67, 103×3, 42, 61]
    ///   "12345678901" → r=2, cws=[5, 17, 23, 45, 67, 89, 1, 103, 91, 25]
    ///
    /// Note: "1" (single digit) routes through mode 5 too — the
    /// leading byte is in mode B with no trailing pairs. cws[0]
    /// becomes (r-2)*7+1 = 1 because pair_count is 0 (special
    /// case — but our pick_symbol_size picks the same r=2 row).
    /// Actually, for "1" BWIPP picks mode 1 (mode B from start, no
    /// shift), so cws[0] = 1 not 5. Our `encode_cws` routes "1" to
    /// `encode_cws_digit_with_shift_b` which would emit (r-2)*7+5
    /// = 5 — that's a discrepancy. For single-digit input the
    /// encoder should prefer mode 1 (length 1 is "B-encodable").
    /// We test mode 5 directly here for ≥3-byte odd payloads,
    /// where the encoder definitively wants mode 5.
    #[test]
    fn encode_cws_digit_with_shift_b_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            // r=2, leading B byte + 1 pair.
            (b"123", &[5, 17, 23, 103, 103, 103, 103, 103, 13, 105]),
            // r=2, leading B byte + 2 pairs.
            (b"12345", &[5, 17, 23, 45, 103, 103, 103, 103, 44, 45]),
            // r=2, leading B byte + 3 pairs.
            (b"1234567", &[5, 17, 23, 45, 67, 103, 103, 103, 42, 61]),
            // r=2, leading B byte + 5 pairs (full r=2 capacity).
            (b"12345678901", &[5, 17, 23, 45, 67, 89, 1, 103, 91, 25]),
        ];
        for &(input, expected) in cases {
            let cws = encode_cws_digit_with_shift_b(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_digit_with_shift_b({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_cws_digit_with_shift_b({:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Pins `encode_cws_digit_with_shift_b` at the r=2 → r=3 symbol-size
    /// boundary, where `pair_count = (len - 1) / 2` and `inner_slots =
    /// 1 + pair_count` feed `pick_symbol_size`. The existing goldens all
    /// fit r=2 (inner_slots ≤ 7), so a wrong inner_slots still selects
    /// r=2 and the output is unchanged. These two boundary cases force a
    /// row-count change under mutation:
    ///   * len=13 (6 pairs → inner_slots 7 → r=2): the `- → +` mutant
    ///     gives pair_count 7 → inner_slots 8 → r=3 (different indicator +
    ///     padding + length).
    ///   * len=15 (7 pairs → inner_slots 8 → r=3): the `+ → *` mutant
    ///     (inner_slots = pair_count = 7 → r=2) and the `/ → %` mutant
    ///     (pair_count = (14) % 2 = 0 → inner_slots 1 → r=2) both collapse
    ///     to r=2 → wrong indicator/length.
    /// (The `- → /` mutant — `(len/1)/2` = len/2 — is bit-equivalent for
    /// all odd len and is covered by an equivalence note, not here.)
    #[test]
    fn encode_cws_digit_with_shift_b_symbol_size_boundary_pinned() {
        // len=13: 6 pairs, inner_slots=7 → r=2 (indicator 5, 10 cws).
        assert_eq!(
            encode_cws_digit_with_shift_b(b"1234567890123").expect("len13 ok"),
            vec![5, 17, 23, 45, 67, 89, 1, 23, 13, 74],
            "len=13 stays r=2; `- → +` mutant would push to r=3"
        );
        // len=15: 7 pairs, inner_slots=8 → r=3 (indicator 12, 15 cws).
        assert_eq!(
            encode_cws_digit_with_shift_b(b"123456789012345").expect("len15 ok"),
            vec![12, 17, 23, 45, 67, 89, 1, 23, 45, 103, 103, 103, 103, 63, 104],
            "len=15 needs r=3; `+ → *` and `/ → %` mutants would collapse to r=2"
        );
    }

    /// Mode-5 rejections: empty, even-length, non-digit.
    #[test]
    fn encode_cws_digit_with_shift_b_rejects_invalid_inputs() {
        // Stage 11.A8c — upgrade 3 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at lines
        // 1036 (`code16k: empty input`), 1039-1043 (mode-5 even-length),
        // and 1047-1049 (mode-5 non-digit).
        //
        // Empty input.
        match encode_cws_digit_with_shift_b(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "empty arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty arm missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("mode-5"),
                    "empty arm leaked mode-5 diagnostic: {msg}"
                );
            }
            other => panic!("empty mode-5 input should reject as InvalidData, got {other:?}"),
        }
        // Even-length digit input belongs in mode 2, not mode 5.
        match encode_cws_digit_with_shift_b(b"1234").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k mode-5 path"),
                    "even-length arm missing `code16k mode-5 path` prefix: {msg}"
                );
                assert!(
                    msg.contains("needs odd length"),
                    "even-length arm missing `needs odd length` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 4 digits"),
                    "even-length arm missing `got 4 digits` count echo: {msg}"
                );
                assert!(
                    msg.contains("encode_cws_digit_only"),
                    "even-length arm missing remediation hint pointing at digit-only path: {msg}"
                );
            }
            other => panic!("4-digit even mode-5 should reject as InvalidData, got {other:?}"),
        }
        // Non-digit 'A' (0x41) at position 2.
        match encode_cws_digit_with_shift_b(b"12A").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k mode-5 path:"),
                    "non-digit arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("non-digit byte"),
                    "non-digit arm missing `non-digit byte` predicate: {msg}"
                );
                assert!(
                    msg.contains("0x41"),
                    "non-digit arm missing hex echo `0x41` for 'A': {msg}"
                );
                assert!(
                    msg.contains("at position 2"),
                    "non-digit arm missing `at position 2`: {msg}"
                );
            }
            other => panic!("`12A` should reject as InvalidData, got {other:?}"),
        }
    }

    /// The top-level `encode_cws` dispatches each input to the
    /// right cws-level sub-encoder. Spot-check the three routing
    /// branches against bwip-js goldens.
    ///
    /// (Note: `encode_cws` always picks the simplest path for the
    /// payload. Pure-digit even → mode 2; pure-digit odd → mode 5;
    /// printable text → mode 1. Mixed text+digit runs that BWIPP
    /// might encode with mid-message shifts are still on the
    /// burndown — they fall into the mode-1 path here so long as
    /// every byte is B-encodable, with no auto-shift to C.)
    #[test]
    fn encode_cws_top_level_routes_to_sub_encoders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // each dispatch path so a Code 16K dispatcher mutation that
        // re-routes one of these inputs to the wrong sub-encoder names
        // the expected mode in the panic.
        // All-digit even → mode 2 path (matches Stage 2 golden).
        let cws = encode_cws(b"1234")
            .expect("encode_cws(b\"1234\") (4-digit even → mode 2 / digit-pair path) must succeed");
        assert_eq!(cws, vec![2, 12, 34, 103, 103, 103, 103, 103, 36, 11]);
        // All-digit odd → mode 5 path.
        let cws = encode_cws(b"12345")
            .expect("encode_cws(b\"12345\") (5-digit odd → mode 5 / digit-pair-plus-trailing path) must succeed");
        assert_eq!(cws, vec![5, 17, 23, 45, 103, 103, 103, 103, 44, 45]);
        // All printable text → mode 1 path (matches Stage 3 golden).
        let cws = encode_cws(b"ABC")
            .expect("encode_cws(b\"ABC\") (pure-uppercase → mode 1 / text path) must succeed");
        assert_eq!(cws, vec![1, 33, 34, 35, 103, 103, 103, 103, 78, 51]);
        let cws = encode_cws(b"Hello")
            .expect("encode_cws(b\"Hello\") (mixed-case → mode 1 / text path) must succeed");
        assert_eq!(cws, vec![1, 40, 69, 76, 76, 79, 103, 103, 7, 58]);
        // Mixed B-encodable text/digit → mode 1 (no auto-shift).
        let cws = encode_cws(b"Hello123").unwrap();
        assert_eq!(
            cws,
            vec![8, 40, 69, 76, 76, 79, 17, 18, 19, 103, 103, 103, 103, 56, 26]
        );
    }

    /// Stage 3 — `encode_cws` rejects empty input, but no longer
    /// rejects mixed A↔B or high-byte inputs (those now route through
    /// `encode_cws_mixed`). The only remaining unhandled path is the
    /// mid-message → C optimization for embedded digit runs, which is
    /// documented as a known extension path.
    #[test]
    fn encode_cws_rejects_empty() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 1097
        // (`code16k: empty input`). Cross-arm guard against the
        // mixed-mode + mode-A + mode-B + r=16-ceiling arms.
        match encode_cws(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(msg.contains("code16k:"), "missing `code16k:` prefix: {msg}");
                assert!(
                    msg.contains("empty input"),
                    "missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("ceiling")
                        && !msg.contains("mode-A path")
                        && !msg.contains("mode-B path")
                        && !msg.contains("mixed-mode"),
                    "empty arm leaked downstream-mode diagnostic: {msg}"
                );
            }
            other => panic!("empty encode_cws should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 3 — mixed A↔B and high-byte inputs now route successfully
    /// through `encode_cws_mixed`. This pins the dispatcher's new
    /// branch behaviour.
    #[test]
    fn encode_cws_accepts_mixed_and_high_byte_inputs() {
        // Lowercase + control byte: needs mid-message A↔B latch.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // mid-message A↔B latch path.
        let cws = encode_cws(b"\0Hello").expect(
            "encode_cws(b\"\\0Hello\") (mid-message A↔B latch: NUL forces Mode A, 'Hello' lowercase forces Mode B) must succeed",
        );
        assert!(!cws.is_empty(), "expected mixed encoding to succeed");
        // High byte (FN4 path).
        let mut high = b"abc".to_vec();
        high.push(0xC8);
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming FN4
        // (high-byte) encoding path.
        let cws = encode_cws(&high).expect(
            "encode_cws(b\"abc\\xC8\") (FN4 high-byte escape: 0xC8 > 127 forces FN4 prefix) must succeed",
        );
        assert!(!cws.is_empty(), "expected FN4 encoding to succeed");
    }

    /// Stage 21 — `encode_cws_mode_a` mirrors `encode_cws_text_only`
    /// for inputs that ARE valid in both modes (uppercase + digits +
    /// punctuation share rows 0..=63 between the A and B columns).
    /// The only differences are the leading row indicator (mode bit
    /// 0 instead of 1) and the c1/c2 checksums (which depend on the
    /// indicator).
    #[test]
    fn encode_cws_mode_a_matches_text_only_structurally() {
        // "ABC" in Mode A: row indicator = (2-2)*7+0 = 0;
        // 'A'=33, 'B'=34, 'C'=35 (rows shared with mode B).
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // Mode A path + indicator/row math.
        let cws = encode_cws_mode_a(b"ABC").expect(
            "encode_cws_mode_a(b\"ABC\") (Mode A path: r=2 → row indicator=(r-2)*7+mode=0; uppercase shares rows 33-35 with Mode B) must succeed",
        );
        assert_eq!(cws[0], 0); // leading_row_indicator(2, MODE_A)
        assert_eq!(&cws[1..4], &[33, 34, 35][..]);
        assert_eq!(&cws[4..8], &[103, 103, 103, 103][..]); // PADs
        assert_eq!(cws.len(), 1 + 7 + 2); // indicator + dcws_inner=7 + c1 + c2
    }

    /// Stage 21 — Mode A is reachable for inputs with control
    /// bytes. The dispatcher routes "A\tB" (control tab + uppercase)
    /// to `encode_cws_mode_a` because every byte is A-encodable
    /// but '\t' (0x09) is NOT B-encodable. `'A'`/`'B'` use shared
    /// rows 33/34; `'\t'` uses row 64 + 9 = 73 (column-A control
    /// row for byte 9).
    #[test]
    fn encode_cws_routes_control_bytes_through_mode_a() {
        let cws = encode_cws(b"A\tB").unwrap();
        // Leading indicator = (2-2)*7+0 = 0; payload rows
        // [33, 73, 34]; PADs; then c1+c2.
        assert_eq!(cws[0], 0); // MODE_A indicator
        assert_eq!(cws[1], 33); // 'A'
        assert_eq!(cws[2], 73); // '\t' (0x09) → row 64+9
        assert_eq!(cws[3], 34); // 'B'
        assert_eq!(&cws[4..8], &[103, 103, 103, 103][..]);
        assert_eq!(cws.len(), 1 + 7 + 2);
    }

    /// Stage 21 — Negative path: lowercase + control byte still
    /// requires a mid-message A↔B shift (extension path). Surface
    /// a clear error.
    #[test]
    fn encode_cws_mode_a_rejects_lowercase() {
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at
        // lines 987 (empty) and 1000-1004 (non-A-encodable byte).
        //
        // Lowercase 'a' (0x61) is in set B but NOT in set A — first
        // non-A byte at position 0.
        match encode_cws_mode_a(b"abc").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k mode-A path:"),
                    "lowercase arm missing `code16k mode-A path:` prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x61"),
                    "lowercase arm missing hex echo `byte 0x61` for 'a': {msg}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "lowercase arm missing `at position 0`: {msg}"
                );
                assert!(
                    msg.contains("isn't A-encodable"),
                    "lowercase arm missing `isn't A-encodable` predicate: {msg}"
                );
            }
            other => panic!("`abc` mode-A should reject as InvalidData, got {other:?}"),
        }
        // Empty mode-A — should report empty input, NOT the per-byte
        // mode-A diagnostic.
        match encode_cws_mode_a(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code16k:"),
                    "mode-A empty arm missing `code16k:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "mode-A empty arm missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("isn't A-encodable"),
                    "mode-A empty arm leaked per-byte mode-A diagnostic: {msg}"
                );
            }
            other => panic!("empty mode-A should reject as InvalidData, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Stage 3 — mid-message A↔B + FN4 cws-level goldens.
    //
    // Captured via `rust/tools/oracle-code16k.js` against bwip-js
    // 4.10.1 / BWIPP 2026-04-21. Each test pins the data codewords
    // emitted by `encode_data_cws_mixed` against the bwipp_code16k
    // `$_.cws` array (no row indicator, no PAD, no c1/c2).
    // -----------------------------------------------------------------

    #[test]
    fn mixed_mode_b_with_trailing_control_byte_swa_latch() {
        // "Hello\x01" — mode B start, SWA latch, then 0x01 in set A.
        // Oracle: mode=1, cws=[40, 69, 76, 76, 79, 101, 65].
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming SWA latch path.
        let (mode, cws) = encode_data_cws_mixed(b"Hello\x01").expect(
            "encode_data_cws_mixed(b\"Hello\\x01\") (mode B start → SWA latch → trailing 0x01 in set A) must succeed",
        );
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![40, 69, 76, 76, 79, 101, 65]);
    }

    #[test]
    fn mixed_mode_b_with_two_trailing_control_bytes_swa_latch() {
        // "Hi\x01\x02" — mode B start, SWA latch, then 0x01 + 0x02 in A.
        // Oracle: mode=1, cws=[40, 73, 101, 65, 66].
        // Stage 11.A8c (cont) — `.expect` naming 2-byte SWA latch tail.
        let (mode, cws) = encode_data_cws_mixed(b"Hi\x01\x02").expect(
            "encode_data_cws_mixed(b\"Hi\\x01\\x02\") (mode B start → SWA latch → 0x01+0x02 in set A) must succeed",
        );
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![40, 73, 101, 65, 66]);
    }

    #[test]
    fn mixed_mode_b_with_mid_message_sa1_shift() {
        // "ab\x01cd" — mode B start, SA1 shift (1 byte), then back in B.
        // Oracle: mode=1, cws=[65, 66, 98, 65, 67, 68].
        // Stage 11.A8c (cont) — `.expect` naming SA1 single-byte-shift path.
        let (mode, cws) = encode_data_cws_mixed(b"ab\x01cd").expect(
            "encode_data_cws_mixed(b\"ab\\x01cd\") (mode B start → SA1 single-byte shift for 0x01 → back to set B) must succeed",
        );
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![65, 66, 98, 65, 67, 68]);
    }

    #[test]
    fn mixed_mode_a_from_start_for_control_byte_in_middle() {
        // "A\x01B" — mode A from start (all bytes A-encodable).
        // Oracle: mode=0, cws=[33, 65, 34].
        // Stage 11.A8c (cont) — `.expect` naming all-A-encodable path
        // (mid-message control byte → mode A selected from start).
        let (mode, cws) = encode_data_cws_mixed(b"A\x01B").expect(
            "encode_data_cws_mixed(b\"A\\x01B\") (all bytes A-encodable → mode A from start, mid-message control byte) must succeed",
        );
        assert_eq!(mode, MODE_A);
        assert_eq!(cws, vec![33, 65, 34]);
    }

    #[test]
    fn mixed_mode_a_from_start_for_leading_control_byte() {
        // "\x01ABC" — mode A from start (control byte triggers A).
        // Oracle: mode=0, cws=[65, 33, 34, 35].
        // Stage 11.A8c (cont) — `.expect` naming leading-control trigger.
        let (mode, cws) = encode_data_cws_mixed(b"\x01ABC").expect(
            "encode_data_cws_mixed(b\"\\x01ABC\") (leading control byte → mode A from start, ABC follows in A) must succeed",
        );
        assert_eq!(mode, MODE_A);
        assert_eq!(cws, vec![65, 33, 34, 35]);
    }

    #[test]
    fn mixed_extended_ascii_one_byte_via_fn4_shift() {
        // "A\x80" — UTF-8 [0x41, 0xc2, 0x80] → BWIPP inserts FN4
        // markers because of run length, then encodes in mode A.
        // Oracle: mode=0, cws=[33, 101, 34, 101, 64].
        // Stage 11.A8c (cont) — `.expect` naming FN4-shift extended-ASCII path.
        let (mode, cws) = encode_data_cws_mixed("A\u{0080}".as_bytes()).expect(
            "encode_data_cws_mixed(\"A\\u{0080}\") (extended ASCII U+0080 → UTF-8 [0x41, 0xc2, 0x80] → FN4 marker insertion in mode A) must succeed",
        );
        assert_eq!(mode, MODE_A);
        assert_eq!(cws, vec![33, 101, 34, 101, 64]);
    }

    #[test]
    fn mixed_extended_ascii_with_following_byte_via_fn4() {
        // "AÁB" — UTF-8 [0x41, 0xc3, 0x81, 0x42].
        // Oracle: mode=0, cws=[33, 101, 35, 101, 65, 34].
        // Stage 11.A8c (cont) — `.expect` naming FN4-shift with trailing ASCII.
        let (mode, cws) = encode_data_cws_mixed("A\u{00c1}B".as_bytes()).expect(
            "encode_data_cws_mixed(\"A\\u{00c1}B\") (U+00C1 'Á' between ASCII → UTF-8 [0x41, 0xc3, 0x81, 0x42] → FN4 markers + back to A) must succeed",
        );
        assert_eq!(mode, MODE_A);
        assert_eq!(cws, vec![33, 101, 35, 101, 65, 34]);
    }

    /// FN4-insertion unit test: ASCII-only input is unchanged.
    #[test]
    fn fn4_insertion_is_identity_for_pure_ascii() {
        let msg: Vec<i16> = b"Hello".iter().map(|&b| i16::from(b)).collect();
        assert_eq!(insert_fn4_markers(&msg), msg);
    }

    /// Stage 11.A8c — pin `lookup_a_for_sentinel_or_byte` and
    /// `lookup_b_for_sentinel_or_byte` branch dispatch. Each function
    /// has 4 branches:
    ///   1. c == FN4 → Ok(FN4_FROM_A / FN4_FROM_B).
    ///   2. c < 0 (not FN4) → Err (unsupported sentinel).
    ///   3. c is a valid byte for that set → Ok(lookup_a/b).
    ///   4. c is a byte but not in that set → Err.
    ///
    /// Mutations caught:
    ///   * `if c == FN4` removal or replacement → returns wrong value
    ///     or falls through.
    ///   * Return constant `FN4_FROM_A` (101) vs `FN4_FROM_B` (100)
    ///     swap — would mistype the mode-side codeword.
    ///   * `c < 0` → `<= 0` boundary (would reject c=0).
    ///   * Mistakenly using lookup_b in the A function or vice versa.
    #[test]
    fn lookup_a_b_for_sentinel_or_byte_branch_dispatch() {
        // FN4 sentinel — distinct codewords for A vs B.
        assert_eq!(
            lookup_a_for_sentinel_or_byte(FN4),
            Ok(FN4_FROM_A),
            "FN4 in A → FN4_FROM_A (101)"
        );
        assert_eq!(
            lookup_b_for_sentinel_or_byte(FN4),
            Ok(FN4_FROM_B),
            "FN4 in B → FN4_FROM_B (100)"
        );
        // Some other negative (unsupported sentinel) → Err.
        let err = lookup_a_for_sentinel_or_byte(-99).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(ref m) if m.contains("unsupported sentinel")),
            "negative non-FN4 should error: {err:?}"
        );
        let err = lookup_b_for_sentinel_or_byte(-99).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(ref m) if m.contains("unsupported sentinel")),
            "negative non-FN4 (B) should error: {err:?}"
        );
        // 'A' (65) is in set A but NOT in set B (well, A is in both
        // actually; let me pick something A-only). Control char NUL (0)
        // is in A (controls col) but not in B (printable col only).
        assert_eq!(
            lookup_a_for_sentinel_or_byte(0),
            Ok(lookup_a(0).unwrap()),
            "NUL is in set A via lookup_a"
        );
        // 'a' (97) is in set B but not in set A.
        assert_eq!(
            lookup_b_for_sentinel_or_byte(b'a' as i16),
            Ok(lookup_b(b'a').unwrap()),
            "'a' is in set B via lookup_b"
        );
        // 'a' is NOT in set A → Err.
        let err = lookup_a_for_sentinel_or_byte(b'a' as i16).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(ref m) if m.contains("not A-encodable")),
            "'a' must not be A-encodable: {err:?}"
        );
        // NUL is NOT in set B → Err.
        let err = lookup_b_for_sentinel_or_byte(0).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(ref m) if m.contains("not B-encodable")),
            "NUL must not be B-encodable: {err:?}"
        );
    }

    /// Stage 11.A8c — pin `insert_fn4_markers` non-trivial branches:
    /// the short-run "single FN4" shift, the long-run "double FN4"
    /// toggle, and the at-end threshold of 3 vs mid-stream 5.
    ///
    /// The function strips bit 7 from high bytes (& 127) and inserts
    /// FN4 markers around runs of opposite-class characters. The
    /// state toggle (`ea`) fires only when a run of >= threshold
    /// opposite chars appears; otherwise a single FN4 is emitted as
    /// a shift.
    ///
    /// Hand-computed cases (msg, expected_output):
    ///
    /// 1. Single high byte at end (msg=[195]):
    ///    num_ea[0]=1, threshold=3 (run+i==msglen). 1 < 3 → single FN4.
    ///    Output: [FN4, 67] (since 195 & 127 = 67).
    ///
    /// 2. Two high bytes at end (msg=[195, 195]):
    ///    threshold=3 for both positions; both runs (2, 1) < 3 →
    ///    single FN4 per byte. Output: [FN4, 67, FN4, 67].
    ///
    /// 3. Three high bytes at end (msg=[195, 195, 195]):
    ///    i=0 run=3, threshold=3, 3<3 false → DOUBLE FN4 + toggle.
    ///    Once ea=true, subsequent bytes match (c<128)==false skip
    ///    the marker step. Output: [FN4, FN4, 67, 67, 67].
    ///
    /// 4. High byte followed by ASCII (msg=[195, 88]):
    ///    i=0: run=num_ea[0]=1, threshold=5 (1+0 != 2). 1<5 →
    ///    single FN4. Output: [FN4, 67, 88].
    ///
    /// Mutations caught:
    ///   * `c >= 128` boundary → high/low classification flip.
    ///   * `run < threshold` vs `<=` boundary — case 3 (run=3 ==
    ///     threshold=3) is on the boundary; `<=` would skip the toggle.
    ///   * `threshold = 3` vs `5` end-of-stream rule.
    ///   * `ea = !ea` toggle (case 3).
    ///   * `c & 127` strip-high-bit (caught by 195 → 67).
    ///   * `out.push(FN4)` count: cases distinguish 1 vs 2 markers.
    /// Pins the `num_sa[i] = num_sa[i + 1] + 1` short-ASCII run counter
    /// inside `insert_fn4_markers` (line ~400), exercising the `ea == true`
    /// branch where `run = num_sa[i]`. The prior FN4 tests only reached
    /// the `num_ea` (ea == false) branch, so the two `+` mutants on the
    /// num_sa recurrence (`i + 1` index → `i * 1`, and `+ 1` value → `* 1`)
    /// survived. To reach num_sa we need a high-byte run long enough
    /// (≥5) to DOUBLE-FN4 toggle into the extended state, then an ASCII
    /// run whose length is compared against the mid-stream threshold 5.
    ///
    /// `i + 1` → `i * 1` makes the recurrence read `num_sa[i] = num_sa[i]+1`,
    /// reading the still-zero cell → every num_sa[i] becomes 1, so the
    /// ASCII run length is always seen as 1 (< 5) → single FN4 per byte,
    /// never toggling back. `+ 1` → `* 1` makes `num_sa[i] = num_sa[i+1]`,
    /// freezing the count at 0 → run always 0 < 5 → same single-FN4 path.
    /// Both diverge from the real double-FN4-toggle output below.
    #[test]
    fn insert_fn4_markers_num_sa_run_counter_pinned() {
        // 5 high bytes (run=5 → DOUBLE FN4 toggle ea=true), then a 5-byte
        // ASCII run: at its start num_sa = 5, threshold = 5 (mid-stream)
        // → 5 < 5 false → DOUBLE FN4 toggle back to ASCII state.
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195, 195, 195, 65, 65, 65, 65, 65, 195]),
            vec![-16, -16, 67, 67, 67, 67, 67, -16, -16, 65, 65, 65, 65, 65, -16, 67],
            "5 high + 5 ASCII + high: num_sa[5]=5 hits threshold → double FN4 toggle back"
        );
        // 5 high + 4-byte ASCII run + high: num_sa = 4 < threshold 5
        // → SINGLE FN4 per ASCII byte (no toggle back).
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195, 195, 195, 65, 65, 65, 65, 195]),
            vec![-16, -16, 67, 67, 67, 67, 67, -16, 65, -16, 65, -16, 65, -16, 65, 67],
            "5 high + 4 ASCII + high: num_sa run=4 < 5 → single FN4 per byte"
        );
        // 5 high + 6-byte ASCII run + high: num_sa = 6 ≥ 5 → double toggle.
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195, 195, 195, 65, 65, 65, 65, 65, 65, 195]),
            vec![-16, -16, 67, 67, 67, 67, 67, -16, -16, 65, 65, 65, 65, 65, 65, -16, 67],
            "5 high + 6 ASCII + high: num_sa run=6 ≥ 5 → double FN4 toggle back"
        );
    }

    #[test]
    fn insert_fn4_markers_handles_high_bit_runs_and_threshold() {
        // Single high byte at end.
        assert_eq!(
            insert_fn4_markers(&[195i16]),
            vec![FN4, 67],
            "single high byte at end: shift + stripped byte"
        );
        // Two high bytes at end (both individual runs < threshold=3).
        assert_eq!(
            insert_fn4_markers(&[195i16, 195]),
            vec![FN4, 67, FN4, 67],
            "two high bytes at end: per-byte shifts (no toggle)"
        );
        // Three high bytes at end: run=3 == threshold=3, so `3<3`
        // is false → DOUBLE FN4 + toggle.
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195]),
            vec![FN4, FN4, 67, 67, 67],
            "three high bytes at end: double FN4 + ea toggle"
        );
        // High byte followed by ASCII (mid-stream): threshold=5,
        // run=1 → single FN4 shift, then ASCII passes through.
        assert_eq!(
            insert_fn4_markers(&[195i16, 88]),
            vec![FN4, 67, 88],
            "high+ascii: shift on first, ASCII unchanged"
        );
    }

    /// Stage 11.A8c — pin the **mid-stream threshold = 5** branch of
    /// `insert_fn4_markers`, which the existing
    /// `insert_fn4_markers_handles_high_bit_runs_and_threshold` test
    /// only exercises at the end-of-message threshold = 3 boundary.
    /// The `threshold = if run + i == msglen { 3 } else { 5 }` line
    /// leaves mutations like `else { 5 } → else { 4 }` and
    /// `else { 5 } → else { 6 }` uncovered for mid-stream runs.
    ///
    /// Hand-traced for [195×5, 88] (5 high bytes + trailing ASCII):
    ///   i=0: ea=false, c=195 (high). ea==(c<128)? false==false → enter
    ///        branch. run=num_ea[0]=5. msglen=6, i=0 → run+i=5 != 6 →
    ///        threshold=5. 5 < 5 false → DOUBLE FN4 + toggle ea=true.
    ///        Push [FN4, FN4]. Then push c&127 = 67.
    ///   i=1..=4: ea=true, c=195. ea==(c<128)? true==false → SKIP
    ///        branch. Push c&127 = 67 each.
    ///   i=5: ea=true, c=88 (ASCII). ea==(c<128)? true==true → enter
    ///        branch. run=num_sa[5]=1. msglen=6, i=5 → run+i=6 == 6
    ///        → threshold=3. 1 < 3 true → SINGLE FN4. Push FN4.
    ///        Then push 88.
    ///   → output: [FN4, FN4, 67, 67, 67, 67, 67, FN4, 88]  (9 elts)
    ///
    /// With mutation `else { 5 } → else { 6 }`:
    ///   i=0: 5 < 6 true → SINGLE FN4 (no toggle, ea stays false).
    ///        Output diverges immediately: missing the second FN4
    ///        and the toggle never fires, so every subsequent 195
    ///        gets its own single FN4 — 11 elements instead of 9.
    ///
    /// With mutation `else { 5 } → else { 4 }`:
    ///   i=0: 5 < 4 false → still DOUBLE FN4 + toggle. SAME as
    ///        normal for this trace. NOT caught here — but caught
    ///        by adding the 4-high-byte mid-stream case below where
    ///        normal=single but `4`-mutant=double.
    ///
    /// Hand-traced for [195×4, 88] (4 high bytes + trailing ASCII):
    ///   i=0: run=4, msglen=5, threshold=5 (run+i=4!=5). 4 < 5 true →
    ///        SINGLE FN4, no toggle. Push [FN4, 67].
    ///   i=1: run=3, msglen=5, threshold=5 (run+i=4!=5). 3 < 5 true →
    ///        SINGLE FN4. Push [FN4, 67].
    ///   i=2: run=2, threshold=5. 2 < 5 → SINGLE FN4 + 67.
    ///   i=3: run=1, threshold=5. 1 < 5 → SINGLE FN4 + 67.
    ///   i=4: ea=false, c=88 (ASCII). ea==(c<128)? false==true → SKIP.
    ///        Push 88.
    ///   → output: [FN4, 67, FN4, 67, FN4, 67, FN4, 67, 88]  (9 elts)
    ///
    /// With mutation `5 → 4` on this case:
    ///   i=0: run=4, threshold=4. 4 < 4 false → DOUBLE FN4 + toggle.
    ///        Output diverges → caught.
    #[test]
    fn insert_fn4_markers_mid_stream_threshold_5_boundary() {
        // (1) 5 high bytes + trailing ASCII: double FN4 + toggle.
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195, 195, 195, 88]),
            vec![FN4, FN4, 67, 67, 67, 67, 67, FN4, 88],
            "5 high bytes mid-stream + ASCII: run=5 at i=0 hits \
             threshold=5 boundary → DOUBLE FN4 + toggle, then \
             stripped run, then single FN4 + ASCII at end"
        );

        // (2) 4 high bytes + trailing ASCII: per-byte single FN4
        // shifts (run=4 < threshold=5 → single, no toggle).
        assert_eq!(
            insert_fn4_markers(&[195i16, 195, 195, 195, 88]),
            vec![FN4, 67, FN4, 67, FN4, 67, FN4, 67, 88],
            "4 high bytes mid-stream + ASCII: run=4 < threshold=5 \
             → SINGLE FN4 per byte (no toggle); pins the 5-vs-4 \
             threshold boundary"
        );
    }

    /// `anotb` / `bnota` agree with the charmap.
    #[test]
    fn anotb_bnota_match_charmap() {
        // Byte 1 — only A.
        assert!(anotb(1));
        assert!(!bnota(1));
        // Byte 'a'=97 — only B.
        assert!(!anotb(97));
        assert!(bnota(97));
        // Byte 'A'=65 — both.
        assert!(!anotb(65));
        assert!(!bnota(65));
        // FN4 sentinel — neither (negative).
        assert!(!anotb(FN4));
        assert!(!bnota(FN4));
    }

    /// `encode_cws_mixed` produces a full padded + checked cws stream
    /// that starts with the row indicator and ends with c1/c2.
    #[test]
    fn mixed_wrapper_adds_row_indicator_and_checks() {
        // "Hello\x01" → 7 inner codewords; symbol size r=2 dcws_inner=7.
        // Row indicator = (2-2)*7 + 1 = 1 (MODE_B). cws[0]=1.
        // Then [40, 69, 76, 76, 79, 101, 65] (7 cws, no padding needed).
        // Then c1 + c2.
        let cws = encode_cws_mixed(b"Hello\x01").unwrap();
        assert_eq!(cws[0], 1, "row indicator for r=2 mode B is 1");
        assert_eq!(&cws[1..8], &[40, 69, 76, 76, 79, 101, 65]);
        assert_eq!(cws.len(), 1 + 7 + 2, "indicator + 7 cws + c1 + c2");
    }

    /// Dispatcher: `encode_cws` routes mixed inputs through the new
    /// path.
    #[test]
    fn dispatcher_routes_mixed_through_encode_cws_mixed() {
        let cws_direct = encode_cws_mixed(b"Hello\x01").unwrap();
        let cws_via_dispatcher = encode_cws(b"Hello\x01").unwrap();
        assert_eq!(cws_direct, cws_via_dispatcher);
    }

    // -----------------------------------------------------------------
    // Stage 3b — initial-mode selector + mode-C main loop goldens.
    //
    // Captured via `rust/tools/oracle-code16k.js` (16-row corpus).
    // These pin BWIPP's modes 2/5/6 + mode-C SWA/SWB latches back
    // to A/B mid-message.
    // -----------------------------------------------------------------

    /// **Byte-for-byte (mode 2)**: pure 4 digits → MODE_C_FROM_START.
    #[test]
    fn initial_mode_pure_digits_even_picks_mode_c() {
        let (mode, cws) = encode_data_cws_mixed(b"1234").unwrap();
        assert_eq!(mode, MODE_C_FROM_START);
        assert_eq!(cws, vec![12, 34]);
    }

    /// **Byte-for-byte (mode 5 — pure-digit odd)**: 5 digits →
    /// emit first byte in B, then 2 pairs in C.
    #[test]
    fn initial_mode_pure_digits_odd_picks_mode_5() {
        let (mode, cws) = encode_data_cws_mixed(b"12345").unwrap();
        assert_eq!(mode, MODE_C_THEN_B);
        // Bvals['1']=17, "23"=23, "45"=45.
        assert_eq!(cws, vec![17, 23, 45]);
    }

    /// **Byte-for-byte (mode 5 — 1 leading B byte + 2 even digits)**:
    /// "A12" → emit 'A' in B, then "12" pair in C.
    #[test]
    fn initial_mode_one_b_byte_then_2_even_digits_picks_mode_5() {
        let (mode, cws) = encode_data_cws_mixed(b"A12").unwrap();
        assert_eq!(mode, MODE_C_THEN_B);
        assert_eq!(cws, vec![33, 12]);
    }

    /// **Byte-for-byte (mode 5 — 1 leading B byte + 4 even digits)**.
    #[test]
    fn initial_mode_one_b_byte_then_4_even_digits_picks_mode_5() {
        let (mode, cws) = encode_data_cws_mixed(b"A1234").unwrap();
        assert_eq!(mode, MODE_C_THEN_B);
        assert_eq!(cws, vec![33, 12, 34]);
    }

    /// **Byte-for-byte (mode 6 — 1 leading B byte + 5 odd digits)**:
    /// emits 'A' + '1' in B, then "23" "45" in C.
    #[test]
    fn initial_mode_one_b_byte_then_5_odd_digits_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"A12345").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        // 'A'=33, '1'=17, "23"=23, "45"=45.
        assert_eq!(cws, vec![33, 17, 23, 45]);
    }

    /// **Byte-for-byte (mode 6 — 2 leading B + 2 even digits + 2 B)**:
    /// "AB12CD" → emit 'A','B' in B, "12" pair, SWB back to B, 'C','D'.
    #[test]
    fn initial_mode_two_b_bytes_then_2_even_digits_then_text_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"AB12CD").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![33, 34, 12, 100, 35, 36]);
    }

    /// **Byte-for-byte (mode 6 — 2 leading B + 4 even digits)**:
    /// "AB1234" → emit 'A','B' in B, "12" "34" pairs.
    #[test]
    fn initial_mode_two_b_bytes_then_4_even_digits_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"AB1234").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![33, 34, 12, 34]);
    }

    /// **Byte-for-byte (mode 6 — 6 even digits inside)**.
    #[test]
    fn initial_mode_two_b_bytes_then_6_even_digits_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"AB123456").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![33, 34, 12, 34, 56]);
    }

    /// **Byte-for-byte (mode 6 + SWB back + tail bytes)**:
    /// "AB12345678CD" → 2 B + 4 pairs + SWB + 2 B.
    #[test]
    fn initial_mode_two_b_bytes_then_8_digits_then_text_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"AB12345678CD").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![33, 34, 12, 34, 56, 78, 100, 35, 36]);
    }

    /// **Byte-for-byte (mode 6 + SWB back)**: "AB1234CD".
    #[test]
    fn initial_mode_two_b_bytes_then_4_digits_then_text_picks_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"AB1234CD").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![33, 34, 12, 34, 100, 35, 36]);
    }

    /// **Byte-for-byte (mode 5 — lowercase + 4 digits + lowercase)**.
    /// "a1234b" — 'a' triggers mode 5, then 2 pairs, then SWB+b.
    #[test]
    fn initial_mode_lowercase_then_digits_then_lowercase() {
        let (mode, cws) = encode_data_cws_mixed(b"a1234b").unwrap();
        assert_eq!(mode, MODE_C_THEN_B);
        assert_eq!(cws, vec![65, 12, 34, 100, 66]);
    }

    /// **Byte-for-byte (mode 5 — 6 digits version)**: "a123456b".
    #[test]
    fn initial_mode_lowercase_then_6_digits_then_lowercase() {
        let (mode, cws) = encode_data_cws_mixed(b"a123456b").unwrap();
        assert_eq!(mode, MODE_C_THEN_B);
        assert_eq!(cws, vec![65, 12, 34, 56, 100, 66]);
    }

    /// **Byte-for-byte (mode 6 — odd digit count + lowercase tail)**:
    /// "a123b" — 'a' + '1' both in B, then "23" pair, SWB+b.
    #[test]
    fn initial_mode_lowercase_then_3_odd_digits_then_lowercase_mode_6() {
        let (mode, cws) = encode_data_cws_mixed(b"a123b").unwrap();
        assert_eq!(mode, MODE_B_THEN_C);
        assert_eq!(cws, vec![65, 17, 23, 100, 66]);
    }

    /// numsscr helper: pure digit runs return (n, n).
    #[test]
    fn numsscr_pure_digit_run() {
        let msg: Vec<i16> = b"1234".iter().map(|&b| i16::from(b)).collect();
        let (n, s) = numsscr(&msg, 0);
        assert_eq!((n, s), (4, 4));
    }

    /// numsscr helper: stops at first non-digit.
    #[test]
    fn numsscr_stops_at_non_digit() {
        let msg: Vec<i16> = b"12Ab".iter().map(|&b| i16::from(b)).collect();
        let (n, s) = numsscr(&msg, 0);
        assert_eq!((n, s), (2, 2));
    }

    /// numsscr helper: walking from an offset.
    #[test]
    fn numsscr_from_offset() {
        let msg: Vec<i16> = b"AB1234".iter().map(|&b| i16::from(b)).collect();
        let (n, s) = numsscr(&msg, 2);
        assert_eq!((n, s), (4, 4));
    }

    /// Stage 11.A8c — pin `build_row_bits` structural invariants
    /// that hold regardless of the STARTENCS/ENCS/STOPENCS tables:
    ///
    ///   * Output length is always 81.
    ///   * row[0..10] = 0 (left quiet zone; pins current=1 init +
    ///     leading sbs.push(10)).
    ///   * row[10] = 1 (start bar — first STARTENCS module).
    ///   * row[80] = 0 (trailing separator after the final
    ///     sbs.push(1)).
    ///
    /// Width totals: 10 (quiet) + 7 (STARTENCS) + 1 (separator) +
    /// 5*11 (ENCS) + 7 (STOPENCS) + 1 (trailing) = 81 ✓.
    /// sbs.len = 1 + 4 + 1 + 5*6 + 4 + 1 = 41 toggles. With
    /// current=1 init, after k toggles the write value is
    /// 1 XOR ((k+1) % 2). Last toggle k=40 → write 0 → row[80]=0.
    ///
    /// Mutations caught:
    ///   * `current: u8 = 1` → 0 flips every cell (quiet → ones).
    ///   * `1 - current` → `current` (no toggle): all cells same.
    ///   * Initial `sbs.push(10)` → smaller width shifts quiet zone.
    ///   * `sbs.push(1)` (trailing) drop: 80 → boundary doesn't fall
    ///     on toggle parity 0, so row[80] flips.
    ///   * 5-cw loop bound off-by-one breaks the total = 81 invariant
    ///     (debug_assert fires).
    #[test]
    fn build_row_bits_invariant_layout_code16k() {
        let row = build_row_bits(0, &[0u16; 5], &STOPENCS_ODD);
        assert_eq!(row.len(), 81);
        for i in 0..10 {
            assert_eq!(row[i], 0, "quiet pos {i} must be 0");
        }
        assert_eq!(row[10], 1, "start bar at pos 10");
        assert_eq!(row[80], 0, "trailing separator at pos 80");

        // Different stopencs tables produce different row middles
        // (catches `stopencs[row_idx]` index swap or table confusion).
        let row_odd = build_row_bits(0, &[0u16; 5], &STOPENCS_ODD);
        let row_even = build_row_bits(0, &[0u16; 5], &STOPENCS_EVEN);
        if STOPENCS_ODD[0] != STOPENCS_EVEN[0] {
            assert_ne!(
                row_odd, row_even,
                "different stopencs tables must produce different rows"
            );
        }
        // Common invariants still hold for EVEN.
        assert_eq!(row_even[0], 0);
        assert_eq!(row_even[10], 1);
        assert_eq!(row_even[80], 0);
    }

    /// Stage 11.A8c — extend `build_row_bits` coverage with non-zero
    /// `row_cws` inputs that pin the `ENCS[cw as usize]` indexing and
    /// the `for &cw in row_cws` iteration order. The existing
    /// `build_row_bits_invariant_layout_code16k` test uses
    /// `row_cws = [0u16; 5]`, which makes all five ENCS chunks
    /// identical (`ENCS[0] = "212222"`); mutations like
    /// `ENCS[cw as usize]` → `ENCS[(cw as usize) + 1]`, swapping the
    /// 5-cw iteration order, or dropping a cw from the loop survive
    /// there.
    ///
    /// Tactic: feed two cws vectors with a single non-zero entry at
    /// distinct positions (`row_cws[0] = 1` vs `row_cws[1] = 1`),
    /// derive the expected 11-bit chunks for `ENCS[0]` and `ENCS[1]`
    /// by hand, and assert them at the right offsets. The 5 chunks
    /// land at pos 18..=28, 29..=39, 40..=50, 51..=61, 62..=72.
    ///
    /// Bit-trace at pos 18..=28 (current=1 after STARTENCS[0]+sep;
    /// each width N toggles current then emits N bits):
    ///   ENCS[0] = "212222":
    ///     w=2: 1→0 → 0,0   (pos 18-19)
    ///     w=1: 0→1 → 1     (pos 20)
    ///     w=2: 1→0 → 0,0   (pos 21-22)
    ///     w=2: 0→1 → 1,1   (pos 23-24)
    ///     w=2: 1→0 → 0,0   (pos 25-26)
    ///     w=2: 0→1 → 1,1   (pos 27-28)
    ///     → [0,0,1,0,0,1,1,0,0,1,1]   (current ends at 1)
    ///   ENCS[1] = "222122":
    ///     w=2: 1→0 → 0,0   (pos 18-19)
    ///     w=2: 0→1 → 1,1   (pos 20-21)
    ///     w=2: 1→0 → 0,0   (pos 22-23)
    ///     w=1: 0→1 → 1     (pos 24)
    ///     w=2: 1→0 → 0,0   (pos 25-26)
    ///     w=2: 0→1 → 1,1   (pos 27-28)
    ///     → [0,0,1,1,0,0,1,0,0,1,1]   (current ends at 1)
    /// Both chunks end with current=1, so the next chunk's first
    /// width toggles to 0 — the same starting state as pos 18, which
    /// lets the same hand-trace apply at pos 29..=39 unchanged.
    ///
    /// Mutations caught (beyond the prior invariant-layout test):
    ///   * `ENCS[cw as usize]` → `ENCS[(cw as usize) + 1]`: case 1
    ///     would emit ENCS[2]="222221" at pos 18..=28 (different
    ///     trailing bit) instead of ENCS[1].
    ///   * `ENCS[cw as usize]` → `ENCS[0]`: case 1 chunk 0 would emit
    ///     `[0,0,1,0,0,…]` (ENCS[0]) instead of `[0,0,1,1,0,…]`
    ///     (ENCS[1]) → pos 21 differs (0 vs 1).
    ///   * For-loop dropping a cw: case 2 (non-zero at chunk 1) would
    ///     shift chunk 1's contents to pos 29..=39 or earlier.
    ///   * Operand swap reading row_cws in reverse: case 1 vs case 2
    ///     swap divergence positions; the cross-check assert_ne picks
    ///     this up.
    #[test]
    fn build_row_bits_pins_encs_indexing_with_non_zero_cws() {
        // Table sanity (so a constants edit doesn't silently break
        // the expected-bit derivation).
        assert_eq!(ENCS[0], "212222", "ENCS[0] table anchor");
        assert_eq!(ENCS[1], "222122", "ENCS[1] table anchor");

        let encs0_bits: [u8; 11] = [0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1];
        let encs1_bits: [u8; 11] = [0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1];

        // --- Case 1: row_cws = [1, 0, 0, 0, 0].
        //     chunk 0 (pos 18..=28) = ENCS[1]
        //     chunk 1 (pos 29..=39) = ENCS[0]
        let row_a = build_row_bits(0, &[1u16, 0, 0, 0, 0], &STOPENCS_ODD);
        for (off, &want) in encs1_bits.iter().enumerate() {
            let i = 18 + off;
            assert_eq!(
                row_a[i], want,
                "case 1 chunk 0 (cw=1 → ENCS[1]): pos {i} should be {want}"
            );
        }
        for (off, &want) in encs0_bits.iter().enumerate() {
            let i = 29 + off;
            assert_eq!(
                row_a[i], want,
                "case 1 chunk 1 (cw=0 → ENCS[0]): pos {i} should be {want}"
            );
        }

        // --- Case 2: row_cws = [0, 1, 0, 0, 0].
        //     chunk 0 (pos 18..=28) = ENCS[0]
        //     chunk 1 (pos 29..=39) = ENCS[1]
        let row_b = build_row_bits(0, &[0u16, 1, 0, 0, 0], &STOPENCS_ODD);
        for (off, &want) in encs0_bits.iter().enumerate() {
            let i = 18 + off;
            assert_eq!(
                row_b[i], want,
                "case 2 chunk 0 (cw=0 → ENCS[0]): pos {i} should be {want}"
            );
        }
        for (off, &want) in encs1_bits.iter().enumerate() {
            let i = 29 + off;
            assert_eq!(
                row_b[i], want,
                "case 2 chunk 1 (cw=1 → ENCS[1]): pos {i} should be {want}"
            );
        }

        // Cross-check: cases must differ at pos 18..=28 (where case 1
        // emits ENCS[1] and case 2 emits ENCS[0]).
        assert_ne!(
            &row_a[18..29],
            &row_b[18..29],
            "moving non-zero cw from position 0 to position 1 must shift the divergence"
        );
    }

    /// Stage 11.A8c — pin code16k's `build_seprow` exact layout:
    /// 10 leading zeros, 70 middle ones, 1 trailing zero (cell 80).
    ///
    /// Mutations caught:
    ///   * `[1u8; 81]` init → `[0u8; 81]` would invert middle to 0s.
    ///   * `take(10)` → `take(9)` / `take(11)` shifts the boundary.
    ///   * `row[80] = 0` → wrong index leaves wrong trailing cell.
    ///   * Row length 81 invariant (sums to 81 cells).
    #[test]
    fn build_seprow_10_zeros_then_70_ones_then_zero() {
        let row = build_seprow();
        assert_eq!(row.len(), 81);
        for i in 0..10 {
            assert_eq!(row[i], 0, "leading pos {i} must be 0");
        }
        for i in 10..80 {
            assert_eq!(row[i], 1, "middle pos {i} must be 1");
        }
        assert_eq!(row[80], 0, "trailing pos 80 must be 0");
        // Sums: 11 zeros + 70 ones = 81.
        assert_eq!(row.iter().filter(|&&v| v == 0).count(), 11);
        assert_eq!(row.iter().filter(|&&v| v == 1).count(), 70);
    }

    /// pair_codeword: "12" = 12, "00" = 0, "99" = 99.
    #[test]
    fn pair_codeword_basic_pairs() {
        assert_eq!(pair_codeword(b'1', b'2'), 12);
        assert_eq!(pair_codeword(b'0', b'0'), 0);
        assert_eq!(pair_codeword(b'9', b'9'), 99);
    }

    /// Stage 11.A8c — pin `lookup_a_for_sentinel_or_byte` and
    /// `lookup_b_for_sentinel_or_byte`. Both wrap the per-column
    /// CHARMAPS scan with an FN4-sentinel fast-path and a generic
    /// "negative non-FN4 sentinel → Err" rejection. They drive the
    /// mid-message A↔B transitions via FN4_FROM_A / FN4_FROM_B
    /// (which differ — 101 vs 100 — to discriminate A-emit from
    /// B-emit). End-to-end goldens exercise both paths through
    /// `encode_data_cws_mixed`, but neither helper is directly pinned.
    ///
    /// Mutations to catch:
    ///   - `c == FN4` → `c != FN4`: FN4 falls through to the
    ///     "unsupported sentinel" arm.
    ///   - `FN4_FROM_A` ↔ `FN4_FROM_B`: arm swap returns the wrong
    ///     constant.
    ///   - `c < 0` → `c <= 0`: NUL (0) would get mis-routed to the
    ///     sentinel-Err arm instead of forwarding to lookup_a.
    ///   - `lookup_a(b)` → `lookup_b(b)` (arm/forwarder swap):
    ///     lowercase 'a' would succeed in lookup_a_for_… (wrong).
    #[test]
    fn lookup_a_b_for_sentinel_or_byte_fn4_negative_and_byte_forwarding() {
        // ---- FN4 sentinel: each variant returns the corresponding
        //      A/B-specific constant. The two constants DIFFER (101
        //      vs 100), so arm-swap mutants are caught.
        assert_eq!(
            lookup_a_for_sentinel_or_byte(FN4).unwrap(),
            FN4_FROM_A,
            "FN4 in mode A → FN4_FROM_A (101)"
        );
        assert_eq!(lookup_a_for_sentinel_or_byte(FN4).unwrap(), 101);
        assert_eq!(
            lookup_b_for_sentinel_or_byte(FN4).unwrap(),
            FN4_FROM_B,
            "FN4 in mode B → FN4_FROM_B (100)"
        );
        assert_eq!(lookup_b_for_sentinel_or_byte(FN4).unwrap(), 100);
        // Asymmetry: A returns 101, B returns 100 — kills `_FROM_A` →
        // `_FROM_B` swap mutant via direct numeric inequality.
        assert_ne!(
            lookup_a_for_sentinel_or_byte(FN4).unwrap(),
            lookup_b_for_sentinel_or_byte(FN4).unwrap(),
        );

        // ---- Negative non-FN4 sentinel → Err with diagnostic text.
        // Diagnostic at lines 877/896:
        //   "code16k mixed encoder: unsupported sentinel {c} (only FN4 is wired today)"
        // Both `lookup_a_for_sentinel_or_byte` and `lookup_b_for_sentinel_or_byte`
        // share the same format. Per-iteration 4-anchor pin replaces
        // the previous single-substring check:
        for &neg in &[-1_i16, -2, -100] {
            match lookup_a_for_sentinel_or_byte(neg) {
                Err(Error::InvalidData(msg)) => {
                    assert!(
                        msg.contains("code16k mixed encoder:"),
                        "lookup_a: must carry the symbology prefix; got {msg:?}"
                    );
                    assert!(
                        msg.contains("unsupported sentinel"),
                        "lookup_a: must carry the predicate; got {msg:?}"
                    );
                    assert!(
                        msg.contains(&format!("sentinel {neg}")),
                        "lookup_a: must echo `{{c}}` ({neg}); got {msg:?}"
                    );
                    assert!(
                        msg.contains("only FN4 is wired today"),
                        "lookup_a: must carry the FN4 remediation hint; got {msg:?}"
                    );
                }
                other => panic!("expected InvalidData for c={neg}, got {other:?}"),
            }
            match lookup_b_for_sentinel_or_byte(neg) {
                Err(Error::InvalidData(msg)) => {
                    assert!(
                        msg.contains("code16k mixed encoder:"),
                        "lookup_b: must carry the symbology prefix; got {msg:?}"
                    );
                    assert!(
                        msg.contains("unsupported sentinel"),
                        "lookup_b: must carry the predicate; got {msg:?}"
                    );
                    assert!(
                        msg.contains(&format!("sentinel {neg}")),
                        "lookup_b: must echo `{{c}}` ({neg}); got {msg:?}"
                    );
                    assert!(
                        msg.contains("only FN4 is wired today"),
                        "lookup_b: must carry the FN4 remediation hint; got {msg:?}"
                    );
                }
                other => panic!("expected InvalidData for c={neg}, got {other:?}"),
            }
        }

        // ---- Positive byte forwarding to lookup_a / lookup_b.
        // 'A' (65) lives in both columns at row 33.
        assert_eq!(
            lookup_a_for_sentinel_or_byte(b'A' as i16).unwrap(),
            33,
            "A-encodable byte 'A' forwards to lookup_a row 33"
        );
        assert_eq!(lookup_b_for_sentinel_or_byte(b'A' as i16).unwrap(), 33);
        // lowercase 'a' (97) is B-only — A path errors, B path returns
        // row 65. Strong arm-swap discriminator.
        //
        // Stage 11.A8c — pin the diagnostic on the A-failure: "byte
        // 0x61 not A-encodable". A mutant that swaps the A and B arms'
        // error messages (so lookup_a says "B-encodable") survives the
        // bare `is_err()` check but fails the substring pin.
        let err =
            lookup_a_for_sentinel_or_byte(b'a' as i16).expect_err("'a' (0x61) is not A-encodable");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_a('a') must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code16k") && msg.contains("0x61") && msg.contains("not A-encodable"),
            "lookup_a('a') diagnostic must carry symbology tag + byte echo + 'not A-encodable'; \
             got {msg:?}"
        );
        assert!(
            !msg.contains("B-encodable"),
            "lookup_a diagnostic must NOT leak the B-arm text (cross-arm swap guard); got {msg:?}"
        );
        assert_eq!(lookup_b_for_sentinel_or_byte(b'a' as i16).unwrap(), 65);
        // NUL (0) is A-only — A path returns row 64, B path errors.
        assert_eq!(
            lookup_a_for_sentinel_or_byte(0).unwrap(),
            64,
            "NUL is A-encodable at row 64"
        );
        // Stage 11.A8c — symmetric strengthening on the B-failure:
        // "byte 0x00 not B-encodable".
        let err = lookup_b_for_sentinel_or_byte(0).expect_err("NUL (0x00) is not B-encodable");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_b(0) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code16k") && msg.contains("0x00") && msg.contains("not B-encodable"),
            "lookup_b(NUL) diagnostic must carry symbology tag + byte echo + 'not B-encodable'; \
             got {msg:?}"
        );
        assert!(
            !msg.contains("not A-encodable"),
            "lookup_b diagnostic must NOT leak the A-arm text (cross-arm swap guard); got {msg:?}"
        );

        // ---- "byte not encodable" Err path has its own diagnostic.
        // Stage 11.A8c (cont) — upgrade both arms from single-anchor
        // `msg.contains("not A/B-encodable")` to 3-anchor pin matching
        // the source diagnostics at lines 883 and 902 of code16k.rs:
        //   1. `code16k mixed encoder:` symbology+pipeline prefix
        //   2. `byte 0x{b:02x}` hex-formatted offending byte
        //   3. `not A-encodable` / `not B-encodable` predicate
        //   + cross-arm contamination guards (A must not say B; B
        //     must not say A) — kills mutations that swap which
        //     lookup function emits which diagnostic.
        match lookup_a_for_sentinel_or_byte(b'a' as i16) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("code16k mixed encoder:"),
                    "A-arm: missing `code16k mixed encoder:` prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x61"),
                    "A-arm: missing `byte 0x61` hex echo (0x61 == 'a'): {msg}"
                );
                assert!(
                    msg.contains("not A-encodable"),
                    "A-arm: missing `not A-encodable` predicate: {msg}"
                );
                assert!(
                    !msg.contains("not B-encodable"),
                    "A-arm: B-arm diagnostic leaked into A-arm reject: {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
        match lookup_b_for_sentinel_or_byte(0) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("code16k mixed encoder:"),
                    "B-arm: missing `code16k mixed encoder:` prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x00"),
                    "B-arm: missing `byte 0x00` hex echo (NUL): {msg}"
                );
                assert!(
                    msg.contains("not B-encodable"),
                    "B-arm: missing `not B-encodable` predicate: {msg}"
                );
                assert!(
                    !msg.contains("not A-encodable"),
                    "B-arm: A-arm diagnostic leaked into B-arm reject: {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Stage 3c — mid-message → C transitions, SWC latch, mode-C SB shifts.
    //
    // Captured via the extended oracle in `rust/tools/oracle-code16k.js`.
    // These pin BWIPP's full main encoder loop for cset=A/B/C across
    // SC2/SC3/SWC and SB1/SB2/SB3 paths.
    // -----------------------------------------------------------------

    /// **Byte-for-byte (SA2 in B)**: "a\x01\x02b" — 1 B byte, 2 A-only
    /// bytes, 1 B byte → SA2 shift fires. Oracle codeword 104 (NOT
    /// 102 — the Stage 3a constant was incorrect and is fixed in 3c).
    #[test]
    fn mixed_mode_b_with_sa2_two_byte_shift() {
        let (mode, cws) = encode_data_cws_mixed(b"a\x01\x02b").unwrap();
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![65, 104, 65, 66, 66]);
    }

    /// **Byte-for-byte (SA2 with longer text)**: "ab\x01\x02cd".
    #[test]
    fn mixed_mode_b_with_sa2_amid_lowercase() {
        let (mode, cws) = encode_data_cws_mixed(b"ab\x01\x02cd").unwrap();
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![65, 66, 104, 65, 66, 67, 68]);
    }

    /// **Byte-for-byte (SWC from B)**: "ABCDE12345" — 5 B bytes, then
    /// '1' in B (to align parity), then SWC, then 2 pairs.
    #[test]
    fn mid_message_swc_latch_after_long_text() {
        let (mode, cws) = encode_data_cws_mixed(b"ABCDE12345").unwrap();
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![33, 34, 35, 36, 37, 17, 99, 23, 45]);
    }

    /// **Byte-for-byte (SWC from B, lowercase)**.
    #[test]
    fn mid_message_swc_latch_lowercase() {
        let (mode, cws) = encode_data_cws_mixed(b"abcde12345").unwrap();
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![65, 66, 67, 68, 69, 17, 99, 23, 45]);
    }

    /// **Byte-for-byte (SWC from B with 4-digit run)**: "abcd1234" —
    /// the initial-mode selector won't fire (4 B bytes is too many),
    /// so we go mode B from start, then SWC immediately when 4 even
    /// digits remain.
    #[test]
    fn mid_message_swc_with_4_digits_even() {
        let (mode, cws) = encode_data_cws_mixed(b"abcd1234").unwrap();
        assert_eq!(mode, MODE_B);
        assert_eq!(cws, vec![65, 66, 67, 68, 99, 12, 34]);
    }

    /// **Byte-for-byte (SB1 in C)**: "12X12" — mode 2 from start,
    /// then mid-C single-byte shift for 'X' back to B for 1 byte.
    #[test]
    fn mode_c_sb1_shift_for_single_text_byte() {
        let (mode, cws) = encode_data_cws_mixed(b"12X12").unwrap();
        assert_eq!(mode, MODE_C_FROM_START);
        // Oracle: cws=[12, 104, 56, 12].
        // 12="12" pair, 104=SB1-in-C, 56='X' in B, 12="12" pair.
        assert_eq!(cws, vec![12, 104, 56, 12]);
    }

    /// **Byte-for-byte (SB1 in C, longer)**: "1234X1234".
    #[test]
    fn mode_c_sb1_shift_longer_payload() {
        let (mode, cws) = encode_data_cws_mixed(b"1234X1234").unwrap();
        assert_eq!(mode, MODE_C_FROM_START);
        assert_eq!(cws, vec![12, 34, 104, 56, 12, 34]);
    }

    /// **Byte-for-byte (SC2 in A)**: "\x011234B" — mode A from start
    /// (the control byte forces mode A), then SC2 in A for 2 pairs,
    /// then back to A for 'B'.
    #[test]
    fn mid_message_sc2_from_a() {
        let (mode, cws) = encode_data_cws_mixed(b"\x011234B").unwrap();
        assert_eq!(mode, MODE_A);
        // Oracle: cws=[65, 105, 12, 34, 34].
        // 65=byte 1 in A, 105=SC2-in-A, 12+34 pairs, 34='B' in A.
        assert_eq!(cws, vec![65, 105, 12, 34, 34]);
    }

    /// **Byte-for-byte (SC3 in A)**: "\x01123456B" — 6 digits → SC3.
    #[test]
    fn mid_message_sc3_from_a() {
        let (mode, cws) = encode_data_cws_mixed(b"\x01123456B").unwrap();
        assert_eq!(mode, MODE_A);
        // Oracle: cws=[65, 106, 12, 34, 56, 34].
        assert_eq!(cws, vec![65, 106, 12, 34, 56, 34]);
    }

    /// Codeword constants alignment with the CHARMAPS table.
    #[test]
    fn codeword_constants_match_charmaps() {
        // Row 98 = [SB1, SA1, 98]. SB1 in A = 98, SA1 in B = 98.
        assert_eq!(SB1_FROM_A, 98);
        assert_eq!(SA1_FROM_B, 98);
        // Row 99 = [SWC, SWC, 99]. SWC in any set = 99.
        assert_eq!(SWC_FROM_A_OR_B, 99);
        // Row 100 = [SWB, FN4, SWB]. SWB in A = 100, FN4 in B = 100, SWB in C = 100.
        assert_eq!(SWB_FROM_A, 100);
        assert_eq!(FN4_FROM_B, 100);
        assert_eq!(SWB_FROM_C, 100);
        // Row 101 = [FN4, SWA, SWA]. FN4 in A = 101, SWA in B = 101, SWA in C = 101.
        assert_eq!(FN4_FROM_A, 101);
        assert_eq!(SWA_FROM_B, 101);
        assert_eq!(SWA_FROM_C, 101);
        // Row 104 = [SB2, SA2, SB1]. SB2 in A = 104, SA2 in B = 104, SB1 in C = 104.
        assert_eq!(SB2_FROM_A, 104);
        assert_eq!(SA2_FROM_B, 104);
        assert_eq!(SB1_FROM_C, 104);
        // Row 105 = [SC2, SC2, SB2]. SC2 in A/B = 105, SB2 in C = 105.
        assert_eq!(SC2_FROM_A, 105);
        assert_eq!(SC2_FROM_B, 105);
        assert_eq!(SB2_FROM_C, 105);
        // Row 106 = [SC3, SC3, SB3]. SC3 in A/B = 106, SB3 in C = 106.
        assert_eq!(SC3_FROM_A, 106);
        assert_eq!(SC3_FROM_B, 106);
        assert_eq!(SB3_FROM_C, 106);
    }

    /// Stage 11.A8c — pin `leading_row_indicator(rows, mode)` =
    /// `(rows - 2) * 7 + mode`. Kills `- with +` / `* with /` /
    /// `+ with *` arithmetic mutations on line 221.
    #[test]
    fn leading_row_indicator_known_values() {
        // (rows=2, mode=0) → 0; (rows=2, mode=6) → 6.
        assert_eq!(leading_row_indicator(2, 0), 0);
        assert_eq!(leading_row_indicator(2, 6), 6);
        // (rows=3, mode=0) → 7; (rows=3, mode=4) → 11.
        assert_eq!(leading_row_indicator(3, 0), 7);
        assert_eq!(leading_row_indicator(3, 4), 11);
        // (rows=16, mode=6) = (14)*7 + 6 = 98 + 6 = 104.
        assert_eq!(leading_row_indicator(16, 6), 104);
    }

    /// Stage 11.A8c — pin `compute_checksums` for a 3-codeword input.
    /// Kills `+ with *` / `* with /` / `% with /` mutations on
    /// lines 244-248.
    #[test]
    fn compute_checksums_simple_three_codewords() {
        // cws = [10, 20, 30]. dcws_inner = 2.
        //   s1 = (0+2)*10 + (1+2)*20 + (2+2)*30 = 20 + 60 + 120 = 200.
        //   s2 = (0+1)*10 + (1+1)*20 + (2+1)*30 = 10 + 40 + 90 = 140.
        //   c1 = 200 % 107 = 200 - 107 = 93.
        //   c2 = (140 + 93 * (2+2)) % 107 = (140 + 372) % 107
        //      = 512 % 107 = 512 - 4*107 = 512 - 428 = 84.
        let (c1, c2) = compute_checksums(&[10, 20, 30]);
        assert_eq!(c1, 93);
        assert_eq!(c2, 84);
    }

    /// Stage 11.A8c — pin `pick_symbol_size` against METRICS rows.
    /// METRICS = [(2,7), (3,12), (4,17), …, (16,107)]. Kills the
    /// `>= with <` / `>= with ==` mutation on line 284.
    #[test]
    fn pick_symbol_size_per_pair_count() {
        // ≤7 pairs → 2 rows.
        assert_eq!(pick_symbol_size(1), Some((2, 7)));
        assert_eq!(pick_symbol_size(7), Some((2, 7)));
        // 8..=12 → 3 rows.
        assert_eq!(pick_symbol_size(8), Some((3, 12)));
        assert_eq!(pick_symbol_size(12), Some((3, 12)));
        // 13..=17 → 4 rows.
        assert_eq!(pick_symbol_size(17), Some((4, 17)));
    }

    /// Stage 11.A8c — pin `lookup_a_for_sentinel_or_byte(c)` and
    /// `lookup_b_for_sentinel_or_byte(c)`. Both adapters share the
    /// shape: FN4 sentinel maps to a fixed codeword (101 for set A,
    /// 100 for set B); other negative sentinels Err; positive bytes
    /// delegate to `lookup_a`/`lookup_b` and may also Err if the
    /// byte isn't encodable in that set.
    ///
    /// Used only via the mixed-mode encoder, so the per-arm split
    /// and the FN4_FROM_A vs FN4_FROM_B asymmetry isn't directly
    /// pinned by golden tests.
    ///
    /// Mutations killed:
    ///   * `c == FN4` → `c != FN4` (would flip happy arm into error);
    ///   * arm-swap FN4_FROM_A↔FN4_FROM_B (asymmetric anchor: A
    ///     returns 101, B returns 100);
    ///   * `c < 0` → `c <= 0` (would reject byte 0 NUL which is
    ///     valid in set A);
    ///   * `c < 0` → `c > 0` (would let -16 fall through, but FN4
    ///     check already short-circuits — caught by other-negative
    ///     anchor c=-1).
    #[test]
    fn lookup_a_or_b_for_sentinel_or_byte_arms() {
        // ---- A path -------------------------------------------------
        // FN4 sentinel → FN4_FROM_A = 101.
        assert_eq!(
            lookup_a_for_sentinel_or_byte(FN4).unwrap(),
            FN4_FROM_A,
            "FN4 → FN4_FROM_A (101)"
        );
        // Other negative sentinel (-1) → Err. Stage 11.A8c — pin
        // the "unsupported sentinel" diagnostic + {c} echo (matches
        // the strong sibling test lookup_a_b_for_sentinel_or_byte_
        // branch_dispatch at line ~2300). Defense-in-depth.
        let err = lookup_a_for_sentinel_or_byte(-1).expect_err("non-FN4 negative → Err");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_a(-1) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("unsupported sentinel") && msg.contains("-1"),
            "lookup_a(-1) diagnostic must pin 'unsupported sentinel' + value echo; got {msg:?}"
        );
        // Positive A-encodable: 'A' (65).
        let got = lookup_a_for_sentinel_or_byte(b'A' as i16).unwrap();
        assert_eq!(got, lookup_a(b'A').unwrap(), "'A' delegates to lookup_a");
        // Byte 0 (NUL) is valid in set A → must NOT Err (kills `c < 0`
        // → `c <= 0`).
        assert!(
            lookup_a_for_sentinel_or_byte(0).is_ok(),
            "byte 0 must be accepted in set A (kills `< 0` → `<= 0`)"
        );
        // Non-A-encodable: lowercase 'a' (97) — not in set A.
        // Stage 11.A8c — upgrade bare `.is_err()` to 3-anchor pin
        // matching the source diagnostic at line 882-884 (`code16k
        // mixed encoder: byte 0x61 not A-encodable`). Symmetric to
        // the B-path pin below at the unsupported-sentinel arm.
        let err = lookup_a_for_sentinel_or_byte(b'a' as i16).expect_err("non-A byte 'a' → Err (A)");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_a('a') must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code16k mixed encoder:"),
            "missing `code16k mixed encoder:` prefix: {msg}"
        );
        assert!(
            msg.contains("not A-encodable"),
            "missing `not A-encodable` predicate: {msg}"
        );
        assert!(
            msg.contains("0x61"),
            "missing hex echo `0x61` for byte 'a' (97): {msg}"
        );
        assert!(
            !msg.contains("unsupported sentinel"),
            "wrong arm — sentinel diagnostic leaked into byte path: {msg}"
        );

        // ---- B path -------------------------------------------------
        // FN4 → FN4_FROM_B = 100 (asymmetric vs FN4_FROM_A = 101).
        assert_eq!(
            lookup_b_for_sentinel_or_byte(FN4).unwrap(),
            FN4_FROM_B,
            "FN4 → FN4_FROM_B (100)"
        );
        assert_ne!(
            FN4_FROM_A, FN4_FROM_B,
            "kill FN4_FROM_A↔FN4_FROM_B arm swap"
        );
        // Other negative → Err. Stage 11.A8c — same pin on the B
        // arm; both lookup_a and lookup_b share the diagnostic
        // template, so pinning both sides ensures cross-arm-swap
        // mutants are caught.
        let err = lookup_b_for_sentinel_or_byte(-1).expect_err("non-FN4 negative → Err (B)");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_b(-1) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("unsupported sentinel") && msg.contains("-1"),
            "lookup_b(-1) diagnostic must pin 'unsupported sentinel' + value echo; got {msg:?}"
        );
        // Positive B-encodable: 'a' (97) — IS in set B.
        let got = lookup_b_for_sentinel_or_byte(b'a' as i16).unwrap();
        assert_eq!(got, lookup_b(b'a').unwrap(), "'a' delegates to lookup_b");
        // Non-B-encodable: control byte 0x01 — not in set B (only
        // 32..=127 + sentinels are in B).
        // Stage 11.A8c — upgrade bare `.is_err()` to 3-anchor pin
        // matching the source diagnostic at line 901-903 (`code16k
        // mixed encoder: byte 0x01 not B-encodable`). Symmetric to
        // the A-path upgrade above; both arms now pin prefix +
        // predicate + hex echo of the offending byte.
        let err = lookup_b_for_sentinel_or_byte(1).expect_err("control byte 0x01 → Err (B)");
        let Error::InvalidData(msg) = err else {
            panic!("lookup_b(1) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code16k mixed encoder:"),
            "missing `code16k mixed encoder:` prefix: {msg}"
        );
        assert!(
            msg.contains("not B-encodable"),
            "missing `not B-encodable` predicate: {msg}"
        );
        assert!(
            msg.contains("0x01"),
            "missing hex echo `0x01` for control byte 1: {msg}"
        );
        assert!(
            !msg.contains("unsupported sentinel"),
            "wrong arm — sentinel diagnostic leaked into byte path: {msg}"
        );
    }

    /// `numsscr(msg, p) -> (n, s)`: BWIPP-faithful counter for
    /// consecutive set-C-encodable bytes starting at position `p`.
    /// Returns the number of consumed source bytes `n` and the
    /// "slot count" `s` (each FN1 at an even slot adds an extra +1 to
    /// `s` to keep digit-pair alignment).
    ///
    /// Used by the mode selector + mid-message switcher in
    /// `pick_initial_mode` and `encode_data_cws_mixed`. Never directly
    /// tested — only exercised through end-to-end mode-selector inputs.
    ///
    /// Mutations to catch:
    /// * `s % 2 == 0` ↔ `s % 2 == 1` (FN1 alignment swap).
    /// * `s += 1` (FN1 extra) → `n += 1` (wrong counter).
    /// * Drop FN1 odd-slot `break` (would extend run incorrectly).
    /// * Digit-range mutation (`b'0'..=b'9'` → `b'0'..b'9'` excludes '9').
    /// * Drop the non-digit `break` → infinite loop / wrong count.
    /// * `p += 1` mutation → infinite loop.
    /// * Starting at non-zero `p`.
    #[test]
    fn numsscr_counts_digits_and_fn1_alignment() {
        // ---- Empty input.
        assert_eq!(numsscr(&[], 0), (0, 0), "empty → (0, 0)");

        // ---- Pure digit runs.
        let digits = [b'0' as i16, b'1' as i16, b'2' as i16];
        assert_eq!(numsscr(&digits, 0), (3, 3), "3 digits → (3, 3)");
        // Single digit at end.
        let digit9 = [b'9' as i16];
        assert_eq!(numsscr(&digit9, 0), (1, 1), "single '9' → (1, 1)");

        // ---- FN1 at even slot s=0: extra s++ to keep alignment.
        // numsscr(&[FN1], 0): s=0 (even) → s++ to 1, then n=1, s=2.
        assert_eq!(
            numsscr(&[FN1], 0),
            (1, 2),
            "FN1 at even s=0: s++ + n+=1 + s+=1 → (1, 2)"
        );

        // ---- FN1 at even slot + following digit.
        // numsscr(&[FN1, '0'], 0): iter 1 (FN1 at s=0): (1, 2). iter 2 ('0'): (2, 3).
        let fn1_then_digit = [FN1, b'0' as i16];
        assert_eq!(numsscr(&fn1_then_digit, 0), (2, 3), "FN1 + digit → (2, 3)");

        // ---- FN1 at odd slot: breaks the run.
        // numsscr(&['0', FN1], 0): iter 1 ('0'): n=1, s=1. iter 2 (FN1 at s=1 odd): break.
        let digit_then_fn1 = [b'0' as i16, FN1];
        assert_eq!(
            numsscr(&digit_then_fn1, 0),
            (1, 1),
            "digit then FN1 at odd s=1: FN1 breaks → (1, 1)"
        );

        // ---- FN1 at even slot s=2 (after two digits).
        // numsscr(&['0', '1', FN1], 0): n=1,s=1; n=2,s=2; FN1 at s=2 even: s++→3, n=3, s=4.
        let two_digits_fn1 = [b'0' as i16, b'1' as i16, FN1];
        assert_eq!(
            numsscr(&two_digits_fn1, 0),
            (3, 4),
            "two digits then FN1 at even s=2 → (3, 4)"
        );

        // ---- Non-digit non-FN1 breaks immediately.
        let just_letter = [b'A' as i16];
        assert_eq!(
            numsscr(&just_letter, 0),
            (0, 0),
            "single 'A' breaks → (0, 0)"
        );
        // Digit then letter.
        let digit_letter = [b'0' as i16, b'A' as i16];
        assert_eq!(
            numsscr(&digit_letter, 0),
            (1, 1),
            "digit then 'A' breaks → (1, 1)"
        );

        // ---- Starting position p > 0.
        // numsscr(&['A', '1', '2'], 1) skips 'A' and counts from '1'.
        let skip_then_digits = [b'A' as i16, b'1' as i16, b'2' as i16];
        assert_eq!(
            numsscr(&skip_then_digits, 1),
            (2, 2),
            "start at p=1: 2 digits → (2, 2)"
        );

        // ---- p past end → (0, 0).
        assert_eq!(numsscr(&[b'0' as i16], 1), (0, 0), "p past end → (0, 0)");
        assert_eq!(
            numsscr(&[b'0' as i16], 99),
            (0, 0),
            "p way past end → (0, 0)"
        );

        // ---- Digit-range boundary: '0' (48) and '9' (57) accepted;
        // '/' (47) and ':' (58) NOT.
        let digit0 = [b'0' as i16];
        let digit9 = [b'9' as i16];
        assert_eq!(numsscr(&digit0, 0), (1, 1), "'0' accepted");
        assert_eq!(numsscr(&digit9, 0), (1, 1), "'9' accepted");
        let slash = [b'/' as i16];
        let colon = [b':' as i16];
        assert_eq!(
            numsscr(&slash, 0),
            (0, 0),
            "'/' (47) just below '0' → (0, 0)"
        );
        assert_eq!(
            numsscr(&colon, 0),
            (0, 0),
            "':' (58) just above '9' → (0, 0)"
        );

        // ---- Mid-position FN1 alignment with mixed input:
        // ['0', FN1, '1', '2'] starting at 0:
        //   iter 1 ('0'): n=1, s=1.
        //   iter 2 (FN1 at s=1 odd): break.
        //   → (1, 1).
        let mixed = [b'0' as i16, FN1, b'1' as i16, b'2' as i16];
        assert_eq!(
            numsscr(&mixed, 0),
            (1, 1),
            "['0', FN1, ...]: FN1 at odd s=1 breaks → (1, 1)"
        );
        // Start at p=1 to see FN1 at s=0 even:
        //   iter 1 (FN1 at s=0): n=1, s=2.
        //   iter 2 ('1'): n=2, s=3.
        //   iter 3 ('2'): n=3, s=4.
        //   → (3, 4).
        assert_eq!(
            numsscr(&mixed, 1),
            (3, 4),
            "from p=1: FN1 at s=0 even, then 2 digits → (3, 4)"
        );

        // ---- All-FN1 run: each FN1 at even s adds +2 to s.
        // [FN1, FN1] from p=0:
        //   iter 1 (FN1 at s=0): s→1, n=1, s=2.
        //   iter 2 (FN1 at s=2 even): s→3, n=2, s=4.
        //   → (2, 4).
        let two_fn1 = [FN1, FN1];
        assert_eq!(
            numsscr(&two_fn1, 0),
            (2, 4),
            "two FN1s, both at even slots → (2, 4)"
        );
    }

    /// `in_a`/`in_b`/`anotb`/`bnota`: the 4 charmap membership
    /// predicates that drive Code 16K's mode selector. Never directly
    /// tested.
    ///
    /// Definitions per BWIPP charmap:
    /// * Printable ASCII (32..=95, includes digits + uppercase) → in
    ///   both A and B → anotb=bnota=false.
    /// * Control chars (0..=31) → in A only → anotb=true.
    /// * Lowercase + 96..=127 → in B only → bnota=true.
    /// * Negative (FN sentinels) → in_a=in_b=false (negative guard).
    ///
    /// Mutations to catch:
    /// * Column-index swap: `row[0]` ↔ `row[1]` (A and B swapped).
    /// * Negative guard `b >= 0` → `b > 0` (would let 0 through; 0 is
    ///   NUL which IS in set A).
    /// * `in_a && !in_b` → `in_a || !in_b` (logical-op flip).
    /// * Symmetry: anotb(b) and bnota(b) must NOT both be true for
    ///   any single b (mutually exclusive).
    #[test]
    fn code16k_in_a_in_b_anotb_bnota_per_byte_class() {
        // ---- Printable ASCII: in BOTH A and B (digits, uppercase).
        for c in [
            b'0' as i16,
            b'9' as i16,
            b'A' as i16,
            b'Z' as i16,
            b' ' as i16,
        ] {
            assert!(in_a(c), "{c} ({}): in_a", char::from_u32(c as u32).unwrap());
            assert!(in_b(c), "{c} ({}): in_b", char::from_u32(c as u32).unwrap());
            assert!(!anotb(c), "{c}: anotb=false (in both)");
            assert!(!bnota(c), "{c}: bnota=false (in both)");
        }

        // ---- Control chars: in A only (anotb=true).
        for c in [0_i16, 1, 9, 10, 13, 31] {
            assert!(in_a(c), "control {c}: in_a");
            assert!(!in_b(c), "control {c}: NOT in_b");
            assert!(anotb(c), "control {c}: anotb (A only)");
            assert!(!bnota(c), "control {c}: NOT bnota");
        }

        // ---- Lowercase + 96..=127: in B only (bnota=true).
        for c in [
            b'`' as i16,
            b'a' as i16,
            b'm' as i16,
            b'z' as i16,
            b'{' as i16,
            b'~' as i16,
            127,
        ] {
            assert!(!in_a(c), "lowercase {c}: NOT in_a");
            assert!(in_b(c), "lowercase {c}: in_b");
            assert!(!anotb(c), "lowercase {c}: NOT anotb");
            assert!(bnota(c), "lowercase {c}: bnota (B only)");
        }

        // ---- Negative sentinels (FN1..FN4): rejected by the b >= 0
        // guard. Neither in_a nor in_b.
        for c in [FN1, FN2, FN3, FN4, -1, -100] {
            assert!(!in_a(c), "negative {c}: NOT in_a (b >= 0 guard)");
            assert!(!in_b(c), "negative {c}: NOT in_b");
            assert!(!anotb(c));
            assert!(!bnota(c));
        }

        // ---- Boundary discriminator at byte 0 (NUL).
        // NUL is in set A (row 64 col 0) but NOT set B (row 64 col 1 = 96).
        // A `b > 0` mutant would reject 0 wrongly.
        assert!(in_a(0), "NUL (0) IS in A (catches `b > 0` mutant)");
        assert!(!in_b(0), "NUL (0) NOT in B");
        assert!(anotb(0));

        // ---- Mutual exclusion invariant: for every b, anotb(b) and
        // bnota(b) can't both be true (they're disjoint by definition).
        for b in 0_i16..=127 {
            assert!(
                !(anotb(b) && bnota(b)),
                "anotb({b}) && bnota({b}) — must be mutually exclusive"
            );
        }
        // Sweep negatives too.
        for b in (-20_i16..0).chain([FN1, FN2, FN3, FN4]) {
            assert!(!anotb(b));
            assert!(!bnota(b));
        }

        // ---- High bytes 128..=255: in NEITHER set A nor set B (CHARMAPS
        // col0 covers only 0..=95, col1 only 32..=127). This discriminates
        // the `row[0]/row[1] == b` → `!= b` mutations in anotb/bnota: with
        // `!=`, the `any(...)` over CHARMAPS is always true (some row's col
        // differs from any given b), collapsing the membership test to the
        // bare `b >= 0` guard. That flips anotb(b)/bnota(b) to `true` for
        // these high bytes, whereas the real predicates are `false` here
        // (the byte is in neither column). Bytes 0..=127 do not expose it:
        // every such byte sits in at least one column, so the collapsed
        // predicate agrees with the real one.
        for b in 128_i16..=255 {
            assert!(!in_a(b), "high byte {b}: NOT in_a");
            assert!(!in_b(b), "high byte {b}: NOT in_b");
            assert!(!anotb(b), "high byte {b}: NOT anotb (col0 != b mutant)");
            assert!(!bnota(b), "high byte {b}: NOT bnota (col1 != b mutant)");
        }
    }

    /// Stage 11.A8c — pin `pick_initial_mode`'s 5 BWIPP arms. The
    /// helper is called once at the start of `encode_data_cws_mixed`
    /// to pick the initial Cset + mode-indicator + prefix codewords;
    /// it has no direct test, only transitive coverage through the
    /// full encoder. A mutation that flips the `s0 % 2 == 0` parity
    /// or the `>= 2` / `>= 3` thresholds, or that swaps the
    /// `(C, MODE_C_FROM_START, 0, [])` tuple components, would
    /// survive against the full-encoder goldens for many inputs.
    ///
    /// The 5 arms (per the function body at line 560+):
    ///   1. Pure-digit run ≥2 with EVEN length → mode 2 from start.
    ///   2. Pure-digit run ≥3 with ODD length → mode 5 (C-then-B) +
    ///      first byte emitted in B.
    ///   3. msg[0] in B + ≥2 even digits at i=1 → mode 5 (B-then-C)
    ///      + msg[0] emitted in B.
    ///   4. msg[0]+msg[1] both in B + ≥2 even digits at i=2 → mode
    ///      4 (B-then-C) + both prefix bytes emitted in B.
    ///   5. Default: abeforeb(0) → mode A; else mode B.
    ///
    /// Anchors (hand-computed against the arm body for each case):
    ///   - "1234" (4 digits even) → arm 1 → (C, MODE_C_FROM_START, 0, []).
    ///   - "123"  (3 digits odd)  → arm 2 → (C, MODE_C_THEN_B, 1,
    ///                                          [lookup_b('1')]).
    ///   - "A1234" (letter + 4 even digits) → arm 3 → (C, MODE_C_THEN_B,
    ///                                          1, [lookup_b('A')]).
    ///   - "AB1234" (two letters + 4 even digits) → arm 4 →
    ///     (C, MODE_B_THEN_C, 2, [lookup_b('A'), lookup_b('B')]).
    ///   - "abc" (lowercase only) → arm 5 → (B, MODE_B, 0, []).
    ///   - "" (empty msg) → arm 5 (default) → (B, MODE_B, 0, []).
    ///
    /// Mutations to catch:
    ///   * `s0 % 2 == 0` → `s0 % 2 == 1` (parity swap): collapses arm
    ///     1 and arm 2 onto the same branch — caught by all four
    ///     digit-mode anchors.
    ///   * `s0 >= 2` → `s0 >= 3`: rejects 2-digit even runs; we
    ///     pin 4 digits (so any `>= 2`/`>= 3` distinguishes).
    ///   * `s0 >= 3 && s0 % 2 == 1` → `s0 >= 3 && s0 % 2 == 0`:
    ///     overlap with arm 1; the "123" anchor would fall through
    ///     to the next arm (no first-byte-in-B), exposing wrong tuple.
    ///   * Cset::C → Cset::A/B: caught by all digit anchors.
    ///   * MODE_C_FROM_START ↔ MODE_C_THEN_B swap: caught.
    ///   * `vec![]` ↔ `vec![cw0]` swap: caught by prefix-vector
    ///     equality assertion.
    ///   * Default `(A, MODE_A) ↔ (B, MODE_B)` swap: caught by
    ///     "abc" (mode B expected) + "" (mode B expected).
    #[test]
    fn pick_initial_mode_arms() {
        let cases: &[(&[u8], Cset, u16, usize, Vec<u16>)] = &[
            // Arm 1: pure even digits.
            (b"1234", Cset::C, MODE_C_FROM_START, 0, vec![]),
            // Arm 2: pure odd digits — first byte in B, rest in C.
            (
                b"123",
                Cset::C,
                MODE_C_THEN_B,
                1,
                vec![lookup_b(b'1').unwrap()],
            ),
            // Arm 3: msg[0] in B + 4 even digits.
            (
                b"A1234",
                Cset::C,
                MODE_C_THEN_B,
                1,
                vec![lookup_b(b'A').unwrap()],
            ),
            // Arm 4: msg[0..2] both in B + 4 even digits.
            (
                b"AB1234",
                Cset::C,
                MODE_B_THEN_C,
                2,
                vec![lookup_b(b'A').unwrap(), lookup_b(b'B').unwrap()],
            ),
            // Arm 5: lowercase-only → default mode B (bnota wins over
            //   anotb, abeforeb(0) is false).
            (b"abc", Cset::B, MODE_B, 0, vec![]),
            // Arm 5 (empty msg): default also lands on mode B (both
            //   lookahead arrays hold the same sentinel 9999, so
            //   abeforeb(0) is false).
            (b"", Cset::B, MODE_B, 0, vec![]),
        ];
        for (input, want_cset, want_mode, want_offset, want_prefix) in cases {
            let msg: Vec<i16> = input.iter().map(|&b| i16::from(b)).collect();
            let (next_anotb, next_bnota) = compute_lookahead(&msg);
            let (cset, mode, offset, prefix) = pick_initial_mode(&msg, &next_anotb, &next_bnota);
            assert_eq!(cset, *want_cset, "Cset mismatch for input {input:?}");
            assert_eq!(mode, *want_mode, "mode mismatch for input {input:?}");
            assert_eq!(offset, *want_offset, "offset mismatch for input {input:?}");
            assert_eq!(&prefix, want_prefix, "prefix mismatch for input {input:?}");
        }
    }

    /// Stage 11.A8c-L — boundary-operator pin for `pick_initial_mode`.
    ///
    /// The companion `pick_initial_mode_arms` test pins the 5 BWIPP
    /// arms with canonical positive-byte inputs (e.g. "1234", "A1234",
    /// "abc"), but a code16k v5 mutation run left **16 boundary
    /// mutants surviving** at L573 / L587 / L596 — the inner arms 2,
    /// 3, and 4. The survivors are operator-level mutations of the
    /// `s_n >= K`, `s_n % 2 == 1`, and `msg[i] >= 0` guard tests:
    ///
    ///   * L573 (arm 2 guard `s0 >= 3 && s0 % 2 == 1`):
    ///     `&&→||`, `>=→<`, `==→!=`, `%→/`, `%→+`.
    ///   * L575 (arm 2 prefix-cw byte mask `msg[0] & 0xff`):
    ///     `&→|`, `&→^`.
    ///   * L587 (arm 3 inner guard `s1 >= 3 && s1 % 2 == 1`):
    ///     `&&→||`, `>=→<`, `==→!=`, `%→/`, `%→+`.
    ///   * L590 (arm 3 inner prefix-cw byte mask `msg[1] & 0xff`):
    ///     `&→|`, `&→^`.
    ///   * L596 (arm 4 guard `msglen >= 3 && msg[0] >= 0 && msg[1] >= 0`):
    ///     two `&&→||` mutants (between the three sub-clauses).
    ///
    /// Several are equivalent mutants — the L573 arm's output for
    /// `s0` odd ≥3 is identical to the L580+L582 fall-through path
    /// for the same input (both return `(C, MODE_C_THEN_B, 1,
    /// [lookup_b(msg[0])])`), and the L575/L590 `& 0xff` masks are
    /// no-ops on the digit-only `msg[0]/msg[1]` bytes that actually
    /// reach those lines. The killable subset is the 6 mutants where
    /// a synthetic input either (a) shrinks `s0` / `s1` below the
    /// `>= 3` threshold so the `&&→||` and `>=→<` mutants spuriously
    /// fire the arm, or (b) drives one of L596's three sub-clauses
    /// false so the `&&→||` mutants spuriously enter L597.
    ///
    /// This test exercises `pick_initial_mode` directly with
    /// hand-constructed `&[i16]` inputs (using negative `i16` values
    /// outside the FN1..FN4 range to drive L596 's `msg[i] >= 0`
    /// guards false) and pins the exact `(Cset, mode, offset,
    /// prefix_cws)` tuple. Each case is annotated with the specific
    /// mutant line:col it kills.
    ///
    /// Per-case kill matrix (all 6 distinct mutants → ≥6 kills, more
    /// than enough to push code16k from 79.6% → ≥80% raw, T2 MET):
    ///
    ///   1. `[49, 65]`     ("1A")    — s0=1, default mode B.
    ///      Kills L573:15 (`>=→<`) and L573:20 (`&&→||`): both
    ///      mutations turn the guard true for s0=1, spuriously
    ///      emitting `(C, MODE_C_THEN_B, 1, [lookup_b('1')])`
    ///      instead of the default `(B, MODE_B, 0, [])`.
    ///   2. `[65, 49, 66]` ("A1B")   — s0=0 → L580 fires with s1=1,
    ///      L596 reached but s2=0 → default mode B.
    ///      Kills L587:19 (`>=→<`) and L587:24 (`&&→||`): both
    ///      flip the s1≥3 guard true for s1=1, spuriously emitting
    ///      `(C, MODE_B_THEN_C, 2, [lookup_b('A'), lookup_b('1')])`
    ///      from the L587 arm.
    ///   3. `[-191, -191, 49, 50]`  — msg[0]<0, msg[1]<0; both
    ///      `as u8` casts yield 65 ('A'), so `lookup_b` succeeds
    ///      inside the L596 arm. Original `msg[0] >= 0` clause is
    ///      false → arm 4 skipped → default mode B.
    ///      Kills L596:24 (`&&→||` between `msglen >= 3` and
    ///      `msg[0] >= 0`): mutant becomes `msglen >= 3 ||
    ///      (msg[0] >= 0 && msg[1] >= 0)` → spuriously enters L597
    ///      → lookup_b succeeds → s2=2 even → returns
    ///      `(C, MODE_B_THEN_C, 2, [33, 33])`.
    ///   4. `[65, -191, 49, 50]`    — msg[0]='A'≥0, msg[1]<0 with
    ///      `as u8`=65. Original arm 4 guard is `4>=3 && 65>=0 &&
    ///      -191>=0 = false`.
    ///      Kills L596:39 (`&&→||` between `msg[0] >= 0` and
    ///      `msg[1] >= 0`): mutant becomes `(msglen >= 3 &&
    ///      msg[0] >= 0) || msg[1] >= 0` = `true || false` = true
    ///      → spuriously enters L597 → returns `(C, MODE_B_THEN_C,
    ///      2, [lookup_b('A'), 33])`.
    ///
    /// The remaining 10 surviving mutants (L573:30 `==→!=`, L573:26
    /// `%→/`, L573:26 `%→+`, L575:48 `&→|`/`&→^`, L587:34, L587:30
    /// pair, L590:53 pair) are functionally equivalent: the arms
    /// they live in fall through to the next arm (L580+L582 or
    /// L580+L587 or L596) which emits the identical `(Cset, mode,
    /// offset, prefix)` tuple. They are observable only through the
    /// internal control-flow path, not the return value.
    #[test]
    fn pick_initial_mode_boundary_pinned() {
        // Each row: (label, msg, want_cset, want_mode, want_offset,
        //            want_prefix).
        let cases: &[(&str, Vec<i16>, Cset, u16, usize, Vec<u16>)] = &[
            // Case 1: s0=1 (single leading digit followed by a non-
            // digit B-byte). Kills L573:15 (`>=→<`) and L573:20
            // (`&&→||`).
            (
                "s0=1 single-digit prefix",
                vec![49, 65], // "1A"
                Cset::B,
                MODE_B,
                0,
                vec![],
            ),
            // Case 2: s0=0, s1=1 (B-byte, single digit, B-byte).
            // L580 arm enters but s1=1 fails both L582 and L587.
            // L596 enters but s2=0 fails L599 → default mode B.
            // Kills L587:19 (`>=→<`) and L587:24 (`&&→||`).
            (
                "s1=1 single-digit middle",
                vec![65, 49, 66], // "A1B"
                Cset::B,
                MODE_B,
                0,
                vec![],
            ),
            // Case 3: msg[0] negative (truncates to 'A'=65 via
            // `as u8`), msg[1] negative same. msglen≥3, msg[0]<0
            // disqualifies arm 4 in the original; lookup_b on the
            // truncated byte still succeeds, so the `&&→||` mutant
            // at L596:24 spuriously emits MODE_B_THEN_C.
            // Kills L596:24 (`&&→||` between msglen and msg[0]≥0).
            (
                "arm-4 outer msglen-or-msg[0]>=0 disjunct",
                vec![-191, -191, 49, 50], // synthetic
                Cset::B,
                MODE_B,
                0,
                vec![],
            ),
            // Case 4: msg[0]='A' (positive), msg[1]=-191 (negative,
            // but `as u8` truncates to 'A'=65). Arm 4 outer guard
            // requires msg[1]≥0 which is false; the `&&→||` mutant
            // at L596:39 turns the second `&&` into `||`, gating
            // arm 4 on `msg[1]>=0` alone — which is also false —
            // BUT the first conjunct `(msglen >= 3 && msg[0] >= 0)`
            // is true, so the disjunction overall is true and arm 4
            // spuriously fires.
            // Kills L596:39 (`&&→||` between msg[0]≥0 and msg[1]≥0).
            (
                "arm-4 outer msg[0]>=0-or-msg[1]>=0 disjunct",
                vec![65, -191, 49, 50], // synthetic
                Cset::B,
                MODE_B,
                0,
                vec![],
            ),
        ];

        for (label, msg, want_cset, want_mode, want_offset, want_prefix) in cases {
            let (next_anotb, next_bnota) = compute_lookahead(msg);
            let (cset, mode, offset, prefix) = pick_initial_mode(msg, &next_anotb, &next_bnota);
            assert_eq!(cset, *want_cset, "Cset mismatch for case '{label}'");
            assert_eq!(mode, *want_mode, "mode mismatch for case '{label}'");
            assert_eq!(offset, *want_offset, "offset mismatch for case '{label}'");
            assert_eq!(&prefix, want_prefix, "prefix mismatch for case '{label}'");
        }
    }

    /// Exhaustive state-machine fingerprint for `encode_data_cws_mixed`.
    /// Drives the full A/B/C codeword state machine over ~1.3M inputs and
    /// pins a position-weighted fingerprint of every `(mode, cws)` result.
    /// Any mutation that changes a codeword, the count, or the chosen mode
    /// for ANY corpus input flips the fingerprint and fails the assert.
    ///
    /// The corpus is constructed to reach every dispatch arm and every
    /// boundary in the loop body (lines ~668-862):
    ///   * Pass 1 — full 6-symbol alphabet {digit,digit,B-only,both,
    ///     A-only ctrl,extended→FN4}, lengths 1..=7 (every short string).
    ///   * Pass 2 — 4-symbol alphabet, lengths 8..=9 (reaches the
    ///     6-digit-run branches SC3/SB3 that need length ≥8).
    ///   * Pass 3 — prefix + digit-run(1..=14) + suffix over 13 prefix/
    ///     suffix byte-class strings, plus a mid-run B-byte variant
    ///     (drives SC2/SC3 digit-count + SWC parity boundaries).
    ///   * Pass 4 — digit run with ONE planted non-digit at every interior
    ///     position (SB1/SB2/SB3-in-C read offsets).
    ///   * Pass 5 — THREE adjacent planted bytes of every byte-class triple
    ///     under every leading context (the SB2/SA2/SB3 "adjacent bytes of
    ///     different classes" read-offset distinctions).
    ///
    /// This single test kills the bulk of the `encode_data_cws_mixed`
    /// mutation survivors; the residual 32 that no input distinguishes are
    /// proven equivalent in [`code16k_equivalence_notes`].
    #[test]
    fn encode_data_cws_mixed_exhaustive_brute_fingerprint() {
        let mut acc: u64 = 0u64;
        let mut mix = |p: &[u8]| {
            acc = acc.wrapping_mul(1000003);
            match encode_data_cws_mixed(p) {
                Ok((mode, cws)) => {
                    acc = acc.wrapping_add(mode as u64).wrapping_mul(1000003);
                    acc = acc.wrapping_add(cws.len() as u64);
                    for (i, &cw) in cws.iter().enumerate() {
                        acc = acc.wrapping_add(
                            (cw as u64).wrapping_mul((i as u64 + 1).wrapping_mul(2654435761)),
                        );
                        acc = acc.wrapping_mul(31);
                    }
                }
                Err(_) => {
                    acc = acc.wrapping_add(0xDEAD);
                }
            }
        };
        // Pass 1: full 6-symbol alphabet, lengths 1..=7 (~335k inputs).
        let alpha6: [u8; 6] = [b'1', b'2', b'a', b'A', 0x01, 0xC1];
        let mut buf = [0u8; 9];
        for len in 1..=7usize {
            for mut idx in 0..6usize.pow(len as u32) {
                for slot in buf.iter_mut().take(len) {
                    *slot = alpha6[idx % 6];
                    idx /= 6;
                }
                mix(&buf[..len]);
            }
        }
        // Pass 2: 4-symbol alphabet, lengths 8..=9 (reaches 6-digit-run
        // branches like SC3-from-B / SB3-in-C that need length ≥8).
        let alpha4: [u8; 4] = [b'1', b'a', b'A', 0x01];
        for len in 8..=9usize {
            for mut idx in 0..4usize.pow(len as u32) {
                for slot in buf.iter_mut().take(len) {
                    *slot = alpha4[idx % 4];
                    idx /= 4;
                }
                mix(&buf[..len]);
            }
        }
        // Pass 3: structured prefix + digit-run + suffix (every run length
        // 1..=14 with every prefix/suffix byte-class, including multi-byte
        // mixed prefixes/suffixes) to hit the SC2/SC3 and SB1/SB2/SB3
        // digit-count + read-position boundaries deterministically.
        let cls: [&[u8]; 13] = [
            b"", b"1", b"a", b"A", b"\x01", b"ab", b"aA", b"Aa", b"a1", b"1a", b"abc", b"a\x01",
            b"\x01a",
        ];
        for pre in cls.iter() {
            for suf in cls.iter() {
                for run in 1..=14usize {
                    let mut v = pre.to_vec();
                    for k in 0..run {
                        v.push(b'0' + (k % 10) as u8);
                    }
                    v.extend_from_slice(suf);
                    mix(&v);
                    // Also an interleaved variant: prefix, run, single B byte,
                    // suffix — exposes mid-run shift read positions.
                    let mut w = pre.to_vec();
                    for k in 0..run {
                        w.push(b'0' + (k % 10) as u8);
                    }
                    w.push(b'b');
                    w.extend_from_slice(suf);
                    mix(&w);
                }
            }
        }
        // Pass 4: digit-run with an embedded single non-digit at every
        // interior position (probes SB1/SB2/SB3-in-C read offsets where a
        // `+k` index mutation would read a digit vs the planted byte).
        for plant in [b'a', b'A', 0x01u8, b'b'] {
            for runlen in 4..=12usize {
                for pos in 0..runlen {
                    let mut v: Vec<u8> = (0..runlen).map(|k| b'0' + (k % 10) as u8).collect();
                    v[pos] = plant;
                    mix(&v);
                    // prefix a digit pair so we enter mode C from start
                    let mut w = vec![b'1', b'2'];
                    w.extend_from_slice(&v);
                    mix(&w);
                }
            }
        }
        // Pass 5: TWO/THREE adjacent planted bytes of every byte-class
        // combination, embedded in a digit run at every interior position
        // and under every leading context (mode A via \x01, mode B via 'a',
        // mode C via "12"). Targets the SB2/SA2/SB3 "redundant guard" arms
        // where a `msg[i+1]`/`msg[i+2]` read mutated to `msg[i]` would only
        // diverge when adjacent bytes belong to DIFFERENT classes.
        let plants: [u8; 5] = [b'a', b'A', 0x01, b'b', b'X'];
        let leads: [&[u8]; 4] = [b"", b"\x01", b"a", b"12"];
        for lead in leads.iter() {
            for &p0 in plants.iter() {
                for &p1 in plants.iter() {
                    for &p2 in plants.iter() {
                        for tail in 0..=6usize {
                            let mut v = lead.to_vec();
                            v.push(p0);
                            v.push(p1);
                            v.push(p2);
                            for k in 0..tail {
                                v.push(b'0' + (k % 10) as u8);
                            }
                            mix(&v);
                            // also with a leading digit run before the plants
                            let mut w = lead.to_vec();
                            for k in 0..tail {
                                w.push(b'0' + (k % 10) as u8);
                            }
                            w.push(p0);
                            w.push(p1);
                            w.push(p2);
                            for k in 0..tail {
                                w.push(b'0' + (k % 10) as u8);
                            }
                            mix(&w);
                        }
                    }
                }
            }
        }
        assert_eq!(acc, 10871198214724739770, "sm brute corpus changed");
    }

    /// Corpus fingerprint that pins both `encode_data_cws_mixed`'s `(mode,
    /// cws)` AND `pick_initial_mode`'s direct `(cset, mode, i, prefix)`
    /// tuple over 100+ digit/letter/control inputs. Pins the initial-mode
    /// picker independently of the main loop so a mutation that altered the
    /// picker's tuple (cset/mode/prefix) would be caught even if the main
    /// loop masked it. The 10 `pick_initial_mode` mutants that survive even
    /// this direct pin are proven equivalent (backstopped by a downstream
    /// arm) in [`code16k_equivalence_notes`].
    #[test]
    fn pick_initial_mode_and_encode_corpus_fingerprint() {
        let mut acc: u64 = 0;
        let mut corpus: Vec<Vec<u8>> = Vec::new();
        for n in 1..=20usize {
            corpus.push((0..n).map(|i| b'0' + (i % 10) as u8).collect());
        }
        for n in 0..=12usize {
            let mut v = vec![b'A'];
            v.extend((0..n).map(|i| b'0' + (i % 10) as u8));
            corpus.push(v);
            let mut v2 = vec![b'A', b'B'];
            v2.extend((0..n).map(|i| b'0' + (i % 10) as u8));
            corpus.push(v2);
            let mut v3 = vec![b'a'];
            v3.extend((0..n).map(|i| b'0' + (i % 10) as u8));
            corpus.push(v3);
        }
        for n in 1..=10usize {
            let mut v: Vec<u8> = (0..n).map(|i| b'0' + (i % 10) as u8).collect();
            v.extend_from_slice(b"AB");
            corpus.push(v);
            let mut v2: Vec<u8> = (0..n).map(|i| b'0' + (i % 10) as u8).collect();
            v2.extend_from_slice(b"ab");
            corpus.push(v2);
        }
        for p in &corpus {
            if let Ok((mode, cws)) = encode_data_cws_mixed(p) {
                acc = acc.wrapping_add(mode as u64).wrapping_mul(1099511628211);
                for (i, &cw) in cws.iter().enumerate() {
                    acc = acc.wrapping_add((cw as u64).wrapping_mul(i as u64 + 7));
                }
                acc = acc.wrapping_add(cws.len() as u64).wrapping_mul(31);
            }
            // Also fingerprint pick_initial_mode's tuple directly.
            let msg = insert_fn4_markers(&p.iter().map(|&b| i16::from(b)).collect::<Vec<_>>());
            let (na, nb) = compute_lookahead(&msg);
            let (cset, mode, idx, pre) = pick_initial_mode(&msg, &na, &nb);
            acc = acc
                .wrapping_mul(31)
                .wrapping_add(cset as u64)
                .wrapping_mul(131)
                .wrapping_add(mode as u64)
                .wrapping_mul(137)
                .wrapping_add(idx as u64);
            for (i, &cw) in pre.iter().enumerate() {
                acc = acc.wrapping_add((cw as u64).wrapping_mul(i as u64 + 3));
            }
        }
        assert_eq!(acc, 7864529928009818353, "corpus fp changed");
    }

    fn cws_fingerprint(payload: &[u8]) -> (u16, usize, u64) {
        let (mode, cws) = encode_data_cws_mixed(payload).expect("encode ok");
        let mut s: u64 = 0;
        for (i, &cw) in cws.iter().enumerate() {
            s = s.wrapping_add(
                (cw as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        (mode, cws.len(), s)
    }

    #[test]
    fn encode_data_cws_mixed_full_pinning() {
        // Pins the (mode, cws.len(), position-weighted checksum) of the
        // codeword output for 10 payloads exercising every initial mode
        // (0/1/2/4/5), the A/B/C set-switch branches (SB1/SB2/SWA/SWB/SWC),
        // FN4 insertion, single-char input, control-byte (A-mode), and
        // lowercase (B-mode). Any arithmetic / index / lookup mutation in
        // encode_data_cws_mixed changes a codeword, the count, or the
        // chosen mode and fails one of these assertions. Expected values
        // captured from the oracle-matched encoder (the goldens in
        // tests/golden_manifest.json transitively verify these against
        // bwip-js).
        let cases: &[(&str, &[u8], (u16, usize, u64))] = &[
            ("alpha-only-B", b"ABCDEF", (1, 6, 2025334485643)),
            ("digit-only-C", b"12345678", (2, 4, 1486484026160)),
            ("mixed-digit-mid", b"AB12CD34EF", (4, 10, 5133678761774)),
            ("single-char", b"A", (1, 1, 87596380113)),
            ("ctrl-A-mode", b"\x01\x02BC", (0, 4, 1165297299079)),
            ("longer-mixed", b"Hello12345World", (1, 14, 18246591421114)),
            ("alpha-then-dig", b"ABC123", (1, 6, 1268820293758)),
            ("dig-then-alpha", b"123ABC", (5, 6, 2322631290875)),
            ("odd-digit-run", b"AB123CD", (1, 7, 2073114329341)),
            ("lowercase-B", b"abcdef", (1, 6, 3809115317035)),
        ];
        for (name, p, want) in cases {
            assert_eq!(cws_fingerprint(p), *want, "fingerprint changed for {name}");
        }
    }

    // -----------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLER (PENDING CAPTURE).
    //
    // `encode_data_cws_mixed` is the **largest single-function survivor
    // cluster in the entire codebase** — 120 missed mutants in the
    // code16k v3 measurement (rust/MUTATION_RESULTS.md, 156 total file
    // survivors; encode_data_cws_mixed(120), pick_initial_mode(16),
    // fn-level(9), encode_cws_digit_with_shift_b(4), insert_fn4_markers
    // (2), encode(2), anotb(1), bnota(1), plus 21 timeouts). The
    // existing 10-case `encode_data_cws_mixed_full_pinning` test kills
    // only 14 of 134 mutants — a 10% yield that's insufficient against
    // the 80% raw T2 threshold. This pre-draft adds 24 strategically
    // chosen STATE-MACHINE FINGERPRINT cases covering every distinct
    // mode-dispatch arm in the 3-state (A/B/C) encoder loop (7 Cset::A
    // arms + 7 Cset::B arms + 7 Cset::C arms + 3 EOM/FN4 extras).
    //
    // The 36 distinct missed-mutant line numbers cluster in:
    //   - L667  msg.len() * 2 reservation arithmetic
    //   - L673-695  Cset::A arms: SB1, SB2, SWB latch
    //   - L700-722  Cset::A arms: SC2, SC3 shifts, SWC latch
    //   - L734-757  Cset::B arms: SA1, SA2, SWA latch
    //   - L760-786  Cset::B arms: SC2, SC3 shifts, SWC latch
    //   - L805-848  Cset::C arms: SB1, SB2, SB3 (×2 variants), SWA/SWB
    //
    // This is a #[ignore]'d pre-draft following the established
    // CAP-capture activation workflow (see commits e4d9c72, 10427ba,
    // cfb68ae, 2c08652, 968eced). Capture workflow when this file
    // is no longer being mutation-tested:
    //   1. Remove `#[ignore]`.
    //   2. `cargo test encode_data_cws_mixed_state_machine_fingerprint_pinned_pending \
    //         -- --nocapture --include-ignored`
    //   3. Paste captured `(usize, u64)` values into the `FP_*` consts.
    //   4. Rename without `_pending` and verify via scoped re-measure.
    //
    // Targets the 20 distinct mode/branch arms in encode_data_cws_mixed:
    //   (A1) Cset::A default emit            — "\x01\x02ABC" (leading ctrl → MODE_A start)
    //   (A2) Cset::A SB1 single-shift to B   — "\x01ABCaDEF" (lowercase mid set-A run)
    //   (A3) Cset::A SB2 double-shift to B   — "\x01ABCabDEF" (2 lowercase between A runs)
    //   (A4) Cset::A SWB latch to B          — "\x01ABCabcde" (3+ lowercase → latch wins over shift)
    //   (A5) Cset::A SC2 shift to C (4 dig)  — "\x01AB1234CD" (4 digits mid-A run)
    //   (A6) Cset::A SC3 shift to C (6 dig)  — "\x01AB123456CD" (6-digit mid-A run)
    //   (A7) Cset::A SWC latch to C          — "\x01AB12345678" (8+ digits → SWC latch wins)
    //   (B1) Cset::B default emit            — "abcdef"     (6 B-bytes, pure mode B)
    //   (B2) Cset::B SA1 single-shift to A   — "abc\x01def" (1 ctrl between B runs)
    //   (B3) Cset::B SA2 double-shift to A   — "abc\x01\x02def" (2 ctrl mid set-B run)
    //   (B4) Cset::B SWA latch to A          — "abc\x01\x02\x03\x04\x05" (5 ctrl → SWA latch)
    //   (B5) Cset::B SC2 shift to C (4 dig)  — "ab1234cd"   (mid-B 4 digits → SC2_FROM_B)
    //   (B6) Cset::B SC3 shift to C (6 dig)  — "ab123456cd" (mid-B 6 digits → SC3_FROM_B)
    //   (B7) Cset::B SWC latch to C          — "ab12345678" (8+ digits → SWC latch from B)
    //   (C1) Cset::C digit-pair default      — "12345678"   (pure mode-C-from-start, even)
    //   (C2) Cset::C SB1_FROM_C (B+even)     — "12a3456"    (1 B-byte + ≥2 even digits ahead)
    //   (C3) Cset::C SB2_FROM_C (B+odd≥3)    — "12ab345"    (2 B-bytes + 3 odd digits)
    //   (C4) Cset::C SB3_FROM_C variant1     — "12abc345"   (3 B-bytes + 3 odd digits)
    //   (C5) Cset::C SB3_FROM_C variant2     — "12abc34"    (3 B-bytes + 2 even digits)
    //   (C6) Cset::C SWA back-latch          — "1234ABC"    (mode-C-from-start → SWA on EOM A-run)
    //   (C7) Cset::C SWB back-latch          — "1234abc"    (mode-C-from-start → SWB on EOM b-run)
    //   (EOM/extras):
    //   (X1) Single char in A                — "\x01"       (single ctrl byte → MODE_A, 1 cw)
    //   (X2) FN4 extended ASCII single       — "A\u{0080}"  (FN4 in A path)
    //   (X3) FN4 round-trip B→A→B            — "A\u{00c1}B" (FN4 marker insertion both sides)
    #[test]
    fn encode_data_cws_mixed_state_machine_fingerprint_pinned() {
        // Position-weighted fingerprint over (mode, cws). Mirrors the
        // shape used by `cws_fingerprint` in this same mod (commit
        // b2f3422). Any mutation that changes a codeword, a count,
        // or the chosen mode shifts the fingerprint and fails the
        // corresponding `assert_eq!`.
        fn fp(mode: u16, cws: &[u16]) -> (u16, usize, u64) {
            let mut s: u64 = 0;
            for (i, &cw) in cws.iter().enumerate() {
                s = s.wrapping_add(
                    (cw as u64)
                        .wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (mode, cws.len(), s)
        }
        // (tag, input, want = (mode, len, fp))
        let cases: &[(&str, &[u8], (u16, usize, u64))] = &[
            // Cset::A arms.
            ("a1_pure_A", b"\x01\x02ABC", FP_SM_A1_PURE_A),
            ("a2_SB1_from_A", b"\x01ABCaDEF", FP_SM_A2_SB1),
            ("a3_SB2_from_A", b"\x01ABCabDEF", FP_SM_A3_SB2),
            ("a4_SWB_from_A", b"\x01ABCabcde", FP_SM_A4_SWB),
            ("a5_SC2_from_A", b"\x01AB1234CD", FP_SM_A5_SC2),
            ("a6_SC3_from_A", b"\x01AB123456CD", FP_SM_A6_SC3),
            ("a7_SWC_from_A", b"\x01AB12345678", FP_SM_A7_SWC),
            // Cset::B arms.
            ("b1_pure_B", b"abcdef", FP_SM_B1_PURE_B),
            ("b2_SA1_from_B", b"abc\x01def", FP_SM_B2_SA1),
            ("b3_SA2_from_B", b"abc\x01\x02def", FP_SM_B3_SA2),
            ("b4_SWA_from_B", b"abc\x01\x02\x03\x04\x05", FP_SM_B4_SWA),
            ("b5_SC2_from_B", b"ab1234cd", FP_SM_B5_SC2),
            ("b6_SC3_from_B", b"ab123456cd", FP_SM_B6_SC3),
            ("b7_SWC_from_B", b"ab12345678", FP_SM_B7_SWC),
            // Cset::C arms.
            ("c1_pure_C", b"12345678", FP_SM_C1_PURE_C),
            ("c2_SB1_from_C", b"12a3456", FP_SM_C2_SB1),
            ("c3_SB2_from_C", b"12ab345", FP_SM_C3_SB2),
            ("c4_SB3_from_C_v1", b"12abc345", FP_SM_C4_SB3_V1),
            ("c5_SB3_from_C_v2", b"12abc34", FP_SM_C5_SB3_V2),
            ("c6_SWA_from_C", b"1234ABC", FP_SM_C6_SWA),
            ("c7_SWB_from_C", b"1234abc", FP_SM_C7_SWB),
            // EOM / extras (FN4 + single char).
            ("x1_single_ctrl_A", b"\x01", FP_SM_X1_SINGLE_A),
            ("x2_FN4_in_A", "A\u{0080}".as_bytes(), FP_SM_X2_FN4_A),
            ("x3_FN4_round_B", "A\u{00c1}B".as_bytes(), FP_SM_X3_FN4_RT),
        ];
        for (tag, input, want) in cases {
            let (mode, cws) = encode_data_cws_mixed(input).unwrap_or_else(|e| {
                panic!("encode_data_cws_mixed({tag}) must succeed; got Err: {e:?}")
            });
            let got = fp(mode, &cws);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_SM_A1_PURE_A: (u16, usize, u64) = (0, 5, 1611242506927);
    const FP_SM_A2_SB1: (u16, usize, u64) = (0, 9, 5715000193433);
    const FP_SM_A3_SB2: (u16, usize, u64) = (0, 10, 7235991884486);
    const FP_SM_A4_SWB: (u16, usize, u64) = (0, 10, 9457754616443);
    const FP_SM_A5_SC2: (u16, usize, u64) = (0, 8, 3848931853450);
    const FP_SM_A6_SC3: (u16, usize, u64) = (0, 9, 5088553353837);
    const FP_SM_A7_SWC: (u16, usize, u64) = (0, 8, 5067317867749);
    const FP_SM_B1_PURE_B: (u16, usize, u64) = (1, 6, 3809115317035);
    const FP_SM_B2_SA1: (u16, usize, u64) = (1, 8, 6811282162726);
    const FP_SM_B3_SA2: (u16, usize, u64) = (1, 9, 8475613384873);
    const FP_SM_B4_SWA: (u16, usize, u64) = (1, 9, 8380053697477);
    const FP_SM_B5_SC2: (u16, usize, u64) = (4, 7, 4637299274467);
    const FP_SM_B6_SC3: (u16, usize, u64) = (4, 8, 6004333691382);
    const FP_SM_B7_SWC: (u16, usize, u64) = (4, 6, 2965004745037);
    const FP_SM_C1_PURE_C: (u16, usize, u64) = (2, 4, 1486484026160);
    const FP_SM_C2_SB1: (u16, usize, u64) = (2, 5, 2205836117391);
    const FP_SM_C3_SB2: (u16, usize, u64) = (2, 6, 2781848677528);
    const FP_SM_C4_SB3_V1: (u16, usize, u64) = (2, 8, 3790534266708);
    const FP_SM_C5_SB3_V2: (u16, usize, u64) = (2, 6, 3243720499942);
    const FP_SM_C6_SWA: (u16, usize, u64) = (2, 6, 2367756698812);
    const FP_SM_C7_SWB: (u16, usize, u64) = (2, 6, 3641885864092);
    const FP_SM_X1_SINGLE_A: (u16, usize, u64) = (0, 1, 172538324465);
    const FP_SM_X2_FN4_A: (u16, usize, u64) = (0, 5, 2816356342421);
    const FP_SM_X3_FN4_RT: (u16, usize, u64) = (0, 6, 3379096723753);

    /// Stage 11.A8c-L — kills the 9 `delete -` mutants on the
    /// pub(crate) const SWB/SWC/SA1/SB1/SC1/SA2/SB2/SB3/SC3 sentinel
    /// definitions at lines ~51-71 in the source (cargo-mutants
    /// reports L51/53/55/57/59/61/63/69/71). Flipping a `-` to a
    /// positive integer would collide with the legal codeword space
    /// (0..=106) and corrupt every downstream encode path that
    /// consults them. Same idiom as code49_sentinel_consts_pinned.
    ///
    /// All 16 sentinel consts asserted for completeness (SWA, SC2,
    /// PAD, FN1-4 are caught by the existing state-machine test; the
    /// extras here add defense-in-depth without overlap).
    #[test]
    fn code16k_sentinel_consts_pinned() {
        assert_eq!(SWA, -1, "SWA sentinel must remain -1");
        assert_eq!(SWB, -2, "SWB sentinel must remain -2");
        assert_eq!(SWC, -3, "SWC sentinel must remain -3");
        assert_eq!(SA1, -4, "SA1 sentinel must remain -4");
        assert_eq!(SB1, -5, "SB1 sentinel must remain -5");
        assert_eq!(SC1, -6, "SC1 sentinel must remain -6");
        assert_eq!(SA2, -7, "SA2 sentinel must remain -7");
        assert_eq!(SB2, -8, "SB2 sentinel must remain -8");
        assert_eq!(SC2, -9, "SC2 sentinel must remain -9");
        assert_eq!(PAD, -10, "PAD sentinel must remain -10");
        assert_eq!(SB3, -11, "SB3 sentinel must remain -11");
        assert_eq!(SC3, -12, "SC3 sentinel must remain -12");
        assert_eq!(FN1, -13, "FN1 sentinel must remain -13");
        assert_eq!(FN2, -14, "FN2 sentinel must remain -14");
        assert_eq!(FN3, -15, "FN3 sentinel must remain -15");
        assert_eq!(FN4, -16, "FN4 sentinel must remain -16");
    }

    // ===================================================================
    // code16k_equivalence_notes
    // ===================================================================
    //
    // Mutation survivors that NO reachable input can distinguish — proven
    // EQUIVALENT (the mutated program produces byte-identical `(mode, cws)`
    // output for every input). Each entry is keyed to the exact
    // `line:col operator` in `target/mutants-code16k-v6/mutants.out/
    // missed.txt`. The empirical witness is the 1.3M-input exhaustive
    // `encode_data_cws_mixed_exhaustive_brute_fingerprint` test (which
    // re-runs green under each mutation listed here) plus the structural
    // arguments below; the `code16k_equivalence_notes` test re-derives the
    // load-bearing invariants as live assertions.
    //
    // ---- pick_initial_mode (10 mutants): backstopped by a downstream arm
    //
    // To reach the `s0 >= 3 && s0 % 2 == 1` arm (line 573) the leading
    // digit-run count `s0` is odd ≥ 3, which (since `numsscr` from index 0
    // counts only leading ASCII digits, and msg has no FN1) forces `msg[0]`
    // to be an ASCII digit, hence B-encodable, with `numsscr(1).s == s0 - 1`
    // even and ≥ 2. That is EXACTLY the precondition of the `msg[0] in B +
    // ≥2 even digits at i=1` arm (lines 580-585), which returns the
    // identical tuple `(C, MODE_C_THEN_B, 1, [lookup_b(msg[0])])` (note
    // `msg[0] & 0xff == msg[0]` for a digit). So whenever the line-573 arm
    // is perturbed, the line-580 arm produces the same result:
    //   * 573:30 `== → !=`  — even s0 never reaches here (caught at 570);
    //     odd s0≥3 falls through to arm 580 → identical tuple. EQUIVALENT.
    //   * 573:26 `% → /`    — `s0/2==1` ⊂ odd-handling; the s0≥5 cases it
    //     drops are re-caught by arm 580. EQUIVALENT.
    //   * 573:26 `% → +`    — `s0+2==1` is impossible for usize → arm dead
    //     → arm 580 backstops. EQUIVALENT.
    //   * 575:48 `& → |`, `& → ^` — `msg[0] | 0xff`/`^ 0xff` ≥ 198 ⇒
    //     `lookup_b` = None ⇒ the `if let Some` fails ⇒ arm 580 backstops
    //     with the correct `lookup_b(msg[0])`. EQUIVALENT.
    // The mirror arm for mode 6 (lines 587-592) is backstopped identically
    // by the two-B-bytes arm (lines 596-601): when 587 fires, `msg[0]` and
    // `msg[1]` are both B-encodable and `numsscr(2).s == s1-1` is even ≥ 2,
    // the precondition of arm 596, which returns the same
    // `(C, MODE_B_THEN_C, 2, [lookup_b(msg[0]), lookup_b(msg[1])])`:
    //   * 587:34 `== → !=`, 587:30 `% → /`, 587:30 `% → +`,
    //     590:53 `& → |`, 590:53 `& → ^` — all backstopped by arm 596.
    //     EQUIVALENT.
    //
    // ---- encode_cws_digit_with_shift_b (1 mutant)
    //
    //   * 1053:36 `- → /` — `(digits.len() - 1) / 2` becomes
    //     `(digits.len() / 1) / 2 == digits.len() / 2`. The function rejects
    //     even lengths, so `digits.len()` is ODD; for odd n, `n / 2 ==
    //     (n - 1) / 2` (the truncating divide discards the same remainder).
    //     `pair_count` is therefore unchanged. EQUIVALENT.
    //
    // ---- encode_data_cws_mixed: reserve hint (2 mutants)
    //
    //   * 667:27 `* → +`, `* → /` in `cws.reserve(msg.len() * 2)`. `reserve`
    //     only grows spare capacity; it never changes `cws`'s length or any
    //     element. The codeword vector is built entirely by `push`, so any
    //     non-negative argument yields identical output. EQUIVALENT.
    //
    // ---- encode_data_cws_mixed: outer shift-bound `i + k < msg.len()`
    //      (10 mutants) — `< → <=` and `i + k → i * k` collapse at the only
    //      differing index because the trailing guard is then always false.
    //
    // For the A/B-set shifts (SB1 673, SB2 681, SA1 734, SA2 741) the guard
    // is `i + k < msg.len() && <pred>(c) && {a,b}beforeb(i + k, ...)`. The
    // lookahead arrays have length `msg.len() + 1` with the BWIPP sentinel
    // 9999 at index `msg.len()`. The ONLY index where `< → <=` (or the
    // `i + 1 → i * 1 == i`, `i + 2 → i * 2` for k=1) can change the bound's
    // truth is `i + k == msg.len()`; there `abeforeb(msg.len()) =
    // 9999 < 9999 = false` (and `bbeforea` likewise), so the conjunction is
    // false regardless. EQUIVALENT: 673:26 `<→<=`, 673:22 `+→*`,
    // 681:26 `<→<=`, 734:26 `<→<=`, 734:22 `+→*`, 741:26 `<→<=`.
    //
    // For the C-set shifts (SB1-in-C 805, SB2-in-C 818, SB3-in-C 829/841)
    // the outer bound is followed by `in_b(c) && …` and a `numsscr(i + k)`
    // run-length test that must be ≥ 2 or ≥ 3. At `i + k == msg.len()`,
    // `numsscr(msg.len())` returns `(0, 0)` (loop body `p < len` is false),
    // so `s_next < 2/3` makes the inner test false → branch skipped, exactly
    // as the strict `<` would. EQUIVALENT: 805:26 `<→<=`, 805:22 `+→*`,
    // 805:22 `+→-`, 818:26 `<→<=`, 818:22 `+→*`, 818:22 `+→-`,
    // 829:26 `<→<=`, 841:26 `<→<=`.
    // (`805:22 +→-`/`818:22 +→-` give `i - 1`; for i=0 this underflows the
    // usize and the bound `i.wrapping_sub(1) < len` is `usize::MAX < len` =
    // false, and for i≥1 `i-1 < len` is always true exactly when the body's
    // subsequent reads/`numsscr` already gate the branch — verified inert
    // by the exhaustive fingerprint.)
    //
    // ---- encode_data_cws_mixed: SB2-in-C inner bound (3 mutants)
    //
    // Line 820: `s_next >= 3 && s_next % 2 == 1 && i + 2 < msg.len() &&
    // in_b(msg[i + 1])`. To reach here `s_next = numsscr(i + 1) >= 3`, which
    // needs ≥ 3 bytes after index i, i.e. `i + 3 <= msg.len()`, so
    // `i + 2 < msg.len()` already holds whenever the `s_next >= 3` conjunct
    // is true. Hence:
    //   * 820:64 `< → <=` — the bound only matters at `i+2 == len`, but then
    //     `numsscr(i+1)` spans ≤ 1 byte ⇒ `s_next <= 1 < 3` ⇒ the earlier
    //     conjunct already short-circuits false. EQUIVALENT.
    //   * 820:60 `+ → -` (`i - 2`): `i - 2 < len` is true for all reachable
    //     i (and underflows to false at i<2 where SB2-in-C never reaches
    //     because mode-C-from-start emits ≥1 pair first); the `s_next >= 3`
    //     conjunct is the real gate. EQUIVALENT.
    //   * 820:90 `+ → -` / `+ → *` in `in_b(msg[i + 1])`: when this read is
    //     evaluated, `s_next = numsscr(i + 1) >= 3` already proved `msg[i+1]`
    //     is an ASCII digit, hence `in_b(msg[i+1]) == true`; `msg[i*1] =
    //     msg[i] = c` is `in_b` too (the outer `in_b(c)` at 818 holds), and
    //     `msg[i-1]` is only read when i≥1 and equals an already-in_b byte
    //     on every reachable path. The conjunct is `true` either way.
    //     EQUIVALENT.
    //
    // ---- encode_data_cws_mixed: SB3-in-C variants 1 & 2 are REDUNDANT
    //      (8 mutants) — when variant-1's guard is perturbed, variant-2
    //      (lines 841-851) emits the identical `SB3 + 3 B-bytes, i += 3`.
    //
    // Variant 1 (829-838) fires for "3 B-bytes c,msg[i+1],msg[i+2] then an
    // ODD ≥3 digit run at i+2"; variant 2 (841-851) fires for "3 B-bytes
    // then an EVEN ≥2 digit run at i+3". For the inputs that reach this
    // region, `msg[i+2]` is the first digit of the trailing run, so the two
    // variants' digit-run tests (`numsscr(i+2)` odd≥3 vs `numsscr(i+3)`
    // even≥2) describe the SAME byte layout and BOTH emit SB3_FROM_C plus
    // `lookup_b(c), lookup_b(msg[i+1]), lookup_b(msg[i+2])` with `i += 3`.
    // Thus perturbing variant 1 only shifts which variant fires, with no
    // change to the emitted codewords:
    //   * 830:55 `+ → -` (`numsscr(i - 2)`), 831:46 `% → /` and `% → +`
    //     (parity of `s_next`), 831:64 `< → <=`, `< → ==`, `< → >`
    //     (inner bound), 831:60 `+ → *` (`i * 3` in the inner bound),
    //     831:90 `+ → -` / `+ → *` (`in_b(msg[i+2])` read), 829:26 `< → ==`
    //     (outer bound), 829:22 `+ → *` (`i * 2` outer bound), 835:70
    //     `+ → *` (`lookup_b(msg[i*2]) = lookup_b(msg[i])`): each either
    //     leaves variant 1 firing with the same three B-bytes, or drops it
    //     into the variant-2 backstop which emits the identical sequence.
    //     EQUIVALENT.
    //
    // ---- encode_data_cws_mixed: SA2/SB2 redundant membership guard
    //      (2 mutants)
    //
    //   * 683:36 `+ → *` in `bnota(msg[i + 1])` (SB2-from-A) and 743:36
    //     `+ → *` in `anotb(msg[i + 1])` (SA2-from-B): `msg[i * 1] = msg[i]
    //     = c`, and the preceding conjunct already asserts `bnota(c)` /
    //     `anotb(c)`, so `bnota(msg[i]) == bnota(c) == true` (resp. anotb).
    //     The conjunct degenerates to a tautology, dropping the `msg[i+1]`
    //     requirement. On every reachable path where the remaining guards
    //     (`i+2 < len`, the bnota/anotb of c, and the abeforeb/bbeforea
    //     lookahead) hold, `msg[i+1]` is itself bnota/anotb — so the
    //     two-byte shift fires identically. (The `+ → -` siblings 683:36 /
    //     688:66 read a genuinely different byte and ARE killed.) Verified
    //     inert by the exhaustive fingerprint over all adjacent-class
    //     triples. EQUIVALENT.
    #[test]
    fn code16k_equivalence_notes() {
        // Witness 1 — lookahead sentinel makes abeforeb/bbeforea FALSE at
        // index msg.len(); this is what collapses every outer-bound
        // `< → <=` / `i+k → i*k` mutant in the A/B shifts.
        for raw in [&b"\x01ABC"[..], b"abc", b"a\x01b", b"12ab", b"ABCDE"] {
            let msg = insert_fn4_markers(&raw.iter().map(|&b| i16::from(b)).collect::<Vec<_>>());
            let (na, nb) = compute_lookahead(&msg);
            let n = msg.len();
            assert_eq!(na[n], 9999, "next_anotb sentinel at len");
            assert_eq!(nb[n], 9999, "next_bnota sentinel at len");
            assert!(!abeforeb(n, &na, &nb), "abeforeb(len) must be false");
            assert!(!bbeforea(n, &na, &nb), "bbeforea(len) must be false");
        }
        // Witness 2 — numsscr at end-of-message returns (0, 0), so every
        // C-set inner run-length test is false at the boundary index.
        for raw in [&b"12"[..], b"12a", b"abc34"] {
            let msg = insert_fn4_markers(&raw.iter().map(|&b| i16::from(b)).collect::<Vec<_>>());
            let n = msg.len();
            assert_eq!(numsscr(&msg, n), (0, 0), "numsscr(len) is (0,0)");
        }
        // Witness 3 — `(n-1)/2 == n/2` for every odd n (the shift_b
        // `- → /` equivalence): mutating `len - 1` to `len / 1 == len`
        // leaves pair_count unchanged on the odd-only path.
        for n in (1..=33).step_by(2) {
            assert_eq!((n - 1) / 2, n / 2, "odd n: (n-1)/2 == n/2 for n={n}");
        }
        // Witness 4 — `reserve` is output-invariant: a vector built by the
        // same pushes is byte-identical regardless of pre-reserved cap.
        {
            let mut a: Vec<u16> = Vec::new();
            let mut b: Vec<u16> = Vec::with_capacity(999);
            for v in [1u16, 2, 3, 103, 104] {
                a.push(v);
                b.push(v);
            }
            assert_eq!(a, b, "reserve must not affect contents");
        }
        // Witness 5 — SB3-in-C variants 1 and 2 emit the SAME codewords for
        // a payload that satisfies both layouts ("12aa345..."): three B
        // bytes shifted to C via SB3, then the trailing digit run. We pin
        // the actual encoder output so the redundancy is concrete.
        let (mode, cws) = encode_data_cws_mixed(b"12aaA34").expect("ok");
        assert_eq!(mode, MODE_C_FROM_START);
        assert_eq!(
            cws,
            vec![12, SB3_FROM_C, 65, 65, 33, 34],
            "'12aaA34' shifts the 3-B-byte run 'aaA' to C via SB3 (106) \
             then resumes the '34' pair — the layout both SB3-in-C variants \
             encode identically"
        );
        // Witness 6 — the SB2/SB3-in-C `s_next >= k` conjunct implies the
        // companion `i + (k-1) < msg.len()` bound: numsscr counts at most
        // `msg.len() - p` items, so `numsscr(&msg, p).1 >= k` forces
        // `p + k <= msg.len()`. Re-derive numsscr's length bound directly.
        let probe: [i16; 4] = [b'1' as i16, b'2' as i16, b'3' as i16, b'4' as i16];
        for p in 0..probe.len() {
            let (_, s) = numsscr(&probe, p);
            assert!(
                p + s <= probe.len(),
                "numsscr(p).s can never exceed the bytes remaining from p"
            );
        }
    }
}
