//! POSICODE — linear bar-code symbology (BWIPP `posicode`).
//!
//! The full BWIPP encoder spans four versions:
//!
//! * `"a"` — default version, three-set alphanumeric + extended ASCII
//!   with latches (LA0/LA1/LA2) and shifts (SF0/SF1/SF2) between sets.
//! * `"b"` — same alphabet as `a`, different bar-pattern tables (each
//!   codeword's pattern is one module wider).
//! * `"limiteda"` — single-set alphanumeric (`0..9`, `A..Z`, `-`,
//!   `.`). No latches or shifts. Smallest module count.
//! * `"limitedb"` — same alphabet as `limiteda`, different bar-pattern
//!   tables.
//!
//! BWIPP source: bwip-js `dist/bwip-js-node.js` lines 18064-18545
//! (`bwipp_posicode`). The encoder pipeline is:
//!
//!   1. Parse `barcode` input through the FNC parser (FNC1/2/3
//!      mapped to internal sentinel values per the
//!      `POSICODE_CHARMAPSNORMAL` table below).
//!   2. Auto-encoding mode selector: walk the input one byte at a
//!      time, picking the cheapest set-0 / set-1 / set-2 transition.
//!      For each step emit either the byte's codeword in the active
//!      set, or a shift (`SF*`) followed by the byte, or a latch
//!      (`LA*`) and switch the active set. The result is `cws` —
//!      the codeword stream (each codeword 0..=45).
//!   3. Checksum: a 16-bit CRC-like accumulator over the codeword
//!      bit-stream, normalised differently for normal vs limited
//!      versions, then decomposed into 6 weight-table indices that
//!      become the 6 odd-position bars of a 12-module check pattern.
//!   4. SBS emission: `encs[length-2]` (start pattern) + per-codeword
//!      6-module patterns + 12-module check pattern + `encs[length-1]`
//!      (stop pattern). Pattern characters encode bar/space widths as
//!      `ASCII_char - 48`.
//!
//! **Current status (Stage 22d)**: all four versions are
//! byte-for-byte verified against bwip-js. The single-set
//! variants `"limiteda"` (Stage 22b) and `"limitedb"` (Stage
//! 22c.1) use the private `encode_limited` helper. The multi-set
//! variants `"a"` and `"b"` (Stage 22d, this revision) use the
//! private `encode_normal` helper, which ports the full BWIPP
//! auto-encoder state machine: set-0/1/2 three-way lookup,
//! LA1/LA0 latches, SF1/SF0/SF2 single-character shifts, and
//! FN4-based ASCII ↔ extended-ASCII transitions via the private
//! `insert_fn4_markers` pre-encoder pass. `posicode` is tracked
//! in PORT_STATUS as **verified**.
//!
//! The only BWIPP-supported POSICODE knob still pending is the
//! `parsefnc` option (which enables `^FNC1`/`^FNC2`/`^FNC3`
//! escape recognition by the input parser). That option must be
//! opted into by the caller via `opts.extras["parsefnc"]` and is
//! not part of the default encoder path.

// Some of the constants below (the three-set charmap rows, the latch
// / shift / FNC sentinels, the normal-version weight columns) are
// only consumed by the Stage-22d auto-encoder for versions `a`
// and `b`, which is still in progress. Suppress dead-code warnings
// module-wide until that lands; the auto-encoder will reach into
// them and the allow can be lifted then.
#![allow(dead_code)]

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

// ---------------------------------------------------------------------------
// Internal sentinel constants — BWIPP `posicode_la*` / `posicode_sf*` /
// `posicode_fn*` (bwip-js lines 18047-18053).
//
// These appear as the table values for latch / shift / FNC rows so a
// charmap can mix literal byte values with the sentinel constants
// without needing a separate marker type. The mode selector emits
// them as codewords directly when latching / shifting / emitting a
// FNC.
// ---------------------------------------------------------------------------

/// Latch to set 0.
pub(crate) const POSICODE_LA0: i16 = -1;
/// Latch to set 1.
pub(crate) const POSICODE_LA1: i16 = -2;
/// Latch to set 2.
pub(crate) const POSICODE_LA2: i16 = -3;
/// Shift to set 0 (one-character shift).
pub(crate) const POSICODE_SF0: i16 = -4;
/// Shift to set 1 (one-character shift).
pub(crate) const POSICODE_SF1: i16 = -5;
/// Shift to set 2 (one-character shift).
pub(crate) const POSICODE_SF2: i16 = -6;
/// FNC1 marker (GS1 separator).
pub(crate) const POSICODE_FN1: i16 = -7;
/// FNC2 marker.
pub(crate) const POSICODE_FN2: i16 = -8;
/// FNC3 marker.
pub(crate) const POSICODE_FN3: i16 = -9;
/// FNC4 marker (high-bit shift).
pub(crate) const POSICODE_FN4: i16 = -10;

/// Sentinel used by BWIPP's limited charmap to denote "no entry in
/// set 1 / set 2 for this row" (bwip-js line 18092). Encoded as -98
/// in the literal table so a value lookup against the limited
/// charmap never matches a real byte.
pub(crate) const LIMITED_NA: i16 = -98;

// ---------------------------------------------------------------------------
// POSICODE_CHARMAPSNORMAL — 46 rows × 3 columns (set 0 / set 1 /
// set 2). Direct port of BWIPP `posicode_charmapsnormal` (bwip-js
// line 18076).
//
// Row layout per BWIPP:
//   0..=9   digits '0'..'9' (set 0) ↔ punctuation (set 1) ↔ control
//           bytes 27..31 + '!' / '"' / '#' / '&' (set 2).
//   10..=35 uppercase 'A'..'Z' (set 0) ↔ lowercase 'a'..'z' (set 1) ↔
//           control bytes 1..26 (set 2).
//   36..=37 '-'/'.' shared by sets 0 and 1; set 2 has 40/41.
//   38      space / DEL / 0.
//   39..=41 '$'/'/'/+'/'%' (set 0) ↔ '{'/'|'/'}'/'~' (set 1) ↔
//           '*'/','/':'/FN1 (set 2).
//   42..=45 mode-control rows: latches + shifts + FNCs, each row's
//           value is one of the sentinel constants above.
// ---------------------------------------------------------------------------

/// Three-column charmap for versions `"a"` and `"b"`. Each row is a
/// `[set0, set1, set2]` triple. Values are either positive byte
/// values (0..=127) or one of the negative sentinel constants
/// (`POSICODE_LA*` / `POSICODE_SF*` / `POSICODE_FN*`).
#[rustfmt::skip]
pub(crate) const POSICODE_CHARMAPSNORMAL: [[i16; 3]; 46] = [
    // 0..=9: digits in set 0.
    [b'0' as i16, b'^' as i16, b'\'' as i16],
    [b'1' as i16, b';' as i16, 27],
    [b'2' as i16, b'<' as i16, 28],
    [b'3' as i16, b'=' as i16, 29],
    [b'4' as i16, b'>' as i16, 30],
    [b'5' as i16, b'?' as i16, 31],
    [b'6' as i16, b'@' as i16, b'!' as i16],
    [b'7' as i16, b'[' as i16, b'"' as i16],
    [b'8' as i16, 92,           b'#' as i16],   // 92 = '\\'
    [b'9' as i16, b']' as i16, b'&' as i16],

    // 10..=35: A..Z in set 0; a..z in set 1; control bytes 1..26 in
    // set 2.
    [b'A' as i16, b'a' as i16, 1],
    [b'B' as i16, b'b' as i16, 2],
    [b'C' as i16, b'c' as i16, 3],
    [b'D' as i16, b'd' as i16, 4],
    [b'E' as i16, b'e' as i16, 5],
    [b'F' as i16, b'f' as i16, 6],
    [b'G' as i16, b'g' as i16, 7],
    [b'H' as i16, b'h' as i16, 8],
    [b'I' as i16, b'i' as i16, 9],
    [b'J' as i16, b'j' as i16, 10],
    [b'K' as i16, b'k' as i16, 11],
    [b'L' as i16, b'l' as i16, 12],
    [b'M' as i16, b'm' as i16, 13],
    [b'N' as i16, b'n' as i16, 14],
    [b'O' as i16, b'o' as i16, 15],
    [b'P' as i16, b'p' as i16, 16],
    [b'Q' as i16, b'q' as i16, 17],
    [b'R' as i16, b'r' as i16, 18],
    [b'S' as i16, b's' as i16, 19],
    [b'T' as i16, b't' as i16, 20],
    [b'U' as i16, b'u' as i16, 21],
    [b'V' as i16, b'v' as i16, 22],
    [b'W' as i16, b'w' as i16, 23],
    [b'X' as i16, b'x' as i16, 24],
    [b'Y' as i16, b'y' as i16, 25],
    [b'Z' as i16, b'z' as i16, 26],

    // 36..=37: '-' and '.' (the limited-charmap rows match these).
    [b'-' as i16, b'_' as i16, 40],
    [b'.' as i16, b'`' as i16, 41],

    // 38: space / DEL / 0.
    [b' ' as i16, 127,         0],

    // 39..=42: punctuation in set 0; { / | / } / ~ in set 1;
    // * / , / : / FN1 in set 2.
    [b'$' as i16, b'{' as i16, b'*' as i16],
    [b'/' as i16, b'|' as i16, b',' as i16],
    [b'+' as i16, b'}' as i16, b':' as i16],
    [b'%' as i16, b'~' as i16, POSICODE_FN1],

    // 43..=45: latch / shift / FNC2-4 rows (the mode-control rows).
    [POSICODE_LA1, POSICODE_LA0, POSICODE_FN2],
    [POSICODE_SF1, POSICODE_SF0, POSICODE_FN3],
    [POSICODE_SF2, POSICODE_SF2, POSICODE_FN4],
];

/// Single-column charmap for versions `"limiteda"` and `"limitedb"`.
/// Each row is a `[set0, LIMITED_NA, LIMITED_NA]` triple — the
/// limited variants only use set 0. Direct port of BWIPP
/// `posicode_charmapslimited` (bwip-js line 18092).
#[rustfmt::skip]
pub(crate) const POSICODE_CHARMAPSLIMITED: [[i16; 3]; 38] = [
    // 0..=9: digits.
    [b'0' as i16, LIMITED_NA, LIMITED_NA],
    [b'1' as i16, LIMITED_NA, LIMITED_NA],
    [b'2' as i16, LIMITED_NA, LIMITED_NA],
    [b'3' as i16, LIMITED_NA, LIMITED_NA],
    [b'4' as i16, LIMITED_NA, LIMITED_NA],
    [b'5' as i16, LIMITED_NA, LIMITED_NA],
    [b'6' as i16, LIMITED_NA, LIMITED_NA],
    [b'7' as i16, LIMITED_NA, LIMITED_NA],
    [b'8' as i16, LIMITED_NA, LIMITED_NA],
    [b'9' as i16, LIMITED_NA, LIMITED_NA],

    // 10..=35: A..Z.
    [b'A' as i16, LIMITED_NA, LIMITED_NA],
    [b'B' as i16, LIMITED_NA, LIMITED_NA],
    [b'C' as i16, LIMITED_NA, LIMITED_NA],
    [b'D' as i16, LIMITED_NA, LIMITED_NA],
    [b'E' as i16, LIMITED_NA, LIMITED_NA],
    [b'F' as i16, LIMITED_NA, LIMITED_NA],
    [b'G' as i16, LIMITED_NA, LIMITED_NA],
    [b'H' as i16, LIMITED_NA, LIMITED_NA],
    [b'I' as i16, LIMITED_NA, LIMITED_NA],
    [b'J' as i16, LIMITED_NA, LIMITED_NA],
    [b'K' as i16, LIMITED_NA, LIMITED_NA],
    [b'L' as i16, LIMITED_NA, LIMITED_NA],
    [b'M' as i16, LIMITED_NA, LIMITED_NA],
    [b'N' as i16, LIMITED_NA, LIMITED_NA],
    [b'O' as i16, LIMITED_NA, LIMITED_NA],
    [b'P' as i16, LIMITED_NA, LIMITED_NA],
    [b'Q' as i16, LIMITED_NA, LIMITED_NA],
    [b'R' as i16, LIMITED_NA, LIMITED_NA],
    [b'S' as i16, LIMITED_NA, LIMITED_NA],
    [b'T' as i16, LIMITED_NA, LIMITED_NA],
    [b'U' as i16, LIMITED_NA, LIMITED_NA],
    [b'V' as i16, LIMITED_NA, LIMITED_NA],
    [b'W' as i16, LIMITED_NA, LIMITED_NA],
    [b'X' as i16, LIMITED_NA, LIMITED_NA],
    [b'Y' as i16, LIMITED_NA, LIMITED_NA],
    [b'Z' as i16, LIMITED_NA, LIMITED_NA],

    // 36..=37: '-' / '.'.
    [b'-' as i16, LIMITED_NA, LIMITED_NA],
    [b'.' as i16, LIMITED_NA, LIMITED_NA],
];

// ---------------------------------------------------------------------------
// POSICODE_C2W — 5 × 8 weight table for the check-character decomposition.
// Direct port of BWIPP `posicode_c2w` (bwip-js line 18147).
//
// The greedy algorithm walks the table row by row; for each cell it
// either adds the cell value to the accumulator (when sum + cell ≤ v),
// records the column index in `d[r]` and advances the row, or
// advances within the row.
// ---------------------------------------------------------------------------

/// Weight table for the check-character greedy decomposition.
pub(crate) const POSICODE_C2W: [[u32; 8]; 5] = [
    [495, 330, 210, 126, 70, 35, 15, 5],
    [165, 120, 84, 56, 35, 20, 10, 4],
    [45, 36, 28, 21, 15, 10, 6, 3],
    [9, 8, 7, 6, 5, 4, 3, 2],
    [1, 1, 1, 1, 1, 1, 1, 1],
];

// ---------------------------------------------------------------------------
// Bar-pattern tables. Each entry is a 6-character string of digit /
// punctuation characters; the decoder reads each character's ASCII
// value and subtracts 48 to yield the bar-or-space width in modules.
// '0' → 0, '9' → 9, ':' → 10, ';' → 11, '<' → 12.
//
// Direct port of BWIPP `posicode_encmaps["a" | "b" | "limiteda" |
// "limitedb"]` (bwip-js lines 18100-18137).
//
// Last two entries of each version's table are the start and stop
// patterns (in that order). All other entries are codeword patterns
// indexed by codeword value 0..=45 (normal) or 0..=37 (limited).
// ---------------------------------------------------------------------------

/// Version `a` bar patterns. 48 entries: 46 codeword patterns + 1
/// start + 1 stop.
pub(crate) const POSICODE_ENCS_A: [&str; 48] = [
    "141112",
    "131212",
    "121312",
    "111412",
    "131113",
    "121213",
    "111313",
    "121114",
    "111214",
    "111115",
    "181111",
    "171211",
    "161311",
    "151411",
    "141511",
    "131611",
    "121711",
    "111811",
    "171112",
    "161212",
    "151312",
    "141412",
    "131512",
    "121612",
    "111712",
    "161113",
    "151213",
    "141313",
    "131413",
    "121513",
    "111613",
    "151114",
    "141214",
    "131314",
    "121414",
    "111514",
    "141115",
    "131215",
    "121315",
    "111415",
    "131116",
    "121216",
    "111316",
    "121117",
    "111217",
    "111118",
    // 46: start pattern; 47: stop pattern.
    "1<111112",
    "111111111;1",
];

/// Version `b` bar patterns. 48 entries: 46 codeword patterns + 1
/// start + 1 stop. Each entry's width sum is one module greater than
/// the corresponding `A` entry.
pub(crate) const POSICODE_ENCS_B: [&str; 48] = [
    "151213",
    "141313",
    "131413",
    "121513",
    "141214",
    "131314",
    "121414",
    "131215",
    "121315",
    "121216",
    "191212",
    "181312",
    "171412",
    "161512",
    "151612",
    "141712",
    "131812",
    "121912",
    "181213",
    "171313",
    "161413",
    "151513",
    "141613",
    "131713",
    "121813",
    "171214",
    "161314",
    "151414",
    "141514",
    "131614",
    "121714",
    "161215",
    "151315",
    "141415",
    "131515",
    "121615",
    "151216",
    "141316",
    "131416",
    "121516",
    "141217",
    "131317",
    "121417",
    "131218",
    "121318",
    "121219",
    // 46: start pattern; 47: stop pattern.
    "1<121312",
    "121212121<1",
];

/// Version `limiteda` bar patterns. 40 entries total: 38 codeword
/// patterns then a start and stop pair. Note the stop pattern is a
/// single `"1"` module (the limited variants have a degenerate
/// stop bar).
pub(crate) const POSICODE_ENCS_LIMITEDA: [&str; 40] = [
    "111411", "111312", "111213", "111114", "121311", "121212", "121113", "141111", "131211",
    "131112", "171111", "161211", "151311", "141411", "131511", "121611", "111711", "161112",
    "151212", "141312", "131412", "121512", "111612", "151113", "141213", "131313", "121413",
    "111513", "141114", "131214", "121314", "111414", "131115", "121215", "111315", "121116",
    "111216", "111117", // 38: start pattern; 39: stop pattern.
    "151111", "1",
];

/// Version `limitedb` bar patterns. 40 entries total: 38 codeword
/// patterns then a start and stop pair. Each entry's width sum is
/// one module greater than the corresponding `limiteda` entry.
pub(crate) const POSICODE_ENCS_LIMITEDB: [&str; 40] = [
    "121512", "121413", "121314", "121215", "131412", "131313", "131214", "151212", "141312",
    "141213", "181212", "171312", "161412", "151512", "141612", "131712", "121812", "171213",
    "161313", "151413", "141513", "131613", "121713", "161214", "151314", "141414", "131514",
    "121614", "151215", "141315", "131415", "121515", "141216", "131316", "121416", "131217",
    "121317", "121218", // 38: start pattern; 39: stop pattern.
    "141212", "1",
];

// ---------------------------------------------------------------------------
// Public POSICODE version enum + entry point.
// ---------------------------------------------------------------------------

/// Which POSICODE variant to encode. Defaults to [`PosicodeVersion::A`]
/// to match BWIPP's `$_.version = "a"` default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PosicodeVersion {
    /// Version `"a"` — three-set alphanumeric with latches and shifts.
    /// **Not yet implemented** (Stage 22 burndown — see crate
    /// PORT_STATUS.md). Default to match BWIPP's `$_.version = "a"`.
    #[default]
    A,
    /// Version `"b"` — same alphabet as `A`, wider bar patterns.
    /// **Not yet implemented**.
    B,
    /// Version `"limiteda"` — single-set alphanumeric (digits +
    /// uppercase + `-` + `.`). Smallest variant. **Not yet
    /// implemented in Stage 22a (tables only); the encoder lands in
    /// Stage 22b.**
    LimitedA,
    /// Version `"limitedb"` — same alphabet as `LimitedA`, wider bar
    /// patterns. **Not yet implemented**.
    LimitedB,
}

impl PosicodeVersion {
    /// Return this version's bar-pattern table.
    pub(crate) fn encs(self) -> &'static [&'static str] {
        match self {
            Self::A => &POSICODE_ENCS_A,
            Self::B => &POSICODE_ENCS_B,
            Self::LimitedA => &POSICODE_ENCS_LIMITEDA,
            Self::LimitedB => &POSICODE_ENCS_LIMITEDB,
        }
    }

    /// Return this version's charmap (normal for `A`/`B`, limited
    /// for `LimitedA`/`LimitedB`).
    pub(crate) fn charmap(self) -> &'static [[i16; 3]] {
        match self {
            Self::A | Self::B => &POSICODE_CHARMAPSNORMAL,
            Self::LimitedA | Self::LimitedB => &POSICODE_CHARMAPSLIMITED,
        }
    }
}

impl std::str::FromStr for PosicodeVersion {
    type Err = ();

    /// Parse a BWIPP version identifier. Accepts the exact strings
    /// `"a"` / `"b"` / `"limiteda"` / `"limitedb"`; everything else
    /// returns `Err(())` and the caller surfaces a tagged error.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "a" => Ok(Self::A),
            "b" => Ok(Self::B),
            "limiteda" => Ok(Self::LimitedA),
            "limitedb" => Ok(Self::LimitedB),
            _ => Err(()),
        }
    }
}

// ---------------------------------------------------------------------------
// limiteda encoder (Stage 22b).
//
// The limited variants ship a single 38-entry charmap that maps each
// input byte directly to a codeword 0..=37 — there are no latches,
// shifts, or FNC markers. The encoder pipeline reduces to:
//
//   1. cws[i] = lookup_a(input[i])
//      (return InvalidData if any byte isn't A-encodable).
//   2. v = CRC-like accumulator over the bit-stream of all cws (6 LSBs
//      per cw), via BWIPP's `XOR 7682 + shift-right` step.
//   3. v = v + checkoffset (currently 0 — BWIPP exposes this as an
//      option but defaults to 0).
//   4. limited normalisation: v = v & 1023; if 824 < v < 853, v += 292.
//   5. Greedy weight-table decomposition of v into d[0..6] via
//      POSICODE_C2W.
//   6. cbs (12-module check pattern): start with widths
//      [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]; for i in 5..=0 set
//      cbs[(5 - i) * 2 + 1] = d[i] - 1 (BWIPP's `d[i] + 47` ASCII
//      char encoding minus the codepoint-base 48).
//   7. sbs = start_pattern + per-codeword 6-module patterns + cbs +
//      stop_pattern. For limiteda the start is `encs[38]="151111"`
//      and the stop is `encs[39]="1"` (single trailing module).
//
// BWIPP source: bwip-js `bwipp_posicode` at bwip-js-node.js lines
// 18384-18520. The full corpus that pins this implementation lives
// in `rust/tools/oracle-posicode.js` (regenerated against bwip-js
// 4.10.1 / BWIPP 2026-04-21).
// ---------------------------------------------------------------------------

/// Look up the limited-charmap codeword for byte `b`. Returns
/// `None` if `b` is not part of the limited alphabet (digits +
/// uppercase + `-` + `.`).
fn lookup_limited(b: u8) -> Option<u8> {
    POSICODE_CHARMAPSLIMITED
        .iter()
        .position(|row| row[0] == i16::from(b))
        .map(|i| i as u8)
}

/// Compute the BWIPP-faithful CRC-like accumulator `v` for a codeword
/// stream. The accumulator absorbs each codeword's 6 LSBs by XORing
/// the magic polynomial 7682 in whenever the accumulator-low bit XOR
/// the codeword-low bit is set, then shifts right one position.
///
/// This is the exact BWIPP loop at bwip-js lines 18450-18459.
fn compute_v(cws: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &cw_in in cws {
        let mut cw: u32 = u32::from(cw_in);
        for _ in 0..6 {
            if ((cw ^ v) & 1) != 0 {
                v ^= 7682;
            }
            v >>= 1;
            cw >>= 1;
        }
    }
    v
}

/// Greedy weight-table decomposition of `v` into 6 `d[i]` values via
/// [`POSICODE_C2W`]. BWIPP source at lines 18471-18493.
///
/// Initial state: d = [2; 6], r=c=w=sum=0. Each iteration considers
/// `t = sum + c2w[r][c]`:
///
///   * t == v → bump w; record d[r] = w + 2; sum = t.
///   * t > v  → record d[r] = w + 2; advance r; reset w.
///   * t < v  → advance c; bump w; sum = t.
///
/// On exit, d[5] = 20 - sum(d[0..5]).
fn decompose_check_digits(v: u32) -> [u8; 6] {
    let mut d: [u8; 6] = [2; 6];
    let mut r: usize = 0;
    let mut c: usize = 0;
    let mut w: u32 = 0;
    let mut sum: u32 = 0;
    // Hard cap on iterations as a safety belt — the weight table's
    // top-left cell is 495 and the table sums to 1320, so a healthy
    // run terminates in <100 steps. Use a generous limit anyway.
    for _ in 0..10_000 {
        if sum == v {
            break;
        }
        if r >= POSICODE_C2W.len() || c >= POSICODE_C2W[0].len() {
            break;
        }
        let t = sum + POSICODE_C2W[r][c];
        if t == v {
            w += 1;
            d[r] = (w + 2) as u8;
            sum = t;
        } else if t > v {
            d[r] = (w + 2) as u8;
            r += 1;
            w = 0;
        } else {
            c += 1;
            w += 1;
            sum = t;
        }
    }
    // d[5] = 20 - sum(d[0..5]).
    let head_sum: i32 =
        (d[0] as i32) + (d[1] as i32) + (d[2] as i32) + (d[3] as i32) + (d[4] as i32);
    let tail = 20 - head_sum;
    // Clamp into u8 — the BWIPP algorithm guarantees tail ≥ 0 for
    // valid v values; if a future bug produces a negative, surface 0
    // instead of panicking.
    d[5] = tail.max(0) as u8;
    d
}

/// Build the 12-module check-pattern widths from the d[0..6] vector.
/// Position layout per BWIPP:
///
///   index:   0  1  2  3  4  5  6  7  8  9 10 11
///   value:   1  d5 1  d4 1  d3 1  d2 1  d1 1  d0
///
/// where `d[i] - 1` is the bar/space width (the BWIPP source stores
/// `d[i] + 47` as an ASCII code, and the renderer subtracts 48 to
/// produce the integer width — so width = d - 1). For `version =
/// "limitedb"` (and `"b"`), each d[i] is bumped by 1 *before* the
/// pattern is materialised; that's not yet wired here (Stage 22b
/// ships `limiteda` only).
fn build_cbs(d: [u8; 6]) -> [u8; 12] {
    let mut cbs: [u8; 12] = [1; 12];
    for (i, &di) in d.iter().enumerate() {
        let pos = (5 - i) * 2 + 1;
        cbs[pos] = di.saturating_sub(1);
    }
    cbs
}

/// Parse a 6-character POSICODE bar pattern (digits / `':'` / `';'` /
/// `'<'`) into module widths (1..=12). Each character's bar/space
/// width is `codepoint - 48`.
fn pattern_to_widths(pat: &str) -> Vec<u8> {
    pat.bytes().map(|b| b.saturating_sub(48)).collect()
}

/// Finalise a codeword stream into the complete sbs byte vector by
/// running the BWIPP post-encoding pipeline shared by all four
/// versions:
///
///   1. compute_v (CRC-like accumulator over the 6-LSB bit-stream)
///   2. version-specific normalisation:
///        * limited: `v &= 1023; if 824 < v < 853 { v += 292 }`
///        * normal : `v = (v & 1023) + 45`
///   3. greedy weight-table decomposition into d[0..6]
///   4. for `b` / `limitedb`: bump every d[i] by 1
///   5. cbs = "1d51d41d31d21d11d0" (12 module widths)
///   6. sbs = start + per-cw 6-mod pattern + cbs + stop
///
/// BWIPP source: bwip-js `bwipp_posicode` lines 18447–18520.
fn finalize_sbs(cws: &[u8], version: PosicodeVersion) -> Vec<u8> {
    let mut v = compute_v(cws);
    let is_limited = matches!(
        version,
        PosicodeVersion::LimitedA | PosicodeVersion::LimitedB
    );
    if is_limited {
        v &= 1023;
        if v > 824 && v < 853 {
            v += 292;
        }
    } else {
        // Normal versions add 45 after the 10-bit mask so the
        // check decomposition can never collide with the cbs
        // start-of-pattern sentinel.
        v = (v & 1023) + 45;
    }

    let mut d = decompose_check_digits(v);
    if matches!(version, PosicodeVersion::B | PosicodeVersion::LimitedB) {
        for di in &mut d {
            *di = di.saturating_add(1);
        }
    }

    let cbs = build_cbs(d);
    let encs = version.encs();
    // Start = encs[len - 2]; stop = encs[len - 1].
    let start_pat = pattern_to_widths(encs[encs.len() - 2]);
    let stop_pat = pattern_to_widths(encs[encs.len() - 1]);

    let mut bars: Vec<u8> =
        Vec::with_capacity(start_pat.len() + cws.len() * 6 + 12 + stop_pat.len());
    bars.extend_from_slice(&start_pat);
    for &cw in cws {
        let pat = pattern_to_widths(encs[cw as usize]);
        debug_assert_eq!(pat.len(), 6, "cw {cw} pattern not 6 modules wide");
        bars.extend_from_slice(&pat);
    }
    bars.extend_from_slice(&cbs);
    bars.extend_from_slice(&stop_pat);
    bars
}

/// Stage 22b/c — encode `data` as POSICODE `limiteda` or
/// `limitedb`. Both variants are single-set with no latches /
/// shifts / FNC; they differ only in:
///
///   * the bar-pattern table (`POSICODE_ENCS_LIMITEDA` vs
///     `POSICODE_ENCS_LIMITEDB` — each `limitedb` pattern's module
///     sum is one greater than the matching `limiteda` pattern), and
///   * for `limitedb` only: every check-digit value `d[i]` is bumped
///     by 1 before the cbs pattern is materialised (BWIPP's
///     `$_.d = $_.d.map(x => x + 1)` step at bwip-js line 18495).
///
/// Caller has already validated `version ∈ {LimitedA, LimitedB}`.
///
/// # Errors
///
/// * [`Error::InvalidData`] if `data` is empty or any byte isn't in
///   the limited alphabet (`0..=9`, `A..=Z`, `-`, `.`).
/// * [`Error::InvalidData`] if `data` exceeds 500 bytes (BWIPP's
///   hard upper bound on POSICODE payload, bwip-js line 18246).
fn encode_limited(data: &str, version: PosicodeVersion) -> Result<LinearPattern, Error> {
    debug_assert!(
        matches!(
            version,
            PosicodeVersion::LimitedA | PosicodeVersion::LimitedB
        ),
        "encode_limited only handles LimitedA / LimitedB",
    );
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Err(Error::InvalidData(
            "posicode: empty input is not encodable".into(),
        ));
    }
    if bytes.len() > 500 {
        return Err(Error::InvalidData(format!(
            "posicode: payload of {} bytes exceeds BWIPP's 500-byte limit",
            bytes.len()
        )));
    }

    // Step 1: cws lookup. Limited has no latches/shifts — every
    // byte must map directly to a codeword 0..=37.
    let mut cws: Vec<u8> = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        match lookup_limited(b) {
            Some(cw) => cws.push(cw),
            None => {
                return Err(Error::InvalidData(format!(
                    "posicode limited: byte 0x{b:02x} at position {i} is not in the \
                     limited alphabet (0-9, A-Z, '-', '.')"
                )));
            }
        }
    }

    Ok(LinearPattern {
        bars: finalize_sbs(&cws, version),
        text: None,
    })
}

// ---------------------------------------------------------------------------
// Stage 22d — encode_normal (auto-encoder for versions `a` / `b`).
//
// The normal versions share BWIPP's three-set auto-encoder state
// machine. Each input byte resolves through one of:
//
//   * direct emission in the active set (`cset`),
//   * SF2 shift to set 2 (for non-printable / extended-ASCII rows
//     present only in set 2),
//   * latch (LA0 / LA1) when the *next* byte is also outside the
//     active set,
//   * shift (SF0 / SF1) when the next byte IS in the active set
//     (one-character excursion).
//
// The encoder also handles ASCII ↔ extended-ASCII transitions via
// FN4 markers, but the current implementation rejects any byte
// >= 128 with `Error::InvalidData` since FN4 isn't wired up yet
// (extension path — Stage 22d.1+).
//
// BWIPP source: bwip-js `bwipp_posicode` lines 18228–18441
// (the `parseinput` → `numSA/numEA` → state-machine sequence).
// ---------------------------------------------------------------------------

/// Three set lookup maps for normal POSICODE, built once on first
/// call. Each map indexes by the literal value stored in
/// [`POSICODE_CHARMAPSNORMAL`] (positive bytes for printable chars
/// / control codes, negative sentinels for LA/SF/FN markers) and
/// returns the row index 0..=45 (i.e., the codeword).
fn normal_sets() -> &'static [std::collections::HashMap<i16, u8>; 3] {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static SETS: OnceLock<[HashMap<i16, u8>; 3]> = OnceLock::new();
    SETS.get_or_init(|| {
        let mut sets: [HashMap<i16, u8>; 3] = [HashMap::new(), HashMap::new(), HashMap::new()];
        for (row_idx, row) in POSICODE_CHARMAPSNORMAL.iter().enumerate() {
            for (set_idx, &val) in row.iter().enumerate() {
                sets[set_idx].insert(val, row_idx as u8);
            }
        }
        sets
    })
}

/// Insert FN4 sentinels into a byte stream to mark ASCII ↔
/// extended-ASCII transitions, mirroring BWIPP's pre-encoder pass at
/// bwip-js-node.js lines 18336–18370.
///
/// The encoder tracks a single boolean `ea` ("extended-ASCII active")
/// that flips on `FN4 FN4` (double-marker latch) or one-time-shifts
/// on a single `FN4` (shift only the next byte to the other side).
/// Choice of shift vs latch is governed by the run length of the
/// opposite-side bytes from the transition point:
///
/// * `numSA[i]` = number of standard-ASCII bytes starting at `i`,
/// * `numEA[i]` = number of extended-ASCII bytes starting at `i`,
///
/// both built right-to-left; threshold = 3 when the run reaches the
/// end of `msg`, else 5. Runs shorter than the threshold get a
/// single FN4 (shift); longer runs get a double FN4 (latch).
///
/// Extended-ASCII bytes have their high bit stripped (`c & 0x7f`)
/// before being emitted — the FN4 markers tell the decoder whether
/// to re-apply 0x80.
fn insert_fn4_markers(msg: &[i16]) -> Vec<i16> {
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
        // BWIPP test: insert FN4 when `c >= 0 && ea == (c < 128)`.
        // That captures "current state contradicts the byte's
        // natural side" (standard-when-extended, or vice-versa).
        if c >= 0 && ea == (c < 128) {
            let run = if ea { num_sa[i] } else { num_ea[i] };
            let threshold = if run + i == msglen { 3 } else { 5 };
            if run < threshold {
                out.push(POSICODE_FN4);
            } else {
                ea = !ea;
                out.push(POSICODE_FN4);
                out.push(POSICODE_FN4);
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

/// Run the BWIPP auto-encoder state machine over `msg` (a sequence
/// of byte values; sentinel values are not yet supported in this
/// implementation). Returns the codeword stream.
///
/// State machine summary (one iteration per outer loop turn — the
/// inner `for(;;)` in BWIPP is a goto-emulating switch):
///
///   - `char1 = msg[i]`, `char2 = msg[i+1]` or -99 (sentinel for
///     "no next byte").
///   - If `char1 ∈ cset`: emit `cset[char1]`, i++. (direct)
///   - Else if `char1 ∈ set2`: emit `cset[SF2]`, then `set2[char1]`,
///     i++. (single shift to set 2)
///   - Else if `char2 ∉ cset`: emit `cset[LA1]` or `cset[LA0]`
///     (whichever swaps to the other set); flip cset. NO i++.
///   - Else: emit `cset[SF1]` or `cset[SF0]` (single-char shift to
///     other set); emit `other_set[char1]`; i++.
fn select_codewords_normal(msg: &[i16]) -> Vec<u8> {
    let sets = normal_sets();
    let mut cws: Vec<u8> = Vec::with_capacity(msg.len() * 2);
    let mut i: usize = 0;
    let mut cset: usize = 0; // 0=set0, 1=set1; never starts at set2.

    while i < msg.len() {
        let char1: i16 = msg[i];
        let char2: i16 = if i + 1 < msg.len() { msg[i + 1] } else { -99 };

        // Path A — char1 lives in the active set.
        if let Some(&cw) = sets[cset].get(&char1) {
            cws.push(cw);
            i += 1;
            continue;
        }

        // Path B — char1 lives only in set 2 → SF2 shift.
        if sets[2].contains_key(&char1) {
            cws.push(sets[cset][&POSICODE_SF2]);
            cws.push(sets[2][&char1]);
            i += 1;
            continue;
        }

        // Path C — char1 lives in the *other* set (the one we are
        // not currently in). Decide between latch (when char2 also
        // isn't in cset) and shift (when char2 IS in cset).
        let other = 1 - cset; // 0↔1
        let char2_in_cset = sets[cset].contains_key(&char2);

        if !char2_in_cset {
            // Latch — emit LA1 from set0, or LA0 from set1; flip
            // cset; do NOT consume i (the latch only swaps state).
            let latch_sentinel = if cset == 0 {
                POSICODE_LA1
            } else {
                POSICODE_LA0
            };
            cws.push(sets[cset][&latch_sentinel]);
            cset = other;
            continue;
        } else {
            // Shift — emit SF1 from set0, or SF0 from set1; emit
            // char1's codeword in the other set; i++ to consume
            // char1 (cset unchanged).
            let shift_sentinel = if cset == 0 {
                POSICODE_SF1
            } else {
                POSICODE_SF0
            };
            cws.push(sets[cset][&shift_sentinel]);
            cws.push(sets[other][&char1]);
            i += 1;
            continue;
        }
    }

    cws
}

/// Encode `data` as POSICODE version `a` or `b` using the BWIPP
/// auto-encoder state machine. Versions `a` and `b` share the same
/// state machine; only the bar-pattern table differs (and the
/// d[i]+=1 step for `b`).
///
/// Caller has already validated `version ∈ {A, B}`.
///
/// # Errors
///
/// * [`Error::InvalidData`] if `data` is empty.
/// * [`Error::InvalidData`] if `data` exceeds 500 bytes (BWIPP's
///   `bwipp_posicode` length cap at bwip-js line 18246).
/// * [`Error::InvalidData`] if `data` contains a byte that is not
///   encodable in any of the three normal sets (vanishingly rare:
///   the normal charmap covers every standard-ASCII byte
///   `0x00..=0x7f` through some combination of set0/set1/set2, and
///   extended-ASCII bytes get high-bit-stripped via the FN4
///   transition so they land back in the `0x00..=0x7f` range).
fn encode_normal(data: &str, version: PosicodeVersion) -> Result<LinearPattern, Error> {
    debug_assert!(
        matches!(version, PosicodeVersion::A | PosicodeVersion::B),
        "encode_normal only handles A / B",
    );
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Err(Error::InvalidData(
            "posicode: empty input is not encodable".into(),
        ));
    }
    if bytes.len() > 500 {
        return Err(Error::InvalidData(format!(
            "posicode: payload of {} bytes exceeds BWIPP's 500-byte limit",
            bytes.len()
        )));
    }

    // Step 1: parseinput. With parsefnc=false (the default and the
    // only mode this implementation supports today), parseinput is
    // an identity pass — every byte becomes a non-negative i16 in
    // `initial_msg`.
    let initial_msg: Vec<i16> = bytes.iter().map(|&b| i16::from(b)).collect();

    // Step 2 + 3: numSA / numEA arrays and FN4 marker insertion
    // for ASCII ↔ extended-ASCII transitions.
    let processed_msg = insert_fn4_markers(&initial_msg);

    // Step 4: defensive coverage check — every value in
    // `processed_msg` must resolve in at least one of the three
    // normal sets. After FN4 insertion every byte ≥ 0 is in
    // 0x00..=0x7f and the FN4 sentinel is in set 2.
    let sets = normal_sets();
    for (i, &c) in processed_msg.iter().enumerate() {
        let in_any =
            sets[0].contains_key(&c) || sets[1].contains_key(&c) || sets[2].contains_key(&c);
        if !in_any {
            return Err(Error::InvalidData(format!(
                "posicode: byte 0x{:02x} at processed position {i} is not encodable \
                 in any POSICODE set",
                c as u8
            )));
        }
    }

    // Step 5: run the state machine to produce cws.
    let cws = select_codewords_normal(&processed_msg);

    Ok(LinearPattern {
        bars: finalize_sbs(&cws, version),
        text: None,
    })
}

/// Encode `data` as POSICODE.
///
/// The version is selected via `opts.extras["version"]` (one of
/// `"a"`, `"b"`, `"limiteda"`, `"limitedb"`). Default is `"a"` to
/// match BWIPP's `$_.version = "a"` default.
///
/// # Errors
///
/// * [`Error::InvalidOption`] if `opts.extras["version"]` is set but
///   isn't one of the four BWIPP-valid identifiers.
/// * [`Error::InvalidData`] if `data` is empty, contains a byte
///   outside the active version's alphabet, or exceeds BWIPP's
///   500-byte upper bound. For versions `a`/`b`, bytes ≥ 0x80
///   currently return `InvalidData` because the FN4-based
///   extended-ASCII path is not yet wired (Stage 22d.1).
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    // Parse the version selector. Default to "a" to match BWIPP.
    let version_str = opts.get("version").unwrap_or("a");
    let version = match version_str.parse::<PosicodeVersion>() {
        Ok(v) => v,
        Err(()) => {
            return Err(Error::InvalidOption(format!(
                "posicode: version `{version_str}` is not one of \
                 `a` / `b` / `limiteda` / `limitedb`"
            )));
        }
    };

    match version {
        PosicodeVersion::LimitedA | PosicodeVersion::LimitedB => encode_limited(data, version),
        PosicodeVersion::A | PosicodeVersion::B => encode_normal(data, version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The normal charmap is exactly 46 rows × 3 columns and starts
    /// with the digits row mapping `[ '0', '^', '\'' ]`.
    #[test]
    fn normal_charmap_shape_matches_bwipp() {
        assert_eq!(POSICODE_CHARMAPSNORMAL.len(), 46);
        assert_eq!(
            POSICODE_CHARMAPSNORMAL[0],
            [b'0' as i16, b'^' as i16, b'\'' as i16]
        );
        // Row 35 = 'Z'/'z'/26.
        assert_eq!(POSICODE_CHARMAPSNORMAL[35], [b'Z' as i16, b'z' as i16, 26]);
        // Row 38 = ' '/127/0.
        assert_eq!(POSICODE_CHARMAPSNORMAL[38], [b' ' as i16, 127, 0]);
        // Row 42 = '%'/'~'/FN1.
        assert_eq!(
            POSICODE_CHARMAPSNORMAL[42],
            [b'%' as i16, b'~' as i16, POSICODE_FN1]
        );
        // Row 45 = SF2/SF2/FN4 (mode-control trailing row).
        assert_eq!(
            POSICODE_CHARMAPSNORMAL[45],
            [POSICODE_SF2, POSICODE_SF2, POSICODE_FN4]
        );
    }

    /// The limited charmap is exactly 38 rows × 3 columns. Every set-1
    /// and set-2 entry is `LIMITED_NA`. Set-0 entries are digits
    /// 0..9, uppercase A..Z, '-' and '.'.
    #[test]
    fn limited_charmap_shape_matches_bwipp() {
        assert_eq!(POSICODE_CHARMAPSLIMITED.len(), 38);
        for (i, row) in POSICODE_CHARMAPSLIMITED.iter().enumerate() {
            assert_eq!(row[1], LIMITED_NA, "row {i} set-1 should be LIMITED_NA");
            assert_eq!(row[2], LIMITED_NA, "row {i} set-2 should be LIMITED_NA");
        }
        // Digits 0..9.
        for (d, row) in POSICODE_CHARMAPSLIMITED.iter().take(10).enumerate() {
            assert_eq!(row[0], (b'0' + d as u8) as i16);
        }
        // Uppercase A..Z (rows 10..36).
        for (a, row) in POSICODE_CHARMAPSLIMITED
            .iter()
            .skip(10)
            .take(26)
            .enumerate()
        {
            assert_eq!(row[0], (b'A' + a as u8) as i16);
        }
        assert_eq!(POSICODE_CHARMAPSLIMITED[36][0], b'-' as i16);
        assert_eq!(POSICODE_CHARMAPSLIMITED[37][0], b'.' as i16);
    }

    /// Each version's encs table has 46 (normal) or 38 (limited)
    /// codeword patterns + 1 start + 1 stop.
    #[test]
    fn encs_table_lengths_match_bwipp() {
        assert_eq!(POSICODE_ENCS_A.len(), 48);
        assert_eq!(POSICODE_ENCS_B.len(), 48);
        assert_eq!(POSICODE_ENCS_LIMITEDA.len(), 40);
        assert_eq!(POSICODE_ENCS_LIMITEDB.len(), 40);
    }

    /// Every codeword pattern (the entries up to length-2) is exactly
    /// 6 characters wide. Direct check against BWIPP's
    /// `$_.j = $_.j + 6` step at bwip-js line 18513.
    #[test]
    fn codeword_patterns_are_6_modules() {
        for (i, &p) in POSICODE_ENCS_A[..46].iter().enumerate() {
            assert_eq!(p.len(), 6, "ENCS_A[{i}] should be 6 modules");
        }
        for (i, &p) in POSICODE_ENCS_B[..46].iter().enumerate() {
            assert_eq!(p.len(), 6, "ENCS_B[{i}] should be 6 modules");
        }
        for (i, &p) in POSICODE_ENCS_LIMITEDA[..38].iter().enumerate() {
            assert_eq!(p.len(), 6, "ENCS_LIMITEDA[{i}] should be 6 modules");
        }
        for (i, &p) in POSICODE_ENCS_LIMITEDB[..38].iter().enumerate() {
            assert_eq!(p.len(), 6, "ENCS_LIMITEDB[{i}] should be 6 modules");
        }
    }

    /// Spot-check start + stop patterns against BWIPP source.
    #[test]
    fn start_stop_patterns_match_bwipp() {
        assert_eq!(POSICODE_ENCS_A[46], "1<111112");
        assert_eq!(POSICODE_ENCS_A[47], "111111111;1");
        assert_eq!(POSICODE_ENCS_B[46], "1<121312");
        assert_eq!(POSICODE_ENCS_B[47], "121212121<1");
        assert_eq!(POSICODE_ENCS_LIMITEDA[38], "151111");
        assert_eq!(POSICODE_ENCS_LIMITEDA[39], "1");
        assert_eq!(POSICODE_ENCS_LIMITEDB[38], "141212");
        assert_eq!(POSICODE_ENCS_LIMITEDB[39], "1");
    }

    /// Weight table is exactly 5 × 8 with the expected anchor values
    /// at the corners (BWIPP `posicode_c2w` at bwip-js line 18147).
    #[test]
    fn weight_table_matches_bwipp() {
        assert_eq!(POSICODE_C2W.len(), 5);
        for row in &POSICODE_C2W {
            assert_eq!(row.len(), 8);
        }
        assert_eq!(POSICODE_C2W[0][0], 495);
        assert_eq!(POSICODE_C2W[0][7], 5);
        assert_eq!(POSICODE_C2W[4], [1, 1, 1, 1, 1, 1, 1, 1]);
    }

    /// `PosicodeVersion::from_str` accepts the four BWIPP-valid
    /// identifiers and rejects anything else.
    #[test]
    fn version_from_str_round_trips() {
        use std::str::FromStr;
        assert_eq!(PosicodeVersion::from_str("a"), Ok(PosicodeVersion::A));
        assert_eq!(PosicodeVersion::from_str("b"), Ok(PosicodeVersion::B));
        assert_eq!(
            PosicodeVersion::from_str("limiteda"),
            Ok(PosicodeVersion::LimitedA)
        );
        assert_eq!(
            PosicodeVersion::from_str("limitedb"),
            Ok(PosicodeVersion::LimitedB)
        );
        // Stage 11.A8c — upgrade three bare `.is_err()` rejection
        // checks to `assert_eq!(..., Err(()))` form with descriptive
        // failure-mode labels. Each input exercises a DIFFERENT
        // mutation class:
        //   * `"A"` — case-sensitive arm (kills `to_lowercase()`
        //     prepass / `eq_ignore_ascii_case` mutations).
        //   * `"c"` — extra-character arm (kills wildcard `_` →
        //     fallback-to-A or fallback-to-B mutations).
        //   * `""` — empty-string arm (kills `if !s.is_empty()`
        //     short-circuit that would default to A).
        assert_eq!(
            PosicodeVersion::from_str("A"),
            Err(()),
            "uppercase `A` must reject — BWIPP version IDs are case-sensitive"
        );
        assert_eq!(
            PosicodeVersion::from_str("c"),
            Err(()),
            "unknown letter `c` must reject — only a/b/limiteda/limitedb are valid"
        );
        assert_eq!(
            PosicodeVersion::from_str(""),
            Err(()),
            "empty string must reject — no default version"
        );
    }

    /// `encs()` and `charmap()` accessors return the correct table
    /// per version.
    #[test]
    fn version_table_accessors() {
        assert_eq!(PosicodeVersion::A.encs().len(), 48);
        assert_eq!(PosicodeVersion::B.encs().len(), 48);
        assert_eq!(PosicodeVersion::LimitedA.encs().len(), 40);
        assert_eq!(PosicodeVersion::LimitedB.encs().len(), 40);
        assert_eq!(PosicodeVersion::A.charmap().len(), 46);
        assert_eq!(PosicodeVersion::LimitedA.charmap().len(), 38);
    }

    /// `Options::extras["version"]` set to anything that isn't one
    /// of the four BWIPP-valid identifiers returns
    /// `Error::InvalidOption`.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains
    /// ("version")` upgraded to 4-anchor pin:
    ///   1. `posicode:` symbology prefix
    ///   2. `version `c`` value-Debug-echo
    ///   3. `is not one of` predicate
    ///   4. `a` / `b` / `limiteda` / `limitedb` valid-list anchor
    ///      (kills mutations that drop or rename any of the four
    ///      enumerated valid version identifiers in the format
    ///      string at line 1007-1008 of posicode.rs).
    #[test]
    fn encode_rejects_unknown_version() {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "c".into()));
        let err = encode("HELLO", &opts).unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("version `c`"),
                    "missing version-value Debug echo `version \\`c\\``: {msg:?}"
                );
                assert!(
                    msg.contains("is not one of"),
                    "missing `is not one of` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("`a`")
                        && msg.contains("`b`")
                        && msg.contains("`limiteda`")
                        && msg.contains("`limitedb`"),
                    "missing one of the valid-version identifiers in enumeration: {msg:?}"
                );
            }
            other => panic!("expected InvalidOption(version), got {other:?}"),
        }
    }

    /// Stage 22d default-version smoke: with `Options::default()`
    /// (no `version` extra), POSICODE should now produce a valid
    /// version-`a` sbs instead of returning `Unimplemented`.
    #[test]
    fn encode_default_routes_to_version_a() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE default-version dispatch path: `Options::default()`
        // (no `version` extra) must route to version-a rather than
        // returning Unimplemented (Stage 22d default-version smoke).
        let p = encode("HELLO", &Options::default()).expect(
            "encode(\"HELLO\", default) (POSICODE default-version dispatch; no `version` extra → version-a, not Unimplemented) must succeed",
        );
        // Version-a "HELLO" sbs has start (7 modules) + 5 cws * 6 +
        // 12 (cbs) + 11 (stop). Sanity-check we got something close
        // to that.
        assert!(
            p.bars.len() > 30,
            "expected nontrivial sbs, got {:?}",
            p.bars
        );
    }

    /// Build the limiteda Options once for use across the encoder
    /// goldens.
    fn limiteda_opts() -> Options {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "limiteda".into()));
        opts
    }

    /// `lookup_limited` returns the row index of every limited-alphabet
    /// byte and `None` for anything else.
    #[test]
    fn lookup_limited_matches_charmap() {
        assert_eq!(lookup_limited(b'0'), Some(0));
        assert_eq!(lookup_limited(b'9'), Some(9));
        assert_eq!(lookup_limited(b'A'), Some(10));
        assert_eq!(lookup_limited(b'Z'), Some(35));
        assert_eq!(lookup_limited(b'-'), Some(36));
        assert_eq!(lookup_limited(b'.'), Some(37));
        // Lowercase isn't in limited.
        assert_eq!(lookup_limited(b'a'), None);
        // Space isn't in limited (limited drops the row 38 of normal).
        assert_eq!(lookup_limited(b' '), None);
        assert_eq!(lookup_limited(0), None);
    }

    /// Stage 11.A8c — pin `pattern_to_widths` directly. The helper
    /// converts BWIPP's pattern characters (ASCII '0'..='<') into
    /// module widths via `b - 48`. It's used inside `finalize_sbs`
    /// only, so the only end-to-end check is the full sbs golden;
    /// a mutant like `saturating_sub(48)` → `saturating_sub(47)`
    /// would shift every width by 1 and be caught by goldens, but
    /// only at the symbol level. A direct unit test makes the
    /// breakage point obvious.
    ///
    /// Hand-computed:
    ///   - "0" → [0]   (ASCII '0' = 48, 48-48 = 0)
    ///   - "9" → [9]
    ///   - ":" → [10]  (ASCII ':' = 58)
    ///   - ";" → [11]
    ///   - "<" → [12]
    /// Stage 11.A8c — pin `build_cbs` 12-position interleave:
    ///   cbs[even] = 1; cbs[(5-i)*2 + 1] = d[i].saturating_sub(1).
    /// In effect, d is reverse-placed at odd indices [11, 9, 7, 5, 3, 1].
    ///
    /// Mutations caught:
    ///   * Init `[1; 12]` → `[0; 12]`: even positions break.
    ///   * `(5 - i) * 2 + 1` → `(5 - i) * 2`: even positions get d,
    ///     odd positions stay 1.
    ///   * `(5 - i)` → `i`: forward-place instead of reverse —
    ///     d[0] would land at pos 1 not 11.
    ///   * `saturating_sub(1)` → `saturating_sub(0)`: each width is
    ///     1 larger than expected.
    ///   * `saturating_sub` → plain `-` would panic on d=0.
    #[test]
    fn build_cbs_reverse_interleave_with_sub_one() {
        // d = [3, 5, 7, 4, 6, 2] → cbs:
        //   even pos: 1 each
        //   pos 1: d[5]-1 = 1
        //   pos 3: d[4]-1 = 5
        //   pos 5: d[3]-1 = 3
        //   pos 7: d[2]-1 = 6
        //   pos 9: d[1]-1 = 4
        //   pos 11: d[0]-1 = 2
        let cbs = build_cbs([3, 5, 7, 4, 6, 2]);
        assert_eq!(cbs, [1, 1, 1, 5, 1, 3, 1, 6, 1, 4, 1, 2]);

        // Sanity edge: d[5] = 0 → saturating_sub keeps cbs[1] = 0
        // (not panic; not wrap to 255).
        let cbs0 = build_cbs([3, 5, 7, 4, 6, 0]);
        assert_eq!(cbs0[1], 0, "d[5]=0 → saturating_sub gives 0");
        // Other slots unchanged from the previous case.
        assert_eq!(&cbs0[2..], &cbs[2..]);
    }

    ///   - "" → []
    ///   - "123" → [1, 2, 3]
    ///   - "151111" (BWIPP's start pattern) → [1, 5, 1, 1, 1, 1]
    #[test]
    fn pattern_to_widths_subtracts_ascii_zero() {
        assert_eq!(pattern_to_widths("0"), vec![0]);
        assert_eq!(pattern_to_widths("9"), vec![9]);
        assert_eq!(pattern_to_widths(":"), vec![10], "':' (58) - 48 = 10");
        assert_eq!(pattern_to_widths(";"), vec![11]);
        assert_eq!(pattern_to_widths("<"), vec![12]);
        assert_eq!(pattern_to_widths(""), Vec::<u8>::new());
        assert_eq!(pattern_to_widths("123"), vec![1, 2, 3]);
        // Real BWIPP start pattern.
        assert_eq!(pattern_to_widths("151111"), vec![1, 5, 1, 1, 1, 1]);
        // Sub-ASCII'0' input → saturating_sub clamps to 0 (does not panic).
        assert_eq!(pattern_to_widths("/"), vec![0], "'/' (47) saturates to 0");
        assert_eq!(pattern_to_widths("\0"), vec![0]);
    }

    /// `compute_v` reproduces BWIPP's CRC accumulator for canonical
    /// inputs (captured via `rust/tools/oracle-posicode.js` against
    /// bwip-js 4.10.1 / BWIPP 2026-04-21).
    #[test]
    fn compute_v_matches_bwip_js_oracle() {
        // Single-codeword: cw=0 → v stays 0 through 6 shifts.
        assert_eq!(compute_v(&[0]), 0);
        // cw=1 → v ends at 553 (pre-normalisation).
        let raw_v_1 = compute_v(&[1]);
        // Post-normalisation `v &= 1023` then optional + 292: bwip-js
        // reports v = 553 for "1" (single-cw stream of 1).
        assert_eq!(raw_v_1 & 1023, 553);
        // 10-byte digit run "0123456789": post-normalisation v = 296.
        let cws_digits: Vec<u8> = (0..10).collect();
        let mut v = compute_v(&cws_digits) & 1023;
        if v > 824 && v < 853 {
            v += 292;
        }
        assert_eq!(v, 296);
    }

    /// `decompose_check_digits` matches BWIPP's greedy weight-table
    /// decomposition for the oracle values.
    #[test]
    fn decompose_check_digits_matches_bwip_js_oracle() {
        // v=0  → d=[2,2,2,2,2,10] (the trivial all-min plus tail=10).
        assert_eq!(decompose_check_digits(0), [2, 2, 2, 2, 2, 10]);
        // v=553  → d=[3,2,3,6,2,4] (oracle for barcode "1").
        assert_eq!(decompose_check_digits(553), [3, 2, 3, 6, 2, 4]);
        // v=272  → d=[2,3,6,4,2,3] (oracle for barcode "A").
        assert_eq!(decompose_check_digits(272), [2, 3, 6, 4, 2, 3]);
        // v=889  → d=[4,2,5,2,2,5] (oracle for barcode "Z").
        assert_eq!(decompose_check_digits(889), [4, 2, 5, 2, 2, 5]);
        // v=296  → d=[2,4,2,3,6,3] (oracle for "0123456789").
        assert_eq!(decompose_check_digits(296), [2, 4, 2, 3, 6, 3]);
    }

    /// `build_cbs` lays the d-values out per BWIPP at the odd
    /// positions, with width = d - 1 and 1-module bars at the even
    /// positions.
    #[test]
    fn build_cbs_matches_bwip_js_oracle() {
        // For d=[2,2,2,2,2,10] (oracle for "0"):
        // cbs = [1, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1].
        assert_eq!(
            build_cbs([2, 2, 2, 2, 2, 10]),
            [1, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
        );
        // For d=[3,2,3,6,2,4] (oracle for "1"):
        // cbs = [1, 3, 1, 1, 1, 5, 1, 2, 1, 1, 1, 2].
        assert_eq!(
            build_cbs([3, 2, 3, 6, 2, 4]),
            [1, 3, 1, 1, 1, 5, 1, 2, 1, 1, 1, 2]
        );
    }

    /// **Byte-for-byte oracle**: limiteda encode of "0" matches the
    /// bwip-js sbs stream module-for-module. The full 25-element
    /// sbs decomposes as:
    ///
    /// * Start pattern (encs[38]="151111"): widths [1, 5, 1, 1, 1, 1].
    /// * cws[0]=0 (encs[0]="111411"):        widths [1, 1, 1, 4, 1, 1].
    /// * Check pattern (12 modules):         widths [1, 9, 1, 1, 1, 1,
    ///   1, 1, 1, 1, 1, 1].
    /// * Stop pattern (encs[39]="1"):         widths [1].
    ///
    /// Captured via `mise exec -- node rust/tools/oracle-posicode.js`.
    #[test]
    fn encode_limiteda_digit_zero_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limiteda single-digit zero path: cw 0 →
        // pattern encs[0]="111411", widths [1,1,1,4,1,1].
        let p = encode("0", &limiteda_opts()).expect(
            "encode(\"0\", limiteda) (POSICODE limiteda single-digit '0' → cw 0 → pattern \"111411\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 5, 1, 1, 1, 1, // start
            1, 1, 1, 4, 1, 1, // '0' → cw 0
            1, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // cbs
            1, // stop
        ];
        assert_eq!(
            p.bars, want,
            "limiteda '0' sbs must match bwip-js byte-for-byte"
        );
    }

    /// Byte-for-byte oracle for `limiteda` "1".
    #[test]
    fn encode_limiteda_digit_one_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limiteda single-digit one path: cw 1 → pattern
        // encs[1]="111312", widths [1,1,1,3,1,2].
        let p = encode("1", &limiteda_opts()).expect(
            "encode(\"1\", limiteda) (POSICODE limiteda single-digit '1' → cw 1 → pattern \"111312\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 5, 1, 1, 1, 1, // start
            1, 1, 1, 3, 1, 2, // '1' → cw 1, pattern "111312"
            1, 3, 1, 1, 1, 5, 1, 2, 1, 1, 1, 2, // cbs from d=[3,2,3,6,2,4]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for `limiteda` "A". 'A' maps to cw 10
    /// (limited charmap row 10), which has pattern encs[10]="171111".
    #[test]
    fn encode_limiteda_uppercase_a_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limiteda uppercase-A path: 'A' → cw 10
        // (limited charmap row 10) → pattern encs[10]="171111".
        let p = encode("A", &limiteda_opts()).expect(
            "encode(\"A\", limiteda) (POSICODE limiteda uppercase 'A' → cw 10 → pattern \"171111\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 5, 1, 1, 1, 1, // start
            1, 7, 1, 1, 1, 1, // 'A' → cw 10, pattern "171111"
            1, 2, 1, 1, 1, 3, 1, 5, 1, 2, 1, 1, // cbs from d=[2,3,6,4,2,3]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for `limiteda` "Z". 'Z' maps to cw 35
    /// (limited charmap row 35), pattern encs[35]="121116".
    #[test]
    fn encode_limiteda_uppercase_z_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limiteda uppercase-Z path (charmap boundary at
        // row 35): 'Z' → cw 35 → pattern encs[35]="121116".
        let p = encode("Z", &limiteda_opts()).expect(
            "encode(\"Z\", limiteda) (POSICODE limiteda uppercase 'Z' → cw 35 charmap-boundary → pattern \"121116\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 5, 1, 1, 1, 1, // start
            1, 2, 1, 1, 1, 6, // 'Z' → cw 35, pattern "121116"
            1, 4, 1, 1, 1, 1, 1, 4, 1, 1, 1, 3, // cbs from d=[4,2,5,2,2,5]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for `limiteda` "0123456789" — a 10-digit
    /// stream that exercises 10 distinct cw patterns plus the
    /// post-normalisation v=296 check decomposition.
    #[test]
    fn encode_limiteda_digit_run_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limiteda 10-digit-run path: exercises 10
        // distinct cw patterns (0..9) plus post-normalisation v=296
        // check decomposition (79-module SBS).
        let p = encode("0123456789", &limiteda_opts()).expect(
            "encode(\"0123456789\", limiteda) (POSICODE limiteda 10-digit run exercising cw 0..9 + v=296 check decomposition; 79-module SBS oracle) must succeed",
        );
        // 79-module sbs captured from bwip-js oracle.
        let want: Vec<u8> = vec![
            1, 5, 1, 1, 1, 1, // start
            1, 1, 1, 4, 1, 1, // '0' → cw 0  / "111411"
            1, 1, 1, 3, 1, 2, // '1' → cw 1  / "111312"
            1, 1, 1, 2, 1, 3, // '2' → cw 2  / "111213"
            1, 1, 1, 1, 1, 4, // '3' → cw 3  / "111114"
            1, 2, 1, 3, 1, 1, // '4' → cw 4  / "121311"
            1, 2, 1, 2, 1, 2, // '5' → cw 5  / "121212"
            1, 2, 1, 1, 1, 3, // '6' → cw 6  / "121113"
            1, 4, 1, 1, 1, 1, // '7' → cw 7  / "141111"
            1, 3, 1, 2, 1, 1, // '8' → cw 8  / "131211"
            1, 3, 1, 1, 1, 2, // '9' → cw 9  / "131112"
            1, 2, 1, 5, 1, 2, 1, 1, 1, 3, 1, 1, // cbs from d=[2,4,2,3,6,3]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Empty input is rejected with a clear error.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("empty")`
    /// upgraded to 2-anchor pin:
    ///   1. `posicode:` symbology prefix
    ///   2. `empty input is not encodable` full predicate (kills
    ///      truncation mutations that drop `is not encodable` or
    ///      substitute another noun).
    #[test]
    fn encode_limiteda_rejects_empty() {
        let err = encode("", &limiteda_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("empty input is not encodable"),
                    "missing full predicate `empty input is not encodable`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(empty), got {other:?}"),
        }
    }

    /// Lowercase letters are not in the limited alphabet.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("limited
    /// alphabet")` upgraded to 4-anchor pin:
    ///   1. `posicode limited:` prefix (kills `limited → normal`
    ///      arm-routing mutation)
    ///   2. `byte 0x68` hex echo for first lowercase 'h' at offset 0
    ///   3. `at position 0` position-echo (kills `{i}` interpolation
    ///      drop)
    ///   4. `0-9, A-Z, '-', '.'` valid-alphabet enumeration (kills
    ///      mutations that drop or extend the enumerated allowed
    ///      characters at line 720 of posicode.rs)
    #[test]
    fn encode_limiteda_rejects_lowercase() {
        let err = encode("hello", &limiteda_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode limited:"),
                    "missing posicode limited prefix: {msg:?}"
                );
                assert!(
                    msg.contains("byte 0x68"),
                    "missing `byte 0x68` hex echo for 'h': {msg:?}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "missing `at position 0` position-echo: {msg:?}"
                );
                assert!(
                    msg.contains("0-9, A-Z, '-', '.'"),
                    "missing valid-alphabet enumeration: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(limited alphabet), got {other:?}"),
        }
    }

    /// Space is in the *normal* charmap row 38 but NOT in the limited
    /// charmap — the limited variants only cover digits, uppercase,
    /// `-` and `.`.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains
    /// ("position 2")` upgraded to 4-anchor pin matching the
    /// lowercase sibling:
    ///   1. `posicode limited:` prefix
    ///   2. `byte 0x20` hex echo for space at offset 2
    ///   3. `at position 2` position-echo
    ///   4. `0-9, A-Z, '-', '.'` valid-alphabet enumeration
    #[test]
    fn encode_limiteda_rejects_space() {
        let err = encode("AB C", &limiteda_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode limited:"),
                    "missing posicode limited prefix: {msg:?}"
                );
                assert!(
                    msg.contains("byte 0x20"),
                    "missing `byte 0x20` hex echo for space: {msg:?}"
                );
                assert!(
                    msg.contains("at position 2"),
                    "missing `at position 2` position-echo: {msg:?}"
                );
                assert!(
                    msg.contains("0-9, A-Z, '-', '.'"),
                    "missing valid-alphabet enumeration: {msg:?}"
                );
            }
            other => panic!("expected InvalidData with position 2, got {other:?}"),
        }
    }

    /// A 501-byte payload exceeds BWIPP's 500-byte upper bound and is
    /// rejected.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains
    /// ("500-byte")` upgraded to 3-anchor pin:
    ///   1. `posicode:` symbology prefix
    ///   2. `exceeds BWIPP's 500-byte limit` full predicate
    ///   3. `payload of 501 bytes` value-echo (kills `{}.len()`
    ///      interpolation drop or hardcoded-literal mutations in
    ///      the format string at line 706 of posicode.rs).
    #[test]
    fn encode_limiteda_rejects_overlong() {
        let payload: String = "A".repeat(501);
        let err = encode(&payload, &limiteda_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-byte limit"),
                    "missing full predicate `exceeds BWIPP's 500-byte limit`: {msg:?}"
                );
                assert!(
                    msg.contains("payload of 501 bytes"),
                    "missing `payload of 501 bytes` value-echo: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(overlong), got {other:?}"),
        }
    }

    /// Kills the `> with >=` payload-length cap mutants at lines ~704
    /// (encode_limited) and ~942 (encode_normal). The original
    /// rejects only `len > 500` (i.e. ≥ 501); the mutant rejects
    /// `len >= 500`, so a payload that exactly saturates the cap
    /// must succeed. We pin both the limited and the normal paths.
    #[test]
    fn payload_length_cap_is_strictly_five_hundred() {
        // limited: 500 chars of 'A' must encode (boundary not yet hit).
        let exactly_500_limited: String = "A".repeat(500);
        encode(&exactly_500_limited, &limiteda_opts())
            .expect("500-byte limiteda payload should encode (boundary not yet hit)");

        // normal (default version=a): 500 chars must also encode.
        let exactly_500_normal: String = "A".repeat(500);
        encode(&exactly_500_normal, &Options::default())
            .expect("500-byte normal posicode payload should encode (boundary not yet hit)");

        // Just past the cap: both paths must error.
        //
        // Stage 11.A8c (cont) — two sibling `matches!(_, InvalidData
        // (msg) if msg.contains("500-byte"))` weak checks upgraded
        // to 3-anchor pins each (matching the dedicated
        // encode_{limiteda,a}_rejects_overlong tests):
        //   1. `posicode:` symbology prefix
        //   2. `exceeds BWIPP's 500-byte limit` full predicate
        //   3. `payload of 501 bytes` value-echo
        let length_501: String = "A".repeat(501);
        match encode(&length_501, &limiteda_opts()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("posicode:"),
                    "limited path: missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-byte limit"),
                    "limited path: missing full predicate: {msg:?}"
                );
                assert!(
                    msg.contains("payload of 501 bytes"),
                    "limited path: missing payload-bytes echo: {msg:?}"
                );
            }
            other => panic!("limited path: expected InvalidData(501 overflow), got {other:?}"),
        }
        match encode(&length_501, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("posicode:"),
                    "normal path: missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-byte limit"),
                    "normal path: missing full predicate: {msg:?}"
                );
                assert!(
                    msg.contains("payload of 501 bytes"),
                    "normal path: missing payload-bytes echo: {msg:?}"
                );
            }
            other => panic!("normal path: expected InvalidData(501 overflow), got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Stage 22c.1 — `limitedb` byte-for-byte goldens.
    //
    // Captured via `mise exec -- node rust/tools/oracle-posicode.js`
    // against bwip-js 4.10.1 / BWIPP 2026-04-21. Every limitedb
    // pattern's module sum is exactly one greater than the matching
    // limiteda pattern (start = "141212" vs "151111"; cw 0 = "121512"
    // vs "111411"). The d[i]+=1 step pushes every check-pattern bar
    // one module wider too.
    // -----------------------------------------------------------------

    fn limitedb_opts() -> Options {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "limitedb".into()));
        opts
    }

    /// **Byte-for-byte limitedb oracle**: digit "0".
    ///
    /// * Start "141212" → [1, 4, 1, 2, 1, 2].
    /// * cw[0]=0 (encs_limitedb[0]="121512") → [1, 2, 1, 5, 1, 2].
    /// * cbs from d=[3,3,3,3,3,11] → [1, 10, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2].
    /// * Stop "1" → [1].
    #[test]
    fn encode_limitedb_digit_zero_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limitedb single-digit zero path: cw 0 → pattern
        // encs_limitedb[0]="121512" (one module wider than limiteda
        // "111411" via the d[i]+=1 step).
        let p = encode("0", &limitedb_opts()).expect(
            "encode(\"0\", limitedb) (POSICODE limitedb single-digit '0' → cw 0 → pattern \"121512\"; +1-module-wider than limiteda) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 4, 1, 2, 1, 2, // start "141212"
            1, 2, 1, 5, 1, 2, // '0' → cw 0, pattern "121512"
            1, 10, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, // cbs from d=[3,3,3,3,3,11]
            1, // stop "1"
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte limitedb oracle for "1".
    #[test]
    fn encode_limitedb_digit_one_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limitedb single-digit one path: cw 1 → pattern
        // encs_limitedb[1]="121413".
        let p = encode("1", &limitedb_opts()).expect(
            "encode(\"1\", limitedb) (POSICODE limitedb single-digit '1' → cw 1 → pattern \"121413\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 4, 1, 2, 1, 2, // start
            1, 2, 1, 4, 1, 3, // '1' → cw 1, pattern "121413"
            1, 4, 1, 2, 1, 6, 1, 3, 1, 2, 1, 3, // cbs from d=[4,3,4,7,3,5]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte limitedb oracle for "A". 'A' → cw 10, pattern
    /// encs_limitedb[10]="181212".
    #[test]
    fn encode_limitedb_uppercase_a_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limitedb uppercase-A path: 'A' → cw 10
        // (limited charmap row 10) → pattern encs_limitedb[10]="181212".
        let p = encode("A", &limitedb_opts()).expect(
            "encode(\"A\", limitedb) (POSICODE limitedb uppercase 'A' → cw 10 → pattern \"181212\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 4, 1, 2, 1, 2, // start
            1, 8, 1, 2, 1, 2, // 'A' → cw 10, pattern "181212"
            1, 3, 1, 2, 1, 4, 1, 6, 1, 3, 1, 2, // cbs from d=[3,4,7,5,3,4]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte limitedb oracle for "Z". 'Z' → cw 35, pattern
    /// encs_limitedb[35]="131217".
    #[test]
    fn encode_limitedb_uppercase_z_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limitedb uppercase-Z path (charmap boundary at
        // row 35): 'Z' → cw 35 → pattern encs_limitedb[35]="131217".
        let p = encode("Z", &limitedb_opts()).expect(
            "encode(\"Z\", limitedb) (POSICODE limitedb uppercase 'Z' → cw 35 charmap-boundary → pattern \"131217\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 4, 1, 2, 1, 2, // start
            1, 3, 1, 2, 1, 7, // 'Z' → cw 35, pattern "131217"
            1, 5, 1, 2, 1, 2, 1, 5, 1, 2, 1, 4, // cbs from d=[5,3,6,3,3,6]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte limitedb oracle for "0123456789". 79-module sbs.
    #[test]
    fn encode_limitedb_digit_run_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE limitedb 10-digit-run path: exercises 10
        // distinct limitedb cw patterns (0..9), each one module wider
        // than the corresponding limiteda pattern (79-module SBS).
        let p = encode("0123456789", &limitedb_opts()).expect(
            "encode(\"0123456789\", limitedb) (POSICODE limitedb 10-digit run exercising limitedb cw 0..9; 79-module SBS oracle) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 4, 1, 2, 1, 2, // start
            1, 2, 1, 5, 1, 2, // '0' → cw 0  / "121512"
            1, 2, 1, 4, 1, 3, // '1' → cw 1  / "121413"
            1, 2, 1, 3, 1, 4, // '2' → cw 2  / "121314"
            1, 2, 1, 2, 1, 5, // '3' → cw 3  / "121215"
            1, 3, 1, 4, 1, 2, // '4' → cw 4  / "131412"
            1, 3, 1, 3, 1, 3, // '5' → cw 5  / "131313"
            1, 3, 1, 2, 1, 4, // '6' → cw 6  / "131214"
            1, 5, 1, 2, 1, 2, // '7' → cw 7  / "151212"
            1, 4, 1, 3, 1, 2, // '8' → cw 8  / "141312"
            1, 4, 1, 2, 1, 3, // '9' → cw 9  / "141213"
            1, 3, 1, 6, 1, 3, 1, 2, 1, 4, 1, 2, // cbs from d=[3,5,3,4,7,4]
            1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Invalid-input contract carries over from limiteda: empty,
    /// lowercase, space, and >500 byte payloads are rejected with
    /// the same error messages (the encoder shares the helper
    /// `encode_limited` for both variants).
    #[test]
    fn encode_limitedb_rejects_invalid_inputs() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Error::InvalidData(_))` to multi-anchor pin
        // per arm matching the source diagnostics in encode_limited
        // (lines 699-722 of posicode.rs). All three arms route
        // through `encode` → `encode_limited` for limitedb.

        // Empty arm (line 700-702: `posicode: empty input is not
        // encodable`):
        match encode("", &limitedb_opts()).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "empty arm: missing `posicode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input is not encodable"),
                    "empty arm: missing `empty input is not encodable` predicate: {msg}"
                );
                assert!(
                    !msg.contains("limited:"),
                    "empty arm: empty path uses bare `posicode:`, not `posicode limited:`: {msg}"
                );
                assert!(
                    !msg.contains("500-byte"),
                    "empty arm: wrong arm — 500-byte cap leaked: {msg}"
                );
            }
            other => panic!("empty input should reject as InvalidData, got {other:?}"),
        }

        // Non-limited-alphabet arm (line 718-721: `posicode limited:
        // byte 0x{b:02x} at position {i} is not in the limited
        // alphabet (0-9, A-Z, '-', '.')`):
        // "hello" starts with 'h' (0x68); not in limited alphabet.
        match encode("hello", &limitedb_opts()).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode limited:"),
                    "non-alphabet arm: missing `posicode limited:` prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x68"),
                    "non-alphabet arm: missing `byte 0x68` hex-echo of 'h': {msg}"
                );
                assert!(
                    msg.contains("position 0"),
                    "non-alphabet arm: missing `position 0` index echo: {msg}"
                );
                assert!(
                    msg.contains("limited alphabet (0-9, A-Z, '-', '.')"),
                    "non-alphabet arm: missing full alphabet spec: {msg}"
                );
            }
            other => panic!("\"hello\" should reject as InvalidData, got {other:?}"),
        }

        // 501-byte overflow arm (line 705-708: `posicode: payload of
        // {n} bytes exceeds BWIPP's 500-byte limit`):
        let payload: String = "A".repeat(501);
        match encode(&payload, &limitedb_opts()).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "overflow arm: missing `posicode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload of 501 bytes"),
                    "overflow arm: missing `payload of 501 bytes` value echo: {msg}"
                );
                assert!(
                    msg.contains("500-byte limit"),
                    "overflow arm: missing `500-byte limit` predicate: {msg}"
                );
                assert!(
                    !msg.contains("limited alphabet"),
                    "overflow arm: wrong arm — limited-alphabet diagnostic leaked: {msg}"
                );
            }
            other => panic!("501-byte payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Symmetry pin: every limiteda d-decomposition + 1 must match
    /// the corresponding limitedb d-decomposition. This is the
    /// BWIPP `$_.d = $_.d.map(x => x + 1)` step exactly.
    #[test]
    fn limitedb_d_is_limiteda_d_plus_one() {
        // Use the same v values as the limiteda oracle.
        for (v, _label) in &[
            (0u32, "0"),
            (553, "1"),
            (272, "A"),
            (889, "Z"),
            (296, "0..9"),
        ] {
            let mut da = decompose_check_digits(*v);
            for di in &mut da {
                *di = di.saturating_add(1);
            }
            // Now da is the synthesized limitedb d. Match it against
            // the actual oracle d for limitedb at that v:
            //   v=0   → d=[3,3,3,3,3,11]
            //   v=553 → d=[4,3,4,7,3,5]
            //   v=272 → d=[3,4,7,5,3,4]
            //   v=889 → d=[5,3,6,3,3,6]
            //   v=296 → d=[3,5,3,4,7,4]
            let want: [u8; 6] = match v {
                0 => [3, 3, 3, 3, 3, 11],
                553 => [4, 3, 4, 7, 3, 5],
                272 => [3, 4, 7, 5, 3, 4],
                889 => [5, 3, 6, 3, 3, 6],
                296 => [3, 5, 3, 4, 7, 4],
                _ => unreachable!(),
            };
            assert_eq!(da, want, "limitedb d for v={v} mismatch");
        }
    }

    // -----------------------------------------------------------------
    // Stage 22d — versions `a` and `b` byte-for-byte goldens.
    //
    // Captured via `mise exec -- node rust/tools/oracle-posicode.js`
    // against bwip-js 4.10.1 / BWIPP 2026-04-21. These pin the
    // auto-encoder state machine end-to-end: set-0 happy-path
    // (digits/uppercase), LA1 latches into set-1 (lowercase),
    // SF1 / SF0 single-char shifts (mixed-case), and SF2 shift to
    // set-2 (control bytes).
    // -----------------------------------------------------------------

    fn version_a_opts() -> Options {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "a".into()));
        opts
    }

    fn version_b_opts() -> Options {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "b".into()));
        opts
    }

    /// `normal_sets()` builds three distinct lookup maps from the
    /// 46-row charmap. Each map is keyed by the literal value
    /// (byte or sentinel) and yields the row index.
    #[test]
    fn normal_sets_lookup_matches_charmap() {
        let sets = normal_sets();
        // Set 0 — printable: digits, uppercase, '-', '.', ' ', '$', '/', '+', '%'.
        assert_eq!(sets[0][&i16::from(b'0')], 0);
        assert_eq!(sets[0][&i16::from(b'9')], 9);
        assert_eq!(sets[0][&i16::from(b'A')], 10);
        assert_eq!(sets[0][&i16::from(b'Z')], 35);
        assert_eq!(sets[0][&i16::from(b'-')], 36);
        assert_eq!(sets[0][&i16::from(b'.')], 37);
        assert_eq!(sets[0][&i16::from(b' ')], 38);
        // Sentinel rows present in set 0:
        //   row 43 col 0 = LA1, row 44 col 0 = SF1, row 45 col 0 = SF2.
        assert_eq!(sets[0][&POSICODE_LA1], 43);
        assert_eq!(sets[0][&POSICODE_SF1], 44);
        assert_eq!(sets[0][&POSICODE_SF2], 45);

        // Set 1 — lowercase + some punctuation.
        assert_eq!(sets[1][&i16::from(b'a')], 10);
        assert_eq!(sets[1][&i16::from(b'z')], 35);
        assert_eq!(sets[1][&POSICODE_LA0], 43);
        assert_eq!(sets[1][&POSICODE_SF0], 44);
        assert_eq!(sets[1][&POSICODE_SF2], 45);

        // Set 2 — control codes 1..=26, 27..=31, 0, '(' (40), ')' (41),
        // plus FNC sentinels.
        assert_eq!(sets[2][&1], 10);
        assert_eq!(sets[2][&26], 35);
        assert_eq!(sets[2][&POSICODE_FN1], 42);
        assert_eq!(sets[2][&POSICODE_FN4], 45);
    }

    /// State-machine unit test: pure set-0 input (digits + uppercase)
    /// never emits latches or shifts.
    #[test]
    fn select_codewords_normal_set0_only() {
        // "HELLO" → set0 lookups: H=17, E=14, L=21, L=21, O=24.
        let msg: Vec<i16> = "HELLO".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![17, 14, 21, 21, 24]);
    }

    /// State-machine unit test: pure set-1 input (lowercase) opens
    /// with a single LA1 latch then emits set-1 codewords directly.
    #[test]
    fn select_codewords_normal_set1_latch() {
        // "abc" → [LA1=43, a=10, b=11, c=12].
        let msg: Vec<i16> = "abc".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![43, 10, 11, 12]);
    }

    /// State-machine unit test: mixed set-0 / set-1 with the
    /// next-byte-still-in-cset path (`"Ab"` is set0 then a single
    /// set1 byte at the end → SF1 then set1[b]).
    #[test]
    fn select_codewords_normal_set0_then_sf1() {
        // "Ab" with char2=-99 — `b` not in set0, set2 doesn't have b,
        // so latch path applies (char2=-99 is not in set0). cws = [A=10, LA1=43, b=11].
        let msg: Vec<i16> = "Ab".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![10, 43, 11]);
    }

    /// State-machine unit test: `"AbC"` — set0 byte 'A', then 'b'
    /// (set1) followed by 'C' (set0) → SF1 single-shift to set1
    /// for 'b', then back to set0 emission for 'C'.
    #[test]
    fn select_codewords_normal_set0_sf1_for_single_char() {
        // "AbC" → [A=10, SF1=44, b=11, C=12].
        let msg: Vec<i16> = "AbC".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![10, 44, 11, 12]);
    }

    /// Kills the `delete -` mutant at line 860 (the char2 sentinel
    /// when i+1 >= msg.len()). The original uses `-99`; the mutant
    /// deletes the negation, leaving `99` (which happens to be 'c'
    /// in i16, an actual character in set 1 of POSICODE_CHARMAPSNORMAL).
    ///
    /// Payload "aaB" (bytes [97, 97, 66]):
    ///   * After "aa": cset latches to 1, cws = [LA1=43, a=10, a=10].
    ///   * At i=2 char1='B'=66 (set 0, not in cset=1) → Path C.
    ///   * char2 = sentinel.
    ///     - Original (-99): `sets[1].contains_key(-99)` = false →
    ///       latch path → emit `set1[LA0]=43`, flip cset=0. Then i=2
    ///       again with cset=0: Path A → emit `set0[B]=11`. Final
    ///       cws = [43, 10, 10, 43, 11].
    ///     - Mutant (99): 99 = 'c' IS in set 1 (row 12 of
    ///       CHARMAPSNORMAL). `sets[1].contains_key(99)` = true →
    ///       shift path → emit `set1[SF0]=44` then `set0[B]=11`,
    ///       i++. Final cws = [43, 10, 10, 44, 11].
    ///
    /// The 4th codeword (43 vs 44) is the kill anchor.
    #[test]
    fn select_codewords_normal_char2_sentinel_is_negative() {
        let msg: Vec<i16> = "aaB".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(
            cws,
            vec![43, 10, 10, 43, 11],
            "select_codewords_normal: at end-of-message in cset=1, \
             a Path-C char must trigger a latch (LA0=43) not a shift \
             (SF0=44). The mutant that removes the negation on the \
             -99 sentinel makes char2=99 ('c'), which IS in set 1, \
             flipping the latch/shift decision."
        );
    }

    /// Kills the `decompose_check_digits` `|| with &&` mutant at
    /// line 556. The bounds check guards against indexing past the
    /// 5×8 POSICODE_C2W table:
    ///   * Original: `r >= 5 || c >= 8` → break when EITHER bound
    ///     is reached.
    ///   * Mutant: `r >= 5 && c >= 8` → break only when BOTH bounds
    ///     are reached, allowing an out-of-bounds `POSICODE_C2W[r][c]`
    ///     access in between.
    ///
    /// The existing tests use small v values (0, 296, 553, 272, 889)
    /// where the loop exits via `sum == v` long before either bound.
    /// We pass `u32::MAX` to force the loop to advance c through the
    /// entire first row of the table without ever matching sum=v:
    ///
    ///   iters 0..=7: r=0, c=0..7, t < v each time → c++, sum
    ///                accumulates the row.
    ///   iter 8: r=0, c=8 → bounds check fires.
    ///     Original: c >= 8 = true → break, return d=[2,2,2,2,2,10].
    ///     Mutant: r < 5 → don't break → POSICODE_C2W[0][8] is
    ///       out-of-bounds → panic.
    ///
    /// Under the original the function returns [2,2,2,2,2,10] (the
    /// initial d[..5]=2 with d[5]=20-10=10); the mutant panics.
    #[test]
    fn decompose_check_digits_bounds_check_breaks_on_either_axis() {
        assert_eq!(
            decompose_check_digits(u32::MAX),
            [2, 2, 2, 2, 2, 10],
            "decompose_check_digits(u32::MAX) should saturate the C2W \
             row 0 walk and return the all-min d-array. The 556 \
             `||→&&` mutant would skip the bound break and panic on \
             POSICODE_C2W[0][8] before the function returns."
        );
    }

    /// Kills the `finalize_sbs` limited-mode range adjustment
    /// mutants at lines 635-636:
    ///   * 635:14 `> with == / >=` (v > 824 boundary)
    ///   * 635:25 `< with == / <=` (v < 853 boundary)
    ///   * 636:15 `+= with -= / *=` (v += 292 arithmetic)
    ///
    /// The `if v > 824 && v < 853 { v += 292 }` clause fires for
    /// 28 distinct post-mask v values. We pin four cases that
    /// exercise both boundaries and the arithmetic:
    ///   * cws=[11]      → v_masked=825 (inside range; +292 applies)
    ///   * cws=[8, 34]   → v_masked=852 (inside range, top boundary)
    ///   * cws=[11, 26]  → v_masked=853 (just outside, no adjustment)
    ///   * cws=[0, 3, 51] → v_masked=824 (just outside, no adjustment)
    ///
    /// Mutation effects:
    ///   * `> with ==` flips behavior at v=824 (false → true).
    ///   * `> with >=` flips behavior at v=824 (false → true).
    ///   * `< with ==` flips behavior at v=852 (true → false: v != 853).
    ///   * `< with <=` flips behavior at v=853 (false → true).
    ///   * `+= with -=` changes the v=825 case from 1117 to 533.
    ///   * `+= with *=` changes the v=825 case from 1117 to 240900.
    ///
    /// We pin finalize_sbs(&cws, LimitedA).len() and a checksum byte
    /// for each case. The four expected lengths/checksums are
    /// extracted from the current encoder output.
    #[test]
    fn finalize_sbs_limited_range_adjustment() {
        // Lengths and checksums for the four test points. We don't
        // need the exact bars (those depend on the d-decomposition
        // and cbs build); a (length, sum) tuple is enough to
        // distinguish original from any of the 6 mutants.
        let cases: &[(&[u8], &str)] = &[
            (&[11u8], "v=825 inside-range, +=292 applies"),
            (&[8, 34], "v=852 inside-range top-boundary"),
            (&[11, 26], "v=853 just-outside top-boundary"),
            (&[1, 0, 33], "v=824 just-outside bottom-boundary"),
        ];
        // Pin the full output bars for each case. (length, byte_sum)
        // was too coarse to catch range-adjustment mutations; the
        // bar sequences are short (25-37 bytes), so we can anchor
        // them in full.
        let bars: Vec<Vec<u8>> = cases
            .iter()
            .map(|(cws, _)| finalize_sbs(cws, PosicodeVersion::LimitedA))
            .collect();
        assert_eq!(
            bars,
            vec![
                // cws=[11]: v=825 inside-range, +292 applies → v=1117.
                vec![1, 5, 1, 1, 1, 1, 1, 6, 1, 2, 1, 1, 1, 2, 1, 2, 1, 1, 1, 3, 1, 2, 1, 4, 1,],
                // cws=[8, 34]: v=852 inside-range top boundary, +292 → v=1144.
                vec![
                    1, 5, 1, 1, 1, 1, 1, 3, 1, 2, 1, 1, 1, 1, 1, 3, 1, 5, 1, 1, 1, 1, 1, 2, 1, 3,
                    1, 3, 1, 4, 1,
                ],
                // cws=[11, 26]: v=853 just-outside, no adjustment.
                vec![
                    1, 5, 1, 1, 1, 1, 1, 6, 1, 2, 1, 1, 1, 2, 1, 4, 1, 3, 1, 6, 1, 1, 1, 1, 1, 2,
                    1, 1, 1, 3, 1,
                ],
                // cws=[1, 0, 33]: v=824 just-outside bottom, no adjustment.
                vec![
                    1, 5, 1, 1, 1, 1, 1, 1, 1, 3, 1, 2, 1, 1, 1, 4, 1, 1, 1, 2, 1, 2, 1, 5, 1, 1,
                    1, 1, 1, 1, 1, 1, 1, 8, 1, 2, 1,
                ],
            ],
            "finalize_sbs bars shifted — one of the 635-636 mutants on \
             the limited-mode v range adjustment activated; the cws \
             inputs hit v ∈ {{824, 825, 852, 853}} after the v&=1023 mask, \
             so any of the boundary or arithmetic mutants diverges the \
             d-decomposition and the cbs bar pattern"
        );
    }

    /// State-machine unit test: `"A\x01"` — 'A' in set0, then
    /// control byte 0x01 only in set 2 → SF2 shift.
    #[test]
    fn select_codewords_normal_sf2_for_control_byte() {
        // "A\x01" → [A=10, SF2=45, set2[0x01]=10].
        let msg: Vec<i16> = vec![i16::from(b'A'), 1];
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![10, 45, 10]);
    }

    /// State-machine unit test: `"aB"` — 'a' (set1) then 'B' (set0)
    /// while still in set0 → SF1 shift for 'a', stay in set0 for 'B'.
    #[test]
    fn select_codewords_normal_set0_sf1_at_start() {
        // "aB" — char1='a' not in set0, not in set2; char2='B' IS in
        // set0 → shift path. cset stays set0; emit SF1=44, set1[a]=10,
        // then set0[B]=11. Result: [44, 10, 11].
        let msg: Vec<i16> = "aB".bytes().map(i16::from).collect();
        let cws = select_codewords_normal(&msg);
        assert_eq!(cws, vec![44, 10, 11]);
    }

    /// **Byte-for-byte oracle**: version-`a` "0" — single digit,
    /// simplest cws=[0] case. 37-module sbs.
    #[test]
    fn encode_a_digit_zero_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a single-digit zero path: cws=[0] →
        // pattern encs_a[0]="141112"; 37-module SBS.
        let p = encode("0", &version_a_opts()).expect(
            "encode(\"0\", version_a) (POSICODE version-a single-digit '0' → cw 0 → pattern \"141112\"; 37-module SBS) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start "1<111112"
            1, 4, 1, 1, 1, 2, // cw 0 → "141112"
            1, 8, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, // cbs from d=[2,2,3,2,2,9]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop "111111111;1"
        ];
        assert_eq!(
            p.bars, want,
            "version a '0' sbs must match bwip-js byte-for-byte"
        );
    }

    /// Byte-for-byte oracle for version-`a` "1".
    #[test]
    fn encode_a_digit_one_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a single-digit one path: cw 1 → pattern
        // encs_a[1]="131212".
        let p = encode("1", &version_a_opts()).expect(
            "encode(\"1\", version_a) (POSICODE version-a single-digit '1' → cw 1 → pattern \"131212\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 3, 1, 2, 1, 2, // cw 1 → "131212"
            1, 1, 1, 4, 1, 1, 1, 5, 1, 1, 1, 2, // cbs from d=[3,2,6,2,5,2]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "A".
    #[test]
    fn encode_a_uppercase_a_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a set-0 uppercase-A path: 'A' (set 0)
        // → cw 10 → pattern encs_a[10]="181111".
        let p = encode("A", &version_a_opts()).expect(
            "encode(\"A\", version_a) (POSICODE version-a set-0 uppercase 'A' → cw 10 → pattern \"181111\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 8, 1, 1, 1, 1, // cw 10 → "181111"
            1, 2, 1, 5, 1, 1, 1, 2, 1, 3, 1, 1, // cbs from d=[2,4,3,2,6,3]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "Z" — last single-byte
    /// uppercase in set 0.
    #[test]
    fn encode_a_uppercase_z_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a set-0 uppercase-Z path (last set-0
        // single-byte): 'Z' → cw 35 → pattern encs_a[35]="111514".
        let p = encode("Z", &version_a_opts()).expect(
            "encode(\"Z\", version_a) (POSICODE version-a set-0 uppercase 'Z' last-set-0 → cw 35 → pattern \"111514\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 1, 1, 5, 1, 4, // cw 35 → "111514"
            1, 1, 1, 5, 1, 1, 1, 2, 1, 2, 1, 3, // cbs from d=[4,3,3,2,6,2]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "HELLO" — 5-byte set-0
    /// stream, no latches/shifts/FN4.
    #[test]
    fn encode_a_hello_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a 5-byte set-0 stream path: HELLO ⇒
        // [17, 14, 21, 21, 24] — no latches/shifts/FN4 triggered;
        // pure set-0 emission.
        let p = encode("HELLO", &version_a_opts()).expect(
            "encode(\"HELLO\", version_a) (POSICODE version-a 5-byte set-0 stream → cws [17,14,21,21,24] without latch/shift/FN4) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 1, 1, 8, 1, 1, // 'H' → cw 17 / "111811"
            1, 4, 1, 5, 1, 1, // 'E' → cw 14 / "141511"
            1, 4, 1, 4, 1, 2, // 'L' → cw 21 / "141412"
            1, 4, 1, 4, 1, 2, // 'L' → cw 21
            1, 1, 1, 7, 1, 2, // 'O' → cw 24 / "111712"
            1, 3, 1, 2, 1, 2, 1, 1, 1, 3, 1, 3, // cbs from d=[4,4,2,3,3,4]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "12345" — 5 distinct
    /// digits, no latches/shifts.
    #[test]
    fn encode_a_digit_run_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a 5-digit run path: 5 distinct set-0
        // digits cw 1..5, no latches/shifts.
        let p = encode("12345", &version_a_opts()).expect(
            "encode(\"12345\", version_a) (POSICODE version-a 5-digit set-0 run → cws [1,2,3,4,5] without latch/shift) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 3, 1, 2, 1, 2, // '1' → cw 1
            1, 2, 1, 3, 1, 2, // '2' → cw 2 / "121312"
            1, 1, 1, 4, 1, 2, // '3' → cw 3 / "111412"
            1, 3, 1, 1, 1, 3, // '4' → cw 4 / "131113"
            1, 2, 1, 2, 1, 3, // '5' → cw 5 / "121213"
            1, 6, 1, 1, 1, 2, 1, 2, 1, 2, 1, 1, // cbs from d=[2,3,3,3,2,7]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "abc" — pure lowercase
    /// stream triggers LA1 at position 0.
    #[test]
    fn encode_a_lowercase_run_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a LA1 latch path: pure-lowercase input
        // triggers LA1 (cw 43) at position 0, then set-1 emission for
        // 'a','b','c'.
        let p = encode("abc", &version_a_opts()).expect(
            "encode(\"abc\", version_a) (POSICODE version-a LA1 latch at pos 0 → set-1 emission for 'abc' (cws 10,11,12)) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 2, 1, 1, 1, 7, // LA1 → cw 43 in set0 / "121117"
            1, 8, 1, 1, 1, 1, // 'a' → cw 10 in set1 / "181111"
            1, 7, 1, 2, 1, 1, // 'b' → cw 11 in set1 / "171211"
            1, 6, 1, 3, 1, 1, // 'c' → cw 12 in set1 / "161311"
            1, 1, 1, 3, 1, 5, 1, 3, 1, 1, 1, 1, // cbs from d=[2,2,4,6,4,2]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "Ab" — LA1 mid-message
    /// (set0 → set1).
    #[test]
    fn encode_a_la1_mid_message_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a LA1 mid-message latch path: 'A' in
        // set 0, then LA1 cw 43 latches to set 1, then 'b' in set 1.
        let p = encode("Ab", &version_a_opts()).expect(
            "encode(\"Ab\", version_a) (POSICODE version-a LA1 mid-message latch: set0 → set1 between 'A' and 'b' (cws 10,43,11)) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 / "181111"
            1, 2, 1, 1, 1, 7, // LA1 → cw 43 / "121117"
            1, 7, 1, 2, 1, 1, // 'b' → cw 11 / "171211"
            1, 4, 1, 1, 1, 2, 1, 3, 1, 1, 1, 3, // cbs from d=[4,2,4,3,2,5]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "AbC" — SF1 single-shift
    /// from set0 (not a latch because char2='C' is back in set0).
    #[test]
    fn encode_a_sf1_single_shift_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a SF1 single-shift path: 'A' in set 0,
        // then SF1 cw 44 single-shifts to set 1 for 'b', then 'C' is
        // back in set 0 — distinguishes shift (no latch) from LA1.
        let p = encode("AbC", &version_a_opts()).expect(
            "encode(\"AbC\", version_a) (POSICODE version-a SF1 single-shift; set0 → set1 for 'b' only, return to set0 for 'C' (cws 10,44,11,12); guards SF1 vs LA1 confusion) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 / "181111"
            1, 1, 1, 2, 1, 7, // SF1 → cw 44 / "111217"
            1, 7, 1, 2, 1, 1, // 'b' → cw 11 in set1 / "171211"
            1, 6, 1, 3, 1, 1, // 'C' → cw 12 in set0 / "161311"
            1, 2, 1, 4, 1, 4, 1, 1, 1, 1, 1, 2, // cbs from d=[3,2,2,5,5,3]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "A\x01" — SF2 single-shift
    /// to set 2 for a control byte.
    #[test]
    fn encode_a_sf2_control_byte_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a SF2 single-shift-to-set-2 path: 'A'
        // in set 0, then SF2 cw 45 single-shifts to set 2 for the
        // control byte 0x01 — distinguishes SF2 from SF1.
        let p = encode("A\x01", &version_a_opts()).expect(
            "encode(\"A\\x01\", version_a) (POSICODE version-a SF2 single-shift-to-set-2 for control byte; cws 10,45,10 distinguishing SF2 vs SF1) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 / "181111"
            1, 1, 1, 1, 1, 8, // SF2 → cw 45 in set0 / "111118"
            1, 8, 1, 1, 1, 1, // 0x01 → cw 10 in set2 / "181111"
            1, 1, 1, 1, 1, 1, 1, 5, 1, 4, 1, 2, // cbs from d=[3,5,6,2,2,2]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`b` "0" — exercises the
    /// wider bar table + d[i]+=1 step end-to-end.
    #[test]
    fn encode_b_digit_zero_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-b single-digit zero path: end-to-end
        // exercise of the wider bar table + d[i]+=1 step.
        let p = encode("0", &version_b_opts()).expect(
            "encode(\"0\", version_b) (POSICODE version-b single-digit '0' → cw 0 → pattern \"151213\"; exercises wider bar table + d[i]+=1) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 2, 1, 3, 1, 2, // start "1<121312"
            1, 5, 1, 2, 1, 3, // cw 0 → "151213"
            1, 9, 1, 2, 1, 2, 1, 3, 1, 2, 1, 2, // cbs from d=[3,3,4,3,3,10]
            1, 2, 1, 2, 1, 2, 1, 2, 1, 12, 1, // stop "121212121<1"
        ];
        assert_eq!(
            p.bars, want,
            "version b '0' sbs must match bwip-js byte-for-byte"
        );
    }

    /// Byte-for-byte oracle for version-`b` "A".
    #[test]
    fn encode_b_uppercase_a_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-b set-0 uppercase-A path: 'A' (set 0)
        // → cw 10 → pattern encs_b[10]="191212".
        let p = encode("A", &version_b_opts()).expect(
            "encode(\"A\", version_b) (POSICODE version-b set-0 uppercase 'A' → cw 10 → pattern \"191212\") must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 2, 1, 3, 1, 2, // start
            1, 9, 1, 2, 1, 2, // cw 10 → "191212"
            1, 3, 1, 6, 1, 2, 1, 3, 1, 4, 1, 2, // cbs from d=[3,5,4,3,7,4]
            1, 2, 1, 2, 1, 2, 1, 2, 1, 12, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`b` "HELLO".
    #[test]
    fn encode_b_hello_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-b 5-byte set-0 stream path: HELLO ⇒
        // [17, 14, 21, 21, 24] under version-b's wider bar table.
        let p = encode("HELLO", &version_b_opts()).expect(
            "encode(\"HELLO\", version_b) (POSICODE version-b 5-byte set-0 stream → cws [17,14,21,21,24] under wider bar table) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 2, 1, 3, 1, 2, // start
            1, 2, 1, 9, 1, 2, // 'H' → cw 17 / "121912"
            1, 5, 1, 6, 1, 2, // 'E' → cw 14 / "151612"
            1, 5, 1, 5, 1, 3, // 'L' → cw 21 / "151513"
            1, 5, 1, 5, 1, 3, // 'L' → cw 21
            1, 2, 1, 8, 1, 3, // 'O' → cw 24 / "121813"
            1, 4, 1, 3, 1, 3, 1, 2, 1, 4, 1, 4, // cbs from d=[5,5,3,4,4,5]
            1, 2, 1, 2, 1, 2, 1, 2, 1, 12, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`b` "abc" — verifies the
    /// latch path works for the `b` table too.
    #[test]
    fn encode_b_lowercase_run_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-b LA1 latch path: verifies LA1 cw 43
        // works for the version-b table too (encs_b[43]="131218").
        let p = encode("abc", &version_b_opts()).expect(
            "encode(\"abc\", version_b) (POSICODE version-b LA1 latch at pos 0 → set-1 emission for 'abc' under version-b bar table) must succeed",
        );
        let want: Vec<u8> = vec![
            1, 12, 1, 2, 1, 3, 1, 2, // start
            1, 3, 1, 2, 1, 8, // LA1 → cw 43 / "131218"
            1, 9, 1, 2, 1, 2, // 'a' → cw 10 / "191212"
            1, 8, 1, 3, 1, 2, // 'b' → cw 11 / "181312"
            1, 7, 1, 4, 1, 2, // 'c' → cw 12 / "171412"
            1, 2, 1, 4, 1, 6, 1, 4, 1, 2, 1, 2, // cbs from d=[3,3,5,7,5,3]
            1, 2, 1, 2, 1, 2, 1, 2, 1, 12, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Empty input is rejected with a clear error.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("empty")`
    /// upgraded to 2-anchor pin matching the limiteda sibling
    /// (encode_normal arm parallel at line 939 of posicode.rs):
    ///   1. `posicode:` symbology prefix
    ///   2. `empty input is not encodable` full predicate
    #[test]
    fn encode_a_rejects_empty() {
        let err = encode("", &version_a_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("empty input is not encodable"),
                    "missing full predicate `empty input is not encodable`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(empty), got {other:?}"),
        }
    }

    /// A 501-byte payload exceeds BWIPP's 500-byte upper bound.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains
    /// ("500-byte")` upgraded to 3-anchor pin matching the
    /// limiteda sibling pattern:
    ///   1. `posicode:` symbology prefix
    ///   2. `exceeds BWIPP's 500-byte limit` full predicate
    ///   3. `payload of 501 bytes` value-echo (kills `{}.len()`
    ///      interpolation drop or hardcoded-literal mutations in
    ///      the format string at line 944 of posicode.rs — the
    ///      `encode_normal` arm parallel to `encode_limited` at
    ///      line 706 already covered by limiteda_rejects_overlong).
    #[test]
    fn encode_a_rejects_overlong() {
        let payload: String = "A".repeat(501);
        let err = encode(&payload, &version_a_opts()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("posicode:"),
                    "missing posicode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-byte limit"),
                    "missing full predicate `exceeds BWIPP's 500-byte limit`: {msg:?}"
                );
                assert!(
                    msg.contains("payload of 501 bytes"),
                    "missing `payload of 501 bytes` value-echo: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(overlong), got {other:?}"),
        }
    }

    /// `insert_fn4_markers` is an identity transform for pure
    /// standard-ASCII input — no transitions, no FN4s inserted.
    #[test]
    fn fn4_insertion_is_identity_for_ascii_only() {
        let msg: Vec<i16> = "HELLO".bytes().map(i16::from).collect();
        let out = insert_fn4_markers(&msg);
        assert_eq!(out, msg);
    }

    /// `insert_fn4_markers` for a single trailing extended-ASCII
    /// byte: single FN4 (shift) at the boundary, byte's high bit
    /// stripped. UTF-8 encoded `"A\u{0080}"` = `[0x41, 0xc2, 0x80]`,
    /// so we have two extended-ASCII bytes at the end and BWIPP
    /// inserts a single FN4 (run=2 < threshold 3).
    #[test]
    fn fn4_insertion_single_shift_for_trailing_extended() {
        let msg: Vec<i16> = "A\u{0080}".bytes().map(i16::from).collect();
        // bytes = [0x41, 0xc2, 0x80]; numEA[1] = 2.
        // Walking: i=0 standard, no FN4. i=1 first extended,
        // run=2 < 3 threshold → single FN4. i=2 still standard
        // mode (ea didn't flip) → another single FN4.
        let out = insert_fn4_markers(&msg);
        assert_eq!(
            out,
            vec![0x41, POSICODE_FN4, 0xc2 & 0x7f, POSICODE_FN4, 0x80 & 0x7f]
        );
    }

    /// Kills the `insert_fn4_markers` ea-flip-path mutants at line
    /// ~818 (`run + i == msglen` boundary) and ~822 (`ea = !ea`
    /// toggle). With a single extended byte at the end the existing
    /// test takes the "run < threshold" path; we add a payload where
    /// the run is *long enough* to trigger the mode flip and pin
    /// the double-FN4 emission + later ASCII bytes coming out with
    /// their high bit stripped via the ea-side state.
    ///
    /// Payload "A\u{0080}\u{0081}\u{0082}" → bytes
    ///   [0x41, 0xC2, 0x80, 0xC2, 0x81, 0xC2, 0x82] (msglen=7).
    /// Walk:
    ///   i=0 c=0x41 standard → push 0x41 (no FN4)
    ///   i=1 c=0xC2 extended → run=num_ea[1]=6, threshold=3 (run+i=7
    ///        == msglen=7), run≥threshold → ea=!ea=true, push 2×FN4,
    ///        push 0xC2 & 127 = 0x42
    ///   i=2 c=0x80 extended → ea matches → no FN4, push 0x00
    ///   i=3 c=0xC2 extended → ea matches → no FN4, push 0x42
    ///   i=4 c=0x81 extended → ea matches → no FN4, push 0x01
    ///   i=5 c=0xC2 extended → ea matches → no FN4, push 0x42
    ///   i=6 c=0x82 extended → ea matches → no FN4, push 0x02
    ///
    /// The `818 + with *` mutant flips `run + i == msglen` to
    /// `run * i == msglen`: 6*1=6≠7 → threshold becomes 5, run=6 ≥ 5
    /// still flips → same output here. So this test alone doesn't
    /// kill `+ with *` — but it does kill the `==!=` flip at 818
    /// (which would make the threshold expression evaluate to the
    /// wrong branch), and the `delete !` mutant at 822 (which would
    /// leave ea=false so every subsequent extended byte triggers
    /// another FN4 — the assertion against the expected vector
    /// fails immediately).
    #[test]
    fn fn4_insertion_flips_mode_on_long_extended_run() {
        let msg: Vec<i16> = "A\u{0080}\u{0081}\u{0082}".bytes().map(i16::from).collect();
        let out = insert_fn4_markers(&msg);
        assert_eq!(
            out,
            vec![
                0x41,         // standard 'A'
                POSICODE_FN4, // FN4 #1 (mode flip)
                POSICODE_FN4, // FN4 #2 (mode flip)
                0xC2 & 0x7f,  // 0x42, ea-mode (high bit stripped)
                0x80 & 0x7f,  // 0x00
                0xC2 & 0x7f,  // 0x42
                0x81 & 0x7f,  // 0x01
                0xC2 & 0x7f,  // 0x42
                0x82 & 0x7f,  // 0x02
            ],
            "insert_fn4_markers: a run of 6 extended bytes after a single \
             standard byte must flip the ea state, emit 2×FN4, and pass \
             the subsequent extended bytes through with their high bit \
             stripped (no further FN4 markers)"
        );
    }

    /// Kills the `insert_fn4_markers` num_sa pre-pass mutant at line
    /// ~805 (`num_sa[i] = num_sa[i + 1] + 1`). The mutant
    /// `num_sa[i + 1] * 1 = num_sa[i + 1]` makes num_sa zero
    /// everywhere — the standard-run counter never accumulates.
    ///
    /// To observe the effect we need a payload that flips ea to true
    /// (via a long extended-byte run) THEN has a long standard run
    /// in ea mode. Under the original, the run length tells the
    /// encoder to flip ea back; under the mutant, run is always 0,
    /// so single-FN4 markers get inserted before EVERY standard byte
    /// instead of a single mode-flip.
    ///
    /// Payload "\u{0080}\u{0081}\u{0082}AAAAAA" →
    ///   bytes [0xC2, 0x80, 0xC2, 0x81, 0xC2, 0x82, 0x41, 0x41,
    ///          0x41, 0x41, 0x41, 0x41] (msglen=12).
    ///
    /// Original walk (after the extended run flips ea=true at i=0):
    ///   i=6 c=0x41 standard in ea-mode: run=num_sa[6]=6,
    ///       threshold = if 6+6==12 { 3 } else { 5 } = 3,
    ///       run(6) ≥ threshold(3) → flip ea=false, push 2×FN4.
    ///   i=7..11 standard, ea=false: condition false (ea matches
    ///       byte's natural side), push directly without FN4.
    ///
    /// The mutant keeps num_sa[6] = 0, so the threshold check
    /// becomes `0 < 5 = true` → single FN4. ea stays true forever
    /// and every subsequent standard byte at i=7..11 gets ANOTHER
    /// single FN4. Total FN4 count + bar layout diverges.
    #[test]
    fn fn4_insertion_num_sa_pre_pass_accumulates() {
        let msg: Vec<i16> = "\u{0080}\u{0081}\u{0082}AAAAAA"
            .bytes()
            .map(i16::from)
            .collect();
        let out = insert_fn4_markers(&msg);
        assert_eq!(
            out,
            vec![
                POSICODE_FN4, // FN4 #1 (mode flip at i=0)
                POSICODE_FN4, // FN4 #2 (mode flip at i=0)
                0xC2 & 0x7f,  // 0x42 (ea-mode)
                0x80 & 0x7f,  // 0x00
                0xC2 & 0x7f,  // 0x42
                0x81 & 0x7f,  // 0x01
                0xC2 & 0x7f,  // 0x42
                0x82 & 0x7f,  // 0x02
                POSICODE_FN4, // FN4 #3 (mode flip BACK at i=6)
                POSICODE_FN4, // FN4 #4 (mode flip BACK at i=6)
                0x41,         // 'A' (standard-mode)
                0x41,
                0x41,
                0x41,
                0x41,
                0x41,
            ],
            "insert_fn4_markers: after the extended-byte run flips ea=true, \
             the 6-byte standard run at i=6..11 must trigger a SINGLE \
             mode-flip-back (ea=false + 2×FN4) at i=6, then the remaining \
             5 standard bytes pass through without FN4. The mutant on \
             line 805 (num_sa[i+1]+1 → *1) zeros the run counter, so the \
             threshold check evaluates to 0<5=true and each of the 6 \
             standard bytes gets its own FN4."
        );
    }

    /// Kills the `insert_fn4_markers` threshold-boundary mutants at
    /// lines 818-819:
    ///   * 818:40 `== with !=` (run + i == msglen → flips threshold)
    ///   * 818:36 `+ with *` (run + i → run * i, same flip target)
    ///   * 819:20 `< with <=` (run < threshold → run <= threshold)
    ///
    /// Synthetic msg `[0x41, 0x41, 0xC2, 0x80, 0xC2]` (msglen=5):
    /// num_ea[2]=3 (run of 3 extended bytes starting at i=2),
    /// num_sa[0]=2 (run of 2 standard bytes from i=0).
    ///
    /// Walk:
    ///   i=0,1 standard, ea=false, no FN4.
    ///   i=2 extended: run=3, i=2, msglen=5 →
    ///       Original (`3+2==5`): threshold=3 → run<threshold? 3<3
    ///         false → enter else: flip ea=true, push 2×FN4.
    ///       818:40 `!=` mutant: 3+2!=5 false → threshold=5 →
    ///         3<5 true → single FN4 (no flip).
    ///       818:36 `*` mutant: 3*2==5 false → threshold=5 →
    ///         3<5 true → single FN4.
    ///       819:20 `<=` mutant on original threshold=3:
    ///         3<=3 true → single FN4 (no flip).
    ///
    /// Original output ends with [..., FN4, FN4, 0x42, ...]
    /// (mode flip). Mutants emit a single FN4 then keep processing
    /// in the wrong ea state.
    #[test]
    fn fn4_insertion_threshold_boundary_at_three_run_length() {
        let msg: Vec<i16> = vec![0x41, 0x41, 0xC2, 0x80, 0xC2];
        let out = insert_fn4_markers(&msg);
        assert_eq!(
            out,
            vec![
                0x41,         // standard at i=0
                0x41,         // standard at i=1
                POSICODE_FN4, // FN4 #1 (mode flip at i=2: run==threshold==3)
                POSICODE_FN4, // FN4 #2 (mode flip at i=2)
                0xC2 & 0x7f,  // 0x42 (ea-mode)
                0x80 & 0x7f,  // 0x00 (ea-mode, no FN4)
                0xC2 & 0x7f,  // 0x42 (ea-mode, no FN4)
            ],
            "insert_fn4_markers: msg=[0x41,0x41,0xC2,0x80,0xC2] places \
             run+i (3+2=5) exactly at msglen=5, picking the smaller \
             threshold=3. run==threshold, so run<threshold is false → \
             enter mode-flip branch. The 818-819 mutants all flip the \
             condition's outcome, emitting a single FN4 instead and \
             leaving ea=false for the subsequent extended bytes."
        );
    }

    /// **Byte-for-byte oracle (FN4)**: version-`a` "A\u{0080}" — the
    /// state machine emits 8 codewords for the 3-byte UTF-8 input
    /// via two SF2 shifts for the FN4 sentinels.
    #[test]
    fn encode_a_with_fn4_extended_byte_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a FN4 extended-byte path: input
        // "A\u{0080}" (3-byte UTF-8) ⇒ 8 cws including 5 SF2 shifts
        // (4 around two FN4 sentinels), exercising the FN4 state
        // machine end-to-end.
        let p = encode("A\u{0080}", &version_a_opts()).expect(
            "encode(\"A\\u{0080}\", version_a) (POSICODE version-a FN4 extended-byte path; 3-byte UTF-8 → 8 cws with 5 SF2 shifts incl. 2 FN4 sentinels) must succeed",
        );
        // Oracle: cws=[10, 45, 45, 11, 45, 45, 45, 38], d=[3,8,2,2,2,3].
        // 8 cws × 6 mod + 8 start + 12 cbs + 11 stop = 79 modules.
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 in set0 / "181111"
            1, 1, 1, 1, 1, 8, // SF2 → cw 45 / "111118"
            1, 1, 1, 1, 1, 8, // set2[FN4] → cw 45 / "111118"
            1, 7, 1, 2, 1, 1, // 0x42 ('B') → cw 11 in set0 / "171211"
            1, 1, 1, 1, 1, 8, // SF2 → cw 45 / "111118"
            1, 1, 1, 1, 1, 8, // set2[FN4] → cw 45 / "111118"
            1, 1, 1, 1, 1, 8, // SF2 → cw 45 / "111118"
            1, 2, 1, 3, 1, 5, // set2[0] → cw 38 / "121315"
            1, 2, 1, 1, 1, 1, 1, 1, 1, 7, 1, 2, // cbs from d=[3,8,2,2,2,3]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Byte-for-byte oracle for version-`a` "\u{0081}A" — extended-
    /// then-standard at message head. UTF-8 = [0xc2, 0x81, 0x41].
    /// Oracle cws = [45, 45, 12, 45, 45, 45, 10, 10].
    #[test]
    fn encode_a_with_fn4_leading_extended_matches_bwip_js_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the POSICODE version-a FN4 leading-extended path: input
        // "\u{00c1}A" (extended-then-standard at message head) ⇒
        // FN4 must trigger at position 0 before any set-0 emission.
        let p = encode("\u{00c1}A", &version_a_opts()).expect(
            "encode(\"\\u{00c1}A\", version_a) (POSICODE version-a FN4 leading-extended path; FN4 sentinel at pos 0 before any set-0 cw) must succeed",
        );
        // \u{00c1} = UTF-8 [0xc3, 0x81].
        // Oracle from "ÁA": cws=[45, 45, 12, 45, 45, 45, 10, 10],
        // d=[4, 2, 4, 6, 2, 2].
        let want: Vec<u8> = vec![
            1, 12, 1, 1, 1, 1, 1, 2, // start
            1, 1, 1, 1, 1, 8, // SF2 → cw 45
            1, 1, 1, 1, 1, 8, // set2[FN4] → cw 45
            1, 6, 1, 3, 1, 1, // 0x43 ('C') → cw 12 in set0 / "161311"
            1, 1, 1, 1, 1, 8, // SF2 → cw 45
            1, 1, 1, 1, 1, 8, // set2[FN4] → cw 45
            1, 1, 1, 1, 1, 8, // SF2 → cw 45
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 (set0[A]=10) / "181111"
            1, 8, 1, 1, 1, 1, // 'A' → cw 10 / "181111"
            1, 1, 1, 1, 1, 5, 1, 3, 1, 1, 1, 3, // cbs from d=[4,2,4,6,2,2]
            1, 1, 1, 1, 1, 1, 1, 1, 1, 11, 1, // stop
        ];
        assert_eq!(p.bars, want);
    }

    /// Stage 11.A8c — pin `lookup_limited(b)`. POSICODE LimitedA/B
    /// share a 38-entry charmap: digits 0..=9, uppercase A..Z, then
    /// '-' and '.'. The helper does a position search and returns
    /// the row index as u8, else None.
    ///
    /// 5 happy + 4 rejection arms — only exercised end-to-end via
    /// POSICODE LimitedA/B goldens.
    ///
    /// Mutations killed:
    ///   * `row[0]` → `row[1]` or `row[2]` (would compare against
    ///     LIMITED_NA, never match);
    ///   * `position(...)` → other iter combinators (return value
    ///     drift);
    ///   * `i as u8` width truncation (table has 38 entries, fits u8).
    #[test]
    fn lookup_limited_table_anchors_plus_boundary_rejections() {
        // Digits: 0..=9 → 0..=9.
        for d in 0..=9u8 {
            assert_eq!(
                lookup_limited(b'0' + d),
                Some(d),
                "digit '{}' → {d}",
                (b'0' + d) as char
            );
        }
        // Letters: A..=Z → 10..=35.
        for (i, c) in (b'A'..=b'Z').enumerate() {
            assert_eq!(
                lookup_limited(c),
                Some(10 + i as u8),
                "letter '{}' → {}",
                c as char,
                10 + i as u8
            );
        }
        // Boundary anchors: '-' → 36, '.' → 37.
        assert_eq!(lookup_limited(b'-'), Some(36), "'-' → 36 (penultimate)");
        assert_eq!(lookup_limited(b'.'), Some(37), "'.' → 37 (last)");

        // Rejections: lowercase, space, '/', '\0', other punct.
        assert_eq!(lookup_limited(b'a'), None, "lowercase 'a' not in limited");
        assert_eq!(lookup_limited(b'z'), None, "lowercase 'z' not in limited");
        assert_eq!(lookup_limited(b' '), None, "space not in limited");
        // '/' is between '.' (46) and '0' (48), tests that '/' just
        // before '0' doesn't sneak into the digit range.
        assert_eq!(lookup_limited(b'/'), None, "'/' just before '0'");
        // ':' just after '9'.
        assert_eq!(lookup_limited(b':'), None, "':' just after '9'");
        // '@' just before 'A'.
        assert_eq!(lookup_limited(b'@'), None, "'@' just before 'A'");
        // '[' just after 'Z'.
        assert_eq!(lookup_limited(b'['), None, "'[' just after 'Z'");
        assert_eq!(lookup_limited(0), None, "NUL not in limited");
        assert_eq!(lookup_limited(255), None, "0xFF not in limited");
    }
}
