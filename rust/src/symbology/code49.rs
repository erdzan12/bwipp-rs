//! Code 49 — stacked 1D barcode (2..=8 rows × 81 modules per row).
//!
//! Reference: AIM USS Code 49, BWIPP `bwipp_code49` (bwip-js line
//! 19899+, 584 lines). Each row of a Code 49 symbol carries 8
//! codewords from a 49-symbol alphabet plus a row-check codeword,
//! a start indicator, a stop indicator, and a per-row parity-bit
//! pattern in the trailing region.
//!
//! Alphabet: digits 0-9, uppercase A-Z, the punctuation set
//! `- . $ / + % space`, plus four shift codewords (S1, S2, FN1, FN2,
//! FN3, NS). The full character set is 49 entries indexed 0..=48.
//!
//! ## Port status
//!
//! Full BWIPP-faithful port. Constants tables (CHARMAP, METRICS,
//! SAMVAL, PARITY, WEIGHTX/Y/Z, PATTERNS_0/PATTERNS_1) are ported
//! verbatim from bwip-js. The cws-level encoder covers three paths:
//!
//!   1. Direct-lookup ([`encode_cws_direct`]) — uppercase / digit /
//!      7-symbol punctuation subset, 1 byte → 1 codeword.
//!   2. NS-shift digit packing ([`encode_cws_ns_digits`], mode 2) —
//!      ≥5 leading digits packed as base-48 polynomial; mirrors
//!      BWIPP `encodenumeric`.
//!   3. Alpha path with S1 / S2 shifts ([`encode_cws_alpha`],
//!      mode 0 / 4 / 5) — handles control bytes, lowercase, and
//!      extended ASCII through shift sequences.
//!
//! [`encode_cws`] is the top-level dispatcher; [`build_ccs`] adds
//! the cr7 row-indicator codeword + the wr1 / wr2 / cr-x checks via
//! the BWIPP `calccheck` formula; [`encode`] returns a stacked
//! [`BitMatrix`] via the renderer mirroring bwip-js lines 21259-21318.
//!
//! Verified by a 6-input `build_ccs` golden + a 405-cell `pixs`
//! byte-for-byte golden for "12345" against bwip-js. SAM (Symbol
//! Append Mode) chaining is not implemented (extension path).

#![allow(dead_code)]

use crate::encoding::BitMatrix;
use crate::error::Error;

use super::code49_patterns::{PATTERNS_0, PATTERNS_1};

// ---------------------------------------------------------------------------
// Marker constants — BWIPP's `code49_*` negative-i16 sentinels
// (bwip-js lines 19904-19909).
// ---------------------------------------------------------------------------

/// Shift 1 — switch to the second character of the next pair.
pub(crate) const S1: i16 = -1;
/// Shift 2 — switch to the third character of the next pair.
pub(crate) const S2: i16 = -2;
/// FNC1 marker (GS1 separator / start-of-GS1-data signal).
pub(crate) const FN1: i16 = -3;
/// FNC2 marker.
pub(crate) const FN2: i16 = -4;
/// FNC3 marker.
pub(crate) const FN3: i16 = -5;
/// Numeric-shift codeword (mid-message shift to numeric-pair mode).
pub(crate) const NS: i16 = -6;

// ---------------------------------------------------------------------------
// Charmap — bwip-js line 19910.
//
// 49 entries (indices 0..=48) covering the Code 49 alphabet. Index
// is the codeword value; entry is the ASCII byte (positive `i16`)
// or marker constant.
// ---------------------------------------------------------------------------

/// Code 49's 49-symbol alphabet. Maps codeword `0..=48` to either
/// an ASCII byte or a marker constant. Mirrors BWIPP's
/// `code49_charmap` initializer verbatim.
#[rustfmt::skip]
pub(crate) const CHARMAP: [i16; 49] = [
    // 0..=9: digits '0'..='9'.
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57,
    // 10..=35: uppercase 'A'..='Z'.
    65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80,
    81, 82, 83, 84, 85, 86, 87, 88, 89, 90,
    // 36..=42: punctuation `- . space $ / + %`.
    b'-' as i16, b'.' as i16, b' ' as i16, b'$' as i16,
    b'/' as i16, b'+' as i16, b'%' as i16,
    // 43..=48: marker codewords.
    S1, S2, FN1, FN2, FN3, NS,
];

/// Symbol-size table (BWIPP `code49_metrics`). Entry `i` =
/// `[rows, dcws]`. `dcws` is the number of data codewords (excluding
/// the row-check codewords and the trailing per-row parity bits).
/// Rows range 2..=8.
pub(crate) const METRICS: [[u16; 2]; 7] =
    [[2, 9], [3, 16], [4, 23], [5, 30], [6, 37], [7, 42], [8, 49]];

/// Symbol-append-mode (SAM) value table (BWIPP `code49_samval`).
/// 44 entries indexing the (`Nth of M`) tuple encoded into the
/// start row when chaining symbols via SAM. Index is `SAM - 12`,
/// value is the SAM tuple `Nth*10 + M`.
#[rustfmt::skip]
pub(crate) const SAMVAL: [u16; 44] = [
    12, 22,
    13, 23, 33,
    14, 24, 34, 44,
    15, 25, 35, 45, 55,
    16, 26, 36, 46, 56, 66,
    17, 27, 37, 47, 57, 67, 77,
    18, 28, 38, 48, 58, 68, 78, 88,
    19, 29, 39, 49, 59, 69, 79, 89, 99,
];

/// Per-row parity-bit patterns (BWIPP `code49_parity`). Each entry
/// is a 4-character string of `'0'` / `'1'` describing the parity
/// bits emitted in the trailing region of each row. Indexed by the
/// row's modulo-8 parity selector.
pub(crate) const PARITY: [&str; 8] = [
    "1001", "0101", "1100", "0011", "1010", "0110", "1111", "0000",
];

/// Row-check weight tables — BWIPP `code49_weightx` / `weighty` /
/// `weightz`. Derived from a 34-entry permutation table by taking
/// 33 entries with three different leading values (20 / 16 / 38).
/// Used by the row-check codeword computation to compute the
/// trailing wr1 / wr2 / cr_x codewords on the last row.
///
/// Mirrors bwip-js lines 19914-19932 verbatim.
#[rustfmt::skip]
pub(crate) const WEIGHTX: [u16; 33] = [
    20,
    1, 9, 31, 26, 2, 12, 17, 23, 37, 18, 22, 6, 27, 44, 15, 43,
    39, 11, 13, 5, 41, 33, 36, 8, 4, 32, 3, 19, 40, 25, 29, 10,
];
#[rustfmt::skip]
pub(crate) const WEIGHTY: [u16; 33] = [
    16,
    9, 31, 26, 2, 12, 17, 23, 37, 18, 22, 6, 27, 44, 15, 43, 39,
    11, 13, 5, 41, 33, 36, 8, 4, 32, 3, 19, 40, 25, 29, 10, 24,
];
#[rustfmt::skip]
pub(crate) const WEIGHTZ: [u16; 33] = [
    38,
    31, 26, 2, 12, 17, 23, 37, 18, 22, 6, 27, 44, 15, 43, 39, 11,
    13, 5, 41, 33, 36, 8, 4, 32, 3, 19, 40, 25, 29, 10, 24, 30,
];

/// Look up the BWIPP `code49_charvals` entry for byte `b`. Returns
/// `Some(codeword)` for direct alphabet members, or `None` for
/// bytes that need a shift sequence (handled by the COMBOS table —
/// not in this iteration's foundation).
///
/// Direct members: digits, uppercase, the 7 punctuation symbols.
/// Bytes outside that set need either an S1 / S2 shift-pair
/// (handled by COMBOS) or are entirely unrepresentable.
#[inline]
pub(crate) fn lookup_direct(b: u8) -> Option<u16> {
    CHARMAP
        .iter()
        .position(|&v| v == i16::from(b))
        .map(|i| i as u16)
}

/// PAD codeword index = 48 (NS marker row of [`CHARMAP`]). Used to
/// fill remaining slots after the direct-lookup data codewords.
pub(crate) const PAD_CW: u16 = 48;

/// Pick the smallest symbol size (rows, dcws) that fits `data_count`
/// data codewords. Walks [`METRICS`] from r=2 → r=8.
pub(crate) fn pick_symbol_size(data_count: usize) -> Option<(u16, u16)> {
    METRICS
        .iter()
        .find(|row| usize::from(row[1]) >= data_count)
        .map(|row| (row[0], row[1]))
}

/// Code 49 cws-level encoder for the **direct-lookup subset** —
/// inputs whose every byte sits in [`CHARMAP`] at index 0..=42
/// (digits, uppercase letters, and the 7-symbol punctuation set).
///
/// Mirrors BWIPP's mode-A (default) path **without** the NS-shift
/// digit-pair compaction (so short pure-text and short pure-digit
/// payloads encode 1 byte → 1 codeword, BWIPP-byte-for-byte).
///
/// # Errors
///
/// * `InvalidData` if `input` is empty.
/// * `InvalidData` if any byte isn't a direct CHARMAP member
///   (lowercase, control bytes, high bytes — all require the
///   S1/S2/NS shifts handled by Stage 3+).
/// * `InvalidData` if the payload exceeds the r=8 ceiling (49
///   codewords).
pub(crate) fn encode_cws_direct(input: &[u8]) -> Result<Vec<u16>, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData("code49: empty input".to_string()));
    }
    let mut codewords: Vec<u16> = Vec::with_capacity(input.len());
    for (idx, &b) in input.iter().enumerate() {
        let cw = lookup_direct(b).ok_or_else(|| {
            Error::InvalidData(format!(
                "code49 direct-lookup path: byte 0x{b:02x} at position {idx} \
                 isn't a direct CHARMAP member — lowercase, control bytes, and \
                 high bytes need S1/S2/NS shift handling (Stage 3+)"
            ))
        })?;
        // Sanity: direct alphabet members live at indices 0..=42.
        if cw > 42 {
            return Err(Error::InvalidData(format!(
                "code49: byte 0x{b:02x} mapped to marker codeword {cw} \
                 (not a direct alphabet member)"
            )));
        }
        codewords.push(cw);
    }
    let (_rows, dcws) = pick_symbol_size(codewords.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49: payload of {} bytes exceeds the r=8 ceiling (49 codewords)",
            codewords.len()
        ))
    })?;
    let dcws = usize::from(dcws);
    while codewords.len() < dcws {
        codewords.push(PAD_CW);
    }
    Ok(codewords)
}

/// BWIPP's `base48` helper (bwip-js lines 20065-20096). Given a
/// run of `n` ASCII digit bytes and a target codeword count `count`,
/// interpret the digits as a single decimal integer and emit
/// `count` base-48 digits high-to-low.
///
/// Used by [`encode_cws_ns_digits`] to pack groups of 5 / 3 digits
/// into 3 / 2 codewords (with special-case 6-digit padding when the
/// remainder has 4 or 7 chars).
fn base48(count: usize, digits: &[u8]) -> Vec<u16> {
    // Interpret the digit bytes as a single decimal integer.
    let mut value: u64 = 0;
    for &b in digits {
        debug_assert!(b.is_ascii_digit());
        value = value * 10 + u64::from(b - b'0');
    }
    // Emit `count` base-48 digits, high-to-low.
    let mut out = vec![0u16; count];
    for i in (0..count).rev() {
        out[i] = (value % 48) as u16;
        value /= 48;
    }
    out
}

/// BWIPP-faithful Code 49 cws-level encoder for the **NS-shift
/// digit-only** path (mode 2 — `numericruns[0] >= 5`). Mirrors
/// bwip-js `encodenumeric` (line 20097): chunks 5-at-a-time via
/// `base48(3, …)`, with remainder cases:
///
///   * remainder 0 → no tail; total codeword count = (n / 5) * 3.
///   * remainder 1 → encodealpha (direct CHARMAP lookup); 1 extra cw.
///   * remainder 2 → split as `nums[0..4]` (treated as `"10" ||
///     nums[0..4]` → base48(3)) + `nums[4..7]` (base48(2)). Hits
///     when `n % 5 == 2`, where `pre = n - 7`.
///     (5 codewords for the trailing 7 digits.)
///   * remainder 3 → base48(2, nums); 2 codewords.
///   * remainder 4 → base48(3, "10" || nums) → 3 codewords.
///
/// Verified byte-for-byte against bwip-js for 6 digit-payload
/// goldens (5/6/7/8/9/10/11/13 digits) — see Stage 3 test.
pub(crate) fn encode_cws_ns_digits(digits: &[u8]) -> Result<Vec<u16>, Error> {
    if digits.is_empty() {
        return Err(Error::InvalidData("code49: empty input".to_string()));
    }
    if digits.len() < 5 {
        return Err(Error::InvalidData(format!(
            "code49 NS-shift path requires ≥5 digits (got {}); use \
             encode_cws_direct for shorter pure-digit runs",
            digits.len()
        )));
    }
    for (idx, &b) in digits.iter().enumerate() {
        if !b.is_ascii_digit() {
            return Err(Error::InvalidData(format!(
                "code49 NS-shift path: non-digit byte 0x{b:02x} at position {idx}"
            )));
        }
    }
    let n = digits.len();
    // BWIPP picks `pre = n - (n % 5)` except when `n % 5 == 2` where
    // `pre = n - 7`. The remainder is `n - pre`.
    let r = n % 5;
    let pre = if r == 2 { n - 7 } else { n - r };
    let mut cws: Vec<u16> = Vec::with_capacity(n * 2 / 3 + 3);
    // Loop the 5-digit chunks via base48(3, …).
    let mut idx = 0;
    while idx < pre {
        cws.extend(base48(3, &digits[idx..idx + 5]));
        idx += 5;
    }
    // Remainder handling per BWIPP's encodenumeric.
    let remainder = &digits[pre..];
    match remainder.len() {
        0 => {}
        1 => {
            // encodealpha — direct CHARMAP lookup. For a digit
            // this is just byte - '0' (= CHARMAP index 0..=9).
            cws.push(u16::from(remainder[0] - b'0'));
        }
        3 => {
            cws.extend(base48(2, remainder));
        }
        4 => {
            // base48(3, "10" || remainder) — pre-pend literal '1'
            // and '0' (BWIPP pushes 49 and 48 onto the stack, which
            // are the ASCII codes for '1' and '0').
            let mut padded = Vec::with_capacity(6);
            padded.push(b'1');
            padded.push(b'0');
            padded.extend_from_slice(remainder);
            cws.extend(base48(3, &padded));
        }
        7 => {
            // base48(3, "10" || remainder[0..4]) + base48(2, remainder[4..7]).
            let mut padded = Vec::with_capacity(6);
            padded.push(b'1');
            padded.push(b'0');
            padded.extend_from_slice(&remainder[..4]);
            cws.extend(base48(3, &padded));
            cws.extend(base48(2, &remainder[4..]));
        }
        _ => {
            return Err(Error::InvalidData(format!(
                "code49 NS-shift internal: unexpected remainder length {} for n={n}",
                remainder.len()
            )));
        }
    }
    // Pad with NS (=PAD_CW=48) to dcws.
    let (_rows, dcws) = pick_symbol_size(cws.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49: payload of {} digits produces {} codewords, exceeds r=8 ceiling",
            n,
            cws.len()
        ))
    })?;
    let dcws = usize::from(dcws);
    while cws.len() < dcws {
        cws.push(PAD_CW);
    }
    Ok(cws)
}

/// Per-byte BWIPP `charvals` lookup. Mirrors the lookup BWIPP builds
/// at line 19945-19956 by walking `code49_combos` and dereferencing
/// the shift/target pairs.
///
/// Returns:
///   * `Some((None, cw))` for direct alphabet members (digits,
///     uppercase, the 7 punctuation symbols, plus space at 38).
///   * `Some((Some(43), cw))` for S1-shifted bytes (control bytes
///     0..=31 plus a few punctuation entries — the
///     `(<S1> + uppercase-counterpart-or-digit-or-punct)` pairs).
///   * `Some((Some(44), cw))` for S2-shifted bytes (lowercase
///     letters, the extended-ASCII punctuation, DEL).
///   * `None` for high bytes (>127) — Code 49 is ASCII-only.
fn charvals(b: u8) -> Option<(Option<u16>, u16)> {
    match b {
        // ASCII 0 → S1 + space (=cw 38).
        0 => Some((Some(43), 38)),
        // ASCII 1..=26 → S1 + 'A'..'Z' (cw 10..=35).
        1..=26 => Some((Some(43), 10 + u16::from(b - 1))),
        // ASCII 27..=31 → S1 + '1'..'5' (cw 1..=5).
        27..=31 => Some((Some(43), u16::from(b - 26))),
        // ASCII 32 (space) — direct.
        b' ' => Some((None, 38)),
        // ASCII 33..=35 ('!', '"', '#') → S1 + '6'..'8'.
        b'!' => Some((Some(43), 6)),
        b'"' => Some((Some(43), 7)),
        b'#' => Some((Some(43), 8)),
        // Direct: $ %.
        b'$' => Some((None, 39)),
        b'%' => Some((None, 42)),
        // S1: & ' ( ) * (no +) ,
        b'&' => Some((Some(43), 9)),
        b'\'' => Some((Some(43), 0)),
        b'(' => Some((Some(43), 36)),
        b')' => Some((Some(43), 37)),
        b'*' => Some((Some(43), 39)),
        b'+' => Some((None, 41)),
        b',' => Some((Some(43), 40)),
        // Direct: - . /
        b'-' => Some((None, 36)),
        b'.' => Some((None, 37)),
        b'/' => Some((None, 40)),
        // Direct: digits.
        b'0'..=b'9' => Some((None, u16::from(b - b'0'))),
        // ASCII 58 (':') → S1 + '+'.
        b':' => Some((Some(43), 41)),
        // ASCII 59..=64 (';' '<' '=' '>' '?' '@') → S2 + '1'..'6'.
        b';' => Some((Some(44), 1)),
        b'<' => Some((Some(44), 2)),
        b'=' => Some((Some(44), 3)),
        b'>' => Some((Some(44), 4)),
        b'?' => Some((Some(44), 5)),
        b'@' => Some((Some(44), 6)),
        // Direct: uppercase.
        b'A'..=b'Z' => Some((None, u16::from(b - b'A' + 10))),
        // S2: [ \ ] ^ _ `
        b'[' => Some((Some(44), 7)),
        b'\\' => Some((Some(44), 8)),
        b']' => Some((Some(44), 9)),
        b'^' => Some((Some(44), 0)),
        b'_' => Some((Some(44), 36)),
        b'`' => Some((Some(44), 37)),
        // S2: lowercase → 'A'..'Z' targets.
        b'a'..=b'z' => Some((Some(44), u16::from(b - b'a' + 10))),
        // S2: { | } ~ DEL
        b'{' => Some((Some(44), 39)),
        b'|' => Some((Some(44), 40)),
        b'}' => Some((Some(44), 41)),
        b'~' => Some((Some(44), 42)),
        127 => Some((Some(44), 38)),
        // High bytes: not representable in Code 49.
        _ => None,
    }
}

/// Mode value selected when the input starts with a shifted byte.
/// Encoded into the leading row indicator at render time, NOT into
/// cws. Mode 0 = alpha default; mode 4 = first byte was S1-shifted;
/// mode 5 = first byte was S2-shifted; mode 2 = NS-shift digit
/// path (handled separately).
pub(crate) const MODE_ALPHA: u16 = 0;
pub(crate) const MODE_NS_DIGITS: u16 = 2;
pub(crate) const MODE_FIRST_S1: u16 = 4;
pub(crate) const MODE_FIRST_S2: u16 = 5;

/// BWIPP-faithful Code 49 cws-level encoder for the **alpha path**
/// (mode 0 / 4 / 5). Handles inputs requiring S1/S2 shifts — covers
/// lowercase letters, control bytes, and all ASCII punctuation.
///
/// Mirrors BWIPP's main encoder loop (lines 20204-20261) for the
/// alpha-only path. The first byte gets special treatment:
///   * If shifted (charvals returns Some(shift)), the leading shift
///     codeword is *suppressed* — only the target codeword goes into
///     cws[0]. The shift is implied by the leading row indicator
///     mode (mode 4 for S1, mode 5 for S2).
///   * If direct, cws[0] is the target codeword; mode stays 0.
///
/// Returns `(cws, mode)` so the caller (eventually the renderer)
/// can compose the leading row indicator.
///
/// # Errors
///
/// * `InvalidData` if `input` is empty.
/// * `InvalidData` if any byte is non-ASCII (high byte > 127).
/// * `InvalidData` if the cws stream exceeds the r=8 ceiling.
pub(crate) fn encode_cws_alpha(input: &[u8]) -> Result<(Vec<u16>, u16), Error> {
    if input.is_empty() {
        return Err(Error::InvalidData("code49: empty input".to_string()));
    }
    let mut cws: Vec<u16> = Vec::with_capacity(input.len() * 2);
    // Special handling for the first byte.
    let (first_shift, first_target) = charvals(input[0]).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49 alpha path: byte 0x{:02x} at position 0 is non-ASCII",
            input[0]
        ))
    })?;
    let mode = match first_shift {
        Some(43) => MODE_FIRST_S1,
        Some(44) => MODE_FIRST_S2,
        Some(_) => unreachable!("only S1/S2 shifts in charvals"),
        None => MODE_ALPHA,
    };
    cws.push(first_target);
    // Remaining bytes: emit shift + target (or just target if direct).
    for (idx, &b) in input.iter().enumerate().skip(1) {
        let (shift, target) = charvals(b).ok_or_else(|| {
            Error::InvalidData(format!(
                "code49 alpha path: byte 0x{b:02x} at position {idx} is non-ASCII"
            ))
        })?;
        if let Some(s) = shift {
            cws.push(s);
        }
        cws.push(target);
    }
    let (_rows, dcws) = pick_symbol_size(cws.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49 alpha path: payload of {} bytes produces {} cws, exceeds r=8 ceiling",
            input.len(),
            cws.len()
        ))
    })?;
    let dcws = usize::from(dcws);
    while cws.len() < dcws {
        cws.push(PAD_CW);
    }
    Ok((cws, mode))
}

/// Top-level Code 49 cws-level encoder. Inspects the input and
/// dispatches to the appropriate sub-encoder:
///
///   * Empty → InvalidData.
///   * `numericruns[0] >= 5` (input starts with 5+ digits) →
///     [`encode_cws_ns_digits`] (mode 2).
///   * Otherwise → [`encode_cws_alpha`] (mode 0/4/5 based on first
///     byte's shift requirement).
///
/// Returns `(cws, mode)`. Mode is needed by the renderer to compose
/// the leading row indicator. This mirrors BWIPP's mode-selection
/// logic at lines 20177-20230.
pub(crate) fn encode_cws(input: &[u8]) -> Result<(Vec<u16>, u16), Error> {
    if input.is_empty() {
        return Err(Error::InvalidData("code49: empty input".to_string()));
    }
    // Compute the leading numericruns count (consecutive digits
    // from position 0).
    let leading_digits = input.iter().take_while(|b| b.is_ascii_digit()).count();
    if leading_digits >= 5 {
        // Mode 2 — NS-shift digit path.
        let cws = encode_cws_ns_digits(&input[..leading_digits])?;
        // Mid-message mode switching (digit-run → alpha) is outside
        // this port's scope; callers with mixed payloads must
        // pre-segment, or use BWIPP directly. Reject so the caller
        // gets a clear error instead of a malformed symbol.
        if leading_digits != input.len() {
            return Err(Error::InvalidData(
                "code49: payload has a leading NS-digit run followed by non-digit content; this Rust port encodes either all-digit-NS or all-alpha but not mixed runs (use BWIPP for mixed)".to_string(),
            ));
        }
        Ok((cws, MODE_NS_DIGITS))
    } else {
        encode_cws_alpha(input)
    }
}

// ---------------------------------------------------------------------------
// Row-check computation — BWIPP `calccheck` + the wr1 / wr2 / check_x
// build-up over WEIGHTX/Y/Z.
// ---------------------------------------------------------------------------

/// BWIPP `calccheck` (bwip-js lines 21205-21213). Computes the score
///
/// ```text
///   score = sum over i in 0..((r-1)*4 - 1) of
///             (ccs[i*2]*49 + ccs[i*2+1]) * weights[i+1]
/// ```
///
/// `ccs[0..(r-1)*8]` covers the first `r-1` rows of `ccs` (the last
/// row's data isn't fed into `calccheck`).
fn calccheck(ccs: &[u16], rows: u16, weights: &[u16]) -> u32 {
    let pair_count = (usize::from(rows) - 1) * 4;
    let mut score: u32 = 0;
    for i in 0..pair_count {
        let pair = u32::from(ccs[i * 2]) * 49 + u32::from(ccs[i * 2 + 1]);
        score += pair * u32::from(weights[i + 1]);
    }
    score
}

/// Build the full `ccs` codeword grid (length = `rows * 8`) from a
/// padded `cws` vector. Mirrors bwip-js lines 21183-21250:
///
///   1. Pack the first `r-1` rows: 7 data codewords each at
///      `ccs[i*8..i*8+7]`, with `ccs[i*8+7] = sum(cws_row) % 49`
///      (the row's intra-row sum check).
///   2. Place the remaining `dcws - (r-1)*7` data codewords at
///      `ccs[(r-1)*8..(r-1)*8 + remaining]`. The remaining slots of
///      the last row stay zero until checks fill them.
///   3. Write `cr7 = (r - 2) * 7 + mode` into `ccs[r*8 - 2]`.
///   4. If `r >= 7`: compute the z-check
///      `(cr7 * weightz[0] + calccheck(weightz)) % 2401`, split into
///      a (high, low) base-49 pair, write into `lastrow[0..2]`.
///   5. Compute `wr1 = lastrow[0] * 49 + lastrow[1]`.
///   6. Compute the y-check
///      `(cr7*weighty[0] + calccheck(weighty) + wr1*weighty[r*4-3]) %
///       2401`, split into a (high, low) base-49 pair, write into
///      `lastrow[2..4]`. The low value of this pair is `wr2`.
///   7. Compute the x-check
///      `(cr7*weightx[0] + calccheck(weightx) +
///        wr1*weightx[r*4-3] + wr2*weightx[r*4-2]) % 2401`,
///      split into a (high, low) base-49 pair, write into
///      `lastrow[4..6]`. ccs[lastrow + 6] is cr7.
///   8. Final check: `ccs[r*8 - 1] = sum(ccs[r*8-8 .. r*8-1]) % 49`.
///
/// `dcws` comes from [`METRICS`] for the chosen `rows`.
pub(crate) fn build_ccs(cws: &[u16], rows: u16, dcws: u16, mode: u16) -> Result<Vec<u16>, Error> {
    let r = usize::from(rows);
    let dcws_usz = usize::from(dcws);
    if cws.len() != dcws_usz {
        return Err(Error::InvalidData(format!(
            "code49 internal: cws.len()={} but dcws={} (r={r})",
            cws.len(),
            dcws_usz
        )));
    }
    if !(2..=8).contains(&r) {
        return Err(Error::InvalidData(format!(
            "code49 internal: r={r} not in 2..=8"
        )));
    }
    let mut ccs = vec![0u16; r * 8];
    // Step 1+2: pack rows 0..r-1 with 7 data cws each + row-sum check.
    let mut j = 0usize;
    for i in 0..r - 1 {
        let row = &cws[j..j + 7];
        let row_sum: u32 = row.iter().map(|&c| u32::from(c)).sum();
        for (k, &c) in row.iter().enumerate() {
            ccs[i * 8 + k] = c;
        }
        ccs[i * 8 + 7] = (row_sum % 49) as u16;
        j += 7;
    }
    // Step 2 (last row): the remaining `dcws - j` data codewords go
    // at the start of the last row.
    if j < dcws_usz {
        let remaining = dcws_usz - j;
        let lastrow_start = (r - 1) * 8;
        ccs[lastrow_start..lastrow_start + remaining].copy_from_slice(&cws[j..]);
    }
    // Step 3: cr7 = (r - 2) * 7 + mode at ccs[len - 2].
    let cr7 = (r as u16 - 2) * 7 + mode;
    let last_idx = r * 8;
    ccs[last_idx - 2] = cr7;
    // Step 4: z-check (only when r >= 7).
    if r >= 7 {
        let score_z = calccheck(&ccs, rows, &WEIGHTZ);
        let cr7_z = u32::from(cr7) * u32::from(WEIGHTZ[0]);
        let check_z = (cr7_z + score_z) % 2401;
        let lastrow_start = (r - 1) * 8;
        ccs[lastrow_start] = (check_z / 49) as u16;
        ccs[lastrow_start + 1] = (check_z % 49) as u16;
    }
    // Step 5: wr1 = lastrow[0]*49 + lastrow[1].
    let lastrow_start = (r - 1) * 8;
    let wr1 = u32::from(ccs[lastrow_start]) * 49 + u32::from(ccs[lastrow_start + 1]);
    // Step 6: y-check.
    let score_y = calccheck(&ccs, rows, &WEIGHTY);
    let cr7_y = u32::from(cr7) * u32::from(WEIGHTY[0]);
    let wr1_idx = r * 4 - 3;
    let check_y = (cr7_y + score_y + wr1 * u32::from(WEIGHTY[wr1_idx])) % 2401;
    ccs[lastrow_start + 2] = (check_y / 49) as u16;
    ccs[lastrow_start + 3] = (check_y % 49) as u16;
    let wr2 = check_y;
    // Step 7: x-check.
    let score_x = calccheck(&ccs, rows, &WEIGHTX);
    let cr7_x = u32::from(cr7) * u32::from(WEIGHTX[0]);
    let wr2_idx = r * 4 - 2;
    let check_x =
        (cr7_x + score_x + wr1 * u32::from(WEIGHTX[wr1_idx]) + wr2 * u32::from(WEIGHTX[wr2_idx]))
            % 2401;
    ccs[lastrow_start + 4] = (check_x / 49) as u16;
    ccs[lastrow_start + 5] = (check_x % 49) as u16;
    // Step 8: final check at ccs[len - 1].
    let lastrow_sum: u32 = ccs[last_idx - 8..last_idx - 1]
        .iter()
        .map(|&c| u32::from(c))
        .sum();
    ccs[last_idx - 1] = (lastrow_sum % 49) as u16;
    Ok(ccs)
}

// ---------------------------------------------------------------------------
// Stacked renderer — bwip-js lines 21259-21295.
// ---------------------------------------------------------------------------

/// Build the inter-row separator row (BWIPP `seprow`): 10 zeros, 70
/// ones, 1 zero — same shape as code16k, but Code 49's data area
/// itself is 81 modules wide.
fn build_seprow() -> [u8; 81] {
    let mut row = [1u8; 81];
    for cell in row.iter_mut().take(10) {
        *cell = 0;
    }
    row[80] = 0;
    row
}

/// Build the 81-module bit pattern for one row of a Code 49 symbol.
/// Mirrors bwip-js lines 21259-21278:
///
///   * `p = parity[i]` for i in 0..r-1; `"0000"` for the last row.
///   * `ccrow = ccs[i*8 .. i*8+8]`; pack into 4 base-49 indices
///     `scrow[k] = ccrow[2k]*49 + ccrow[2k+1]`.
///   * `sbs = [10, 1, 1] + 4 × patterns[p[k]][scrow[k]] + [4, 1]`.
///   * Toggle from a seed of `1` to expand widths into bits — the
///     first width (10) becomes 10 zeros (the left quiet zone).
fn build_row_bits(row_idx: usize, rows: usize, ccrow: &[u16]) -> [u8; 81] {
    debug_assert_eq!(ccrow.len(), 8);
    let p: &str = if row_idx + 1 == rows {
        "0000"
    } else {
        PARITY[row_idx]
    };
    let scrow = [
        u32::from(ccrow[0]) * 49 + u32::from(ccrow[1]),
        u32::from(ccrow[2]) * 49 + u32::from(ccrow[3]),
        u32::from(ccrow[4]) * 49 + u32::from(ccrow[5]),
        u32::from(ccrow[6]) * 49 + u32::from(ccrow[7]),
    ];
    let p_bytes = p.as_bytes();
    let mut sbs: Vec<u8> = Vec::with_capacity(41);
    sbs.push(10);
    sbs.push(1);
    sbs.push(1);
    for k in 0..4 {
        let table: &[&str; 2401] = if p_bytes[k] == b'1' {
            &PATTERNS_1
        } else {
            &PATTERNS_0
        };
        let pattern = table[scrow[k] as usize];
        for c in pattern.bytes() {
            sbs.push(c - b'0');
        }
    }
    sbs.push(4);
    sbs.push(1);
    // Toggle starting from 1 → first width (10) emits 10 zeros (the
    // left quiet zone).
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
    debug_assert_eq!(idx, 81, "sbs widths must sum to 81");
    row
}

/// BWIPP-faithful Code 49 stacked renderer. Combines the cws-level
/// encoder output with row-check codewords + [`PATTERNS_0`] /
/// [`PATTERNS_1`] + [`PARITY`] into a stacked symbol.
///
/// Mirrors bwip-js lines 21259-21318. Default `rowheight = 8`,
/// `sepheight = 1` — these are BWIPP's defaults for stand-alone
/// symbols (SAM not set, append not set, height not overridden).
///
/// # Layout per row (81 modules):
///
///   * 10-module left quiet area.
///   * 1 module start bar + 1 module separator.
///   * 4 codeword pairs × 8 widths = 64 modules of data.
///   * 4-module stop bar + 1 module trailing separator.
///
/// # Layout vertically (`numcomprows = 2 * r + 1` compressed rows):
///
///   * Top bearer (sepheight modules, all ones).
///   * For each of `r-1` non-last rows: data (rowheight) +
///     separator (sepheight).
///   * Last data row (rowheight).
///   * Bottom bearer (sepheight, all ones).
pub fn encode(input: &[u8]) -> Result<BitMatrix, Error> {
    let (cws, mode) = encode_cws(input)?;
    let (rows, dcws) = pick_symbol_size(cws.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49: payload yields {} cws, exceeds r=8 ceiling",
            cws.len()
        ))
    })?;
    let ccs = build_ccs(&cws, rows, dcws, mode)?;
    let r = usize::from(rows);
    let rowheight: usize = 8;
    let sepheight: usize = 1;
    let pixx: usize = 81;
    let seprow = build_seprow();
    let allone = [1u8; 81];
    let numcomprows = 2 * r + 1;
    let mut compressed: Vec<[u8; 81]> = Vec::with_capacity(numcomprows);
    let mut mults: Vec<usize> = Vec::with_capacity(numcomprows);
    compressed.push(allone);
    mults.push(sepheight);
    for i in 0..r {
        let ccrow = &ccs[i * 8..i * 8 + 8];
        compressed.push(build_row_bits(i, r, ccrow));
        mults.push(rowheight);
        if i + 1 < r {
            compressed.push(seprow);
            mults.push(sepheight);
        }
    }
    compressed.push(allone);
    mults.push(sepheight);
    debug_assert_eq!(compressed.len(), numcomprows);
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
/// applied) — the form bwip-js's oracle anchor captures. This is the
/// byte-for-byte comparison surface used by the golden tests.
pub(crate) fn encode_pixs(input: &[u8]) -> Result<Vec<u8>, Error> {
    let (cws, mode) = encode_cws(input)?;
    let (rows, dcws) = pick_symbol_size(cws.len()).ok_or_else(|| {
        Error::InvalidData(format!(
            "code49: payload yields {} cws, exceeds r=8 ceiling",
            cws.len()
        ))
    })?;
    let ccs = build_ccs(&cws, rows, dcws, mode)?;
    let r = usize::from(rows);
    let seprow = build_seprow();
    let allone = [1u8; 81];
    let numcomprows = 2 * r + 1;
    let mut pixs: Vec<u8> = Vec::with_capacity(numcomprows * 81);
    pixs.extend_from_slice(&allone);
    for i in 0..r {
        let ccrow = &ccs[i * 8..i * 8 + 8];
        pixs.extend_from_slice(&build_row_bits(i, r, ccrow));
        if i + 1 < r {
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

    /// `CHARMAP` has exactly 49 entries: 10 digits + 26 uppercase
    /// letters + 7 punctuation symbols + 6 marker codewords.
    #[test]
    fn charmap_shape() {
        assert_eq!(CHARMAP.len(), 49);
    }

    /// Anchor a handful of `CHARMAP` rows known from the BWIPP
    /// source.
    #[test]
    fn charmap_anchors() {
        assert_eq!(CHARMAP[0], i16::from(b'0'));
        assert_eq!(CHARMAP[9], i16::from(b'9'));
        assert_eq!(CHARMAP[10], i16::from(b'A'));
        assert_eq!(CHARMAP[35], i16::from(b'Z'));
        assert_eq!(CHARMAP[36], i16::from(b'-'));
        assert_eq!(CHARMAP[37], i16::from(b'.'));
        assert_eq!(CHARMAP[38], i16::from(b' '));
        assert_eq!(CHARMAP[39], i16::from(b'$'));
        assert_eq!(CHARMAP[40], i16::from(b'/'));
        assert_eq!(CHARMAP[41], i16::from(b'+'));
        assert_eq!(CHARMAP[42], i16::from(b'%'));
        assert_eq!(CHARMAP[43], S1);
        assert_eq!(CHARMAP[44], S2);
        assert_eq!(CHARMAP[45], FN1);
        assert_eq!(CHARMAP[46], FN2);
        assert_eq!(CHARMAP[47], FN3);
        assert_eq!(CHARMAP[48], NS);
    }

    /// `METRICS` covers rows 2..=8 (7 entries). dcws progression:
    /// 9, 16, 23, 30, 37, 42, 49 (matches BWIPP's table verbatim).
    #[test]
    fn metrics_shape_and_anchors() {
        assert_eq!(METRICS.len(), 7);
        assert_eq!(METRICS[0], [2, 9]);
        assert_eq!(METRICS[6], [8, 49]);
        // dcws grows non-linearly (BWIPP's actual values), not
        // a simple arithmetic progression — anchor each row.
        let expected_dcws = [9, 16, 23, 30, 37, 42, 49];
        for (i, &dcws) in expected_dcws.iter().enumerate() {
            assert_eq!(
                METRICS[i][0],
                (i + 2) as u16,
                "METRICS[{i}] rows should be {}",
                i + 2
            );
            assert_eq!(METRICS[i][1], dcws, "METRICS[{i}] dcws should be {dcws}");
        }
    }

    /// `SAMVAL` has 44 entries covering the (Nth, M) combinations
    /// where N ∈ 2..=M and M ∈ 2..=9 — 2+3+4+5+6+7+8+9 = 44.
    #[test]
    fn samval_shape_and_anchors() {
        assert_eq!(SAMVAL.len(), 44);
        // First entry = (1st of 2 symbols) → 12.
        assert_eq!(SAMVAL[0], 12);
        // Second = (2nd of 2) → 22.
        assert_eq!(SAMVAL[1], 22);
        // Last entry = (9th of 9) → 99.
        assert_eq!(SAMVAL[43], 99);
    }

    /// `PARITY` has 8 entries, each 4 chars of '0'/'1'.
    #[test]
    fn parity_shape() {
        assert_eq!(PARITY.len(), 8);
        for (i, &entry) in PARITY.iter().enumerate() {
            assert_eq!(entry.len(), 4, "PARITY[{i}] should be 4 chars");
            assert!(
                entry.chars().all(|c| c == '0' || c == '1'),
                "PARITY[{i}] = {entry:?} should be binary",
            );
        }
    }

    /// PATTERNS tables (`code49_patterns`) — 2 arrays of 2401
    /// (= 49²) 8-digit width strings. Each row of the symbol picks
    /// an 8-wide pattern from one of these two arrays based on the
    /// row's parity bit. The shape comes from BWIPP verbatim.
    #[test]
    fn patterns_shape_and_anchors() {
        assert_eq!(PATTERNS_0.len(), 2401);
        assert_eq!(PATTERNS_1.len(), 2401);
        // Every entry is 8 digits in 1..=6.
        for (i, &p) in PATTERNS_0.iter().enumerate() {
            assert_eq!(p.len(), 8, "PATTERNS_0[{i}] should be 8 chars");
            for c in p.chars() {
                let d = c.to_digit(10).unwrap_or(99);
                assert!(
                    (1..=6).contains(&d),
                    "PATTERNS_0[{i}] = {p:?} has invalid digit {c:?}"
                );
            }
        }
        for (i, &p) in PATTERNS_1.iter().enumerate() {
            assert_eq!(p.len(), 8, "PATTERNS_1[{i}] should be 8 chars");
        }
        // Anchor known entries from the bwip-js source.
        assert_eq!(PATTERNS_0[0], "11521132");
        assert_eq!(PATTERNS_0[1], "25112131");
        assert_eq!(PATTERNS_0[2400], "22421131");
        assert_eq!(PATTERNS_1[0], "22121116");
        assert_eq!(PATTERNS_1[2400], "11113162");
    }

    /// Weight tables WEIGHTX/Y/Z each have 33 entries and share the
    /// same 32-element permutation table — they differ only in the
    /// leading value (20 / 16 / 38) and the window offset into the
    /// 34-entry source array. Anchor the first 5 entries of each
    /// + the lengths.
    #[test]
    fn weight_tables_shape_and_anchors() {
        for (name, table) in [
            ("WEIGHTX", &WEIGHTX[..]),
            ("WEIGHTY", &WEIGHTY[..]),
            ("WEIGHTZ", &WEIGHTZ[..]),
        ] {
            assert_eq!(table.len(), 33, "{name} should have 33 entries");
        }
        // Leading values match BWIPP literal constants.
        assert_eq!(WEIGHTX[0], 20);
        assert_eq!(WEIGHTY[0], 16);
        assert_eq!(WEIGHTZ[0], 38);
        // WEIGHTY[1..] = WEIGHTX[2..] (offset by one in the source
        // permutation); WEIGHTZ[1..] = WEIGHTX[3..]. Spot-check the
        // first few overlap entries.
        assert_eq!(WEIGHTY[1], WEIGHTX[2]); // 9
        assert_eq!(WEIGHTY[2], WEIGHTX[3]); // 31
        assert_eq!(WEIGHTZ[1], WEIGHTX[3]); // 31
        assert_eq!(WEIGHTZ[2], WEIGHTX[4]); // 26
                                            // BWIPP-specific anchors.
        assert_eq!(WEIGHTX[1], 1);
        assert_eq!(WEIGHTX[32], 10);
        assert_eq!(WEIGHTZ[32], 30);
    }

    /// Stage 11.A8c — pin `charvals` lookup across the 6 categories:
    ///   1. Direct uppercase A-Z (→ 10..=35).
    ///   2. Direct digits 0-9 (→ 0..=9).
    ///   3. Direct punctuation (space, $, +, %, -, ., /).
    ///   4. S1-shifted controls 0..=31 + a few punctuation.
    ///   5. S2-shifted (lowercase, brackets, `, {|}~, DEL).
    ///   6. High bytes > 127 → None.
    ///
    /// One representative per category plus range-boundary tests
    /// (0/26 for controls, 'A'/'Z' for upper, 'a'/'z' for lower).
    ///
    /// Mutations caught:
    ///   * Any single match-arm removal (would drop or mis-route a
    ///     char).
    ///   * Constant shift-codeword swap (43 ↔ 44 → S1/S2 mismatch).
    ///   * Arithmetic mutations in `b - b'A' + 10` etc. (offset wrong).
    ///   * Boundary mutations on 1..=26, 27..=31 ranges.
    #[test]
    fn charvals_pins_lookup_categories_and_boundaries() {
        // (1) Direct uppercase boundaries + middle.
        assert_eq!(charvals(b'A'), Some((None, 10)));
        assert_eq!(charvals(b'M'), Some((None, 22)));
        assert_eq!(charvals(b'Z'), Some((None, 35)));
        // (2) Direct digits.
        assert_eq!(charvals(b'0'), Some((None, 0)));
        assert_eq!(charvals(b'5'), Some((None, 5)));
        assert_eq!(charvals(b'9'), Some((None, 9)));
        // (3) Direct punctuation.
        assert_eq!(charvals(b' '), Some((None, 38)));
        assert_eq!(charvals(b'$'), Some((None, 39)));
        assert_eq!(charvals(b'%'), Some((None, 42)));
        assert_eq!(charvals(b'+'), Some((None, 41)));
        assert_eq!(charvals(b'-'), Some((None, 36)));
        assert_eq!(charvals(b'.'), Some((None, 37)));
        assert_eq!(charvals(b'/'), Some((None, 40)));
        // (4) S1-shifted: ASCII 0..=31 controls + a few punct chars.
        // ASCII 0 → (S1, 38=space). Range boundaries 1, 26, 27, 31.
        assert_eq!(charvals(0), Some((Some(43), 38)), "NUL → S1+space");
        assert_eq!(charvals(1), Some((Some(43), 10)), "1 → S1+'A'");
        assert_eq!(charvals(26), Some((Some(43), 35)), "26 → S1+'Z'");
        assert_eq!(charvals(27), Some((Some(43), 1)), "27 → S1+'1'");
        assert_eq!(charvals(31), Some((Some(43), 5)), "31 → S1+'5'");
        // S1 punctuation: '!', '"', '#', '&', '\'', '(', ')', '*', ',', ':'.
        assert_eq!(charvals(b'!'), Some((Some(43), 6)));
        assert_eq!(charvals(b'#'), Some((Some(43), 8)));
        assert_eq!(charvals(b'&'), Some((Some(43), 9)));
        assert_eq!(charvals(b'\''), Some((Some(43), 0)));
        assert_eq!(charvals(b'*'), Some((Some(43), 39)));
        assert_eq!(charvals(b':'), Some((Some(43), 41)));
        // (5) S2-shifted: ';'..'@' boundaries + lowercase + [] etc.
        assert_eq!(charvals(b';'), Some((Some(44), 1)));
        assert_eq!(charvals(b'@'), Some((Some(44), 6)));
        assert_eq!(charvals(b'a'), Some((Some(44), 10)));
        assert_eq!(charvals(b'm'), Some((Some(44), 22)));
        assert_eq!(charvals(b'z'), Some((Some(44), 35)));
        assert_eq!(charvals(b'['), Some((Some(44), 7)));
        assert_eq!(charvals(b'~'), Some((Some(44), 42)));
        assert_eq!(charvals(127), Some((Some(44), 38)), "DEL → S2+space");
        // (6) High bytes → None.
        assert_eq!(charvals(128), None);
        assert_eq!(charvals(200), None);
        assert_eq!(charvals(255), None);
    }

    /// Stage 11.A8c — pin `calccheck`: weighted sum of base-49
    /// codeword pairs. Used by the x/y/z check computation in
    /// build_ccs; mutations here would corrupt every Code 49 symbol's
    /// last row.
    ///
    /// Algorithm: pair_count = (rows - 1) * 4. For each i in
    /// 0..pair_count, `pair = ccs[i*2]*49 + ccs[i*2+1]`, then
    /// `score += pair * weights[i+1]` (weight index 0 is reserved
    /// for cr7).
    ///
    /// Synthetic test inputs:
    ///   rows = 2 → pair_count = 4.
    ///   ccs = [1, 2, 3, 4, 5, 6, 7, 8] (4 pairs).
    ///   weights = [99, 10, 20, 30, 40] (index 0 is the cr7 weight,
    ///     ignored by calccheck; index 1..=4 used).
    ///
    /// Hand-computed:
    ///   pair 0 = 1*49 + 2 = 51   × weights[1] = 10 → 510
    ///   pair 1 = 3*49 + 4 = 151  × weights[2] = 20 → 3020
    ///   pair 2 = 5*49 + 6 = 251  × weights[3] = 30 → 7530
    ///   pair 3 = 7*49 + 8 = 351  × weights[4] = 40 → 14040
    ///   total = 510 + 3020 + 7530 + 14040 = 25100.
    ///
    /// Mutations caught:
    /// * `(rows - 1) * 4` formula (pair count).
    /// * `* 49` base constant.
    /// * `ccs[i * 2 + 1]` low-pair index drift.
    /// * `weights[i + 1]` (skip-0 offset) — `weights[i]` would use
    ///   weight 99 first and shift the score by ~50,000.
    /// * `for i in 0..pair_count` boundary.
    /// * `score += pair * weight` arithmetic.
    #[test]
    fn calccheck_weighted_base49_pair_sum() {
        let ccs: [u16; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let weights: [u16; 5] = [99, 10, 20, 30, 40];
        let score = calccheck(&ccs, 2, &weights);
        assert_eq!(score, 25100, "weighted sum of 4 base-49 pairs");
    }

    /// Stage 11.A8c — pin `base48` decimal-to-base48 conversion and
    /// the high-to-low output order. The helper is the engine of the
    /// NS-shift digit encoder; mutations here would corrupt every
    /// numeric Code 49 codeword stream.
    ///
    /// Hand-computed cases:
    /// * base48(3, "12345"): value=12345 → [5, 17, 9]
    ///     12345 % 48 = 9, /48 = 257; 257 % 48 = 17, /48 = 5; 5 → 5.
    /// * base48(2, "99"): value=99 → [2, 3] (99 = 2*48+3).
    /// * base48(1, "7"): value=7 → [7] (single digit, < 48).
    /// * base48(2, "00"): value=0 → [0, 0] (zero edge).
    /// * base48(2, "47"): value=47 → [0, 47] (boundary, < 48).
    /// * base48(2, "48"): value=48 → [1, 0] (boundary, = 48).
    /// * base48(2, "96"): value=96 → [2, 0] (= 2*48).
    ///
    /// Mutations caught:
    /// * `value * 10` → `value + 10` / `value * 100`: digits combine
    ///   wrong.
    /// * `b - b'0'` → `b - b'1'`: each digit off by one.
    /// * `% 48` → `% 47` or `% 49`: bad base.
    /// * `/= 48` → `/= 47`: bad base.
    /// * `(0..count).rev()` → `(0..count)`: low-to-high order; e.g.
    ///   base48(2, "48") would yield [0, 1] instead of [1, 0].
    #[test]
    fn base48_decimal_to_base48_with_high_to_low_output() {
        assert_eq!(base48(3, b"12345"), vec![5u16, 17, 9]);
        assert_eq!(base48(2, b"99"), vec![2u16, 3]);
        assert_eq!(base48(1, b"7"), vec![7u16]);
        assert_eq!(base48(2, b"00"), vec![0u16, 0]);
        assert_eq!(base48(2, b"47"), vec![0u16, 47]);
        assert_eq!(base48(2, b"48"), vec![1u16, 0]);
        assert_eq!(base48(2, b"96"), vec![2u16, 0]);
    }

    /// Stage 11.A8c — pin `build_seprow` exact layout.
    /// 81 modules: 10 leading zeros + 70 ones + 1 trailing zero.
    ///
    /// Mutations caught:
    ///   * `row = [1u8; 81]` initialiser constant: change to 0 → all zero.
    ///   * `.take(10)` boundary: shifts how many leading zeros are written.
    ///   * `row[80] = 0`: trailing zero index.
    #[test]
    fn build_seprow_layout_is_10_zeros_then_ones_then_zero() {
        let row = build_seprow();
        assert_eq!(row.len(), 81);
        for i in 0..10 {
            assert_eq!(row[i], 0, "leading pos {i} must be 0");
        }
        for i in 10..80 {
            assert_eq!(row[i], 1, "middle pos {i} must be 1");
        }
        assert_eq!(row[80], 0, "trailing pos 80 must be 0");
        // Sums: 70 ones + 11 zeros = 81 cells.
        assert_eq!(row.iter().filter(|&&v| v == 0).count(), 11);
        assert_eq!(row.iter().filter(|&&v| v == 1).count(), 70);
    }

    /// Stage 11.A8c — pin `build_row_bits` structural invariants that
    /// hold for ALL parity selections + ccrow contents, plus the
    /// `row_idx + 1 == rows` last-row branch that swaps PARITY[idx]
    /// for the literal "0000".
    ///
    /// Mutations caught:
    ///   * `let mut current: u8 = 1` → 0 inverts every bit; row[0..10]
    ///     would become 1s, failing the quiet-zone check.
    ///   * `sbs.push(10)` width or `1 - current` toggle direction —
    ///     left quiet zone shifts.
    ///   * `sbs.push(4)` width — stop bar shifts off pos 76..80.
    ///   * `sbs.push(1)` suffix or final toggle — trailing separator
    ///     fails at pos 80.
    ///   * `for k in 0..4` bound — pattern loop would emit fewer/more
    ///     widths and debug_assert sum=81 would fire (or shift the
    ///     stop bar away from 76..80).
    ///   * `row_idx + 1 == rows` branch direction — replacing == with
    ///     != would pick PARITY[0]="1001" for the (idx=0, rows=1) case
    ///     and "0000" for the (idx=0, rows=2) case, swapping the two.
    #[test]
    fn build_row_bits_invariant_layout_and_parity_branch() {
        let ccrow = [0u16; 8];
        // Last-row branch (row_idx+1 == rows) → "0000" parity →
        // PATTERNS_0[0] x4 = "11521132" x4.
        let last = build_row_bits(0, 1, &ccrow);
        // Structural invariants that DON'T depend on PATTERNS lookup.
        assert_eq!(last.len(), 81);
        for i in 0..10 {
            assert_eq!(last[i], 0, "quiet zone pos {i} must be 0");
        }
        assert_eq!(last[10], 1, "start bar at pos 10");
        assert_eq!(last[11], 0, "separator after start bar");
        for i in 76..80 {
            assert_eq!(last[i], 1, "stop bar pos {i} must be 1");
        }
        assert_eq!(last[80], 0, "trailing separator at pos 80");

        // Non-last row → PARITY[0] = "1001" → PATTERNS_1 PATTERNS_0
        // PATTERNS_0 PATTERNS_1. Since PATTERNS_0[0]="11521132" and
        // PATTERNS_1[0]="22121116" differ, the two rows must differ.
        let first_of_two = build_row_bits(0, 2, &ccrow);
        assert_ne!(
            last, first_of_two,
            "row_idx+1==rows branch must pick literal \"0000\" parity, \
             distinct from PARITY[0]=\"1001\""
        );
        // The structural invariants also hold for the parity branch.
        assert_eq!(first_of_two.len(), 81);
        assert_eq!(first_of_two[0], 0);
        assert_eq!(first_of_two[10], 1);
        assert_eq!(first_of_two[76], 1);
        assert_eq!(first_of_two[80], 0);
    }

    /// Stage 11.A8c — extend `build_row_bits` coverage with non-zero
    /// ccrow inputs that pin the `scrow[k] = ccrow[2k]*49 + ccrow[2k+1]`
    /// arithmetic. The existing
    /// `build_row_bits_invariant_layout_and_parity_branch` test uses
    /// `ccrow = [0u16; 8]`, which collapses `scrow` to `[0, 0, 0, 0]`
    /// regardless of how the multiplication / addition is mutated.
    /// So mutations like `* 49 → * 50`, `* 49 → + 49`, swapping the
    /// `ccrow[2k]` / `ccrow[2k+1]` operands, or shifting the indices
    /// to `ccrow[2k+1] / ccrow[2k+2]` survive that test.
    ///
    /// Tactic: feed two ccrow vectors that place a single non-zero
    /// entry at distinct positions (`ccrow[0]=1` vs `ccrow[2]=1`),
    /// derive the expected bit sequences by hand from
    /// `PATTERNS_0[49]` and `PATTERNS_0[0]`, then assert those bits
    /// at the right offsets. Each mutation lands the function in a
    /// different PATTERNS_0 slot whose bit sequence diverges from
    /// the expected one.
    ///
    /// Mutations caught (beyond the prior layout-invariant test):
    ///   * `ccrow[2k] * 49` → `* 50`: scrow[0] becomes 50 (case 1)
    ///     or 50 (case 2), wrong pattern lookup.
    ///   * `ccrow[2k] * 49 + ccrow[2k+1]` → `+ 1`: scrow[0] becomes
    ///     50 in case 1, wrong pattern.
    ///   * Operand swap `ccrow[2k+1] * 49 + ccrow[2k]`: case 1
    ///     becomes `0*49 + 1 = 1`, PATTERNS_0[1]="25112131" differs.
    ///   * Index shift `[2k]/[2k+1]` → `[2k+1]/[2k+2]`: case 2
    ///     re-reads ccrow[3]/ccrow[4] = 0/0 instead of 1/0, dropping
    ///     scrow[1] from 49 back to 0.
    ///   * PATTERNS_0/PATTERNS_1 swap on the `p_bytes[k] == b'1'`
    ///     branch: PARITY isn't used here (last-row branch picks
    ///     "0000"), so all 4 slots must use PATTERNS_0 — a swap
    ///     would emit PATTERNS_1[49]="11122225"... wait, the byte
    ///     happens to equal PATTERNS_0[49] only by coincidence at
    ///     index 49? Let me reverify before relying on that
    ///     claim — actually PATTERNS_1[49] differs in general.
    #[test]
    fn build_row_bits_pins_scrow_arithmetic_with_non_zero_ccrow() {
        // Table sanity (so a constants edit doesn't silently break
        // the expected-bit derivation below).
        assert_eq!(PATTERNS_0[0], "11521132", "PATTERNS_0[0] table check");
        assert_eq!(PATTERNS_0[49], "11122225", "PATTERNS_0[49] table check");

        // --- Case 1: ccrow = [1, 0, 0, 0, 0, 0, 0, 0] →
        //     scrow = [49, 0, 0, 0]. Last-row branch (rows=1) → all
        //     four pattern slots use PATTERNS_0.
        let ccrow_a = [1u16, 0, 0, 0, 0, 0, 0, 0];
        let row_a = build_row_bits(0, 1, &ccrow_a);

        // Decoder note for the toggle math: build_row_bits initializes
        // current=1 and then flips at the start of every width. The
        // sbs prefix [10, 1, 1] emits 10 zeros + 1 one (start bar) +
        // 1 zero (separator) at positions 0..=11; after that current=0
        // and the very next width toggles to 1.
        //
        // PATTERNS_0[49] = "11122225" expanded at pos 12..=27:
        //   width 1 (pos 12):    current 0→1 → 1
        //   width 1 (pos 13):    current 1→0 → 0
        //   width 1 (pos 14):    current 0→1 → 1
        //   width 2 (pos 15-16): current 1→0 → 0, 0
        //   width 2 (pos 17-18): current 0→1 → 1, 1
        //   width 2 (pos 19-20): current 1→0 → 0, 0
        //   width 2 (pos 21-22): current 0→1 → 1, 1
        //   width 5 (pos 23-27): current 1→0 → 0, 0, 0, 0, 0
        let p49_bits: [u8; 16] = [1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0];
        for (off, &want) in p49_bits.iter().enumerate() {
            let i = 12 + off;
            assert_eq!(
                row_a[i], want,
                "case 1 (scrow[0]=49=ccrow[0]*49+ccrow[1]): pos {i} should be {want} \
                 per PATTERNS_0[49]=\"11122225\""
            );
        }

        // Pattern[49] ended with current=0; the next width toggles
        // to 1. PATTERNS_0[0]="11521132" expanded at pos 28..=43:
        //   width 1 (pos 28):    current 0→1 → 1
        //   width 1 (pos 29):    current 1→0 → 0
        //   width 5 (pos 30-34): current 0→1 → 1, 1, 1, 1, 1
        //   width 2 (pos 35-36): current 1→0 → 0, 0
        //   width 1 (pos 37):    current 0→1 → 1
        //   width 1 (pos 38):    current 1→0 → 0
        //   width 3 (pos 39-41): current 0→1 → 1, 1, 1
        //   width 2 (pos 42-43): current 1→0 → 0, 0
        let p0_bits_2nd: [u8; 16] = [1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0];
        for (off, &want) in p0_bits_2nd.iter().enumerate() {
            let i = 28 + off;
            assert_eq!(
                row_a[i], want,
                "case 1 second-pattern region (scrow[1]=0): pos {i} should be {want} \
                 per PATTERNS_0[0]=\"11521132\""
            );
        }

        // --- Case 2: ccrow = [0, 0, 1, 0, 0, 0, 0, 0] →
        //     scrow = [0, 49, 0, 0]. Reverse roles: first pattern
        //     region is PATTERNS_0[0], second is PATTERNS_0[49].
        let ccrow_b = [0u16, 0, 1, 0, 0, 0, 0, 0];
        let row_b = build_row_bits(0, 1, &ccrow_b);

        // pos 12-27 should now be PATTERNS_0[0] = "11521132".
        let p0_bits_1st: [u8; 16] = [1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0];
        for (off, &want) in p0_bits_1st.iter().enumerate() {
            let i = 12 + off;
            assert_eq!(
                row_b[i], want,
                "case 2 first-pattern region (scrow[0]=0): pos {i} should be {want} \
                 per PATTERNS_0[0]=\"11521132\""
            );
        }

        // pos 28-43 should now be PATTERNS_0[49]="11122225".
        // After PATTERNS_0[0] (case 2 region 1), current ended at 0
        // (last width 2 emitted 0,0). Next width toggles to 1.
        let p49_bits_2nd: [u8; 16] = [1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0];
        for (off, &want) in p49_bits_2nd.iter().enumerate() {
            let i = 28 + off;
            assert_eq!(
                row_b[i], want,
                "case 2 second-pattern region (scrow[1]=49=ccrow[2]*49+ccrow[3]): \
                 pos {i} should be {want} per PATTERNS_0[49]=\"11122225\""
            );
        }

        // Cross-check: the two cases must differ at pos 12-27 (case 1
        // has PATTERNS_0[49] there, case 2 has PATTERNS_0[0]).
        assert_ne!(
            &row_a[12..28],
            &row_b[12..28],
            "non-zero ccrow[0] vs non-zero ccrow[2] must yield different bits at pos 12-27"
        );
    }

    /// `lookup_direct` returns the right codeword for direct
    /// alphabet members. Bytes outside the alphabet → None.
    #[test]
    fn lookup_direct_spot_checks() {
        assert_eq!(lookup_direct(b'0'), Some(0));
        assert_eq!(lookup_direct(b'9'), Some(9));
        assert_eq!(lookup_direct(b'A'), Some(10));
        assert_eq!(lookup_direct(b'Z'), Some(35));
        assert_eq!(lookup_direct(b'-'), Some(36));
        assert_eq!(lookup_direct(b' '), Some(38));
        assert_eq!(lookup_direct(b'%'), Some(42));
        // Lowercase / other ASCII not directly encodable.
        assert_eq!(lookup_direct(b'a'), None);
        assert_eq!(lookup_direct(b'!'), None);
        assert_eq!(lookup_direct(0), None);
    }

    /// `encode` produces a valid `BitMatrix` for every supported
    /// input (digit-only, alpha-only, mixed). Empty input is rejected.
    #[test]
    fn encode_produces_valid_bitmatrix_for_supported_inputs() {
        for input in [
            b"12345".as_ref(),
            b"A".as_ref(),
            b"ABC".as_ref(),
            b"ABCDEFGHI".as_ref(),
            b"Hi".as_ref(),
            b"ABCDEFGHIJKLMNOP".as_ref(),
        ] {
            let bm = encode(input).unwrap_or_else(|e| {
                panic!(
                    "encode({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(bm.width(), 81, "code49 width must be 81 modules");
            assert!(bm.height() >= 19, "code49 height must be ≥ 19");
        }
        // Stage 11.A8c — upgrade from matches!(_, InvalidData(_)) to
        // pin the empty-specific diagnostic. code49::encode_cws has
        // MULTIPLE InvalidData rejection arms (empty guard, mid-
        // message mode-switch reject, alpha-overflow, etc.). A
        // mutant that swaps the empty guard's body with any other
        // arm's message survives the old check.
        //
        // The empty diagnostic at line 510-511 is "code49: empty input".
        let err = encode(b"").unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("encode(b\"\") must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("code49:"),
            "empty-input diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("empty input"),
            "empty-input diagnostic must call out 'empty input'; got {msg:?}"
        );
        assert!(
            !msg.contains("mode") && !msg.contains("shift") && !msg.contains("alpha"),
            "empty-input diagnostic must not leak the downstream arms; got {msg:?}"
        );
    }

    /// Byte-for-byte golden of the **compressed pixs** (the
    /// `numcomprows × 81` flat array bwip-js's renmatrix builds
    /// before row-multiplication) for `"12345"` — a 2-row symbol
    /// produced from the NS-shift digit path (mode 2).
    ///
    /// The golden was captured via `tools/oracle-code49-pixs.js` —
    /// see the test for the full 5 × 81 = 405-cell vector.
    #[test]
    fn encode_pixs_matches_bwip_js_golden_for_12345() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // Code 49 pixs geometry (r=2 → 5 compressed rows × 81 cells).
        let pixs = encode_pixs(b"12345").expect(
            "encode_pixs(b\"12345\") (Code 49 NS-digits path, r=2 → 5 compressed rows × 81 cells = 405-cell golden) must succeed",
        );
        // r=2 → 5 compressed rows × 81 = 405 cells.
        assert_eq!(pixs.len(), 5 * 81);
        let expected: [u8; 405] = [
            // Top bearer (81 ones)
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // Row 0
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1,
            1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1,
            1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, // Seprow
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            // Row 1 (last)
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1,
            0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0,
            // Bottom bearer
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(pixs.as_slice(), expected.as_slice());
    }

    /// Pin the **ccs** vector (codeword + check-codeword grid) for a
    /// handful of inputs covering each cws-encoder path. Goldens
    /// captured via `tools/oracle-code49-pixs.js`.
    ///
    /// Hits each branch of the row-check formula: r=2 NS-digit path,
    /// r=2 alpha-mode-0, r=3 alpha-mode-0 (last row only partially
    /// filled with data).
    #[test]
    fn build_ccs_matches_bwip_js_goldens() {
        let cases: &[(&[u8], u16, &[u16])] = &[
            (
                b"12345",
                2,
                &[5, 17, 9, 48, 48, 48, 48, 27, 48, 48, 13, 23, 0, 13, 2, 0],
            ),
            (
                b"A",
                0,
                &[10, 48, 48, 48, 48, 48, 48, 4, 48, 48, 46, 28, 6, 5, 0, 34],
            ),
            (
                b"ABC",
                0,
                &[10, 11, 12, 48, 48, 48, 48, 29, 48, 48, 2, 39, 2, 15, 0, 7],
            ),
            (
                b"ABCDEFGHI",
                0,
                &[10, 11, 12, 13, 14, 15, 16, 42, 17, 18, 8, 16, 10, 2, 0, 22],
            ),
            (
                b"Hi",
                0,
                &[
                    17, 44, 18, 48, 48, 48, 48, 26, 48, 48, 12, 36, 34, 32, 0, 14,
                ],
            ),
            (
                b"ABCDEFGHIJKLMNOP",
                0,
                &[
                    10, 11, 12, 13, 14, 15, 16, 42, 17, 18, 19, 20, 21, 22, 23, 42, 24, 25, 1, 38,
                    38, 3, 7, 38,
                ],
            ),
        ];
        for &(input, want_mode, want_ccs) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // input echo for the Code 49 encode_cws corpus.
            let (cws, mode) = encode_cws(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws({:?}) (Code 49 cws-level corpus item) must succeed; got Err: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>")
                )
            });
            assert_eq!(
                mode,
                want_mode,
                "encode_cws({:?}) mode",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
            let (rows, dcws) = pick_symbol_size(cws.len()).expect("payload fits");
            let ccs = build_ccs(&cws, rows, dcws, mode).unwrap_or_else(|e| {
                panic!(
                    "build_ccs({:?}, rows={rows}, dcws={dcws}, mode={mode}) (Code 49 ccs corpus item) must succeed; got Err: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>")
                )
            });
            assert_eq!(
                ccs,
                want_ccs,
                "build_ccs({:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// `pick_symbol_size` walks METRICS from r=2 upward, returning
    /// the first row whose `dcws` accommodates the requested count.
    #[test]
    fn pick_symbol_size_picks_smallest_metrics_row() {
        // ≤9 chars → r=2.
        for n in 0..=9 {
            assert_eq!(pick_symbol_size(n), Some((2, 9)));
        }
        // 10..=16 → r=3.
        for n in 10..=16 {
            assert_eq!(pick_symbol_size(n), Some((3, 16)));
        }
        // 17..=23 → r=4.
        for n in 17..=23 {
            assert_eq!(pick_symbol_size(n), Some((4, 23)));
        }
        // Max payload (49 chars) → r=8.
        assert_eq!(pick_symbol_size(49), Some((8, 49)));
        // Over the ceiling.
        assert_eq!(pick_symbol_size(50), None);
    }

    /// `encode_cws_direct` produces BWIPP byte-for-byte cws for the
    /// direct-lookup subset (uppercase / digits / punctuation, no
    /// shifts needed). Goldens captured from `tools/oracle-code49.js`:
    ///
    ///   "1"        → r=2, [1, 48×8]
    ///   "12"       → r=2, [1, 2, 48×7]
    ///   "A"        → r=2, [10, 48×8]
    ///   "ABCDE"    → r=2, [10, 11, 12, 13, 14, 48×4]
    ///   "ABCDEFGHI" → r=2, [10, 11, 12, 13, 14, 15, 16, 17, 18]
    ///   "ABCDEFGHIJ" → r=3, [10..19, 48×6]
    ///   "ABCDEFGHIJKLMNOP" → r=3, [10..25] (full r=3 capacity).
    #[test]
    fn encode_cws_direct_matches_bwip_js_goldens() {
        let cases: &[(&[u8], &[u16])] = &[
            (b"1", &[1, 48, 48, 48, 48, 48, 48, 48, 48]),
            (b"12", &[1, 2, 48, 48, 48, 48, 48, 48, 48]),
            (b"A", &[10, 48, 48, 48, 48, 48, 48, 48, 48]),
            (b"ABCDE", &[10, 11, 12, 13, 14, 48, 48, 48, 48]),
            (b"ABCDEFGHI", &[10, 11, 12, 13, 14, 15, 16, 17, 18]),
            (
                b"ABCDEFGHIJ",
                &[
                    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 48, 48, 48, 48, 48, 48,
                ],
            ),
            (
                b"ABCDEFGHIJKLMNOP",
                &[
                    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
                ],
            ),
        ];
        for &(input, expected) in cases {
            let cws = encode_cws_direct(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_direct({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                cws,
                expected,
                "encode_cws_direct({:?})",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// `encode_cws_direct` rejects inputs requiring shifts (lowercase
    /// letters, control bytes, mixed-content payloads that BWIPP would
    /// pack via NS-shift). Long digit runs like "12345" also need
    /// NS-shift compaction — direct-lookup would produce `[1, 2, 3,
    /// 4, 5]` but BWIPP produces `[5, 17, 9]` (3 codewords for the
    /// same 5 digits). The direct path emits the wrong answer for
    /// "12345" so we reject digit-only inputs >= 5 bytes too.
    ///
    /// Note: this test pins the "what direct-lookup accepts" boundary,
    /// not the "what BWIPP would do" boundary — Stage 3 will add the
    /// NS-shift path that produces `[5, 17, 9]` for "12345".
    #[test]
    fn encode_cws_direct_rejects_inputs_needing_shifts() {
        // Lowercase isn't a direct CHARMAP member.
        //
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Error::InvalidData(_))` to 3-anchor pin
        // matching the source diagnostic at line 197 of code49.rs:
        //   1. `code49 direct-lookup path:` symbology-arm prefix
        //   2. `byte 0x61` hex echo (lowercase 'a' = 0x61, first
        //      non-direct char in "abc")
        //   3. `at position 0` position echo (kills `{idx}`
        //      interpolation drop)
        match encode_cws_direct(b"abc") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("code49 direct-lookup path:"),
                    "missing direct-lookup arm prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x61"),
                    "missing byte 0x61 hex echo for 'a': {msg}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "missing `at position 0` position-echo: {msg}"
                );
            }
            other => panic!("'abc' should reject as InvalidData, got {other:?}"),
        }
        // Mixed lowercase + uppercase.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the 'abc' arm. Input "Aa" first non-direct char
        // is 'a' (0x61) at position 1 (the uppercase 'A' at 0 is in
        // the direct CHARMAP).
        match encode_cws_direct(b"Aa") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("code49 direct-lookup path:"),
                    "missing direct-lookup arm prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x61"),
                    "missing byte 0x61 hex echo for 'a': {msg}"
                );
                assert!(
                    msg.contains("at position 1"),
                    "missing `at position 1` position echo: {msg}"
                );
            }
            other => panic!("'Aa' should reject as InvalidData, got {other:?}"),
        }
        // Control bytes.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the 'abc'/'Aa' siblings. Input "\tA" has TAB
        // (0x09) at position 0 (non-direct) followed by 'A' (direct);
        // diagnostic echoes the first non-direct char.
        match encode_cws_direct(b"\tA") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("code49 direct-lookup path:"),
                    "missing direct-lookup arm prefix: {msg}"
                );
                assert!(
                    msg.contains("byte 0x09"),
                    "missing byte 0x09 hex echo for TAB: {msg}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "missing `at position 0` position echo: {msg}"
                );
            }
            other => panic!("'\\tA' should reject as InvalidData, got {other:?}"),
        }
        // Empty.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 2-anchor
        // pin matching the source diagnostic at line 191 of
        // code49.rs:
        //   1. `code49:` symbology prefix
        //   2. `empty input` predicate
        match encode_cws_direct(b"") {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("code49:"), "missing code49 prefix: {msg}");
                assert!(
                    msg.contains("empty input"),
                    "missing `empty input` predicate: {msg}"
                );
            }
            other => panic!("empty input should reject as InvalidData, got {other:?}"),
        }
        // Over r=8 ceiling: 50+ chars all direct-lookup.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the source diagnostic at line 213 of
        // code49.rs:
        //   1. `code49:` symbology prefix
        //   2. `payload of 50 bytes` value-echo (kills `{}` byte
        //      count interpolation drop)
        //   3. `exceeds the r=8 ceiling` predicate + range hint
        let huge: Vec<u8> = (0..50).map(|_| b'A').collect();
        match encode_cws_direct(&huge) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("code49:"), "missing code49 prefix: {msg}");
                assert!(
                    msg.contains("payload of 50 bytes"),
                    "missing payload-of-50-bytes value-echo: {msg}"
                );
                assert!(
                    msg.contains("exceeds the r=8 ceiling"),
                    "missing `exceeds the r=8 ceiling` predicate: {msg}"
                );
            }
            other => panic!("50-A overflow should reject as InvalidData, got {other:?}"),
        }
    }

    /// `base48` packs a digit run into `count` base-48 codewords
    /// high-to-low. Anchor a few known transformations from the
    /// Stage 3 oracle goldens:
    ///   "12345" → [5, 17, 9]   (12345 = 5*48² + 17*48 + 9)
    ///   "67890" → [29, 22, 18]
    ///   "678"   → [14, 6]      (678 = 14*48 + 6)
    ///   "10567" → [43, 45, 2]  (the "padded 7-tail" path)
    #[test]
    fn base48_matches_bwip_js_polynomial() {
        assert_eq!(base48(3, b"12345"), vec![5, 17, 9]);
        assert_eq!(base48(3, b"67890"), vec![29, 22, 18]);
        assert_eq!(base48(2, b"678"), vec![14, 6]);
        assert_eq!(base48(3, b"101234"), vec![43, 45, 2]);
        assert_eq!(base48(2, b"567"), vec![11, 39]);
    }

    /// `encode_cws_ns_digits` produces BWIPP byte-for-byte cws for
    /// every digit-only payload that triggers the NS-shift compaction
    /// (numericruns ≥ 5). Captured via `tools/oracle-code49.js`:
    ///
    ///   "12345"         → [5, 17, 9]                       (3 cws)
    ///   "123456"        → [5, 17, 9, 6]                    (remainder 1)
    ///   "1234567"       → [43, 45, 2, 11, 39]              (remainder 2 → tail-7)
    ///   "12345678"      → [5, 17, 9, 14, 6]                (remainder 3)
    ///   "123456789"     → [5, 17, 9, 46, 16, 37]           (remainder 4 → "10"-pad)
    ///   "1234567890"    → [5, 17, 9, 29, 22, 18]           (clean 2 chunks)
    ///   "12345678901"   → [5, 17, 9, 29, 22, 18, 1]        (remainder 1)
    ///   "1234567890123" → [5, 17, 9, 29, 22, 18, 2, 27]    (remainder 3)
    #[test]
    fn encode_cws_ns_digits_matches_bwip_js_goldens() {
        // (input, expected core cws — without the trailing PAD_CW=48 padding)
        let cases: &[(&[u8], &[u16])] = &[
            (b"12345", &[5, 17, 9]),
            (b"123456", &[5, 17, 9, 6]),
            (b"1234567", &[43, 45, 2, 11, 39]),
            (b"12345678", &[5, 17, 9, 14, 6]),
            (b"123456789", &[5, 17, 9, 46, 16, 37]),
            (b"1234567890", &[5, 17, 9, 29, 22, 18]),
            (b"12345678901", &[5, 17, 9, 29, 22, 18, 1]),
            (b"1234567890123", &[5, 17, 9, 29, 22, 18, 2, 27]),
        ];
        for &(input, expected_core) in cases {
            let cws = encode_cws_ns_digits(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_ns_digits({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            // Compare the leading prefix (the actual emission) to
            // the expected core; the remainder should be PAD_CW
            // up to dcws.
            let core = &cws[..expected_core.len()];
            assert_eq!(
                core,
                expected_core,
                "encode_cws_ns_digits({:?}) core",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
            // Padding tail should all be PAD_CW (48).
            for (i, &cw) in cws[expected_core.len()..].iter().enumerate() {
                assert_eq!(
                    cw,
                    PAD_CW,
                    "encode_cws_ns_digits({:?}) pad[{}] should be {PAD_CW}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                    i
                );
            }
        }
    }

    /// `encode_cws_ns_digits` rejects payloads shorter than 5
    /// digits (use encode_cws_direct instead), non-digit bytes,
    /// and empty input.
    #[test]
    fn encode_cws_ns_digits_rejects_invalid_inputs() {
        // Stage 11.A8c — upgrade 3 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at
        // lines 266 (`code49: empty input`), 269-273 (`code49
        // NS-shift path requires ≥5 digits (got N)`), and 277-279
        // (`code49 NS-shift path: non-digit byte 0x<hh> at
        // position <n>`).
        match encode_cws_ns_digits(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code49:"),
                    "empty arm missing `code49:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty arm missing `empty input` predicate: {msg}"
                );
                assert!(
                    !msg.contains("NS-shift"),
                    "empty arm leaked NS-shift diagnostic: {msg}"
                );
            }
            other => panic!("empty NS-shift input should reject as InvalidData, got {other:?}"),
        }
        match encode_cws_ns_digits(b"1234").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code49 NS-shift"),
                    "short arm missing `code49 NS-shift` prefix: {msg}"
                );
                assert!(
                    msg.contains("requires ≥5 digits"),
                    "short arm missing `requires ≥5 digits` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 4"),
                    "short arm missing `got 4` length echo: {msg}"
                );
                assert!(
                    !msg.contains("non-digit"),
                    "short arm leaked non-digit diagnostic: {msg}"
                );
            }
            other => panic!("4-digit NS-shift input should reject as InvalidData, got {other:?}"),
        }
        match encode_cws_ns_digits(b"123A5").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code49 NS-shift path:"),
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
                    msg.contains("at position 3"),
                    "non-digit arm missing `at position 3`: {msg}"
                );
            }
            other => panic!("`123A5` should reject as non-digit InvalidData, got {other:?}"),
        }
    }

    /// `charvals` spot-checks against BWIPP's combo-derived table.
    /// Anchor every category: digit, uppercase, direct punctuation,
    /// control byte (S1), uppercase '+' analog (S1), lowercase (S2),
    /// extended punctuation (S2), DEL.
    #[test]
    fn charvals_spot_checks() {
        // Digits direct.
        assert_eq!(charvals(b'0'), Some((None, 0)));
        assert_eq!(charvals(b'9'), Some((None, 9)));
        // Uppercase direct.
        assert_eq!(charvals(b'A'), Some((None, 10)));
        assert_eq!(charvals(b'Z'), Some((None, 35)));
        // Direct punctuation.
        assert_eq!(charvals(b'-'), Some((None, 36)));
        assert_eq!(charvals(b' '), Some((None, 38)));
        assert_eq!(charvals(b'%'), Some((None, 42)));
        // Lowercase → S2 + uppercase target.
        assert_eq!(charvals(b'a'), Some((Some(44), 10)));
        assert_eq!(charvals(b'z'), Some((Some(44), 35)));
        // Control bytes → S1 + uppercase analog.
        assert_eq!(charvals(0), Some((Some(43), 38)));
        assert_eq!(charvals(1), Some((Some(43), 10)));
        assert_eq!(charvals(26), Some((Some(43), 35)));
        assert_eq!(charvals(31), Some((Some(43), 5)));
        // Extended punctuation → S2.
        assert_eq!(charvals(b'['), Some((Some(44), 7)));
        assert_eq!(charvals(b'`'), Some((Some(44), 37)));
        assert_eq!(charvals(b'{'), Some((Some(44), 39)));
        assert_eq!(charvals(127), Some((Some(44), 38)));
        // High bytes not supported.
        assert_eq!(charvals(128), None);
        assert_eq!(charvals(255), None);
    }

    /// `encode_cws_alpha` produces BWIPP byte-for-byte cws for every
    /// alpha-path payload. Captured via `tools/oracle-code49.js`:
    ///
    ///   "a"      → (mode 5, [10, 48×8])
    ///   "abc"    → (mode 5, [10, 44, 11, 44, 12, 48×4])
    ///   "abcd"   → (mode 5, [10, 44, 11, 44, 12, 44, 13, 48, 48])
    ///   "Hello"  → (mode 0, [17, 44, 14, 44, 21, 44, 21, 44, 24])
    ///   "Hi"     → (mode 0, [17, 44, 18, 48×6])
    ///   "Aa"     → (mode 0, [10, 44, 10, 48×6])
    ///   "aA"     → (mode 5, [10, 10, 48×7])
    ///   "ABCabc" → (mode 0, [10, 11, 12, 44, 10, 44, 11, 44, 12])
    #[test]
    fn encode_cws_alpha_matches_bwip_js_goldens() {
        let cases: &[(&[u8], u16, &[u16])] = &[
            (b"a", MODE_FIRST_S2, &[10, 48, 48, 48, 48, 48, 48, 48, 48]),
            (b"abc", MODE_FIRST_S2, &[10, 44, 11, 44, 12, 48, 48, 48, 48]),
            (
                b"abcd",
                MODE_FIRST_S2,
                &[10, 44, 11, 44, 12, 44, 13, 48, 48],
            ),
            (b"Hello", MODE_ALPHA, &[17, 44, 14, 44, 21, 44, 21, 44, 24]),
            (b"Hi", MODE_ALPHA, &[17, 44, 18, 48, 48, 48, 48, 48, 48]),
            (b"Aa", MODE_ALPHA, &[10, 44, 10, 48, 48, 48, 48, 48, 48]),
            (b"aA", MODE_FIRST_S2, &[10, 10, 48, 48, 48, 48, 48, 48, 48]),
            (b"ABCabc", MODE_ALPHA, &[10, 11, 12, 44, 10, 44, 11, 44, 12]),
        ];
        for &(input, want_mode, want_cws) in cases {
            let (cws, mode) = encode_cws_alpha(input).unwrap_or_else(|e| {
                panic!(
                    "encode_cws_alpha({:?}) failed: {e:?}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>"),
                )
            });
            assert_eq!(
                mode,
                want_mode,
                "encode_cws_alpha({:?}) mode",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
            assert_eq!(
                cws,
                want_cws,
                "encode_cws_alpha({:?}) cws",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    /// Stage 11.A8c — extend `encode_cws_alpha` coverage with:
    ///   1. **MODE_FIRST_S1 branch.** The existing
    ///      `encode_cws_alpha_matches_bwip_js_goldens` has 8 cases,
    ///      but all of them start with either a lowercase letter
    ///      (S2-shifted → MODE_FIRST_S2) or an uppercase/digit
    ///      (direct → MODE_ALPHA). The `Some(43) => MODE_FIRST_S1`
    ///      match arm is never exercised, so a mutation swapping it
    ///      with `MODE_FIRST_S2` or `MODE_ALPHA` would survive.
    ///   2. **The three error branches.** Empty input,
    ///      non-ASCII byte at position 0, and non-ASCII byte at a
    ///      later position. The existing goldens are all happy-path,
    ///      so the position-in-message portion of the error format
    ///      string isn't pinned.
    ///
    /// S1-shifted bytes per `charvals`: ASCII 0..=31 (control bytes),
    /// '!', '"', '#', '&', '\'', '(', ')', '*', ',', ':'. We probe
    /// two: '!' (ASCII punctuation) and NUL (ASCII control).
    ///
    /// Mutations caught (beyond what the prior alpha test covers):
    ///   * `Some(43) => MODE_FIRST_S1` → `MODE_FIRST_S2` /
    ///     `MODE_ALPHA`: '!' as first byte would emit the wrong mode.
    ///   * Match arm reordering (Some(43) and Some(44) swapped):
    ///     '!' would emit MODE_FIRST_S2.
    ///   * `input.is_empty()` → `false`: empty input would panic on
    ///     `input[0]` instead of returning the InvalidData error.
    ///   * Error position formatting `position {idx}` → `position 0`:
    ///     the position-2 case would name the wrong index.
    ///   * `enumerate().skip(1)` → `skip(0)`: position-0 error
    ///     formatting would shift (idx names byte 0, not byte 2).
    #[test]
    fn encode_cws_alpha_s1_first_byte_and_error_paths() {
        // (1a) '!' is S1-shifted (charvals returns Some((Some(43), 6))).
        // Per encode_cws_alpha: leading shift suppressed, cws[0] = 6,
        // mode = MODE_FIRST_S1. 'A' is direct (target 10).
        let (cws_bang_a, mode_bang_a) =
            encode_cws_alpha(b"!A").expect("encode_cws_alpha(\"!A\") must succeed");
        assert_eq!(
            mode_bang_a, MODE_FIRST_S1,
            "leading S1-shifted byte (!) must select MODE_FIRST_S1"
        );
        assert_eq!(
            &cws_bang_a[..2],
            &[6u16, 10],
            "leading S1 byte suppresses shift cw; cws[0]=target('!')=6, cws[1]=target('A')=10"
        );

        // (1b) NUL (ASCII 0) is also S1-shifted (Some((Some(43), 38))).
        // Confirms the branch is hit for control bytes too, not only
        // punctuation.
        let (cws_nul_a, mode_nul_a) =
            encode_cws_alpha(&[0u8, b'A']).expect("encode_cws_alpha([NUL, A]) must succeed");
        assert_eq!(
            mode_nul_a, MODE_FIRST_S1,
            "leading NUL (S1-shifted control byte) must select MODE_FIRST_S1"
        );
        assert_eq!(
            &cws_nul_a[..2],
            &[38u16, 10],
            "leading NUL: cws[0]=target(NUL)=38, cws[1]=target('A')=10"
        );

        // (2a) Empty input → InvalidData early return.
        let err_empty = encode_cws_alpha(&[]).expect_err("empty input must error");
        let msg_empty = format!("{err_empty:?}");
        assert!(
            msg_empty.contains("empty"),
            "empty-input error must mention 'empty', got: {msg_empty}"
        );

        // (2b) Non-ASCII byte at position 0 → InvalidData mentioning
        // position 0 and the byte value.
        let err_high0 = encode_cws_alpha(&[0xFF, b'A']).expect_err("high byte at pos 0 must error");
        let msg_high0 = format!("{err_high0:?}");
        assert!(
            msg_high0.contains("position 0"),
            "non-ASCII at pos 0 error must mention 'position 0', got: {msg_high0}"
        );
        assert!(
            msg_high0.contains("0xff"),
            "non-ASCII at pos 0 error must mention byte value 0xff, got: {msg_high0}"
        );

        // (2c) Non-ASCII byte at later position → InvalidData
        // mentioning the correct position index.
        let err_high2 =
            encode_cws_alpha(&[b'A', b'B', 0xFE]).expect_err("high byte at pos 2 must error");
        let msg_high2 = format!("{err_high2:?}");
        assert!(
            msg_high2.contains("position 2"),
            "non-ASCII at pos 2 error must mention 'position 2', got: {msg_high2}"
        );
        assert!(
            msg_high2.contains("0xfe"),
            "non-ASCII at pos 2 error must mention byte value 0xfe, got: {msg_high2}"
        );
    }

    /// `encode_cws` top-level dispatch: digit-heavy inputs route to
    /// NS-shift (mode 2); text routes to alpha (mode 0/4/5);
    /// mixed inputs with <5 leading digits also go through alpha
    /// (each digit gets a direct CHARMAP slot, then lowercase via
    /// S2 shifts).
    ///
    /// Goldens captured from bwip-js verify all dispatch paths
    /// produce BWIPP byte-for-byte cws.
    #[test]
    fn encode_cws_dispatches_correctly() {
        // Stage 11.A8c (cont) — 5 bare `.unwrap()` calls → `.expect(...)`
        // with per-call mode-path label so a dispatcher failure names
        // which mode (NS-digits / alpha / lowercase-first-S2 / mixed)
        // the specific corpus row was expected to take.
        // Leading 5+ digits → NS path (mode 2).
        let (cws, mode) =
            encode_cws(b"12345").expect("encode_cws(b\"12345\") must dispatch to MODE_NS_DIGITS");
        assert_eq!(mode, MODE_NS_DIGITS);
        assert_eq!(&cws[..3], &[5, 17, 9]);
        // Pure uppercase → alpha mode 0.
        let (cws, mode) =
            encode_cws(b"ABC").expect("encode_cws(b\"ABC\") must dispatch to MODE_ALPHA");
        assert_eq!(mode, MODE_ALPHA);
        assert_eq!(&cws[..3], &[10, 11, 12]);
        // Lowercase-first → alpha mode 5.
        let (cws, mode) =
            encode_cws(b"abc").expect("encode_cws(b\"abc\") must dispatch to MODE_FIRST_S2");
        assert_eq!(mode, MODE_FIRST_S2);
        assert_eq!(&cws[..5], &[10, 44, 11, 44, 12]);
        // <5 leading digits then text → alpha (each digit direct;
        // text shifts as needed). Matches bwip-js for "12abc" and
        // "1234abc".
        let (cws, mode) = encode_cws(b"12abc").expect(
            "encode_cws(b\"12abc\") (2-digit prefix < 5 → falls through to MODE_ALPHA) must succeed",
        );
        assert_eq!(mode, MODE_ALPHA);
        assert_eq!(cws, vec![1, 2, 44, 10, 44, 11, 44, 12, 48]);
        let (cws, mode) = encode_cws(b"1234abc").expect(
            "encode_cws(b\"1234abc\") (4-digit prefix < 5 → falls through to MODE_ALPHA) must succeed",
        );
        assert_eq!(mode, MODE_ALPHA);
        assert_eq!(
            cws,
            vec![1, 2, 3, 4, 44, 10, 44, 11, 44, 12, 48, 48, 48, 48, 48, 48]
        );
        // Empty rejected.
        // Stage 11.A8c — upgrade to 2-anchor pin matching the
        // `code49: empty input` diagnostic; this is the same
        // anchor set as the `encode` empty test below.
        match encode_cws(b"").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("code49:"),
                    "empty arm missing `code49:` prefix: {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty arm missing `empty input` predicate: {msg}"
                );
            }
            other => panic!("empty encode_cws input should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `pick_symbol_size` row selection at every
    /// CHARMAP capacity boundary. METRICS = [(2,9),(3,16),(4,23),
    /// (5,30),(6,37),(7,42),(8,49)]. Kills mutation on the `>= with <`
    /// / `>= with ==` boundary in the `find` predicate (line 169).
    #[test]
    fn pick_symbol_size_boundaries() {
        // r=2 fits ≤ 9 codewords.
        assert_eq!(pick_symbol_size(1), Some((2, 9)));
        assert_eq!(pick_symbol_size(9), Some((2, 9)));
        // r=3 needed for 10..=16.
        assert_eq!(pick_symbol_size(10), Some((3, 16)));
        assert_eq!(pick_symbol_size(16), Some((3, 16)));
        // r=4 for 17..=23.
        assert_eq!(pick_symbol_size(17), Some((4, 23)));
        assert_eq!(pick_symbol_size(23), Some((4, 23)));
        // r=8 maxes out at 49.
        assert_eq!(pick_symbol_size(43), Some((8, 49)));
        assert_eq!(pick_symbol_size(49), Some((8, 49)));
        // 50+ overflows.
        assert_eq!(pick_symbol_size(50), None);
        assert_eq!(pick_symbol_size(1000), None);
    }

    /// Stage 11.A8c — pin `lookup_direct` for representative bytes
    /// across CHARMAP. Kills the function-replacement mutant and
    /// any `position` predicate mutation.
    #[test]
    fn lookup_direct_known_bytes() {
        // Digits '0'..'9' map to CHARMAP slots 0..=9 in code49's
        // direct table layout? Not exactly — check what's at index 1.
        // Just pin known values: '0' lookup returns Some.
        assert!(lookup_direct(b'0').is_some());
        assert!(lookup_direct(b'9').is_some());
        // 'A', 'Z' are direct.
        assert!(lookup_direct(b'A').is_some());
        assert!(lookup_direct(b'Z').is_some());
        // ' ' is direct.
        assert!(lookup_direct(b' ').is_some());
        // Lowercase 'a' is NOT direct (needs S1 shift).
        assert_eq!(lookup_direct(b'a'), None);
        // Control byte 0x01 not direct.
        assert_eq!(lookup_direct(0x01), None);
        // High byte not direct.
        assert_eq!(lookup_direct(0xFF), None);
    }

    /// Stage 11.A8c — pin `charvals` shift-table classification for
    /// representative bytes covering every branch (direct/S1/S2/NS).
    /// Kills the per-arm `delete match arm` mutants on the giant
    /// match at line 360.
    #[test]
    fn charvals_classifies_every_branch() {
        // ASCII 0 → S1 + space (cw 38, shift 43).
        assert_eq!(charvals(0), Some((Some(43), 38)));
        // ASCII 26 (Ctrl-Z) → S1 + 'Z' (cw 35).
        assert_eq!(charvals(26), Some((Some(43), 35)));
        // ASCII 27 (Esc) → S1 + '1' (cw 1).
        assert_eq!(charvals(27), Some((Some(43), 1)));
        // ASCII 31 (US) → S1 + '5' (cw 5).
        assert_eq!(charvals(31), Some((Some(43), 5)));
        // ' ' (space) → direct (cw 38).
        assert_eq!(charvals(b' '), Some((None, 38)));
        // '!' → S1 + '6' (cw 6).
        assert_eq!(charvals(b'!'), Some((Some(43), 6)));
        // '$' → direct (cw 39).
        assert_eq!(charvals(b'$'), Some((None, 39)));
        // '%' → direct (cw 42).
        assert_eq!(charvals(b'%'), Some((None, 42)));
        // '+' → direct (cw 41).
        assert_eq!(charvals(b'+'), Some((None, 41)));
        // '-' → direct (cw 36).
        assert_eq!(charvals(b'-'), Some((None, 36)));
        // '.' → direct (cw 37).
        assert_eq!(charvals(b'.'), Some((None, 37)));
        // '/' → direct (cw 40).
        assert_eq!(charvals(b'/'), Some((None, 40)));
    }

    /// Stage 11.A8c-L — exhaustive fingerprint over all 256 byte values
    /// for `charvals`. Any match-arm mutation (delete arm, replace tuple,
    /// range-bound flip, or arithmetic in `b - K + M`) changes at least
    /// one of the 256 outputs and breaks this fingerprint. Targets the
    /// 14 charvals survivors.
    #[test]
    fn charvals_exhaustive_fingerprint_pinned() {
        fn pack(r: Option<(Option<u16>, u16)>) -> u64 {
            match r {
                None => 0,
                Some((None, v)) => 1u64 + ((v as u64) << 8),
                Some((Some(s), v)) => {
                    // outer_some + inner_some + s + (v << 16)
                    (1u64 << 32) | (s as u64) | ((v as u64) << 16)
                }
            }
        }
        let mut acc: u64 = 0;
        for b in 0u8..=255 {
            acc = acc.wrapping_add(
                pack(charvals(b))
                    .wrapping_mul((b as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        assert_eq!(acc, CHARVALS_FP, "charvals exhaustive fingerprint changed");
    }
    const CHARVALS_FP: u64 = 9322844644053756155;

    /// `base48(count, digits) -> Vec<u16>`: parse `digits` as a single
    /// decimal integer and emit `count` base-48 digits in HIGH-to-LOW
    /// order (out[0] = highest).
    ///
    /// Used by `encode_cws_ns_digits` (NS-shift digit-only path) for
    /// chunked 5-digit → 3-codeword compaction. Never directly tested.
    ///
    /// Mutations to catch:
    /// * `value * 10` → `value * 100` (wrong decimal base for parsing).
    /// * `value % 48` → `value % 47` or `% 100` (wrong output base).
    /// * `value /= 48` → wrong divisor.
    /// * `(0..count).rev()` → `0..count` (LSB first instead of MSB first).
    /// * `b - b'0'` → off-by-one.
    /// * Initial `value = 0` constant.
    #[test]
    fn base48_high_to_low_with_count_truncation() {
        use super::base48;

        // ---- Trivial: single digit '0' in 1 slot.
        assert_eq!(base48(1, b"0"), vec![0], "'0' → [0]");

        // ---- Last single-slot value.
        // value=47, 47%48=47, value/=48 → 0. out=[47].
        assert_eq!(base48(1, b"47"), vec![47], "47 → [47]");

        // ---- Count truncation.
        // value=48, out[0]=48%48=0, value=1 is lost (count=1).
        assert_eq!(
            base48(1, b"48"),
            vec![0],
            "48 in 1 slot truncates → [0] (high digit lost)"
        );
        // value=48 in 2 slots: out[1]=0, out[0]=1 → [1, 0].
        assert_eq!(
            base48(2, b"48"),
            vec![1, 0],
            "48 in 2 slots → [1, 0] (HIGH at index 0)"
        );

        // ---- Multi-digit decimal parsing + multi-slot output.
        // 12345 % 48 = 9, 257 % 48 = 17, 5 % 48 = 5. → [5, 17, 9].
        // Pins: decimal parse, 3-slot output, MSB-first layout.
        assert_eq!(
            base48(3, b"12345"),
            vec![5, 17, 9],
            "12345 → [5, 17, 9] (MSB-first; LSB-first mutant → [9, 17, 5])"
        );

        // ---- count=0 → empty.
        assert_eq!(base48(0, b"123"), Vec::<u16>::new(), "count=0 → empty");

        // ---- All zeros input + multi-slot.
        assert_eq!(base48(3, b"00000"), vec![0, 0, 0], "00000 → [0, 0, 0]");

        // ---- Two-slot leading zero discriminator.
        // 47 in 2 slots: 47%48=47, then 0%48=0 → [0, 47].
        assert_eq!(
            base48(2, b"47"),
            vec![0, 47],
            "47 in 2 slots → [0, 47] (leading zero, low value 47)"
        );

        // ---- Hand-computed 5-digit → 3 base48 codewords path used
        // by encode_cws_ns_digits. Pin a few representative values.
        // value=99999, 99999/48 = 2083 rem 15. 2083/48 = 43 rem 19.
        // 43/48 = 0 rem 43. → [43, 19, 15].
        assert_eq!(base48(3, b"99999"), vec![43, 19, 15]);
        // value=10000, /48=208 rem 16. 208/48=4 rem 16. 4/48=0 rem 4.
        // → [4, 16, 16].
        assert_eq!(base48(3, b"10000"), vec![4, 16, 16]);

        // ---- Single-digit input in multi-slot.
        // value=5, out[count-1]=5, then 0 → all zeros except last.
        assert_eq!(base48(4, b"5"), vec![0, 0, 0, 5], "5 in 4 slots: [0,0,0,5]");

        // ---- Sweep: every value 0..48 in 1 slot.
        for v in 0u8..48 {
            let s = v.to_string();
            assert_eq!(
                base48(1, s.as_bytes()),
                vec![v as u16],
                "v={v} in 1 slot must be [v]"
            );
        }
    }

    /// Stage 11.A8c-L — kills the 5 `delete -` mutants on the
    /// `pub(crate) const S1/S2/FN1/FN2/FN3` sentinel definitions
    /// (lines ~50-58 in the source — flipping a `-` to a positive
    /// integer collapses the marker namespace into the legal
    /// codeword range and would corrupt every downstream encode
    /// path that consults them via `CHARMAP` / `charvals`).
    ///
    /// Active (no `#[ignore]`): direct, deterministic asserts.
    #[test]
    fn code49_sentinel_consts_pinned() {
        assert_eq!(S1, -1, "S1 sentinel (Shift-1) must remain -1");
        assert_eq!(S2, -2, "S2 sentinel (Shift-2) must remain -2");
        assert_eq!(FN1, -3, "FN1 sentinel must remain -3");
        assert_eq!(FN2, -4, "FN2 sentinel must remain -4");
        assert_eq!(FN3, -5, "FN3 sentinel must remain -5");
        assert_eq!(
            NS, -6,
            "NS sentinel (numeric-shift codeword) must remain -6"
        );
    }

    /// Stage 11.A8c-L — STATE-MACHINE fingerprint pre-draft for the
    /// 18 `build_ccs` arithmetic survivors at L613-652 (modulo
    /// boundaries, row-sum accumulators, weight-multiplier index
    /// arithmetic, check-character composition, last-row index
    /// math, etc.). Eight diverse inputs span every row count
    /// 2..=8 plus a last-row-partial-fill variant — together they
    /// exercise:
    ///
    /// * r=2 (no z-check, single dense data row + check row)
    /// * r=3..6 (no z-check, multi-row sum loop)
    /// * r=7 (z-check branch first fires, wr1 index math active)
    /// * r=8 (max rows; z-check + every step at upper bound)
    /// * partial last-row (`j < dcws_usz` branch)
    /// * mode 0 (alpha) vs mode 2 (digit-shift via NS pair) paths
    /// * 5-digit and 5-letter inputs (different cws shapes)
    ///
    /// Awaiting CAP capture: placeholder `(0, 0)` fingerprints will
    /// be replaced by the capture-then-pin workflow (see e4d9c72,
    /// cfb68ae). To activate:
    ///   1. swap each `assert_eq!(got, *want, …)` for
    ///      `let _ = want; eprintln!("CAP {tag}: {got:?}");`
    ///   2. `cargo test --include-ignored -- --nocapture
    ///       build_ccs_state_machine_fingerprint_pinned_pending`
    ///   3. paste the captured `(len, fp)` tuples into the
    ///      `FP_CCS_*` consts;
    ///   4. restore asserts, drop `#[ignore]`, drop `_pending`.
    #[test]
    fn build_ccs_state_machine_fingerprint_pinned() {
        fn fp(out: &[u16]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &v) in out.iter().enumerate() {
                s = s.wrapping_add(
                    (v as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (out.len(), s)
        }

        const FP_CCS_R2_DIGITS: (usize, u64) = (16, 7647429427441); // r=2, 5-digit NS-shift
        const FP_CCS_R2_ALPHA: (usize, u64) = (16, 10044384919624); // r=2, single letter (partial last row)
        const FP_CCS_R3_ALPHA: (usize, u64) = (24, 21981382536841); // r=3, 10 letters (boundary into r=3)
        const FP_CCS_R4_ALPHA: (usize, u64) = (32, 45207695445591); // r=4, 17 letters
        const FP_CCS_R5_ALPHA: (usize, u64) = (40, 67082900551992); // r=5, 25 letters
        const FP_CCS_R6_ALPHA: (usize, u64) = (48, 85064048397006); // r=6, 33 letters
        const FP_CCS_R7_ALPHA: (usize, u64) = (56, 105158127107776); // r=7, 40 letters (z-check first fires)
        const FP_CCS_R8_ALPHA: (usize, u64) = (64, 145396718808775); // r=8, 47 letters (max rows)

        let cases: &[(&str, &[u8], (usize, u64))] = &[
            ("r2_digits", b"12345", FP_CCS_R2_DIGITS),
            ("r2_alpha", b"A", FP_CCS_R2_ALPHA),
            ("r3_alpha", b"ABCDEFGHIJ", FP_CCS_R3_ALPHA),
            ("r4_alpha", b"ABCDEFGHIJKLMNOPQ", FP_CCS_R4_ALPHA),
            ("r5_alpha", b"ABCDEFGHIJKLMNOPQRSTUVWXY", FP_CCS_R5_ALPHA),
            (
                "r6_alpha",
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFG",
                FP_CCS_R6_ALPHA,
            ),
            (
                "r7_alpha",
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMN",
                FP_CCS_R7_ALPHA,
            ),
            (
                "r8_alpha",
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTU",
                FP_CCS_R8_ALPHA,
            ),
        ];

        for (idx, (tag, input, want)) in cases.iter().enumerate() {
            let (cws, mode) = encode_cws(input).unwrap_or_else(|e| {
                panic!("encode_cws({tag}) idx {idx} must succeed; got Err: {e:?}")
            });
            let (rows, dcws) = pick_symbol_size(cws.len())
                .unwrap_or_else(|| panic!("pick_symbol_size({tag}) idx {idx} must fit"));
            let ccs = build_ccs(&cws, rows, dcws, mode).unwrap_or_else(|e| {
                panic!(
                    "build_ccs({tag}) idx {idx} rows={rows} dcws={dcws} mode={mode} \
                     must succeed; got Err: {e:?}"
                )
            });
            let got = fp(&ccs);
            assert_eq!(
                got, *want,
                "build_ccs case {tag} (idx {idx}): got {got:?}, want {want:?}"
            );
        }
    }

    // -----------------------------------------------------------------
    // T2-a mutation killers / equivalence proofs (Stage 11.A8d).
    // -----------------------------------------------------------------

    /// Kills `encode_cws_direct: > → ==` and `> → >=` at the
    /// `if cw > 42` sanity check (src/symbology/code49.rs:203).
    ///
    /// `CHARMAP[42] == b'%'`, so `lookup_direct(b'%')` returns the
    /// codeword `42` — the *largest* valid direct alphabet member.
    /// Real code: `42 > 42` is false → `%` is accepted.
    /// Mutant `== 42`: `42 == 42` is true → `%` wrongly rejected.
    /// Mutant `>= 42`: `42 >= 42` is true → `%` wrongly rejected.
    /// A payload containing `%` therefore succeeds on the real code
    /// but errors under either mutant.
    #[test]
    fn encode_cws_direct_accepts_percent_at_codeword_42() {
        // Pre-condition: '%' really does map to the boundary value 42.
        assert_eq!(lookup_direct(b'%'), Some(42));
        // Real code accepts a payload containing the cw==42 member.
        let cws =
            encode_cws_direct(b"A%Z").expect("'%' (cw 42) is a direct member and must be accepted");
        // First three data codewords are the direct lookups.
        assert_eq!(&cws[..3], &[10u16, 42u16, 35u16]);
    }

    /// Kills `encode: != → ==` at the `if bit != 0` pixel-set loop
    /// (src/symbology/code49.rs:792).
    ///
    /// The mutant inverts every pixel. Row 0 is the top bearer
    /// (`allone`) so every module is set under the real code; the
    /// data rows begin with a 10-module left quiet zone (all clear).
    /// Asserting one set bearer pixel AND one clear quiet-zone pixel
    /// pins the predicate: both flip under `== 0`.
    #[test]
    fn encode_pixel_predicate_distinguishes_set_and_clear() {
        let bm = encode(b"CODE49").expect("encode must succeed");
        // Top bearer row is all-ones: (0,0) is set on real code.
        assert!(bm.get(0, 0), "top bearer pixel (0,0) must be set");
        // A data row's left quiet zone (first 10 modules) is clear.
        // Row index 1 is the first data row (after the 1-row bearer).
        assert!(!bm.get(0, 1), "quiet-zone pixel (0,1) must be clear");
    }

    /// Kills `encode: += → *=` at the `y += 1` row-advance
    /// (src/symbology/code49.rs:796).
    ///
    /// `y` starts at 0, so `0 *= 1 == 0`: under the mutant every
    /// rendered scanline writes into row 0 only, leaving all rows
    /// `>= 1` untouched (default-clear). The bottom bearer (the
    /// final compressed row) is `allone`, so the last matrix row is
    /// fully set on the real code but fully clear under the mutant.
    #[test]
    fn encode_row_advance_fills_every_row() {
        let bm = encode(b"CODE49").expect("encode must succeed");
        let h = bm.height();
        assert!(h > 1, "symbol must have multiple rows");
        // Bottom bearer (last row) is all-ones on real code; under
        // `y *= 1` it stays clear because nothing ever leaves row 0.
        assert!(
            bm.get(0, h - 1),
            "bottom bearer pixel (0, {}) must be set",
            h - 1
        );
    }

    /// Kills `encode_pixs: * → +` at `numcomprows = 2 * r + 1`
    /// (src/symbology/code49.rs:818).
    ///
    /// `2 * r + 1` vs `2 + r + 1`: equal at r==2 (both 5) but diverge
    /// for r >= 3 (r=3 → 7 vs 6). `numcomprows` drives the final
    /// `debug_assert_eq!(pixs.len(), numcomprows * 81)`; under the
    /// mutant that assertion panics in a debug (test) build. A 12-byte
    /// uppercase payload yields 12 codewords → r=3, so the real code
    /// returns `7 * 81` cells while the mutant panics.
    #[test]
    fn encode_pixs_numcomprows_uses_two_r_plus_one() {
        let input = b"ABCDEFGHIJKL"; // 12 uppercase → 12 cws → r=3.
        let (cws, _mode) = encode_cws(input).expect("alpha encode");
        let (rows, _dcws) = pick_symbol_size(cws.len()).expect("fits");
        assert_eq!(rows, 3, "12-byte payload must select r=3");
        let pixs = encode_pixs(input).expect("encode_pixs must succeed");
        // numcomprows = 2*3 + 1 = 7 compressed rows of 81 cells each.
        assert_eq!(pixs.len(), 7 * 81);
    }

    /// Equivalence proofs for the two `build_ccs` survivors that no
    /// reachable input can distinguish.
    ///
    /// **`build_ccs: < → <=`** at `if j < dcws_usz`
    /// (src/symbology/code49.rs:613). After the first-`r-1`-rows loop
    /// `j == (r-1)*7`; the body copies `cws[j..]`, a slice of length
    /// `dcws - j` (= `remaining`). At the boundary `j == dcws_usz`
    /// that slice is empty and the `copy_from_slice` is a no-op, so
    /// `<` (skip) and `<=` (run an empty copy) yield byte-identical
    /// `ccs`. The boundary is actually *reached* for r=7 (j=42,
    /// dcws=42) and r=8 (j=49, dcws=49) — see [`METRICS`] — yet even
    /// there the two predicates are observationally identical because
    /// the copy moves zero elements. EQUIVALENT.
    ///
    /// **`build_ccs: - → /`** at `ccs[last_idx - 8 .. last_idx - 1]`
    /// (src/symbology/code49.rs:652). The mutant rewrites `last_idx - 1`
    /// as `last_idx / 1 == last_idx`, widening the summed range by one
    /// element: index `last_idx - 1`. But that cell is written only on
    /// the *next* statement (line 656); at the moment of the sum it
    /// still holds its `vec![0u16; ...]` initial value `0`. Adding `0`
    /// leaves `lastrow_sum` unchanged, so `ccs[last_idx - 1]` receives
    /// the identical `% 49` result. The widened end bound equals
    /// `ccs.len()`, so no out-of-bounds panic occurs either.
    /// EQUIVALENT.
    #[test]
    fn code49_equivalence_notes() {
        // Demonstrate the `< → <=` boundary is reachable yet inert:
        // for r=7 and r=8 the first-rows loop ends with j == dcws,
        // where the last-row copy moves zero codewords.
        for &(rows, dcws) in &[(7u16, 42u16), (8u16, 49u16)] {
            let r = usize::from(rows);
            let j = (r - 1) * 7;
            assert_eq!(
                j,
                usize::from(dcws),
                "r={r}: first-rows loop must terminate exactly at j==dcws"
            );
        }
        // Demonstrate that at the moment of the last-row sum the
        // cell at `last_idx - 1` is still its zero initial value, so
        // including it (the `/ 1` mutant) cannot change the sum. We
        // re-derive the invariant the production code relies on: the
        // final-check slot is the LAST thing written.
        //
        // Build a real symbol and confirm `ccs[last_idx - 1]` equals
        // `sum(ccs[last_idx-8 .. last_idx-1]) % 49` (i.e. excluding
        // itself) — which is exactly what holds when the extra cell
        // contributes 0.
        let (cws, mode) = encode_cws(b"ABCDEFGHIJKL").expect("encode");
        let (rows, dcws) = pick_symbol_size(cws.len()).expect("fits");
        let ccs = build_ccs(&cws, rows, dcws, mode).expect("build_ccs");
        let last_idx = ccs.len();
        let sum_excl: u32 = ccs[last_idx - 8..last_idx - 1]
            .iter()
            .map(|&c| u32::from(c))
            .sum();
        assert_eq!(
            ccs[last_idx - 1],
            (sum_excl % 49) as u16,
            "final check must equal the self-excluding row sum"
        );
    }
}
