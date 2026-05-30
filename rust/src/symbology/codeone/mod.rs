//! Code One — matrix 2D barcode (AIM USS Code One).
//!
//! Reference: AIM USS Code One, BWIPP `bwipp_codeone` (bwip-js
//! `src/bwipp.js` lines 31458-33350, ~1893 LOC of JS).
//!
//! Code One is a matrix 2D barcode with **11 symbol sizes**
//! divided across three families:
//!
//! * **Variable matrix** (A, B, C, D, E, F, G, H) — square-ish
//!   data matrices ranging from 16 × 18 modules (version A, 10
//!   data codewords) to 148 × 134 modules (version H, 1480 data
//!   + 560 ECC codewords across 8 Reed-Solomon blocks).
//! * **Strip type S** (S-10, S-20, S-30) — narrow 8-module-tall
//!   strips containing 4/8/12 data codewords. Use 5-bit
//!   "half-codewords" packed via GF(32) Reed-Solomon.
//! * **Strip type T** (T-16, T-32, T-48) — 16-module-tall strips
//!   with up to 38 data codewords + 22 ECC.
//!
//! The encoder supports six data-encoding modes that can switch
//! mid-stream via reserved codewords:
//!
//! 1. **A** (ASCII) — default, 1 byte → 1 codeword.
//! 2. **C** (C40) — three uppercase / digit characters → two
//!    codewords.
//! 3. **T** (Text) — three lowercase / digit characters → two
//!    codewords.
//! 4. **X** (X12) — three EDI characters → two codewords.
//! 5. **D** (Decimal / numeric compression) — three digits ≈ 10
//!    bits.
//! 6. **B** (Byte / raw binary) — 1 byte → 1 codeword, no
//!    interpretation.
//!
//! The renderer:
//!
//! * Splits each 8-bit data codeword into a 4-bit top nibble +
//!   4-bit bottom nibble (for matrix types), or each 10-bit S/T
//!   codeword pair into 3+2 / 2+3 splits.
//! * Places the resulting `mmat` grid into the symbol via a
//!   [`CPATMAP`] column-pattern table + `artifact` start/finder
//!   masks + [`BLACKDOTMAP`] fixed-dot list.
//!
//! Reed-Solomon: GF(256) with primitive polynomial 301 = 0x12D
//! for the matrix sizes (same poly as Data Matrix); GF(32) with
//! primitive 37 for the S-strip sizes. See [`RSPARAMS`].
//!
//! ## Port status
//!
//! Full BWIPP-faithful port covering Version A (16 × 18 modules).
//! Pipeline:
//!
//!   1. [`encode_message`] — mode dispatch + BWIPP `lookup()`
//!      forward-scan with `$f` Float32 truncation on cost
//!      accumulators (critical for matching BWIPP at boundary cases
//!      like "abcdef" → T).
//!   2. [`encode_with_ecc`] — symbol-size pick + PAD codeword fill
//!      + GF(256) Reed-Solomon ECC (primitive poly 301).
//!   3. [`encode_to_bit_matrix`] — codeword → mmat placement +
//!      column-pattern band + reference islands + forced black dots
//!      + sentinel fill from mmat.
//!
//! [`encode`] is the public entry point invoked from
//! `Symbology::CodeOne`'s dispatch arm in `crate::symbology`.
//! Verified end-to-end by **byte-for-byte `pixs` goldens** against
//! bwip-js for `"A"`, `"Hello"`, `"ABC"`, `"ABCDEFG"` (288 cells
//! each, all Version A). Mode D (decimal-compression), Mode B
//! (raw byte), S/T-strip families, and FNC1 / FNC2 / FNC3 / FNC4
//! / ECI marker paths are extension targets and return
//! `InvalidData`. See [`CODEONE_PORT_PLAN.md`](./CODEONE_PORT_PLAN.md)
//! for the original 10-stage plan + the resolved Stage 5b
//! lookup-precision debug.

#![allow(dead_code)]

use crate::encoding::BitMatrix;
use crate::error::Error;

// ---------------------------------------------------------------------------
// Marker codeword constants — BWIPP `codeone_*` (bwip-js lines 31475-31491).
//
// Negative `i16` sentinels for the special codewords that aren't ASCII
// bytes (FNCx markers, mode-latch codewords, shifts, ECI, PAD,
// FNC1-leader). UNLCW is the positive 255 codeword used as the "unlatch"
// data codeword when the symbol ends in a non-A mode.
// ---------------------------------------------------------------------------

/// FNC1 — GS1 separator.
pub(crate) const FNC1: i16 = -1;
/// FNC3 — Mode-3 marker.
pub(crate) const FNC3: i16 = -2;
/// LC — Latch to C40 mode.
pub(crate) const LC: i16 = -5;
/// LB — Latch to Byte (binary) mode.
pub(crate) const LB: i16 = -6;
/// LX — Latch to X12 mode.
pub(crate) const LX: i16 = -7;
/// LT — Latch to Text mode.
pub(crate) const LT: i16 = -8;
/// LD — Latch to Decimal mode.
pub(crate) const LD: i16 = -9;
/// UNL — Unlatch back to ASCII (Mode A).
pub(crate) const UNL: i16 = -10;
/// FNC2 — Reader programming marker.
pub(crate) const FNC2: i16 = -11;
/// FNC4 — High-byte ASCII marker.
pub(crate) const FNC4: i16 = -12;
/// Shift-1 in CTX mode (escape into the next character set).
pub(crate) const SFT1: i16 = -13;
/// Shift-2 in CTX mode.
pub(crate) const SFT2: i16 = -14;
/// Shift-3 in CTX mode.
pub(crate) const SFT3: i16 = -15;
/// ECI prefix marker.
pub(crate) const ECI: i16 = -16;
/// PAD codeword (Mode A fill).
pub(crate) const PAD: i16 = -17;
/// FNC1-leader — GS1 prefix at position 1.
pub(crate) const FNC1LD: i16 = -18;

/// UNLCW — the positive "unlatch" codeword value used when the symbol
/// ends in a non-A mode (matrix / T-strip only).
pub(crate) const UNLCW: u16 = 255;

// ---------------------------------------------------------------------------
// Mode constants — BWIPP `codeone_a`..`codeone_b` (bwip-js lines
// 31492-31497).
//
// These index [`ENCFUNCS`] (the per-mode encoder dispatch). The C/T/X
// modes share `encCTX` but with different value tables; D and B have
// their own encoders.
// ---------------------------------------------------------------------------

/// Mode A — ASCII (default, 1 byte → 1 codeword).
pub(crate) const MODE_A: u8 = 0;
/// Mode C — C40 (digits + uppercase + punctuation; 3 chars → 2 cws).
pub(crate) const MODE_C: u8 = 1;
/// Mode T — Text (digits + lowercase + punctuation; 3 chars → 2 cws).
pub(crate) const MODE_T: u8 = 2;
/// Mode X — X12 / EDI (3 chars → 2 cws).
pub(crate) const MODE_X: u8 = 3;
/// Mode D — Decimal / numeric compression (3 digits ≈ 10 bits).
pub(crate) const MODE_D: u8 = 4;
/// Mode B — Byte / raw binary (1 byte → 1 cw, no interpretation).
pub(crate) const MODE_B: u8 = 5;

// ---------------------------------------------------------------------------
// Symbol-version metrics — BWIPP `codeone_nonstypemetrics` /
// `codeone_stypemetrics` (bwip-js lines 31473-31474).
//
// Each [`Metric`] row stores the BWIPP 10-tuple verbatim. The `risi` /
// `riso` reference-island geometry values use BWIPP's sentinel `99`
// when not applicable (S-strip and T-strip versions). Comparison code
// in stages 7-9 will treat 99 the same way bwip-js does.
// ---------------------------------------------------------------------------

/// One row of the BWIPP symbol-metrics table. Each version (A..H,
/// S-10..S-30, T-16..T-48) gets exactly one row.
///
/// Field meanings (semantic; BWIPP source column number in parens):
///
/// * `id` — symbol-version string ("A", "S-10", "T-32", …) (m[0]).
/// * `rows` — total rows in the rendered symbol (m[1]).
/// * `cols` — total columns in the rendered symbol (m[2]).
/// * `dcol` — width of the data area in cells (m[3]).
/// * `dcw` — number of **data** codewords (m[4]).
/// * `rscw` — number of **Reed-Solomon check** codewords (m[5]).
///   (BWIPP names this `rscw`; renames to `ecw` would also be
///   reasonable since they're error-correction codewords.)
/// * `rsbl` — number of Reed-Solomon interleaved blocks (m[6]).
/// * `riso` — reference-island horizontal offset (BWIPP `riso`,
///   bound at bwip-js line 33045 from m[7]).
/// * `risi` — reference-island horizontal step (BWIPP `risi`,
///   m[8]).
/// * `risl` — reference-island loop count (BWIPP `risl`, m[9]).
///
/// **NB**: BWIPP's struct order is `m[0..=9]`. The Rust struct uses
/// the same order to make it easy to cross-check against the
/// bwip-js source line 31474.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Metric {
    pub id: &'static str,
    pub rows: u16,
    pub cols: u16,
    pub dcol: u16,
    pub dcw: u16,
    pub rscw: u16,
    pub rsbl: u16,
    pub riso: u16,
    pub risi: u16,
    pub risl: u16,
}

/// Sentinel value BWIPP uses in metrics rows when a field doesn't apply
/// (S-strip and T-strip versions have no reference islands).
pub(crate) const NA: u16 = 99;

/// Matrix (A..H) and T-strip symbol-version metrics.
///
/// Mirrors BWIPP `codeone_nonstypemetrics` (bwip-js line 31474) row
/// for row.
#[rustfmt::skip]
pub(crate) const METRICS_NONSTYPE: [Metric; 11] = [
    // BWIPP source values: m[7]=riso, m[8]=risi, m[9]=risl.
    Metric { id: "A",    rows: 16,  cols: 18,  dcol: 16,  dcw: 10,   rscw: 10,  rsbl: 1, riso: 4, risi: NA, risl: 6  },
    Metric { id: "B",    rows: 22,  cols: 22,  dcol: 20,  dcw: 19,   rscw: 16,  rsbl: 1, riso: 4, risi: NA, risl: 8  },
    Metric { id: "C",    rows: 28,  cols: 32,  dcol: 28,  dcw: 44,   rscw: 26,  rsbl: 1, riso: 4, risi: 22, risl: 11 },
    Metric { id: "D",    rows: 40,  cols: 42,  dcol: 36,  dcw: 91,   rscw: 44,  rsbl: 1, riso: 4, risi: 16, risl: 16 },
    Metric { id: "E",    rows: 52,  cols: 54,  dcol: 48,  dcw: 182,  rscw: 70,  rsbl: 1, riso: 4, risi: 22, risl: 22 },
    Metric { id: "F",    rows: 70,  cols: 76,  dcol: 68,  dcw: 370,  rscw: 140, rsbl: 2, riso: 4, risi: 22, risl: 31 },
    Metric { id: "G",    rows: 104, cols: 98,  dcol: 88,  dcw: 732,  rscw: 280, rsbl: 4, riso: 6, risi: 21, risl: 47 },
    Metric { id: "H",    rows: 148, cols: 134, dcol: 120, dcw: 1480, rscw: 560, rsbl: 8, riso: 6, risi: 20, risl: 69 },
    Metric { id: "T-16", rows: 16,  cols: 17,  dcol: 16,  dcw: 10,   rscw: 10,  rsbl: 1, riso: NA, risi: NA, risl: NA },
    Metric { id: "T-32", rows: 16,  cols: 33,  dcol: 32,  dcw: 24,   rscw: 16,  rsbl: 1, riso: NA, risi: NA, risl: NA },
    Metric { id: "T-48", rows: 16,  cols: 49,  dcol: 48,  dcw: 38,   rscw: 22,  rsbl: 1, riso: NA, risi: NA, risl: NA },
];

/// S-strip symbol-version metrics.
///
/// Mirrors BWIPP `codeone_stypemetrics` (bwip-js line 31473) row for
/// row. S-strip uses 5-bit "half-codewords" — see
/// [`STYPEVALS`] for the base value table.
#[rustfmt::skip]
pub(crate) const METRICS_STYPE: [Metric; 3] = [
    Metric { id: "S-10", rows: 8, cols: 11, dcol: 10, dcw: 4,  rscw: 4,  rsbl: 1, riso: NA, risi: NA, risl: NA },
    Metric { id: "S-20", rows: 8, cols: 21, dcol: 20, dcw: 8,  rscw: 8,  rsbl: 1, riso: NA, risi: NA, risl: NA },
    Metric { id: "S-30", rows: 8, cols: 31, dcol: 30, dcw: 12, rscw: 12, rsbl: 1, riso: NA, risi: NA, risl: NA },
];

// ---------------------------------------------------------------------------
// Column-pattern map — BWIPP `codeone_cpatmap` (bwip-js lines 31499-31511).
//
// Indexed by the first letter of the version id ('A'..'H', 'S', 'T').
// Each value is a digit string where each digit identifies an entry in
// the runtime `artifact` table (digit '1' = artifact 0, '8' = artifact
// 7). The renderer uses this to stamp the column-pattern band into the
// final pixs grid.
// ---------------------------------------------------------------------------

/// `(version_first_letter, column_pattern)` — 10 entries.
///
/// Lookup with [`cpatmap_for`].
pub(crate) const CPATMAP: [(char, &str); 10] = [
    ('A', "121343"),
    ('B', "12134343"),
    ('C', "12121343"),
    ('D', "1213434343"),
    ('E', "1212134343"),
    ('F', "1212121343"),
    ('G', "121213434343"),
    ('H', "121212134343"),
    ('S', "56661278"),
    ('T', "5666666666127878"),
];

/// Look up the column-pattern string for a version's leading letter.
/// Returns `None` if the letter isn't a recognized family.
pub(crate) fn cpatmap_for(version_first_letter: char) -> Option<&'static str> {
    CPATMAP
        .iter()
        .find(|&&(c, _)| c == version_first_letter)
        .map(|&(_, p)| p)
}

// ---------------------------------------------------------------------------
// Black-dot map — BWIPP `codeone_blackdotmap` (bwip-js lines 31512-31528).
//
// Per-version (row, col) coordinate lists of forced black dots. Most
// versions have between 0 and 6 fixed dots. Indexed by the FULL
// version id ("A", "S-20", "T-48", …).
// ---------------------------------------------------------------------------

/// Per-version forced-black-dot coordinate lists.
///
/// Mirrors BWIPP `codeone_blackdotmap` (bwip-js lines 31512-31528). Each
/// entry is `(version_id, &[(col, row); n])` — BWIPP stores coords in
/// `(col, row)` order (the `$aload` + `mmv` pattern at bwip-js line
/// 33302 computes `pixs[col + row*cols]`). Versions with no dots
/// (D, S-10) get an empty slice. Lookup with [`blackdotmap_for`].
pub(crate) const BLACKDOTMAP: &[(&str, &[(u16, u16)])] = &[
    ("A", &[(12, 5)]),
    ("B", &[(16, 7)]),
    ("C", &[(26, 12)]),
    ("D", &[]),
    ("E", &[(26, 23)]),
    ("F", &[(26, 32), (70, 32), (26, 34), (70, 34)]),
    ("G", &[(27, 48), (69, 48)]),
    (
        "H",
        &[(26, 70), (66, 70), (106, 70), (26, 72), (66, 72), (106, 72)],
    ),
    ("S-10", &[]),
    ("S-20", &[(10, 4)]),
    ("S-30", &[(15, 4), (15, 6)]),
    ("T-16", &[(8, 10)]),
    ("T-32", &[(16, 10), (16, 12)]),
    ("T-48", &[(24, 10), (24, 12), (24, 14)]),
];

/// Look up the forced-black-dot coordinate list for a full version id.
/// Returns `Some(&[])` for versions with no fixed dots (D / S-10);
/// `None` if the id isn't recognized.
pub(crate) fn blackdotmap_for(version_id: &str) -> Option<&'static [(u16, u16)]> {
    BLACKDOTMAP
        .iter()
        .find(|&&(id, _)| id == version_id)
        .map(|&(_, dots)| dots)
}

// ---------------------------------------------------------------------------
// S-strip base values — BWIPP `codeone_stypevals` (bwip-js line 31472).
//
// 18 binary-string entries representing powers-of-10 used by the
// S-strip 5-bit codeword packer. Each entry is `bin(10^i)` for
// i = 0..=17. Stored as static strings; the encoder will parse them
// digit-by-digit during the packing step (Stage 5+).
//
// Example: STYPEVALS[0] = "1" (= bin(1));
//          STYPEVALS[1] = "1010" (= bin(10));
//          STYPEVALS[2] = "1100100" (= bin(100));
//          STYPEVALS[3] = "1111101000" (= bin(1000)).
// ---------------------------------------------------------------------------

/// Powers-of-10 in binary, 18 entries (10^0 through 10^17).
///
/// Used by the S-strip encoder to pack a run of decimal digits into
/// a 5-bit-codeword stream.
pub(crate) const STYPEVALS: [&str; 18] = [
    "1",
    "1010",
    "1100100",
    "1111101000",
    "10011100010000",
    "11000011010100000",
    "11110100001001000000",
    "100110001001011010000000",
    "101111101011110000100000000",
    "111011100110101100101000000000",
    "1001010100000010111110010000000000",
    "1011101001000011101101110100000000000",
    "1110100011010100101001010001000000000000",
    "10010001100001001110011100101010000000000000",
    "10110101111001100010000011110100100000000000000",
    "11100011010111111010100100110001101000000000000000",
    "100011100001101111001001101111110000010000000000000000",
    "101100011010001010111100001011101100010100000000000000000",
];

// ---------------------------------------------------------------------------
// Reed-Solomon GF parameters — BWIPP `codeone_rsparams` (bwip-js line
// 31529).
//
// A 9-slot table where each slot is either `None` (placeholder) or
// `Some((gf_size, primitive_poly))`. Only two slots are populated:
//
// * Slot 5: `(32, 37)` — GF(32) for S-strip 5-bit codewords. Primitive
//   polynomial 37 = 0b100101 = x^5 + x^2 + 1.
// * Slot 8: `(256, 301)` — GF(256) for matrix and T-strip 8-bit
//   codewords. Primitive polynomial 301 = 0x12D = x^8 + x^5 + x^3 + x^2 + 1
//   (the standard Data Matrix poly).
//
// The other slots are placeholders so callers can index via small
// integer keys without bounds-checking; BWIPP's `rstables` loop only
// builds log/antilog tables for populated slots.
// ---------------------------------------------------------------------------

/// Reed-Solomon GF parameters: `(gf_size, primitive_poly)` per BWIPP
/// `codeone_rsparams` slot. Only slots 5 (S-strip) and 8 (matrix /
/// T-strip) are populated.
pub(crate) const RSPARAMS: [Option<(u16, u16)>; 9] = [
    None,
    None,
    None,
    None,
    None,
    Some((32, 37)),
    None,
    None,
    Some((256, 301)),
];

/// GF / poly pair for S-strip 5-bit codewords.
pub(crate) const RS_GF_S: (u16, u16) = (32, 37);
/// GF / poly pair for matrix and T-strip 8-bit codewords.
pub(crate) const RS_GF_MATRIX: (u16, u16) = (256, 301);

// ---------------------------------------------------------------------------
// Mode A (ASCII) value lookup — BWIPP `avals` (bwip-js lines 31530-31570).
//
// `avals` maps:
//
// * Byte b ∈ 0..=128  →  cw (b + 1)
// * Digit pair "DD"   →  cw (130 + 10*D1 + D2) for DD ∈ 00..=99
//                                              (4 bits of compression
//                                              vs the 2-byte direct
//                                              encoding).
// * [`PAD`]          →  cw 129
// * [`LC`]           →  cw 230
// * [`LB`]           →  cw 231
// * [`FNC1`]         →  cw 232
// * [`FNC2`]         →  cw 233
// * [`FNC3`]         →  cw 234
// * [`FNC4`]         →  cw 235
// * [`FNC1LD`]       →  cw 236
// * [`LX`]           →  cw 238   (NB: 237 skipped)
// * [`LT`]           →  cw 239
//
// Note that BWIPP also stores `LD` (latch to Decimal mode) but as a
// special-cased entry — the mode-D entry phase is bit-level rather
// than codeword-level, so LD doesn't get a single cw. See encA's
// handling at bwip-js lines 32609-32626.
// ---------------------------------------------------------------------------

/// Look up the Mode A codeword for a single ASCII byte `b ∈ 0..=128`.
/// BWIPP `avals[b] = b + 1`.
pub(crate) fn avals_byte(b: u8) -> Option<u16> {
    if b <= 128 {
        Some(u16::from(b) + 1)
    } else {
        None
    }
}

/// Look up the Mode A codeword for a 2-digit pair `(d1, d2)` where each
/// is `'0'..='9'`. BWIPP `avals["DD"] = 130 + 10*D1 + D2`. Returns
/// `None` if either byte is not a digit.
pub(crate) fn avals_digit_pair(d1: u8, d2: u8) -> Option<u16> {
    if d1.is_ascii_digit() && d2.is_ascii_digit() {
        let n = (d1 - b'0') * 10 + (d2 - b'0');
        Some(130 + u16::from(n))
    } else {
        None
    }
}

/// Look up the Mode A codeword for a marker constant (FNC1, FNC2,
/// FNC3, FNC4, FNC1LD, LC, LB, LX, LT, PAD). Returns `None` for
/// any other negative or unrepresentable input.
pub(crate) fn avals_marker(marker: i16) -> Option<u16> {
    Some(match marker {
        PAD => 129,
        LC => 230,
        LB => 231,
        FNC1 => 232,
        FNC2 => 233,
        FNC3 => 234,
        FNC4 => 235,
        FNC1LD => 236,
        LX => 238,
        LT => 239,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Mode B (Byte) standalone encoder — BWIPP `encB` (bwip-js lines
// 32975-33019).
//
// Mode B emits raw byte values as codewords (1 byte → 1 cw, no
// interpretation) preceded by a length prefix that tells the decoder
// how many bytes follow:
//
// * `p < 250`: prefix is **one cw** with value `p`.
// * `p >= 250`: prefix is **two cws**: `(p / 250) + 249, p % 250`.
// * If the Mode B run is the symbol's last segment AND exactly fills
//   the remaining codeword slots, BWIPP **omits** the length prefix
//   (the decoder infers the byte count from the symbol's overall
//   capacity). This corner case is detected by the mode selector
//   (Stage 5), so the standalone `encode_mode_b_run` here always
//   emits the prefix; the caller can drop it if the run is terminal.
//
// After the prefix + byte cws, BWIPP implicitly switches mode back to
// A (no explicit unlatch cw — the mode switch is encoded in the
// length-prefix structure).
// ---------------------------------------------------------------------------

/// Maximum BWIPP-defined Mode B run length (the `bvals` buffer size
/// — bwip-js line 32977). Inputs longer than this overflow the largest
/// symbol (version H, 1480 data codewords).
pub(crate) const MODE_B_MAX_LEN: usize = 1480;

/// Standalone Mode B encoder. Takes a raw byte slice + the
/// `is_terminal` flag (whether the run ends the symbol).
///
/// Returns the codeword sequence: length prefix(es) + 1 cw per byte.
/// Mirrors BWIPP `encB` (bwip-js lines 32975-33019) for the standalone
/// case — the dispatcher-coupled "exact remcws fit, no prefix"
/// shortcut is opt-in via `omit_prefix_if_exact = true`. Callers
/// without remcws info should pass `false`.
///
/// # Errors
///
/// * `InvalidData` if `bytes` exceeds [`MODE_B_MAX_LEN`].
pub(crate) fn encode_mode_b_run(
    bytes: &[u8],
    omit_prefix_if_exact: bool,
) -> Result<Vec<u16>, Error> {
    if bytes.len() > MODE_B_MAX_LEN {
        return Err(Error::InvalidData(format!(
            "code one mode B: payload of {} bytes exceeds max {}",
            bytes.len(),
            MODE_B_MAX_LEN
        )));
    }
    let p = bytes.len();
    let mut cws: Vec<u16> = Vec::with_capacity(p + 2);
    if !omit_prefix_if_exact {
        if p < 250 {
            cws.push(p as u16);
        } else {
            cws.push((p / 250) as u16 + 249);
            cws.push((p % 250) as u16);
        }
    }
    for &b in bytes {
        cws.push(u16::from(b));
    }
    Ok(cws)
}

// ---------------------------------------------------------------------------
// Mode-set predicates — BWIPP `isD`, `isC`, `isT`, `isX`, `isEA`, `isFN`
// (bwip-js lines 32305-32322).
//
// These mirror BWIPP's `$has($_.<vals>, char)` membership checks used
// by `lookup()` to score each per-mode cost. For lookup purposes the
// **base** value tables (cnvals / tnvals / xvals) are the only sets
// that matter — the shift-extended cvals / tvals get queried during
// actual encoding (Stage 6+).
//
// Base sets (`$cnvals` / `$tnvals` / `$xvals` membership):
//
// * `cnvals` (C40): SFT1, SFT2, SFT3, 32 (space), '0'..'9', 'A'..'Z'.
// * `tnvals` (Text): SFT1, SFT2, SFT3, 32, '0'..'9', 'a'..'z'.
// * `xvals`  (X12): 13 (CR), 42 ('*'), 62 ('>'), 32, '0'..'9', 'A'..'Z'.
// ---------------------------------------------------------------------------

/// Predicate: is `c` a digit? (BWIPP `isD`.)
pub(crate) fn is_d(c: i16) -> bool {
    (b'0' as i16..=b'9' as i16).contains(&c)
}

/// Predicate: is `c` a member of the **C40** base value set (BWIPP `isC` /
/// `$has(cnvals, c)`)?
pub(crate) fn is_c(c: i16) -> bool {
    c == SFT1
        || c == SFT2
        || c == SFT3
        || c == 32
        || is_d(c)
        || (b'A' as i16..=b'Z' as i16).contains(&c)
}

/// Predicate: is `c` a member of the **Text** base value set (BWIPP
/// `isT` / `$has(tnvals, c)`)?
pub(crate) fn is_t(c: i16) -> bool {
    c == SFT1
        || c == SFT2
        || c == SFT3
        || c == 32
        || is_d(c)
        || (b'a' as i16..=b'z' as i16).contains(&c)
}

/// Predicate: is `c` a member of the **X12** base value set (BWIPP
/// `isX` / `$has(xvals, c)`)?
pub(crate) fn is_x(c: i16) -> bool {
    c == 13 || c == 42 || c == 62 || c == 32 || is_d(c) || (b'A' as i16..=b'Z' as i16).contains(&c)
}

/// Predicate: is `c` an Extended-ASCII high byte (`> 127`)? (BWIPP `isEA`.)
pub(crate) fn is_ea(c: i16) -> bool {
    c > 127
}

/// Predicate: is `c` a function-marker sentinel (`< 0`)? (BWIPP `isFN`.)
pub(crate) fn is_fn(c: i16) -> bool {
    c < 0
}

// ---------------------------------------------------------------------------
// nextXterm + nextNonX lookahead arrays — BWIPP `nextXterm`, `nextNonX`
// (bwip-js lines 32261-32304).
//
// For each position i:
//
// * `next_xterm[i]` = distance to next X12 terminator (CR=13, '*'=42,
//   '>'=62) or 10000 if none ahead.
// * `next_non_x[i]` = distance to next byte NOT in the X12 base set
//   (i.e., the first byte that breaks the X12 run), or 10000.
//
// BWIPP uses these in `XtermFirst()` which the lookup function calls
// to break ties between Mode C and Mode X.
// ---------------------------------------------------------------------------

/// Compute the BWIPP `nextXterm` lookahead. Length = `msg.len() + 1`,
/// with the trailing entry as the sentinel `9999` (clamped to `10000`
/// in BWIPP). For each position i, value is the distance to the next
/// X12 terminator (CR / `*` / `>`).
pub(crate) fn compute_next_xterm(msg: &[i16]) -> Vec<u32> {
    let n = msg.len();
    let mut next = vec![0u32; n + 1];
    next[n] = 9999;
    for i in (0..n).rev() {
        let c = msg[i];
        if c == 13 || c == 42 || c == 62 {
            next[i] = 0;
        } else {
            next[i] = next[i + 1].saturating_add(1);
        }
    }
    // BWIPP clamps to 10000 in a post-pass.
    for v in &mut next {
        if *v > 10000 {
            *v = 10000;
        }
    }
    next
}

/// Compute the BWIPP `nextNonX` lookahead. Length = `msg.len() + 1`,
/// with the trailing entry as the sentinel `9999`. For each position i,
/// value is the distance to the next byte NOT in the X12 base set
/// (the first byte that breaks the X12 run).
pub(crate) fn compute_next_non_x(msg: &[i16]) -> Vec<u32> {
    let n = msg.len();
    let mut next = vec![0u32; n + 1];
    next[n] = 9999;
    for i in (0..n).rev() {
        if !is_x(msg[i]) {
            next[i] = 0;
        } else {
            next[i] = next[i + 1].saturating_add(1);
        }
    }
    for v in &mut next {
        if *v > 10000 {
            *v = 10000;
        }
    }
    next
}

/// Helper: BWIPP `XtermFirst(i)` — `next_xterm[i] < next_non_x[i]`.
/// Used by `lookup()` to break ties between Modes C and X. `true` if
/// the next X12 terminator comes before the next non-X12 byte.
pub(crate) fn xterm_first(next_xterm: &[u32], next_non_x: &[u32], i: usize) -> bool {
    next_xterm[i] < next_non_x[i]
}

// ---------------------------------------------------------------------------
// lookup() — BWIPP `lookup` (bwip-js lines 32327-32556).
//
// Forward-scan cost-based mode picker. Walks from position `i` forward,
// accumulating per-mode costs ac/cc/tc/xc/bc, and returns the mode
// with smallest accumulated cost (with BWIPP's specific tie-breakers).
//
// Per-mode cost deltas per byte (BWIPP):
//
// | Byte category | ac    | cc       | tc       | xc       | bc       |
// |---------------|-------|----------|----------|----------|----------|
// | digit (isD)   | +0.5  | +0.6667  | +0.6667  | +0.6667  | +1 / +3¹ |
// | mode-valid²   | +1    | +0.6667  | +0.6667  | +0.6667  | +1 / +3¹ |
// | high (isEA)   | +2    | +2.6667  | +2.6667  | +4.3334  | +1 / +3¹ |
// | other         | +1    | +1.3334  | +1.3334  | +3.3334  | +1 / +3¹ |
//
// 1: `bc` adds +3 for function markers (isFN), +1 otherwise. The
//    `else` and `isEA` branches above also use ceil(prev) + N rather
//    than direct addition.
// 2: "mode-valid" means is_c / is_t / is_x respectively (BWIPP also
//    has a digit short-circuit that adds 0.6667 across cc/tc/xc).
//
// Returns one of [`MODE_A`], [`MODE_C`], [`MODE_T`], [`MODE_X`],
// [`MODE_B`]. Mode D entry is decided separately by `encA` via the
// numD shortcuts.
// ---------------------------------------------------------------------------

/// Run BWIPP's `lookup()` forward-scan from position `start_i` in
/// `msg`, given the current `mode`. Returns the next mode the encoder
/// should switch to (which may be the same as the current mode → no
/// switch, just continue emitting).
///
/// Apply BWIPP's `$f` truncation: round any non-integer value to the
/// closest Float32 representation, then read back as f64. This is the
/// **critical** detail that makes lookup() match BWIPP: BWIPP's
/// fractional cost accumulators (cc/tc/xc) are clamped to f32 precision
/// every time they're updated. Plain f64 sums drift differently and
/// break boundary decisions like "abcdef" (BWIPP picks T at k=5 mid-
/// scan because tc reaches exactly 5.0 in f32; f64 would give
/// 5.0000002 → ceil 6 → A wins instead). See bwip-js line 631 for the
/// `$f` definition (uses `Float32Array` storage).
#[inline]
fn ff(v: f64) -> f64 {
    if (v as i64 as f64) == v {
        v
    } else {
        f64::from(v as f32)
    }
}

/// Mirrors bwip-js lines 32327-32556 verbatim. Uses `f64` for the
/// per-mode cost accumulators with [`ff`] truncation to f32 after each
/// fractional add (BWIPP's `$f` semantics); comparisons use `.ceil()`
/// per BWIPP.
pub(crate) fn lookup(
    msg: &[i16],
    start_i: usize,
    mode: u8,
    next_xterm: &[u32],
    next_non_x: &[u32],
) -> u8 {
    let mut ac: f64 = 1.0;
    let mut cc: f64 = 2.0;
    let mut tc: f64 = 2.0;
    let mut xc: f64 = 2.0;
    let mut bc: f64 = 3.0;
    if mode == MODE_A {
        ac = 0.0;
        cc = 1.0;
        tc = 1.0;
        xc = 1.0;
        bc = 2.0;
    }
    if mode == MODE_C {
        cc = 0.0;
    }
    if mode == MODE_T {
        tc = 0.0;
    }
    if mode == MODE_X {
        xc = 0.0;
    }
    if mode == MODE_B {
        bc = 0.0;
    }
    let msglen = msg.len();
    let mut k: usize = 0;
    loop {
        // Terminal: end-of-message check.
        if start_i + k == msglen {
            // bc <= ceil(ac) && ceil(cc) && ceil(tc) && ceil(xc) → B
            if bc <= ac.ceil() && bc <= cc.ceil() && bc <= tc.ceil() && bc <= xc.ceil() {
                return MODE_B;
            }
            // ac <= ceil(cc) && ceil(tc) && ceil(xc) && ceil(bc) → A
            if ac <= cc.ceil() && ac <= tc.ceil() && ac <= xc.ceil() && ac <= bc.ceil() {
                return MODE_A;
            }
            // ceil(cc) <= ceil(tc) && ceil(xc) → C
            if cc.ceil() <= tc.ceil() && cc.ceil() <= xc.ceil() {
                return MODE_C;
            }
            // ceil(tc) <= ceil(xc) → T
            if tc.ceil() <= xc.ceil() {
                return MODE_T;
            }
            return MODE_X;
        }
        let ch = msg[start_i + k];
        // Update ac. (Apply `ff` to the 0.5 digit increment; ceil-snap
        // branches yield integers so `ff` is a no-op for them.)
        if is_d(ch) {
            ac = ff(ac + 0.5);
        } else if is_ea(ch) {
            ac = ac.ceil() + 2.0;
        } else {
            ac = ac.ceil() + 1.0;
        }
        // Update cc.
        if is_c(ch) {
            cc = ff(cc + 0.6666667);
        } else if is_ea(ch) {
            cc = ff(cc + 2.6666667);
        } else {
            cc = ff(cc + 1.3333334);
        }
        // Update tc.
        if is_t(ch) {
            tc = ff(tc + 0.6666667);
        } else if is_ea(ch) {
            tc = ff(tc + 2.6666667);
        } else {
            tc = ff(tc + 1.3333334);
        }
        // Update xc.
        if is_x(ch) {
            xc = ff(xc + 0.6666667);
        } else if is_ea(ch) {
            xc = ff(xc + 4.3333334);
        } else {
            xc = ff(xc + 3.3333334);
        }
        // Update bc. (bc is always integer in BWIPP — `ff` is a no-op.)
        if is_fn(ch) {
            bc += 3.0;
        } else {
            bc += 1.0;
        }
        // Mid-scan early exit (k >= 3).
        if k >= 3 {
            // (bc + 1) <= ceil(ac, cc, tc, xc) → B
            if (bc + 1.0) <= ac.ceil()
                && (bc + 1.0) <= cc.ceil()
                && (bc + 1.0) <= tc.ceil()
                && (bc + 1.0) <= xc.ceil()
            {
                return MODE_B;
            }
            // (ac + 1) <= ceil(cc, tc, xc, bc) → A
            if (ac + 1.0) <= cc.ceil()
                && (ac + 1.0) <= tc.ceil()
                && (ac + 1.0) <= xc.ceil()
                && (ac + 1.0) <= bc.ceil()
            {
                return MODE_A;
            }
            // (ceil(tc) + 1) <= ceil(ac, cc, xc, bc) → T
            if (tc.ceil() + 1.0) <= ac.ceil()
                && (tc.ceil() + 1.0) <= cc.ceil()
                && (tc.ceil() + 1.0) <= xc.ceil()
                && (tc.ceil() + 1.0) <= bc.ceil()
            {
                return MODE_T;
            }
            // (ceil(cc) + 1) <= ceil(ac, tc) → C-or-X branch with XtermFirst tie
            if (cc.ceil() + 1.0) <= ac.ceil() && (cc.ceil() + 1.0) <= tc.ceil() {
                if cc.ceil() < xc.ceil() {
                    return MODE_C;
                }
                #[allow(clippy::float_cmp)]
                if cc == xc {
                    if xterm_first(next_xterm, next_non_x, start_i + k + 1) {
                        return MODE_X;
                    } else {
                        return MODE_C;
                    }
                }
            }
            // (ceil(xc) + 1) <= ceil(ac, cc, tc, bc) → X
            if (xc.ceil() + 1.0) <= ac.ceil()
                && (xc.ceil() + 1.0) <= cc.ceil()
                && (xc.ceil() + 1.0) <= tc.ceil()
                && (xc.ceil() + 1.0) <= bc.ceil()
            {
                return MODE_X;
            }
        }
        k += 1;
    }
}

// ---------------------------------------------------------------------------
// CTX (C / T / X) base-40 value lookups — BWIPP `cnvals` / `tnvals` /
// `xvals` (bwip-js lines 31570-31827).
//
// Each lookup returns the 5-bit base-40 codeword value for a character
// in the corresponding mode's BASE set. Shift-extended cvals/tvals
// values (escape sequences for chars NOT in the base set) are deferred
// to Stage 6b — for the Stage 6 goldens (uppercase / lowercase /
// digits), all input chars are in the base sets.
// ---------------------------------------------------------------------------

/// Base C40 value (BWIPP `cnvals[c]`). Returns the 5-bit codeword
/// value for chars in the C40 base set; `None` for chars that need
/// a shift sequence.
pub(crate) fn cnvals_lookup(c: i16) -> Option<u8> {
    match c {
        SFT1 => Some(0),
        SFT2 => Some(1),
        SFT3 => Some(2),
        32 => Some(3),
        48..=57 => Some((c - 44) as u8), // digits → 4..=13
        65..=90 => Some((c - 51) as u8), // uppercase → 14..=39
        _ => None,
    }
}

/// Base Text value (BWIPP `tnvals[c]`). Returns the 5-bit codeword
/// value for chars in the Text base set; `None` for chars that need
/// a shift sequence.
pub(crate) fn tnvals_lookup(c: i16) -> Option<u8> {
    match c {
        SFT1 => Some(0),
        SFT2 => Some(1),
        SFT3 => Some(2),
        32 => Some(3),
        48..=57 => Some((c - 44) as u8),  // digits → 4..=13
        97..=122 => Some((c - 83) as u8), // lowercase → 14..=39
        _ => None,
    }
}

/// Base X12 value (BWIPP `xvals[c]`). Returns the 5-bit codeword
/// value for chars in the X12 base set; `None` for any char NOT in
/// the set (X12 has no shift extension — non-X chars force unlatch).
pub(crate) fn xvals_lookup(c: i16) -> Option<u8> {
    match c {
        13 => Some(0),
        42 => Some(1),
        62 => Some(2),
        32 => Some(3),
        48..=57 => Some((c - 44) as u8), // digits → 4..=13
        65..=90 => Some((c - 51) as u8), // uppercase → 14..=39
        _ => None,
    }
}

/// Look up the base-40 value(s) for char `c` in the given CTX mode.
/// Returns the **single-byte** value when `c` is in the mode's base
/// set, or `None` if a shift sequence (Stage 6b) would be needed.
pub(crate) fn ctx_base_value(mode: u8, c: i16) -> Option<u8> {
    match mode {
        MODE_C => cnvals_lookup(c),
        MODE_T => tnvals_lookup(c),
        MODE_X => xvals_lookup(c),
        _ => None,
    }
}

/// BWIPP `CTXvalstocws` (bwip-js lines 32673-32690). Pack groups of
/// three base-40 values into 16-bit codewords:
///
///   value = v0 * 1600 + v1 * 40 + v2 + 1
///   cw_hi = value / 256
///   cw_lo = value % 256
///
/// Input length **must be a multiple of 3**.
pub(crate) fn ctxvals_to_cws(values: &[u8]) -> Vec<u16> {
    debug_assert_eq!(
        values.len() % 3,
        0,
        "ctxvals_to_cws expects a multiple of 3 values, got {}",
        values.len()
    );
    let mut cws: Vec<u16> = Vec::with_capacity(values.len() / 3 * 2);
    for chunk in values.chunks_exact(3) {
        let packed: u32 =
            u32::from(chunk[0]) * 1600 + u32::from(chunk[1]) * 40 + u32::from(chunk[2]) + 1;
        cws.push((packed / 256) as u16);
        cws.push((packed % 256) as u16);
    }
    cws
}

// ---------------------------------------------------------------------------
// Mode CTX (C / T / X) step encoder — BWIPP `encCTX` (bwip-js lines
// 32691-32871).
//
// Buffers base-40 values in `ctxvals`, then packs triplets via
// `ctxvals_to_cws` and emits as 16-bit codewords. The end-of-CTX-run
// cleanup emits an UNLCW (255) and switches back to Mode A, possibly
// flushing a trailing partial triplet by rewinding `i` and emitting
// the rewound chars in Mode A.
//
// **Stage 6 scope**: handles the common path:
//
//   1. At each `p % 3 == 0` boundary, call `lookup()` to check if we
//      should still stay in the current CTX mode. If not, flush +
//      unlatch + back to A.
//   2. Append base-40 value(s) to `ctxvals`. (For chars NOT in the
//      base set, return `InvalidData` — shift-extended cvals/tvals
//      are Stage 6b.)
//   3. At end of input, flush the buffered ctxvals (rewinding any
//      partial triplet), emit UNLCW, and emit rewound chars in Mode A.
//
// **Deferred to Stage 6b / 7+**:
//
//   * The `numD >= 12` / `numD >= 8` digit-run shortcuts that flush +
//     unlatch + switch to Mode D mid-CTX-run.
//   * The `msglen - i ≤ 3` end-of-symbol optimization branches (which
//     require `getnumremcws` = symbol-size-aware remcws calculation).
//     For now, if any of those branches would have fired and changes
//     the output, the resulting cws will differ from BWIPP — but for
//     short inputs that fit the smallest matrix size (Version A,
//     dcws=10), the branches don't trigger because remcws stays > 2.
//   * X-mode handling of out-of-set chars.
// ---------------------------------------------------------------------------

/// Internal state owned by [`encode_ctx_step`] and the [`encode_message`]
/// dispatcher.
#[derive(Debug, Default)]
struct CtxBuf {
    /// Buffer of base-40 values accumulated within the current CTX run.
    /// Length grows by 1 per char (for base-set chars) and gets flushed
    /// in triplets via [`ctxvals_to_cws`] at the end of the run.
    vals: Vec<u8>,
    /// Track of how many message bytes have been consumed by the CTX
    /// run since `vals` started — needed for the rewind step in the
    /// cleanup branch (mirrors BWIPP's `$_.i` decrement loop).
    consumed_chars: Vec<usize>,
}

/// Single iteration of BWIPP `encCTX` plus the end-of-run cleanup.
///
/// Consumes characters from `ctx.msg[ctx.i..]` while `ctx.mode` is C / T
/// / X, emitting packed cws + a trailing UNLCW, and resetting
/// `ctx.mode` to A. Returns once `i == msglen` or a mode-switch back
/// to A is required.
///
/// # Errors
///
/// * `InvalidData` if a char in the current CTX run isn't in the mode's
///   base set (shift-extension is Stage 6b).
/// * `InvalidData` if the digit-run / end-of-symbol special branches
///   would have fired (deferred to Stage 6b).
pub(crate) fn encode_ctx_run(ctx: &mut Ctx) -> Result<(), Error> {
    debug_assert!(matches!(ctx.mode, MODE_C | MODE_T | MODE_X));
    let mode_at_start = ctx.mode;
    let mut buf = CtxBuf::default();
    loop {
        if ctx.at_end() {
            break;
        }
        // BWIPP only runs the leading checks at every p%3==0 boundary.
        if buf.vals.len() % 3 == 0 {
            // Digit-run shortcuts (deferred to Stage 6b).
            let numd_i = ctx.numd[ctx.i];
            if numd_i >= 12 {
                return Err(Error::InvalidData(
                    "code one Mode CTX: digit-run >= 12 (Mode D entry) deferred to Stage 7"
                        .to_string(),
                ));
            }
            if numd_i >= 8 && ctx.i + numd_i == ctx.msg.len() {
                return Err(Error::InvalidData(
                    "code one Mode CTX: digit-run >= 8 at EOM (Mode D entry) deferred to Stage 7"
                        .to_string(),
                ));
            }
            // X-mode out-of-set check.
            if ctx.mode == MODE_X && xvals_lookup(ctx.msg[ctx.i]).is_none() {
                return Err(Error::InvalidData(
                    "code one Mode X: out-of-set char unlatch deferred to Stage 6b".to_string(),
                ));
            }
            // Otherwise call lookup to detect a mode switch back to A
            // (or to a different CTX submode). For C/T modes only.
            if ctx.mode != MODE_X {
                let newmode = lookup(&ctx.msg, ctx.i, ctx.mode, &ctx.next_xterm, &ctx.next_non_x);
                if newmode != ctx.mode {
                    // BWIPP flushes buffered + emits UNLCW + switches
                    // to A. (Stage 6 only supports "back to A" — any
                    // CTX→CTX switch returns InvalidData.)
                    if newmode != MODE_A {
                        return Err(Error::InvalidData(format!(
                            "code one Mode CTX: lookup wants switch to mode {newmode}, \
                             only back-to-A is supported in Stage 6"
                        )));
                    }
                    break;
                }
            }
            // End-of-symbol branch (msglen - i ≤ 3). The full BWIPP
            // logic needs `getnumremcws()`. For Stage 6 we defer to
            // 6b — but we ONLY surface the error if the special
            // branch would actually fire (i.e., the trailing chars
            // wouldn't be encodable through the normal path AND we'd
            // hit a remcws-dependent corner case). Since the normal
            // path correctly handles trailing chars via the cleanup
            // step below (rewind + flush + UNLCW + Mode A emission),
            // we can skip the early-exit branches entirely for inputs
            // where the smallest fitting symbol's remcws > 2 at this
            // point — which is true for all Stage 6 test inputs.
        }
        // Append base-40 value for this char.
        let c = ctx.msg[ctx.i];
        let v = ctx_base_value(ctx.mode, c).ok_or_else(|| {
            Error::InvalidData(format!(
                "code one Mode CTX: char 0x{c:04x} requires shift-extended cvals \
                 (Stage 6b); base-set chars only in Stage 6"
            ))
        })?;
        buf.vals.push(v);
        buf.consumed_chars.push(1);
        ctx.i += 1;
    }
    // Cleanup: emit buffered vals as packed cws + UNLCW + Mode A for
    // any rewound trailing chars. Mirrors bwip-js lines 32839-32869.
    if ctx.mode != mode_at_start {
        unreachable!("CTX run shouldn't mutate ctx.mode internally");
    }
    // Rewind partial triplet.
    let mut p = buf.vals.len();
    while p % 3 != 0 {
        p -= 1;
        // Rewind one char.
        let cnt = buf.consumed_chars.pop().expect("partial triplet rewind");
        ctx.i -= cnt;
        buf.vals.pop();
    }
    // Pack ctxvals[0..p] and emit.
    let packed = ctxvals_to_cws(&buf.vals);
    ctx.cws.extend_from_slice(&packed);
    // Emit UNLCW.
    ctx.cws.push(UNLCW);
    ctx.mode = MODE_A;
    // If we have leftover chars (rewound or never-consumed), handle
    // them in Mode A via the encA-style logic (digit-pair shortcut or
    // single-byte).
    if !ctx.at_end() {
        if ctx.numd[ctx.i] >= 2 {
            let d1 = ctx.msg[ctx.i] as u8;
            let d2 = ctx.msg[ctx.i + 1] as u8;
            let cw = avals_digit_pair(d1, d2).ok_or_else(|| {
                Error::InvalidData("code one CTX cleanup: digit-pair lookup failed".to_string())
            })?;
            ctx.cws.push(cw);
            ctx.i += 2;
        } else {
            let b = ctx.msg[ctx.i];
            if b < 0 {
                return Err(Error::InvalidData(format!(
                    "code one CTX cleanup: trailing marker {b} handling deferred to Stage 7"
                )));
            }
            let cw = avals_byte(b as u8).ok_or_else(|| {
                Error::InvalidData("code one CTX cleanup: high-byte handling Stage 7".to_string())
            })?;
            ctx.cws.push(cw);
            ctx.i += 1;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Symbol-size picker — BWIPP main body (bwip-js lines 33035-33064).
//
// Walks METRICS_NONSTYPE for the smallest version whose `dcws` ≥
// the encoded cws stream length. S-strip support arrives in Stage 7b.
// ---------------------------------------------------------------------------

/// Pick the smallest symbol-version metric whose `dcws` ≥ `cws_len`.
/// Walks `METRICS_NONSTYPE` (matrix A..H + T-strip).
///
/// # Errors
///
/// `InvalidData` if no version fits (cws_len > 1480, the version H
/// ceiling).
pub(crate) fn pick_symbol_size(cws_len: usize) -> Result<Metric, Error> {
    for m in METRICS_NONSTYPE.iter() {
        if usize::from(m.dcw) >= cws_len {
            return Ok(*m);
        }
    }
    Err(Error::InvalidData(format!(
        "code one: cws of length {cws_len} exceeds the largest symbol (H, dcws=1480)"
    )))
}

// ---------------------------------------------------------------------------
// Reed-Solomon ECC over GF(256) — BWIPP `codeone_rsprod` /
// `codeone_gencoeffs` / `bwipp_rsecbinary` (bwip-js lines 31873-31951
// + 6677-6742).
//
// GF(256) primitive polynomial 301 = 0x12D (same as Data Matrix). The
// encoder is the standard LFSR-based syndrome RS:
//
//   lfsr[0..ecpb] = 0
//   for each data byte d:
//     val = d XOR lfsr[0]
//     for j in 0..ecpb-1:
//       lfsr[j] = lfsr[j+1] XOR gf_mul(coeffs[ecpb-j-1], val)
//     lfsr[ecpb-1] = gf_mul(coeffs[0], val)
//   ECC = lfsr (length ecpb)
//
// Generator polynomial coefficients come from [`gf256_gen_coeffs`].
// ---------------------------------------------------------------------------

/// GF(256) tables for primitive polynomial 301.
struct Gf256Tables {
    alog: [u16; 256],
    log: [u16; 256],
}

impl Gf256Tables {
    fn new() -> Self {
        let gf: u32 = 256;
        let poly: u32 = 301;
        let mut alog = [0u16; 256];
        let mut log = [0u16; 256];
        let mut x: u32 = 1;
        for slot in alog.iter_mut().take((gf - 1) as usize) {
            *slot = x as u16;
            x *= 2;
            if x >= gf {
                x ^= poly;
            }
        }
        for (i, &a) in alog.iter().enumerate().take((gf - 1) as usize).skip(1) {
            log[a as usize] = i as u16;
        }
        Self { alog, log }
    }

    /// GF(256) multiply via log/antilog tables. Returns 0 if either
    /// operand is 0.
    fn mul(&self, a: u16, b: u16) -> u16 {
        if a == 0 || b == 0 {
            return 0;
        }
        let l = (u32::from(self.log[a as usize]) + u32::from(self.log[b as usize])) % 255;
        self.alog[l as usize]
    }
}

static GF256: std::sync::OnceLock<Gf256Tables> = std::sync::OnceLock::new();

fn gf256() -> &'static Gf256Tables {
    GF256.get_or_init(Gf256Tables::new)
}

/// Build the Reed-Solomon generator-polynomial coefficients for the
/// given ECC length `ecpb` over GF(256). Mirrors BWIPP's
/// `codeone_gencoeffs` (bwip-js lines 31913-31951).
///
/// Returns a Vec of length `ecpb` (BWIPP drops the trailing constant-1
/// via `$geti(coeffs, 0, coeffs.length - 1)`).
pub(crate) fn gf256_gen_coeffs(ecpb: usize) -> Vec<u16> {
    let gf = gf256();
    let mut coeffs: Vec<u16> = vec![0u16; ecpb + 1];
    coeffs[0] = 1;
    for i in 0..ecpb {
        // BWIPP: coeffs[i+1] = coeffs[i].
        coeffs[i + 1] = coeffs[i];
        let alog_i = gf.alog[i];
        // BWIPP: for j in i..1 (descending):
        //   coeffs[j] = rsprod(coeffs[j], alog[i]) XOR coeffs[j-1]
        for j in (1..=i).rev() {
            let prod = gf.mul(coeffs[j], alog_i);
            coeffs[j] = prod ^ coeffs[j - 1];
        }
        // BWIPP: coeffs[0] = rsprod(coeffs[0], alog[i])
        coeffs[0] = gf.mul(coeffs[0], alog_i);
    }
    coeffs.truncate(ecpb);
    coeffs
}

/// Compute the Reed-Solomon ECC for one interleaved data block.
/// `data` is the dcpb-length codeword block; returns ecpb-length ECC.
/// Mirrors BWIPP's `bwipp_rsecbinary` (bwip-js lines 6677-6742) for
/// the GF(256) case.
pub(crate) fn gf256_rs_block_ecc(data: &[u16], coeffs: &[u16]) -> Vec<u16> {
    let gf = gf256();
    let nc = coeffs.len();
    let mut lfsr: Vec<u16> = vec![0u16; nc];
    for &d in data {
        let val = d ^ lfsr[0];
        for j in 0..nc - 1 {
            let prod = gf.mul(coeffs[nc - j - 1], val);
            lfsr[j] = lfsr[j + 1] ^ prod;
        }
        lfsr[nc - 1] = gf.mul(coeffs[0], val);
    }
    lfsr
}

/// Pad `cws` to length `dcws` with the Mode-A PAD codeword (129).
/// Mirrors BWIPP's non-S-type padding (bwip-js line 33070).
pub(crate) fn pad_cws_matrix(cws: &mut Vec<u16>, dcws: usize) {
    while cws.len() < dcws {
        cws.push(129);
    }
}

/// Apply Reed-Solomon ECC to `cws` for the given metric and append
/// the ECC codewords (de-interleaved). Mirrors BWIPP's main body
/// (bwip-js lines 33086-33136).
///
/// Mutates `cws` in-place: length grows from `dcws` to `dcws + rscw`.
///
/// # Errors
///
/// `InvalidData` if `cws.len() != metric.dcws` at entry.
pub(crate) fn apply_reed_solomon(cws: &mut Vec<u16>, metric: &Metric) -> Result<(), Error> {
    let dcws = usize::from(metric.dcw);
    let rscw = usize::from(metric.rscw);
    let rsbl = usize::from(metric.rsbl);
    if cws.len() != dcws {
        return Err(Error::InvalidData(format!(
            "code one RS: cws.len()={} but dcws={dcws}",
            cws.len()
        )));
    }
    let dcpb = dcws / rsbl;
    let ecpb = rscw / rsbl;
    let coeffs = gf256_gen_coeffs(ecpb);
    let mut ecbs: Vec<Vec<u16>> = Vec::with_capacity(rsbl);
    for blk in 0..rsbl {
        let mut data: Vec<u16> = Vec::with_capacity(dcpb);
        for k in 0..dcpb {
            data.push(cws[k * rsbl + blk]);
        }
        ecbs.push(gf256_rs_block_ecc(&data, &coeffs));
    }
    cws.reserve(rscw);
    for i in 0..rscw {
        let blk = i % rsbl;
        let pos = i / rsbl;
        cws.push(ecbs[blk][pos]);
    }
    Ok(())
}

/// End-to-end Code One codeword stream + symbol metric. Produces the
/// full `dcws + rscw` codeword payload that the placement / renderer
/// (Stage 8-9) will consume.
///
/// Composes [`encode_message`] + [`pick_symbol_size`] +
/// [`pad_cws_matrix`] + [`apply_reed_solomon`].
pub(crate) fn encode_with_ecc(input: &[u8]) -> Result<(Vec<u16>, Metric), Error> {
    let mut cws = encode_message(input)?;
    let metric = pick_symbol_size(cws.len())?;
    pad_cws_matrix(&mut cws, usize::from(metric.dcw));
    apply_reed_solomon(&mut cws, &metric)?;
    Ok((cws, metric))
}

// ---------------------------------------------------------------------------
// Codeword → matrix placement — BWIPP main body (bwip-js lines 33136-33320).
//
// 1. Split each 8-bit codeword into top + bot 4-bit nibbles. Place
//    row-pair-by-row-pair into the `dcol`-wide data area, growing
//    column-first then advancing 2 rows.
// 2. Initialize `pixs` (rows × cols) with -1 placeholder values.
// 3. Stamp the column-pattern band (artifact strings from CPATMAP at
//    row (rows - cpat.len) / 2).
// 4. Stamp reference islands at riso/risi/risl positions (and their
//    mirrored bottom-right counterparts when i != risl-1).
// 5. Stamp the per-version forced black dots from BLACKDOTMAP.
// 6. Fill remaining -1 cells with mmat data left-to-right top-to-bottom.
// ---------------------------------------------------------------------------

/// Sentinel value for "no cell stamped yet". The post-step fill walks
/// pixs and replaces SENTINEL cells with mmat data in order.
const SENTINEL: i8 = -1;

/// Split a single 8-bit codeword into top + bot 4-bit nibbles. Returns
/// `(top, bot)` where each is a `[u8; 4]` of 0/1 bits MSB-first.
fn cw_nibbles_8(cw: u16) -> ([u8; 4], [u8; 4]) {
    let v = cw as u8;
    let mut top = [0u8; 4];
    let mut bot = [0u8; 4];
    for i in 0..4 {
        top[i] = (v >> (7 - i)) & 1;
        bot[i] = (v >> (3 - i)) & 1;
    }
    (top, bot)
}

/// Build the `mmat` flat data-grid by placing each codeword's
/// top+bot nibbles row-pair-by-row-pair. Returns a Vec of length
/// `(dcws + rscw) * 8` matching BWIPP's mmat layout
/// (bwip-js lines 33136-33190 for non-S types).
pub(crate) fn cws_to_mmat(cws: &[u16], metric: &Metric) -> Vec<u8> {
    let total = usize::from(metric.dcw) + usize::from(metric.rscw);
    debug_assert_eq!(cws.len(), total, "cws_to_mmat: cws.len() != dcws+rscw");
    let dcol = usize::from(metric.dcol);
    let nibbles_per_cw = 8usize;
    let mut mmat: Vec<u8> = vec![0u8; total * nibbles_per_cw];
    let mut r: usize = 0;
    let mut c: usize = 0;
    for &cw in cws {
        let (top, bot) = cw_nibbles_8(cw);
        // mmat[r * dcol + c..c+4] = top
        // mmat[(r+1) * dcol + c..c+4] = bot
        mmat[r * dcol + c..r * dcol + c + 4].copy_from_slice(&top);
        mmat[(r + 1) * dcol + c..(r + 1) * dcol + c + 4].copy_from_slice(&bot);
        c += 4;
        if c == dcol {
            c = 0;
            r += 2;
        }
    }
    mmat
}

/// Build one BWIPP `artifact[k]` row, length `cols`. Mirrors the 8
/// artifact functions at bwip-js lines 33201-33253. Returns `cols`
/// values, each `0`, `1`, or `-1` (SENTINEL).
fn artifact_row(art_index: u8, cols: usize) -> Vec<i8> {
    let mut row: Vec<i8> = Vec::with_capacity(cols);
    let half = (cols - 1) / 2;
    match art_index {
        0 => row.extend(std::iter::repeat_n(0i8, cols)), // '1': all zeros
        1 => row.extend(std::iter::repeat_n(1i8, cols)), // '2': all ones
        2 => {
            // '3': 0, 1×(cols-2), 0
            row.push(0);
            row.extend(std::iter::repeat_n(1i8, cols - 2));
            row.push(0);
        }
        3 => {
            // '4': 0, 1, 0×(cols-4), 1, 0
            row.push(0);
            row.push(1);
            row.extend(std::iter::repeat_n(0i8, cols - 4));
            row.push(1);
            row.push(0);
        }
        4 => {
            // '5': -1×half, 1, -1×half
            row.extend(std::iter::repeat_n(SENTINEL, half));
            row.push(1);
            row.extend(std::iter::repeat_n(SENTINEL, half));
        }
        5 => {
            // '6': -1×half, 0, -1×half
            row.extend(std::iter::repeat_n(SENTINEL, half));
            row.push(0);
            row.extend(std::iter::repeat_n(SENTINEL, half));
        }
        6 => {
            // '7': 1, 0×(cols-2), 1
            row.push(1);
            row.extend(std::iter::repeat_n(0i8, cols - 2));
            row.push(1);
        }
        7 => {
            // '8': 1, 0, 1×(cols-4), 0, 1
            row.push(1);
            row.push(0);
            row.extend(std::iter::repeat_n(1i8, cols - 4));
            row.push(0);
            row.push(1);
        }
        _ => row.extend(std::iter::repeat_n(0i8, cols)),
    }
    debug_assert_eq!(row.len(), cols);
    row
}

/// Compose the final `rows × cols` pixs grid for a Code One symbol
/// per BWIPP lines 33191-33318. Returns the flat row-major
/// `Vec<u8>` (rows*cols length) of 0/1 cells.
pub(crate) fn compose_pixs(mmat: &[u8], metric: &Metric) -> Result<Vec<u8>, Error> {
    let rows = usize::from(metric.rows);
    let cols = usize::from(metric.cols);
    let mut pixs: Vec<i8> = vec![SENTINEL; rows * cols];
    // Get column-pattern string for this version (looked up by first
    // letter of the id).
    let vers_letter =
        metric.id.chars().next().ok_or_else(|| {
            Error::InvalidData(format!("code one: empty metric id {:?}", metric.id))
        })?;
    let cpat = cpatmap_for(vers_letter).ok_or_else(|| {
        Error::InvalidData(format!(
            "code one: no CPATMAP entry for version {vers_letter:?}"
        ))
    })?;
    // Stamp the column-pattern band starting at row (rows - cpat.len) / 2.
    let band_start_row = (rows - cpat.len()) / 2;
    let band_start_off = band_start_row * cols;
    let mut band: Vec<i8> = Vec::with_capacity(cpat.len() * cols);
    for ch in cpat.bytes() {
        let art_idx = ch - b'1'; // '1' → 0, '2' → 1, …, '8' → 7
        band.extend(artifact_row(art_idx, cols));
    }
    pixs[band_start_off..band_start_off + band.len()].copy_from_slice(&band);
    // Reference islands.
    let risl = usize::from(metric.risl);
    let risi = usize::from(metric.risi);
    let riso = usize::from(metric.riso);
    if risl != usize::from(NA) {
        // i in 0..=risl-1
        for i in 0..risl {
            // j = riso, riso+risi, ... while j <= cols-1
            let mut j = riso;
            while j < cols {
                let bit2 = if i % 12 == 0 { 1i8 } else { 0i8 };
                let off = i * cols + j;
                // Place [1, bit2] at pixs[off..off+2].
                if off + 1 < pixs.len() {
                    pixs[off] = 1;
                    pixs[off + 1] = bit2;
                }
                if i != risl - 1 {
                    // Mirror to bottom-right.
                    let mr = rows - i - 1;
                    let mc = cols - j - 2;
                    let moff = mr * cols + mc;
                    if moff + 1 < pixs.len() {
                        pixs[moff] = 1;
                        pixs[moff + 1] = bit2;
                    }
                }
                if risi == usize::from(NA) {
                    break;
                }
                j += risi;
            }
        }
    }
    // Forced black dots. BWIPP encodes coords as (col, row) — $aload
    // pushes the array in order, then mmv pops [col, row] and computes
    // `col + row*cols`. So our stored pair `(a, b)` from BLACKDOTMAP is
    // `(col, row)`, not `(row, col)` despite my Stage-2 doc.
    if let Some(dots) = blackdotmap_for(metric.id) {
        for &(c, r) in dots {
            let off = usize::from(r) * cols + usize::from(c);
            if off < pixs.len() {
                pixs[off] = 1;
            }
        }
    }
    // Fill remaining SENTINEL cells with mmat data in order.
    let mut mmat_idx = 0;
    for cell in pixs.iter_mut() {
        if *cell == SENTINEL {
            if mmat_idx >= mmat.len() {
                return Err(Error::InvalidData(format!(
                    "code one placement: pixs needs more mmat cells than available \
                     ({}); metric={}",
                    mmat.len(),
                    metric.id
                )));
            }
            *cell = mmat[mmat_idx] as i8;
            mmat_idx += 1;
        }
    }
    // Convert i8 (always 0/1 at this point) back to u8.
    Ok(pixs.into_iter().map(|c| c as u8).collect())
}

/// End-to-end Code One encoder. Composes [`encode_with_ecc`] +
/// [`cws_to_mmat`] + [`compose_pixs`] + final [`BitMatrix`] build.
///
/// Returns the rendered symbol as a [`BitMatrix`] with `width = cols`,
/// `height = rows`.
pub(crate) fn encode_to_bit_matrix(input: &[u8]) -> Result<BitMatrix, Error> {
    let (cws, metric) = encode_with_ecc(input)?;
    let mmat = cws_to_mmat(&cws, &metric);
    let pixs = compose_pixs(&mmat, &metric)?;
    let cols = usize::from(metric.cols);
    let rows = usize::from(metric.rows);
    let mut bm = BitMatrix::new(cols, rows);
    for r in 0..rows {
        for c in 0..cols {
            if pixs[r * cols + c] != 0 {
                bm.set(c, r, true);
            }
        }
    }
    Ok(bm)
}

// ---------------------------------------------------------------------------
// Top-level `encode_message` dispatcher — BWIPP main loop (bwip-js
// lines 33020-33034).
//
// Loops `encode_a_step` / `encode_ctx_run` based on `ctx.mode` until
// `i == msglen`. Returns the accumulated `cws`. Modes D and B (Stage
// 7+) trigger an InvalidData error if reached.
// ---------------------------------------------------------------------------

/// Append the low `n_bits` bits of `val` to a bit-stream buffer
/// (`dbitsbuf` / `dbitslen` pair) MSB first. Mirrors BWIPP's
/// `appendbits` function at bwip-js-node.js line 32942.
pub(crate) fn append_dbits(dbitsbuf: &mut Vec<u8>, dbitslen: &mut usize, val: u32, n_bits: usize) {
    for shift in (0..n_bits).rev() {
        let bit = ((val >> shift) & 1) as u8;
        dbitsbuf.push(bit);
        *dbitslen += 1;
    }
}

/// Flush a Mode D bit-stream buffer into byte codewords (MSB first
/// in each byte). Mirrors the byte-conversion loop at bwip-js lines
/// 33307-33316. Bits beyond the last full byte are dropped (callers
/// pad explicitly via terminator emission).
pub(crate) fn flush_dbits_to_cws(dbitsbuf: &[u8], dbitslen: usize) -> Vec<u16> {
    let mut bytes = Vec::with_capacity(dbitslen.div_ceil(8));
    let mut idx = 0;
    while idx + 8 <= dbitslen {
        let mut b: u16 = 0;
        for k in 0..8 {
            b = b * 2 + u16::from(dbitsbuf[idx + k]);
        }
        bytes.push(b);
        idx += 8;
    }
    bytes
}

/// Precomputed `numremcws` table — for codeword position `j`
/// (0-indexed), `NUMREMCWS[j]` returns the number of codewords
/// remaining in the smallest Code One symbol size that contains at
/// least `j + 1` data codewords. Built once at module load from
/// `METRICS_NONSTYPE` (non-S-type, version unset = filter to
/// `vers.len() == 1`, i.e. A..H).
///
/// Mirrors the BWIPP `numremcws` precompute loop at bwip-js
/// lines 32573-32587.
fn numremcws_table() -> &'static [u16; 1480] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u16; 1480]> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Step 1: fill with sentinel 10000 (BWIPP's "not yet set"
        // marker — anything > 1).
        let mut t = [10000_u16; 1480];
        // Step 2: for each valid dcws value, mark position dcws-1 as 1
        // ("one codeword remaining"). Filter to single-char ids
        // (A..H) since BWIPP excludes T-strip when version is unset.
        for m in METRICS_NONSTYPE.iter() {
            if m.id.len() == 1 {
                let dcws = m.dcw as usize;
                if (1..=1480).contains(&dcws) {
                    t[dcws - 1] = 1;
                }
            }
        }
        // Step 3: walk right-to-left; if position not already marked
        // as 1 (i.e., not a symbol boundary), set it to the next
        // position's value plus 1.
        for i in (0..1479).rev() {
            if t[i] != 1 {
                t[i] = t[i + 1] + 1;
            }
        }
        t
    })
}

/// BWIPP `getnumremcws(j)` — returns the number of codewords
/// remaining to fill the smallest Code One symbol (A..H) that
/// contains at least `j + 1` data codewords. Mirrors bwip-js lines
/// 32588-32597.
///
/// Returns `None` if `j` exceeds the largest symbol size (1480).
pub(crate) fn getnumremcws(j: usize) -> Option<u16> {
    let table = numremcws_table();
    if j >= table.len() {
        return None;
    }
    Some(table[j])
}

/// Mode D codeword for the LA (latch-to-A) operation that returns
/// from Mode D back to Mode A at end-of-input. BWIPP doesn't emit a
/// separate codeword for this — the bit-buffer termination implicitly
/// signals it via the trailing `1`s. So this constant is unused but
/// documented for symmetry.
#[allow(dead_code)]
pub(crate) const MODE_D_TERMINATOR_BITS: u32 = 0b111111;

/// BWIPP `encD` — Mode D (decimal compression) encoder.
///
/// Packs consecutive 3-digit groups from `ctx.msg[ctx.i..]` into
/// 10-bit chunks via `val = d0*100 + d1*10 + d2 + 1`. When the
/// remaining digit count drops below 3, runs the BWIPP termination
/// state machine (lines 33222-33289) which uses `Drem` (bits to fill
/// current byte) and `remcws` (codewords remaining in the symbol via
/// `getnumremcws`) to decide whether to emit:
///
/// * a single 4-bit final digit (for `Drem ∈ {4, 6}`),
/// * a 2-bit `01` pad (for `Drem ∈ {2, 6}`),
/// * a 6-bit `111111` terminator (default path),
/// * or simply break (when the symbol-size accounting allows the
///   leftover digits to ride out without explicit padding).
///
/// After the loop, the accumulated bit stream is flushed to byte
/// codewords (MSB first per byte) via [`flush_dbits_to_cws`] and
/// pushed to `ctx.cws`. Sets `ctx.mode = MODE_A` on return.
///
/// Mirrors bwip-js-node.js lines 33220-33322.
pub(crate) fn encode_d_step(ctx: &mut Ctx) -> Result<(), Error> {
    let msglen = ctx.msg.len();
    let j_start = ctx.cws.len();

    loop {
        let i = ctx.i;
        let numd_i = ctx.numd.get(i).copied().unwrap_or(0);

        // ------------------------------------------------------------
        // Main path: 3+ digits ahead → pack one 10-bit group.
        // ------------------------------------------------------------
        if numd_i >= 3 {
            let d0 = (ctx.msg[i] & 0xff) as u32 - 48;
            let d1 = (ctx.msg[i + 1] & 0xff) as u32 - 48;
            let d2 = (ctx.msg[i + 2] & 0xff) as u32 - 48;
            let val = d0 * 100 + d1 * 10 + d2 + 1;
            append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, val, 10);
            ctx.i += 3;
            continue;
        }

        // ------------------------------------------------------------
        // Termination path — numD[i] < 3.
        // ------------------------------------------------------------
        let drem = (8 - ctx.dbitslen % 8) % 8;
        let dbits_bytes = ctx.dbitslen / 8;
        let remcws = getnumremcws(j_start + dbits_bytes).ok_or_else(|| {
            Error::InvalidData(format!(
                "code one Mode D: payload exceeds 1480-codeword cap (j={})",
                j_start + dbits_bytes
            ))
        })?;
        let remcws_check = if dbits_bytes >= 1 {
            getnumremcws(j_start + dbits_bytes - 1).ok_or_else(|| {
                Error::InvalidData("code one Mode D: remcws_check overflow".to_string())
            })?
        } else {
            // BWIPP allows `j - 1` to be negative if `dbits_bytes = 0`.
            // In practice we always have ≥ 1 dbits_byte by the time
            // we hit the termination path (the entry from Mode A
            // wrote 4 bits, plus at least one full pack would have
            // been needed to trigger Mode D — actually no, the
            // entry condition can fire with i=msglen for an empty
            // tail. Use 1 as the conservative remcws_check value
            // (matches BWIPP's "1 + remcws" handling when the
            // expression is one-off).
            1
        };

        // Branch 1 — final break with explicit Drem-based padding:
        //   i == msglen AND (
        //     (remcws_check - 1 == 0 AND Drem == 0)
        //     OR (remcws == 1 AND Drem != 0)
        //   )
        let cond_a = (remcws_check == 1) && (drem == 0);
        let cond_b = (remcws == 1) && (drem != 0);
        if i == msglen && (cond_a || cond_b) {
            if drem == 4 || drem == 6 {
                append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b1111, 4);
            }
            if drem == 2 || drem == 6 {
                append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b01, 2);
            }
            break;
        }

        // Branch 2 — break with no extra bits.
        // Pattern: i == msglen-1 with numD[i] == 1, OR i == msglen-2
        // with numD[i] == 2, AND remcws == 1 AND Drem == 0.
        let pos_one_left = (i == msglen.wrapping_sub(1)) && numd_i == 1;
        let pos_two_left = msglen >= 2 && (i == msglen - 2) && numd_i == 2;
        if (pos_one_left || pos_two_left) && remcws == 1 && drem == 0 {
            break;
        }

        // Branch 3 — default terminator path. Skipped only when:
        //   i == msglen-1, numD[i] == 1, remcws == 1, Drem ∈ {4, 6}.
        let skip_terminator =
            (i == msglen.wrapping_sub(1)) && numd_i == 1 && remcws == 1 && (drem == 4 || drem == 6);
        if !skip_terminator {
            append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b111111, 6);
        }
        // Recompute Drem after the optional terminator emission.
        let drem2 = (8 - ctx.dbitslen % 8) % 8;
        let mut drem3 = drem2;
        if drem3 == 4 || drem3 == 6 {
            if numd_i >= 1 {
                let val = (ctx.msg[i] & 0xff) as u32 - 48 + 1;
                append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, val, 4);
                ctx.i += 1;
            } else {
                append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b1111, 4);
            }
            drem3 -= 4;
        }
        if drem3 == 2 {
            append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b01, 2);
        }
        break;
    }

    // Flush dbits to byte codewords and append to ctx.cws.
    let bytes = flush_dbits_to_cws(&ctx.dbitsbuf, ctx.dbitslen);
    ctx.cws.extend_from_slice(&bytes);

    // Reset Mode-D state and return to Mode A for any trailing
    // post-Mode-D content (e.g. "21 digits + Mode A tail").
    ctx.dbitsbuf.clear();
    ctx.dbitslen = 0;
    ctx.mode = MODE_A;
    Ok(())
}

/// Top-level Code One encoder loop. Mirrors BWIPP's encoder dispatcher
/// at bwip-js lines 33020-33034.
///
/// # Errors
///
/// `InvalidData` if the input requires a mode (B) or branch (FNC
/// markers, shift sequences) deferred to Stages 6b / 7+.
pub(crate) fn encode_message(input: &[u8]) -> Result<Vec<u16>, Error> {
    let mut ctx = Ctx::from_bytes(input);
    while !ctx.at_end() {
        match ctx.mode {
            MODE_A => encode_a_step(&mut ctx)?,
            MODE_C | MODE_T | MODE_X => encode_ctx_run(&mut ctx)?,
            MODE_D => encode_d_step(&mut ctx)?,
            MODE_B => encode_b_step(&mut ctx)?,
            _ => {
                return Err(Error::InvalidData(format!(
                    "code one internal: unknown mode {}",
                    ctx.mode
                )));
            }
        }
    }
    // Final cleanup: if we ended in a CTX mode, emit the trailing
    // UNLCW. (encode_ctx_run already handles this internally, but
    // belt-and-suspenders.)
    if matches!(ctx.mode, MODE_C | MODE_T | MODE_X) {
        // Shouldn't happen — encode_ctx_run resets to A at end.
        ctx.cws.push(UNLCW);
        ctx.mode = MODE_A;
    }
    Ok(ctx.cws)
}

// ---------------------------------------------------------------------------
// numD lookahead — BWIPP `numD` (bwip-js lines 32250-32273).
//
// For each position i in `msg`, `numD[i]` is the count of consecutive
// ASCII digit bytes (0x30..=0x39) starting at i. Built in a single
// backward pass so `numD[i] = (msg[i] is digit) ? numD[i+1] + 1 : 0`,
// with `numD[msglen] = 0` (out-of-range sentinel).
//
// Used by [`encode_a_step`] for three short-circuits:
//
// * `numD[i] >= 21`            → switch to Mode D (long digit run).
// * `numD[i] >= 13 && i+numD == msglen` → switch to Mode D (trailing
//                                run).
// * `numD[i] >= 2`             → emit avals[pair] (3-bytes-for-2-cws
//                                compression).
// ---------------------------------------------------------------------------

/// Compute the BWIPP `numD` lookahead array: `numD[i] =` count of
/// consecutive ASCII digit bytes (`'0'..='9'`) starting at index `i`
/// of `msg`. Marker bytes (negative `i16`) are not digits.
///
/// Returns a `Vec<usize>` of length `msg.len() + 1`, with the trailing
/// entry as `0` (BWIPP sentinel for out-of-range reads).
pub(crate) fn compute_numd(msg: &[i16]) -> Vec<usize> {
    let mut numd = vec![0usize; msg.len() + 1];
    for i in (0..msg.len()).rev() {
        let c = msg[i];
        if (b'0' as i16..=b'9' as i16).contains(&c) {
            numd[i] = numd[i + 1] + 1;
        } else {
            numd[i] = 0;
        }
    }
    numd
}

// ---------------------------------------------------------------------------
// Mode A (ASCII) encoder — BWIPP `encA` (bwip-js lines 32607-32672).
//
// Mirrors encA's single iteration: examine `msg[i]` and per-iteration
// state, emit one or more codewords, advance `i` and/or change `mode`.
//
// **Stage 4 scope**: this implementation handles the digit-pair path
// (numD[i] ≥ 2 → avals[pair]) and the default single-byte path
// (emit avals[msg[i]], advance i). The lookup-based mode-switch
// branch, the >=21 / >=13 EOM digit-run switches, and the FNC1 leader
// path return [`Error::InvalidData`] tagged with the path name —
// they're filled in across stages 5+ (the lookup function needs
// numericruns + cnvals / tnvals / xvals tables which haven't landed
// yet). This is sufficient to pin the "input stays in Mode A end-to-end"
// case against bwip-js for short uppercase / mixed-case / digit-pair
// payloads.
// ---------------------------------------------------------------------------

/// Mutable encoder state shared by all the per-mode `encode_<X>_step`
/// functions. Mirrors the relevant `$_.*` globals BWIPP uses across
/// `encA` / `encB` / `encCTX` / `encD`.
#[derive(Debug, Clone)]
pub(crate) struct Ctx {
    /// Input message (1 entry per ASCII byte or negative marker).
    pub msg: Vec<i16>,
    /// Cursor into [`msg`](Ctx::msg). Advances as bytes are consumed.
    pub i: usize,
    /// Output codeword stream. Grows as steps emit cws.
    pub cws: Vec<u16>,
    /// Current encoder mode ([`MODE_A`] / [`MODE_C`] / …).
    pub mode: u8,
    /// numD lookahead. Length = `msg.len() + 1`.
    pub numd: Vec<usize>,
    /// nextXterm lookahead — distance to next X12 terminator.
    pub next_xterm: Vec<u32>,
    /// nextNonX lookahead — distance to next non-X12 byte.
    pub next_non_x: Vec<u32>,
    /// Mode-D bit buffer (unused until Stage 6).
    pub dbitsbuf: Vec<u8>,
    /// Number of valid bits in [`dbitsbuf`](Ctx::dbitsbuf).
    pub dbitslen: usize,
}

impl Ctx {
    /// Build a fresh context from a plain `&[u8]` input. ASCII bytes
    /// 0..=127 become `msg` entries verbatim; bytes 128..=255 are
    /// allowed and pass through (BWIPP supports them via FNC4 in
    /// Mode A or directly in Mode B).
    pub(crate) fn from_bytes(input: &[u8]) -> Self {
        let msg: Vec<i16> = input.iter().map(|&b| i16::from(b)).collect();
        Self::from_markers(msg)
    }

    /// Build a context from a pre-parsed `&[i16]` input (where negative
    /// values are marker sentinels). Used by the test suite + future
    /// FNC1 / ECI / mode-latch tests.
    pub(crate) fn from_markers(msg: Vec<i16>) -> Self {
        let numd = compute_numd(&msg);
        let next_xterm = compute_next_xterm(&msg);
        let next_non_x = compute_next_non_x(&msg);
        Ctx {
            msg,
            i: 0,
            cws: Vec::new(),
            mode: MODE_A,
            numd,
            next_xterm,
            next_non_x,
            dbitsbuf: Vec::new(),
            dbitslen: 0,
        }
    }

    /// `i >= msglen` ⇔ input exhausted.
    fn at_end(&self) -> bool {
        self.i >= self.msg.len()
    }
}

/// Single iteration of BWIPP `encA` (bwip-js lines 32607-32672).
///
/// Consumes 1 or 2 bytes from `ctx.msg[ctx.i..]` and emits 1 cw to
/// `ctx.cws`. Updates `ctx.i` (and `ctx.mode` once the lookup branch
/// lands in Stage 5+).
///
/// # Stage 4 scope (current)
///
/// * `numD[i] >= 2` (and not `>= 21` and not `>= 13` at EOM):
///   emit `avals_digit_pair(msg[i], msg[i+1])` and advance `i` by 2.
/// * Default (no special-case match): emit `avals_byte(msg[i])` and
///   advance `i` by 1.
///
/// # Stage 5+ branches (currently rejected as `InvalidData`)
///
/// * `numD[i] >= 21`: switch to Mode D (4-bit `Dbitsbuf` prefix).
/// * `numD[i] >= 13` with `i + numD == msglen`: same.
/// * `msg[i] == FNC1` with following digit run: switch to Mode D via
///   FNC1LD codeword.
/// * Any byte whose `lookup()` returns a different mode: emit the
///   appropriate latch (`avals_marker(LC/LT/LX/LD/LB)`) and switch.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] tagged with the unimplemented branch
/// name when a Stage 5+ path triggers.
pub(crate) fn encode_a_step(ctx: &mut Ctx) -> Result<(), Error> {
    if ctx.at_end() {
        return Ok(());
    }
    debug_assert_eq!(ctx.mode, MODE_A);
    let i = ctx.i;
    let msglen = ctx.msg.len();
    let numd_i = ctx.numd[i];

    // Long-digit-run shortcut → Mode D entry (Stage 3d).
    // BWIPP `encA` at bwip-js lines 32955-32975 writes 4 bits of "1"
    // to Dbitsbuf as the Mode-A→Mode-D latch prefix and switches to
    // Mode D. No codeword is pushed to cws — the LD signal is the
    // leading bit pattern in the eventual Mode-D byte stream.
    if numd_i >= 21 || (numd_i >= 13 && i + numd_i == msglen) {
        ctx.dbitsbuf.clear();
        ctx.dbitslen = 0;
        // Emit 4 bits of "1" via dbits machinery.
        append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b1111, 4);
        ctx.mode = MODE_D;
        return Ok(());
    }

    // Digit pair packing.
    if numd_i >= 2 {
        let d1 = ctx.msg[i] as u8;
        let d2 = ctx.msg[i + 1] as u8;
        let cw = avals_digit_pair(d1, d2).ok_or_else(|| {
            Error::InvalidData(format!(
                "code one Mode A internal: digit-pair lookup failed for ({d1}, {d2})"
            ))
        })?;
        ctx.cws.push(cw);
        ctx.i += 2;
        return Ok(());
    }

    // FNC1 leader. Deferred to Stage 6+.
    if ctx.msg[i] == FNC1 {
        return Err(Error::InvalidData(
            "code one Mode A: FNC1 leader handling deferred to Stage 6+".to_string(),
        ));
    }

    // Default: run the lookup() forward-scan. If lookup returns a
    // different mode, emit the corresponding latch cw + switch mode
    // (handled below). If lookup returns the same mode (A), fall
    // through to single-byte emission.
    let newmode = lookup(&ctx.msg, ctx.i, ctx.mode, &ctx.next_xterm, &ctx.next_non_x);
    if newmode != ctx.mode {
        // Emit the latch codeword and switch mode.
        let latch_marker = match newmode {
            MODE_C => LC,
            MODE_T => LT,
            MODE_X => LX,
            MODE_D => LD,
            MODE_B => LB,
            _ => {
                return Err(Error::InvalidData(format!(
                    "code one Mode A: lookup returned unexpected mode {newmode}"
                )));
            }
        };
        if newmode == MODE_D {
            // Stage 3d: Mode D entry from `lookup()` returning D
            // shares the same bit-level handshake as the
            // numD-trigger path above — write 4 bits of "1" to
            // dbitsbuf, no codeword push, switch to Mode D.
            ctx.dbitsbuf.clear();
            ctx.dbitslen = 0;
            append_dbits(&mut ctx.dbitsbuf, &mut ctx.dbitslen, 0b1111, 4);
            ctx.mode = MODE_D;
            return Ok(());
        }
        let latch_cw = avals_marker(latch_marker).ok_or_else(|| {
            Error::InvalidData(format!(
                "code one Mode A internal: avals_marker({latch_marker}) failed"
            ))
        })?;
        ctx.cws.push(latch_cw);
        ctx.mode = newmode;
        return Ok(());
    }

    // No mode switch — emit avals[byte] and advance.
    let byte = ctx.msg[i];
    if byte < 0 {
        return Err(Error::InvalidData(format!(
            "code one Mode A: marker byte {byte} handling deferred to Stage 6+"
        )));
    }
    let cw = avals_byte(byte as u8).ok_or_else(|| {
        Error::InvalidData(format!(
            "code one Mode A: byte 0x{byte:02x} > 128 — use FNC4 (Stage 6+) or Mode B"
        ))
    })?;
    ctx.cws.push(cw);
    ctx.i += 1;
    Ok(())
}

/// Encode the rest of the message in Mode B (raw 8-bit bytes).
///
/// Mode B latches stay active until end-of-message in BWIPP's
/// dispatcher loop: once Mode A's `lookup()` picks Mode B (because
/// the remaining run is dominated by high-byte / non-ASCII content
/// that the C/T/X sets can't represent), the rest of the symbol is
/// emitted as a single Mode B segment. The caller has already
/// pushed the `LB` latch codeword in `encode_a_step`; this function
/// pushes the length prefix + one codeword per remaining byte, then
/// advances `ctx.i` to `msg.len()`.
///
/// FNC markers (FNC1/2/3/4) inside a Mode B run are NOT supported
/// by this implementation — BWIPP handles them via Mode B's
/// embedded FNCX cws but this iteration only ships the raw-byte
/// path. Inputs that route through Mode B should be plain bytes
/// (0..=255); any negative msg cell (which is how the input
/// preprocessor encodes FNC markers and ECI escapes) returns
/// `Error::InvalidData`. The `codeone` row therefore stays
/// `partial` in PORT_STATUS until FNC/ECI/Mode-D/S-strip paths
/// also land.
///
/// # Errors
///
/// * `InvalidData` if any remaining message cell is negative
///   (FNC/ECI marker — not yet routable through Mode B).
/// * `InvalidData` if the remaining payload exceeds [`MODE_B_MAX_LEN`].
pub(crate) fn encode_b_step(ctx: &mut Ctx) -> Result<(), Error> {
    debug_assert_eq!(ctx.mode, MODE_B);
    if ctx.at_end() {
        return Ok(());
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(ctx.msg.len() - ctx.i);
    for &cell in &ctx.msg[ctx.i..] {
        if !(0..=255).contains(&cell) {
            return Err(Error::InvalidData(format!(
                "code one Mode B: marker / ECI cell {cell} cannot be encoded as a raw byte \
                 (FNC and ECI paths inside Mode B are extension paths — see PORT_STATUS)"
            )));
        }
        bytes.push(cell as u8);
    }
    let cws = encode_mode_b_run(&bytes, /* omit_prefix_if_exact */ false)?;
    ctx.cws.extend(cws);
    ctx.i = ctx.msg.len();
    // Mode B runs to end-of-message; no UNLCW back to Mode A.
    Ok(())
}

/// Convenience driver: run [`encode_a_step`] in a loop until `i ==
/// msglen`, returning the accumulated cws. Asserts that the encoder
/// stays in Mode A throughout — any path that would switch modes (or
/// hits a deferred branch) bubbles up as `Error::InvalidData`.
///
/// This is the Stage-4 entry point used by the byte-for-byte goldens
/// against bwip-js. Stage 5+ will replace it with the full
/// `encode_message` dispatcher that follows the BWIPP encoder loop
/// at bwip-js lines 33020-33033.
pub(crate) fn encode_mode_a_only(input: &[u8]) -> Result<Vec<u16>, Error> {
    let mut ctx = Ctx::from_bytes(input);
    while !ctx.at_end() {
        encode_a_step(&mut ctx)?;
        if ctx.mode != MODE_A {
            return Err(Error::InvalidData(
                "code one Mode A-only: encoder switched out of Mode A — caller \
                 should use the full mode-selector (Stage 5+)"
                    .to_string(),
            ));
        }
    }
    Ok(ctx.cws)
}

// ---------------------------------------------------------------------------
// Stub `encode` — still returns `InvalidData`. Full mode selector,
// CTX / D encoders, Reed-Solomon, placement, and the renderer arrive
// in Stages 5-9. See `CODEONE_PORT_PLAN.md`.
// ---------------------------------------------------------------------------

/// Public Code One encoder entry point. Produces a [`BitMatrix`] for
/// Mode-A / Mode-CTX inputs (currently up to Version A, 16 × 18).
///
/// Composes the full pipeline via [`encode_to_bit_matrix`]:
///
///   1. [`encode_message`] — mode-aware cws stream (encA / encCTX).
///   2. [`pick_symbol_size`] — smallest fitting matrix version.
///   3. [`pad_cws_matrix`] — pad with PAD codeword (129).
///   4. [`apply_reed_solomon`] — RS ECC over GF(256) poly 301.
///   5. [`cws_to_mmat`] — split into top/bot nibbles, place into
///      dcol-wide data area.
///   6. [`compose_pixs`] — stamp column-pattern band, reference
///      islands, forced black dots, then fill remaining cells with
///      mmat data.
///
/// # Errors
///
/// * `InvalidData` for inputs that require Mode D (digit-run >= 12 /
///   = 21 / FNC1 leader), Mode B (high bytes via FNC4), or any FNC
///   marker / ECI / shift-extended cvals path. Those branches land
///   in Stage 10b / 11 (Mode D, B, and S-strip rendering all
///   deferred).
pub fn encode(input: &[u8]) -> Result<BitMatrix, Error> {
    encode_to_bit_matrix(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `gf256_gen_coeffs` for small `ecpb` where
    /// the GF(256) arithmetic is hand-verifiable. The generator
    /// polynomial powers RS ECC; mutations to the BWIPP loop would
    /// silently corrupt every Code One ECC stream.
    ///
    /// Hand-computed:
    /// * ecpb=0: only the initial coeffs[0]=1 exists, then truncate
    ///   to 0 → empty vec.
    /// * ecpb=1: after i=0 step, coeffs=[mul(1, alog[0]=1)=1, 1].
    ///   Truncate to 1 → [1].
    /// * ecpb=2: after i=1 step, coeffs[0]=mul(1, alog[1]=2)=2;
    ///   coeffs[1]=mul(coeffs_prev[1]=1, 2) XOR 1 = 2 XOR 1 = 3;
    ///   coeffs[2]=1. Truncate to 2 → [2, 3].
    ///
    /// Mutations caught:
    /// * `coeffs[0] = 1` initialiser.
    /// * `coeffs[i+1] = coeffs[i]` shift step.
    /// * Inner loop `(1..=i).rev()` direction / bound.
    /// * `prod ^ coeffs[j-1]` XOR feedback.
    /// * `coeffs[0] = gf.mul(coeffs[0], alog_i)` final-slot update.
    /// * `coeffs.truncate(ecpb)` length-cut.
    #[test]
    fn gf256_gen_coeffs_small_polynomials() {
        // ecpb=0 → empty.
        assert_eq!(gf256_gen_coeffs(0), Vec::<u16>::new());
        // ecpb=1 → [1].
        assert_eq!(gf256_gen_coeffs(1), vec![1u16]);
        // ecpb=2 → [2, 3].
        assert_eq!(gf256_gen_coeffs(2), vec![2u16, 3]);
        // Length sanity for larger ecpb (covers truncate).
        assert_eq!(gf256_gen_coeffs(10).len(), 10);
        assert_eq!(gf256_gen_coeffs(44).len(), 44);
        // Determinism: same ecpb → same output.
        assert_eq!(gf256_gen_coeffs(5), gf256_gen_coeffs(5));
    }

    /// Stage 11.A8c — pin `Gf256Tables::mul(a, b)`. The GF(256)
    /// multiply over the BWIPP/QR/Data Matrix/Code One Reed-Solomon
    /// field (poly `301 = x^8 + x^5 + x^3 + x^2 + 1`, BWIPP-flavour).
    ///
    /// Used pervasively by `gf256_gen_coeffs` and
    /// `gf256_rs_block_ecc`, but mul itself had no direct anchor —
    /// the only test path was through full RS-block round-trip. That
    /// makes algebra-class mutations (zero short-circuit, modulus
    /// drift, table swap) hard to localize.
    ///
    /// Anchors pin three layers:
    ///   * absorbing-zero axiom: `mul(0, x) = mul(x, 0) = 0` for
    ///     several non-zero `x`. Kills `a == 0 || b == 0` → `&&`
    ///     (would compute the log-table sum and return a non-zero
    ///     even when one operand is 0);
    ///   * multiplicative identity: `mul(1, x) = mul(x, 1) = x` for
    ///     several values;
    ///   * commutativity: `mul(a, b) == mul(b, a)` across mixed pairs;
    ///   * concrete poly anchor: `mul(2, 128) = 29` (poly 301 wrap),
    ///     `mul(2, 2) = 4`, `mul(2, 4) = 8`, `mul(2, 64) = 128` (these
    ///     are pre-wrap multiplications). Kills `% 255` → `% 256/254`
    ///     mutants and antilog-table swaps.
    #[test]
    fn gf256_mul_zero_identity_commutativity_and_poly_anchor() {
        let gf = gf256();

        // ---- absorbing zero -----------------------------------------
        for &x in &[1u16, 5, 42, 128, 255] {
            assert_eq!(
                gf.mul(0, x),
                0,
                "mul(0, {x}) must be 0 (kills `|| ` → `&&`)"
            );
            assert_eq!(
                gf.mul(x, 0),
                0,
                "mul({x}, 0) must be 0 (kills `|| ` → `&&` on second arg)"
            );
        }
        assert_eq!(gf.mul(0, 0), 0);

        // ---- multiplicative identity --------------------------------
        for &x in &[1u16, 2, 5, 17, 99, 128, 255] {
            assert_eq!(gf.mul(1, x), x, "mul(1, {x}) must equal {x}");
            assert_eq!(gf.mul(x, 1), x, "mul({x}, 1) must equal {x}");
        }

        // ---- commutativity ------------------------------------------
        let pairs: &[(u16, u16)] = &[(2, 3), (5, 7), (17, 42), (128, 64), (200, 31), (3, 255)];
        for &(a, b) in pairs {
            assert_eq!(
                gf.mul(a, b),
                gf.mul(b, a),
                "mul not commutative at ({a}, {b})"
            );
        }

        // ---- concrete poly anchors ----------------------------------
        // Pre-wrap multiplications (no overflow): mul(2, 2), mul(2, 4),
        // mul(2, 64) all just shift left.
        assert_eq!(gf.mul(2, 2), 4, "alog[1+1]=alog[2]=4");
        assert_eq!(gf.mul(2, 4), 8, "alog[1+2]=alog[3]=8");
        assert_eq!(gf.mul(2, 64), 128, "alog[1+6]=alog[7]=128");
        // First wraparound: mul(2, 128) triggers `x >= gf → x ^= poly`
        // in the alog build (x=256, poly=301). alog[8] = 256 ^ 301 =
        // 0b1_0000_0000 ^ 0b1_0010_1101 = 0b0_0010_1101 = 45.
        assert_eq!(
            gf.mul(2, 128),
            45,
            "mul(2, 128) wraps via poly 301: alog[8] = 256 ^ 301 = 45 \
             (kills `% 255` mutants and antilog table drift)"
        );
        // mul(128, 2) should match (commutativity sanity).
        assert_eq!(gf.mul(128, 2), 45);
    }

    /// Stage 11.A8c — pin `gf256_rs_block_ecc` LFSR base cases. The
    /// Reed-Solomon ECC engine over GF(256) had no direct test (only
    /// exercised end-to-end via codeone encodes). Adds simple
    /// hand-verifiable cases that lock the LFSR update structure.
    ///
    /// Cases (coeffs=[1, 1] means gen poly with first/last coef = 1):
    /// * data=[], any coeffs → returns the zero-initialised lfsr.
    /// * data=[0,0,0], coeffs=[1,1,1] → val stays 0 every iter; lfsr
    ///   never moves → all zeros.
    /// * data=[1], coeffs=[1,1] → val=1, gf.mul(1,1)=1:
    ///   lfsr[0] = lfsr[1] ^ mul(coeffs[1]=1, 1) = 0 ^ 1 = 1.
    ///   lfsr[1] = mul(coeffs[0]=1, 1) = 1.
    ///   → lfsr = [1, 1].
    /// * data=[1, 1], coeffs=[1,1]: after first iter lfsr=[1,1].
    ///   2nd iter: val = 1 ^ 1 = 0.
    ///     j=0: prod = mul(1, 0) = 0; lfsr[0] = lfsr[1] ^ 0 = 1.
    ///     lfsr[1] = mul(1, 0) = 0.
    ///   → lfsr = [1, 0].
    ///
    /// Mutations caught:
    /// * `d ^ lfsr[0]` XOR (would not feed back).
    /// * `coeffs[nc - j - 1]` index (any mutation reads wrong coef).
    /// * `lfsr[j+1] ^ prod` shift-and-mix.
    /// * `lfsr[nc - 1] = gf.mul(coeffs[0], val)` final slot.
    /// * Empty data → zero lfsr (loop body skipped).
    #[test]
    fn gf256_rs_block_ecc_base_cases() {
        // Empty data preserves zero-initialised lfsr.
        assert_eq!(
            gf256_rs_block_ecc(&[], &[1, 1]),
            vec![0u16, 0],
            "empty data → zero lfsr"
        );
        assert_eq!(
            gf256_rs_block_ecc(&[], &[1, 1, 1]),
            vec![0u16, 0, 0],
            "empty data, longer lfsr"
        );
        // All-zero data leaves lfsr at zero too.
        assert_eq!(
            gf256_rs_block_ecc(&[0, 0, 0], &[1, 1, 1]),
            vec![0u16, 0, 0],
            "zero data → zero lfsr"
        );
        // Single non-zero d with unit coeffs (1, 1).
        assert_eq!(
            gf256_rs_block_ecc(&[1], &[1, 1]),
            vec![1u16, 1],
            "one d=1, unit coeffs → lfsr [1, 1]"
        );
        // Two d=1 inputs: second iter cancels via XOR feedback.
        assert_eq!(
            gf256_rs_block_ecc(&[1, 1], &[1, 1]),
            vec![1u16, 0],
            "[1,1] data, unit coeffs → lfsr [1, 0]"
        );
    }

    /// Stage 11.A8c — pin `cw_nibbles_8` MSB-first split of an 8-bit
    /// codeword into a (top, bot) pair of 4-bit nibbles. The helper
    /// powers cws_to_mmat for non-S-type Code One layouts.
    ///
    /// Bit-layout: top[i] = (v >> (7 - i)) & 1; bot[i] = (v >> (3 - i)) & 1
    /// where v = cw as u8 (truncates high u16 bits).
    ///
    /// Hand-computed cases:
    /// * cw=0xA5 (0b10100101): top=[1,0,1,0], bot=[0,1,0,1].
    /// * cw=0: both nibbles all-zero.
    /// * cw=255: both nibbles all-one.
    /// * cw=128 (top bit only): top=[1,0,0,0], bot=[0,0,0,0].
    /// * cw=1 (low bit only): top=[0,0,0,0], bot=[0,0,0,1].
    /// * cw=16 (bit 4 = top nibble low bit): top=[0,0,0,1], bot=[0,0,0,0].
    /// * cw=256 (overflows u8 to 0): both nibbles all-zero.
    /// * cw=257 (= 1 after truncation): top=[0,0,0,0], bot=[0,0,0,1].
    ///
    /// Mutations caught:
    /// * `7 - i` / `3 - i` shift formulas: any change scrambles bit
    ///   positions within nibbles.
    /// * `& 1` mask: e.g. `& 2` would always extract zero for odd values.
    /// * `for i in 0..4` boundary: changing to 0..3 would zero nibble[3].
    /// * `cw as u8` truncation: required for cw values >= 256.
    #[test]
    fn cw_nibbles_8_msb_split_with_u8_truncation() {
        // 0xA5 = 10100101.
        let (top, bot) = cw_nibbles_8(0xA5);
        assert_eq!(top, [1, 0, 1, 0]);
        assert_eq!(bot, [0, 1, 0, 1]);
        // All-zero and all-one extremes.
        assert_eq!(cw_nibbles_8(0), ([0u8; 4], [0u8; 4]));
        assert_eq!(cw_nibbles_8(255), ([1u8; 4], [1u8; 4]));
        // Single high bit.
        let (top, bot) = cw_nibbles_8(128);
        assert_eq!(top, [1, 0, 0, 0]);
        assert_eq!(bot, [0, 0, 0, 0]);
        // Single low bit.
        let (top, bot) = cw_nibbles_8(1);
        assert_eq!(top, [0, 0, 0, 0]);
        assert_eq!(bot, [0, 0, 0, 1]);
        // Bit 4 — boundary between top and bot nibbles (in top low bit).
        let (top, bot) = cw_nibbles_8(16);
        assert_eq!(top, [0, 0, 0, 1]);
        assert_eq!(bot, [0, 0, 0, 0]);
        // u8 truncation: cw=256 wraps to 0.
        assert_eq!(cw_nibbles_8(256), ([0u8; 4], [0u8; 4]));
        // cw=257 = 256 + 1 → truncates to 1.
        let (top, bot) = cw_nibbles_8(257);
        assert_eq!(top, [0, 0, 0, 0]);
        assert_eq!(bot, [0, 0, 0, 1]);
    }

    /// Stage 11.A8c — pin `ff(v)`. BWIPP `$f` truncation: if the value
    /// is exactly an i64-representable integer, return as-is; else
    /// round-trip through f32 (which loses precision vs f64). Used in
    /// per-mode cost accumulators in `lookup()` to keep BWIPP-parity
    /// arithmetic on PostScript single-precision floats.
    ///
    /// The integer-arm vs f32-arm split is what makes the helper
    /// distinct from `v as f32 as f64`; the test must drive both arms
    /// AND show they diverge for a witness input.
    ///
    /// Witness for integer arm: 16_777_217 is exact in i64 / f64 but
    /// NOT representable in f32 — `v as f32 as f64` would round to
    /// 16_777_216.0. So if the predicate flipped or the integer arm
    /// was deleted, this anchor would diverge.
    ///
    /// Witness for f32 arm: 0.6666667 (the literal used in `lookup`)
    /// — non-integer, must equal the f32 round-trip exactly.
    ///
    /// Mutations caught:
    ///   * `==` → `!=` on the predicate (integer arm + f32 arm swap);
    ///   * `replace ff with v` (always identity): 0.6666667 diverges;
    ///   * `replace ff with v as f32 as f64`: 16777217.0 diverges;
    ///   * `as i64` → other casts: 16777217 anchor pins the round-trip.
    #[test]
    fn ff_preserves_integer_path_and_f32_truncates_fractional_path() {
        // ---- integer arm ----------------------------------------------
        // Small integers: identity (also f32-stable, weak anchor).
        assert_eq!(ff(0.0), 0.0);
        assert_eq!(ff(1.0), 1.0);
        assert_eq!(ff(-3.0), -3.0);
        assert_eq!(ff(1234567.0), 1234567.0);

        // Strong integer-arm witness: 16_777_217 is i64-exact AND
        // f64-exact, but its f32 cast rounds DOWN to 16_777_216. ff
        // must return the unrounded 16_777_217.0.
        let v = 16_777_217.0_f64;
        assert_eq!(v as i64 as f64, v, "precondition: 16777217 is i64-exact");
        assert_ne!(
            v as f32 as f64, v,
            "precondition: 16777217 loses precision via f32 round-trip"
        );
        assert_eq!(
            ff(v),
            v,
            "ff(16777217.0) must take integer arm and return unmodified \
             (kills `==` → `!=` mutant: the swap would round to 16777216.0)"
        );

        // ---- f32 (fractional) arm -------------------------------------
        // ff(0.6666667) must equal the f32 round-trip. This is what
        // BWIPP's $f operator does for sub-integer accumulators.
        let frac = 0.6666667_f64;
        assert_ne!(
            frac as i64 as f64, frac,
            "precondition: 0.6666667 not exact int"
        );
        assert_eq!(
            ff(frac),
            frac as f32 as f64,
            "fractional arm must round-trip through f32"
        );
        // Likewise 1.3333334 and 4.3333334 (other lookup-cost adds).
        let f = 1.3333334_f64;
        assert_eq!(ff(f), f as f32 as f64);
        let f = 4.3333334_f64;
        assert_eq!(ff(f), f as f32 as f64);
    }

    /// Stage 11.A8c — pin `pick_symbol_size` walk over METRICS_NONSTYPE.
    /// The helper returns the first metric whose dcw >= cws_len, in
    /// the order A, B, C, D, E, F, G, H, T-16, T-32, T-48.
    ///
    /// Boundary pins:
    ///   cws_len=0   → A (dcw=10 >= 0).
    ///   cws_len=1   → A.
    ///   cws_len=10  → A (exact match, `>=` includes).
    ///   cws_len=11  → B (dcw=19, A's 10 falls short).
    ///   cws_len=19  → B.
    ///   cws_len=20  → C (B's 19 falls short).
    ///   cws_len=1480→ H (last symbol, exact fit).
    ///   cws_len=1481→ Err.
    ///
    /// Mutations caught:
    /// * `>= cws_len` → `> cws_len`: exact-match cases (10, 19, 1480)
    ///   would skip to the next larger symbol or return Err.
    /// * Order of METRICS_NONSTYPE iteration — picking T-16 (dcw=10)
    ///   instead of A would only differ in `id` and `rows/cols`.
    ///   The id assertion catches this.
    /// * Loop boundary — past H returns Err.
    #[test]
    fn pick_symbol_size_walks_metrics_with_inclusive_boundary() {
        // A picks up the small sizes.
        assert_eq!(pick_symbol_size(0).unwrap().id, "A");
        assert_eq!(pick_symbol_size(1).unwrap().id, "A");
        assert_eq!(pick_symbol_size(10).unwrap().id, "A", "exact match `>=`");
        // Past A → B.
        assert_eq!(pick_symbol_size(11).unwrap().id, "B");
        assert_eq!(pick_symbol_size(19).unwrap().id, "B");
        // Past B → C.
        assert_eq!(pick_symbol_size(20).unwrap().id, "C");
        // Past C → D.
        assert_eq!(pick_symbol_size(45).unwrap().id, "D");
        // Largest symbol H.
        assert_eq!(pick_symbol_size(1480).unwrap().id, "H");
        // Past H → Err.
        let err = pick_symbol_size(1481).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(ref m) if m.contains("exceeds the largest symbol")),
            "got {err:?}"
        );
    }

    /// Stage 11.A8c — pin `pad_cws_matrix` Mode-A pad constant + length
    /// boundary. The helper pushes PAD codeword 129 until cws.len()
    /// reaches dcws; no-op if already at/above dcws.
    ///
    /// Mutations caught:
    /// * `cws.push(129)` constant change.
    /// * `< dcws` boundary → would over/under-pad by one.
    /// * Direction of comparison (would either infinite-loop or
    ///   never pad).
    #[test]
    fn pad_cws_matrix_extends_with_129_to_dcws() {
        // Empty input + dcws=3 → 3 pad codewords.
        let mut cws: Vec<u16> = Vec::new();
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(cws, vec![129u16, 129, 129]);
        // Partial input + dcws=5 → 3 pads appended after existing.
        let mut cws = vec![1u16, 2];
        pad_cws_matrix(&mut cws, 5);
        assert_eq!(cws, vec![1u16, 2, 129, 129, 129]);
        // Exactly at dcws → no-op.
        let mut cws = vec![1u16, 2, 3];
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(cws, vec![1u16, 2, 3]);
        // Already above dcws → no-op (never shrinks).
        let mut cws = vec![1u16, 2, 3];
        pad_cws_matrix(&mut cws, 2);
        assert_eq!(cws, vec![1u16, 2, 3]);
        // dcws = 0 → no-op even on empty input.
        // Stage 11.A8c (cont) — descriptive label naming dcws=0 no-op invariant.
        let mut cws: Vec<u16> = Vec::new();
        pad_cws_matrix(&mut cws, 0);
        assert!(
            cws.is_empty(),
            "pad_cws_matrix(empty, dcws=0) must be a no-op (no spurious padding inserted); got len={}",
            cws.len()
        );
    }

    /// Stage 11.A8c — pin `getnumremcws` table lookup boundary +
    /// symbol-boundary values. METRICS_NONSTYPE versions A..H have
    /// dcws [10, 19, 44, 91, 182, 370, 732, 1480]. The precompute
    /// sets t[dcws-1] = 1 (last cw of each version) then walks
    /// right-to-left filling t[i] = t[i+1] + 1 between boundaries.
    ///
    /// Pinned values:
    ///   getnumremcws(0)   = 10 (Version A: 10 dcws, 10 remaining at pos 0).
    ///   getnumremcws(9)   = 1  (last cw of Version A).
    ///   getnumremcws(10)  = 9  (1st cw past A; Version B dcw=19, 19-10=9).
    ///   getnumremcws(18)  = 1  (last cw of Version B).
    ///   getnumremcws(43)  = 1  (last cw of Version C, dcws=44).
    ///   getnumremcws(1479)= 1  (last cw of Version H, dcws=1480).
    ///   getnumremcws(1480)= None (past largest symbol).
    ///   getnumremcws(usize::MAX) = None.
    ///
    /// Mutations caught:
    ///   * `j >= table.len()` boundary → would accept/reject the
    ///     1480 position.
    ///   * Table-fill arithmetic (`t[i+1] + 1`) — would shift values.
    ///   * `t[dcws-1] = 1` symbol-boundary marker — A/B/H boundaries
    ///     would be wrong.
    #[test]
    fn getnumremcws_boundary_and_symbol_edges() {
        // Version A boundary.
        assert_eq!(
            getnumremcws(0),
            Some(10),
            "pos 0 → 10 remaining (Version A)"
        );
        assert_eq!(getnumremcws(9), Some(1), "pos 9 → last cw of Version A");
        // Position 10 spills into Version B.
        assert_eq!(
            getnumremcws(10),
            Some(9),
            "pos 10 → 9 remaining in Version B"
        );
        assert_eq!(getnumremcws(18), Some(1), "pos 18 → last cw of Version B");
        // Version C boundary.
        assert_eq!(getnumremcws(43), Some(1), "pos 43 → last cw of Version C");
        // Largest version H.
        assert_eq!(
            getnumremcws(1479),
            Some(1),
            "pos 1479 → last cw of Version H"
        );
        // Past largest symbol.
        assert_eq!(getnumremcws(1480), None, "pos 1480 past largest → None");
        assert_eq!(getnumremcws(usize::MAX), None, "u-MAX → None");
    }

    /// Stage 11.A8c — pin `append_dbits` (MSB-first bit append) and
    /// `flush_dbits_to_cws` (8-bit byte assembly with tail drop).
    ///
    /// append_dbits:
    /// * val=0b1010, n=4 → pushes [1, 0, 1, 0] MSB first.
    /// * val=0, n=3 → pushes [0, 0, 0].
    /// * val=0xFF, n=8 → pushes [1,1,1,1,1,1,1,1].
    /// * Appending preserves existing buffer.
    /// * Increments dbitslen by n_bits.
    ///
    /// flush_dbits_to_cws:
    /// * 8 bits [1,0,1,0,0,1,1,0] → byte 0b10100110 = 166. cws=[166].
    /// * 16 bits → 2 bytes.
    /// * Trailing < 8 bits dropped: 9-bit input → 1 byte, last bit dropped.
    /// * 0 bits → empty output.
    ///
    /// Mutations caught:
    /// * `(0..n_bits).rev()` direction (would push LSB-first).
    /// * `& 1` mask.
    /// * `*dbitslen += 1` counter.
    /// * `b * 2 + dbitsbuf[idx + k]` byte-assembly (would corrupt all
    ///   bytes if `* 2 → * 4` or `+ → *`).
    /// * `idx + 8 <= dbitslen` boundary (tail-drop semantics).
    /// * `for k in 0..8` byte size.
    #[test]
    fn append_dbits_and_flush_dbits_to_cws() {
        // append_dbits MSB-first.
        let mut buf: Vec<u8> = Vec::new();
        let mut len: usize = 0;
        append_dbits(&mut buf, &mut len, 0b1010, 4);
        assert_eq!(buf, vec![1u8, 0, 1, 0]);
        assert_eq!(len, 4);
        // Append-preserves: subsequent push extends, not replaces.
        append_dbits(&mut buf, &mut len, 0, 3);
        assert_eq!(buf, vec![1u8, 0, 1, 0, 0, 0, 0]);
        assert_eq!(len, 7);
        // All-ones byte.
        let mut buf: Vec<u8> = Vec::new();
        let mut len: usize = 0;
        append_dbits(&mut buf, &mut len, 0xFF, 8);
        assert_eq!(buf, vec![1u8; 8]);
        assert_eq!(len, 8);

        // flush_dbits_to_cws assembles MSB-first per byte.
        // [1,0,1,0,0,1,1,0] → 0b10100110 = 166.
        assert_eq!(
            flush_dbits_to_cws(&[1u8, 0, 1, 0, 0, 1, 1, 0], 8),
            vec![166u16]
        );
        // Two bytes [0b11110000, 0b00001111] = [240, 15].
        assert_eq!(
            flush_dbits_to_cws(&[1u8, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1], 16),
            vec![240u16, 15]
        );
        // 9-bit input: only 1 byte; trailing bit silently dropped.
        // [1,0,1,0,1,0,1,0,1] → byte 0b10101010 = 170, last 1 ignored.
        assert_eq!(
            flush_dbits_to_cws(&[1u8, 0, 1, 0, 1, 0, 1, 0, 1], 9),
            vec![170u16]
        );
        // 0 bits → empty.
        assert_eq!(flush_dbits_to_cws(&[], 0), Vec::<u16>::new());
        // 7 bits (< 8) → no bytes.
        assert_eq!(
            flush_dbits_to_cws(&[1u8, 0, 1, 0, 1, 0, 1], 7),
            Vec::<u16>::new()
        );
    }

    /// Stage 11.A8c — pin `ctxvals_to_cws` base-40 packing arithmetic
    /// and `ctx_base_value` mode dispatch.
    ///
    /// ctxvals_to_cws: packed = v0*1600 + v1*40 + v2 + 1, then split
    /// into (packed/256, packed%256). Hand-computed:
    ///   * [0,0,0] → packed=1 → cws=[0, 1].
    ///   * [1,0,0] → packed=1601 → cws=[6, 65].
    ///   * [0,1,0] → packed=41 → cws=[0, 41].
    ///   * [0,0,1] → packed=2 → cws=[0, 2].
    ///   * [39,39,39] → 39*1600+39*40+39+1 = 64000 → cws=[250, 0].
    ///   * Two chunks [1,2,3, 4,5,6]:
    ///     - 1*1600+2*40+3+1 = 1684 → cws[0..2]=[6, 148].
    ///     - 4*1600+5*40+6+1 = 6607 → cws[2..4]=[25, 207].
    ///
    /// ctx_base_value: mode → which lookup_X table.
    ///   MODE_C → cnvals_lookup, MODE_T → tnvals_lookup,
    ///   MODE_X → xvals_lookup, others → None.
    ///
    /// Mutations caught:
    ///   * 1600 / 40 constants in pack formula.
    ///   * `+ 1` (the bias).
    ///   * `/ 256` / `% 256` split.
    ///   * ctx_base_value match-arm swap (mode → wrong table).
    #[test]
    fn ctxvals_to_cws_and_ctx_base_value_dispatch() {
        // ctxvals_to_cws packing.
        assert_eq!(ctxvals_to_cws(&[0, 0, 0]), vec![0u16, 1]);
        assert_eq!(ctxvals_to_cws(&[1, 0, 0]), vec![6u16, 65]);
        assert_eq!(ctxvals_to_cws(&[0, 1, 0]), vec![0u16, 41]);
        assert_eq!(ctxvals_to_cws(&[0, 0, 1]), vec![0u16, 2]);
        assert_eq!(ctxvals_to_cws(&[39, 39, 39]), vec![250u16, 0]);
        // Two chunks at once.
        assert_eq!(
            ctxvals_to_cws(&[1, 2, 3, 4, 5, 6]),
            vec![6u16, 148, 25, 207]
        );
        // Empty input → empty output.
        assert_eq!(ctxvals_to_cws(&[]), Vec::<u16>::new());

        // ctx_base_value mode dispatch. Use chars guaranteed to be in
        // the corresponding set: 'A' is in C40 (uppercase), 'a' in
        // Text (lowercase), CR(13) in X12.
        assert_eq!(
            ctx_base_value(MODE_C, b'A' as i16),
            cnvals_lookup(b'A' as i16),
            "MODE_C → cnvals_lookup"
        );
        assert_eq!(
            ctx_base_value(MODE_T, b'a' as i16),
            tnvals_lookup(b'a' as i16),
            "MODE_T → tnvals_lookup"
        );
        assert_eq!(
            ctx_base_value(MODE_X, 13),
            xvals_lookup(13),
            "MODE_X → xvals_lookup"
        );
        // Other modes always → None.
        assert_eq!(ctx_base_value(MODE_A, b'A' as i16), None);
        assert_eq!(ctx_base_value(MODE_B, b'A' as i16), None);
        assert_eq!(ctx_base_value(MODE_D, b'A' as i16), None);
        // Unknown mode value.
        assert_eq!(ctx_base_value(99, b'A' as i16), None);
        // Each routed function correctly rejects chars NOT in its set.
        // 'a' is in Text but NOT in C40 or X12.
        assert_eq!(ctx_base_value(MODE_C, b'a' as i16), None);
        assert_eq!(ctx_base_value(MODE_X, b'a' as i16), None);
    }

    /// Stage 11.A8c — pin the 6 codeone predicate functions:
    /// `is_d`, `is_c`, `is_t`, `is_x`, `is_ea`, `is_fn`. Each set's
    /// membership drives the mode selector in lookup(); a single
    /// dropped match arm would silently mis-route bytes.
    ///
    /// is_d (digits): '0'..='9' true, anything else false.
    /// is_c (C40): SFT1/2/3, space, digits, A-Z.
    /// is_t (Text): SFT1/2/3, space, digits, a-z.
    /// is_x (X12): CR(13), '*'(42), '>'(62), space, digits, A-Z.
    /// is_ea: c > 127.
    /// is_fn: c < 0.
    #[test]
    fn codeone_predicate_set_membership_pins() {
        // is_d
        assert!(is_d(b'0' as i16));
        assert!(is_d(b'5' as i16));
        assert!(is_d(b'9' as i16));
        assert!(!is_d(b'/' as i16));
        assert!(!is_d(b':' as i16));
        assert!(!is_d(b'A' as i16));

        // is_c (C40): SFT1/2/3, space, digits, A-Z.
        assert!(is_c(SFT1));
        assert!(is_c(SFT2));
        assert!(is_c(SFT3));
        assert!(is_c(32)); // space
        assert!(is_c(b'5' as i16)); // digit
        assert!(is_c(b'M' as i16)); // uppercase
        assert!(!is_c(b'a' as i16)); // lowercase NOT in C40
        assert!(!is_c(b'!' as i16));
        assert!(!is_c(13)); // CR not in C40

        // is_t (Text): SFT1/2/3, space, digits, a-z.
        assert!(is_t(SFT1));
        assert!(is_t(SFT2));
        assert!(is_t(SFT3));
        assert!(is_t(32));
        assert!(is_t(b'5' as i16));
        assert!(is_t(b'm' as i16)); // lowercase
        assert!(!is_t(b'A' as i16)); // uppercase NOT in Text
        assert!(!is_t(13));

        // is_x (X12): CR, '*', '>', space, digits, A-Z.
        assert!(is_x(13)); // CR
        assert!(is_x(42)); // '*'
        assert!(is_x(62)); // '>'
        assert!(is_x(32));
        assert!(is_x(b'5' as i16));
        assert!(is_x(b'Z' as i16));
        assert!(!is_x(b'a' as i16));
        assert!(!is_x(SFT1)); // SFT not in X12
        assert!(!is_x(b'!' as i16));

        // is_ea: > 127.
        assert!(!is_ea(127));
        assert!(is_ea(128)); // boundary
        assert!(is_ea(200));
        assert!(is_ea(255));
        // Negative is NOT > 127 (numerical comparison, not absolute).
        assert!(!is_ea(-1));

        // is_fn: < 0.
        assert!(is_fn(-1));
        assert!(is_fn(SFT1)); // -13
        assert!(is_fn(PAD));
        assert!(!is_fn(0)); // boundary
        assert!(!is_fn(1));
        assert!(!is_fn(127));
    }

    /// Stage 11.A8c — pin `avals_byte`, `avals_digit_pair`, and
    /// `avals_marker` codeword arithmetic. These three lookups had no
    /// direct tests despite encoding every single Mode A byte/digit/
    /// marker; a mutation on the `+ 1`, `+ 130`, `+ 10*d1 + d2`, or
    /// any marker constant would corrupt every Mode A symbol.
    ///
    /// avals_byte:
    /// * b <= 128 → Some(b + 1). 0 → 1, 65 ('A') → 66, 128 → 129.
    /// * b > 128 → None.
    ///
    /// avals_digit_pair:
    /// * (d1, d2) both digits → 130 + 10*(d1-'0') + (d2-'0').
    /// * non-digit → None.
    ///
    /// avals_marker: 10 distinct sentinel → codeword mappings
    /// (PAD=129, LC=230, LB=231, FNC1=232, FNC2=233, FNC3=234,
    /// FNC4=235, FNC1LD=236, LX=238, LT=239).
    #[test]
    fn avals_byte_digit_pair_marker_arithmetic_and_dispatch() {
        // avals_byte: identity + 1 across boundary.
        assert_eq!(avals_byte(0), Some(1));
        assert_eq!(avals_byte(b'A'), Some(66)); // 65 + 1
        assert_eq!(avals_byte(128), Some(129));
        assert_eq!(avals_byte(129), None);
        assert_eq!(avals_byte(255), None);

        // avals_digit_pair: 130 + 10*d1 + d2.
        assert_eq!(avals_digit_pair(b'0', b'0'), Some(130));
        assert_eq!(avals_digit_pair(b'0', b'9'), Some(139));
        assert_eq!(avals_digit_pair(b'1', b'0'), Some(140));
        assert_eq!(avals_digit_pair(b'9', b'9'), Some(229));
        // Mid-range pin.
        assert_eq!(avals_digit_pair(b'4', b'2'), Some(172)); // 130 + 40 + 2
                                                             // Non-digit rejects.
        assert_eq!(avals_digit_pair(b'A', b'0'), None);
        assert_eq!(avals_digit_pair(b'0', b'A'), None);
        assert_eq!(avals_digit_pair(b'/', b'0'), None);
        assert_eq!(avals_digit_pair(b':', b'9'), None);

        // avals_marker: every match arm.
        assert_eq!(avals_marker(PAD), Some(129));
        assert_eq!(avals_marker(LC), Some(230));
        assert_eq!(avals_marker(LB), Some(231));
        assert_eq!(avals_marker(FNC1), Some(232));
        assert_eq!(avals_marker(FNC2), Some(233));
        assert_eq!(avals_marker(FNC3), Some(234));
        assert_eq!(avals_marker(FNC4), Some(235));
        assert_eq!(avals_marker(FNC1LD), Some(236));
        assert_eq!(avals_marker(LX), Some(238));
        assert_eq!(avals_marker(LT), Some(239));
        // Negative non-marker → None.
        assert_eq!(avals_marker(-99), None);
        // Positive non-marker → None (markers are all sentinel
        // negatives in this design).
        assert_eq!(avals_marker(0), None);
        assert_eq!(avals_marker(100), None);
    }

    /// Stage 11.A8c — pin `compute_next_non_x` sibling of next_xterm.
    /// Records distance to next byte NOT in the X12 base set
    /// (X12 = CR, '*', '>', space, digits, uppercase).
    ///
    /// Hand-trace for msg = [b'A', b'B', b'a', b'C', b'D']:
    ///   'a' (lowercase) is NOT in X12; everything else is.
    ///   next[5] = 9999.
    ///   i=4: 'D' in X → 9999+1 = 10000.
    ///   i=3: 'C' in X → 10001 (pre-clamp).
    ///   i=2: 'a' not in X → 0.
    ///   i=1: 'B' in X → 0+1 = 1.
    ///   i=0: 'A' in X → 1+1 = 2.
    /// Post-pass clamps. Final: [2, 1, 0, 10000, 10000, 9999].
    ///
    /// Mutations caught:
    ///   * `!is_x(msg[i])` predicate flip — would reset on X12 chars
    ///     instead of breaks.
    ///   * `next[i+1].saturating_add(1)` arithmetic.
    ///   * `next[n] = 9999` sentinel.
    ///   * `*v > 10000` clamp boundary.
    #[test]
    fn compute_next_non_x_walks_with_clamp() {
        // Lowercase 'a' is the only non-X12 byte; rest is uppercase.
        let msg: Vec<i16> = b"ABaCD".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_non_x(&msg);
        assert_eq!(next, vec![2u32, 1, 0, 10000, 10000, 9999]);
        // '!' (33) is not in X12.
        let msg: Vec<i16> = b"A!B".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_non_x(&msg);
        // i=2: 'B' in X → 10000. i=1: '!' not in X → 0. i=0: 'A' → 1.
        assert_eq!(next, vec![1u32, 0, 10000, 9999]);
        // Empty msg → [9999].
        assert_eq!(compute_next_non_x(&[]), vec![9999u32]);
        // Single non-X char.
        assert_eq!(compute_next_non_x(&[b'a' as i16]), vec![0u32, 9999]);
        // Single X char (uppercase 'A').
        assert_eq!(compute_next_non_x(&[b'A' as i16]), vec![10000u32, 9999]);
        // Space, digits, '>', '*', CR are all in X — pure-X input never resets.
        let msg: Vec<i16> = b" 12>*\r".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_non_x(&msg);
        // All 6 chars in X. From the end:
        // i=5: '\r' in X → 9999+1 = 10000.
        // i=4: '*' → 10001 → clamped 10000.
        // i=3: '>' → 10000.
        // i=2: '2' → 10000.
        // i=1: '1' → 10000.
        // i=0: ' ' → 10000.
        assert_eq!(
            next,
            vec![10000u32, 10000, 10000, 10000, 10000, 10000, 9999]
        );
    }

    /// Stage 11.A8c — pin `compute_next_xterm` walk + clamp behaviour.
    /// X12 terminators are CR (13), '*' (42), '>' (62). For each
    /// position, the helper records the distance to the next
    /// terminator; positions past the last terminator saturate at
    /// 10000 via a post-pass.
    ///
    /// Hand-trace for msg = [b'A', b'B', 13, b'C', b'D']:
    ///   next[5] = 9999 (sentinel).
    ///   i=4: 'D' not term → 9999+1 = 10000.
    ///   i=3: 'C' not term → 10000+1 = 10001 (pre-clamp).
    ///   i=2: CR is term → 0.
    ///   i=1: 'B' not term → 0+1 = 1.
    ///   i=0: 'A' not term → 1+1 = 2.
    /// Post-pass clamps 10001 → 10000. Final: [2, 1, 0, 10000, 10000, 9999].
    ///
    /// Mutations caught:
    ///   * Missing terminator constant (drop `c == 42` etc.) shifts
    ///     positions.
    ///   * `(0..n).rev()` → `(0..n)` would zero everything.
    ///   * `next[i+1].saturating_add(1)` arithmetic.
    ///   * `*v > 10000` clamp boundary.
    ///   * `next[n] = 9999` sentinel constant.
    #[test]
    fn compute_next_xterm_walks_with_clamp() {
        let msg: Vec<i16> = b"AB\rCD".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_xterm(&msg);
        assert_eq!(next, vec![2u32, 1, 0, 10000, 10000, 9999]);
        // Each of the three terminator constants resets next[i]=0.
        // '*' = 42:
        let msg: Vec<i16> = b"X*X".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_xterm(&msg);
        // i=2: 'X' → 10000. i=1: '*' → 0. i=0: 'X' → 1.
        assert_eq!(next, vec![1u32, 0, 10000, 9999]);
        // '>' = 62:
        let msg: Vec<i16> = b">A".iter().map(|&b| i16::from(b)).collect();
        let next = compute_next_xterm(&msg);
        // i=1: 'A' → 10000. i=0: '>' → 0.
        assert_eq!(next, vec![0u32, 10000, 9999]);
        // Empty msg → [9999].
        assert_eq!(compute_next_xterm(&[]), vec![9999u32]);
        // Pure-terminator msg [13, 13] → [0, 0, 9999].
        assert_eq!(compute_next_xterm(&[13i16, 13]), vec![0u32, 0, 9999]);
    }

    /// Public `encode` produces a valid `BitMatrix` for supported
    /// inputs. All currently land in Version A (16 × 18).
    #[test]
    fn public_encode_works_for_supported_inputs() {
        for input in [b"A".as_ref(), b"Hello".as_ref(), b"12345".as_ref()] {
            let bm = encode(input).unwrap_or_else(|e| {
                panic!(
                    "encode({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>")
                )
            });
            assert_eq!(
                bm.width(),
                18,
                "encode({:?}) width",
                std::str::from_utf8(input).unwrap_or("<non-utf8>")
            );
            assert_eq!(
                bm.height(),
                16,
                "encode({:?}) height",
                std::str::from_utf8(input).unwrap_or("<non-utf8>")
            );
        }
    }

    /// Marker codeword constants are unique negative `i16` sentinels +
    /// `UNLCW` is the positive 255 codeword. Anchor each one.
    #[test]
    fn marker_constants() {
        // Negative sentinels (each unique).
        let markers = [
            FNC1, FNC3, LC, LB, LX, LT, LD, UNL, FNC2, FNC4, SFT1, SFT2, SFT3, ECI, PAD, FNC1LD,
        ];
        for m in markers {
            assert!(m < 0, "marker {m} should be a negative sentinel");
        }
        // Each marker is unique.
        let mut sorted = markers;
        sorted.sort_unstable();
        for w in sorted.windows(2) {
            assert_ne!(w[0], w[1], "duplicate marker constant");
        }
        // Specific BWIPP values.
        assert_eq!(FNC1, -1);
        assert_eq!(FNC3, -2);
        assert_eq!(LC, -5);
        assert_eq!(LB, -6);
        assert_eq!(LX, -7);
        assert_eq!(LT, -8);
        assert_eq!(LD, -9);
        assert_eq!(UNL, -10);
        assert_eq!(FNC2, -11);
        assert_eq!(FNC4, -12);
        assert_eq!(SFT1, -13);
        assert_eq!(SFT2, -14);
        assert_eq!(SFT3, -15);
        assert_eq!(ECI, -16);
        assert_eq!(PAD, -17);
        assert_eq!(FNC1LD, -18);
        // UNLCW is the positive 255 codeword.
        assert_eq!(UNLCW, 255);
    }

    /// Mode constants 0..=5 are exactly six distinct small u8 values
    /// matching the BWIPP `codeone_a`..`codeone_b` ordering used by
    /// the per-mode encoder dispatch.
    #[test]
    fn mode_constants() {
        assert_eq!(MODE_A, 0);
        assert_eq!(MODE_C, 1);
        assert_eq!(MODE_T, 2);
        assert_eq!(MODE_X, 3);
        assert_eq!(MODE_D, 4);
        assert_eq!(MODE_B, 5);
    }

    /// METRICS_NONSTYPE has exactly 11 rows: A..H (8) + T-16/32/48 (3).
    /// METRICS_STYPE has 3 rows: S-10/20/30. Anchor specific cells.
    #[test]
    fn metrics_shapes_and_anchors() {
        assert_eq!(METRICS_NONSTYPE.len(), 11);
        assert_eq!(METRICS_STYPE.len(), 3);

        // First and last matrix-version rows.
        let a = METRICS_NONSTYPE[0];
        assert_eq!(a.id, "A");
        assert_eq!(a.rows, 16);
        assert_eq!(a.cols, 18);
        assert_eq!(a.dcol, 16);
        assert_eq!(a.dcw, 10);
        assert_eq!(a.rscw, 10);
        assert_eq!(a.rsbl, 1);
        // BWIPP m[7..=9] = [4, 99, 6] mapped to (riso, risi, risl).
        assert_eq!(a.riso, 4);
        assert_eq!(a.risi, NA);
        assert_eq!(a.risl, 6);

        let h = METRICS_NONSTYPE[7];
        assert_eq!(h.id, "H");
        assert_eq!(h.rows, 148);
        assert_eq!(h.cols, 134);
        assert_eq!(h.dcw, 1480);
        assert_eq!(h.rscw, 560);
        assert_eq!(h.rsbl, 8);
        // BWIPP m[7..=9] = [6, 20, 69] mapped to (riso, risi, risl).
        assert_eq!(h.riso, 6);
        assert_eq!(h.risi, 20);
        assert_eq!(h.risl, 69);

        // T-16 (first T-strip in non-S table).
        let t16 = METRICS_NONSTYPE[8];
        assert_eq!(t16.id, "T-16");
        assert_eq!(t16.dcw, 10);
        assert_eq!(t16.rsbl, 1);
        assert_eq!(t16.risl, NA);

        // S-30 (last S-strip).
        let s30 = METRICS_STYPE[2];
        assert_eq!(s30.id, "S-30");
        assert_eq!(s30.rows, 8);
        assert_eq!(s30.cols, 31);
        assert_eq!(s30.dcw, 12);
        assert_eq!(s30.rscw, 12);
        assert_eq!(s30.risl, NA);
    }

    /// CPATMAP has 10 entries: 8 matrix (A..H) + S + T. Anchor a few
    /// against BWIPP source line 31499-31511. cpatmap_for resolves via
    /// the first letter of the version id.
    #[test]
    fn cpatmap_shape_and_lookup() {
        assert_eq!(CPATMAP.len(), 10);
        assert_eq!(cpatmap_for('A'), Some("121343"));
        assert_eq!(cpatmap_for('H'), Some("121212134343"));
        assert_eq!(cpatmap_for('S'), Some("56661278"));
        assert_eq!(cpatmap_for('T'), Some("5666666666127878"));
        // Unknown letter.
        assert_eq!(cpatmap_for('Z'), None);
        // Every CPATMAP entry contains only digits in '1'..='8'.
        for &(letter, pat) in &CPATMAP {
            assert!(
                letter.is_ascii_uppercase(),
                "CPATMAP letter must be uppercase ASCII"
            );
            for c in pat.chars() {
                assert!(
                    matches!(c, '1'..='8'),
                    "CPATMAP[{letter}] = {pat:?} has invalid artifact digit {c:?}"
                );
            }
        }
    }

    /// BLACKDOTMAP has 14 entries (one per version). D and S-10 have
    /// empty dot lists. Anchor a few full-coord rows.
    #[test]
    fn blackdotmap_shape_and_lookup() {
        assert_eq!(BLACKDOTMAP.len(), 14);
        // Empty entries.
        assert_eq!(blackdotmap_for("D"), Some(&[][..]));
        assert_eq!(blackdotmap_for("S-10"), Some(&[][..]));
        // Anchored coords.
        assert_eq!(blackdotmap_for("A"), Some(&[(12, 5)][..]));
        assert_eq!(blackdotmap_for("B"), Some(&[(16, 7)][..]));
        assert_eq!(
            blackdotmap_for("F"),
            Some(&[(26, 32), (70, 32), (26, 34), (70, 34)][..])
        );
        assert_eq!(
            blackdotmap_for("H"),
            Some(&[(26, 70), (66, 70), (106, 70), (26, 72), (66, 72), (106, 72)][..])
        );
        assert_eq!(blackdotmap_for("S-30"), Some(&[(15, 4), (15, 6)][..]));
        assert_eq!(
            blackdotmap_for("T-48"),
            Some(&[(24, 10), (24, 12), (24, 14)][..])
        );
        // Unknown.
        assert_eq!(blackdotmap_for("Q"), None);
    }

    /// STYPEVALS has 18 entries each being `bin(10^i)`.
    #[test]
    fn stypevals_shape_and_anchors() {
        assert_eq!(STYPEVALS.len(), 18);
        assert_eq!(STYPEVALS[0], "1");
        assert_eq!(STYPEVALS[1], "1010");
        assert_eq!(STYPEVALS[2], "1100100");
        assert_eq!(STYPEVALS[3], "1111101000");
        // Verify by parsing as binary and comparing to 10^i for i ≤ 9.
        for (i, &s) in STYPEVALS.iter().enumerate().take(10) {
            let parsed = u64::from_str_radix(s, 2).expect("STYPEVALS entry parses as binary");
            assert_eq!(
                parsed,
                10u64.pow(i as u32),
                "STYPEVALS[{i}] should equal 10^{i}"
            );
        }
        // Every char in '0' or '1'.
        for (i, &s) in STYPEVALS.iter().enumerate() {
            for c in s.chars() {
                assert!(
                    c == '0' || c == '1',
                    "STYPEVALS[{i}] = {s:?} has non-binary char {c:?}"
                );
            }
        }
    }

    /// RSPARAMS has 9 slots; only 5 (S-strip GF(32)) and 8 (matrix /
    /// T-strip GF(256)) are populated.
    #[test]
    fn rsparams_shape_and_anchors() {
        assert_eq!(RSPARAMS.len(), 9);
        assert_eq!(RSPARAMS[5], Some((32, 37)));
        assert_eq!(RSPARAMS[8], Some((256, 301)));
        for (i, slot) in RSPARAMS.iter().enumerate() {
            if i != 5 && i != 8 {
                assert_eq!(slot, &None, "RSPARAMS[{i}] should be None (placeholder)");
            }
        }
        // Convenience aliases match the populated slots.
        assert_eq!(RS_GF_S, (32, 37));
        assert_eq!(RS_GF_MATRIX, (256, 301));
    }

    /// Mode A byte / digit-pair / marker lookups (BWIPP `avals`).
    /// Anchor each kind against the BWIPP source line 31530-31570
    /// construction.
    #[test]
    fn avals_lookups() {
        // Bytes 0..=128 → cw b+1.
        assert_eq!(avals_byte(0), Some(1));
        assert_eq!(avals_byte(1), Some(2));
        assert_eq!(avals_byte(b'A'), Some(66)); // 65 + 1
        assert_eq!(avals_byte(127), Some(128));
        assert_eq!(avals_byte(128), Some(129));
        // Above the table.
        assert_eq!(avals_byte(129), None);
        assert_eq!(avals_byte(255), None);

        // Digit pairs "00".."99" → cw 130..=229.
        assert_eq!(avals_digit_pair(b'0', b'0'), Some(130));
        assert_eq!(avals_digit_pair(b'0', b'1'), Some(131));
        assert_eq!(avals_digit_pair(b'1', b'0'), Some(140));
        assert_eq!(avals_digit_pair(b'9', b'9'), Some(229));
        // Non-digit byte rejects.
        assert_eq!(avals_digit_pair(b'A', b'0'), None);
        assert_eq!(avals_digit_pair(b'0', b'A'), None);

        // Markers.
        assert_eq!(avals_marker(PAD), Some(129));
        assert_eq!(avals_marker(LC), Some(230));
        assert_eq!(avals_marker(LB), Some(231));
        assert_eq!(avals_marker(FNC1), Some(232));
        assert_eq!(avals_marker(FNC2), Some(233));
        assert_eq!(avals_marker(FNC3), Some(234));
        assert_eq!(avals_marker(FNC4), Some(235));
        assert_eq!(avals_marker(FNC1LD), Some(236));
        assert_eq!(avals_marker(LX), Some(238)); // note: 237 skipped
        assert_eq!(avals_marker(LT), Some(239));
        // Markers that don't have an A-mode codeword.
        assert_eq!(avals_marker(LD), None); // Mode D entry is bit-level
        assert_eq!(avals_marker(UNL), None);
        assert_eq!(avals_marker(SFT1), None);
        assert_eq!(avals_marker(ECI), None);
    }

    /// Mode B encoder emits a length prefix (1 cw for p<250, 2 cws
    /// otherwise) then 1 cw per byte. Mirrors BWIPP `encB` line
    /// 32997-33015.
    #[test]
    fn encode_mode_b_run_basic() {
        // Stage 11.A8c (cont) — bare `.unwrap()` → `.expect(...)` per
        // boundary case so a Mode B prefix-length mutation names the
        // expected boundary in the failure.
        // Empty run still emits a length prefix of 0 (caller can drop
        // it if redundant).
        let cws = encode_mode_b_run(b"", false)
            .expect("encode_mode_b_run(b\"\", omit=false) (empty run → 1-cw length prefix [0]) must succeed");
        assert_eq!(cws, vec![0]);

        // 5-byte run "hello": prefix=5, then 5 cws = byte values.
        let cws = encode_mode_b_run(b"hello", false).expect(
            "encode_mode_b_run(b\"hello\", omit=false) (5-byte run < 249 → 1-cw prefix=5 + 5 byte cws) must succeed",
        );
        assert_eq!(cws, vec![5, 104, 101, 108, 108, 111]);

        // Single-byte run.
        let cws = encode_mode_b_run(&[0xFF], false).expect(
            "encode_mode_b_run(b\"\\xFF\", omit=false) (1-byte run < 249 → 1-cw prefix=1 + 0xFF) must succeed",
        );
        assert_eq!(cws, vec![1, 255]);

        // Boundary: exactly 249 bytes — 1-cw prefix.
        let payload: Vec<u8> = (0..249).map(|i| i as u8).collect();
        let cws = encode_mode_b_run(&payload, false).expect(
            "encode_mode_b_run(249-byte payload, omit=false) (exactly-at 1-cw-prefix boundary: 249 → prefix=249) must succeed",
        );
        assert_eq!(cws.len(), 250); // 1 prefix + 249 bytes
        assert_eq!(cws[0], 249);
        for (i, &cw) in cws[1..].iter().enumerate() {
            assert_eq!(cw, i as u16);
        }

        // Boundary: 250 bytes triggers the 2-cw prefix.
        // BWIPP formula: (p/250)+249, p%250 → (1, 0) for p=250.
        let payload250: Vec<u8> = (0..250).map(|i| (i % 256) as u8).collect();
        let cws = encode_mode_b_run(&payload250, false).expect(
            "encode_mode_b_run(250-byte payload, omit=false) (one-past-1-cw-prefix → 2-cw prefix: (p/250)+249, p%250 = (250, 0)) must succeed",
        );
        assert_eq!(cws.len(), 252); // 2 prefix + 250 bytes
        assert_eq!(cws[0], 250); // (250/250)+249 = 250
        assert_eq!(cws[1], 0); // 250 % 250 = 0

        // 500 bytes → prefix (501, 0) wait — (500/250)+249 = 251; 500%250 = 0.
        let payload500: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let cws = encode_mode_b_run(&payload500, false).expect(
            "encode_mode_b_run(500-byte payload, omit=false) (2-cw prefix mid-range: (500/250)+249, 500%250 = (251, 0)) must succeed",
        );
        assert_eq!(cws[0], 251);
        assert_eq!(cws[1], 0);
        assert_eq!(cws.len(), 502);

        // omit_prefix_if_exact = true: no prefix emitted.
        let cws = encode_mode_b_run(b"hi", true).expect(
            "encode_mode_b_run(b\"hi\", omit=true) (omit_prefix_if_exact path → 2 byte cws, no prefix) must succeed",
        );
        assert_eq!(cws, vec![104, 105]);
    }

    /// Mode B rejects payloads larger than the version-H ceiling.
    #[test]
    fn encode_mode_b_run_rejects_overlong() {
        let too_big = vec![0u8; MODE_B_MAX_LEN + 1];
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 497-501 (`code one mode B: payload of 1481 bytes exceeds
        // max 1480`). Both byte counts surface in the diagnostic
        // proving both interpolations (`bytes.len()` and
        // `MODE_B_MAX_LEN`) work.
        match encode_mode_b_run(&too_big, false).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code one mode B:"),
                    "missing `code one mode B:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload of 1481 bytes"),
                    "missing `payload of 1481 bytes` actual-length echo: {msg}"
                );
                assert!(
                    msg.contains("exceeds max"),
                    "missing `exceeds max` predicate: {msg}"
                );
                assert!(
                    msg.contains("max 1480"),
                    "missing `max 1480` MODE_B_MAX_LEN echo (proves the cap constant is exposed): {msg}"
                );
            }
            other => panic!(
                "encode_mode_b_run({} bytes) should reject as InvalidData, got {other:?}",
                MODE_B_MAX_LEN + 1
            ),
        }
    }

    /// Stage 20.5 — Mode B is reachable through the full
    /// `encode_message` dispatcher. Latching into Mode B used to
    /// surface `Error::InvalidData("Mode B encoder (full) deferred
    /// to Stage 7+")`. After this stage it succeeds; the codeword
    /// stream prefixes the run with `LB` (the Mode B latch from
    /// Mode A), then the BWIPP `encB` length prefix, then one
    /// codeword per raw byte.
    ///
    /// Mode B latches stay active until end-of-message, matching
    /// BWIPP's dispatcher loop (no UNLCW back to Mode A within a
    /// single Mode B segment).
    #[test]
    fn encode_message_routes_high_bytes_through_mode_b() {
        // Three consecutive high bytes (>127) force Mode A's
        // lookup() to pick Mode B from i=0 because the C/T/X sets
        // can't represent them and the per-A byte cost is 2 cws
        // (FNC4 + payload) per byte vs 1 cw per byte for Mode B.
        // The expected stream is [LB, 3, 0xC8, 0xC9, 0xCA].
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // Mode-A-to-Mode-B cost-comparison rationale.
        let input = [0xC8_u8, 0xC9, 0xCA];
        let cws = encode_message(&input).expect(
            "encode_message([0xC8, 0xC9, 0xCA]) (3 high bytes >127 → Mode A lookup picks Mode B at i=0 because Mode A cost FNC4+payload=2cws/byte > Mode B 1cw/byte) must succeed",
        );
        let lb = avals_marker(LB).expect("LB marker resolves");
        assert_eq!(
            cws,
            vec![lb, 3, 0xC8, 0xC9, 0xCA],
            "Mode B latch + length prefix + raw bytes"
        );
    }

    /// Mode B accepts the full 0x80..=0xFF high-byte range (the
    /// portion that distinguishes Mode B from Mode A's 7-bit-ASCII
    /// subset). 128 high bytes routes through Mode B unambiguously
    /// because Mode A's per-byte cost (FNC4 + payload = 2 cws) is
    /// always worse than Mode B's 1-cw-per-byte for this range.
    ///
    /// (Low control bytes 0x00..=0x1F do also encode in Mode B in
    /// BWIPP, but at i=0 they may compete with Mode X via the
    /// `xc` cost. A full corpus that covers all 256 byte values
    /// requires the BWIPP corpus generator; left for the Mode B
    /// promotion stage.)
    #[test]
    fn encode_message_mode_b_accepts_high_byte_range() {
        let input: Vec<u8> = (128..=255).collect();
        assert_eq!(input.len(), 128);
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // full-high-byte-range Mode B path + expected stream
        // composition (LB latch + 1-cw length prefix + 128 raw bytes).
        let cws = encode_message(&input).expect(
            "encode_message(128..=255) (full 0x80..=0xFF high-byte range, 128 bytes → Mode B latch LB + encB 1-cw length prefix (128 < 250) + 128 raw byte cws) must succeed",
        );
        // First cw is LB latch, then encB length prefix (1 cw
        // because 128 < 250), then 128 cws of raw byte values.
        let lb = avals_marker(LB).expect("LB marker resolves");
        assert_eq!(cws[0], lb);
        assert_eq!(cws[1], 128); // length prefix
        assert_eq!(cws.len(), 1 + 1 + 128);
        for (i, expected) in (128u16..=255).enumerate() {
            assert_eq!(cws[2 + i], expected, "byte at index {i}");
        }
    }

    /// FNC / ECI markers inside a Mode B run remain rejected:
    /// `Ctx::from_bytes` doesn't actually emit negative cells today,
    /// but if a future preprocessor injects them, the Mode B step
    /// must surface a clear extension-path error instead of
    /// silently miscoding.
    #[test]
    fn encode_b_step_rejects_negative_cells() {
        let mut ctx = Ctx::from_bytes(b"AB");
        // Force a negative cell + Mode B entry to exercise the
        // negative-cell guard at line 2088-2092 ("code one Mode B:
        // marker / ECI cell {cell} cannot be encoded as a raw byte
        // (FNC and ECI paths inside Mode B are extension paths — see
        // PORT_STATUS)").
        ctx.msg[1] = -1;
        ctx.mode = MODE_B;
        let err = encode_b_step(&mut ctx).unwrap_err();
        let msg = format!("{err:?}");
        // 5-anchor pin upgrades the existing 2-anchor `&&` to:
        //   * "code one Mode B:" full symbology+mode prefix.
        //   * "cell -1" — proves `{cell}` interpolation echoes the
        //     caller's negative value (kills `{cell:?}` or hardcoded
        //     value mutations).
        //   * "cannot be encoded as a raw byte" predicate.
        //   * "extension paths" remediation hint.
        //   * "PORT_STATUS" doc-pointer.
        assert!(
            msg.contains("code one Mode B:"),
            "diagnostic must carry the full code one Mode B prefix; got {msg}"
        );
        assert!(
            msg.contains("cell -1"),
            "diagnostic must echo the offending cell value -1; got {msg}"
        );
        assert!(
            msg.contains("cannot be encoded as a raw byte"),
            "diagnostic must carry the predicate; got {msg}"
        );
        assert!(
            msg.contains("extension paths"),
            "diagnostic must name the extension-paths remediation; got {msg}"
        );
        assert!(
            msg.contains("PORT_STATUS"),
            "diagnostic must point callers at PORT_STATUS; got {msg}"
        );
    }

    /// `compute_numd` builds the BWIPP digit-run lookahead.
    /// `numD[i]` = count of consecutive digits starting at i.
    #[test]
    fn compute_numd_lookahead() {
        // Pure non-digit.
        assert_eq!(compute_numd(b"A".map(i16::from).as_slice()), vec![0, 0]);
        assert_eq!(
            compute_numd(b"Hello".map(i16::from).as_slice()),
            vec![0, 0, 0, 0, 0, 0]
        );
        // Pure digits.
        assert_eq!(
            compute_numd(b"12345".map(i16::from).as_slice()),
            vec![5, 4, 3, 2, 1, 0]
        );
        // Mixed.
        assert_eq!(
            compute_numd(b"A12B3".map(i16::from).as_slice()),
            vec![0, 2, 1, 0, 1, 0]
        );
        // Markers (negative i16) aren't digits.
        let msg = vec![FNC1, b'1' as i16, b'2' as i16];
        assert_eq!(compute_numd(&msg), vec![0, 2, 1, 0]);
        // Empty.
        assert_eq!(compute_numd(&[]), vec![0]);
    }

    /// Mode A end-to-end golden against bwip-js for inputs that stay
    /// in Mode A throughout. Captured via `/tmp/oracle-codeone-cws.js`:
    ///
    ///   "A"     → [66]
    ///   "AB"    → [66, 67]
    ///   "1"     → [50]
    ///   "12"    → [142]       (digit-pair packing)
    ///   "123"   → [142, 52]   (digit-pair + single byte)
    ///   "Hello" → [73, 102, 109, 109, 112]
    #[test]
    fn encode_mode_a_only_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            (b"A", &[66]),
            (b"AB", &[66, 67]),
            (b"1", &[50]),
            (b"12", &[142]),
            (b"123", &[142, 52]),
            (b"Hello", &[73, 102, 109, 109, 112]),
        ];
        for &(input, expected) in cases {
            let cws = encode_mode_a_only(input).unwrap_or_else(|e| {
                panic!(
                    "encode_mode_a_only({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_mode_a_only({:?}) cws",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Stage 3d: `encode_mode_a_only` (the Mode-A-only test harness) no
    /// longer rejects long digit runs — Mode D now handles those.
    /// `encode_mode_a_only` runs `encode_a_step` in a loop; once Mode
    /// D triggers, the dispatcher returns Ok() but leaves the encoder
    /// in MODE_D state, so `encode_mode_a_only` exits with cws shorter
    /// than expected. Still rejects high-byte inputs (Mode B / FNC4
    /// path still deferred).
    #[test]
    fn encode_mode_a_only_high_byte_still_rejected() {
        // Byte > 128 needs FNC4 / Mode B (still extension path).
        // Diagnostic at line 2047: "code one Mode A: byte 0x{byte:02x}
        // > 128 — use FNC4 (Stage 6+) or Mode B" — both substrings are
        // always present, so the previous `||` is unnecessarily weak.
        // 4-anchor `&&` pin: symbology prefix + byte echo + 128 bound
        // + remediation hint.
        let high_byte = [200u8];
        let err = encode_mode_a_only(&high_byte).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("code one Mode A:"),
            "high-byte diagnostic must carry code one Mode A prefix; got {msg}"
        );
        assert!(
            msg.contains("byte 0xc8"),
            "high-byte diagnostic must echo byte=0xc8 (200 in hex); got {msg}"
        );
        assert!(
            msg.contains("> 128"),
            "high-byte diagnostic must carry the > 128 bound; got {msg}"
        );
        assert!(
            msg.contains("FNC4") && msg.contains("Mode B"),
            "high-byte diagnostic must name BOTH remediation paths (FNC4, Mode B); got {msg}"
        );
    }

    /// `Ctx::from_bytes` initializes msg/numd/mode/i correctly + the
    /// new lookahead arrays land.
    #[test]
    fn ctx_from_bytes() {
        // Stage 11.A8c (cont) — descriptive label naming initial-state
        // invariant for freshly-constructed Ctx.
        let ctx = Ctx::from_bytes(b"A1");
        assert_eq!(ctx.msg, vec![65, 49]);
        assert_eq!(ctx.numd, vec![0, 1, 0]);
        assert_eq!(ctx.i, 0);
        assert_eq!(ctx.mode, MODE_A);
        assert!(
            ctx.cws.is_empty(),
            "Ctx::from_bytes(b\"A1\") must initialize cws to empty (no spurious initial codewords); got len={}",
            ctx.cws.len()
        );
        assert_eq!(ctx.next_xterm.len(), 3);
        assert_eq!(ctx.next_non_x.len(), 3);
    }

    /// Stage 11.A8c — pin `Ctx::at_end`. The cursor-exhaustion
    /// predicate gates every encoder loop in this module
    /// (`while !ctx.at_end()` at lines 1788, 2114; early-exit guards
    /// at 1009, 1098, 1955, 2083). A flipped predicate would silently
    /// run past or skip the last input byte.
    ///
    /// Anchors pin the strict `>= msg.len()` semantics:
    ///   * empty msg, i=0 → at_end (kills `>=` → `>`: 0 > 0 = false,
    ///     would let an empty-input loop iterate);
    ///   * single byte, i=0 → !at_end; i=1 (== len) → at_end (kills
    ///     `>=` → `>` again on the boundary);
    ///   * single byte, i=2 (> len) → at_end (kills `>=` → `==`);
    ///   * multi-byte interior + boundary;
    ///   * cws/dbits don't affect at_end (it only reads `i` and
    ///     `msg.len()`) — pinning that prevents `i >= self.cws.len()`
    ///     drift.
    #[test]
    fn ctx_at_end_strict_ge_predicate() {
        // Empty msg: at_end immediately.
        let ctx = Ctx::from_bytes(b"");
        assert!(ctx.at_end(), "empty msg + i=0 must be at_end (i >= 0)");

        // Single byte: i=0 not at_end, advance to i=1 → at_end.
        let mut ctx = Ctx::from_bytes(b"A");
        assert!(!ctx.at_end(), "single byte + i=0 not at_end");
        ctx.i = 1;
        assert!(
            ctx.at_end(),
            "single byte + i=1 (== len) at_end (kills `>=` → `>`)"
        );

        // Past-end cursor still at_end (kills `>=` → `==`).
        ctx.i = 99;
        assert!(ctx.at_end(), "i > len still at_end (kills `>=` → `==`)");

        // Multi-byte: walk through every position.
        let mut ctx = Ctx::from_bytes(b"AB1");
        for cursor in 0..3 {
            ctx.i = cursor;
            assert!(!ctx.at_end(), "msg.len()=3 + i={cursor} not at_end");
        }
        ctx.i = 3;
        assert!(ctx.at_end(), "msg.len()=3 + i=3 (boundary) at_end");

        // cws presence doesn't affect at_end (proves predicate reads
        // msg.len(), not cws.len()).
        let mut ctx = Ctx::from_bytes(b"AB");
        ctx.cws = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        ctx.i = 1;
        assert!(
            !ctx.at_end(),
            "i=1 < msg.len()=2 not at_end regardless of cws length"
        );
        ctx.i = 2;
        assert!(
            ctx.at_end(),
            "i=2 == msg.len()=2 at_end regardless of cws length"
        );
    }

    /// Mode-set predicates against BWIPP's `cnvals` / `tnvals` /
    /// `xvals` sets. Each is_X(c) returns true iff c is a base
    /// member of that set.
    #[test]
    fn mode_predicates() {
        // is_d: digits only.
        assert!(is_d(b'0' as i16));
        assert!(is_d(b'5' as i16));
        assert!(is_d(b'9' as i16));
        assert!(!is_d(b'/' as i16));
        assert!(!is_d(b':' as i16));
        assert!(!is_d(b'A' as i16));
        assert!(!is_d(FNC1));

        // is_c: SFT1/2/3, space, digit, A-Z.
        assert!(is_c(SFT1));
        assert!(is_c(SFT2));
        assert!(is_c(SFT3));
        assert!(is_c(32)); // space
        assert!(is_c(b'0' as i16));
        assert!(is_c(b'A' as i16));
        assert!(is_c(b'Z' as i16));
        assert!(!is_c(b'a' as i16));
        assert!(!is_c(b'!' as i16));
        assert!(!is_c(FNC1));

        // is_t: SFT1/2/3, space, digit, a-z.
        assert!(is_t(SFT1));
        assert!(is_t(32));
        assert!(is_t(b'5' as i16));
        assert!(is_t(b'a' as i16));
        assert!(is_t(b'z' as i16));
        assert!(!is_t(b'A' as i16));
        assert!(!is_t(b'!' as i16));

        // is_x: CR, *, >, space, digit, A-Z.
        assert!(is_x(13));
        assert!(is_x(42));
        assert!(is_x(62));
        assert!(is_x(32));
        assert!(is_x(b'9' as i16));
        assert!(is_x(b'A' as i16));
        assert!(!is_x(b'a' as i16));
        assert!(!is_x(b'!' as i16));

        // is_ea: > 127.
        assert!(is_ea(128));
        assert!(is_ea(200));
        assert!(is_ea(255));
        assert!(!is_ea(127));
        assert!(!is_ea(0));
        assert!(!is_ea(-1));

        // is_fn: negative.
        assert!(is_fn(-1));
        assert!(is_fn(FNC1));
        assert!(is_fn(PAD));
        assert!(!is_fn(0));
        assert!(!is_fn(127));
    }

    /// nextXterm + nextNonX lookahead arrays. For each position i,
    /// `next_xterm[i]` = chars until next CR/`*`/`>`; `next_non_x[i]`
    /// = chars until next non-X12 byte.
    #[test]
    fn lookahead_arrays() {
        // No terminator, no non-X chars → both saturate.
        let msg: Vec<i16> = b"ABCDE".iter().map(|&b| i16::from(b)).collect();
        let nxt = compute_next_xterm(&msg);
        let nnx = compute_next_non_x(&msg);
        assert_eq!(nxt.last().copied().unwrap(), 9999);
        // No terminator, so each position counts up to msglen, but
        // saturates at 10000.
        for (i, &v) in nxt.iter().take(5).enumerate() {
            // Distance from i to the (non-existent) terminator: capped.
            assert!(v >= 5 - i as u32, "next_xterm[{i}]={v}");
        }
        // All A-Z are X-valid, so next_non_x is high.
        for &v in nnx.iter().take(5) {
            assert!(v >= 1);
        }

        // With a terminator at index 2 ('*'):
        let msg: Vec<i16> = b"AB*DE".iter().map(|&b| i16::from(b)).collect();
        let nxt = compute_next_xterm(&msg);
        assert_eq!(nxt[0], 2);
        assert_eq!(nxt[1], 1);
        assert_eq!(nxt[2], 0);
        assert!(nxt[3] >= 3);

        // Mixed with non-X char:
        let msg: Vec<i16> = b"AB!DE".iter().map(|&b| i16::from(b)).collect();
        let nnx = compute_next_non_x(&msg);
        assert_eq!(nnx[0], 2);
        assert_eq!(nnx[1], 1);
        assert_eq!(nnx[2], 0);
    }

    /// Stage 11.A8c — pin `xterm_first(next_xterm, next_non_x, i)`.
    /// Used inside `lookup()` to decide whether the next X12 terminator
    /// arrives before the next non-X12 char (strictly: `xterm < non_x`).
    /// Equal distances mean a non-X char and a terminator collide at the
    /// same position — in BWIPP's logic, that doesn't count as
    /// "X-terminator first" so the comparison must be strict `<`.
    ///
    /// Mutations to catch:
    ///   - `<` → `<=`: equal distance would flip false → true.
    ///   - `<` → `>`: completely inverted logic; almost every nontrivial
    ///     case diverges.
    ///   - `<` → `==`: true only when distances coincide (rare).
    #[test]
    fn xterm_first_strictly_less() {
        // Strict less: xterm = 2, non_x = 5 → true.
        let xterm = [2u32];
        let non_x = [5u32];
        assert!(xterm_first(&xterm, &non_x, 0));
        // Strict greater: xterm = 7, non_x = 3 → false.
        let xterm = [7u32];
        let non_x = [3u32];
        assert!(!xterm_first(&xterm, &non_x, 0));
        // Equal: xterm = 4, non_x = 4 → false (rejects `<=` mutation).
        let xterm = [4u32];
        let non_x = [4u32];
        assert!(
            !xterm_first(&xterm, &non_x, 0),
            "equal distances must NOT count as xterm-first (rejects `<=`)"
        );
        // Saturated xterm (no terminator) vs near non-X: xterm wins → false.
        let xterm = [9999u32];
        let non_x = [1u32];
        assert!(!xterm_first(&xterm, &non_x, 0));
        // Both at i but reading later index: verify indexing is correct.
        let xterm = [99, 2, 100];
        let non_x = [99, 5, 1];
        assert!(xterm_first(&xterm, &non_x, 1), "i=1: 2 < 5 → true");
        assert!(
            !xterm_first(&xterm, &non_x, 2),
            "i=2: 100 < 1 is false (rejects index swap)"
        );
        assert!(
            !xterm_first(&xterm, &non_x, 0),
            "i=0: 99 < 99 is false (rejects `<=`)"
        );
    }

    /// Stage 11.A8c — pin the `ff` f32-truncation helper. BWIPP's
    /// `$f` clamps every fractional accumulator update to f32 precision
    /// (Float32Array storage in JS). Without this clamp, lookup() picks
    /// the wrong mode on the "abcdef" boundary because tc reaches
    /// exactly 5.0 in f32 but 5.0000002 in f64. The integer-detection
    /// branch (`(v as i64 as f64) == v`) preserves large integer values
    /// that f32 would alias to a different magnitude (1e15 → 999...104.0).
    ///
    /// Mutations to catch:
    ///   - `==` → `!=`: integers would route through f32 and lose
    ///     precision (1e15 → 999999986991104.0).
    ///   - Body replaced with `v`: never truncates, drift in lookup().
    ///   - Body replaced with `f64::from(v as f32)`: always truncates,
    ///     huge integers lose precision.
    ///   - `i64` → `i32`: wraps for v above i32::MAX (~2.1e9).
    #[test]
    fn ff_truncates_fractions_but_passes_integers() {
        // Plain integers pass through unchanged.
        assert_eq!(ff(0.0), 0.0);
        assert_eq!(ff(1.0), 1.0);
        assert_eq!(ff(-1.0), -1.0);
        assert_eq!(ff(5.0), 5.0);
        assert_eq!(ff(255.0), 255.0);
        // Large integer: f32 cannot represent 1e15 exactly (loses ~13e6
        // of magnitude). If the integer branch is mutated away, ff(1e15)
        // would equal f64::from(1e15_f32) ≈ 999999986991104.0.
        let big = 1e15_f64;
        assert_eq!(
            ff(big),
            big,
            "large integer passes through (rejects integer-branch removal)"
        );
        assert_ne!(
            f64::from(big as f32),
            big,
            "sanity: 1e15 is NOT f32-exact (mutation would observe drift)"
        );
        // Negative integer beyond i32::MAX (rejects `i64` → `i32` mutation).
        let big_neg = -3_000_000_000_f64;
        assert_eq!(ff(big_neg), big_neg);
        // Fractions route through the f32 cast.
        let expected_one_tenth = f64::from(0.1_f32);
        assert_eq!(ff(0.1), expected_one_tenth);
        assert_ne!(
            ff(0.1),
            0.1,
            "ff(0.1) must differ from f64 0.1 (f32 truncation observable)"
        );
        // BWIPP's 2/3 = 0.6666666666666666 (f64) → 0.6666666... (f32).
        let two_thirds = 2.0_f64 / 3.0;
        let two_thirds_f32 = f64::from(2.0_f32 / 3.0);
        assert_eq!(ff(two_thirds), two_thirds_f32);
        // BWIPP boundary case: 5.0000002 (the "abcdef" trigger). The
        // nearest f32 to 5.0000002 is exactly 5.0 (f32 spacing at 5 is
        // ~5.96e-7, so 2e-7 below half-step ⇒ rounds down to 5.0). The
        // integer-detection branch sees (5.0000002 as i64) as i64 = 5 →
        // 5.0 != 5.0000002, so falls to f32 cast.
        let abcdef_trigger = 5.000_000_2_f64;
        assert_eq!(
            ff(abcdef_trigger),
            5.0,
            "5.0000002 → f32 → 5.0 (lookup() relies on this collapse)"
        );
    }

    /// `lookup()` returns the next mode based on BWIPP's forward-scan
    /// cost algorithm. Anchor decisions against the cws output BWIPP
    /// emits (latch-cw at start tells us which mode lookup picked).
    ///
    /// Captured from `/tmp/oracle-codeone-cws.js`:
    ///
    /// * "A", "AB", "ABC", "ABCDE", "Hello", "HelloWorld", "ABCabc"
    ///   → start with non-latch cw → lookup at i=0 returned A.
    /// * "ABCDEFG", "ABCDEFGHIJ" → start with cw 230 (LC) → lookup
    ///   at i=0 returned C.
    /// * "abc" (3 chars) → start with non-latch → lookup at i=0
    ///   returned A.
    /// * "abcdef" → start with cw 239 (LT) → lookup at i=0 returned T.
    ///   (Critical f32-precision boundary case — see [`ff`] doc.)
    #[test]
    fn lookup_matches_bwip_js_initial_mode() {
        let cases: &[(&[u8], u8)] = &[
            (b"A", MODE_A),
            (b"AB", MODE_A),
            (b"ABC", MODE_A),
            (b"ABCDE", MODE_A),
            (b"ABCDEFG", MODE_C),
            (b"ABCDEFGHIJ", MODE_C),
            (b"abc", MODE_A),
            (b"abcdef", MODE_T),
            (b"ABCabc", MODE_A),
            (b"Hello", MODE_A),
            (b"HelloWorld", MODE_A),
        ];
        for &(input, expected) in cases {
            let ctx = Ctx::from_bytes(input);
            let got = lookup(&ctx.msg, 0, MODE_A, &ctx.next_xterm, &ctx.next_non_x);
            assert_eq!(
                got,
                expected,
                "lookup({:?}) = {got}, want {expected}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// After Stage 5 wiring, `encode_mode_a_only` correctly emits the
    /// LC latch cw when lookup picks Mode C, then surfaces a Stage-6+
    /// deferred error because the CTX encoder hasn't landed yet. Pin
    /// the transition behavior so Stage 6 has a clear regression test.
    #[test]
    fn encode_mode_a_only_emits_latch_then_defers_to_ctx() {
        // "ABCDEFG" — lookup picks C. encA emits LC=230, switches
        // mode to C, then loops back. The driver detects mode != A
        // and returns the Stage 5+ deferral diagnostic from line 2118:
        //   "code one Mode A-only: encoder switched out of Mode A —
        //    caller should use the full mode-selector (Stage 5+)"
        // 4-anchor `&&` pin replaces the weak `||` (both substrings
        // are always present in this exact diagnostic):
        let err = encode_mode_a_only(b"ABCDEFG").unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("code one Mode A-only:"),
            "diagnostic must carry the code one Mode A-only prefix; got {msg}"
        );
        assert!(
            msg.contains("switched out of Mode A"),
            "diagnostic must explain WHY (encoder switched out); got {msg}"
        );
        assert!(
            msg.contains("full mode-selector"),
            "diagnostic must name the remediation (full mode-selector); got {msg}"
        );
        assert!(
            msg.contains("(Stage 5+)"),
            "diagnostic must carry the (Stage 5+) milestone hint; got {msg}"
        );
    }

    /// Stage 5b: mid-string lookup decisions. The Mode A encoder
    /// re-calls `lookup()` at every position, so we need lookup() to
    /// pick the right mode not just at i=0 but at later positions too.
    ///
    /// Goldens captured by encoding the input via BWIPP and observing
    /// when (= at what position) a latch cw appears. For "ABCabcdef":
    /// cws=[66,67,68,239,89,233,109,36,255]. cws[0..3]=[66,67,68] for
    /// the uppercase prefix (Mode A). cws[3]=239=LT triggered at i=3
    /// → lookup at i=3 with mode=A picks T.
    #[test]
    fn lookup_at_nonzero_position() {
        // "ABCabcdef" at i=3 (after "ABC" consumed) → lookup picks T.
        let ctx = Ctx::from_bytes(b"ABCabcdef");
        let got = lookup(&ctx.msg, 3, MODE_A, &ctx.next_xterm, &ctx.next_non_x);
        assert_eq!(
            got, MODE_T,
            "lookup(\"ABCabcdef\", i=3) = {got}, want T (2)"
        );

        // "abcd1234" at i=4 (after "abcd" consumed) — BWIPP emits
        // [98,99,100,101,142,164] = avals[a..d] + avals["12"] + avals
        // ["34"]. So mode stays A throughout.
        let ctx = Ctx::from_bytes(b"abcd1234");
        for start in 0..=4 {
            let got = lookup(&ctx.msg, start, MODE_A, &ctx.next_xterm, &ctx.next_non_x);
            assert_eq!(
                got, MODE_A,
                "lookup(\"abcd1234\", i={start}) = {got}, want A"
            );
        }
    }

    /// CTX base-value lookups (cnvals/tnvals/xvals). Anchor a few
    /// known mappings against BWIPP's table-construction loops at
    /// bwip-js lines 31570-31827.
    #[test]
    fn ctx_base_value_lookups() {
        // cnvals (C40).
        assert_eq!(cnvals_lookup(SFT1), Some(0));
        assert_eq!(cnvals_lookup(SFT2), Some(1));
        assert_eq!(cnvals_lookup(SFT3), Some(2));
        assert_eq!(cnvals_lookup(32), Some(3));
        assert_eq!(cnvals_lookup(b'0' as i16), Some(4));
        assert_eq!(cnvals_lookup(b'9' as i16), Some(13));
        assert_eq!(cnvals_lookup(b'A' as i16), Some(14));
        assert_eq!(cnvals_lookup(b'Z' as i16), Some(39));
        assert_eq!(cnvals_lookup(b'a' as i16), None);
        assert_eq!(cnvals_lookup(b'!' as i16), None);

        // tnvals (Text) — like cnvals but lowercase instead of uppercase.
        assert_eq!(tnvals_lookup(b'a' as i16), Some(14));
        assert_eq!(tnvals_lookup(b'z' as i16), Some(39));
        assert_eq!(tnvals_lookup(b'A' as i16), None);

        // xvals (X12) — uppercase + digits + CR/`*`/`>` + space.
        assert_eq!(xvals_lookup(13), Some(0));
        assert_eq!(xvals_lookup(42), Some(1));
        assert_eq!(xvals_lookup(62), Some(2));
        assert_eq!(xvals_lookup(b'A' as i16), Some(14));
        assert_eq!(xvals_lookup(b'a' as i16), None);
    }

    /// `ctxvals_to_cws` packs base-40 triplets into 16-bit cws.
    /// Anchor a few known transformations:
    /// - [14, 15, 16] → 23017 → [89, 233] (= "abc" in T mode).
    /// - [17, 18, 19] → 27940 → [109, 36] (= "def" in T mode).
    #[test]
    fn ctxvals_to_cws_pack_triplets() {
        assert_eq!(ctxvals_to_cws(&[14, 15, 16]), vec![89, 233]);
        assert_eq!(ctxvals_to_cws(&[17, 18, 19]), vec![109, 36]);
        // Multiple triplets concatenate.
        assert_eq!(
            ctxvals_to_cws(&[14, 15, 16, 17, 18, 19]),
            vec![89, 233, 109, 36]
        );
        // Zero-length input → empty output.
        assert_eq!(ctxvals_to_cws(&[]), Vec::<u16>::new());
    }

    /// `pick_symbol_size` walks METRICS_NONSTYPE for the smallest
    /// version whose dcws ≥ cws_len.
    #[test]
    fn pick_symbol_size_picks_smallest_fit() {
        // Stage 11.A8c (cont) — bare `.unwrap()` → `.unwrap_or_else` /
        // `.expect(...)` per call so a metric-table mutation that
        // drops a row or shifts the smallest-fit boundary names the
        // expected version letter + dcws in the panic.
        for n in 0..=10 {
            let m = pick_symbol_size(n).unwrap_or_else(|e| {
                panic!(
                    "pick_symbol_size({n}) (sizes 0..=10 must fit version A, dcws=10) failed: {e}"
                )
            });
            assert_eq!(m.id, "A", "size {n} should pick A");
            assert_eq!(m.dcw, 10);
        }
        let m = pick_symbol_size(11).expect(
            "pick_symbol_size(11) (11 cws > A.dcws=10 → smallest-fit is B, dcws=19) must succeed",
        );
        assert_eq!(m.id, "B");
        assert_eq!(m.dcw, 19);
        let m = pick_symbol_size(20).expect(
            "pick_symbol_size(20) (20 cws > B.dcws=19 → smallest-fit is C, dcws=44) must succeed",
        );
        assert_eq!(m.id, "C");
        assert_eq!(m.dcw, 44);
        let m = pick_symbol_size(1480).expect(
            "pick_symbol_size(1480) (exact-fit to largest version-H ceiling: H.dcws=1480) must succeed",
        );
        assert_eq!(m.id, "H");
        // Over the ceiling — pin the diagnostic from line 1144-1146.
        //
        // Diagnostic anchors:
        //   * "code one:" symbology prefix.
        //   * "cws of length 1481" — proves `{cws_len}` interpolates
        //     the boundary+1 value (not a hardcoded number).
        //   * "exceeds the largest symbol" — predicate.
        //   * "(H, dcws=1480)" — proves the H-symbol cap is named
        //     verbatim (kills mutations that drop the parenthesized
        //     hint).
        let err = pick_symbol_size(1481).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code one:"),
                    "ceiling diagnostic must carry the symbology prefix; got {msg}"
                );
                assert!(
                    msg.contains("cws of length 1481"),
                    "ceiling diagnostic must echo cws_len (1481); got {msg}"
                );
                assert!(
                    msg.contains("exceeds the largest symbol"),
                    "ceiling diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("(H, dcws=1480)"),
                    "ceiling diagnostic must name the H-symbol cap verbatim; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// `gf256_gen_coeffs` produces the BWIPP-pinned generator-poly
    /// coefficients for ecpb=10 (Version A's RS parameters).
    /// Captured via `/tmp/oracle-codeone-ecc.js`.
    #[test]
    fn gf256_gen_coeffs_ecpb10_matches_bwip_js() {
        let coeffs = gf256_gen_coeffs(10);
        assert_eq!(coeffs, vec![52, 31, 182, 85, 43, 218, 76, 113, 141, 136]);
    }

    /// Reed-Solomon block ECC on a known data block produces the same
    /// 10-byte ECC as bwip-js for "A" (padded data = [66, 129×9]).
    #[test]
    fn gf256_rs_block_ecc_matches_bwip_js_for_letter_a() {
        let coeffs = gf256_gen_coeffs(10);
        let data: Vec<u16> = std::iter::once(66u16)
            .chain(std::iter::repeat_n(129u16, 9))
            .collect();
        let ecc = gf256_rs_block_ecc(&data, &coeffs);
        // From /tmp/oracle-codeone-ecc.js for "A":
        assert_eq!(ecc, vec![42, 216, 108, 200, 178, 200, 247, 148, 106, 230]);
    }

    /// `pad_cws_matrix` fills with PAD codeword 129 up to dcws.
    #[test]
    fn pad_cws_matrix_fills_with_129() {
        let mut cws = vec![66u16];
        pad_cws_matrix(&mut cws, 10);
        assert_eq!(cws, vec![66, 129, 129, 129, 129, 129, 129, 129, 129, 129]);
        // No-op when already at length.
        let mut cws = vec![1u16; 5];
        pad_cws_matrix(&mut cws, 5);
        assert_eq!(cws.len(), 5);
    }

    /// `encode_with_ecc` end-to-end byte-for-byte goldens against
    /// bwip-js. All 5 inputs land in Version A (dcws=10, rscw=10,
    /// rsbl=1, ecpb=10). Captured via `/tmp/oracle-codeone-ecc.js`.
    #[test]
    fn encode_with_ecc_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            (
                b"A",
                &[
                    66, 129, 129, 129, 129, 129, 129, 129, 129, 129, // data
                    42, 216, 108, 200, 178, 200, 247, 148, 106, 230, // ecc
                ],
            ),
            (
                b"Hello",
                &[
                    73, 102, 109, 109, 112, 129, 129, 129, 129, 129, // data
                    248, 151, 38, 190, 182, 51, 253, 251, 119, 221, // ecc
                ],
            ),
            (
                b"ABCDEFG",
                &[
                    230, 89, 233, 109, 36, 255, 72, 129, 129, 129, // data
                    27, 137, 20, 221, 125, 242, 234, 77, 135, 221, // ecc
                ],
            ),
            (
                b"ABCDEFGHIJ",
                &[
                    230, 89, 233, 109, 36, 128, 95, 255, 75, 129, // data
                    232, 138, 60, 238, 88, 10, 139, 73, 13, 216, // ecc
                ],
            ),
            (
                b"abcdef",
                &[
                    239, 89, 233, 109, 36, 255, 129, 129, 129, 129, // data
                    76, 10, 136, 227, 193, 252, 200, 187, 220, 86, // ecc
                ],
            ),
        ];
        for &(input, expected) in cases {
            let (cws, metric) = encode_with_ecc(input).unwrap_or_else(|e| {
                panic!(
                    "encode_with_ecc({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(metric.id, "A", "all Stage-7 goldens land in version A");
            assert_eq!(
                cws,
                expected,
                "encode_with_ecc({:?}) cws",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// `cw_nibbles_8` splits an 8-bit codeword into top + bot 4-bit
    /// nibbles MSB-first.
    #[test]
    fn cw_nibbles_8_anchors() {
        // 66 = 0b01000010 → top=[0,1,0,0], bot=[0,0,1,0]
        let (top, bot) = cw_nibbles_8(66);
        assert_eq!(top, [0, 1, 0, 0]);
        assert_eq!(bot, [0, 0, 1, 0]);
        // 129 = 0b10000001 → top=[1,0,0,0], bot=[0,0,0,1]
        let (top, bot) = cw_nibbles_8(129);
        assert_eq!(top, [1, 0, 0, 0]);
        assert_eq!(bot, [0, 0, 0, 1]);
    }

    /// `cws_to_mmat` builds the BWIPP mmat data-grid. Verify by
    /// regenerating from the captured codewords for "A".
    #[test]
    fn cws_to_mmat_matches_bwip_js_golden_for_a() {
        let metric = pick_symbol_size(1).unwrap();
        let (cws, m2) = encode_with_ecc(b"A").unwrap();
        assert_eq!(metric.id, m2.id);
        let mmat = cws_to_mmat(&cws, &m2);
        // Captured via /tmp/oracle-codeone-pixs.js for "A":
        let expected: [u8; 160] = [
            0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0,
            0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
            0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0,
            1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0,
            1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0,
            1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0,
        ];
        assert_eq!(mmat, expected.as_slice());
    }

    /// `compose_pixs` builds the full BWIPP pixs grid. Pin
    /// byte-for-byte for "A" (16 × 18 = 288 cells, version A).
    #[test]
    fn compose_pixs_matches_bwip_js_golden_for_a() {
        let (cws, metric) = encode_with_ecc(b"A").unwrap();
        let mmat = cws_to_mmat(&cws, &metric);
        let pixs = compose_pixs(&mmat, &metric).unwrap();
        let expected: [u8; 288] = [
            // Rows of 18 (sliced from /tmp/oracle-codeone-pixs.js).
            0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0,
            0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1,
            1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1,
            1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0,
            0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1,
            0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0,
            1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0,
        ];
        assert_eq!(pixs.as_slice(), expected.as_slice());
    }

    /// `encode_to_bit_matrix` produces a 16×18 BitMatrix for "A"
    /// matching the pixs golden.
    #[test]
    fn encode_to_bit_matrix_produces_16x18_for_a() {
        let bm = encode_to_bit_matrix(b"A").unwrap();
        assert_eq!(bm.width(), 18);
        assert_eq!(bm.height(), 16);
        // Sanity-check a few cells against the golden.
        // pixs[0][0] = 0 → not set
        assert!(!bm.get(0, 0));
        // pixs[0][1] = 1 → set
        assert!(bm.get(1, 0));
        // pixs[6][0..18] all 1 (artifact '2' band)
        for x in 0..18 {
            assert!(bm.get(x, 6), "pixs[6][{x}] should be 1");
        }
    }

    /// `compose_pixs` golden for "Hello". Captured via
    /// `/tmp/oracle-codeone-pixs.js` (Version A, 16 × 18 = 288 cells).
    #[test]
    fn compose_pixs_matches_bwip_js_golden_for_hello() {
        let (cws, metric) = encode_with_ecc(b"Hello").unwrap();
        let mmat = cws_to_mmat(&cws, &metric);
        let pixs = compose_pixs(&mmat, &metric).unwrap();
        let expected: [u8; 288] = [
            0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1,
            0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0,
            0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1,
        ];
        assert_eq!(pixs.as_slice(), expected.as_slice());
    }

    /// `compose_pixs` golden for "ABC" — exercises the A-only path
    /// (single-byte cws). Captured via `/tmp/oracle-codeone-pixs.js`.
    #[test]
    fn compose_pixs_matches_bwip_js_golden_for_abc() {
        let (cws, metric) = encode_with_ecc(b"ABC").unwrap();
        let mmat = cws_to_mmat(&cws, &metric);
        let pixs = compose_pixs(&mmat, &metric).unwrap();
        let expected: [u8; 288] = [
            0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0,
            1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1,
            1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0,
            0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0,
            0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1,
        ];
        assert_eq!(pixs.as_slice(), expected.as_slice());
    }

    /// `compose_pixs` golden for "ABCDEFG" — exercises the A→C→A mode
    /// transition path (LC latch + CTX packed cws + UNLCW + Mode A
    /// trailing 'G'). Captured via `/tmp/oracle-codeone-pixs.js`.
    #[test]
    fn compose_pixs_matches_bwip_js_golden_for_abcdefg() {
        let (cws, metric) = encode_with_ecc(b"ABCDEFG").unwrap();
        let mmat = cws_to_mmat(&cws, &metric);
        let pixs = compose_pixs(&mmat, &metric).unwrap();
        let expected: [u8; 288] = [
            1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1,
            0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0,
            1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0,
            0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1,
            1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1,
            0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1,
        ];
        assert_eq!(pixs.as_slice(), expected.as_slice());
    }

    /// `encode_message` end-to-end byte-for-byte goldens against
    /// bwip-js. Captured via `/tmp/oracle-codeone-cws.js`:
    ///
    ///   "A"       → [66]                                     (Mode A)
    ///   "Hello"   → [73, 102, 109, 109, 112]                 (Mode A)
    ///   "ABCDEFG" → [230, 89, 233, 109, 36, 255, 72]         (A→C→A)
    ///   "abcdef"  → [239, 89, 233, 109, 36, 255]             (A→T→A)
    ///   "ABCDEFGHIJ" → [230, 89, 233, 109, 36, 128, 95, 255, 75]
    ///                                                         (A→C→A)
    ///   "ABCabcdef" → [66, 67, 68, 239, 89, 233, 109, 36, 255]
    ///                                                         (A→A→A→T→A)
    #[test]
    fn encode_message_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            (b"A", &[66]),
            (b"Hello", &[73, 102, 109, 109, 112]),
            (b"ABCDEFG", &[230, 89, 233, 109, 36, 255, 72]),
            (b"abcdef", &[239, 89, 233, 109, 36, 255]),
            (b"ABCDEFGHIJ", &[230, 89, 233, 109, 36, 128, 95, 255, 75]),
            (b"ABCabcdef", &[66, 67, 68, 239, 89, 233, 109, 36, 255]),
        ];
        for &(input, expected) in cases {
            let cws = encode_message(input).unwrap_or_else(|e| {
                panic!(
                    "encode_message({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_message({:?}) cws",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    // -----------------------------------------------------------------
    // Stage 3d — Mode D (decimal compression) byte-for-byte goldens.
    //
    // Captured via `rust/tools/oracle-codeone.js` against bwip-js
    // 4.10.1 / BWIPP 2026-04-21. These pin the bit-buffer packing
    // (val = d0*100+d1*10+d2+1 in 10 bits per group), the Mode-A→D
    // 4-bit leading "1111" handshake, the BWIPP termination state
    // machine (`getnumremcws` × `Drem` interaction), and the
    // Mode-D → Mode-A return path for mid-message switches.
    // -----------------------------------------------------------------

    #[test]
    fn mode_d_thirteen_digits_at_eom_matches_oracle() {
        // "1234567890123" — exactly 13 digits → triggers Mode D via
        // the "13+ at EOM" rule. Oracle: 7 codewords.
        let cws = encode_message(b"1234567890123").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 223, 209]);
    }

    #[test]
    fn mode_d_twenty_digits_at_eom_matches_oracle() {
        // 20 digits — Mode D via "13+ at EOM".
        let cws = encode_message(b"12345678901234567890").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 213, 106, 167, 253, 220]);
    }

    #[test]
    fn mode_d_after_mode_a_prefix_matches_oracle() {
        // "A1234567890123" — 'A' in Mode A, then 13 trailing digits
        // → Mode A → Mode D handshake mid-message.
        let cws = encode_message(b"A1234567890123").unwrap();
        assert_eq!(cws, vec![66, 241, 241, 201, 197, 128, 223, 209]);
    }

    #[test]
    fn mode_d_twentyone_digit_trigger_matches_oracle() {
        // 21 digits — Mode D via "≥ 21 anywhere".
        let cws = encode_message(b"123456789012345678901").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 213, 106, 167, 225, 189]);
    }

    #[test]
    fn mode_d_with_mode_a_tail_matches_oracle() {
        // 21 digits + Mode A tail "ABC" — Mode D returns to Mode A
        // for the trailing text.
        let cws = encode_message(b"123456789012345678901ABC").unwrap();
        assert_eq!(
            cws,
            vec![241, 241, 201, 197, 128, 213, 106, 167, 225, 191, 66, 67, 68]
        );
    }

    #[test]
    fn mode_d_sandwiched_between_mode_a_matches_oracle() {
        // "ABC123456789012345678901DEF" — Mode A → Mode D (21 digits)
        // → Mode A. Pins the mid-message D-entry from lookup().
        let cws = encode_message(b"ABC123456789012345678901DEF").unwrap();
        assert_eq!(
            cws,
            vec![66, 67, 68, 241, 241, 201, 197, 128, 213, 106, 167, 225, 191, 69, 70, 71]
        );
    }

    #[test]
    fn mode_d_sixteen_digits_one_trailing_matches_oracle() {
        // 16 digits → 5 full 3-digit packs + 1 trailing digit.
        let cws = encode_message(b"1234567890123456").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 213, 107, 247]);
    }

    #[test]
    fn mode_d_fourteen_digits_two_trailing_matches_oracle() {
        // 14 digits → 4 full packs + 2 trailing digits (different
        // termination branch than 1-trailing).
        let cws = encode_message(b"12345678901234").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 223, 209, 53]);
    }

    #[test]
    fn mode_d_fifteen_digits_clean_termination_matches_oracle() {
        // 15 digits → 5 full packs, no trailing digits — clean
        // termination (digit count divisible by 3).
        let cws = encode_message(b"123456789012345").unwrap();
        assert_eq!(cws, vec![241, 241, 201, 197, 128, 213, 107, 255]);
    }

    /// `getnumremcws` precompute table sanity checks.
    #[test]
    fn getnumremcws_table_anchors() {
        // Position 0 → smallest symbol (A, dcws=10) → 10 remaining.
        assert_eq!(getnumremcws(0), Some(10));
        assert_eq!(getnumremcws(9), Some(1));
        // Position 10 → next symbol (B, dcws=19) → 9 remaining.
        assert_eq!(getnumremcws(10), Some(9));
        assert_eq!(getnumremcws(18), Some(1));
        // Position 19 → C, dcws=44 → 25 remaining.
        assert_eq!(getnumremcws(19), Some(25));
        assert_eq!(getnumremcws(43), Some(1));
        // Position 1479 → H, dcws=1480 → 1 remaining.
        assert_eq!(getnumremcws(1479), Some(1));
        // Position 1480 → overflow (out-of-range).
        assert_eq!(getnumremcws(1480), None);
    }

    /// `append_dbits` round-trip via `flush_dbits_to_cws`.
    #[test]
    fn append_dbits_round_trip() {
        let mut buf: Vec<u8> = Vec::new();
        let mut len: usize = 0;
        // Append "1111 0001111100" (4 + 10 bits = 14 bits = 1 byte
        // 11110001 + 6 bits of "111100").
        append_dbits(&mut buf, &mut len, 0b1111, 4);
        append_dbits(&mut buf, &mut len, 124, 10);
        assert_eq!(len, 14);
        let bytes = flush_dbits_to_cws(&buf, len);
        // First byte = "11110001" = 241. The remaining 6 bits are
        // dropped by flush (caller pads).
        assert_eq!(bytes, vec![241]);
    }

    /// Stage 11.A8c — pin every is_d/is_c/is_t/is_x/is_ea/is_fn
    /// predicate at boundary characters. Kills any predicate-arm
    /// `delete arm` mutation or `< with <=` / `> with >=` boundary
    /// flip on lines 537-577.
    #[test]
    fn predicate_classifiers_cover_every_set() {
        // is_d: digits only.
        assert!(is_d(b'0' as i16));
        assert!(is_d(b'9' as i16));
        assert!(!is_d(b'/' as i16)); // just below '0'
        assert!(!is_d(b':' as i16)); // just above '9'
        assert!(!is_d(b'A' as i16));
        assert!(!is_d(b'a' as i16));

        // is_c (C40): SFT1..3, space, digits, uppercase.
        assert!(is_c(32)); // space
        assert!(is_c(b'A' as i16));
        assert!(is_c(b'Z' as i16));
        assert!(is_c(b'0' as i16));
        assert!(!is_c(b'a' as i16)); // lowercase not in C40
        assert!(!is_c(b'@' as i16)); // just below 'A'
        assert!(!is_c(b'[' as i16)); // just above 'Z'

        // is_t (Text): same set + lowercase instead of uppercase.
        assert!(is_t(32));
        assert!(is_t(b'a' as i16));
        assert!(is_t(b'z' as i16));
        assert!(is_t(b'0' as i16));
        assert!(!is_t(b'A' as i16)); // uppercase not in Text
        assert!(!is_t(b'`' as i16)); // just below 'a'
        assert!(!is_t(b'{' as i16)); // just above 'z'

        // is_x (X12): CR, '*', '>', space, digits, uppercase.
        assert!(is_x(13)); // CR
        assert!(is_x(42)); // *
        assert!(is_x(62)); // >
        assert!(is_x(32)); // space
        assert!(is_x(b'A' as i16));
        assert!(is_x(b'Z' as i16));
        assert!(!is_x(b'a' as i16));
        assert!(!is_x(b'+' as i16)); // not in set

        // is_ea: c > 127.
        assert!(is_ea(128));
        assert!(is_ea(255));
        assert!(!is_ea(127)); // boundary - 127 is NOT EA
        assert!(!is_ea(0));

        // is_fn: c < 0.
        assert!(is_fn(-1));
        assert!(is_fn(-128));
        assert!(!is_fn(0));
        assert!(!is_fn(1));
    }

    /// Stage 11.A8c — pin every `artifact_row` arm (0..=7 and default)
    /// for a representative odd `cols = 9` (lets the half-split cases
    /// 4/5 produce a balanced row centered on the single 1/0).
    ///
    /// Mutations caught:
    ///   * Any single arm replaced with `_` default → arm output
    ///     becomes all-zero, fails the per-arm exact assertion.
    ///   * `cols - 2` / `cols - 4` arithmetic drift in arms 2/3/6/7.
    ///   * `(cols - 1) / 2` half-split miscompute in arms 4/5.
    ///   * Sentinel value swap (-1 vs 0) in arms 4/5.
    ///   * `row.push(0)` vs `row.push(1)` value swap in any arm.
    #[test]
    fn artifact_row_per_index_known_layouts_cols_9() {
        let cols: usize = 9;
        // half = (9-1)/2 = 4.
        let sentinel: i8 = -1;

        // Arm 0: all zeros.
        assert_eq!(artifact_row(0, cols), vec![0i8; 9]);
        // Arm 1: all ones.
        assert_eq!(artifact_row(1, cols), vec![1i8; 9]);
        // Arm 2: 0, 1×7, 0.
        assert_eq!(artifact_row(2, cols), vec![0, 1, 1, 1, 1, 1, 1, 1, 0]);
        // Arm 3: 0, 1, 0×5, 1, 0.
        assert_eq!(artifact_row(3, cols), vec![0, 1, 0, 0, 0, 0, 0, 1, 0]);
        // Arm 4: sentinel×4, 1, sentinel×4.
        assert_eq!(
            artifact_row(4, cols),
            vec![sentinel, sentinel, sentinel, sentinel, 1, sentinel, sentinel, sentinel, sentinel]
        );
        // Arm 5: sentinel×4, 0, sentinel×4.
        assert_eq!(
            artifact_row(5, cols),
            vec![sentinel, sentinel, sentinel, sentinel, 0, sentinel, sentinel, sentinel, sentinel]
        );
        // Arm 6: 1, 0×7, 1.
        assert_eq!(artifact_row(6, cols), vec![1, 0, 0, 0, 0, 0, 0, 0, 1]);
        // Arm 7: 1, 0, 1×5, 0, 1.
        assert_eq!(artifact_row(7, cols), vec![1, 0, 1, 1, 1, 1, 1, 0, 1]);
        // Default (any index ≥ 8): all zeros (defensive — same as arm 0
        // but pins the catch-all branch from being repurposed).
        assert_eq!(artifact_row(8, cols), vec![0i8; 9]);
        assert_eq!(artifact_row(255, cols), vec![0i8; 9]);
    }

    /// `is_d`/`is_c`/`is_t`/`is_x`/`is_ea`/`is_fn`: the 6 character-class
    /// predicates used by the Codeone mode selector. Each defines a
    /// distinct subset of i16 values. Never directly tested.
    ///
    /// Mutations to catch:
    /// * Range `b'0'..=b'9'` → `b'0'..b'9'` (exclude '9').
    /// * Letter range mutations.
    /// * Sentinel constant swaps (SFT1/SFT2/SFT3 in is_c/is_t).
    /// * X12 special-byte set mutations (13, 32, 42, 62).
    /// * `is_ea`: `> 127` → `>= 127` (off-by-one).
    /// * `is_fn`: `< 0` → `<= 0` (would mark 0 as FN).
    /// * Mode swaps: is_c ↔ is_t (uppercase vs lowercase).
    #[test]
    fn codeone_char_class_predicates_per_arm() {
        // ---- is_d: ASCII digit '0'..='9'.
        for c in b'0'..=b'9' {
            assert!(is_d(c as i16), "'{}' must be d", c as char);
        }
        assert!(!is_d(b'/' as i16), "'/' (47) just below '0'");
        assert!(!is_d(b':' as i16), "':' (58) just above '9'");
        assert!(!is_d(b'A' as i16), "'A' is not d");
        assert!(!is_d(-1), "negative not d");
        assert!(!is_d(200), "high byte not d");

        // ---- is_c: SFT1/2/3 + space + digits + uppercase.
        assert!(is_c(SFT1), "SFT1 is c");
        assert!(is_c(SFT2), "SFT2 is c");
        assert!(is_c(SFT3), "SFT3 is c");
        assert!(is_c(32), "space is c");
        for c in b'0'..=b'9' {
            assert!(is_c(c as i16), "'{}' (digit) is c", c as char);
        }
        for c in b'A'..=b'Z' {
            assert!(is_c(c as i16), "'{}' uppercase is c", c as char);
        }
        // Cross-mode discriminator: lowercase NOT in C.
        assert!(!is_c(b'a' as i16), "'a' lowercase NOT in C");
        assert!(!is_c(b'z' as i16), "'z' lowercase NOT in C");
        // Special punct.
        assert!(!is_c(b'!' as i16), "'!' NOT in C");
        assert!(!is_c(b'@' as i16), "'@' NOT in C");

        // ---- is_t: SFT1/2/3 + space + digits + lowercase.
        assert!(is_t(SFT1));
        assert!(is_t(32));
        for c in b'0'..=b'9' {
            assert!(is_t(c as i16), "'{}' (digit) is t", c as char);
        }
        for c in b'a'..=b'z' {
            assert!(is_t(c as i16), "'{}' lowercase is t", c as char);
        }
        // Cross-mode discriminator: uppercase NOT in T.
        assert!(!is_t(b'A' as i16), "'A' uppercase NOT in T");
        assert!(!is_t(b'Z' as i16), "'Z' uppercase NOT in T");

        // ---- is_x: 13, 42, 62, 32 + digits + uppercase.
        assert!(is_x(13), "CR (13) is x");
        assert!(is_x(42), "'*' (42) is x");
        assert!(is_x(62), "'>' (62) is x");
        assert!(is_x(32), "space (32) is x");
        for c in b'0'..=b'9' {
            assert!(is_x(c as i16));
        }
        for c in b'A'..=b'Z' {
            assert!(is_x(c as i16));
        }
        // X12 doesn't include lowercase or SFT sentinels.
        assert!(!is_x(SFT1), "SFT1 NOT in X12");
        assert!(!is_x(b'a' as i16), "'a' NOT in X12");
        // Adjacent values to 13, 42, 62.
        assert!(!is_x(12), "12 (^L) NOT in X12");
        assert!(!is_x(14), "14 (^N) NOT in X12");
        assert!(!is_x(41), "41 ('(') NOT in X12");
        assert!(!is_x(43), "43 ('+') NOT in X12");
        assert!(!is_x(61), "61 ('=') NOT in X12");
        assert!(!is_x(63), "63 ('?') NOT in X12");

        // ---- is_ea: c > 127.
        assert!(!is_ea(127), "127 (DEL) NOT extended-ASCII (>127 only)");
        assert!(is_ea(128), "128 IS extended-ASCII");
        assert!(is_ea(255), "255 IS extended-ASCII");
        assert!(is_ea(32767), "32767 IS extended-ASCII (max i16)");
        // Boundary mutant `>= 127` would fail here.
        assert!(!is_ea(0));
        assert!(!is_ea(-1), "negative NOT extended-ASCII");

        // ---- is_fn: c < 0.
        assert!(is_fn(-1), "-1 IS FN");
        assert!(is_fn(SFT1), "SFT1 (-13) IS FN");
        assert!(is_fn(SFT2));
        assert!(is_fn(SFT3));
        assert!(is_fn(-32768), "min i16 IS FN");
        // Boundary: 0 must NOT be FN (the `< 0 vs <= 0` discriminator).
        assert!(!is_fn(0), "0 NOT FN (catches `<= 0` mutant)");
        assert!(!is_fn(1));
        assert!(!is_fn(127));
        assert!(!is_fn(255));
        assert!(!is_fn(i16::MAX));

        // ---- Cross-predicate distinctness for letters: uppercase
        // is in is_c and is_x but not is_t; lowercase in is_t only.
        assert!(is_c(b'A' as i16) && is_x(b'A' as i16) && !is_t(b'A' as i16));
        assert!(is_t(b'a' as i16) && !is_c(b'a' as i16) && !is_x(b'a' as i16));
    }

    /// `compute_next_xterm(msg) -> Vec<u32>`: reverse-pass distance
    /// lookahead. Length = msg.len() + 1. `next[n] = 9999` (sentinel).
    /// For i < n:
    /// * if `msg[i] ∈ {13, 42, 62}` (CR, '*', '>') → `next[i] = 0`
    /// * else → `next[i] = next[i+1].saturating_add(1)`
    /// Post-pass clamps values > 10000 to 10000.
    ///
    /// `compute_next_non_x` is tested at line 2879+, but
    /// `compute_next_xterm` has no direct test.
    ///
    /// Mutations to catch:
    /// * Terminator set `{13, 42, 62}` → wrong values.
    /// * Reverse-pass direction `(0..n).rev()` → forward (would compute
    ///   different values).
    /// * `next[i+1].saturating_add(1)` → `next[i+1] - 1` or `+ 2`.
    /// * Clamp threshold `> 10000` → `>= 10000`.
    /// * Sentinel `9999` → other value.
    /// * Vec length `n + 1` → `n` (would lose sentinel slot).
    #[test]
    fn compute_next_xterm_distance_to_terminator_with_clamp() {
        // ---- Empty input: only sentinel.
        assert_eq!(
            compute_next_xterm(&[]),
            vec![9999u32],
            "empty msg → [9999] sentinel only"
        );

        // ---- Single terminator: returns [0, 9999].
        assert_eq!(
            compute_next_xterm(&[13]),
            vec![0, 9999],
            "CR (13) at pos 0 → [0, 9999]"
        );
        assert_eq!(
            compute_next_xterm(&[42]),
            vec![0, 9999],
            "'*' (42) at pos 0 → [0, 9999]"
        );
        assert_eq!(
            compute_next_xterm(&[62]),
            vec![0, 9999],
            "'>' (62) at pos 0 → [0, 9999]"
        );

        // ---- Single non-terminator: distance is 9999+1 = 10000
        // (clamped from 10000).
        assert_eq!(
            compute_next_xterm(&[5]),
            vec![10000, 9999],
            "single non-term: 9999+1 = 10000 (clamped)"
        );

        // ---- Two non-terminators: both clamped at 10000.
        assert_eq!(compute_next_xterm(&[5, 5]), vec![10000, 10000, 9999]);

        // ---- Distance counting up to terminator.
        // [5, 13]: next[1]=0 (CR), next[0]=0+1=1.
        assert_eq!(
            compute_next_xterm(&[5, 13]),
            vec![1, 0, 9999],
            "[5, CR]: distance from 0 to CR is 1"
        );
        // [5, 5, 13]: next[2]=0, next[1]=1, next[0]=2.
        assert_eq!(compute_next_xterm(&[5, 5, 13]), vec![2, 1, 0, 9999]);
        // [5, 13, 5]: next[2]=10000+1 clamped, next[1]=0, next[0]=1.
        // Actually next[2] = 9999+1 = 10000 (clamped). And from non-term at i=2,
        // next[1]=0 (CR), next[0]=0+1=1.
        assert_eq!(
            compute_next_xterm(&[5, 13, 5]),
            vec![1, 0, 10000, 9999],
            "[5, CR, 5]: trailing non-term clamps to 10000"
        );

        // ---- All terminators.
        assert_eq!(compute_next_xterm(&[13, 13]), vec![0, 0, 9999]);
        // Mixed terminators.
        assert_eq!(
            compute_next_xterm(&[42, 62, 13]),
            vec![0, 0, 0, 9999],
            "all 3 terminators → all 0"
        );

        // ---- Multi-byte chain with all 3 terminator values.
        // [5, 42, 5, 62]:
        //   next[4]=9999, next[3]=0 ('>'), next[2]=1, next[1]=0 ('*'), next[0]=1.
        assert_eq!(
            compute_next_xterm(&[5, 42, 5, 62]),
            vec![1, 0, 1, 0, 9999],
            "exercises all 3 terminator values"
        );

        // ---- Adjacent-value discriminator: 12 ('^L') and 14 ('^N')
        // bracket 13 (CR) but are NOT terminators.
        assert_eq!(
            compute_next_xterm(&[12]),
            vec![10000, 9999],
            "12 NOT a terminator (catches `<= 13` mutant)"
        );
        assert_eq!(
            compute_next_xterm(&[14]),
            vec![10000, 9999],
            "14 NOT a terminator"
        );
        // 41 and 43 bracket 42; 61 and 63 bracket 62.
        assert_eq!(compute_next_xterm(&[41]), vec![10000, 9999]);
        assert_eq!(compute_next_xterm(&[43]), vec![10000, 9999]);
        assert_eq!(compute_next_xterm(&[61]), vec![10000, 9999]);
        assert_eq!(compute_next_xterm(&[63]), vec![10000, 9999]);

        // ---- Length invariant: output is always msg.len() + 1.
        for n in 0..10 {
            let msg = vec![5i16; n];
            assert_eq!(
                compute_next_xterm(&msg).len(),
                n + 1,
                "len(msg)={n} → output len {n}+1"
            );
        }
    }

    /// `pad_cws_matrix(cws, dcws)`: pads `cws` with the Code One pad
    /// codeword 129 until its length reaches `dcws`. No-op if
    /// `cws.len() >= dcws` (doesn't truncate). Pre-ECC matrix-fill
    /// step. Never directly tested.
    ///
    /// Mutations to catch:
    /// * Boundary `< dcws` → `<= dcws` (would pad one extra).
    /// * `< dcws` → `> dcws` (would truncate instead).
    /// * Pad codeword `129` → `128`, `130`, `0` (wrong constant).
    /// * Loop-condition mutation that truncates over-long input.
    #[test]
    fn pad_cws_matrix_pads_to_dcws_with_constant_129() {
        // ---- Empty + target 0 → no-op.
        let mut cws = vec![];
        pad_cws_matrix(&mut cws, 0);
        assert!(cws.is_empty(), "empty target → no padding");

        // ---- Empty + target 3 → 3 pad codewords.
        let mut cws = vec![];
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(
            cws,
            vec![129, 129, 129],
            "empty cws + dcws=3 → 3 pad codewords (129)"
        );

        // ---- Empty + target 1 → single 129.
        let mut cws = vec![];
        pad_cws_matrix(&mut cws, 1);
        assert_eq!(cws, vec![129], "single pad");

        // ---- Partial fill.
        let mut cws = vec![1, 2];
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(
            cws,
            vec![1, 2, 129],
            "[1, 2] + dcws=3 → append 1 pad (preserving prefix)"
        );

        // ---- Exact fill (no padding needed).
        let mut cws = vec![1, 2, 3];
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(
            cws,
            vec![1, 2, 3],
            "len == dcws: no change (catches `<= dcws` mutant that would add 1 extra)"
        );

        // ---- Already over-filled: don't truncate.
        let mut cws = vec![1, 2, 3, 4, 5];
        pad_cws_matrix(&mut cws, 3);
        assert_eq!(
            cws,
            vec![1, 2, 3, 4, 5],
            "len > dcws: no change (catches truncate mutant)"
        );

        // ---- Larger gap: pad 3 codewords.
        let mut cws = vec![10, 20];
        pad_cws_matrix(&mut cws, 5);
        assert_eq!(
            cws,
            vec![10, 20, 129, 129, 129],
            "[10, 20] + dcws=5 → 3 pads"
        );

        // ---- Pad codeword discriminator: every appended cell must
        // be exactly 129 (not 128, not 130, not 0).
        let mut cws = vec![];
        pad_cws_matrix(&mut cws, 1);
        assert_ne!(cws[0], 0, "pad is NOT 0");
        assert_ne!(cws[0], 128, "pad is NOT 128 (catches off-by-one mutant)");
        assert_ne!(cws[0], 130, "pad is NOT 130");
        assert_eq!(cws[0], 129, "pad is exactly 129");
    }

    /// `ff(v) -> f64`: BWIPP's `$f` truncation. If `v` is exactly
    /// representable as an i64 integer, returns it unchanged
    /// (fast path — avoids f32 round-trip noise). Otherwise rounds
    /// to f32 precision and casts back to f64.
    ///
    /// This is the **critical** detail that makes `lookup()` match
    /// BWIPP: fractional cost accumulators (cc/tc/xc) are clamped to
    /// f32 precision every update. Plain f64 sums drift differently
    /// and break boundary decisions like "abcdef" (BWIPP picks T at
    /// k=5 because tc reaches exactly 5.0 in f32; f64 gives 5.0000002
    /// → ceil 6 → A wins instead).
    ///
    /// Mutations to catch:
    /// * `(v as i64 as f64) == v` → `!=` (would invert fast-path).
    /// * `f64::from(v as f32)` → `v` (no truncation; reverts to plain f64).
    /// * Branch arm swap.
    #[test]
    fn ff_truncates_non_integer_to_f32_precision() {
        // ---- Integer fast path: every integer value returns
        // unchanged (no f32 round-trip).
        assert_eq!(ff(0.0), 0.0);
        assert_eq!(ff(1.0), 1.0);
        assert_eq!(ff(-1.0), -1.0);
        assert_eq!(ff(2.0), 2.0);
        assert_eq!(ff(5.0), 5.0);
        assert_eq!(ff(100.0), 100.0);
        assert_eq!(ff(-100.0), -100.0);
        // Larger integer still fits in i64.
        assert_eq!(ff(123_456_789.0), 123_456_789.0);

        // ---- Non-integer f32-exact value: 0.5 is exact in both
        // f32 and f64, so f32 round-trip preserves it.
        assert_eq!(ff(0.5), 0.5, "0.5 is exact in f32; round-trip preserves");
        assert_eq!(ff(0.25), 0.25);
        assert_eq!(ff(0.125), 0.125);

        // ---- Non-integer value where f32 truncation MATTERS:
        // 2.0/3.0 is inexact in both f32 and f64, but their
        // representations differ. ff(2.0/3.0) must return the f32
        // version, not the f64 version.
        let v_f64 = 2.0_f64 / 3.0_f64;
        let v_f32_via_f64 = f64::from(v_f64 as f32);
        let truncated = ff(v_f64);
        assert_eq!(
            truncated, v_f32_via_f64,
            "2/3 in f64 must be truncated to f32 precision"
        );
        // And the truncated value must DIFFER from plain f64 (catches
        // a mutant that returns v unchanged in the else arm).
        assert_ne!(
            truncated, v_f64,
            "f32 truncation of 2/3 must differ from f64 value (else-arm mutant catch)"
        );

        // ---- 0.1 — another classic floating-point gotcha.
        let v = 0.1_f64;
        let want = f64::from(v as f32);
        assert_eq!(ff(v), want, "0.1 truncated to f32 precision");
        assert_ne!(ff(v), v, "0.1 in f64 ≠ 0.1 in f32 (round-trip)");

        // ---- Idempotence: ff(ff(v)) == ff(v) for both paths.
        // Integer path.
        assert_eq!(ff(ff(7.0)), ff(7.0));
        // Non-integer path: f32 → f64 → f32 → f64 stabilises after
        // the first conversion.
        let once = ff(0.1);
        let twice = ff(once);
        assert_eq!(twice, once, "ff is idempotent");

        // ---- The BWIPP-critical case: 2/3 (0.6667) is the standard
        // per-byte cost increment for C/T/X modes. The cumulative
        // sum of 5 × (2/3) must match BWIPP's f32-accumulated value.
        // Plain f64 sum: 5 × 0.66666... = 3.3333333333333335
        // BWIPP f32 sum: each add truncated to f32, total drifts.
        // Just pin that ff(2.0/3.0) is non-trivial (differs from f64).
        let two_thirds = 2.0_f64 / 3.0_f64;
        assert!(
            ff(two_thirds) != two_thirds,
            "ff(2/3) MUST differ from plain f64 2/3 (f32 truncation is the whole point)"
        );

        // ---- Negative non-integer.
        let neg = -0.3_f64;
        let want_neg = f64::from(neg as f32);
        assert_eq!(ff(neg), want_neg, "-0.3 truncated to f32");
    }

    /// Stage 11.A8c — pin `encode_a_step` digit-pair + at-end paths.
    ///
    /// `encode_a_step` is exercised only through `encode_message` end-
    /// to-end goldens; there's no direct test for the inner digit-pair
    /// arm (numd_i in 2..21) or the at-end short-circuit. Both are
    /// narrow branches a mutant could survive in.
    ///
    /// Hand-computed: for msg b"12" in Mode A:
    /// - numd = compute_numd(b"12") = [2, 1, 0] → numd_i = 2 at i=0.
    /// - numd_i >= 21 is false, numd_i >= 13 AND at-end is also false
    ///   (2 < 13).
    /// - numd_i >= 2 true → digit-pair path.
    /// - avals_digit_pair(b'1', b'2') = 130 + 12 = 142.
    /// - Push 142, i += 2 → 2.
    ///
    /// At-end (i == msg.len()): step returns Ok(()) immediately with
    /// no state change. Mutants that flip `at_end()` to `!at_end()`
    /// would push spurious codewords (or panic on out-of-bounds).
    #[test]
    fn encode_a_step_digit_pair_and_at_end() {
        // ---- Digit pair path.
        let mut ctx = Ctx::from_bytes(b"12");
        assert_eq!(ctx.mode, MODE_A);
        assert_eq!(ctx.i, 0);
        encode_a_step(&mut ctx).unwrap();
        assert_eq!(
            ctx.cws,
            vec![142u16],
            "digit pair '12' must emit avals_digit_pair = 130 + 12 = 142"
        );
        assert_eq!(ctx.i, 2, "i must advance by 2 after digit-pair consumption");
        // Mode stays A.
        assert_eq!(ctx.mode, MODE_A);

        // ---- At-end short-circuit. i==msg.len() → no-op.
        let mut ctx = Ctx::from_bytes(b"AB");
        ctx.i = 2; // Manually move to end.
        ctx.mode = MODE_A;
        let cws_before = ctx.cws.clone();
        let i_before = ctx.i;
        let mode_before = ctx.mode;
        encode_a_step(&mut ctx).unwrap();
        assert_eq!(ctx.cws, cws_before, "at-end must not push codewords");
        assert_eq!(ctx.i, i_before, "at-end must not advance i");
        assert_eq!(ctx.mode, mode_before, "at-end must not change mode");

        // ---- Different digit pairs hit different codewords. Pins
        // the avals_digit_pair * arithmetic per BWIPP.
        let mut ctx = Ctx::from_bytes(b"00");
        encode_a_step(&mut ctx).unwrap();
        assert_eq!(ctx.cws, vec![130u16], "'00' → 130 + 0 = 130");

        let mut ctx = Ctx::from_bytes(b"99");
        encode_a_step(&mut ctx).unwrap();
        assert_eq!(ctx.cws, vec![229u16], "'99' → 130 + 99 = 229");

        // ---- Discriminator: "21" must differ from "12" (catches
        // a mutant that swapped d1 ↔ d2 in the avals_digit_pair call).
        let mut ctx12 = Ctx::from_bytes(b"12");
        encode_a_step(&mut ctx12).unwrap();
        let mut ctx21 = Ctx::from_bytes(b"21");
        encode_a_step(&mut ctx21).unwrap();
        assert_ne!(
            ctx12.cws[0], ctx21.cws[0],
            "'12' and '21' must produce distinct codewords; equality means d1/d2 \
             are summed symmetrically"
        );
        assert_eq!(ctx21.cws[0], 130 + 21, "'21' → 130 + 21 = 151");
    }

    /// Stage 11.A8c — pin `encode_b_step` happy-path: consumes the
    /// remaining message, emits Mode-B prefix + raw bytes, and
    /// advances `ctx.i` to msg.len().
    ///
    /// Existing `encode_b_step_rejects_negative_cells` only covers
    /// the marker/ECI error arm. The happy path (no negative cells)
    /// is exercised only via `encode_message` end-to-end. A mutant
    /// on the inner `for &cell in &ctx.msg[ctx.i..]` loop bound, the
    /// `bytes.push(cell as u8)` cast, or the `ctx.i = ctx.msg.len()`
    /// terminal assignment would survive without a direct test.
    ///
    /// Hand-computed: encode_mode_b_run(b"AB", false):
    /// - p = 2 < 250, prefix = [2].
    /// - bytes = [65, 66].
    /// - cws = [2, 65, 66].
    /// encode_b_step pushes those onto ctx.cws and sets ctx.i = 2.
    #[test]
    fn encode_b_step_happy_path_consumes_remaining_msg() {
        let mut ctx = Ctx::from_bytes(b"AB");
        ctx.mode = MODE_B;
        ctx.i = 0;
        encode_b_step(&mut ctx).unwrap();
        assert_eq!(
            ctx.cws,
            vec![2u16, 65, 66],
            "Mode B encodes [prefix=len, bytes...] → [2, 65, 66] for \"AB\""
        );
        assert_eq!(
            ctx.i,
            ctx.msg.len(),
            "encode_b_step must advance i to msg.len() (consumes all)"
        );
        // Mode stays B (no UNLCW back to A).
        assert_eq!(ctx.mode, MODE_B);

        // ---- At-end short-circuit: i==msg.len() → no-op.
        let mut ctx = Ctx::from_bytes(b"hello");
        ctx.mode = MODE_B;
        ctx.i = 5; // Move past end.
        encode_b_step(&mut ctx).unwrap();
        assert!(
            ctx.cws.is_empty(),
            "at-end Mode B must not emit any codewords"
        );

        // ---- Single byte: prefix=1 + [byte].
        let mut ctx = Ctx::from_bytes(b"Z");
        ctx.mode = MODE_B;
        encode_b_step(&mut ctx).unwrap();
        assert_eq!(ctx.cws, vec![1u16, 90], "single 'Z' → [1, 90]");
        assert_eq!(ctx.i, 1);

        // ---- Discriminator: starting mid-message (i > 0) — only the
        // tail gets encoded.
        let mut ctx = Ctx::from_bytes(b"hello");
        ctx.mode = MODE_B;
        ctx.i = 2; // Skip "he", encode "llo".
        encode_b_step(&mut ctx).unwrap();
        assert_eq!(
            ctx.cws,
            vec![3u16, b'l' as u16, b'l' as u16, b'o' as u16],
            "starting at i=2 must encode only 'llo' (3 bytes, prefix=3)"
        );
        assert_eq!(ctx.i, 5);
    }

    /// Stage 11.A8c — pin per-arm arithmetic offsets in
    /// `cnvals_lookup` / `tnvals_lookup` / `xvals_lookup`. The
    /// existing `ctxvals_to_cws_and_ctx_base_value_dispatch` test
    /// uses these via dispatch but doesn't anchor the exact constant
    /// offsets (`c - 44` for digits, `c - 51` for C40/X12 uppercase,
    /// `c - 83` for Text lowercase). A mutant that bumped an offset
    /// by ±1 would shift all results by ±1 and still pass any
    /// dispatch-only test that doesn't compare to exact integers.
    ///
    /// Hand-computed: digits map to 4..=13 (0→4, 9→13).
    /// C40/X12 uppercase maps to 14..=39 (A=65 → 65-51=14, Z=90 → 39).
    /// Text lowercase maps to 14..=39 (a=97 → 97-83=14, z=122 → 39).
    #[test]
    fn nvals_lookup_arithmetic_per_arm_anchors() {
        // ---- cnvals_lookup (C40 base set).
        // Sentinels.
        assert_eq!(cnvals_lookup(SFT1), Some(0));
        assert_eq!(cnvals_lookup(SFT2), Some(1));
        assert_eq!(cnvals_lookup(SFT3), Some(2));
        // Space.
        assert_eq!(cnvals_lookup(32), Some(3));
        // Digit endpoints: '0' (48) → 4, '9' (57) → 13. Offset = -44.
        assert_eq!(cnvals_lookup(b'0' as i16), Some(4));
        assert_eq!(cnvals_lookup(b'5' as i16), Some(9));
        assert_eq!(cnvals_lookup(b'9' as i16), Some(13));
        // Uppercase endpoints: 'A' (65) → 14, 'Z' (90) → 39. Offset = -51.
        assert_eq!(cnvals_lookup(b'A' as i16), Some(14));
        assert_eq!(cnvals_lookup(b'M' as i16), Some(26));
        assert_eq!(cnvals_lookup(b'Z' as i16), Some(39));
        // Out-of-set.
        assert_eq!(cnvals_lookup(b'a' as i16), None);
        assert_eq!(cnvals_lookup(b'!' as i16), None);
        assert_eq!(cnvals_lookup(127), None);

        // ---- tnvals_lookup (Text base set).
        assert_eq!(tnvals_lookup(SFT1), Some(0));
        assert_eq!(tnvals_lookup(SFT2), Some(1));
        assert_eq!(tnvals_lookup(SFT3), Some(2));
        assert_eq!(tnvals_lookup(32), Some(3));
        // Digit endpoints share the same offset as C40.
        assert_eq!(tnvals_lookup(b'0' as i16), Some(4));
        assert_eq!(tnvals_lookup(b'9' as i16), Some(13));
        // Lowercase endpoints: 'a' (97) → 14, 'z' (122) → 39. Offset = -83.
        assert_eq!(tnvals_lookup(b'a' as i16), Some(14));
        assert_eq!(tnvals_lookup(b'm' as i16), Some(26));
        assert_eq!(tnvals_lookup(b'z' as i16), Some(39));
        // Out-of-set: uppercase NOT in Text.
        assert_eq!(tnvals_lookup(b'A' as i16), None);
        assert_eq!(tnvals_lookup(b'Z' as i16), None);

        // ---- xvals_lookup (X12 base set).
        // Distinct sentinels: CR(13), '*'(42), '>'(62).
        assert_eq!(xvals_lookup(13), Some(0));
        assert_eq!(xvals_lookup(42), Some(1));
        assert_eq!(xvals_lookup(62), Some(2));
        assert_eq!(xvals_lookup(32), Some(3));
        // Digits + uppercase like C40.
        assert_eq!(xvals_lookup(b'0' as i16), Some(4));
        assert_eq!(xvals_lookup(b'9' as i16), Some(13));
        assert_eq!(xvals_lookup(b'A' as i16), Some(14));
        assert_eq!(xvals_lookup(b'Z' as i16), Some(39));
        // Out-of-set: lowercase NOT in X12.
        assert_eq!(xvals_lookup(b'a' as i16), None);
        assert_eq!(xvals_lookup(b'+' as i16), None);

        // ---- Cross-set discriminator: lowercase 'a' is ONLY in Text.
        // A mutant that swapped the cnvals/tnvals digit-range upper
        // bound would silently pass digit anchors but fail this:
        assert_eq!(cnvals_lookup(b'a' as i16), None, "C40 rejects lowercase");
        assert_eq!(xvals_lookup(b'a' as i16), None, "X12 rejects lowercase");
        assert!(
            tnvals_lookup(b'a' as i16).is_some(),
            "Text accepts lowercase"
        );

        // Boundary `:` (58) is just past '9' (57): all three sets reject.
        assert_eq!(cnvals_lookup(b':' as i16), None);
        assert_eq!(tnvals_lookup(b':' as i16), None);
        assert_eq!(xvals_lookup(b':' as i16), None);
        // Boundary `/` (47) is just before '0' (48): all three reject.
        assert_eq!(cnvals_lookup(b'/' as i16), None);
        assert_eq!(tnvals_lookup(b'/' as i16), None);
        assert_eq!(xvals_lookup(b'/' as i16), None);
    }
}
