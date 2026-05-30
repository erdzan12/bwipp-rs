//! GS1 DataBar Expanded.
//!
//! Pure-Rust port of BWIPP's `bwipp_databarexpanded` (bwip-js line
//! 13344 onwards). [`encode`] turns a parenthesised GS1 element
//! string into the run-length [`LinearPattern`] the standard
//! renderers consume.
//!
//! Methods supported (all 7 BWIPP dispatch paths):
//! - **1** — single (01)+GTIN-14 (with or without trailing AIs;
//!   trailing AIs flow through the general-purpose encoder)
//! - **0100** — (01)+(3103) with GTIN starting "9" and weight ≤ 32767
//! - **0101** — (01)+(3202)/(3203) with the same GTIN constraint
//! - **0111xxx** — (01)+(310x|320x) optionally with (11|13|15|17)
//! - **01100** — (01)+(392x) (price single currency) with GTIN
//!   starting "9"
//! - **01101** — (01)+(393x) (price with 3-digit ISO currency code)
//!   with GTIN starting "9"
//! - **00** — general-purpose for any GS1 AI combination
//!
//! All static tables (TAB_174 / FINDER_WIDTHS / FINDER_SEQ /
//! CHECK_WEIGHTS / FILL_PAT / SEP_PAD / charset sentinels) are
//! verified byte-for-byte against BWIPP via
//! `tools/oracle-databarexpanded-tables.js`. The full pipeline
//! (binval / dxw / fxw / checksum / sbs) is verified end-to-end
//! against `tools/oracle-databarexpanded.js` across 12+ diverse
//! inputs covering all 7 method dispatchers plus their trailing-AI
//! gpf variants.
//!
//! [`LinearPattern`]: crate::encoding::LinearPattern

// FILL_PAT / SEP_PAD and the LATCH_* charset sentinels are referenced
// only by the (still-pending) stacked variant + general-purpose
// encoder respectively. Suppress the dead-code lint until those land
// so the deny-warnings clippy build stays green.
#![allow(dead_code)]

/// `tab174` — the symbol-character lookup table, BWIPP source line
/// `dotcode_tab174` (5 rows × 8 fields).
///
/// Two passes through the table use different field 0 thresholds:
///
/// 1. **Data-value scan** (driven by `$_.j`): treat field 0 as
///    `maxDataValue`. The remaining 7 fields are
///    `[minDataValue, t, n, mw, el, gsum, parity]`.
/// 2. **Checksum scan** (driven by `$_.i`): treat field 0 as
///    `maxChecksum`. Inner fields differ — refer to
///    `bwipp_databarexpanded` lines 13593 onwards.
///
/// Indexing convention: pre-multiply `row * 8` to land on field 0
/// of the row, then read field `k` at `row * 8 + k`.
pub(crate) const TAB_174: [u32; 40] = [
    347, 0, 12, 5, 7, 2, 87, 4, 1387, 348, 10, 7, 5, 4, 52, 20, 2947, 1388, 8, 9, 4, 5, 30, 52,
    3987, 2948, 6, 11, 3, 6, 10, 104, 4191, 3988, 4, 13, 1, 8, 1, 204,
];

/// `finderwidths` — twelve finder patterns × five module widths each.
///
/// BWIPP indexes by `finderId * 5`, so [`FINDER_SEQ`] entries are
/// pre-multiplied: `$_.fxw[x] = $geti(FINDER_WIDTHS, seq[x] * 5, 5)`.
pub(crate) const FINDER_WIDTHS: [u8; 60] = [
    1, 8, 4, 1, 1, // F00
    1, 1, 4, 8, 1, // F01
    3, 6, 4, 1, 1, // F02
    1, 1, 4, 6, 3, // F03
    3, 4, 6, 1, 1, // F04
    1, 1, 6, 4, 3, // F05
    3, 2, 8, 1, 1, // F06
    1, 1, 8, 2, 3, // F07
    2, 6, 5, 1, 1, // F08
    1, 1, 5, 6, 2, // F09
    2, 2, 9, 1, 1, // F10
    1, 1, 9, 2, 2, // F11
];

/// `finderseq` — finder-id sequences per data-segment count.
///
/// Index by `(datalen - 2) / 2`. The returned slice's length is
/// `ceil(datalen / 2)`, matching the number of finder pairs in the
/// symbol. Entries beyond the slice's length are unused.
///
/// The variable-length nature is why we use `&[&[u8]]` rather than
/// a fixed 2D array.
pub(crate) const FINDER_SEQ: [&[u8]; 10] = [
    &[0, 1],                              // datalen = 2..=3
    &[0, 3, 2],                           // datalen = 4..=5
    &[0, 5, 2, 7],                        // datalen = 6..=7
    &[0, 9, 2, 7, 4],                     // datalen = 8..=9
    &[0, 9, 2, 7, 6, 11],                 // datalen = 10..=11
    &[0, 9, 2, 7, 8, 11, 10],             // datalen = 12..=13
    &[0, 1, 2, 3, 4, 5, 6, 7],            // datalen = 14..=15
    &[0, 1, 2, 3, 4, 5, 6, 9, 8],         // datalen = 16..=17
    &[0, 1, 2, 3, 4, 5, 6, 9, 10, 11],    // datalen = 18..=19
    &[0, 1, 2, 3, 4, 7, 6, 9, 8, 11, 10], // datalen = 20..=21
];

/// `checkweights` — Reed-Solomon-style mod-211 checksum weights.
///
/// BWIPP loads `$geti(CHECK_WEIGHTS, dataIdx * 16, 16)` for each
/// data symbol character (after the initial 8-entry `-1` sentinel
/// row that represents the placeholder slot).
///
/// `i8` rather than `u8`: the first 8 entries are `-1` flags.
pub(crate) const CHECK_WEIGHTS: [i16; 192] = [
    -1, -1, -1, -1, -1, -1, -1, -1, 77, 96, 32, 81, 27, 9, 3, 1, 20, 60, 180, 118, 143, 7, 21, 63,
    205, 209, 140, 117, 39, 13, 145, 189, 193, 157, 49, 147, 19, 57, 171, 91, 132, 44, 85, 169,
    197, 136, 186, 62, 185, 133, 188, 142, 4, 12, 36, 108, 50, 87, 29, 80, 97, 173, 128, 113, 150,
    28, 84, 41, 123, 158, 52, 156, 166, 196, 206, 139, 187, 203, 138, 46, 76, 17, 51, 153, 37, 111,
    122, 155, 146, 119, 110, 107, 106, 176, 129, 43, 16, 48, 144, 10, 30, 90, 59, 177, 164, 125,
    112, 178, 200, 137, 116, 109, 70, 210, 208, 202, 184, 130, 179, 115, 190, 204, 68, 93, 31, 151,
    191, 134, 148, 22, 66, 198, 172, 94, 71, 2, 40, 154, 192, 64, 162, 54, 18, 6, 120, 149, 25, 75,
    14, 42, 126, 167, 175, 199, 207, 69, 23, 78, 26, 79, 103, 98, 83, 38, 114, 131, 182, 124, 159,
    53, 88, 170, 127, 183, 61, 161, 55, 165, 73, 8, 24, 72, 5, 15, 89, 100, 174, 58, 160, 194, 135,
    45,
];

/// `fillpat` — 5-module separator pattern for the row separators
/// in the stacked variant.
pub(crate) const FILL_PAT: [u8; 5] = [0, 0, 1, 0, 0];

/// `seppad` — 4-module padding placed at each end of the separator
/// rows in the stacked variant.
pub(crate) const SEP_PAD: [u8; 4] = [0, 0, 0, 0];

/// FNC1 sentinel as encoded inside the alphanumeric / iso646 charset
/// (BWIPP `databarexpanded_fnc1 = -1`).
pub(crate) const FNC1: i32 = -1;

/// Latch-to-numeric charset sentinel (BWIPP `databarexpanded_lnumeric = -2`).
pub(crate) const LATCH_NUMERIC: i32 = -2;

/// Latch-to-alphanumeric charset sentinel (BWIPP
/// `databarexpanded_lalphanumeric = -3`).
pub(crate) const LATCH_ALPHANUMERIC: i32 = -3;

/// Latch-to-ISO-646 charset sentinel (BWIPP `databarexpanded_liso646 = -4`).
pub(crate) const LATCH_ISO646: i32 = -4;

/// Format an integer `n` as a `width`-character binary string, MSB
/// first, zero-padded on the left. Port of BWIPP's `$_.tobin`
/// (bwip-js line 13811).
///
/// Returns an [`Error::InvalidData`] if `n` doesn't fit in `width`
/// bits — BWIPP's stack-machine equivalent silently truncates, but
/// surfacing the overflow is safer for a hand-callable Rust API.
pub(crate) fn to_bin(n: u64, width: usize) -> Result<String, crate::error::Error> {
    if width < 64 && n >= (1u64 << width) {
        return Err(crate::error::Error::InvalidData(format!(
            "DataBar Expanded: value {n} doesn't fit in {width} bits",
        )));
    }
    let mut s = String::with_capacity(width);
    for k in (0..width).rev() {
        s.push(if (n >> k) & 1 == 1 { '1' } else { '0' });
    }
    Ok(s)
}

/// Convert a 12-digit ASCII string to a 40-bit binary string. Port of
/// BWIPP's `$_.conv12to40` (bwip-js line 13785).
///
/// Splits the 12 digits into 4 groups of 3, converts each group to a
/// 10-bit binary string, and concatenates the four. Result is always
/// exactly 40 characters of '0'/'1'.
///
/// Returns [`Error::InvalidData`] if the input isn't exactly 12
/// ASCII digits.
pub(crate) fn conv12to40(digits: &[u8]) -> Result<String, crate::error::Error> {
    if digits.len() != 12 || !digits.iter().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "DataBar Expanded: conv12to40 needs exactly 12 ASCII digits, got {:?}",
            std::str::from_utf8(digits).unwrap_or("<non-utf8>"),
        )));
    }
    let mut out = String::with_capacity(40);
    for group in 0..4 {
        let chunk = &digits[group * 3..group * 3 + 3];
        // SAFETY: validated as ASCII digits above; `from_utf8`/parse
        // can't fail.
        let n: u64 = std::str::from_utf8(chunk).unwrap().parse().unwrap();
        out.push_str(&to_bin(n, 10)?);
    }
    Ok(out)
}

/// Convert a 13-digit ASCII string to a 44-bit binary string. Port
/// of BWIPP's `$_.conv13to44` (bwip-js line 13798).
///
/// The leading digit (value 0..=9) becomes 4 bits via [`to_bin`];
/// the remaining 12 digits feed [`conv12to40`].
///
/// Returns [`Error::InvalidData`] if the input isn't exactly 13
/// ASCII digits.
pub(crate) fn conv13to44(digits: &[u8]) -> Result<String, crate::error::Error> {
    if digits.len() != 13 || !digits.iter().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "DataBar Expanded: conv13to44 needs exactly 13 ASCII digits, got {:?}",
            std::str::from_utf8(digits).unwrap_or("<non-utf8>"),
        )));
    }
    let head: u64 = u64::from(digits[0] - b'0');
    let mut out = to_bin(head, 4)?;
    out.push_str(&conv12to40(&digits[1..])?);
    Ok(out)
}

/// Decoded row from [`TAB_174`]: the seven parameters BWIPP loads
/// after locating the row whose threshold `≥ value`. Named after the
/// `dxx` / `cxx` variables BWIPP uses inside the data-segment and
/// checksum scans respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tab174Row {
    /// Group sum — the cumulative value of all rows above this one.
    pub gs: u32,
    /// Odd-side element module total.
    pub elo: u32,
    /// Even-side element module total.
    pub ele: u32,
    /// Max width per odd-side element.
    pub mwo: u32,
    /// Max width per even-side element.
    pub mwe: u32,
    /// Number of (val − gs) divisions taken by `te` before exhausting
    /// the odd-side enumeration. Unused by the current encoder paths
    /// but kept for completeness.
    pub to: u32,
    /// Number of even-side combinations per odd-side combination —
    /// `(val − gs) / te` picks the odd combo and `(val − gs) % te`
    /// picks the even combo.
    pub te: u32,
}

/// Locate the [`Tab174Row`] applicable to `value` (a 12-bit data
/// codeword or checksum). Returns the row whose field 0 (the max
/// threshold) is the smallest value `≥ value`. Panics if `value`
/// exceeds the largest threshold (4191) — which can't happen for a
/// well-formed 12-bit input.
pub(crate) fn tab174_row_for(value: u32) -> Tab174Row {
    let mut j = 0;
    while j < TAB_174.len() {
        if value <= TAB_174[j] {
            return Tab174Row {
                gs: TAB_174[j + 1],
                elo: TAB_174[j + 2],
                ele: TAB_174[j + 3],
                mwo: TAB_174[j + 4],
                mwe: TAB_174[j + 5],
                to: TAB_174[j + 6],
                te: TAB_174[j + 7],
            };
        }
        j += 8;
    }
    panic!("DataBar Expanded: value {value} exceeds TAB_174's max threshold (4191)");
}

/// Extract the 8-element widths array for a data symbol character.
///
/// `d` is the 12-bit data codeword (0..=4191). `segment_index` is
/// the position of this character in the data section of the binval
/// — its parity controls the interleaving (BWIPP `dxw` orientation).
///
/// Even-index segments place `dwo` at indices 7,5,3,1 (descending)
/// and `dwe` at indices 6,4,2,0; odd-index segments use the natural
/// `[dwo,dwe,dwo,dwe,...]` order. This is what gives DataBar Expanded
/// its mirrored-pairs look across each finder.
pub(crate) fn extract_data_character(d: u32, segment_index: usize) -> [u8; 8] {
    let row = tab174_row_for(d);
    let v = d - row.gs;
    let odd_val = v / row.te;
    let even_val = v % row.te;
    let dwo = crate::symbology::databar::get_rss_widths(
        i64::from(odd_val),
        i64::from(row.elo),
        i64::from(row.mwo),
        4,
        true,
    );
    let dwe = crate::symbology::databar::get_rss_widths(
        i64::from(even_val),
        i64::from(row.ele),
        i64::from(row.mwe),
        4,
        false,
    );
    let mut out = [0u8; 8];
    if segment_index % 2 == 0 {
        for j in 0..4 {
            out[7 - j * 2] = dwo[j];
            out[6 - j * 2] = dwe[j];
        }
    } else {
        for j in 0..4 {
            out[j * 2] = dwo[j];
            out[j * 2 + 1] = dwe[j];
        }
    }
    out
}

/// Extract the 8-element widths array for the checksum character.
///
/// Same `tab174_row_for` lookup as for data, but the checksum slot
/// always uses the natural `[cwo[0], cwe[0], cwo[1], cwe[1], ...]`
/// interleave — no parity-based flip. The checksum character lives
/// at `dxw[0]`, ahead of all data characters.
pub(crate) fn extract_checksum_character(checksum: u32) -> [u8; 8] {
    let row = tab174_row_for(checksum);
    let v = checksum - row.gs;
    let odd_val = v / row.te;
    let even_val = v % row.te;
    let cwo = crate::symbology::databar::get_rss_widths(
        i64::from(odd_val),
        i64::from(row.elo),
        i64::from(row.mwo),
        4,
        true,
    );
    let cwe = crate::symbology::databar::get_rss_widths(
        i64::from(even_val),
        i64::from(row.ele),
        i64::from(row.mwe),
        4,
        false,
    );
    let mut out = [0u8; 8];
    for i in 0..4 {
        out[i * 2] = cwo[i];
        out[i * 2 + 1] = cwe[i];
    }
    out
}

/// Pack a 12-bit slice of `binval` (MSB first) into an integer
/// value. Helper for stepping through binval one symbol character
/// at a time. Panics if `slice.len() != 12` — caller is expected to
/// supply already-bounded slices.
pub(crate) fn pack_12_bits(slice: &[u8]) -> u32 {
    assert_eq!(slice.len(), 12, "pack_12_bits requires exactly 12 bits");
    let mut v = 0u32;
    for &bit in slice {
        v = (v << 1) | u32::from(bit);
    }
    v
}

/// BWIPP's `rembits`: how many zero-pad bits to append to bring the
/// data-codeword count to a valid symbol size.
///
/// `unpadded_bits` is BWIPP's `_Bj` — the pre-pad bit count *plus*
/// the implicit 12 bits the checksum will occupy. `segments` is the
/// user-controlled max characters per row (default 22 for the
/// non-stacked "expanded" format).
///
/// The rules, lifted from bwip-js line 14062:
///   1. Round `unpadded_bits` up to the next multiple of 12.
///   2. If the rounded value is below 48 (the minimum-symbol cap),
///      bump it to 48.
///   3. If the resulting codeword count is `≡ 1 (mod segments)`,
///      bump it by another 12. Symbols whose final row would hold
///      one stray character are forbidden.
///   4. Return the difference from the input.
pub(crate) fn rembits(unpadded_bits: usize, segments: usize) -> usize {
    let rounded = unpadded_bits.div_ceil(12) * 12;
    let mut target = rounded.max(48);
    let cw_count = target / 12;
    if cw_count % segments == 1 {
        target = (cw_count + 1) * 12;
    }
    target - unpadded_bits
}

/// Concatenate the linkage flag, method bits, vlf bits, cdf bits,
/// gpf bits, and pad bits into BWIPP's `binval` bit array.
///
/// `cdf`, `gpf` are expected to be slices of 0/1 bytes. `method` is
/// the method-prefix as 0/1 bytes (e.g. `[1]` for the (01)+GTIN-14
/// fast path, `[0,1,0,0]` for `0100`, etc.). `linkage` is the
/// composite-flag bit BWIPP threads through.
///
/// `vlf_len` is 0 when `gpfallow` is false (no variable-length AI
/// can follow the compressed prefix); 2 otherwise.
///
/// `segments` is forwarded to [`rembits`].
///
/// Returns the assembled `binval` (data codewords only — the
/// checksum is prepended separately during the symbol-character
/// extraction stage).
pub(crate) fn assemble_binval(
    linkage: u8,
    method: &[u8],
    vlf_len: usize,
    cdf: &[u8],
    gpf: &[u8],
    final_mode: CharsetMode,
    segments: usize,
) -> Vec<u8> {
    debug_assert!(vlf_len == 0 || vlf_len == 2);
    debug_assert!(linkage <= 1);
    // _Bj — the bit count BWIPP feeds into rembits. The fixed `12`
    // accounts for the checksum character that gets prepended later
    // and so should be reserved when computing how many codewords
    // the final symbol holds.
    let bj = 1 + 12 + method.len() + vlf_len + cdf.len() + gpf.len();
    let pad_len = rembits(bj, segments);
    let total_chars = (bj + pad_len) / 12;
    let vlf_vals = if vlf_len == 2 {
        let v0 = (total_chars % 2) as u8;
        let v1 = if total_chars <= 14 { 0 } else { 1 };
        [v0, v1]
    } else {
        [0, 0]
    };
    let mut out = Vec::with_capacity(1 + method.len() + vlf_len + cdf.len() + gpf.len() + pad_len);
    out.push(linkage);
    out.extend_from_slice(method);
    if vlf_len == 2 {
        out.push(vlf_vals[0]);
        out.push(vlf_vals[1]);
    }
    out.extend_from_slice(cdf);
    out.extend_from_slice(gpf);
    // Pad with FILL_PAT repeating. When the encoder ended in numeric
    // mode, BWIPP prepends 4 zeros and truncates the tail — net
    // effect is the fill pattern is offset by 4 bits.
    if pad_len > 0 {
        let shift = match final_mode {
            CharsetMode::Numeric => 4,
            _ => 0,
        };
        for i in 0..pad_len {
            let bit = if i < shift {
                0
            } else {
                FILL_PAT[(i - shift) % 5]
            };
            out.push(bit);
        }
    }
    out
}

/// Output of one of the AI-specific method-dispatch helpers below.
/// Method 1 / 0100 / 0101 / 0111xxx / 01100 / 01101 / 00 each pack
/// the AI string into a fixed-or-variable cdf prefix, optionally
/// leave trailing AIs for the general-purpose encoder, and decide
/// whether the variable-length flag (vlf) is needed.
#[derive(Debug, Clone)]
pub(crate) struct MethodOutput {
    /// The method-prefix bits (e.g. `[1]` for method "1").
    pub method_bits: Vec<u8>,
    /// Length of the variable-length flag — 0 when this method
    /// doesn't allow trailing general-purpose AIs, 2 otherwise.
    pub vlf_len: usize,
    /// Compressed data-field bits.
    pub cdf: Vec<u8>,
    /// Initial general-purpose-field BYTES (not bits) — fed through
    /// the general-purpose encoder after the dispatcher returns to
    /// produce the actual gpf bits. The bytes use BWIPP's
    /// `'^'`-as-FNC1 convention.
    pub gpf_prefix_bytes: Vec<u8>,
    /// AIs the method consumed and shouldn't be re-encoded by the
    /// general encoder.
    pub consumed_ais: usize,
}

/// Shared validation for the "compressed 2-AI" methods (0100, 0101).
///
/// All of them require:
///   * exactly two AIs, the first being (01) with 14 ASCII digits,
///     starting with '9';
///   * the second AI matching `expected_second_ai`;
///   * the second AI's data being a 6-digit ASCII numeric value.
///
/// Returns `(gtin_first12, second_value)` on success, or `None`
/// when the precondition fails (so the caller falls through to the
/// next method).
fn parse_two_ai_compressed_inputs<'a>(
    elements: &'a [crate::util::gs1::Element],
    expected_second_ai: &str,
) -> Option<(&'a [u8], u64)> {
    if elements.len() != 2 || elements[0].ai != "01" || elements[1].ai != expected_second_ai {
        return None;
    }
    let v0 = &elements[0].data;
    let v1 = &elements[1].data;
    if v0.len() != 14
        || !v0.bytes().all(|b| b.is_ascii_digit())
        || v0.as_bytes()[0] != b'9'
        || v1.len() != 6
        || !v1.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let v1_int: u64 = v1.parse().ok()?;
    Some((&v0.as_bytes()[1..13], v1_int))
}

/// Method "0100" — (01) + (3103) where GTIN starts with '9' and the
/// (3103) weight value is ≤ 32767.
///
/// cdf layout (55 bits):
///   * bits 0..40: conv12to40 of GTIN digits 1..13 (skipping the
///     leading '9' and trailing check digit);
///   * bits 40..55: tobin(weight, 15).
///
/// `gpfallow` is false — no trailing variable AIs are permitted.
pub(crate) fn encode_method_0100(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    let Some((gtin12, v1)) = parse_two_ai_compressed_inputs(elements, "3103") else {
        return Ok(None);
    };
    if v1 > 32767 {
        return Ok(None);
    }
    let mut cdf_str = conv12to40(gtin12)?;
    cdf_str.push_str(&to_bin(v1, 15)?);
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    Ok(Some(MethodOutput {
        method_bits: vec![0, 1, 0, 0],
        vlf_len: 0,
        cdf,
        gpf_prefix_bytes: Vec::new(),
        consumed_ais: 2,
    }))
}

/// Method "0101" — two BWIPP precondition flavours collapsed into
/// one Rust helper:
///   * (01) + (3202) with weight ≤ 9999, encoded as tobin(weight, 15);
///   * (01) + (3203) with weight ≤ 22767, encoded as tobin(weight+10000, 15).
///
/// The +10000 offset is what lets the decoder tell 3202 from 3203
/// at decode time despite the shared method prefix.
///
/// `gpfallow` is false — no trailing variable AIs are permitted.
pub(crate) fn encode_method_0101(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    // Try 3202 first, then 3203.
    let (gtin12, value_for_tobin) =
        if let Some((g, v)) = parse_two_ai_compressed_inputs(elements, "3202") {
            if v > 9999 {
                return Ok(None);
            }
            (g, v)
        } else if let Some((g, v)) = parse_two_ai_compressed_inputs(elements, "3203") {
            if v > 22767 {
                return Ok(None);
            }
            (g, v + 10000)
        } else {
            return Ok(None);
        };
    let mut cdf_str = conv12to40(gtin12)?;
    cdf_str.push_str(&to_bin(value_for_tobin, 15)?);
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    Ok(Some(MethodOutput {
        method_bits: vec![0, 1, 0, 1],
        vlf_len: 0,
        cdf,
        gpf_prefix_bytes: Vec::new(),
        consumed_ais: 2,
    }))
}

/// Charset mode for BWIPP's general-purpose encoder.
///
/// The general-purpose encoder packs the GS1 element string into a
/// bit array by walking the input one (or two) bytes at a time. The
/// active mode determines which alphabet is in play; mode latches
/// emit a fixed-width prefix and switch the active alphabet for
/// subsequent characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharsetMode {
    Numeric,
    Alphanumeric,
    Iso646,
}

/// Sentinel byte BWIPP uses to mark FNC1 inside the general-purpose
/// byte stream — it replaces FNC1's normal `-1` representation with
/// `'^'` (94) so the value fits in a `u8` charset key.
pub(crate) const FNC1_SENTINEL_BYTE: u8 = b'^';

/// Look up the (bit value, bit width) for a 2-byte pair in numeric
/// mode. Each byte is either an ASCII digit `'0'..='9'` or the FNC1
/// sentinel `'^'`. Pairs are encoded as `(d0*11 + d1) + 8` in 7 bits,
/// where `d0`/`d1` are 0..=9 for digits and 10 for FNC1.
///
/// Returns `None` if either byte is outside the mode's alphabet.
pub(crate) fn numeric_pair_bits(c0: u8, c1: u8) -> Option<(u32, u8)> {
    fn digit(c: u8) -> Option<u32> {
        match c {
            b'0'..=b'9' => Some(u32::from(c - b'0')),
            FNC1_SENTINEL_BYTE => Some(10),
            _ => None,
        }
    }
    let g = digit(c0)? * 11 + digit(c1)?;
    Some((g + 8, 7))
}

/// Look up the (bit value, bit width) for a single byte in
/// alphanumeric mode. Covers digits, FNC1, uppercase letters, and
/// the eight punctuation symbols BWIPP includes.
pub(crate) fn alphanumeric_byte_bits(c: u8) -> Option<(u32, u8)> {
    match c {
        b'0'..=b'9' => Some((u32::from(c) - 43, 5)),
        FNC1_SENTINEL_BYTE => Some((15, 5)),
        b'A'..=b'Z' => Some((u32::from(c) - 33, 6)),
        b'*' => Some((58, 6)),
        b',' | b'-' | b'.' | b'/' => Some((u32::from(c) + 15, 6)),
        _ => None,
    }
}

/// Look up the (bit value, bit width) for a single byte in iso646
/// mode. Covers digits, FNC1, uppercase + lowercase letters, space,
/// and BWIPP's iso646 punctuation subset.
pub(crate) fn iso646_byte_bits(c: u8) -> Option<(u32, u8)> {
    match c {
        b'0'..=b'9' => Some((u32::from(c) - 43, 5)),
        FNC1_SENTINEL_BYTE => Some((15, 5)),
        b'A'..=b'Z' => Some((u32::from(c) - 1, 7)),
        b'a'..=b'z' => Some((u32::from(c) - 7, 7)),
        b'!' => Some((232, 8)),
        b'"' => Some((233, 8)),
        b'%' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b'-' | b'.' | b'/' => {
            Some((u32::from(c) + 197, 8))
        }
        b':' | b';' | b'<' | b'=' | b'>' | b'?' => Some((u32::from(c) + 187, 8)),
        b'_' => Some((251, 8)),
        b' ' => Some((252, 8)),
        _ => None,
    }
}

/// Bit-prefix values for the mode-latch sentinels.
const LATCH_FROM_NUMERIC_TO_ALPHA: (u32, u8) = (0, 4);
const LATCH_FROM_ALPHA_TO_NUMERIC: (u32, u8) = (0, 3);
const LATCH_FROM_ALPHA_TO_ISO646: (u32, u8) = (4, 5);
const LATCH_FROM_ISO646_TO_NUMERIC: (u32, u8) = (0, 3);
const LATCH_FROM_ISO646_TO_ALPHA: (u32, u8) = (4, 5);

/// Mutable bit-string builder. Mirrors BWIPP's `$_.gpfenc[$_.j]`
/// in-place bit writes. Push fixed-width bit fields with [`push`];
/// finalise to a [`Vec<u8>`] of 0/1 bits via [`into_bits`].
struct BitBuf {
    bits: Vec<u8>,
}

impl BitBuf {
    fn new() -> Self {
        Self { bits: Vec::new() }
    }
    fn push(&mut self, value: u32, width: u8) {
        for k in (0..width).rev() {
            self.bits.push(u8::try_from((value >> k) & 1).unwrap());
        }
    }
    fn len(&self) -> usize {
        self.bits.len()
    }
    fn into_bits(self) -> Vec<u8> {
        self.bits
    }
}

/// Reverse-pass lookahead tables BWIPP uses to drive its mode-decision
/// logic in the general-purpose encoder.
#[derive(Debug, Clone)]
pub(crate) struct GpfLookahead {
    /// `numeric_runs[i]` = how many bytes starting at position `i`
    /// can all be encoded in numeric mode, advancing by pairs. Always
    /// even (each numeric step consumes 2 bytes). Length is `gpf.len()+2`.
    pub numeric_runs: Vec<u32>,
    /// `alphanumeric_runs[i]` = how many consecutive bytes starting
    /// at position `i` are in the alphanumeric charset. Length is
    /// `gpf.len()+1`.
    pub alphanumeric_runs: Vec<u32>,
    /// `next_iso646_only[i]` = distance from `i` to the next byte that
    /// is in iso646 but NOT in alphanumeric, capped at 9999. Length
    /// is `gpf.len()+1`.
    pub next_iso646_only: Vec<u32>,
}

/// Compute the [`GpfLookahead`] tables for a general-purpose byte
/// stream. Each byte must be either an ASCII digit / letter /
/// punctuation in BWIPP's iso646 alphabet, or the FNC1 sentinel
/// (already substituted to `'^'`).
///
/// Direct port of bwip-js lines 14109-14167. Reverse pass starting
/// from the tail of `gpf`.
pub(crate) fn compute_gpf_lookahead(gpf: &[u8]) -> GpfLookahead {
    let n = gpf.len();
    let mut numeric_runs = vec![0u32; n + 2];
    let mut alphanumeric_runs = vec![0u32; n + 1];
    let mut next_iso646_only = vec![9999u32; n + 1];

    for i in (0..n).rev() {
        let c = gpf[i];
        let c_next = if i + 1 < n { Some(gpf[i + 1]) } else { None };
        // Numeric pair status — both bytes must be in the numeric
        // alphabet (digits + FNC1). If only one byte remains, the
        // pair status is false.
        let numeric_pair_ok = match c_next {
            Some(n2) => numeric_pair_bits(c, n2).is_some(),
            None => false,
        };
        numeric_runs[i] = if numeric_pair_ok {
            numeric_runs[i + 2] + 2
        } else {
            0
        };
        // Alphanumeric run length using the *current* byte only.
        alphanumeric_runs[i] = if alphanumeric_byte_bits(c).is_some() {
            alphanumeric_runs[i + 1] + 1
        } else {
            0
        };
        // ISO-646-only distance: if c is in iso646 but NOT in
        // alphanumeric, distance is 0. Otherwise inherit + 1.
        next_iso646_only[i] =
            if iso646_byte_bits(c).is_some() && alphanumeric_byte_bits(c).is_none() {
                0
            } else {
                next_iso646_only[i + 1].saturating_add(1)
            };
    }

    GpfLookahead {
        numeric_runs,
        alphanumeric_runs,
        next_iso646_only,
    }
}

/// Encode the general-purpose byte stream `gpf` into the variable-mode
/// bit field BWIPP appends after the cdf.
///
/// `bits_before_gpf` is the BWIPP `(12 + 1) + method.len + vlf_len +
/// cdf.len` accounting term — used by the rembits check in numeric
/// mode's lone-digit special case.
///
/// `segments` is forwarded to [`rembits`].
///
/// Returns the encoded bits as a `Vec<u8>` of 0/1 values. Mirrors
/// bwip-js lines 14168-14311 step-for-step:
///   * Mode is initially numeric.
///   * Numeric mode encodes digit / FNC1 pairs (7 bits each) and
///     handles the lone-digit edge case at the end of input.
///   * Alphanumeric mode encodes one byte at a time, latching to
///     numeric when the run-length tables say it pays off, and to
///     iso646 when a non-alphanumeric iso646 byte is hit.
///   * Iso646 mode also encodes one byte at a time, with lookahead
///     for back-latches to numeric (when ≥4 numeric pairs remain
///     and the next iso646-only byte is ≥10 chars away) or
///     alphanumeric (≥5-byte run + same iso646-only check).
pub(crate) fn encode_general_purpose(
    gpf: &[u8],
    bits_before_gpf: usize,
    segments: usize,
) -> Result<(Vec<u8>, CharsetMode), crate::error::Error> {
    let look = compute_gpf_lookahead(gpf);
    let mut out = BitBuf::new();
    let mut i = 0usize;
    let mut mode = CharsetMode::Numeric;
    while i < gpf.len() {
        match mode {
            CharsetMode::Numeric => {
                if i + 1 < gpf.len() {
                    let c0 = gpf[i];
                    let c1 = gpf[i + 1];
                    if let Some((value, width)) = numeric_pair_bits(c0, c1) {
                        out.push(value, width);
                        i += 2;
                    } else {
                        // Pair not encodable in numeric mode →
                        // latch to alphanumeric and try again.
                        out.push(LATCH_FROM_NUMERIC_TO_ALPHA.0, LATCH_FROM_NUMERIC_TO_ALPHA.1);
                        mode = CharsetMode::Alphanumeric;
                    }
                } else {
                    // Exactly one byte left.
                    let c = gpf[i];
                    if !c.is_ascii_digit() {
                        // Lone non-digit (including FNC1) — latch to
                        // alphanumeric and re-handle there.
                        out.push(LATCH_FROM_NUMERIC_TO_ALPHA.0, LATCH_FROM_NUMERIC_TO_ALPHA.1);
                        mode = CharsetMode::Alphanumeric;
                    } else {
                        // Lone digit. If the remaining symbol bits
                        // are 4..=6, BWIPP encodes the digit in
                        // exactly that many bits (right-justified).
                        // Otherwise pair it with the FNC1 sentinel.
                        let rem = rembits(bits_before_gpf + out.len(), segments);
                        if (4..=6).contains(&rem) {
                            let value = u32::from(c) - 47;
                            out.push(value, rem as u8);
                            break;
                        }
                        let (value, width) = numeric_pair_bits(c, FNC1_SENTINEL_BYTE)
                            .expect("digit + FNC1 always encodable in numeric");
                        out.push(value, width);
                        i += 1;
                    }
                }
            }
            CharsetMode::Alphanumeric => {
                let c = gpf[i];
                if c == FNC1_SENTINEL_BYTE {
                    // FNC1 in alphanumeric — encode it (5 bits) and
                    // implicit-latch back to numeric mode.
                    let (value, width) = alphanumeric_byte_bits(c).unwrap();
                    out.push(value, width);
                    mode = CharsetMode::Numeric;
                    i += 1;
                    continue;
                }
                // ISO-646-only byte? Latch to iso646.
                if iso646_byte_bits(c).is_some() && alphanumeric_byte_bits(c).is_none() {
                    out.push(LATCH_FROM_ALPHA_TO_ISO646.0, LATCH_FROM_ALPHA_TO_ISO646.1);
                    mode = CharsetMode::Iso646;
                    continue;
                }
                // Look-ahead: ≥6 numeric pairs starting here? Latch
                // to numeric. Also latch if the numeric run extends
                // to the end of the input (≥4 pairs).
                let nr = look.numeric_runs[i];
                if nr >= 6 || (nr >= 4 && i + nr as usize == gpf.len()) {
                    out.push(LATCH_FROM_ALPHA_TO_NUMERIC.0, LATCH_FROM_ALPHA_TO_NUMERIC.1);
                    mode = CharsetMode::Numeric;
                    continue;
                }
                let (value, width) = alphanumeric_byte_bits(c).ok_or_else(|| {
                    crate::error::Error::InvalidData(format!(
                        "DataBar Expanded: byte 0x{c:02x} not encodable in alphanumeric mode",
                    ))
                })?;
                out.push(value, width);
                i += 1;
            }
            CharsetMode::Iso646 => {
                let c = gpf[i];
                if c == FNC1_SENTINEL_BYTE {
                    let (value, width) = iso646_byte_bits(c).unwrap();
                    out.push(value, width);
                    mode = CharsetMode::Numeric;
                    i += 1;
                    continue;
                }
                let nr = look.numeric_runs[i];
                let nio = look.next_iso646_only[i];
                if nr >= 4 && nio >= 10 {
                    out.push(
                        LATCH_FROM_ISO646_TO_NUMERIC.0,
                        LATCH_FROM_ISO646_TO_NUMERIC.1,
                    );
                    mode = CharsetMode::Numeric;
                    continue;
                }
                let ar = look.alphanumeric_runs[i];
                if ar >= 5 && nio >= 10 {
                    out.push(LATCH_FROM_ISO646_TO_ALPHA.0, LATCH_FROM_ISO646_TO_ALPHA.1);
                    mode = CharsetMode::Alphanumeric;
                    continue;
                }
                let (value, width) = iso646_byte_bits(c).ok_or_else(|| {
                    crate::error::Error::InvalidData(format!(
                        "DataBar Expanded: byte 0x{c:02x} not encodable in iso646 mode",
                    ))
                })?;
                out.push(value, width);
                i += 1;
            }
        }
    }
    Ok((out.into_bits(), mode))
}

/// Match an AI string against BWIPP's "is this a 310x or 320x"
/// table, returning `(is_320x, last_digit)` on hit, `None` on miss.
fn match_310x_or_320x(ai: &str) -> Option<(bool, u8)> {
    let bytes = ai.as_bytes();
    if bytes.len() != 4 {
        return None;
    }
    if bytes[3].is_ascii_digit() {
        if &bytes[..3] == b"310" {
            return Some((false, bytes[3] - b'0'));
        }
        if &bytes[..3] == b"320" {
            return Some((true, bytes[3] - b'0'));
        }
    }
    None
}

/// BWIPP date-AI table for method 0111xxx. The index is the value
/// encoded into bits 4-5 of the method prefix.
const DATE_AI_TABLE: &[(&str, u8)] = &[("11", 0), ("13", 1), ("15", 2), ("17", 3)];

/// Method family "0111xxx" — (01) + (310x|320x) optionally + (11|13|15|17).
///
/// 8 distinct method strings collapsed into one helper:
/// ```text
///   bits 0..3  = "0111"
///   bit  3..5  = date_kind (00 = none/11, 01 = 13, 10 = 15, 11 = 17)
///   bit  6     = 0 if 310x, 1 if 320x
/// ```
/// cdf layout (76 bits):
/// ```text
///   bits 0..40  conv12to40(GTIN[1..13])
///   bits 40..60 tobin(last_digit_of_31xx_or_32xx * 100_000 + value, 20)
///   bits 60..76 tobin(date_encoded, 16) where date_encoded =
///                 yy*384 + (mm-1)*32 + dd, or the sentinel 38400
///                 when no date AI is present.
/// ```
///
/// Preconditions per BWIPP:
///   * (01) value is 14 ASCII digits starting with '9';
///   * (310x|320x) value is 6 ASCII digits ≤ 99999;
///   * date value, if present, is a valid YYMMDD (month 01-12, day 0-31).
pub(crate) fn encode_method_0111(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    if !(2..=3).contains(&elements.len()) || elements[0].ai != "01" {
        return Ok(None);
    }
    let Some((is_320x, last_digit)) = match_310x_or_320x(&elements[1].ai) else {
        return Ok(None);
    };
    let v0 = &elements[0].data;
    let v1 = &elements[1].data;
    if v0.len() != 14
        || !v0.bytes().all(|b| b.is_ascii_digit())
        || v0.as_bytes()[0] != b'9'
        || v1.len() != 6
        || !v1.bytes().all(|b| b.is_ascii_digit())
    {
        return Ok(None);
    }
    let v1_int: u64 = v1.parse().unwrap();
    if v1_int > 99999 {
        return Ok(None);
    }
    let (date_kind, date_encoded) = if elements.len() == 3 {
        let Some(&(_, kind)) = DATE_AI_TABLE.iter().find(|(ai, _)| *ai == elements[2].ai) else {
            return Ok(None);
        };
        let d = &elements[2].data;
        if d.len() != 6 || !d.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(None);
        }
        let yy: u32 = d[0..2].parse().unwrap();
        let mm: u32 = d[2..4].parse().unwrap();
        let dd: u32 = d[4..6].parse().unwrap();
        if !(1..=12).contains(&mm) || dd > 31 {
            return Ok(None);
        }
        (kind, yy * 384 + (mm - 1) * 32 + dd)
    } else {
        (0u8, 38400u32)
    };
    let method_bits = vec![
        0u8,
        1,
        1,
        1,
        (date_kind >> 1) & 1,
        date_kind & 1,
        u8::from(is_320x),
    ];
    let mut cdf_str = conv12to40(&v0.as_bytes()[1..13])?;
    let combined = u64::from(last_digit) * 100_000 + (v1_int % 100_000);
    cdf_str.push_str(&to_bin(combined, 20)?);
    cdf_str.push_str(&to_bin(u64::from(date_encoded), 16)?);
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    Ok(Some(MethodOutput {
        method_bits,
        vlf_len: 0,
        cdf,
        gpf_prefix_bytes: Vec::new(),
        consumed_ais: elements.len(),
    }))
}

/// Method "01100" — (01) + (392x) with GTIN starting "9".
///
/// cdf layout (42 bits):
///   * bits 0..40: conv12to40(GTIN[1..13]);
///   * bits 40..42: tobin(last_digit_of_3920_to_3923, 2).
///
/// gpf prefix: the raw bytes of the (392x) value (variable length).
/// Method consumes 2 AIs.
pub(crate) fn encode_method_01100(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    if elements.len() < 2 || elements[0].ai != "01" {
        return Ok(None);
    }
    let ai1 = &elements[1].ai;
    let last_digit = match ai1.as_str() {
        "3920" => 0u8,
        "3921" => 1,
        "3922" => 2,
        "3923" => 3,
        _ => return Ok(None),
    };
    let v0 = &elements[0].data;
    if v0.len() != 14 || !v0.bytes().all(|b| b.is_ascii_digit()) || v0.as_bytes()[0] != b'9' {
        return Ok(None);
    }
    let v1_bytes = elements[1].data.as_bytes();
    if v1_bytes.is_empty() || !v1_bytes.iter().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let mut cdf_str = conv12to40(&v0.as_bytes()[1..13])?;
    cdf_str.push_str(&to_bin(u64::from(last_digit), 2)?);
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    // gpf prefix: the (392x) value bytes. If more AIs follow, append
    // an FNC1 sentinel — BWIPP does this inside the method dispatcher
    // (bwip-js line 13983-13986) rather than in the trailing-AI loop.
    let mut gpf_prefix = v1_bytes.to_vec();
    if elements.len() > 2 {
        gpf_prefix.push(FNC1_SENTINEL_BYTE);
    }
    Ok(Some(MethodOutput {
        method_bits: vec![0, 1, 1, 0, 0],
        vlf_len: 2,
        cdf,
        gpf_prefix_bytes: gpf_prefix,
        consumed_ais: 2,
    }))
}

/// Method "01101" — (01) + (393x) with GTIN starting "9".
///
/// cdf layout (52 bits):
///   * bits 0..40: conv12to40(GTIN[1..13]);
///   * bits 40..42: tobin(last_digit_of_3930_to_3933, 2);
///   * bits 42..52: tobin(currency code (first 3 digits of value), 10).
///
/// gpf prefix: the raw bytes of the (393x) value AFTER the 3-digit
/// currency code prefix. Method consumes 2 AIs.
pub(crate) fn encode_method_01101(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    if elements.len() < 2 || elements[0].ai != "01" {
        return Ok(None);
    }
    let ai1 = &elements[1].ai;
    let last_digit = match ai1.as_str() {
        "3930" => 0u8,
        "3931" => 1,
        "3932" => 2,
        "3933" => 3,
        _ => return Ok(None),
    };
    let v0 = &elements[0].data;
    if v0.len() != 14 || !v0.bytes().all(|b| b.is_ascii_digit()) || v0.as_bytes()[0] != b'9' {
        return Ok(None);
    }
    let v1_bytes = elements[1].data.as_bytes();
    if v1_bytes.len() < 3 || !v1_bytes[..3].iter().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let currency: u64 = std::str::from_utf8(&v1_bytes[..3])
        .unwrap()
        .parse()
        .unwrap();
    let mut cdf_str = conv12to40(&v0.as_bytes()[1..13])?;
    cdf_str.push_str(&to_bin(u64::from(last_digit), 2)?);
    cdf_str.push_str(&to_bin(currency, 10)?);
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    // gpf prefix: v1 chars AFTER the 3 currency digits.
    let mut gpf_prefix = v1_bytes[3..].to_vec();
    if elements.len() > 2 {
        gpf_prefix.push(FNC1_SENTINEL_BYTE);
    }
    Ok(Some(MethodOutput {
        method_bits: vec![0, 1, 1, 0, 1],
        vlf_len: 2,
        cdf,
        gpf_prefix_bytes: gpf_prefix,
        consumed_ais: 2,
    }))
}

/// Method "1" — single AI (01) holding a 14-digit GTIN.
///
/// BWIPP cdf is `conv13to44(gtin14[0..13])` — the leading 13 digits;
/// the 14th (check) digit is reconstructed at decode time from the
/// GTIN-13 mod-10 algorithm.
///
/// Returns `None` when the input doesn't satisfy the method's
/// preconditions (single (01), exactly 14 ASCII digits). The
/// top-level encoder then falls through to the next method or to
/// the general-purpose path.
pub(crate) fn encode_method_1(
    elements: &[crate::util::gs1::Element],
) -> Result<Option<MethodOutput>, crate::error::Error> {
    if elements.is_empty() || elements[0].ai != "01" {
        return Ok(None);
    }
    let data = &elements[0].data;
    if data.len() != 14 || !data.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let cdf_str = conv13to44(&data.as_bytes()[..13])?;
    let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
    Ok(Some(MethodOutput {
        method_bits: vec![1],
        vlf_len: 2,
        cdf,
        gpf_prefix_bytes: Vec::new(),
        consumed_ais: 1,
    }))
}

/// Build the BWIPP general-purpose byte stream for a list of parsed
/// GS1 elements. Concatenates each AI's digits + data, inserting the
/// FNC1 sentinel (`'^'` = 94) after every variable-length AI that
/// has a following element. Direct port of bwip-js lines 14048-14057.
pub(crate) fn build_gpf_bytes(
    elements: &[crate::util::gs1::Element],
) -> Result<Vec<u8>, crate::error::Error> {
    let mut out = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        out.extend_from_slice(e.ai.as_bytes());
        out.extend_from_slice(e.data.as_bytes());
        let variable = crate::util::gs1::ai_is_variable_length(&e.ai).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "DataBar Expanded: AI ({}) not in the GS1 table",
                e.ai,
            ))
        })?;
        if variable && i + 1 < elements.len() {
            out.push(FNC1_SENTINEL_BYTE);
        }
    }
    Ok(out)
}

/// Method "00" — general-purpose fallback for any AI combination
/// the compressed methods don't claim.
///
/// `method_bits` is `[0, 0]`, `vlf_len` is 2 (gpfallow=true), `cdf`
/// is empty, and the entire payload sits in the gpf — actually
/// encoded by [`encode_general_purpose`] from the dispatcher (see
/// `encode_expanded_bits` calls). This helper just builds the byte
/// stream; the binval assembler runs gpf through the encoder once
/// `bits_before_gpf` is known.
pub(crate) fn encode_method_00_skeleton(
    _elements: &[crate::util::gs1::Element],
) -> Result<MethodOutput, crate::error::Error> {
    Ok(MethodOutput {
        method_bits: vec![0, 0],
        vlf_len: 2,
        cdf: Vec::new(),
        // The top-level encode() dispatcher walks elements[consumed_ais..]
        // and appends them itself, so method 00 just declares it
        // consumes nothing and the general-purpose encoder takes
        // care of the entire payload.
        gpf_prefix_bytes: Vec::new(),
        consumed_ais: 0,
    })
}

/// Default segments-per-row for the non-stacked "expanded" format,
/// matching BWIPP's `databarexpanded` default (bwip-js line 13505).
pub(crate) const DEFAULT_SEGMENTS: usize = 22;

/// Default segments-per-row for the stacked variant. BWIPP uses 4
/// when the option `format` is `expandedstacked` (bwip-js line 13505).
pub(crate) const STACKED_SEGMENTS: usize = 4;

/// End-to-end DataBar Expanded encode for a GS1 element string.
///
/// Supports all 7 BWIPP method-dispatch paths:
///
/// - method "1" — a single (01)+14-digit GTIN
/// - method "0100" — (01)+(3103)
/// - method "0101" — (01)+(3202)/(3203)
/// - method "0111xxx" — (01)+(310x|320x) optionally with (11|13|15|17)
/// - method "01100" — (01)+(392x)
/// - method "01101" — (01)+(393x) with 3-digit ISO currency code
/// - method "00" — general-purpose numeric/alphanumeric/iso646 fallback
///
/// All are verified byte-for-byte against bwip-js's
/// `tools/oracle-databarexpanded.js`.
///
/// `linkage` controls the BWIPP composite-flag (set when this
/// barcode is paired with a 2D composite component). Pass `false`
/// for a standalone DataBar Expanded barcode.
pub fn encode(
    input: &str,
    linkage: bool,
) -> Result<crate::encoding::LinearPattern, crate::error::Error> {
    let elements = crate::util::gs1::parse(input).map_err(|e| {
        crate::error::Error::InvalidData(format!("DataBar Expanded: GS1 parse failed: {e}",))
    })?;

    // Try methods in BWIPP's priority order. Compressed methods win
    // over the always-applicable method "1" and the general-purpose
    // "00" fallback when a payload could match either.
    let method = if let Some(m) = encode_method_0100(&elements)? {
        m
    } else if let Some(m) = encode_method_0101(&elements)? {
        m
    } else if let Some(m) = encode_method_0111(&elements)? {
        m
    } else if let Some(m) = encode_method_01100(&elements)? {
        m
    } else if let Some(m) = encode_method_01101(&elements)? {
        m
    } else if let Some(m) = encode_method_1(&elements)? {
        m
    } else {
        encode_method_00_skeleton(&elements)?
    };

    // Append any trailing AIs the method dispatcher didn't consume
    // to the gpf byte stream, then hand it to the general-purpose
    // encoder. Methods 0100/0101/0111 consume all available AIs and
    // disable vlf, so this branch is a no-op for them. Methods 1
    // and 00 leave room here for the trailing AIs (e.g. (01)+(10)…
    // for a GTIN + lot-number combo).
    let mut gpf_bytes = method.gpf_prefix_bytes.clone();
    for (i, e) in elements[method.consumed_ais..].iter().enumerate() {
        gpf_bytes.extend_from_slice(e.ai.as_bytes());
        gpf_bytes.extend_from_slice(e.data.as_bytes());
        let remaining = elements.len() - method.consumed_ais;
        let is_last = i + 1 == remaining;
        let variable = crate::util::gs1::ai_is_variable_length(&e.ai).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "DataBar Expanded: AI ({}) not in the GS1 table",
                e.ai,
            ))
        })?;
        if variable && !is_last {
            gpf_bytes.push(FNC1_SENTINEL_BYTE);
        }
    }
    let (gpf_bits, final_mode) = if gpf_bytes.is_empty() {
        (Vec::new(), CharsetMode::Numeric)
    } else {
        let bits_before_gpf = 1 + 12 + method.method_bits.len() + method.vlf_len + method.cdf.len();
        encode_general_purpose(&gpf_bytes, bits_before_gpf, DEFAULT_SEGMENTS)?
    };

    let (dxw, seq, _datalen) =
        encode_to_dxw_and_seq(linkage, &method, &gpf_bits, final_mode, DEFAULT_SEGMENTS)?;
    let sbs = assemble_sbs(&dxw, seq);
    Ok(crate::encoding::LinearPattern {
        bars: sbs,
        text: None,
    })
}

/// End-to-end DataBar Expanded Stacked encode for a GS1 element string.
///
/// Same encoder pipeline as [`encode`] but with `segments = 4` (the
/// stacked default per BWIPP bwip-js line 13505). The resulting
/// per-character widths arrays are laid out across `ceil(datalen / 4)`
/// rows, with row separators and an inter-row alternating-stripe
/// separator BWIPP-compatibly.
///
/// Output: a [`BitMatrix`] of the final symbol's module grid. The
/// caller can render it via [`render_svg`] / [`render_png`] like any
/// other 2D barcode.
///
/// [`BitMatrix`]: crate::encoding::BitMatrix
/// [`render_svg`]: crate::render_svg
/// [`render_png`]: crate::render_png
pub fn encode_stacked(
    input: &str,
    linkage: bool,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let elements = crate::util::gs1::parse(input).map_err(|e| {
        crate::error::Error::InvalidData(
            format!("DataBar Expanded Stacked: GS1 parse failed: {e}",),
        )
    })?;

    let method = if let Some(m) = encode_method_0100(&elements)? {
        m
    } else if let Some(m) = encode_method_0101(&elements)? {
        m
    } else if let Some(m) = encode_method_0111(&elements)? {
        m
    } else if let Some(m) = encode_method_01100(&elements)? {
        m
    } else if let Some(m) = encode_method_01101(&elements)? {
        m
    } else if let Some(m) = encode_method_1(&elements)? {
        m
    } else {
        encode_method_00_skeleton(&elements)?
    };

    let mut gpf_bytes = method.gpf_prefix_bytes.clone();
    for (i, e) in elements[method.consumed_ais..].iter().enumerate() {
        gpf_bytes.extend_from_slice(e.ai.as_bytes());
        gpf_bytes.extend_from_slice(e.data.as_bytes());
        let remaining = elements.len() - method.consumed_ais;
        let is_last = i + 1 == remaining;
        let variable = crate::util::gs1::ai_is_variable_length(&e.ai).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "DataBar Expanded Stacked: AI ({}) not in the GS1 table",
                e.ai,
            ))
        })?;
        if variable && !is_last {
            gpf_bytes.push(FNC1_SENTINEL_BYTE);
        }
    }
    let (gpf_bits, final_mode) = if gpf_bytes.is_empty() {
        (Vec::new(), CharsetMode::Numeric)
    } else {
        let bits_before_gpf = 1 + 12 + method.method_bits.len() + method.vlf_len + method.cdf.len();
        encode_general_purpose(&gpf_bytes, bits_before_gpf, STACKED_SEGMENTS)?
    };
    let (dxw, seq, datalen) =
        encode_to_dxw_and_seq(linkage, &method, &gpf_bits, final_mode, STACKED_SEGMENTS)?;
    let data_chars = datalen - 1; // pre-checksum count

    // Build fxw — per-finder-pair widths.
    let fxw: Vec<[u8; 5]> = seq
        .iter()
        .map(|&id| {
            let base = id as usize * 5;
            [
                FINDER_WIDTHS[base],
                FINDER_WIDTHS[base + 1],
                FINDER_WIDTHS[base + 2],
                FINDER_WIDTHS[base + 3],
                FINDER_WIDTHS[base + 4],
            ]
        })
        .collect();
    let _ = data_chars; // satisfy `unused` in some cfg builds

    let numrows = datalen.div_ceil(STACKED_SEGMENTS);

    // 1. Build per-row sbs (widths). Each row: [optional 0][1,1][dxw[pos]
    //    interleaved with fxw at even pos for each pos in this row's
    //    segment][1,1].
    let row_sbs: Vec<Vec<u8>> = (0..numrows)
        .map(|r| {
            let mut row: Vec<u8> = Vec::new();
            if STACKED_SEGMENTS % 4 != 0 && r % 2 == 1 {
                row.push(0);
            }
            row.push(1);
            row.push(1);
            for q in 0..STACKED_SEGMENTS {
                let pos = q + r * STACKED_SEGMENTS;
                if pos < datalen {
                    row.extend_from_slice(&dxw[pos]);
                    if pos % 2 == 0 {
                        row.extend_from_slice(&fxw[pos / 2]);
                    }
                }
            }
            row.push(1);
            row.push(1);
            row
        })
        .collect();

    // 2. Expand each row's sbs to module bits (even index → 0/space,
    //    odd index → 1/bar).
    let mut row_bits: Vec<Vec<u8>> = row_sbs.iter().map(|sbs| expand_row_sbs(sbs)).collect();

    // 3. Compute per-row separators.
    let mut row_seps: Vec<Vec<u8>> = row_bits.iter().map(|r| compute_row_sep(r)).collect();

    // 4. For odd rows with segments % 4 == 0, either reverse the row
    //    or prepend a zero — depending on whether this is the last
    //    (short) row with an odd finder count.
    if STACKED_SEGMENTS % 4 == 0 {
        let row0_len = row_bits[0].len();
        for r in 0..numrows {
            if r % 2 != 1 {
                continue;
            }
            let finder_count = count_finder_positions(row_bits[r].len());
            let length_differs = row_bits[r].len() != row0_len;
            if length_differs && finder_count % 2 == 1 {
                // Prepend a 0 to row and sep (no reversal).
                let mut new_row = vec![0u8];
                new_row.extend_from_slice(&row_bits[r]);
                row_bits[r] = new_row;
                let mut new_sep = vec![0u8];
                new_sep.extend_from_slice(&row_seps[r]);
                row_seps[r] = new_sep;
            } else {
                // Reverse both row and sep.
                row_bits[r].reverse();
                row_seps[r].reverse();
            }
        }
    }

    // 5. Pad the last row + last sep to the full pixx width (the
    //    first row's length).
    let pixx = row_bits[0].len();
    if numrows > 1 {
        let last = numrows - 1;
        if row_bits[last].len() < pixx {
            row_bits[last].resize(pixx, 0);
        }
        if row_seps[last].len() < pixx {
            row_seps[last].resize(pixx, 0);
        }
    }

    // 6. Inter-row separator: alternating [0,1,0,1,...] of length pixx
    //    with the first 4 and last 4 modules zeroed (BWIPP SEP_PAD).
    let inter_sep: Vec<u8> = build_inter_row_sep(pixx);

    // 7. Concatenate strips + rowmult, expand to BitMatrix.
    //    Strips order (per BWIPP):
    //      r=0:           rows[0], seps[0], inter_sep
    //      r=1..numrows-2 seps[r], rows[r], seps[r], inter_sep
    //      r=numrows-1:   seps[last], rows[last]
    let mut strips: Vec<&[u8]> = Vec::new();
    let mut row_heights: Vec<usize> = Vec::new();
    const DATA_ROW_HEIGHT: usize = 34;
    const SEP_HEIGHT: usize = 1;
    for r in 0..numrows {
        if r != 0 {
            strips.push(&row_seps[r]);
            row_heights.push(SEP_HEIGHT);
        }
        strips.push(&row_bits[r]);
        row_heights.push(DATA_ROW_HEIGHT);
        if r + 1 != numrows {
            strips.push(&row_seps[r]);
            row_heights.push(SEP_HEIGHT);
            strips.push(&inter_sep);
            row_heights.push(SEP_HEIGHT);
        }
    }

    let total_height: usize = row_heights.iter().sum();
    let mut bm = crate::encoding::BitMatrix::new(pixx, total_height);
    let mut y = 0;
    for (strip, &height) in strips.iter().zip(row_heights.iter()) {
        for _ in 0..height {
            for (x, &bit) in strip.iter().enumerate() {
                bm.set(x, y, bit != 0);
            }
            y += 1;
        }
    }
    debug_assert_eq!(y, total_height);
    Ok(bm)
}

/// Expand a row's sbs widths array to module bits per BWIPP:
/// even-index widths emit `0` (space), odd-index emit `1` (bar).
fn expand_row_sbs(sbs: &[u8]) -> Vec<u8> {
    let total: usize = sbs.iter().map(|&w| w as usize).sum();
    let mut out = Vec::with_capacity(total);
    for (i, &w) in sbs.iter().enumerate() {
        let bit = if i % 2 == 0 { 0u8 } else { 1u8 };
        for _ in 0..w {
            out.push(bit);
        }
    }
    out
}

/// Build the BWIPP separator for one row's module bits.
///
/// Base pattern is the row's bitwise complement (`1 - row[i]`). At
/// each finder position the separator is then overridden module by
/// module: at module `i` within a finder (`p..=p+14`), if `row[i] ==
/// 0` *and* `row[i-1] == 1` → `sep[i] = 1`; otherwise it tracks the
/// previous `sep[i-1]` (`0` follows `1` follows `0`...). When
/// `row[i] == 1` the separator is forced to `0`. Finally the first
/// and last 4 modules are zeroed (BWIPP `seppad`).
fn compute_row_sep(row: &[u8]) -> Vec<u8> {
    let n = row.len();
    let mut sep: Vec<u8> = row.iter().map(|&b| 1 - b).collect();
    // Identify finder positions: 19, 68, 19+98, 68+98, ... ≤ n-13.
    let finder_positions: Vec<usize> = {
        let mut v = Vec::new();
        if n >= 13 {
            let max = n - 13;
            let mut p = 19;
            while p <= max {
                v.push(p);
                p += 98;
            }
            let mut p = 68;
            while p <= max {
                v.push(p);
                p += 98;
            }
        }
        v
    };
    for &p in &finder_positions {
        for i in p..=(p + 14).min(n - 1) {
            let bit = if row[i] == 1 {
                0
            } else if i > 0 && (row[i - 1] == 1 || sep[i - 1] == 0) {
                1
            } else {
                0
            };
            sep[i] = bit;
        }
    }
    // SEP_PAD: zero first and last 4 modules.
    for slot in sep.iter_mut().take(4) {
        *slot = 0;
    }
    if n >= 4 {
        for slot in sep[n - 4..].iter_mut() {
            *slot = 0;
        }
    }
    sep
}

/// Count finder positions in a row of `n` modules — BWIPP's
/// `finderpos` iteration at offsets 19 and 68 mod 98.
fn count_finder_positions(n: usize) -> usize {
    if n < 13 {
        return 0;
    }
    let max = n - 13;
    let mut c = 0;
    let mut p = 19;
    while p <= max {
        c += 1;
        p += 98;
    }
    let mut p = 68;
    while p <= max {
        c += 1;
        p += 98;
    }
    c
}

/// Build the inter-row separator: alternating `[0, 1, 0, 1, ...]`
/// of length `pixx` with the first 4 and last 4 modules zeroed
/// (BWIPP `seppad`).
fn build_inter_row_sep(pixx: usize) -> Vec<u8> {
    let mut sep: Vec<u8> = (0..pixx).map(|i| (i % 2) as u8).collect();
    for slot in sep.iter_mut().take(4) {
        *slot = 0;
    }
    if pixx >= 4 {
        for slot in sep[pixx - 4..].iter_mut() {
            *slot = 0;
        }
    }
    sep
}

/// Run the encoder pipeline from the method-dispatcher output through
/// the per-character widths array and finder sequence.
///
/// Returns:
/// 1. `dxw[0]` = checksum-character widths, `dxw[1..]` = data-character
///    widths (one entry per 12-bit codeword in the assembled binval);
/// 2. `seq` = the finder-id sequence (a `&'static` slice of [`FINDER_SEQ`]);
/// 3. `total_chars` = `dxw.len()` (`datalen` after BWIPP's checksum
///    insertion).
///
/// Shared between the linear (`encode`) and stacked
/// (`encode_stacked`) entries — the difference between them is just
/// the `segments` argument (22 for linear, 4 for stacked) plus the
/// downstream layout assembly.
pub(crate) fn encode_to_dxw_and_seq(
    linkage: bool,
    method: &MethodOutput,
    gpf_bits: &[u8],
    final_mode: CharsetMode,
    segments: usize,
) -> Result<(Vec<[u8; 8]>, &'static [u8], usize), crate::error::Error> {
    let binval = assemble_binval(
        u8::from(linkage),
        &method.method_bits,
        method.vlf_len,
        &method.cdf,
        gpf_bits,
        final_mode,
        segments,
    );
    debug_assert_eq!(binval.len() % 12, 0);
    let data_chars = binval.len() / 12;
    // GS1 DataBar Expanded holds at most 21 data symbol characters (plus
    // 1 checksum = 22 total); the encoder also needs at least 2. Inputs
    // that pack out of this range must fail gracefully rather than trip
    // `finder_sequence`'s bounds assert — render_svg(DatabarExpanded*, …)
    // reaches here with attacker-controlled length (fuzz crash
    // databar_expanded.rs:1636, "got 25").
    if !(2..=21).contains(&data_chars) {
        return Err(crate::error::Error::InvalidData(format!(
            "DataBar Expanded: encoded data needs {data_chars} symbol \
             characters, but the symbology holds only 2..=21 (max 21 data \
             + 1 checksum) — input too long",
        )));
    }
    let data_dxw: Vec<[u8; 8]> = (0..data_chars)
        .map(|x| {
            let d = pack_12_bits(&binval[x * 12..x * 12 + 12]);
            extract_data_character(d, x)
        })
        .collect();
    let seq = finder_sequence(data_chars);
    let checksum = compute_checksum(&data_dxw, seq);
    let mut dxw: Vec<[u8; 8]> = Vec::with_capacity(data_chars + 1);
    dxw.push(extract_checksum_character(checksum));
    dxw.extend(data_dxw);
    let total_chars = dxw.len();
    Ok((dxw, seq, total_chars))
}

/// Pick the finder-id sequence BWIPP uses for a symbol with
/// `data_chars` 12-bit data codewords (i.e. `binval.len() / 12`,
/// **before** the checksum character is prepended).
///
/// BWIPP looks up `finderseq[(datalen-2)/2]` while `$_.datalen` is
/// still the data-only count; the assignment at bwip-js line 14622
/// later bumps `$_.datalen` to include the checksum, but the seq
/// has already been frozen at that point. Indexing by the total
/// (checksum-inclusive) count drifts to a different row at every
/// even data-count boundary — got me on (01)+(3103), which fell
/// through to bracket 2 (`[0,5,2,7]`) instead of staying in bracket
/// 1 (`[0,3,2]`) where it belongs.
///
/// The returned slice's length is exactly the number of finder
/// pairs the symbol carries (one after every even-indexed dxw
/// slot, counting the checksum slot at index 0).
///
/// Panics if `data_chars < 2` or `data_chars > 21` — DataBar
/// Expanded symbols are bounded by the encoder's "Maximum length
/// exceeded" check at 21 total characters (1 checksum + 20 data).
pub(crate) fn finder_sequence(data_chars: usize) -> &'static [u8] {
    assert!(
        (2..=21).contains(&data_chars),
        "DataBar Expanded data_chars must be 2..=21, got {data_chars}",
    );
    FINDER_SEQ[(data_chars - 2) / 2]
}

/// Assemble the final start-bar-space (sbs) pattern from the
/// extracted symbol-character widths and the finder sequence.
///
/// Layout per BWIPP `databarexpanded` (bwip-js lines 14654–14665):
/// ```text
///   1 (start margin)
///   dxw[0]                       — checksum character (8 widths)
///   fxw[0]                       — first finder (5 widths)
///   dxw[1], dxw[2]               — data chars 0, 1 (8 widths each)
///   fxw[1]                       — second finder
///   dxw[3], dxw[4]               — data chars 2, 3
///   fxw[2]                       — third finder
///   ...
///   1 (end margin)
///   1 (terminator)
/// ```
///
/// In words: a finder pattern follows every even-indexed (in BWIPP's
/// post-reassignment indexing) dxw slot. The checksum at dxw[0] is
/// followed by finder[0]; dxw[1]/dxw[2] are a back-to-back pair
/// straddling finder[1]; and so on.
///
/// The total length is `1 + 8 * datalen + 5 * seq.len() + 2`.
pub(crate) fn assemble_sbs(dxw: &[[u8; 8]], seq: &[u8]) -> Vec<u8> {
    let datalen = dxw.len();
    debug_assert_eq!(
        seq.len(),
        datalen.div_ceil(2),
        "seq length should match datalen/2",
    );
    let mut sbs: Vec<u8> = Vec::with_capacity(1 + 8 * datalen + 5 * seq.len() + 2);
    sbs.push(1); // start margin
    for (i, dw) in dxw.iter().enumerate() {
        sbs.extend_from_slice(dw);
        if i % 2 == 0 {
            let finder_id = seq[i / 2] as usize;
            sbs.extend_from_slice(&FINDER_WIDTHS[finder_id * 5..finder_id * 5 + 5]);
        }
    }
    sbs.push(1); // end margin
    sbs.push(1); // terminator
    sbs
}

/// Compute the mod-211 checksum BWIPP injects between the encoder
/// output and the symbol-character extraction.
///
/// `dxw` is the per-segment widths array (length `datalen`, before
/// the checksum is inserted). `seq` is the finder sequence for this
/// `datalen`. Returns the encoded checksum value already biased by
/// `(datalen - 3) * 211` per BWIPP line 13583 — this is what the
/// row scan compares against.
pub(crate) fn compute_checksum(dxw: &[[u8; 8]], seq: &[u8]) -> u32 {
    // Flatten dxw to a single widths vector (8 elements per segment).
    let datalen = dxw.len();
    let mut widths = Vec::with_capacity(datalen * 8);
    for w in dxw {
        widths.extend_from_slice(w);
    }
    // Build the per-segment check weight stream by concatenating the
    // 16-element weight row for each finder in `seq`, then dropping
    // the leading 8 sentinel entries (BWIPP `getinterval(_, 8, _)`).
    let mut weights: Vec<i16> = Vec::with_capacity(seq.len() * 16);
    for &finder_id in seq {
        let base = (finder_id as usize) * 16;
        weights.extend_from_slice(&CHECK_WEIGHTS[base..base + 16]);
    }
    let weights = &weights[8..];

    let mut checksum: i32 = 0;
    for (i, &w) in widths.iter().enumerate() {
        checksum += i32::from(w) * i32::from(weights[i]);
    }
    let cs = checksum.rem_euclid(211);
    // Bias by (datalen - 3) * 211 so the row scan picks the right
    // tab174 entry. Datalen here is the number of data characters
    // (before the checksum is inserted).
    let biased = cs + (datalen as i32 - 3) * 211;
    u32::try_from(biased).expect("checksum bias overflow shouldn't happen for valid input")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_174_has_five_rows_of_eight_fields() {
        assert_eq!(TAB_174.len(), 40);
        // Thresholds at field 0 (rows 0..=4): 347, 1387, 2947, 3987, 4191.
        assert_eq!(TAB_174[0], 347);
        assert_eq!(TAB_174[8], 1387);
        assert_eq!(TAB_174[16], 2947);
        assert_eq!(TAB_174[24], 3987);
        assert_eq!(TAB_174[32], 4191);
        // The min-value field of each row should equal the previous
        // row's max + 1.
        for row in 1..5 {
            let prev_max = TAB_174[(row - 1) * 8];
            let this_min = TAB_174[row * 8 + 1];
            assert_eq!(this_min, prev_max + 1, "row {row} min ≠ prev max + 1");
        }
    }

    #[test]
    fn finder_widths_match_bwipp_shape() {
        assert_eq!(FINDER_WIDTHS.len(), 60);
        // Each finder is exactly 17 modules total (5 widths summing
        // to the finder character width — BWIPP design constraint).
        for f in 0..12 {
            let s: u32 = FINDER_WIDTHS[f * 5..f * 5 + 5]
                .iter()
                .map(|&w| u32::from(w))
                .sum();
            assert_eq!(s, 15, "finder {f} widths don't sum to 15");
        }
        // Spot-check first/last: F00 = (1,8,4,1,1), F11 = (1,1,9,2,2).
        assert_eq!(&FINDER_WIDTHS[..5], &[1u8, 8, 4, 1, 1]);
        assert_eq!(&FINDER_WIDTHS[55..], &[1u8, 1, 9, 2, 2]);
    }

    #[test]
    fn finder_seq_length_grows_with_datalen() {
        assert_eq!(FINDER_SEQ.len(), 10);
        // Each entry's length matches the number of finder pairs
        // for the next data-segment-count bracket.
        // bracket 0 → datalen 2..=3 → 1 finder pair (ceil(3/2)=2 but
        // BWIPP rounds via floor((datalen-1)/2) — the actual length
        // is ceil(datalen/2). For datalen=2 we have 1 finder; for
        // datalen=3 we have 2 finders. Length of the sequence
        // accommodates the max in the bracket.
        let expected_lens = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        for (i, expected) in expected_lens.into_iter().enumerate() {
            assert_eq!(FINDER_SEQ[i].len(), expected, "seq[{i}].len()");
        }
        // Every value must be in 0..12 (a valid finder id).
        for (i, seq) in FINDER_SEQ.iter().enumerate() {
            for (j, &id) in seq.iter().enumerate() {
                assert!(id < 12, "seq[{i}][{j}] = {id} ≥ 12");
            }
        }
    }

    #[test]
    fn check_weights_has_sentinel_row_plus_eleven_data_rows() {
        // 16 sentinel cells (`-1`) for the placeholder slot, then
        // 11 rows × 16 weights = 176 active weights = 192 total.
        assert_eq!(CHECK_WEIGHTS.len(), 192);
        // First 8 entries are `-1` sentinels (the placeholder row).
        for (k, &v) in CHECK_WEIGHTS.iter().enumerate().take(8) {
            assert_eq!(v, -1, "sentinel cell {k}");
        }
        // First active weight is 77 (BWIPP row 1, column 0).
        assert_eq!(CHECK_WEIGHTS[8], 77);
        // Last active weight is 45 (BWIPP row 11, column 15).
        assert_eq!(CHECK_WEIGHTS[191], 45);
        // No active weight may exceed 210 (mod-211 codomain).
        for (k, &w) in CHECK_WEIGHTS.iter().enumerate().skip(8) {
            assert!((0..211).contains(&w), "weight {k} = {w} outside [0, 210]",);
        }
    }

    #[test]
    fn fill_and_sep_pat_shapes() {
        assert_eq!(FILL_PAT, [0, 0, 1, 0, 0]);
        assert_eq!(SEP_PAD, [0, 0, 0, 0]);
    }

    #[test]
    fn sentinels_are_distinct() {
        let mut s = [FNC1, LATCH_NUMERIC, LATCH_ALPHANUMERIC, LATCH_ISO646];
        for v in s {
            assert!(v < 0, "{v} should be a negative sentinel");
        }
        s.sort_unstable();
        for i in 1..s.len() {
            assert_ne!(s[i - 1], s[i], "sentinel collision at {i}");
        }
    }

    #[test]
    fn to_bin_pads_with_leading_zeros() {
        assert_eq!(to_bin(0, 4).unwrap(), "0000");
        assert_eq!(to_bin(5, 4).unwrap(), "0101");
        assert_eq!(to_bin(15, 4).unwrap(), "1111");
        assert_eq!(to_bin(255, 8).unwrap(), "11111111");
        assert_eq!(to_bin(0, 1).unwrap(), "0");
        assert_eq!(to_bin(1, 1).unwrap(), "1");
        // 12-bit wide value: 4095 fits, 4096 doesn't.
        assert_eq!(to_bin(4095, 12).unwrap(), "111111111111");

        // Stage 11.A8c — strengthen the previously-weak `is_err()`
        // check to pin the symbology tag, the value+width echo, and
        // the "doesn't fit" predicate. The overflow guard at line
        // 146-150 produces:
        //   "DataBar Expanded: value {n} doesn't fit in {width} bits"
        //
        // A mutant that drops `{n}` or `{width}` from the format
        // string, or swaps the predicate text, would survive the
        // old is_err() check. Both echo substrings also kill the
        // off-by-one mutants `n >= ... → n > ...` (4096 would still
        // error but with a different value if the predicate fired
        // for 4097 instead).
        // Stage 11.A8c (cont) — switch from `let-else` to `match` so the
        // panic carries the actual rejected variant, killing mutations that
        // re-route overflow through a non-InvalidData error variant.
        let msg = match to_bin(4096, 12).unwrap_err() {
            crate::error::Error::InvalidData(m) => m,
            other => panic!(
                "to_bin(4096, 12) must reject as InvalidData (value-overflows-width); got {other:?}"
            ),
        };
        assert!(
            msg.contains("DataBar Expanded:"),
            "overflow diagnostic must carry symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("4096"),
            "overflow diagnostic must echo the offending value; got {msg:?}"
        );
        assert!(
            msg.contains("12 bits"),
            "overflow diagnostic must echo the requested width; got {msg:?}"
        );
        assert!(
            msg.contains("doesn't fit"),
            "overflow diagnostic must use the 'doesn't fit' predicate; got {msg:?}"
        );

        // Width=0 edge case: to_bin(1, 0) overflows (1 >= 1<<0=1) and
        // anchors the lower-width boundary. to_bin(0, 0) succeeds
        // (empty string) — both kill mutants that off-by-one the
        // width-comparison or special-case width=0.
        //
        // Defense-in-depth diagnostic pin: the width=0 anchor pins the
        // same format substrings as the 4096/12 anchor above, but with
        // distinct values "1" + "0 bits". A mutation that hardcoded
        // {n}=4096 or {width}=12 (instead of interpolating) would pass
        // the upper anchor but fail this lower one.
        match to_bin(1, 0).unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("DataBar Expanded:"),
                    "width-0 overflow diagnostic must carry symbology tag; got {msg:?}"
                );
                assert!(
                    msg.contains("value 1"),
                    "width-0 overflow diagnostic must echo n=1 (not 4096); got {msg:?}"
                );
                assert!(
                    msg.contains("0 bits"),
                    "width-0 overflow diagnostic must echo width=0 (not 12); got {msg:?}"
                );
                assert!(
                    msg.contains("doesn't fit"),
                    "width-0 overflow diagnostic must use the 'doesn't fit' predicate; got {msg:?}"
                );
            }
            other => panic!("to_bin(1, 0) must yield InvalidData; got {other:?}"),
        }
        assert_eq!(
            to_bin(0, 0).unwrap(),
            "",
            "to_bin(0, 0) must return empty string (0 < 1<<0)"
        );
    }

    #[test]
    fn conv12to40_matches_bwipp_examples() {
        // BWIPP-equivalent: split "000111222333" → [0, 111, 222, 333]
        // → 10-bit each → "0000000000 0001101111 0011011110 0101001101".
        assert_eq!(
            conv12to40(b"000111222333").unwrap(),
            "0000000000000110111100110111100101001101",
        );
        // Trailing/leading zeros.
        assert_eq!(conv12to40(b"000000000000").unwrap(), "0".repeat(40),);
        // Largest 3-digit value 999 = 0b1111100111 → group of all-9s.
        assert_eq!(conv12to40(b"999999999999").unwrap(), "1111100111".repeat(4),);
    }

    /// Stage 11.A8c — upgrade from 4 weak is_err() checks to per-input
    /// diagnostic-substring + body-echo pins. conv12to40 has ONE
    /// rejection arm (line 167-173) but the message has THREE
    /// distinguishable parts:
    ///   * symbology tag "DataBar Expanded:" — kills tag-drop mutants
    ///   * function name "conv12to40" — distinguishes from
    ///     conv13to44's parallel arm (cross-function swap)
    ///   * "12 ASCII digits" — boundary anchor (kills `12 → 11/13`
    ///     mutants in the length check)
    ///   * body echo via {:?} — kills `{digits:?}` drop
    #[test]
    fn conv12to40_rejects_bad_input() {
        // Iterate four inputs that all hit the same arm but for
        // distinct reasons. Each must report the SAME path-specific
        // diagnostic (kills variant-swap mutants).
        for input in [b"" as &[u8], b"12345", b"1234567890123", b"12345678901a"] {
            let err = conv12to40(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("conv12to40({input:?}) must yield InvalidData; got other variant");
            };
            assert!(
                msg.contains("DataBar Expanded:"),
                "diagnostic for {input:?} must carry the symbology tag; got {msg:?}"
            );
            assert!(
                msg.contains("conv12to40"),
                "diagnostic for {input:?} must name conv12to40; got {msg:?}"
            );
            assert!(
                msg.contains("12 ASCII digits"),
                "diagnostic for {input:?} must mention '12 ASCII digits'; got {msg:?}"
            );
            assert!(
                !msg.contains("conv13to44"),
                "conv12to40 diagnostic must not leak conv13to44 name; got {msg:?}"
            );
        }
    }

    #[test]
    fn conv13to44_uses_head_4bits_plus_conv12to40() {
        // Head digit "9" = 4 bits "1001", body "001234567890"
        // → group [001, 234, 567, 890] = [1, 234, 567, 890]
        // → 10 bits each: 0000000001 0011101010 1000110111 1101111010
        // Concatenated: 1001 0000000001 0011101010 1000110111 1101111010
        assert_eq!(
            conv13to44(b"9001234567890").unwrap(),
            "10010000000001001110101010001101111101111010",
        );
        // All zeros: head "0" = "0000", body all zeros = "0"*40.
        assert_eq!(conv13to44(b"0000000000000").unwrap(), "0".repeat(44));
        // First 14 digits of (01)90012345678908: GTIN body 0012345678905
        // -> head=0, body=012345678905
        // groups: 012, 345, 678, 905 -> 12, 345, 678, 905
        // 10-bit each:
        //   12  = 0000001100
        //   345 = 0101011001
        //   678 = 1010100110
        //   905 = 1110001001
        // head "0" = 0000. Total 44 bits.
        assert_eq!(
            conv13to44(b"0012345678905").unwrap(),
            "00000000001100010101100110101001101110001001",
        );
    }

    /// Stage 11.A8c — parallel strengthening of conv13to44's four
    /// weak is_err() checks (mirrors the conv12to40 hardening pattern).
    /// conv13to44 has ONE rejection arm (line 193-199):
    ///   "DataBar Expanded: conv13to44 needs exactly 13 ASCII digits,
    ///    got {:?}"
    ///
    /// Cross-function contamination guard: assert ABSENCE of
    /// "conv12to40" so a mutant that swaps the two functions' arm
    /// bodies is caught. Length-13 boundary anchor pins the `13 → N`
    /// mutants in the length check.
    ///
    /// Note: conv13to44 calls conv12to40 on the body (digits[1..]),
    /// so for a 13-byte input with a non-digit at position 0, the
    /// `conv13to44` arm fires first (length=13 passes, all-digit
    /// check fails on the head). For a 13-byte input with a non-digit
    /// at positions 1..=12, the conv12to40 inner call fires.
    /// "a012345678901" has 13 bytes with 'a' at position 0 → fires
    /// conv13to44's own arm.
    #[test]
    fn conv13to44_rejects_bad_input() {
        for input in [b"" as &[u8], b"12345", b"12345678901234", b"a012345678901"] {
            let err = conv13to44(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("conv13to44({input:?}) must yield InvalidData; got other variant");
            };
            assert!(
                msg.contains("DataBar Expanded:"),
                "diagnostic for {input:?} must carry the symbology tag; got {msg:?}"
            );
            assert!(
                msg.contains("conv13to44"),
                "diagnostic for {input:?} must name conv13to44; got {msg:?}"
            );
            assert!(
                msg.contains("13 ASCII digits"),
                "diagnostic for {input:?} must mention '13 ASCII digits' (kills 13→12/14 \
                 off-by-one); got {msg:?}"
            );
            assert!(
                !msg.contains("conv12to40"),
                "conv13to44 diagnostic must not leak conv12to40 name (cross-function swap); \
                 got {msg:?}"
            );
        }
    }

    #[test]
    fn tab174_row_for_picks_first_threshold_above_value() {
        // Row 0 covers 0..=347.
        assert_eq!(tab174_row_for(0).gs, 0);
        assert_eq!(tab174_row_for(347).gs, 0);
        // Row 1 covers 348..=1387.
        assert_eq!(tab174_row_for(348).gs, 348);
        assert_eq!(tab174_row_for(1387).gs, 348);
        // Row 2 covers 1388..=2947.
        assert_eq!(tab174_row_for(1388).gs, 1388);
        assert_eq!(tab174_row_for(1680).gs, 1388);
        assert_eq!(tab174_row_for(2947).gs, 1388);
        // Row 4 covers 3988..=4191 (the top).
        assert_eq!(tab174_row_for(4191).gs, 3988);
    }

    /// Stage 11.A8c — pin **all seven non-threshold fields** in three
    /// `tab174_row_for` rows. The existing
    /// `tab174_row_for_picks_first_threshold_above_value` test only
    /// asserts on `gs` (`TAB_174[j + 1]`), leaving mutations to the
    /// other six field indices (`+ 2` through `+ 7` for `elo`, `ele`,
    /// `mwo`, `mwe`, `to`, `te`) uncovered: e.g. `elo = TAB_174[j + 2]`
    /// → `+ 3` would silently return `ele` as `elo`, but the existing
    /// test never reads `elo` so it can't detect that.
    ///
    /// TAB_174 layout (5 rows × 8 fields, indexed `j + 0..=j + 7`):
    ///   row 0 (j=0):  [347,  0, 12,  5, 7, 2,  87,   4]
    ///   row 1 (j=8):  [1387, 348, 10, 7, 5, 4,  52,  20]
    ///   row 2 (j=16): [2947, 1388, 8, 9, 4, 5,  30,  52]
    ///   row 3 (j=24): [3987, 2948, 6, 11, 3, 6, 10, 104]
    ///   row 4 (j=32): [4191, 3988, 4, 13, 1, 8,  1, 204]
    ///
    /// Mutations caught (in addition to the gs-only coverage above):
    ///   * Any field-offset shift (`+ 2 → + 3`, `+ 3 → + 4`, etc.).
    ///   * Field-order swap (e.g. mwo ↔ mwe via index swap).
    ///   * TAB_174 constants drift on any of the 21 read cells.
    #[test]
    fn tab174_row_for_pins_all_seven_fields_per_row() {
        // Row 0: gs=0, elo=12, ele=5, mwo=7, mwe=2, to=87, te=4.
        let r0 = tab174_row_for(0);
        assert_eq!(r0.gs, 0, "row 0 gs");
        assert_eq!(r0.elo, 12, "row 0 elo");
        assert_eq!(r0.ele, 5, "row 0 ele");
        assert_eq!(r0.mwo, 7, "row 0 mwo");
        assert_eq!(r0.mwe, 2, "row 0 mwe");
        assert_eq!(r0.to, 87, "row 0 to");
        assert_eq!(r0.te, 4, "row 0 te");

        // Row 2 (mid table, picked via value=1680 which is the first
        // 12-bit chunk from the (01)90012345678908 oracle):
        // gs=1388, elo=8, ele=9, mwo=4, mwe=5, to=30, te=52.
        let r2 = tab174_row_for(1680);
        assert_eq!(r2.gs, 1388, "row 2 gs");
        assert_eq!(r2.elo, 8, "row 2 elo");
        assert_eq!(r2.ele, 9, "row 2 ele");
        assert_eq!(r2.mwo, 4, "row 2 mwo");
        assert_eq!(r2.mwe, 5, "row 2 mwe");
        assert_eq!(r2.to, 30, "row 2 to");
        assert_eq!(r2.te, 52, "row 2 te");

        // Row 4 (top): gs=3988, elo=4, ele=13, mwo=1, mwe=8, to=1, te=204.
        let r4 = tab174_row_for(4000);
        assert_eq!(r4.gs, 3988, "row 4 gs");
        assert_eq!(r4.elo, 4, "row 4 elo");
        assert_eq!(r4.ele, 13, "row 4 ele");
        assert_eq!(r4.mwo, 1, "row 4 mwo");
        assert_eq!(r4.mwe, 8, "row 4 mwe");
        assert_eq!(r4.to, 1, "row 4 to");
        assert_eq!(r4.te, 204, "row 4 te");
    }

    #[test]
    fn pack_12_bits_msb_first() {
        // [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] = 2048
        let v = pack_12_bits(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(v, 0b1000_0000_0000);
        // All ones = 4095
        assert_eq!(pack_12_bits(&[1u8; 12]), 4095);
        // The first 12 bits of binval for (01)90012345678908:
        //   [0,1,1,0,1,0,0,1,0,0,0,0] = 0b011010010000 = 1680
        assert_eq!(pack_12_bits(&[0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0]), 1680,);
    }

    #[test]
    fn extract_data_character_matches_oracle_segments_for_input_a() {
        // For (01)90012345678908, oracle gives binval =
        // [0,1,1,0,1,0,0,1, 0,0,0,0, 0,0,0,0, 0,1,0,0, 1,1,1,0,
        //  1,0,1,0, 1,0,0,0, 1,1,0,1, 1,1,1,1, 0,1,1,1, 1,0,1,0]
        // datalen (data only) = 4, plus checksum slot → 5 total.
        // 12-bit codewords:
        //   d[0] = 0b011010010000 = 1680   (segment_index = 0 even)
        //   d[1] = 0b000000000001 = 1      (segment_index = 1 odd)
        //   d[2] = 0b001110101010 = 938    (segment_index = 2 even)
        //   d[3] = 0b100011011111 = 2271   (segment_index = 3 odd)
        // Hmm — let me re-derive from binval; the simpler way is
        // to pack via `pack_12_bits` on slices of the binval array.
        let binval: &[u8] = &[
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
        ];
        assert_eq!(binval.len(), 48);
        let mut codewords = [0u32; 4];
        for x in 0..4 {
            codewords[x] = pack_12_bits(&binval[x * 12..x * 12 + 12]);
        }
        // Oracle dxw[1..=4] = the 4 data character widths.
        // dxw[0] is the checksum, which we test separately.
        let expected: &[[u8; 8]] = &[
            [1, 2, 1, 3, 5, 2, 2, 1], // x=0 even
            [1, 1, 4, 2, 2, 1, 5, 1], // x=1 odd
            [3, 1, 1, 2, 4, 2, 1, 3], // x=2 even
            [3, 4, 1, 2, 1, 1, 1, 4], // x=3 odd
        ];
        for x in 0..4 {
            let got = extract_data_character(codewords[x], x);
            assert_eq!(
                got, expected[x],
                "data segment {x} mismatch — d={}, expected {:?}, got {:?}",
                codewords[x], expected[x], got
            );
        }
    }

    #[test]
    fn extract_checksum_character_matches_oracle_input_a() {
        // For (01)90012345678908, oracle reports checksum=246 and
        // the resulting widths dxw[0]=[3,1,2,2,1,1,6,1].
        let got = extract_checksum_character(246);
        assert_eq!(got, [3, 1, 2, 2, 1, 1, 6, 1]);
    }

    #[test]
    fn rembits_matches_bwipp_examples() {
        // For (01)+GTIN-14 (method 1, gpfallow=true):
        //   _Bj = 1 + 12 + 1 + 2 + 44 + 0 = 60. Already multiple of 12,
        //   ≥ 48, 5 % 22 ≠ 1. → pad = 0.
        assert_eq!(rembits(60, 22), 0);
        // Smallest possible: 12-bit input → pad to 48.
        assert_eq!(rembits(12, 22), 36);
        assert_eq!(rembits(48, 22), 0);
        // 49 bits → round up to 60, no constraint triggers → pad 11.
        assert_eq!(rembits(49, 22), 11);
        // 13 bits → round up to 24, bump to 48. Pad 35.
        assert_eq!(rembits(13, 22), 35);
        // Hitting the "≡ 1 (mod segments)" bump: _Bj = 12 (1 char)
        // → rounded 12 → bumped to 48 (4 chars) → 4 % 22 = 4 (not 1)
        // → pad = 36. So plain `12, 22` doesn't trip it. Force the
        // case by setting segments=4 and feeding 60 bits:
        //   target = 60. cw_count = 5. 5 % 4 = 1 → bump to 72.
        //   pad = 72 - 60 = 12.
        assert_eq!(rembits(60, 4), 12);
    }

    /// Stage 11.A8c — pin `parse_two_ai_compressed_inputs` strict
    /// validation:
    ///   * exactly 2 elements
    ///   * elements[0].ai == "01"
    ///   * elements[1].ai == expected_second_ai
    ///   * v0 (GTIN) length 14, all digits, leading '9'
    ///   * v1 length 6, all digits, parses as u64
    ///   * returns (gtin[1..13], v1_int).
    ///
    /// Mutations caught:
    ///   * `elements.len() != 2` → `>= 2`: accepts 3+ elements.
    ///   * Drop the `ai == "01"` check: wrong leading AI passes.
    ///   * Drop the `[0] != b'9'` check: non-9 GTINs accepted.
    ///   * Slice `[1..13]` boundary swap: returns wrong substring.
    #[test]
    fn parse_two_ai_compressed_inputs_strict_predicates() {
        use crate::util::gs1::Element;
        let mk = |ai: &str, data: &str| Element {
            ai: ai.into(),
            data: data.into(),
        };

        // Happy path: "01" + "3103" with valid GTIN starting with 9.
        let elements = vec![mk("01", "90012345678905"), mk("3103", "001234")];
        let got = parse_two_ai_compressed_inputs(&elements, "3103");
        assert_eq!(
            got,
            Some((b"001234567890".as_slice(), 1234)),
            "happy path: returns GTIN[1..13] + parsed v1"
        );

        // Wrong number of elements → None.
        assert_eq!(
            parse_two_ai_compressed_inputs(&[mk("01", "90012345678905")], "3103"),
            None,
            "1 element rejected"
        );
        let three = vec![
            mk("01", "90012345678905"),
            mk("3103", "001234"),
            mk("10", "X"),
        ];
        assert_eq!(
            parse_two_ai_compressed_inputs(&three, "3103"),
            None,
            "3+ elements rejected"
        );

        // Wrong leading AI → None.
        let wrong_a0 = vec![mk("02", "90012345678905"), mk("3103", "001234")];
        assert_eq!(parse_two_ai_compressed_inputs(&wrong_a0, "3103"), None);
        // Wrong second AI → None.
        let wrong_a1 = vec![mk("01", "90012345678905"), mk("3104", "001234")];
        assert_eq!(parse_two_ai_compressed_inputs(&wrong_a1, "3103"), None);

        // GTIN not starting with '9' → None (pins the [0] != b'9' check).
        let no9 = vec![mk("01", "80012345678905"), mk("3103", "001234")];
        assert_eq!(parse_two_ai_compressed_inputs(&no9, "3103"), None);
        // GTIN length != 14 → None.
        let short = vec![mk("01", "9001234567890"), mk("3103", "001234")];
        assert_eq!(parse_two_ai_compressed_inputs(&short, "3103"), None);
        // GTIN with non-digit → None.
        let bad_d = vec![mk("01", "9001234567890A"), mk("3103", "001234")];
        assert_eq!(parse_two_ai_compressed_inputs(&bad_d, "3103"), None);
        // v1 length != 6 → None.
        let short_v1 = vec![mk("01", "90012345678905"), mk("3103", "12345")];
        assert_eq!(parse_two_ai_compressed_inputs(&short_v1, "3103"), None);
        // v1 with non-digit → None.
        let bad_v1 = vec![mk("01", "90012345678905"), mk("3103", "12345A")];
        assert_eq!(parse_two_ai_compressed_inputs(&bad_v1, "3103"), None);
    }

    /// Stage 11.A8c — pin every `iso646_byte_bits` arm. iso646 is
    /// the superset alphabet (digits + upper + lower + punctuation +
    /// space + FNC1) so every arm has a distinct constant.
    ///
    /// Per-arm formulas:
    ///   * digit '0'..='9' → (c-43, 5)
    ///   * FNC1 '^' → (15, 5)
    ///   * upper 'A'..='Z' → (c-1, 7)
    ///   * lower 'a'..='z' → (c-7, 7)
    ///   * '!' → (232, 8)
    ///   * '"' → (233, 8)
    ///   * '%'..'/' (11-char block: % & ' ( ) * + , - . /) → (c+197, 8)
    ///   * ':'..'?' (6-char block: : ; < = > ?) → (c+187, 8)
    ///   * '_' → (251, 8)
    ///   * ' ' → (252, 8)
    ///   * else → None.
    #[test]
    fn iso646_byte_bits_per_arm_anchors() {
        // Digit + FNC1 (width 5, same as alphanumeric).
        assert_eq!(iso646_byte_bits(b'0'), Some((5, 5)));
        assert_eq!(iso646_byte_bits(b'9'), Some((14, 5)));
        assert_eq!(iso646_byte_bits(FNC1_SENTINEL_BYTE), Some((15, 5)));

        // Uppercase — c - 1, width 7.
        assert_eq!(iso646_byte_bits(b'A'), Some((64, 7)));
        assert_eq!(iso646_byte_bits(b'Z'), Some((89, 7)));
        // Lowercase — c - 7, width 7.
        assert_eq!(iso646_byte_bits(b'a'), Some((90, 7)));
        assert_eq!(iso646_byte_bits(b'z'), Some((115, 7)));

        // Single-char punct.
        assert_eq!(iso646_byte_bits(b'!'), Some((232, 8)));
        assert_eq!(iso646_byte_bits(b'"'), Some((233, 8)));

        // First punct block '%'..='/' → c + 197 (excluding '$').
        assert_eq!(iso646_byte_bits(b'%'), Some((234, 8)));
        assert_eq!(iso646_byte_bits(b'/'), Some((244, 8)));
        assert_eq!(iso646_byte_bits(b'*'), Some((239, 8)));

        // Second punct block ':'..='?' → c + 187.
        assert_eq!(iso646_byte_bits(b':'), Some((245, 8)));
        assert_eq!(iso646_byte_bits(b'?'), Some((250, 8)));

        // Singletons '_' and ' '.
        assert_eq!(iso646_byte_bits(b'_'), Some((251, 8)));
        assert_eq!(iso646_byte_bits(b' '), Some((252, 8)));

        // Boundary chars in the gaps → None.
        assert_eq!(iso646_byte_bits(b'#'), None, "'#' not in iso646");
        assert_eq!(iso646_byte_bits(b'$'), None, "'$' gap between '\"' and '%'");
        assert_eq!(iso646_byte_bits(b'@'), None, "'@' gap between '?' and 'A'");
        assert_eq!(iso646_byte_bits(b'['), None, "'[' between 'Z' and '_'");
        assert_eq!(iso646_byte_bits(b'\\'), None);
        assert_eq!(iso646_byte_bits(b']'), None);
        assert_eq!(iso646_byte_bits(b'`'), None, "'`' between '_' and 'a'");
        assert_eq!(iso646_byte_bits(b'{'), None);
        assert_eq!(iso646_byte_bits(b'~'), None);
        assert_eq!(iso646_byte_bits(0x00), None);
    }

    /// Stage 11.A8c — pin every `alphanumeric_byte_bits` arm:
    ///   * digit `'0'..='9'` → `(c - 43, 5)` (width 5).
    ///   * FNC1 ('^') → `(15, 5)`.
    ///   * upper `'A'..='Z'` → `(c - 33, 6)` (width 6).
    ///   * `'*'` → `(58, 6)`.
    ///   * punctuation `',' '-' '.' '/'` → `(c + 15, 6)`.
    ///   * else → `None`.
    ///
    /// Mutations caught:
    ///   * Constant-offset drift (`-43` / `-33` / `+15`) shifts the
    ///     value for the relevant arm.
    ///   * Width swap 5↔6 between digit-and-FNC1 arms vs uppercase
    ///     arms — caught by per-arm explicit `.1` assertions.
    ///   * Punctuation arm collapsed (drop one char) — caught by
    ///     checking all four.
    ///   * Catch-all returning `Some(_)` — caught by None for `'!'`,
    ///     `'a'`, `':'`.
    #[test]
    fn alphanumeric_byte_bits_per_arm_anchors() {
        // Digits — width 5, value c - 43.
        assert_eq!(alphanumeric_byte_bits(b'0'), Some((5, 5)));
        assert_eq!(alphanumeric_byte_bits(b'9'), Some((14, 5)));
        assert_eq!(alphanumeric_byte_bits(b'5'), Some((10, 5)));
        // FNC1 sentinel — width 5, value 15 (one past digit '9').
        assert_eq!(alphanumeric_byte_bits(FNC1_SENTINEL_BYTE), Some((15, 5)));
        // Uppercase — width 6, value c - 33.
        assert_eq!(alphanumeric_byte_bits(b'A'), Some((32, 6)));
        assert_eq!(alphanumeric_byte_bits(b'Z'), Some((57, 6)));
        assert_eq!(alphanumeric_byte_bits(b'M'), Some((44, 6)));
        // '*' — width 6, value 58.
        assert_eq!(alphanumeric_byte_bits(b'*'), Some((58, 6)));
        // Punctuation block ',', '-', '.', '/' — width 6, value c+15.
        assert_eq!(alphanumeric_byte_bits(b','), Some((59, 6)));
        assert_eq!(alphanumeric_byte_bits(b'-'), Some((60, 6)));
        assert_eq!(alphanumeric_byte_bits(b'.'), Some((61, 6)));
        assert_eq!(alphanumeric_byte_bits(b'/'), Some((62, 6)));
        // Out of all arms → None.
        assert_eq!(alphanumeric_byte_bits(b'a'), None, "lowercase rejected");
        assert_eq!(alphanumeric_byte_bits(b'!'), None);
        assert_eq!(alphanumeric_byte_bits(b':'), None, "':' above digit range");
        assert_eq!(alphanumeric_byte_bits(b'@'), None, "'@' below 'A'");
        assert_eq!(alphanumeric_byte_bits(b'['), None, "'[' above 'Z'");
        assert_eq!(alphanumeric_byte_bits(b' '), None);
        assert_eq!(alphanumeric_byte_bits(b'+'), None, "'+' not in punct block");
    }

    /// Stage 11.A8c — pin `numeric_pair_bits` formula and FNC1
    /// branch:
    ///   * digit('0'..='9') → c - '0'.
    ///   * digit(FNC1_SENTINEL_BYTE='^') → 10.
    ///   * `(g + 8, 7)` where g = d0 * 11 + d1.
    ///
    /// Mutations caught:
    ///   * `g + 8` → `g + 7` / `g + 9` shifts every output by 1.
    ///   * `digit(c0) * 11 + digit(c1)` → `* 10 +` collapses many
    ///     pairs (caught by ("00", "10") inequality).
    ///   * `digit('^')` → `Some(9)` would collide with digit '9'.
    ///   * Width `7` → `6` or `8`: every test asserts width=7.
    ///   * Either non-digit input must still return None.
    #[test]
    fn numeric_pair_bits_formula_and_fnc1_branch() {
        // Digit pair anchors: (g + 8, 7) where g = d0*11 + d1.
        assert_eq!(numeric_pair_bits(b'0', b'0'), Some((8, 7)));
        assert_eq!(numeric_pair_bits(b'0', b'1'), Some((9, 7)));
        assert_eq!(numeric_pair_bits(b'1', b'0'), Some((19, 7)));
        assert_eq!(numeric_pair_bits(b'9', b'9'), Some((116, 7)));

        // FNC1 in either slot maps to digit-value 10.
        assert_eq!(
            numeric_pair_bits(b'0', FNC1_SENTINEL_BYTE),
            Some((18, 7)),
            "d1=FNC1 → 0*11+10+8=18"
        );
        assert_eq!(
            numeric_pair_bits(FNC1_SENTINEL_BYTE, b'0'),
            Some((118, 7)),
            "d0=FNC1 → 10*11+0+8=118"
        );
        assert_eq!(
            numeric_pair_bits(FNC1_SENTINEL_BYTE, FNC1_SENTINEL_BYTE),
            Some((128, 7)),
            "FNC1/FNC1 → 10*11+10+8=128"
        );

        // Non-digit chars in either slot → None.
        assert_eq!(numeric_pair_bits(b'A', b'0'), None);
        assert_eq!(numeric_pair_bits(b'0', b'A'), None);
        assert_eq!(numeric_pair_bits(b' ', b' '), None);
        assert_eq!(numeric_pair_bits(b'/', b'0'), None, "'/' just below '0'");
        assert_eq!(numeric_pair_bits(b'9', b':'), None, "':' just above '9'");
    }

    /// `alphanumeric_byte_bits(c)` is the per-byte lookup BWIPP's
    /// general-purpose encoder calls in alphanumeric mode. It returns
    /// `(value, bit_width)` per arm:
    ///
    /// | byte range  | formula           | width |
    /// |-------------|-------------------|-------|
    /// | '0'..='9'   | c - 43            | 5     |
    /// | '^' (FNC1)  | 15                | 5     |
    /// | 'A'..='Z'   | c - 33            | 6     |
    /// | '*'         | 58                | 6     |
    /// | ',-./'      | c + 15            | 6     |
    /// | else        | None              | -     |
    ///
    /// No direct test exists — every BWIPP method-* encoder calls this
    /// transitively through `encode_general_purpose`, so a mutant
    /// shifting `c - 43` to `c - 42` (or `width 5` to `4`) only fails
    /// when the path is hit by a golden corpus row.
    ///
    /// Anchors cover every arm at its endpoints + boundary characters
    /// just outside each range.
    #[test]
    fn alphanumeric_byte_bits_per_arm_with_boundary_discriminators() {
        // ---- Digit arm: '0'..='9' → c - 43, width 5.
        // '0' (48) → 5; '9' (57) → 14.
        assert_eq!(
            alphanumeric_byte_bits(b'0'),
            Some((5, 5)),
            "'0' → ('0'-43, 5) = (5, 5)"
        );
        assert_eq!(alphanumeric_byte_bits(b'5'), Some((10, 5)), "'5' → (10, 5)");
        assert_eq!(
            alphanumeric_byte_bits(b'9'),
            Some((14, 5)),
            "'9' → ('9'-43, 5) = (14, 5)"
        );
        // Just outside digit range, on the low side: '/' is in the
        // `,-./` arm — NOT the digit arm. Catches a `c <= b'9'` mutant
        // that swallows '/' as a digit.
        assert_eq!(
            alphanumeric_byte_bits(b'/'),
            Some((62, 6)),
            "'/' lands in the ,-./ arm, NOT the digit arm"
        );

        // ---- FNC1 sentinel arm: '^' → (15, 5).
        // The sentinel is the only u8 in this arm. A mutant that
        // drops the arm makes this return None, which the next assertion
        // catches.
        assert_eq!(
            alphanumeric_byte_bits(FNC1_SENTINEL_BYTE),
            Some((15, 5)),
            "FNC1 sentinel '^' → (15, 5)"
        );

        // ---- Letter arm: 'A'..='Z' → c - 33, width 6.
        // 'A' (65) → 32; 'Z' (90) → 57.
        assert_eq!(
            alphanumeric_byte_bits(b'A'),
            Some((32, 6)),
            "'A' → ('A'-33, 6) = (32, 6)"
        );
        assert_eq!(
            alphanumeric_byte_bits(b'M'),
            Some((44, 6)),
            "'M' (77) → (44, 6)"
        );
        assert_eq!(
            alphanumeric_byte_bits(b'Z'),
            Some((57, 6)),
            "'Z' → ('Z'-33, 6) = (57, 6)"
        );

        // ---- Asterisk arm: '*' → (58, 6).
        // '*' (42) is NOT contiguous with the others — must stay its
        // own arm. (58, 6) is also exactly one above 'Z' so a mutant
        // that lumps '*' into the letter arm would compute '*'-33 = 9
        // not 58 — discriminator anchor.
        assert_eq!(
            alphanumeric_byte_bits(b'*'),
            Some((58, 6)),
            "'*' → (58, 6) — own arm, distinct from letter range"
        );

        // ---- Punct arm: ',', '-', '.', '/' → c + 15, width 6.
        // ',' (44) → 59; '-' (45) → 60; '.' (46) → 61; '/' (47) → 62.
        // All four punct chars are contiguous in ASCII so a `c..=c`
        // mutant might collapse them.
        assert_eq!(alphanumeric_byte_bits(b','), Some((59, 6)), "',' → (59, 6)");
        assert_eq!(alphanumeric_byte_bits(b'-'), Some((60, 6)), "'-' → (60, 6)");
        assert_eq!(alphanumeric_byte_bits(b'.'), Some((61, 6)), "'.' → (61, 6)");
        assert_eq!(alphanumeric_byte_bits(b'/'), Some((62, 6)), "'/' → (62, 6)");

        // ---- None arm: rejected bytes.
        // Lowercase letters — outside the alphanumeric alphabet.
        assert_eq!(alphanumeric_byte_bits(b'a'), None, "'a' lowercase: None");
        assert_eq!(alphanumeric_byte_bits(b'z'), None, "'z' lowercase: None");
        // Space, special punct: outside both arms.
        assert_eq!(alphanumeric_byte_bits(b' '), None, "space: None");
        assert_eq!(alphanumeric_byte_bits(b'!'), None, "'!': None");
        assert_eq!(alphanumeric_byte_bits(b'_'), None, "'_': None");
        // '+' (43) is just below the comma arm.
        assert_eq!(
            alphanumeric_byte_bits(b'+'),
            None,
            "'+' (43) is one below ','; must be None"
        );
        // '0' - 1 = '/'  but '/' is in the punct arm; check ':' just
        // above '9' for the upper digit boundary.
        assert_eq!(
            alphanumeric_byte_bits(b':'),
            None,
            "':' (58) just above '9'; must be None"
        );
        // '@' just below 'A'.
        assert_eq!(
            alphanumeric_byte_bits(b'@'),
            None,
            "'@' (64) just below 'A'; must be None"
        );
        // '[' just above 'Z'.
        assert_eq!(
            alphanumeric_byte_bits(b'['),
            None,
            "'[' (91) just above 'Z'; must be None"
        );
        // NUL and DEL.
        assert_eq!(alphanumeric_byte_bits(0), None);
        assert_eq!(alphanumeric_byte_bits(0x7F), None);

        // ---- Distinctness invariant within an arm:
        // For digits '0'..='9', each byte's value is the previous + 1.
        for w in b'0'..b'9' {
            let (v_w, _) = alphanumeric_byte_bits(w).unwrap();
            let (v_next, _) = alphanumeric_byte_bits(w + 1).unwrap();
            assert_eq!(
                v_next,
                v_w + 1,
                "digit-arm value(c+1) must equal value(c)+1 (c={})",
                w as char
            );
        }
        // Same for letters 'A'..='Z'.
        for w in b'A'..b'Z' {
            let (v_w, _) = alphanumeric_byte_bits(w).unwrap();
            let (v_next, _) = alphanumeric_byte_bits(w + 1).unwrap();
            assert_eq!(
                v_next,
                v_w + 1,
                "letter-arm value(c+1) must equal value(c)+1 (c={})",
                w as char
            );
        }
    }

    #[test]
    fn assemble_binval_matches_oracle_input_a() {
        // Reproduce BWIPP's `binval` for (01)90012345678908 (method 1).
        let method_bits = vec![1u8];
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Expanded conv13to44 13→44-bit conversion path.
        let cdf_str = conv13to44(b"9001234567890").expect(
            "conv13to44(b\"9001234567890\") (DataBar Expanded 13-digit GTIN body → 44-bit cdf binary; method-1 helper) must succeed",
        );
        let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
        let binval = assemble_binval(
            0,
            &method_bits,
            2,
            &cdf,
            &[],
            CharsetMode::Numeric,
            DEFAULT_SEGMENTS,
        );
        // Oracle binval for this input (48 bits = 4 data codewords).
        let want: &[u8] = &[
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
        ];
        assert_eq!(binval, want);
    }

    /// Stage 11.A8c — pin the **pipeline orchestration** of
    /// `encode_to_dxw_and_seq`. The existing
    /// `assemble_binval_matches_oracle_input_a`,
    /// `extract_data_character_matches_oracle_segments_for_input_a`,
    /// `extract_checksum_character_matches_oracle_input_a`,
    /// `finder_sequence_returns_bwipp_row`, and `compute_checksum_matches_oracle_input_a`
    /// tests each pin one stage of the dxw + seq pipeline in isolation;
    /// none of them verifies the orchestration (data_chars derivation,
    /// dxw[0]=checksum-first ordering, and the total_chars = dxw.len()
    /// return).
    ///
    /// Mutations caught (in addition to the per-stage tests above):
    ///   * `dxw.push(extract_checksum_character(checksum))` before vs
    ///     after `dxw.extend(data_dxw)`: a swap would put the checksum
    ///     at the end (dxw[4]) instead of the front (dxw[0]).
    ///   * `data_chars = binval.len() / 12` → `* 12`: would crash or
    ///     produce wildly wrong character counts.
    ///   * `dxw.len()` for `total_chars` vs `data_chars`: would return
    ///     4 instead of 5 (missing the checksum slot).
    ///   * `extract_data_character(d, x)` x-arg → `0`: every data
    ///     char would use even-parity layout regardless of segment_index.
    ///   * `compute_checksum(&data_dxw, seq)` arg swap or wrong array.
    ///
    /// Same inputs as `assemble_binval_matches_oracle_input_a`
    /// ((01)90012345678908, method 1, segments=22), so the expected
    /// dxw widths come byte-for-byte from the existing
    /// `extract_data_character_matches_oracle_segments_for_input_a`
    /// and `extract_checksum_character_matches_oracle_input_a` tests.
    #[test]
    fn encode_to_dxw_and_seq_pipeline_for_input_a() {
        let method_bits = vec![1u8];
        let cdf_str = conv13to44(b"9001234567890").expect("conv13to44 must succeed");
        let cdf: Vec<u8> = cdf_str.bytes().map(|b| b - b'0').collect();
        let method = MethodOutput {
            method_bits,
            vlf_len: 2,
            cdf,
            // gpf_prefix_bytes + consumed_ais aren't read by
            // encode_to_dxw_and_seq, only by the upstream method
            // dispatcher — fill with empties.
            gpf_prefix_bytes: vec![],
            consumed_ais: 0,
        };
        let (dxw, seq, total_chars) = encode_to_dxw_and_seq(
            false, // linkage = 0
            &method,
            &[], // gpf_bits empty
            CharsetMode::Numeric,
            DEFAULT_SEGMENTS,
        )
        .expect("input A (data_chars=4) is within the 2..=21 capacity");

        // total_chars = data_chars + 1 (the prepended checksum slot)
        // = 4 data + 1 checksum = 5.
        assert_eq!(total_chars, 5, "data_chars=4 + 1 checksum = 5 total");
        assert_eq!(dxw.len(), 5);

        // dxw[0] is the CHECKSUM character — must come before data.
        // From extract_checksum_character_matches_oracle_input_a:
        // checksum=246 → [3, 1, 2, 2, 1, 1, 6, 1].
        assert_eq!(
            dxw[0],
            [3, 1, 2, 2, 1, 1, 6, 1],
            "dxw[0] must be the checksum character (prepended via push BEFORE extend)"
        );

        // dxw[1..=4] are the 4 data characters in order.
        // From extract_data_character_matches_oracle_segments_for_input_a:
        assert_eq!(
            dxw[1],
            [1, 2, 1, 3, 5, 2, 2, 1],
            "dxw[1] = data seg 0 (even parity)"
        );
        assert_eq!(
            dxw[2],
            [1, 1, 4, 2, 2, 1, 5, 1],
            "dxw[2] = data seg 1 (odd parity)"
        );
        assert_eq!(
            dxw[3],
            [3, 1, 1, 2, 4, 2, 1, 3],
            "dxw[3] = data seg 2 (even parity)"
        );
        assert_eq!(
            dxw[4],
            [3, 4, 1, 2, 1, 1, 1, 4],
            "dxw[4] = data seg 3 (odd parity)"
        );

        // Finder sequence for 4 data chars (bracket 1) = [0, 3, 2].
        // From finder_sequence_returns_bwipp_row.
        assert_eq!(
            seq,
            &[0u8, 3, 2],
            "finder_sequence(4) = bracket 1 [0, 3, 2]"
        );
    }

    #[test]
    fn encode_method_1_falls_through_for_non_gtin_inputs() {
        // Stage 11.A8c — upgrade 2 discriminant-only `matches!`
        // checks on `Ok(None)` to `unwrap_or_else` + explicit Option
        // pattern that surfaces the unexpected payload's
        // `method_bits` + `consumed_ais` fields on regression.
        // A bare `matches!(_, Ok(None))` swallows the failure detail
        // — if a mutation made encode_method_1 incorrectly emit a
        // `Some(Method1Match{...})`, the test would still report
        // `assertion failed` without showing WHICH method-1 match
        // was wrongly produced.
        //
        // Not AI 01.
        let els = vec![crate::util::gs1::Element {
            ai: "10".to_string(),
            data: "ABC".to_string(),
        }];
        let result = encode_method_1(&els)
            .expect("encode_method_1(non-AI-01) must not error, only return None");
        assert!(
            result.is_none(),
            "AI=10 should NOT match method 1, got Some({result:?}) — method 1 only handles AI=01"
        );
        // AI 01 but wrong length.
        let els = vec![crate::util::gs1::Element {
            ai: "01".to_string(),
            data: "12345".to_string(),
        }];
        let result = encode_method_1(&els)
            .expect("encode_method_1(AI=01 wrong length) must not error, only return None");
        assert!(
            result.is_none(),
            "AI=01 with 5-digit body should NOT match method 1, got Some({result:?}) — method 1 requires exactly 14 digits"
        );
        // Multi-AI with (01)+GTIN first: method 1 MATCHES, consuming
        // only the (01); the trailing AIs go into gpf.
        let els = vec![
            crate::util::gs1::Element {
                ai: "01".to_string(),
                data: "90012345678908".to_string(),
            },
            crate::util::gs1::Element {
                ai: "10".to_string(),
                data: "ABC".to_string(),
            },
        ];
        // Stage 11.A8c (cont) — `.unwrap().unwrap()` →
        // `.expect(...).expect(...)` naming the method-1 multi-AI
        // partial-consume path: (01)+GTIN matches method 1, consumes
        // only the (01) element, leaves trailing AIs for gpf.
        let m = encode_method_1(&els)
            .expect("encode_method_1((01)+GTIN multi-AI) (DataBar Expanded method-1 partial-consume; outer Result Ok) must succeed")
            .expect("encode_method_1((01)+GTIN multi-AI) (DataBar Expanded method-1 must match; inner Option Some with method_bits=[1] + consumed_ais=1)");
        assert_eq!(m.method_bits, vec![1]);
        assert_eq!(m.consumed_ais, 1);
    }

    #[test]
    fn encode_input_a_end_to_end_matches_oracle_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Expanded end-to-end Input-A oracle path:
        // method-1 + 14-digit GTIN → 59-element sbs bwip-js golden.
        let pat = encode("(01)90012345678908", false).expect(
            "encode(\"(01)90012345678908\", linkage=false) (DataBar Expanded end-to-end Input-A oracle: method-1 + 14-digit GTIN → 59-element sbs) must succeed",
        );
        let want: &[u8] = &[
            1, 3, 1, 2, 2, 1, 1, 6, 1, 1, 8, 4, 1, 1, 1, 2, 1, 3, 5, 2, 2, 1, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 4, 6, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
        assert!(pat.text.is_none());
    }

    #[test]
    fn encode_method_0100_matches_oracle() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Expanded method-0100 (01+3103) oracle path:
        // 14-digit GTIN + 6-digit weight → 66-element sbs (datalen=6,
        // 3 finders + 6 chars).
        let pat = encode("(01)90012345678908(3103)032000", false).expect(
            "encode(\"(01)90012345678908(3103)032000\", linkage=false) (DataBar Expanded method-0100 oracle: 01+3103 net-weight → 66-element sbs, datalen=6, 3 finders + 6 chars) must succeed",
        );
        // Oracle 66-element sbs (datalen=6, 3 finders + 6 chars).
        let want: &[u8] = &[
            1, 1, 1, 3, 2, 4, 1, 2, 3, 1, 8, 4, 1, 1, 3, 4, 1, 2, 2, 3, 1, 1, 1, 1, 4, 1, 3, 2, 2,
            3, 1, 1, 4, 6, 3, 2, 1, 1, 3, 3, 3, 1, 3, 1, 4, 1, 3, 1, 2, 3, 2, 3, 6, 4, 1, 1, 2, 2,
            2, 1, 4, 2, 3, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_0101_3202_matches_oracle() {
        let pat = encode("(01)90012345678908(3202)001234", false).unwrap();
        let want: &[u8] = &[
            1, 1, 1, 2, 1, 4, 3, 3, 2, 1, 8, 4, 1, 1, 1, 2, 3, 3, 1, 4, 2, 1, 1, 1, 4, 1, 3, 2, 2,
            3, 1, 1, 4, 6, 3, 2, 1, 1, 3, 3, 3, 1, 3, 1, 4, 1, 1, 1, 4, 3, 2, 3, 6, 4, 1, 1, 1, 3,
            3, 2, 2, 1, 1, 4, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_0101_3203_uses_plus_10000_offset() {
        let pat = encode("(01)90012345678908(3203)001234", false).unwrap();
        let want: &[u8] = &[
            1, 1, 1, 3, 3, 5, 1, 1, 2, 1, 8, 4, 1, 1, 1, 2, 3, 3, 1, 4, 2, 1, 1, 1, 4, 1, 3, 2, 2,
            3, 1, 1, 4, 6, 3, 2, 1, 1, 3, 3, 3, 1, 3, 1, 4, 1, 2, 1, 1, 3, 4, 3, 6, 4, 1, 1, 1, 3,
            3, 1, 2, 1, 5, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_0100_falls_through_when_weight_too_large() {
        // (3103) value > 32767 → 0100 declines. Method 0111 also
        // rejects (value > 99999). Method 1 takes the (01) and the
        // (3103) trails into gpf. Should produce a valid symbol via
        // method 1 + general-purpose tail.
        // Stage 11.A8c (cont) — descriptive label naming fall-through path.
        let pat = encode("(01)90012345678908(3103)999999", false).unwrap();
        assert!(
            !pat.bars.is_empty(),
            "encode(\"(01)90012345678908(3103)999999\") (weight 999999 > 32767 → 0100 declines → falls through to method 1 + gpf tail) must produce non-empty DataBar Expanded bars; got len={}",
            pat.bars.len()
        );
        assert_eq!(pat.bars.first(), Some(&1)); // start margin
    }

    #[test]
    fn encode_method_0101_falls_through_when_gtin_does_not_start_with_9() {
        // GTIN doesn't start with '9' → 0101's precondition fails.
        // Bounces to method 1, which consumes the (01) and lets the
        // (3202) trail into gpf.
        // Stage 11.A8c (cont) — descriptive label naming fall-through path.
        let pat = encode("(01)10012345678907(3202)001234", false).unwrap();
        assert!(
            !pat.bars.is_empty(),
            "encode(\"(01)10012345678907(3202)001234\") (GTIN starts with '1' not '9' → 0101 precondition fails → falls through to method 1 + gpf tail) must produce non-empty DataBar Expanded bars; got len={}",
            pat.bars.len()
        );
    }

    #[test]
    fn encode_method_0111000_310x_no_date_matches_oracle() {
        // (01)+(3103)099999 → method 0111000 (310x, no date, weight > 32767 so
        // 0100 declines and falls through).
        let pat = encode("(01)90012345678908(3103)099999", false).unwrap();
        let want: &[u8] = &[
            1, 2, 2, 5, 1, 2, 3, 1, 1, 1, 8, 4, 1, 1, 1, 3, 2, 1, 3, 4, 1, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 6, 4, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 5, 4,
            1, 1, 1, 2, 2, 1, 2, 2, 1, 3, 1, 3, 4, 1, 1, 1, 8, 2, 3, 2, 2, 2, 4, 1, 1, 4, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_0111001_320x_no_date_matches_oracle() {
        // (01)+(3203)099999 → method 0111001 (320x, no date).
        let pat = encode("(01)90012345678908(3203)099999", false).unwrap();
        let want: &[u8] = &[
            1, 3, 3, 1, 1, 3, 2, 3, 1, 1, 8, 4, 1, 1, 3, 1, 1, 3, 2, 4, 1, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 6, 4, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 5, 4,
            1, 1, 1, 2, 2, 1, 2, 2, 1, 3, 1, 3, 4, 1, 1, 1, 8, 2, 3, 2, 2, 2, 4, 1, 1, 4, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_0111000_310x_with_date_11_matches_oracle() {
        // (01)+(3103)001750(11)250101 → method 0111000 (310x with
        // production-date AI 11 — encoded as YY=25, MM=01, DD=01).
        let pat = encode("(01)90012345678908(3103)001750(11)250101", false).unwrap();
        let want: &[u8] = &[
            1, 2, 2, 4, 3, 3, 1, 1, 1, 1, 8, 4, 1, 1, 1, 3, 2, 1, 3, 4, 1, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 6, 4, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 3, 1,
            1, 2, 1, 4, 2, 3, 4, 2, 2, 1, 1, 1, 1, 5, 1, 1, 8, 2, 3, 2, 4, 4, 2, 1, 1, 2, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_stacked_matches_oracle_for_input_a() {
        // (01)90012345678908 stacked → 2 rows × 102 modules wide.
        // Verify every strip's module bits match the oracle pixs.
        let bm = encode_stacked("(01)90012345678908", false).unwrap();
        assert_eq!(bm.width(), 102);
        // Strip layout: row0 (34) + seps[0] (1) + inter_sep (1) +
        // seps[1] (1) + row1 (34) = 71 rows total.
        assert_eq!(bm.height(), 71);

        // Oracle strip patterns (each 102 modules wide).
        let row0: &[u8] = &[
            0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0,
            0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1,
        ];
        let sep0: &[u8] = &[
            0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1,
            1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0,
            1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0,
        ];
        let inter_sep: &[u8] = &[
            0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
            0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0,
        ];
        let sep1: &[u8] = &[
            0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let row1: &[u8] = &[
            0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1,
            0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        // Verify row0 (y = 0..=33).
        for y in 0..34 {
            for (x, &expected) in row0.iter().enumerate() {
                assert_eq!(bm.get(x, y) as u8, expected, "row0 y={y} x={x}");
            }
        }
        // sep0 at y=34, inter_sep at y=35, sep1 at y=36.
        for (x, &expected) in sep0.iter().enumerate() {
            assert_eq!(bm.get(x, 34) as u8, expected, "sep0 x={x}");
        }
        for (x, &expected) in inter_sep.iter().enumerate() {
            assert_eq!(bm.get(x, 35) as u8, expected, "inter_sep x={x}");
        }
        for (x, &expected) in sep1.iter().enumerate() {
            assert_eq!(bm.get(x, 36) as u8, expected, "sep1 x={x}");
        }
        // row1 at y=37..=70.
        for y in 37..71 {
            for (x, &expected) in row1.iter().enumerate() {
                assert_eq!(bm.get(x, y) as u8, expected, "row1 y={y} x={x}");
            }
        }
    }

    #[test]
    fn encode_method_01100_392x_matches_oracle() {
        // (01)+(3922)1234 → method 01100. Value goes through the
        // general-purpose encoder (numeric mode for pure digits).
        let pat = encode("(01)90012345678908(3922)1234", false).unwrap();
        let want: &[u8] = &[
            1, 1, 1, 5, 4, 2, 1, 2, 1, 1, 8, 4, 1, 1, 1, 2, 3, 5, 1, 1, 2, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 6, 4, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 1, 1,
            2, 4, 1, 1, 5, 2, 1, 3, 2, 5, 1, 1, 2, 2, 1, 1, 8, 2, 3, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_01100_with_trailing_lot_ai_matches_oracle() {
        // (01)+(3922)1234+(10)abc — method 01100 with trailing
        // variable AI. BWIPP inserts FNC1 between val[1] and the
        // (10) digits inside the dispatcher; the trailing-AI loop
        // then appends "10abc".
        let pat = encode("(01)90012345678908(3922)1234(10)abc", false).unwrap();
        let want: &[u8] = &[
            1, 1, 4, 1, 3, 2, 1, 4, 1, 1, 8, 4, 1, 1, 4, 3, 1, 4, 1, 1, 1, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 5, 6, 2, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 1, 1,
            2, 4, 1, 1, 5, 2, 1, 6, 3, 1, 1, 1, 1, 3, 1, 1, 8, 2, 3, 1, 4, 1, 3, 2, 4, 1, 1, 1, 1,
            1, 3, 3, 3, 3, 2, 3, 4, 6, 1, 1, 1, 1, 1, 3, 3, 3, 4, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_01101_393x_matches_oracle() {
        // (01)+(3932)84012345 → method 01101. 3-digit currency code
        // "840" packed into 10-bit cdf prefix; the remaining "12345"
        // goes through the general-purpose encoder.
        let pat = encode("(01)90012345678908(3932)84012345", false).unwrap();
        let want: &[u8] = &[
            1, 3, 1, 1, 2, 4, 2, 2, 2, 1, 8, 4, 1, 1, 3, 1, 1, 5, 2, 2, 1, 2, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 6, 4, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 2, 1,
            2, 2, 1, 1, 4, 4, 1, 2, 5, 3, 1, 1, 3, 1, 1, 1, 8, 2, 3, 1, 3, 4, 2, 3, 2, 1, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_stacked_three_rows_with_reversed_middle_row() {
        // Longer payload → 3 rows. The middle row (r=1) gets
        // reversed by BWIPP's segments % 4 == 0 + r odd + length
        // matches row 0 rule. Verifies that codepath too.
        let bm = encode_stacked("(01)90012345678908(11)250101(10)xyz", false).unwrap();
        assert_eq!(bm.width(), 102);
        // 3 rows × 34 + 6 sep strips = 102 + 6 = 108.
        assert_eq!(bm.height(), 108);
        let row0_first15: &[u8] = &[0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0];
        let row1_first15_reversed: &[u8] = &[1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0];
        let row2_first15: &[u8] = &[0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0];
        for (x, &b) in row0_first15.iter().enumerate() {
            assert_eq!(bm.get(x, 0) as u8, b, "row0 x={x}");
        }
        // Row 1 starts at y = 34 + 1 + 1 + 1 = 37.
        for (x, &b) in row1_first15_reversed.iter().enumerate() {
            assert_eq!(bm.get(x, 37) as u8, b, "row1 (reversed) x={x}");
        }
        // Row 2 starts at y = 37 + 34 + 1 + 1 + 1 = 74.
        for (x, &b) in row2_first15.iter().enumerate() {
            assert_eq!(bm.get(x, 74) as u8, b, "row2 x={x}");
        }
    }

    #[test]
    fn encode_method_00_pure_numeric_sscc_matches_oracle() {
        // (00)123456789012345675 → method 00 with pure numeric gpf
        // (an SSCC-18). Exercises the numeric-only encoder path
        // including its trailing alignment.
        let pat = encode("(00)123456789012345675", false).unwrap();
        let want: &[u8] = &[
            1, 2, 1, 3, 1, 1, 2, 4, 3, 1, 8, 4, 1, 1, 2, 5, 1, 5, 1, 1, 1, 1, 1, 2, 5, 3, 1, 1, 3,
            1, 1, 1, 6, 4, 3, 3, 4, 1, 1, 2, 2, 3, 1, 1, 4, 3, 1, 1, 2, 1, 4, 3, 6, 4, 1, 1, 1, 1,
            1, 2, 2, 3, 3, 4, 1, 1, 1, 2, 3, 5, 1, 3, 1, 1, 8, 2, 3, 4, 2, 1, 4, 1, 1, 1, 3, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_1_with_uppercase_trailing_ai() {
        // (01)+(10)ABC123 — method 1 + general-purpose trailing.
        // The trailing AI uses uppercase letters that go through
        // alphanumeric mode (no iso646 escape).
        let pat = encode("(01)90012345678908(10)ABC123", false).unwrap();
        let want: &[u8] = &[
            1, 3, 2, 3, 1, 1, 3, 3, 1, 1, 8, 4, 1, 1, 1, 2, 1, 3, 5, 2, 2, 1, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 5, 6, 2, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 3, 3,
            2, 2, 1, 4, 1, 1, 1, 2, 3, 1, 6, 1, 2, 1, 1, 1, 8, 2, 3, 2, 1, 1, 2, 1, 6, 1, 3, 2, 2,
            1, 3, 2, 1, 1, 5, 3, 4, 6, 1, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_1_with_three_ais_date_then_lot() {
        // (01)+(11)YYMMDD+(10)lot — common GTIN + production date +
        // lot-number combo. Exercises trailing-AIs ordering with
        // an FNC1 separator after the variable-length (10).
        let pat = encode("(01)90012345678908(11)250101(10)xyz", false).unwrap();
        let want: &[u8] = &[
            1, 1, 3, 2, 1, 1, 1, 4, 4, 1, 8, 4, 1, 1, 1, 2, 1, 3, 5, 2, 2, 1, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 5, 6, 2, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 4, 1,
            1, 4, 1, 4, 1, 1, 1, 5, 1, 2, 2, 1, 2, 3, 1, 1, 8, 2, 3, 5, 2, 1, 1, 1, 3, 2, 2, 1, 1,
            4, 2, 2, 1, 5, 1, 3, 2, 8, 1, 1, 1, 5, 4, 1, 1, 1, 1, 3, 2, 2, 4, 1, 1, 3, 1, 3, 1, 1,
            9, 2, 2, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_method_1_with_iso646_punctuation_trailing_ai() {
        // (01)+(10)A!b%c — mixes uppercase, iso646-only punctuation
        // ('!', '%'), and lowercase. Exercises the alphanumeric →
        // iso646 latch.
        let pat = encode("(01)90012345678908(10)A!b%c", false).unwrap();
        let want: &[u8] = &[
            1, 1, 1, 1, 2, 3, 5, 3, 1, 1, 8, 4, 1, 1, 4, 1, 1, 2, 1, 4, 1, 3, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 5, 6, 2, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 3, 3,
            2, 2, 1, 4, 1, 1, 1, 2, 1, 1, 7, 1, 3, 1, 1, 1, 8, 2, 3, 2, 1, 4, 3, 1, 1, 2, 3, 3, 2,
            1, 3, 1, 4, 1, 2, 3, 4, 6, 1, 1, 5, 2, 2, 2, 1, 1, 3, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn encode_handles_method_1_with_trailing_variable_ai_via_general_purpose() {
        // (01)+(10)abc — method 1 consumes the (01), method 00's
        // general-purpose encoder packs the trailing (10)abc bytes
        // into gpf. This is the most common 2-AI DataBar Expanded
        // shape (GTIN + lot number).
        let pat = encode("(01)90012345678908(10)abc", false).unwrap();
        // Oracle 102-element sbs from
        // node tools/oracle-databarexpanded.js "(01)90012345678908(10)abc"
        let want: &[u8] = &[
            1, 3, 1, 3, 2, 1, 1, 3, 3, 1, 8, 4, 1, 1, 1, 2, 1, 3, 5, 2, 2, 1, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 5, 6, 2, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 4, 3,
            1, 2, 1, 4, 1, 1, 3, 3, 5, 1, 1, 2, 1, 1, 1, 1, 8, 2, 3, 1, 3, 5, 1, 1, 3, 2, 1, 3, 1,
            4, 1, 4, 1, 1, 2, 3, 4, 6, 1, 1, 1, 1,
        ];
        assert_eq!(pat.bars, want);
    }

    #[test]
    fn finder_sequence_returns_bwipp_row() {
        // data_chars=2 → bracket 0 → [0, 1].
        assert_eq!(finder_sequence(2), &[0, 1]);
        // data_chars=3 → bracket 0 → same [0, 1].
        assert_eq!(finder_sequence(3), &[0, 1]);
        // data_chars=4 → bracket 1 → [0, 3, 2]. Used for (01)+GTIN-14.
        assert_eq!(finder_sequence(4), &[0, 3, 2]);
        // data_chars=5 → bracket 1 → same [0, 3, 2]. Used for the
        // 2-AI compressed methods (01)+(3103), (01)+(3202|3203).
        assert_eq!(finder_sequence(5), &[0, 3, 2]);
        // data_chars=14 → bracket 6 → [0,1,2,3,4,5,6,7].
        assert_eq!(finder_sequence(14), &[0, 1, 2, 3, 4, 5, 6, 7]);
        // Largest supported: data_chars=21 → bracket 9 → length 11.
        assert_eq!(finder_sequence(21), &[0, 1, 2, 3, 4, 7, 6, 9, 8, 11, 10]);
    }

    #[test]
    fn assemble_sbs_matches_oracle_input_a() {
        // Reproduce dxw for (01)90012345678908 and verify sbs.
        let binval: &[u8] = &[
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
        ];
        let mut dxw: Vec<[u8; 8]> = Vec::with_capacity(5);
        // dxw[0] is the checksum, computed from the data dxw + seq.
        let data_dxw: Vec<[u8; 8]> = (0..4)
            .map(|x| {
                let d = pack_12_bits(&binval[x * 12..x * 12 + 12]);
                extract_data_character(d, x)
            })
            .collect();
        // 4 data codewords (binval is 48 bits = 4 × 12), so
        // BWIPP's pre-checksum-reassign datalen is 4.
        let seq = finder_sequence(4);
        let checksum = compute_checksum(&data_dxw, seq);
        dxw.push(extract_checksum_character(checksum));
        dxw.extend(data_dxw);
        let sbs = assemble_sbs(&dxw, seq);
        // Oracle's 58-element sbs.
        let want: &[u8] = &[
            1, 3, 1, 2, 2, 1, 1, 6, 1, 1, 8, 4, 1, 1, 1, 2, 1, 3, 5, 2, 2, 1, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 4, 6, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 1, 1,
        ];
        assert_eq!(sbs, want);
    }

    #[test]
    fn compute_checksum_matches_oracle_input_a() {
        // After computing dxw for data segments only, the mod-211
        // checksum BWIPP injects ahead of them must match 246 for
        // (01)90012345678908.
        let binval: &[u8] = &[
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
        ];
        let dxw: Vec<[u8; 8]> = (0..4)
            .map(|x| {
                let d = pack_12_bits(&binval[x * 12..x * 12 + 12]);
                extract_data_character(d, x)
            })
            .collect();
        let seq: &[u8] = &[0, 3, 2]; // oracle finder sequence for datalen=5
        let checksum = compute_checksum(&dxw, seq);
        assert_eq!(checksum, 246);
    }

    /// Stage 11.A8c — pin `to_bin` MSB-first bit-string conversion.
    /// Kills `>> with <<` direction-flip and `& with |` mutations on
    /// lines 152-153.
    #[test]
    fn to_bin_boundary_widths() {
        // 0 in any width → all zeros.
        assert_eq!(to_bin(0, 4).unwrap(), "0000");
        assert_eq!(to_bin(0, 1).unwrap(), "0");

        // 1 in width 4 → "0001" (LSB at the right).
        assert_eq!(to_bin(1, 4).unwrap(), "0001");

        // 0b1010 in width 4 → "1010".
        assert_eq!(to_bin(0b1010, 4).unwrap(), "1010");

        // 0xFF in width 8 → all ones.
        assert_eq!(to_bin(0xFF, 8).unwrap(), "11111111");

        // 0b10000000 in width 8 → leading "1" then 7 "0".
        assert_eq!(to_bin(0x80, 8).unwrap(), "10000000");

        // Width-too-small → InvalidData. Stage 11.A8c — pin the
        // diagnostic + value/width echo for both overflow cases.
        // to_bin's arm at line 146-150 produces:
        //   "DataBar Expanded: value V doesn't fit in W bits"
        for (n, width) in [(16u64, 4usize), (256, 8)] {
            let err = to_bin(n, width).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("to_bin({n}, {width}) must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("DataBar Expanded:") && msg.contains("doesn't fit"),
                "to_bin({n}, {width}) must pin symbology tag + 'doesn't fit'; got {msg:?}"
            );
            assert!(
                msg.contains(&n.to_string()) && msg.contains(&format!("{width} bits")),
                "to_bin({n}, {width}) must echo value + width; got {msg:?}"
            );
        }

        // Width=64 — `if width < 64` short-circuits, so the
        // overflow-guard arithmetic doesn't fire.
        let s = to_bin(0xFFFF_FFFF_FFFF_FFFF, 64).unwrap();
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c == '1'));
    }

    /// Stage 11.A8c — pin `rembits` codeword-alignment arithmetic.
    /// Kills `div_ceil with /` / `* 12` / `% with /` / `+ 1` mutations
    /// on lines 363-369.
    #[test]
    fn rembits_codeword_alignment() {
        // Trivial: 0 unpadded bits → 48 bits target → 48 padding.
        assert_eq!(rembits(0, 4), 48);

        // 12 bits → rounded=12, target=max(12,48)=48, cw=4. 4 % 4 == 0,
        // no +1 adjustment. Return 48 - 12 = 36.
        assert_eq!(rembits(12, 4), 36);

        // 48 bits → rounded=48, target=48, cw=4. 4 % 4 == 0. Return 0.
        assert_eq!(rembits(48, 4), 0);

        // 60 bits → rounded=60, target=max(60,48)=60, cw=5.
        // 5 % 4 == 1 → target = 6*12 = 72. Return 72 - 60 = 12.
        assert_eq!(rembits(60, 4), 12);

        // 72 bits, 4 segments → rounded=72, target=72, cw=6.
        // 6 % 4 == 2 (not 1), no +1. Return 0.
        assert_eq!(rembits(72, 4), 0);

        // 1 bit → rounded=div_ceil(1,12)*12 = 1*12 = 12, target=48,
        // cw=4. Return 48 - 1 = 47.
        assert_eq!(rembits(1, 4), 47);
    }

    /// Stage 11.A8c — pin `match_310x_or_320x(ai)`. Classifies AI
    /// `310x` (weight, kg, is_320x=false) and `320x` (weight, lbs,
    /// is_320x=true) — used by `encode_method_0111` to gate the
    /// weight-component compaction path. Returns the last digit (0-9
    /// decimal-place indicator) per BWIPP.
    ///
    /// Three rejection branches + two acceptance branches; no direct
    /// test until now — only exercised through full method-0111 goldens.
    ///
    /// Anchors pin:
    ///   * "3100" → Some((false, 0)) and "3105" → Some((false, 5)) and
    ///     "3109" → Some((false, 9)) (310x branch);
    ///   * "3200" → Some((true, 0)) and "3203" → Some((true, 3)) and
    ///     "3209" → Some((true, 9)) (320x branch);
    ///   * length guards: "" / "310" / "31000" → None;
    ///   * non-digit last char: "310A" / "320X" → None;
    ///   * wrong prefix: "311X" / "330X" / "311X" → None;
    ///   * is_320x boolean is the only difference between the two
    ///     accepted prefixes (kills arm-flip mutant).
    #[test]
    fn match_310x_or_320x_arms_and_length_guards() {
        // 310x prefix: is_320x = false; last digit value extracted.
        assert_eq!(
            match_310x_or_320x("3100"),
            Some((false, 0)),
            "310x boundary: last digit 0"
        );
        assert_eq!(
            match_310x_or_320x("3105"),
            Some((false, 5)),
            "310x mid: last digit 5"
        );
        assert_eq!(
            match_310x_or_320x("3109"),
            Some((false, 9)),
            "310x top: last digit 9"
        );

        // 320x prefix: is_320x = true.
        assert_eq!(match_310x_or_320x("3200"), Some((true, 0)), "320x boundary");
        assert_eq!(match_310x_or_320x("3203"), Some((true, 3)), "320x mid");
        assert_eq!(match_310x_or_320x("3209"), Some((true, 9)), "320x top");

        // Length guards.
        assert_eq!(match_310x_or_320x(""), None, "empty input");
        assert_eq!(match_310x_or_320x("310"), None, "length 3 (one too short)");
        assert_eq!(match_310x_or_320x("31000"), None, "length 5 (one too long)");

        // Non-digit last char.
        assert_eq!(match_310x_or_320x("310A"), None, "non-digit last char");
        assert_eq!(match_310x_or_320x("320X"), None);
        assert_eq!(match_310x_or_320x("310-"), None);

        // Wrong prefix.
        assert_eq!(match_310x_or_320x("3115"), None, "wrong prefix 311");
        assert_eq!(match_310x_or_320x("3305"), None, "wrong prefix 330");
        assert_eq!(match_310x_or_320x("4100"), None, "wrong prefix 410");

        // 4-char but with leading control bytes (non-digit anywhere).
        assert_eq!(match_310x_or_320x("31X0"), None);

        // Asymmetric anchor: (false, 5) vs (true, 5) differ ONLY in
        // is_320x — kills arm-flip mutant.
        assert_eq!(match_310x_or_320x("3105"), Some((false, 5)));
        assert_eq!(match_310x_or_320x("3205"), Some((true, 5)));
    }

    /// Stage 11.A8c — pin `to_bin(n, width)`. Foundational MSB-first
    /// bit-string renderer used by every method-* encoder in this
    /// module to assemble cdfs, ai values, and trailing checkbits.
    /// Sister of `maxicode::to_bin` but returns `Result<String, Error>`
    /// instead of `Option<String>`.
    ///
    /// Anchors pin every observable behavior:
    ///   * width=0, n=0 → Ok("") (empty bit-string is valid);
    ///   * width=4, n=0 → "0000" / n=15 → "1111" (kills '0'/'1' swap);
    ///   * width=4, n=5 → "0101" (kills `.rev()` removal — would
    ///     give "1010");
    ///   * width=8, n=130 → "10000010" (asymmetric anchor — the
    ///     palindromic 129 wouldn't catch a bit-order flip);
    ///   * width=4, n=16 → Err (1<<4 doesn't fit);
    ///   * width=4, n=15 → Ok (boundary inverse);
    ///   * width=63, n=u64::MAX → Err (doesn't fit);
    ///   * width=64, n=u64::MAX → Ok(64×'1') (kills `< 64` → `<= 64`
    ///     overflow mutant computing `1u64 << 64`);
    ///   * length always equals `width` on success.
    #[test]
    fn to_bin_msb_first_with_overflow_guard() {
        // Empty width.
        assert_eq!(to_bin(0, 0).unwrap(), "");

        // All-zero / all-one (kills '0'/'1' swap).
        assert_eq!(to_bin(0, 4).unwrap(), "0000");
        assert_eq!(to_bin(15, 4).unwrap(), "1111");

        // MSB-first order: 5 = 0b0101.
        assert_eq!(
            to_bin(5, 4).unwrap(),
            "0101",
            "MSB-first: removing .rev() would give \"1010\""
        );

        // Asymmetric anchor: 130 = 0b10000010 ≠ reverse 0b01000001.
        assert_eq!(to_bin(130, 8).unwrap(), "10000010");

        // Boundary: 1<<4 doesn't fit.
        //
        // Stage 11.A8c — pin the same diagnostic substring as the
        // sibling tests (ff5e670 / 83ece75) so defense-in-depth
        // covers every to_bin overflow assertion.
        let err = to_bin(16, 4).unwrap_err();
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("to_bin(16, 4) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("DataBar Expanded:")
                && msg.contains("16")
                && msg.contains("4 bits")
                && msg.contains("doesn't fit"),
            "to_bin(16, 4) must pin tag + value + width + 'doesn't fit'; got {msg:?}"
        );
        // Boundary inverse: 1<<width - 1 fits.
        assert!(to_bin(15, 4).is_ok(), "15 fits in 4 bits");

        // width=63 + u64::MAX → Err. Pin the same diagnostic.
        let err = to_bin(u64::MAX, 63).unwrap_err();
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("to_bin(u64::MAX, 63) must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("DataBar Expanded:")
                && msg.contains(&u64::MAX.to_string())
                && msg.contains("63 bits")
                && msg.contains("doesn't fit"),
            "to_bin(u64::MAX, 63) must pin tag + u64::MAX value + 63 bits width; got {msg:?}"
        );

        // width=64 + u64::MAX → Ok(64×'1') (kills `< 64` → `<= 64`
        // mutant computing `1u64 << 64`).
        let s = to_bin(u64::MAX, 64).expect("width=64 always fits");
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c == '1'));

        // width=64 + 0 → 64×'0'.
        assert_eq!(to_bin(0, 64).unwrap(), "0".repeat(64));

        // Length invariant: output.len() == width on every success.
        for w in 0..=12 {
            for v in 0..=7u64 {
                if let Ok(s) = to_bin(v, w) {
                    assert_eq!(s.len(), w, "to_bin({v}, {w}) length should be {w}");
                }
            }
        }
    }

    /// Stage 11.A8c — pin `conv12to40(digits)`. Splits 12 ASCII digits
    /// into 4 groups of 3, converts each group's decimal value (0..=999)
    /// to a 10-bit MSB-first binary string, and concatenates the four
    /// — always 40 chars on success.
    ///
    /// Used by every BWIPP method-* encoder that ships a GTIN body in
    /// the cdf (encode_method_0100, _0101, _0111, _01100, _01101,
    /// _00_skeleton). The 4-group splitting + per-group to_bin(_, 10)
    /// chain has no direct anchor — only goldens.
    ///
    /// Anchors pin:
    ///   * all-zero "000000000000" → 40 '0's;
    ///   * all-nine "999999999999" → 4×"1111100111" (999 in binary);
    ///   * "000000000001" → first 39 zeros then '1' (kills group-index
    ///     reverse);
    ///   * "100000000000" → "0000000001" + 30 zeros (asymmetric anchor,
    ///     kills group-order swap — 100 = 0b0001100100, MSB-first 10
    ///     bits = "0001100100");
    ///   * length guards: 11 or 13 digits → Err;
    ///   * non-digit interior: "12345678901A" → Err;
    ///   * empty input → Err;
    ///   * length invariant: output.len() == 40 on success.
    #[test]
    fn conv12to40_four_groups_of_three_digits_to_ten_bit_chunks() {
        // All zeros → 40 '0's.
        let z = conv12to40(b"000000000000").unwrap();
        assert_eq!(z, "0".repeat(40));

        // All nines → 4×"1111100111" (999 = 0b1111100111).
        let n = conv12to40(b"999999999999").unwrap();
        assert_eq!(n, "1111100111".repeat(4));

        // Single trailing '1' → leading 30 zeros + 10-bit "0000000001".
        let s = conv12to40(b"000000000001").unwrap();
        assert_eq!(s, "0".repeat(30) + "0000000001");

        // Single leading "1" → first group "100" (=100) → "0001100100"
        // (100 in binary, 10 bits MSB-first). Other 30 chars zero.
        let s = conv12to40(b"100000000000").unwrap();
        assert_eq!(s, format!("{}{}", "0001100100", "0".repeat(30)));

        // Stage 11.A8c — upgrade these 5 weak is_err() checks to
        // diagnostic-substring pins (parallel to the strong sibling
        // conv12to40_rejects_bad_input in commit bd7104a which already
        // pins the full diagnostic). Defense-in-depth.
        //
        // conv12to40's single rejection arm (line 167-173) produces:
        //   "DataBar Expanded: conv12to40 needs exactly 12 ASCII
        //    digits, got X"
        //
        // A mutant that:
        //   * widens "12" → "11"/"13" in the length check,
        //   * swaps "conv12to40" name with "conv13to44",
        //   * drops the {digits:?} echo,
        // would survive bare is_err() checks.
        for (input, scenario) in [
            (b"00000000000" as &[u8], "length 11"),
            (b"0000000000000", "length 13"),
            (b"", "empty"),
            (b"12345678901A", "trailing non-digit"),
            (b"A12345678901", "leading non-digit"),
        ] {
            let err = conv12to40(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!(
                    "conv12to40({input:?}, {scenario}) must yield InvalidData; got other variant"
                );
            };
            assert!(
                msg.contains("DataBar Expanded:")
                    && msg.contains("conv12to40")
                    && msg.contains("12 ASCII digits"),
                "{input:?} ({scenario}) must pin tag + function name + '12 ASCII digits'; \
                 got {msg:?}"
            );
            assert!(
                !msg.contains("conv13to44"),
                "{input:?} ({scenario}) diagnostic must not leak conv13to44 name; got {msg:?}"
            );
        }

        // Length invariant: every success returns exactly 40 chars.
        let cases: &[&[u8]] = &[
            b"123456789012",
            b"012345678901",
            b"555555555555",
            b"111222333444",
        ];
        for c in cases {
            let s = conv12to40(c).unwrap();
            assert_eq!(s.len(), 40, "conv12to40 length on {c:?}");
            assert!(s.chars().all(|c| c == '0' || c == '1'));
        }
    }

    /// Stage 11.A8c — pin `conv13to44(digits)`. The leading digit
    /// (value 0..=9) becomes a 4-bit MSB-first chunk; the remaining 12
    /// digits feed `conv12to40`. Always 44 chars on success.
    ///
    /// Same call sites as `conv12to40` but for the 13-digit case (e.g.
    /// the GS1 DataBar Expanded GTIN-13 paths).
    ///
    /// Anchors pin:
    ///   * length & digit guards (12, 14, 0 chars; non-digit anywhere);
    ///   * structural split: head = `digits[0] - b'0'` (so '5' → 5, NOT
    ///     53 — kills `b'0'` arithmetic mutants);
    ///   * head=0 → "0000" prefix; head=9 → "1001" prefix (top nibble);
    ///   * trailing 12 digits delegated to conv12to40 (asymmetric anchor:
    ///     leading digit '1' vs trailing single '1' produce different
    ///     bit positions);
    ///   * total length always 44.
    #[test]
    fn conv13to44_head_nibble_plus_conv12to40_tail() {
        // All zeros → 44 zeros.
        assert_eq!(conv13to44(b"0000000000000").unwrap(), "0".repeat(44));

        // All nines: head 9 → "1001", tail 4×"1111100111".
        let s = conv13to44(b"9999999999999").unwrap();
        assert_eq!(s, format!("{}{}", "1001", "1111100111".repeat(4)));
        assert_eq!(s.len(), 44);

        // Head=1, tail zeros: "0001" + 40×'0'.
        let s = conv13to44(b"1000000000000").unwrap();
        assert_eq!(s, format!("{}{}", "0001", "0".repeat(40)));

        // Head=0, single trailing '1': "0000" + conv12to40("000000000001")
        // = "0000" + 30 zeros + "0000000001".
        let s = conv13to44(b"0000000000001").unwrap();
        assert_eq!(s, format!("{}{}{}", "0000", "0".repeat(30), "0000000001"));

        // Asymmetric leading '1' anchor — pins that head goes FIRST,
        // not last. If split direction were reversed:
        //   reversed split: head = digits[12] ('0') = "0000",
        //   tail = conv12to40(digits[0..12]) = "0001100100"+30 zeros
        //   total: "0000" + "0001100100" + "0".repeat(30)
        //          = "00000001100100" + 30 zeros
        // Correct: head = digits[0] ('1') = "0001", tail = "0".repeat(40)
        // → "0001" + "0".repeat(40). Different first 14 chars.
        let s = conv13to44(b"1000000000000").unwrap();
        assert_eq!(&s[..14], "00010000000000", "head goes FIRST");

        // Head=5: "0101" prefix (NOT "5" decimal — kills b'0' → b'1'
        // or removed-subtraction mutants).
        let s = conv13to44(b"5000000000000").unwrap();
        assert_eq!(&s[..4], "0101", "head=5 → 4-bit nibble '0101'");

        // Head boundary: head=15? Impossible — '5' is the max single
        // digit, but the helper accepts any ASCII digit. Test head=9
        // (max valid) gives nibble "1001".
        let s = conv13to44(b"9000000000000").unwrap();
        assert_eq!(&s[..4], "1001");

        // Stage 11.A8c — upgrade these 5 weak is_err() checks to
        // diagnostic-substring pins (parallel to the strong sibling
        // conv13to44_rejects_bad_input in commit eafb8a7). Provides
        // defense-in-depth.
        //
        // conv13to44's single rejection arm (line 193-199) produces:
        //   "DataBar Expanded: conv13to44 needs exactly 13 ASCII
        //    digits, got X"
        for (input, scenario) in [
            (b"000000000000" as &[u8], "length 12"),
            (b"00000000000000", "length 14"),
            (b"", "empty"),
            (b"A000000000000", "leading non-digit"),
            (b"000000000000A", "trailing non-digit"),
        ] {
            let err = conv13to44(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!(
                    "conv13to44({input:?}, {scenario}) must yield InvalidData; got other variant"
                );
            };
            assert!(
                msg.contains("DataBar Expanded:")
                    && msg.contains("conv13to44")
                    && msg.contains("13 ASCII digits"),
                "{input:?} ({scenario}) must pin tag + function name + '13 ASCII digits'; \
                 got {msg:?}"
            );
            assert!(
                !msg.contains("conv12to40"),
                "{input:?} ({scenario}) diagnostic must not leak conv12to40 (cross-fn swap); got {msg:?}"
            );
        }

        // Length invariant: every success returns exactly 44 chars.
        let cases: &[&[u8]] = &[
            b"1234567890123",
            b"0123456789012",
            b"5555555555555",
            b"7111222333444",
        ];
        for c in cases {
            let s = conv13to44(c).unwrap();
            assert_eq!(s.len(), 44, "conv13to44 length on {c:?}");
            assert!(s.chars().all(|c| c == '0' || c == '1'));
        }
    }

    /// Stage 11.A8c — pin `compute_row_sep(row)` for the short-row
    /// path (n < 13, no finder positions). The full helper has a
    /// complex finder-position branch (lines 1507-1518) but the no-
    /// finder code path — initial `sep[i] = 1 - row[i]` complement
    /// plus the seppad first-4 / last-4 zeroing — is the structural
    /// backbone exercised by every short row. Mutations on the
    /// `1 - b` complement, the `take(4)` prefix size, the
    /// `if n >= 4` guard, or the `sep[n - 4..]` slice base all
    /// survive at the stacked-pixs level when the finder loop
    /// dominates the output.
    ///
    /// Hand-computed (n < 13 means the finder-positions vector is
    /// empty, so step 3 — the finder-aware overrides — is skipped
    /// entirely and the output is just `(1 - row).zeropad(4, 4)`):
    ///   * row=[]  → sep=[]  (degenerate, both seppad branches skip).
    ///   * row=[1, 0, 1]  (n=3) → init [0, 1, 0]; first-4 take(4)
    ///     zeroes all 3 entries; n >= 4 false → final [0, 0, 0].
    ///   * row=[1, 0, 1, 0]  (n=4) → init [0, 1, 0, 1]; first-4 →
    ///     all zeroes; last-4 (n=4) → same range. Final [0, 0, 0, 0].
    ///   * row=[1, 1, 1, 1, 0, 0, 0, 0]  (n=8) → init [0, 0, 0, 0,
    ///     1, 1, 1, 1]; first-4 → [0,0,0,0,1,1,1,1]; last-4 zeroes
    ///     [4..=7]. Final all zeros.
    ///   * row=[0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]  (n=12) →
    ///     init [1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1]; first-4 →
    ///     [0,0,0,0,0,1,0,1,1,1,1,1]; last-4 zeroes [8..=11].
    ///     Final [0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0]  (mid 1s at
    ///     indices 5, 7 survive). Pins both the complement and the
    ///     last-4 slice base.
    ///
    /// Mutations to catch:
    ///   * `1 - b` → `b`: complement inverted; n=12 anchor would
    ///     produce zeros where 1s should be.
    ///   * `iter_mut().take(4)` → `take(3)`: n=12 index 3 would
    ///     retain its complement value (1).
    ///   * `if n >= 4` → `if n > 4`: n=4 case skips second-zeroing
    ///     (no behavior change since first-4 already covered the
    ///     whole vec).
    ///   * `sep[n - 4..]` → `sep[n - 3..]`: n=12 indices 8..=11 are
    ///     zeroed; under mutant only 9..=11 zeroed → index 8 keeps
    ///     its complement value (1).
    #[test]
    fn compute_row_sep_short_row_complement_with_seppad() {
        // Degenerate.
        assert_eq!(compute_row_sep(&[]), Vec::<u8>::new());

        // 3-module row: first-4 take(4) iterates over all 3 entries
        // and zeros them; second-4 zero guard `n >= 4` is false.
        assert_eq!(
            compute_row_sep(&[1, 0, 1]),
            vec![0, 0, 0],
            "n=3: first-4 zeroes all entries; second-4 skipped"
        );

        // 4-module row: first-4 zeros all; second-4 zeros same range.
        assert_eq!(
            compute_row_sep(&[1, 0, 1, 0]),
            vec![0, 0, 0, 0],
            "n=4: first-4 + second-4 both zero the whole vec"
        );

        // 8-module row: complement at index 4..=7 is 1; first-4 then
        // last-4 cover the whole vec.
        assert_eq!(
            compute_row_sep(&[1, 1, 1, 1, 0, 0, 0, 0]),
            vec![0, 0, 0, 0, 0, 0, 0, 0],
            "n=8: complement at index 4..=7 zeroed by last-4 seppad"
        );

        // 12-module row: discriminator — middle (indices 4..=7) is
        // untouched by either seppad. Init complement keeps `1 -
        // row[i]` at indices 5 and 7. Final sep has 1s at indices
        // 5 and 7, zeros elsewhere.
        assert_eq!(
            compute_row_sep(&[0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]),
            vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0],
            "n=12: middle (indices 4..=7) keeps the complement; \
             indices 5 and 7 survive as 1 (1 - 0 = 1)"
        );

        // Length invariant: every output length matches input length.
        let cases: &[&[u8]] = &[&[], &[0], &[1], &[1, 0, 1], &[0u8; 12], &[1u8; 12]];
        for row in cases {
            let sep = compute_row_sep(row);
            assert_eq!(sep.len(), row.len(), "row {row:?}: length must match");
            // The first 4 modules are always 0 (when n >= 4).
            if row.len() >= 4 {
                assert_eq!(&sep[..4], &[0, 0, 0, 0], "row {row:?}: first 4 must be 0");
                assert_eq!(
                    &sep[row.len() - 4..],
                    &[0, 0, 0, 0],
                    "row {row:?}: last 4 must be 0"
                );
            }
        }
    }

    /// Stage 11.A8c — pin `build_inter_row_sep(pixx)`. This helper
    /// builds the inter-row separator: alternating `[0, 1, 0, 1, ...]`
    /// of length `pixx` with the first 4 and last 4 modules zeroed
    /// (BWIPP `seppad`). Used by `assemble_stacked_pixs` between
    /// adjacent stacked rows but never directly pinned, so arithmetic
    /// mutants on lines 1555-1566 (the `i % 2` stride, the
    /// `take(4)` count, the `pixx - 4` slice base, and the
    /// `pixx >= 4` guard) survive on the existing suite.
    ///
    /// Hand-computed (alternation starts with `0` at index 0):
    ///   * pixx=0   → []  (degenerate; iterator yields nothing).
    ///   * pixx=3   → [0, 0, 0]  (first-4 zeroing collapses all 3
    ///     leading entries; second-4 guard `pixx >= 4` skipped).
    ///   * pixx=4   → [0, 0, 0, 0]  (alternation [0,1,0,1] →
    ///     first-4 → all zeros; second-4 zeroes the same slice).
    ///   * pixx=5   → [0, 0, 0, 0, 0]  (alternation [0,1,0,1,0] →
    ///     first-4 → [0,0,0,0,0]; second-4 zeroes indices 1..=4 —
    ///     re-zero is idempotent so all zeros).
    ///   * pixx=8   → [0, 0, 0, 0, 0, 0, 0, 0]  (alternation
    ///     [0,1,0,1,0,1,0,1] → first-4 covers half, second-4
    ///     [indices 4..=7] covers the rest → all zeros).
    ///   * pixx=10  → [0, 0, 0, 0, 0, 1, 0, 0, 0, 0]  (alternation
    ///     [0,1,0,1,0,1,0,1,0,1] → first-4 → [0,0,0,0,0,1,0,1,0,1];
    ///     last-4 zeroes indices 6..=9 → [0,0,0,0,0,1,0,0,0,0]).
    ///   * pixx=12  → [0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0]
    ///     (first-4 + last-4 leave middle indices 4..=7 untouched).
    ///   * pixx=20  → [0,0,0,0, 0,1,0,1,0,1,0,1,0,1,0,1, 0,0,0,0]
    ///     (asymmetric mid: 12 untouched alternations at indices
    ///     4..=15, ending with index 15 = 1 since 15 % 2 = 1).
    ///
    /// Mutations to catch:
    ///   * `i % 2` → `i % 3`: pixx=20 middle becomes [0,1,1,0,1,1,
    ///     0,1,1,0,1,1] instead of [0,1,0,1,0,1,0,1,0,1,0,1].
    ///   * `take(4)` → `take(3)`: pixx=20 index 3 would survive as 1
    ///     (since 3 % 2 = 1) → first prefix becomes [0,0,0,1] not
    ///     [0,0,0,0].
    ///   * `*slot = 0` → `*slot = 1`: first-4 prefix becomes
    ///     [1,1,1,1] for pixx=20.
    ///   * `sep[pixx - 4..]` → `sep[pixx - 3..]`: pixx=20 last 3
    ///     zeroed (indices 17..=19) but index 16 stays at 0 (i%2=0)
    ///     — same output coincidentally because index 16 is even.
    ///     Use pixx=10 where index 6 (even, i%2=0) is `0` originally
    ///     so the shift hides too, BUT under `pixx - 3` only
    ///     indices 7..=9 are zeroed, leaving index 6 at 0 (same).
    ///     The `pixx - 5` direction would zero indices 5..=9 making
    ///     index 5 = 0 instead of 1 — pixx=10 catches that.
    ///   * `pixx >= 4` → `pixx >= 5`: pixx=4 skips second-zeroing →
    ///     same output (first-4 already covered all of pixx=4).
    ///     pixx=5 with the mutant still runs since 5 >= 5; equivalent.
    ///     Not catchable from output — documented as equivalent.
    #[test]
    fn build_inter_row_sep_alternates_with_seppad() {
        // Degenerate cases.
        assert_eq!(build_inter_row_sep(0), Vec::<u8>::new());
        assert_eq!(build_inter_row_sep(1), vec![0u8]);
        assert_eq!(build_inter_row_sep(3), vec![0, 0, 0]);

        // Small cases where first-4/last-4 cover everything.
        assert_eq!(build_inter_row_sep(4), vec![0, 0, 0, 0]);
        assert_eq!(build_inter_row_sep(5), vec![0, 0, 0, 0, 0]);
        assert_eq!(build_inter_row_sep(8), vec![0u8; 8]);

        // First case with a surviving middle '1' at index 5
        // (i=5, 5%2=1 → bit 1). Pins both the alternation stride
        // (`i % 2`) and the last-4 slice base (`pixx - 4`).
        assert_eq!(
            build_inter_row_sep(10),
            vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0],
            "pixx=10: middle index 5 = 1; indices 6..=9 zeroed"
        );

        // Middle is exactly [index 5, index 7] alternation pattern.
        assert_eq!(
            build_inter_row_sep(12),
            vec![0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0],
            "pixx=12: middle indices 4..=7 keep alternation"
        );

        // Wider: middle is full alternation indices 4..=15 (12 elems);
        // last-4 (indices 16..=19) zeroed.
        assert_eq!(
            build_inter_row_sep(20),
            vec![
                0, 0, 0, 0, // first-4 zeroed
                0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, // alternation
                0, 0, 0, 0, // last-4 zeroed
            ],
            "pixx=20: 12-wide alternation centered; first/last 4 zeroed"
        );

        // Length invariant.
        for pixx in 0..=30 {
            assert_eq!(
                build_inter_row_sep(pixx).len(),
                pixx,
                "pixx={pixx}: output length must equal input pixx"
            );
        }
        // The first 4 modules are always 0 (when pixx >= 4).
        for pixx in 4..=30 {
            let sep = build_inter_row_sep(pixx);
            assert_eq!(
                &sep[..4],
                &[0, 0, 0, 0],
                "pixx={pixx}: first 4 must be zero"
            );
            assert_eq!(
                &sep[pixx - 4..],
                &[0, 0, 0, 0],
                "pixx={pixx}: last 4 must be zero"
            );
        }
    }

    /// Stage 11.A8c — pin `count_finder_positions(n)`. This helper
    /// walks the BWIPP `finderpos` schedule at offsets 19 + k*98 and
    /// 68 + k*98 up to `n - 13` and counts hits. It's exercised only
    /// transitively through the stacked-row pipeline at
    /// `assemble_stacked_pixs`; no direct unit test until now, so
    /// arithmetic mutants on lines 1533-1550 (the `<13` early return,
    /// the offset constants 19/68, the stride 98, and the boundary
    /// `p <= max`) all survive on the existing suite.
    ///
    /// Hand-computed:
    ///   * n ∈ 0..=12 → 0 (early return).
    ///   * n=13: max=0; p=19 > 0; p=68 > 0 → 0.
    ///   * n=31: max=18; p=19 > 18; p=68 > 18 → 0.
    ///     (Just under the first finder position.)
    ///   * n=32: max=19; p=19 ≤ 19 → c=1 (first finder fits exactly).
    ///   * n=80: max=67; p=19 ≤ 67 → c=1; p=68 > 67 → 1.
    ///   * n=81: max=68; p=19 ≤ 68 → c=1; p=68 ≤ 68 → c=2 (second
    ///     finder fits exactly).
    ///   * n=130: max=117; p=19,117 both ≤ 117 → c=2; p=68 ≤ 117 → c=3.
    ///   * n=180: max=167; p=19,117 both ≤ 167 → c=2; p=68,166 both
    ///     ≤ 167 → c=4.
    ///
    /// Mutations to catch:
    ///   * `p = 19` → `p = 20`: n=32 anchor flips from 1 → 0.
    ///   * `p = 68` → `p = 69`: n=81 anchor flips from 2 → 1.
    ///   * `p += 98` → `p += 97` or `p += 99`: the n=130 stride catches
    ///     this — under +97 the second hit at p=19+97=116 ≤ 117 keeps
    ///     c=2 (matches); under +99 the second hit p=118 > 117 drops
    ///     to c=1 (mismatch). The n=180 anchor also pins both strides.
    ///   * `while p <= max` → `while p < max`: n=32 anchor would skip
    ///     the exact-boundary hit and return 0 instead of 1.
    ///   * `n - 13` → `n - 12` / `n - 14`: shifts the max bound; n=32
    ///     with `n - 12` gives max=20 (still hits 19 → c=1, no change),
    ///     but n=31 with `n - 12` gives max=19 (hits 19 → c=1 vs
    ///     correct 0). The n=31 anchor catches this.
    ///   * `c += 1` → `c -= 1`: usize underflow panic. Caught by
    ///     n=32.
    ///   * Body replaced with `0`: every positive anchor catches.
    #[test]
    fn count_finder_positions_offset_19_and_68_with_stride_98() {
        // Below the 13-module floor: always 0.
        assert_eq!(count_finder_positions(0), 0, "n=0 → 0");
        assert_eq!(count_finder_positions(12), 0, "n=12 → 0 (under floor)");
        assert_eq!(count_finder_positions(13), 0, "n=13 → 0 (floor, max=0)");
        assert_eq!(
            count_finder_positions(31),
            0,
            "n=31 → 0 (max=18, both p=19 and p=68 > 18)"
        );
        // First finder fits exactly at the boundary.
        assert_eq!(
            count_finder_positions(32),
            1,
            "n=32 → 1 (max=19, p=19 ≤ 19 exact hit)"
        );
        // Just below the second-finder threshold.
        assert_eq!(
            count_finder_positions(80),
            1,
            "n=80 → 1 (max=67, p=19 fits; p=68 > 67)"
        );
        // Second finder fits exactly at the boundary.
        assert_eq!(
            count_finder_positions(81),
            2,
            "n=81 → 2 (max=68, p=19 and p=68 both fit exactly)"
        );
        // Wrap into next cycle: p=19+98=117 fits at max=117 (n=130).
        assert_eq!(
            count_finder_positions(130),
            3,
            "n=130 → 3 (p=19, p=117, p=68 all ≤ 117)"
        );
        // Larger sweep with all four hits within the first two cycles.
        assert_eq!(
            count_finder_positions(180),
            4,
            "n=180 → 4 (p=19,117 ≤ 167 and p=68,166 ≤ 167)"
        );
    }

    /// Stage 11.A8c — pin `expand_row_sbs(sbs)`. BWIPP's per-row
    /// width-to-module-bit expander: even-index widths emit `0`
    /// (space), odd-index emit `1` (bar). DataBar Expanded sbs
    /// arrays start with a space, so index 0 → bit 0. Used by
    /// `assemble_stacked_pixs` before `compute_row_sep` /
    /// `build_inter_row_sep`, but never directly pinned — so
    /// mutations on the `i % 2 == 0` parity, the `0u8` / `1u8`
    /// constants, the `for _ in 0..w` width loop, or the
    /// `push(bit)` body all survive the existing pixs-level goldens
    /// when their length happens to coincide.
    ///
    /// Hand-computed anchors:
    ///   * sbs=[]   → []  (degenerate; outer loop never enters).
    ///   * sbs=[3]  → [0, 0, 0]  (single space of width 3).
    ///   * sbs=[3, 2]  → [0, 0, 0, 1, 1]  (space-3 then bar-2).
    ///   * sbs=[2, 1, 3]  → [0, 0, 1, 0, 0, 0]  (3-element row,
    ///     space-bar-space; the i=2 even index emits 0, not 1).
    ///   * sbs=[1, 1, 1, 1]  → [0, 1, 0, 1]  (alternating, one each).
    ///   * sbs=[0, 0, 5]  → [0, 0, 0, 0, 0]  (zero widths produce no
    ///     output; only the i=2 (even) entry emits its 5 zeros).
    ///   * sbs=[5, 0, 3]  → [0, 0, 0, 0, 0, 0, 0, 0]  (zero-width
    ///     bar in middle still consumed; index 2 even → 0).
    ///
    /// Mutations to catch:
    ///   * `i % 2 == 0` → `i % 2 != 0`: swaps the bar/space
    ///     assignment. sbs=[3, 2] would emit [1, 1, 1, 0, 0] instead
    ///     of [0, 0, 0, 1, 1].
    ///   * `0u8` ↔ `1u8` swap (in either arm): inverts everything.
    ///     sbs=[3] would emit [1, 1, 1] instead of [0, 0, 0].
    ///   * `for _ in 0..w` → `for _ in 0..=w`: extra push per width.
    ///     sbs=[3] would emit 4 bits not 3.
    ///   * `out.push(bit)` → `out.push(0)`: always pushes 0. sbs=[3, 2]
    ///     would emit [0, 0, 0, 0, 0] instead of [0, 0, 0, 1, 1].
    ///   * `out.push(bit)` → `out.push(1)`: always pushes 1. sbs=[3]
    ///     catches.
    ///   * `sbs.iter().enumerate()` → `sbs.iter().rev().enumerate()`:
    ///     index order flips. sbs=[2, 1, 3] would emit [0, 0, 0, 1,
    ///     0, 0] (reversed→[3,1,2]; i=0 even→0×3, i=1 odd→1×1,
    ///     i=2 even→0×2). Distinct from the correct [0, 0, 1, 0, 0,
    ///     0].
    #[test]
    fn expand_row_sbs_alternates_bit_per_index_parity() {
        // Degenerate.
        assert_eq!(expand_row_sbs(&[]), Vec::<u8>::new());

        // Single space (i=0, even → bit 0).
        assert_eq!(expand_row_sbs(&[3]), vec![0, 0, 0]);

        // Single bar would need an even index — impossible by
        // construction. The first odd-indexed entry is i=1; pin
        // that the second slot (i=1) emits 1s of its declared width.
        assert_eq!(
            expand_row_sbs(&[3, 2]),
            vec![0, 0, 0, 1, 1],
            "i=0 (even) → 3 zeros; i=1 (odd) → 2 ones"
        );

        // Three slots: space-bar-space. Pins that i=2 emits 0 (not 1).
        assert_eq!(
            expand_row_sbs(&[2, 1, 3]),
            vec![0, 0, 1, 0, 0, 0],
            "i=0 → 0x2; i=1 → 1x1; i=2 → 0x3 (even parity → bit 0)"
        );

        // Single-width alternation: catches any constant-bit mutation.
        assert_eq!(
            expand_row_sbs(&[1, 1, 1, 1]),
            vec![0, 1, 0, 1],
            "1-1-1-1 alternation must produce [0,1,0,1]"
        );

        // Zero-width slots: skipped via empty inner loop. Index 2
        // still even (→ bit 0), not bit 1.
        assert_eq!(
            expand_row_sbs(&[0, 0, 5]),
            vec![0, 0, 0, 0, 0],
            "leading zeros produce no output; i=2 emits 5 zeros"
        );
        assert_eq!(
            expand_row_sbs(&[5, 0, 3]),
            vec![0, 0, 0, 0, 0, 0, 0, 0],
            "i=0 emits 5 zeros; i=1 zero-width emits nothing; i=2 emits 3 zeros"
        );

        // Length invariant: output length == sum of widths.
        let cases: &[&[u8]] = &[
            &[],
            &[3],
            &[3, 2],
            &[1, 1, 1, 1, 1, 1],
            &[5, 4, 3, 2, 1],
            &[0, 7, 0, 0, 9],
        ];
        for sbs in cases {
            let out = expand_row_sbs(sbs);
            let total: usize = sbs.iter().map(|&w| w as usize).sum();
            assert_eq!(
                out.len(),
                total,
                "sbs {sbs:?}: output length must equal sum of widths"
            );
            // Each emitted bit must be 0 or 1.
            assert!(out.iter().all(|&b| b == 0 || b == 1));
        }
    }

    /// Stage 11.A8c — pin `compute_gpf_lookahead` reverse-walk tables.
    ///
    /// `compute_gpf_lookahead` runs a backward DP over `gpf` that
    /// populates 3 lookahead tables used by `encode_general_purpose`'s
    /// mode-switching state machine. The function has no direct test
    /// — only indirect coverage through `encode_general_purpose` end-
    /// to-end goldens, which gives mutants several places to hide
    /// (e.g. swap +1 with +2 on alphanumeric_runs increment).
    ///
    /// Structural invariants pinned (hand-computed):
    /// - Empty input: returns sentinel-only vectors of the right
    ///   lengths (n+2, n+1, n+1).
    /// - Pure digit pair `"12"`: numeric_runs[0]=2 (one pair),
    ///   alphanumeric_runs[0]=2 (both bytes alphanumeric),
    ///   next_iso646_only[0] saturates upward (no iso646-only byte).
    /// - Single uppercase `"A"`: numeric_runs[0]=0 (no pair),
    ///   alphanumeric_runs[0]=1, next_iso646_only[0]=9999+1=10000
    ///   (no iso646-only).
    /// - Single lowercase `"a"`: numeric_runs[0]=0,
    ///   alphanumeric_runs[0]=0 (not alphanumeric),
    ///   next_iso646_only[0]=0 (iso646-only at position 0).
    ///
    /// A mutant on `numeric_runs[i+2] + 2` → `+ 1` would fail the
    /// digit-pair anchor; a mutant on `alphanumeric_runs[i+1] + 1`
    /// → `+ 0` would fail the alphanumeric anchor; a mutant on the
    /// iso646-only `0` → `1` would fail the lowercase anchor.
    #[test]
    fn compute_gpf_lookahead_reverse_walk_invariants() {
        // ---- Empty input.
        let look = compute_gpf_lookahead(b"");
        assert_eq!(look.numeric_runs, vec![0u32, 0]);
        assert_eq!(look.alphanumeric_runs, vec![0u32]);
        assert_eq!(look.next_iso646_only, vec![9999u32]);

        // ---- Pure digit pair.
        let look = compute_gpf_lookahead(b"12");
        assert_eq!(
            look.numeric_runs,
            vec![2u32, 0, 0, 0],
            "digit pair must give numeric_runs[0]=2"
        );
        assert_eq!(
            look.alphanumeric_runs,
            vec![2u32, 1, 0],
            "both digits alphanumeric → run length 2, 1, 0"
        );
        // next_iso646_only saturates upward by 1 each step (no
        // iso646-only byte). i=1 inherits sentinel 9999 from i=2
        // and adds 1 → 10000; i=0 inherits 10000 and adds 1 → 10001.
        assert_eq!(look.next_iso646_only, vec![10001u32, 10000, 9999]);

        // ---- Single uppercase letter (alphanumeric, not iso646-only).
        let look = compute_gpf_lookahead(b"A");
        assert_eq!(look.numeric_runs, vec![0u32, 0, 0]);
        assert_eq!(
            look.alphanumeric_runs,
            vec![1u32, 0],
            "uppercase is alphanumeric → run length 1, 0"
        );
        assert_eq!(look.next_iso646_only, vec![10000u32, 9999]);

        // ---- Single lowercase letter (iso646-only).
        let look = compute_gpf_lookahead(b"a");
        assert_eq!(look.numeric_runs, vec![0u32, 0, 0]);
        assert_eq!(
            look.alphanumeric_runs,
            vec![0u32, 0],
            "lowercase is NOT in alphanumeric alphabet → run length 0, 0"
        );
        assert_eq!(
            look.next_iso646_only,
            vec![0u32, 9999],
            "lowercase IS iso646-only → distance 0 at position 0"
        );

        // ---- Cross-validation: digit pair has numeric_runs[0]=2 BUT
        // single digit has numeric_runs[0]=0. Catches a mutant that
        // sets numeric_runs[i] unconditionally to `numeric_runs[i+2] + 2`.
        let look = compute_gpf_lookahead(b"1");
        assert_eq!(
            look.numeric_runs[0], 0,
            "lone digit cannot form a pair → numeric_runs[0]=0"
        );
        assert_eq!(look.alphanumeric_runs[0], 1);
    }

    /// Stage 11.A8c — pin `build_gpf_bytes` FNC1-insertion rule:
    /// FNC1 is inserted after a variable-length AI **only if** there
    /// is a following element. Fixed-length AIs never get a trailing
    /// FNC1.
    ///
    /// The function has no direct test — only exercised through
    /// `encode_method_*` end-to-end goldens. A mutant that flips the
    /// condition (e.g. `variable && i + 1 < elements.len()` →
    /// `variable || i + 1 < elements.len()`) would insert FNC1 after
    /// every element except the last, breaking BWIPP byte-stream
    /// alignment.
    ///
    /// AI metadata: "01" (GTIN-14) is fixed; "10" (batch number) is
    /// variable.
    #[test]
    fn build_gpf_bytes_fnc1_insertion_per_variable_ai_with_follower() {
        use crate::util::gs1::Element;

        let mk = |ai: &str, data: &str| Element {
            ai: ai.to_string(),
            data: data.to_string(),
        };

        // ---- Empty input → empty output.
        assert_eq!(build_gpf_bytes(&[]).unwrap(), Vec::<u8>::new());

        // ---- Single fixed-length AI ("01" GTIN-14): no FNC1 (no
        // following element).
        let out = build_gpf_bytes(&[mk("01", "04012345123456")]).unwrap();
        assert_eq!(out, b"0104012345123456".to_vec());

        // ---- Single variable-length AI ("10" batch): also no FNC1
        // because there's no following element to separate from.
        let out = build_gpf_bytes(&[mk("10", "BATCH1")]).unwrap();
        assert_eq!(out, b"10BATCH1".to_vec());

        // ---- Variable AI ("10") followed by another AI: FNC1 ('^' =
        // 94) inserted between them. THIS is the path the mutant
        // breaks.
        let out = build_gpf_bytes(&[mk("10", "BATCH1"), mk("01", "04012345123456")]).unwrap();
        assert_eq!(
            out,
            [
                b'1', b'0', b'B', b'A', b'T', b'C', b'H', b'1', b'^', b'0', b'1', b'0', b'4', b'0',
                b'1', b'2', b'3', b'4', b'5', b'1', b'2', b'3', b'4', b'5', b'6',
            ],
            "FNC1 must separate variable AI from following AI"
        );

        // ---- Fixed AI ("01") followed by variable AI ("10"): NO
        // FNC1 between them (fixed AI doesn't trigger the insertion).
        let out = build_gpf_bytes(&[mk("01", "04012345123456"), mk("10", "BATCH1")]).unwrap();
        assert_eq!(
            out,
            b"0104012345123456"
                .iter()
                .chain(b"10BATCH1")
                .copied()
                .collect::<Vec<u8>>(),
            "fixed AI must NOT insert FNC1 — only variable AIs do"
        );

        // ---- Two variable AIs ("10" + "21"): FNC1 between them
        // (after first variable, before second).
        let out = build_gpf_bytes(&[mk("10", "BATCH1"), mk("21", "SERIAL")]).unwrap();
        assert!(
            out.contains(&FNC1_SENTINEL_BYTE),
            "FNC1 missing between variable AIs"
        );
        // The 9th byte (after "10BATCH1") should be FNC1.
        assert_eq!(out[8], FNC1_SENTINEL_BYTE);
        assert_eq!(&out[..8], b"10BATCH1");
        assert_eq!(&out[9..], b"21SERIAL");

        // ---- Unknown AI → InvalidData error. Pins the
        // `ai_is_variable_length(...).ok_or_else(...)` error arm at
        // line 1133-1138. The diagnostic is:
        //   "DataBar Expanded: AI (99999) not in the GS1 table"
        //
        // Stage 11.A8c — upgrade from `matches!(_, InvalidData(_))` to
        // pin the symbology tag, the AI echo, and the table-membership
        // text. Kills `{e.ai}` drop / fixed-replacement mutants and
        // table-tag swaps.
        let err = build_gpf_bytes(&[mk("99999", "X")]).unwrap_err();
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("unknown AI must surface as InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("DataBar Expanded:"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("AI (99999)"),
            "diagnostic must echo the offending AI; got {msg:?}"
        );
        assert!(
            msg.contains("not in the GS1 table"),
            "diagnostic must carry the table-membership phrase; got {msg:?}"
        );
    }

    /// Stage 11.A8c-L — fingerprint test covering the biggest dbexp survivor
    /// clusters: encode_stacked(21), encode_general_purpose(18),
    /// encode_method_0111(10), encode_method_01101(10), compute_row_sep(7),
    /// encode(6), encode_method_01100(5), encode_method_0101(3). Drives
    /// the public `encode` and `encode_stacked` entries with 8 diverse
    /// GS1 inputs covering GTIN-only / GTIN+weight / GTIN+AI / non-GTIN /
    /// long-AI / multi-AI / linkage on/off paths — each exercises a
    /// distinct method dispatch and bit-packing route in
    /// `encode_general_purpose` / `encode_method_*`. Any arithmetic /
    /// boundary mutation across those functions shifts the bar/matrix
    /// output and breaks one of the per-case fingerprints.
    #[test]
    fn dbexp_encode_and_stacked_fingerprint_pinned() {
        fn fp_lin(p: &crate::encoding::LinearPattern) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &b) in p.bars.iter().enumerate() {
                s = s.wrapping_add(
                    (b as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (p.bars.len(), s)
        }
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // Linear (encode) — 5 GS1 inputs covering each method dispatch.
        let lin_cases: &[(&str, &str, bool, (usize, u64))] = &[
            ("gtin_only", "(01)90012345678908", false, FP_LIN_GTIN),
            (
                "gtin_weight",
                "(01)90012345678908(3103)001750",
                false,
                FP_LIN_GW,
            ),
            ("gtin_ai10", "(01)90012345678908(10)BATCH", false, FP_LIN_GA),
            ("non_gtin", "(10)BATCH123(99)9876543", false, FP_LIN_NG),
            ("linkage", "(01)90012345678908", true, FP_LIN_LINK),
        ];
        for (tag, input, link, want) in lin_cases {
            let p = encode(input, *link).unwrap_or_else(|e| panic!("encode({tag}) ok: {e:?}"));
            let got = fp_lin(&p);
            assert_eq!(got, *want, "linear fingerprint changed for {tag}");
        }
        // Stacked (encode_stacked) — 3 GS1 inputs spanning row counts.
        let stk_cases: &[(&str, &str, bool, (usize, usize, u64))] = &[
            ("stk_gtin", "(01)90012345678908", false, FP_STK_GTIN),
            (
                "stk_long",
                "(01)90012345678908(3103)001750(10)BATCH123",
                false,
                FP_STK_LONG,
            ),
            (
                "stk_linkage",
                "(01)90012345678908(10)BATCH",
                true,
                FP_STK_LINK,
            ),
        ];
        for (tag, input, link, want) in stk_cases {
            let bm = encode_stacked(input, *link)
                .unwrap_or_else(|e| panic!("encode_stacked({tag}) ok: {e:?}"));
            let got = fp_bm(&bm);
            assert_eq!(got, *want, "stacked fingerprint changed for {tag}");
        }
    }
    const FP_LIN_GTIN: (usize, u64) = (58, 10461131334101);
    const FP_LIN_GW: (usize, u64) = (66, 13256252190434);
    const FP_LIN_GA: (usize, u64) = (100, 30956029844782);
    const FP_LIN_NG: (usize, u64) = (100, 31104678247398);
    const FP_LIN_LINK: (usize, u64) = (58, 10381498261271);
    const FP_STK_GTIN: (usize, usize, u64) = (102, 71, 21699701777190963);
    const FP_STK_LONG: (usize, usize, u64) = (102, 145, 113339721915581835);
    const FP_STK_LINK: (usize, usize, u64) = (102, 108, 60900775407773981);

    // -----------------------------------------------------------------
    // Stage 11.A8c-L — STATE-MACHINE FINGERPRINT pre-drafts for the two
    // largest remaining `databar_expanded` v2 clusters (per
    // MUTATION_RESULTS.md row v2, 2026-05): 35 of 79 remaining survivors
    // live inside `encode_general_purpose` (14) and `encode_stacked` (21).
    //
    // The pre-existing `dbexp_encode_and_stacked_fingerprint_pinned`
    // (commit cfb68ae) covers the high-level `encode` and
    // `encode_stacked` wrappers via 5 + 3 GS1 inputs. It already pinned
    // the encode-method dispatch but did not exercise enough of the
    // deeper internal control flow to kill the remaining 35 mutants
    // clustered at L799/818/831/850/855/864 (gp) and
    // L1322/1327/1353/1391/1410/1412/1415 (stacked).
    //
    // These two new state-machine fingerprints exercise the internal
    // functions DIRECTLY (not through `encode`/`encode_stacked`) for
    // gp, and indirectly via `encode_stacked` with cases chosen to
    // span the row-pairing / separator / EOM control-flow arms.
    //
    // Pre-drafts are `#[ignore]`'d with placeholder `(0, 0)` caps.
    // Activation workflow (per established loop iteration; mirrors
    // commits e4d9c72, 10427ba, cfb68ae, 2c08652, 968eced, 57e7c09):
    //   1. Drop `#[ignore]`.
    //   2. `cargo test --include-ignored -- --nocapture \
    //        encode_stacked_state_machine_fingerprint_pinned_pending \
    //        encode_general_purpose_state_machine_fingerprint_pinned_pending`
    //   3. Paste the captured `(len, fp)` / `(w, h, fp)` tuples into
    //      the `FP_EGP_*` / `FP_ESK_*` consts.
    //   4. Drop the `_pending` suffix and rerun scoped mutants.
    //
    // File safe — not in any running mutation service.

    /// Stage 11.A8c-L — STATE-MACHINE fingerprint pre-draft targeting
    /// the **14 `encode_general_purpose` survivors** at
    /// L799/818/831/850/855/864 (per v2 missed.txt).
    ///
    /// The 3-state encoder (Numeric / Alphanumeric / Iso646) dispatches
    /// per-byte through:
    /// * Numeric pair encode (digits + FNC1 sentinel)
    /// * Numeric lone-tail in rem ∈ 4..=6 — `c - 47` arithmetic (L799)
    /// * Numeric → Alpha latch on non-digit pair
    /// * Alpha → Numeric latch via the compound
    ///   `nr >= 6 || (nr >= 4 && i + nr as usize == gpf.len())` (L831)
    /// * Alpha FNC1 → implicit Numeric latch (`i += 1` at L818)
    /// * Alpha → Iso646 latch on iso646-only byte
    /// * Iso646 FNC1 → implicit Numeric latch (`i += 1` at L850)
    /// * Iso646 → Numeric latch via `nr >= 4 && nio >= 10` (L855)
    /// * Iso646 → Alpha latch via `ar >= 5 && nio >= 10` (L864)
    ///
    /// 12 cases — one per surviving-mutant locus, plus baselines:
    ///
    /// 1. `pure_num_pair`     — 2 digits, pair encodes (baseline Numeric).
    /// 2. `lone_num_rem456`   — odd-length digit run with bits_before
    ///    aligned so the lone digit's `rem` is in 4..=6 → pins L799.
    /// 3. `lone_num_pad_fnc1` — odd-length digit run with rem outside
    ///    4..=6 → pair-with-FNC1 path (the L799 sibling branch).
    /// 4. `num_to_alpha`      — non-digit pair forces N→A latch.
    /// 5. `alpha_fnc1_back`   — `FNC1` mid-alpha → implicit N latch
    ///    (pins L818 `i += 1` arithmetic).
    /// 6. `alpha_to_iso_only` — iso646-only byte mid-alpha → A→I latch.
    /// 7. `alpha_nr6_latch`   — alpha run followed by ≥6 numeric pairs
    ///    → first half of L831 disjunction fires.
    /// 8. `alpha_nr4_eom`     — alpha run followed by 4 num pairs at EOM
    ///    → second half of L831 (the `i + nr == gpf.len()` clause).
    /// 9. `iso_fnc1_back`     — `FNC1` mid-iso → implicit N latch (L850).
    /// 10. `iso_to_num`       — iso run with `nr>=4 && nio>=10` → L855.
    /// 11. `iso_to_alpha`     — iso run with `ar>=5 && nio>=10` → L864.
    /// 12. `mixed_round_trip` — N→A→I→A→N round-trip covering all 3
    ///     mode-transition edges in one input.
    ///
    /// Each fingerprint tuple is `(final_mode_tag, len, position_weighted_hash)`.
    /// `final_mode_tag` = 0/1/2 for Numeric/Alphanumeric/Iso646. Any
    /// mutation that changes a bit, a count, or the chosen mode at EOM
    /// shifts the fingerprint and fails the corresponding `assert_eq!`.
    #[test]
    fn encode_general_purpose_state_machine_fingerprint_pinned() {
        fn fp(out: &[u8], mode: CharsetMode) -> (u8, usize, u64) {
            let mut s: u64 = 0;
            for (i, &b) in out.iter().enumerate() {
                s = s.wrapping_add(
                    (b as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            let mode_tag = match mode {
                CharsetMode::Numeric => 0u8,
                CharsetMode::Alphanumeric => 1,
                CharsetMode::Iso646 => 2,
            };
            (mode_tag, out.len(), s)
        }
        const F: u8 = FNC1_SENTINEL_BYTE;
        // (tag, gpf, bits_before_gpf, segments, want)
        // segments fixed to STACKED_SEGMENTS so rembits is deterministic.
        let cases: &[(&str, &[u8], usize, usize, (u8, usize, u64))] = &[
            // (1) baseline 2 digits — numeric pair path.
            ("pure_num_pair", b"12", 24, STACKED_SEGMENTS, FP_EGP_PURE),
            // (2) lone digit with rem in 4..=6 — pins L799 `c - 47`.
            (
                "lone_num_rem456",
                b"7",
                30,
                STACKED_SEGMENTS,
                FP_EGP_LONE_R456,
            ),
            // (3) lone digit with rem outside 4..=6 — pair-with-FNC1.
            (
                "lone_num_pad_fnc1",
                b"7",
                0,
                STACKED_SEGMENTS,
                FP_EGP_LONE_PAD,
            ),
            // (4) non-digit pair forces N→A latch.
            ("num_to_alpha", b"AB", 24, STACKED_SEGMENTS, FP_EGP_N_TO_A),
            // (5) FNC1 mid-alpha — implicit N latch (L818 `i += 1`).
            (
                "alpha_fnc1_back",
                &[b'A', b'B', F, b'1', b'2'],
                24,
                STACKED_SEGMENTS,
                FP_EGP_ALPHA_FNC1,
            ),
            // (6) iso646-only byte mid-alpha → A→I latch.
            //     '?' (0x3F) is alphanumeric AND iso646 — need a strictly
            //     iso646-only byte. Use '!' (0x21) — encoded only in iso646.
            (
                "alpha_to_iso_only",
                b"AB!Z",
                24,
                STACKED_SEGMENTS,
                FP_EGP_A_TO_I,
            ),
            // (7) alpha then ≥6 numeric pairs → first half of L831.
            (
                "alpha_nr6_latch",
                b"AB123456789012345",
                24,
                STACKED_SEGMENTS,
                FP_EGP_NR6,
            ),
            // (8) alpha then 4 numeric pairs at EOM → second half of L831.
            (
                "alpha_nr4_eom",
                b"AB12345678",
                24,
                STACKED_SEGMENTS,
                FP_EGP_NR4_EOM,
            ),
            // (9) FNC1 mid-iso → implicit N latch (L850 `i += 1`).
            //     '!' enters iso, 'a' is iso646-only (lowercase), FNC1
            //     triggers numeric, digits stay numeric.
            (
                "iso_fnc1_back",
                &[b'!', b'a', F, b'1', b'2'],
                24,
                STACKED_SEGMENTS,
                FP_EGP_ISO_FNC1,
            ),
            // (10) iso run then numeric — '!a' enters iso, digits stay
            //      in iso unless conditions for switch fire.
            (
                "iso_to_num",
                b"!a12345678",
                24,
                STACKED_SEGMENTS,
                FP_EGP_I_TO_N,
            ),
            // (11) iso run then alphanumeric — '!a' enters iso, then
            //      uppercase letters (alphanumeric ⊂ iso).
            (
                "iso_to_alpha",
                b"!aABCDEZ",
                24,
                STACKED_SEGMENTS,
                FP_EGP_I_TO_A,
            ),
            // (12) mixed N→A→I→A→N round-trip.
            (
                "mixed_round_trip",
                b"12AB!Z01234567",
                24,
                STACKED_SEGMENTS,
                FP_EGP_ROUND,
            ),
        ];
        for (tag, gpf, bbg, seg, want) in cases {
            let (bits, mode) = encode_general_purpose(gpf, *bbg, *seg)
                .unwrap_or_else(|e| panic!("encode_general_purpose({tag}) ok: {e:?}"));
            let got = fp(&bits, mode);
            eprintln!("CAP encode_general_purpose/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_EGP_PURE: (u8, usize, u64) = (0, 7, 39816536415);
    const FP_EGP_LONE_R456: (u8, usize, u64) = (0, 7, 69015329786);
    const FP_EGP_LONE_PAD: (u8, usize, u64) = (0, 7, 69015329786);
    const FP_EGP_N_TO_A: (u8, usize, u64) = (1, 16, 84941944352);
    const FP_EGP_ALPHA_FNC1: (u8, usize, u64) = (0, 28, 499033923068);
    const FP_EGP_A_TO_I: (u8, usize, u64) = (2, 36, 735278705797);
    const FP_EGP_NR6: (u8, usize, u64) = (0, 72, 3697629015073);
    const FP_EGP_NR4_EOM: (u8, usize, u64) = (0, 47, 1481175154638);
    const FP_EGP_ISO_FNC1: (u8, usize, u64) = (0, 36, 923743644828);
    const FP_EGP_I_TO_N: (u8, usize, u64) = (0, 55, 2075768765102);
    const FP_EGP_I_TO_A: (u8, usize, u64) = (1, 65, 2309359112070);
    const FP_EGP_ROUND: (u8, usize, u64) = (0, 78, 3084454354282);

    /// Stage 11.A8c-L — STATE-MACHINE fingerprint pre-draft targeting
    /// the **21 `encode_stacked` survivors** at
    /// L1322/1327/1353/1391/1410/1412/1415 (per v2 missed.txt).
    ///
    /// `encode_stacked` is a 7-stage pipeline:
    /// 1. GS1 parse + method dispatch (already covered by cfb68ae).
    /// 2. Build `gpf_bytes` + call `encode_general_purpose` with
    ///    `bits_before_gpf = 1 + 12 + method.method_bits.len()
    ///     + method.vlf_len + method.cdf.len()` (L1322).
    /// 3. `encode_to_dxw_and_seq` → `(dxw, seq, datalen)` and
    ///    `data_chars = datalen - 1` (L1327).
    /// 4. Per-row sbs build with the
    ///    `STACKED_SEGMENTS % 4 != 0 && r % 2 == 1` 0-pad guard (L1353).
    /// 5. Odd-row reversal/prepend gated on
    ///    `length_differs && finder_count % 2 == 1` (L1391).
    /// 6. Last-row pad gated on `numrows > 1` (L1410) and the
    ///    `row_bits[last].len() < pixx` / `row_seps[last].len() < pixx`
    ///    resize predicates (L1412/1415).
    /// 7. Strip concat + BitMatrix output.
    ///
    /// 10 cases chosen to span the row-count axis (1, 2, 3+ rows) and
    /// each datalen-mod-STACKED_SEGMENTS class so the last-row gating
    /// at L1410/1412/1415 fires distinctly per case:
    ///
    /// 1. `single_gtin`         — minimal (01)+GTIN: 1 row, no last-row
    ///    padding (pins L1410 `numrows > 1 == false`).
    /// 2. `gtin_link`           — same with linkage bit set — verifies
    ///    L1322 `+ method.method_bits.len()` for the linkage path.
    /// 3. `two_row_balanced`    — datalen exactly STACKED_SEGMENTS*2 →
    ///    last row full, L1412/1415 predicates FALSE (no resize).
    /// 4. `two_row_short`       — datalen STACKED_SEGMENTS+1 → last
    ///    row HAS 1 element only, L1412/1415 TRUE (resize fires).
    /// 5. `three_row_short`     — 3+ row case with short trailing row,
    ///    exercises both inter-row sep + last-row resize together.
    /// 6. `three_row_full`      — datalen exactly 3*STACKED_SEGMENTS,
    ///    full last row (L1412/1415 FALSE on the 3-row branch).
    /// 7. `vlf_path`            — method_00 with long lot AI to force
    ///    vlf_len > 0 (pins L1322 `method.vlf_len` term).
    /// 8. `cdf_long`            — method_0111 date variant — exercises
    ///    `method.cdf.len()` in L1322 with a non-trivial cdf width.
    /// 9. `gpf_alpha_run`       — trailing-AI alpha run pushes
    ///    encode_general_purpose into Alphanumeric mode at EOM —
    ///    fp shifts vs the all-numeric variant by tracking the latch.
    /// 10. `gpf_iso_mid`        — trailing-AI iso646 byte forces an
    ///    A→I latch mid-gpf — exercises the L1353 even/odd row guard
    ///    once datalen lands on an odd boundary.
    ///
    /// Each fingerprint tuple is `(width, height, position_weighted_hash)`
    /// over the BitMatrix bits. Mirrors `fp_bm` from
    /// `dbexp_encode_and_stacked_fingerprint_pinned`.
    #[test]
    fn encode_stacked_state_machine_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // (tag, input, linkage, want)
        let cases: &[(&str, &str, bool, (usize, usize, u64))] = &[
            (
                "single_gtin",
                "(01)90012345678908",
                false,
                FP_ESK_SINGLE_GTIN,
            ),
            ("gtin_link", "(01)90012345678908", true, FP_ESK_GTIN_LINK),
            (
                "two_row_balanced",
                "(01)90012345678908(3103)001750",
                false,
                FP_ESK_TWO_BAL,
            ),
            (
                "two_row_short",
                "(01)90012345678908(10)A",
                false,
                FP_ESK_TWO_SHORT,
            ),
            (
                "three_row_short",
                "(01)90012345678908(3103)001750(10)BATCH12345",
                false,
                FP_ESK_THREE_SHORT,
            ),
            (
                "three_row_full",
                "(01)90012345678908(3103)001750(10)BATCHLOT1234X",
                false,
                FP_ESK_THREE_FULL,
            ),
            ("vlf_path", "(10)LOT123(99)9876543", false, FP_ESK_VLF),
            (
                "cdf_long",
                "(01)90012345678908(3103)001750(13)260101",
                false,
                FP_ESK_CDF,
            ),
            (
                "gpf_alpha_run",
                "(01)90012345678908(10)ABCDEFGH",
                false,
                FP_ESK_GPF_ALPHA,
            ),
            (
                "gpf_iso_mid",
                "(01)90012345678908(10)AB!CD",
                false,
                FP_ESK_GPF_ISO,
            ),
        ];
        for (tag, input, link, want) in cases {
            let bm = encode_stacked(input, *link)
                .unwrap_or_else(|e| panic!("encode_stacked({tag}) ok: {e:?}"));
            let got = fp_bm(&bm);
            eprintln!("CAP encode_stacked/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_ESK_SINGLE_GTIN: (usize, usize, u64) = (102, 71, 21699701777190963);
    const FP_ESK_GTIN_LINK: (usize, usize, u64) = (102, 71, 22286127688818366);
    const FP_ESK_TWO_BAL: (usize, usize, u64) = (102, 71, 23659652201169011);
    const FP_ESK_TWO_SHORT: (usize, usize, u64) = (102, 71, 35298684095342239);
    const FP_ESK_THREE_SHORT: (usize, usize, u64) = (102, 145, 108701318395838259);
    const FP_ESK_THREE_FULL: (usize, usize, u64) = (102, 145, 138945001205362538);
    const FP_ESK_VLF: (usize, usize, u64) = (102, 71, 34602011503253223);
    const FP_ESK_CDF: (usize, usize, u64) = (102, 71, 37155586668642506);
    const FP_ESK_GPF_ALPHA: (usize, usize, u64) = (102, 108, 60258898333099288);
    const FP_ESK_GPF_ISO: (usize, usize, u64) = (102, 108, 64523294508553725);

    /// Stage 11.A8d — regression for the fuzz crash at
    /// databar_expanded.rs:1636 ("data_chars must be 2..=21, got 25").
    /// GS1 input that packs past the 21-data-character ceiling must now
    /// surface a graceful `Error::InvalidData` instead of tripping
    /// `finder_sequence`'s bounds assert. Both the linear (`encode`) and
    /// the stacked (`encode_stacked`) public entries route through
    /// `encode_to_dxw_and_seq`, so both are pinned.
    #[test]
    fn oversized_input_returns_error_not_panic() {
        // A long (10) batch field — alphanumeric, variable-length — packs
        // far more than 21 data characters. Reproduces the fuzz reproducer
        // class (`(99)…(99)…` overlong GS1) via a clean deterministic input.
        let long = format!("(10){}", "A".repeat(60));
        let lin = encode(&long, false);
        assert!(
            matches!(lin, Err(crate::error::Error::InvalidData(_))),
            "oversized linear input must be InvalidData, got {lin:?}"
        );
        let stk = encode_stacked(&long, false);
        assert!(
            matches!(stk, Err(crate::error::Error::InvalidData(_))),
            "oversized stacked input must be InvalidData, got {stk:?}"
        );
        // A within-capacity input still encodes cleanly (no over-eager
        // rejection): a single GTIN is 4 data characters.
        assert!(
            encode("(01)90012345678908", false).is_ok(),
            "in-capacity GTIN must still encode"
        );
    }

    // ====================================================================
    // Stage 11.A8d — databar_expanded T2-a residual-survivor killers.
    //
    // The state-machine fingerprint tests above (encode_general_purpose,
    // encode_stacked) killed the bulk of the mutants. The tests below
    // target the 58 residual survivors from
    // target/mutants-databar_expanded-v4/mutants.out/missed.txt that the
    // fingerprints did not distinguish — almost all are in method-decline
    // / boundary / dead-value paths the success-path fingerprints never
    // exercise. Each test is keyed to specific file:line:operator
    // mutations; equivalence proofs live in
    // `databar_expanded_equivalence_notes`.
    // ====================================================================

    /// Construct a parsed-element list inline (mirrors the
    /// `crate::util::gs1::Element` shape the dispatchers consume).
    fn el(ai: &str, data: &str) -> crate::util::gs1::Element {
        crate::util::gs1::Element {
            ai: ai.to_string(),
            data: data.to_string(),
        }
    }

    /// L513:11 `> → >=` in `encode_method_0100` — `if v1 > 32767`.
    /// At the boundary `v1 == 32767` the method must still encode
    /// (return `Some`); the `>=` mutant rejects it. One past the
    /// boundary (`32768`) must decline under both, so we pin both sides.
    #[test]
    fn method_0100_weight_boundary_32767() {
        let at = vec![el("01", "90012345678908"), el("3103", "032767")];
        let r = encode_method_0100(&at).expect("0100 ok").expect(
            "weight 32767 is the inclusive max — `> 32767` is false so 0100 must encode; \
             the `>= 32767` mutant wrongly declines here",
        );
        assert_eq!(r.method_bits, vec![0, 1, 0, 0]);
        let over = vec![el("01", "90012345678908"), el("3103", "032768")];
        assert!(
            encode_method_0100(&over).expect("0100 ok").is_none(),
            "weight 32768 exceeds the cap — both original and mutant decline"
        );
    }

    /// L543/L548 in `encode_method_0101`:
    ///  * L543:18 `> → ==` and `> → >=` on `if v > 9999` (3202 path);
    ///  * L548:18 `> → >=` on `if v > 22767` (3203 path).
    /// Boundary values must encode; the mutants flip the decision at
    /// the exact boundary.
    #[test]
    fn method_0101_weight_boundaries() {
        // 3202: v == 9999 must encode (`> 9999` false). `>=` and `==`
        // mutants both decline at 9999.
        let at = vec![el("01", "90012345678908"), el("3202", "009999")];
        let r = encode_method_0101(&at).expect("0101 ok").expect(
            "3202 weight 9999 is the inclusive max — `> 9999` false so must encode; \
             `>= 9999` and `== 9999` mutants wrongly decline",
        );
        assert_eq!(r.method_bits, vec![0, 1, 0, 1]);
        // 3202: v == 10000 must decline (`> 9999` true). The `== 9999`
        // mutant evaluates `10000 == 9999` = false and would WRONGLY
        // proceed to encode — so asserting decline kills `== 9999`.
        let over = vec![el("01", "90012345678908"), el("3202", "010000")];
        assert!(
            encode_method_0101(&over).expect("0101 ok").is_none(),
            "3202 weight 10000 must decline — the `== 9999` mutant would encode"
        );
        // 3203: v == 22767 must encode (`> 22767` false). `>= 22767`
        // declines at the boundary.
        let at3 = vec![el("01", "90012345678908"), el("3203", "022767")];
        let r3 = encode_method_0101(&at3).expect("0101 ok").expect(
            "3203 weight 22767 is the inclusive max — `> 22767` false so must encode; \
             `>= 22767` mutant wrongly declines",
        );
        assert_eq!(r3.method_bits, vec![0, 1, 0, 1]);
    }

    /// L936-940 `|| → &&` chain + L953 `|| → &&` + L959 + L962 in
    /// `encode_method_0111`. Because `&&` binds tighter than `||`,
    /// mutating one `||` re-groups the validation disjunction; each
    /// near-miss input below makes exactly one disjunct true so the
    /// original declines (`Ok(None)`) but the regrouped mutant slips
    /// through (returning `Some`, or `Err`/panic on the conv paths).
    #[test]
    fn method_0111_validation_disjunction_each_operand() {
        let mk = |gtin: &str, w: &str| vec![el("01", gtin), el("3103", w)];
        // L937 `(len!=14 && !alldigit) || ...`: len 13, all-digit,
        // starts '9'. Original declines on len; mutant proceeds → Some.
        assert!(
            matches!(encode_method_0111(&mk("9001234567890", "000123")), Ok(None)),
            "L937: 13-digit GTIN must decline; `&&` mutant slips through"
        );
        // L938 `len!=14 || (!alldigit && starts!='9') || ...`: 14 chars,
        // has non-digit, starts '9'. Original declines on !alldigit;
        // mutant proceeds → conv12to40 errors. Ok(None) distinguishes.
        assert!(
            matches!(
                encode_method_0111(&mk("900123456789X8", "000123")),
                Ok(None)
            ),
            "L938: non-digit GTIN must Ok(None)-decline; `&&` mutant errors/proceeds"
        );
        // L939 `... || (starts!='9' && len!=6) || ...`: 14 digits not
        // starting '9'. Original declines; mutant proceeds → Some.
        assert!(
            matches!(
                encode_method_0111(&mk("10012345678908", "000123")),
                Ok(None)
            ),
            "L939: GTIN not starting '9' must decline; `&&` mutant slips through"
        );
        // L940 `... || (v1.len()!=6 && !v1digit)`: v1 is 5 digits.
        // Original declines on length; mutant proceeds → Some.
        assert!(
            matches!(encode_method_0111(&mk("90012345678908", "12345")), Ok(None)),
            "L940: 5-digit (3103) value must decline; `&&` mutant slips through"
        );
        // L953 `d.len()!=6 || !ddigit` → `&&`: a 7-digit all-digit date.
        // Original declines on length; mutant proceeds (parse of first 6
        // succeeds) → Some.
        let date7 = vec![
            el("01", "90012345678908"),
            el("3103", "001750"),
            el("11", "2501011"),
        ];
        assert!(
            matches!(encode_method_0111(&date7), Ok(None)),
            "L953: 7-digit date must decline; `&&` mutant slips through"
        );
        // L959:36 `!(1..=12).contains(mm) || dd>31` → `&&`: mm=13, dd=01.
        // Original declines on month; mutant proceeds → Some.
        let badmonth = vec![
            el("01", "90012345678908"),
            el("3103", "001750"),
            el("11", "251301"),
        ];
        assert!(
            matches!(encode_method_0111(&badmonth), Ok(None)),
            "L959:36: month 13 must decline; `&&` mutant slips through"
        );
        // L959:42 `dd > 31` → `==` / `>=`: dd == 31 is a VALID day and
        // must encode. Both `== 31` and `>= 31` mutants reject dd==31.
        let day31 = vec![
            el("01", "90012345678908"),
            el("3103", "001750"),
            el("11", "250131"),
        ];
        assert!(
            encode_method_0111(&day31).expect("0111 ok").is_some(),
            "L959:42: day 31 is valid (`dd > 31` false) — `== 31`/`>= 31` mutants reject it"
        );
    }

    /// L962:25 `+ → -` and L962:36 `* → /` in the date-encode
    /// arithmetic `yy * 384 + (mm - 1) * 32 + dd`. A date with `mm > 1`
    /// makes `(mm - 1) * 32` non-zero so both the additive term and the
    /// multiply matter; we pin the exact 76-bit cdf.
    #[test]
    fn method_0111_date_encode_arithmetic_exact_cdf() {
        // (13)250601 → yy=25, mm=6, dd=1 → 25*384 + 5*32 + 1 = 9761.
        let els = vec![
            el("01", "90012345678908"),
            el("3103", "001750"),
            el("13", "250601"),
        ];
        let m = encode_method_0111(&els)
            .expect("0111 ok")
            .expect("must encode");
        let want_cdf: Vec<u8> = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1,
            1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1,
            1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1,
        ];
        assert_eq!(
            m.cdf, want_cdf,
            "date_encoded = 25*384 + (6-1)*32 + 1 = 9761; the `+ → -` and `* → /` mutants \
             change the last 16 cdf bits"
        );
    }

    /// L1006 delete arm "3921" and L1008 delete arm "3923" in
    /// `encode_method_01100`. Deleting an arm routes that AI to the
    /// `_ => return Ok(None)` fallback. Each arm must produce `Some`
    /// with its distinct `last_digit` reflected in cdf bits 40..42.
    #[test]
    fn method_01100_392x_arm_coverage() {
        for (ai, bits) in [
            ("3920", [0u8, 0]),
            ("3921", [0, 1]),
            ("3922", [1, 0]),
            ("3923", [1, 1]),
        ] {
            let els = vec![el("01", "90012345678908"), el(ai, "1234")];
            let m = encode_method_01100(&els)
                .expect("01100 ok")
                .unwrap_or_else(|| {
                    panic!("({ai}) must match method 01100 (deleted arm regresses to None)")
                });
            assert_eq!(
                &m.cdf[40..42],
                &bits,
                "({ai}) last-digit cdf bits 40..42 pin the match arm"
            );
        }
    }

    /// L1012:23 / L1012:66 `|| → &&` (GTIN validation) and L1016:28
    /// `|| → &&` (value-non-empty) in `encode_method_01100`.
    #[test]
    fn method_01100_validation_disjunction() {
        // L1012:23 `(len!=14 && !alldigit) || starts!='9'`: 13 digits,
        // starts '9'. Original declines on length; mutant proceeds → Some.
        assert!(
            matches!(
                encode_method_01100(&[el("01", "9001234567890"), el("3920", "1234")]),
                Ok(None)
            ),
            "L1012:23: 13-digit GTIN must decline"
        );
        // L1012:66 `len!=14 || (!alldigit && starts!='9')`: 14 chars,
        // non-digit, starts '9'. Original Ok(None); mutant proceeds → Err.
        assert!(
            matches!(
                encode_method_01100(&[el("01", "900123456789X8"), el("3920", "1234")]),
                Ok(None)
            ),
            "L1012:66: non-digit GTIN must Ok(None)-decline"
        );
        // L1016:28 `is_empty() && !alldigit`: empty (392x) value. Original
        // declines (empty); mutant `true && false` proceeds → Some.
        assert!(
            matches!(
                encode_method_01100(&[el("01", "90012345678908"), el("3920", "")]),
                Ok(None)
            ),
            "L1016:28: empty (392x) value must decline"
        );
    }

    /// L1056 delete arm "3931" and L1058 delete arm "3933" in
    /// `encode_method_01101`.
    #[test]
    fn method_01101_393x_arm_coverage() {
        for (ai, bits) in [
            ("3930", [0u8, 0]),
            ("3931", [0, 1]),
            ("3932", [1, 0]),
            ("3933", [1, 1]),
        ] {
            let els = vec![el("01", "90012345678908"), el(ai, "840123")];
            let m = encode_method_01101(&els)
                .expect("01101 ok")
                .unwrap_or_else(|| {
                    panic!("({ai}) must match method 01101 (deleted arm regresses to None)")
                });
            assert_eq!(
                &m.cdf[40..42],
                &bits,
                "({ai}) last-digit cdf bits 40..42 pin the match arm"
            );
        }
    }

    /// L1062:23 / L1062:66 `|| → &&` (GTIN validation), L1066:23
    /// `< → ==` / `< → <=` and L1066:27 `|| → &&` (currency-prefix
    /// validation) in `encode_method_01101`.
    #[test]
    fn method_01101_validation_disjunction() {
        // L1062:23 (GTIN length): 13 digits, starts '9' → decline.
        assert!(
            matches!(
                encode_method_01101(&[el("01", "9001234567890"), el("3930", "840123")]),
                Ok(None)
            ),
            "L1062:23: 13-digit GTIN must decline"
        );
        // L1062:66 (GTIN non-digit): 14 chars non-digit starts '9' → Ok(None).
        assert!(
            matches!(
                encode_method_01101(&[el("01", "900123456789X8"), el("3930", "840123")]),
                Ok(None)
            ),
            "L1062:66: non-digit GTIN must Ok(None)-decline"
        );
        // L1066:23 `len < 3` → `== 3` / `<= 3`: a value of EXACTLY 3
        // currency digits must encode (`< 3` false). Both `== 3` and
        // `<= 3` mutants reject the 3-digit case.
        let three = vec![el("01", "90012345678908"), el("3930", "840")];
        assert!(
            encode_method_01101(&three).expect("01101 ok").is_some(),
            "L1066:23: a 3-digit (393x) value (currency only) must encode; `==3`/`<=3` reject it"
        );
        // L1066:27 `len < 3 && !digit[..3]`: a 2-digit value. Original
        // short-circuits to Ok(None); the `&&` mutant evaluates the RHS
        // `v1_bytes[..3]` slice on a 2-byte array → panics. Asserting a
        // clean Ok(None) therefore kills the mutant (it panics instead).
        assert!(
            matches!(
                encode_method_01101(&[el("01", "90012345678908"), el("3930", "84")]),
                Ok(None)
            ),
            "L1066:27: a 2-digit value must Ok(None)-decline (mutant panics on [..3] slice)"
        );
    }

    /// L1079:23 `> → ==` / `> → <` / `> → >=` in `encode_method_01101`
    /// — `if elements.len() > 2` decides whether to append the FNC1
    /// sentinel to the gpf prefix.
    ///  * len == 2 (no trailing AI): original appends NO FNC1; the
    ///    `== 2` and `>= 2` mutants WRONGLY append it.
    ///  * len == 3 (one trailing AI): original appends FNC1; the
    ///    `< 2` mutant WRONGLY omits it.
    #[test]
    fn method_01101_trailing_fnc1_gate() {
        let two = vec![el("01", "90012345678908"), el("3932", "840123")];
        let m2 = encode_method_01101(&two).expect("ok").expect("some");
        assert_eq!(
            m2.gpf_prefix_bytes,
            vec![b'1', b'2', b'3'],
            "len==2: gpf prefix is v1[3..] with NO trailing FNC1; `==2`/`>=2` mutants append one"
        );
        let three = vec![
            el("01", "90012345678908"),
            el("3932", "840123"),
            el("10", "AB"),
        ];
        let m3 = encode_method_01101(&three).expect("ok").expect("some");
        assert_eq!(
            m3.gpf_prefix_bytes,
            vec![b'1', b'2', b'3', FNC1_SENTINEL_BYTE],
            "len==3: gpf prefix ends with the FNC1 sentinel; `< 2` mutant omits it"
        );
    }

    /// L1104:28 `|| → &&` in `encode_method_1` —
    /// `if elements.is_empty() || elements[0].ai != "01"`. With `&&`
    /// the empty-slice case evaluates `elements[0]` and panics; the
    /// original short-circuits to `Ok(None)`.
    #[test]
    fn method_1_empty_elements_ok_none() {
        assert!(
            matches!(encode_method_1(&[]), Ok(None)),
            "empty element list must Ok(None)-decline; the `&&` mutant panics on elements[0]"
        );
    }

    /// L1249:65 `+ → *` in `encode` — `bits_before_gpf =
    /// 1 + 12 + method.method_bits.len() + method.vlf_len + ...`. For
    /// method 1, `method_bits.len()==1` and `vlf_len==2`, so `len + vlf`
    /// = 3 but `len * vlf` = 2. The gpf "101" (trailing (10)1) hits the
    /// numeric lone-digit rembits special case where bbg 59 vs 60 yields
    /// a different final pattern (probed: differ).
    #[test]
    fn encode_bits_before_gpf_lone_digit_golden() {
        let pat = encode("(01)90012345678908(10)1", false).expect("encode ok");
        let want: &[u8] = &[
            1, 1, 2, 3, 1, 1, 2, 5, 2, 1, 8, 4, 1, 1, 4, 1, 1, 2, 1, 4, 1, 3, 1, 1, 4, 2, 2, 1, 5,
            1, 1, 1, 4, 6, 3, 3, 1, 1, 2, 4, 2, 1, 3, 3, 4, 1, 2, 1, 1, 1, 4, 3, 6, 4, 1, 1, 2, 3,
            3, 2, 1, 4, 1, 1, 1, 1,
        ];
        assert_eq!(
            pat.bars, want,
            "method-1 + lone trailing digit: the `len * vlf` mutant shifts bits_before_gpf \
             from 60 to 59, changing the lone-digit rembits encoding"
        );
    }

    /// L1322:65 `+ → *` in `encode_stacked` — same `bits_before_gpf`
    /// arithmetic, pinned via the stacked BitMatrix fingerprint.
    #[test]
    fn encode_stacked_bits_before_gpf_lone_digit_fingerprint() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        let bm = encode_stacked("(01)90012345678908(10)1", false).expect("stacked ok");
        assert_eq!(
            fp_bm(&bm),
            (102, 71, 22671862330299603),
            "stacked method-1 + lone trailing digit: `len * vlf` mutant changes bits_before_gpf \
             → different module grid"
        );
    }

    /// L1493:25 `- → +` in `compute_row_sep` — `let max = n - 13`
    /// bounds which finder positions are processed. At `n = 129` the
    /// 19-chain position 117 is EXCLUDED by `n - 13 = 116` but INCLUDED
    /// by `n + 13 = 142`; the all-zero row's separator then alternates
    /// across 117.. instead of staying solid. We pin the exact set of
    /// `sep == 1` indices.
    #[test]
    fn compute_row_sep_max_bound_n129() {
        let z = vec![0u8; 129];
        let ones: Vec<usize> = compute_row_sep(&z)
            .iter()
            .enumerate()
            .filter(|(_, &b)| b == 1)
            .map(|(i, _)| i)
            .collect();
        let want: Vec<usize> = vec![
            4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28, 30, 32, 34,
            35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
            57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 69, 71, 73, 75, 77, 79, 81, 83, 84, 85, 86,
            87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106,
            107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123,
            124,
        ];
        assert_eq!(
            ones, want,
            "n=129: only finders 19 and 68 fire (max=116 excludes 117); the `n + 13` mutant \
             also fires finder 117, alternating the 117.. region"
        );
    }

    /// L1497:19 `+= → *=` (19-chain stride) and L1502:19 `+= → *=`
    /// (68-chain stride) in `compute_row_sep`'s finder-position walk.
    ///  * n=140: the 19-chain must reach 117 (`19 += 98`); the `*=`
    ///    mutant jumps to 19*98 = 1862 and drops 117.
    ///  * n=185: the 68-chain must reach 166 (`68 += 98`); the `*=`
    ///    mutant jumps to 68*98 and drops 166.
    #[test]
    fn compute_row_sep_finder_stride_additive() {
        let ones = |n: usize| -> Vec<usize> {
            compute_row_sep(&vec![0u8; n])
                .iter()
                .enumerate()
                .filter(|(_, &b)| b == 1)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        let want140: Vec<usize> = vec![
            4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28, 30, 32, 34,
            35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
            57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 69, 71, 73, 75, 77, 79, 81, 83, 84, 85, 86,
            87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106,
            107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 118, 120, 122, 124, 126, 128, 130,
            132, 133, 134, 135,
        ];
        assert_eq!(
            ones(140),
            want140,
            "n=140: the 19-chain must step 19→117 via `+= 98`; the `*= 98` mutant drops finder 117"
        );
        let want185: Vec<usize> = vec![
            4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28, 30, 32, 34,
            35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
            57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 69, 71, 73, 75, 77, 79, 81, 83, 84, 85, 86,
            87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106,
            107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 118, 120, 122, 124, 126, 128, 130,
            132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148,
            149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165,
            167, 169, 171, 173, 175, 177, 179,
        ];
        assert_eq!(
            ones(185),
            want185,
            "n=185: the 68-chain must step 68→166 via `+= 98`; the `*= 98` mutant drops finder 166"
        );
    }

    /// L1508:37 `- → +` and `- → /` in `compute_row_sep` —
    /// `for i in p..=(p + 14).min(n - 1)`. At `n = 33` finder 19 fires
    /// (max = 20) and its region `19..=33` is clamped by `n - 1 = 32`.
    /// The `n + 1` (→ 34) and `n / 1` (→ 33) mutants both raise the clamp
    /// to an out-of-bounds index → panic. The original completes with a
    /// well-defined separator, which we pin.
    #[test]
    fn compute_row_sep_finder_clamp_n33() {
        let z = vec![0u8; 33];
        let ones: Vec<usize> = compute_row_sep(&z)
            .iter()
            .enumerate()
            .filter(|(_, &b)| b == 1)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            ones,
            vec![4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28],
            "n=33: finder 19 region clamps to n-1=32; the `n+1`/`n/1` mutants index out of bounds \
             (panic) instead of completing"
        );
    }

    /// Stage 11.A8d — EQUIVALENCE / UNREACHABLE proofs for the residual
    /// survivors that cannot be killed because the mutated operator is
    /// provably observationally identical to the original (closed-form
    /// arguments), each backed by an executable witness here.
    #[test]
    fn databar_expanded_equivalence_notes() {
        // ---- L239:13 `< → <=` in `tab174_row_for` (`while j < len`) ----
        // The loop returns as soon as `value <= TAB_174[j]`. The five
        // thresholds are at j = 0,8,16,24,32; the top threshold 4191 sits
        // at j=32. For every valid 12-bit value (0..=4191) the function
        // returns before j reaches 40, so `j < 40` and `j <= 40` are
        // indistinguishable on the return path. The only divergence is
        // the post-loop panic for value > 4191: `< 40` panics with the
        // custom message after j hits 40; `<= 40` would index TAB_174[40]
        // (out of bounds) and panic too. Both panic on the same inputs,
        // and no in-range value reaches the loop tail. NOTE: this is the
        // single survivor we treat as effectively equivalent on the
        // success path; the boundary value 4191 still returns the top row
        // identically under both. Witness: every threshold returns the
        // correct row well before the tail.
        assert_eq!(
            tab174_row_for(4191).gs,
            3988,
            "top threshold returns before tail"
        );
        assert_eq!(tab174_row_for(0).gs, 0, "first threshold returns at j=0");

        // ---- L341:22 `| → ^` in `pack_12_bits` (`v = (v << 1) | bit`) ----
        // Closed form: `v << 1` always has bit 0 clear, so for any bit
        // b ∈ {0,1}, `(v<<1) | b == (v<<1) ^ b == (v<<1) + b`. OR and XOR
        // coincide whenever the operands share no set bits. Witness: an
        // all-ones input and the oracle codeword both reproduce exactly.
        assert_eq!(
            pack_12_bits(&[1u8; 12]),
            4095,
            "OR == XOR when low bit is clear"
        );
        assert_eq!(pack_12_bits(&[0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0]), 1680);

        // ---- L425:16 `> → >=` and L431:28 `< → <=` in `assemble_binval` ----
        // L425 `if pad_len > 0` guards the fill-pattern loop. When
        // pad_len == 0 the loop body `for i in 0..pad_len` is empty, so
        // entering the guarded block (`>= 0`, always true) runs a
        // zero-iteration loop and pushes nothing — identical output to
        // skipping it. L431 `if i < shift` selects leading zeros; with
        // `<= shift` index i==shift would emit 0 instead of
        // FILL_PAT[(i-shift)%5] = FILL_PAT[0] = 0 — but FILL_PAT[0] is
        // ALSO 0, so the two branches produce the same bit at i==shift.
        // Closed-form equivalent (FILL_PAT == [0,0,1,0,0], so index 0 is
        // 0). Witness: a numeric-mode pad (shift=4) and a non-numeric pad
        // both round-trip through the public encoder unchanged.
        assert_eq!(
            FILL_PAT[0], 0,
            "FILL_PAT[0]==0 makes the i<=shift boundary a no-op"
        );
        let n = assemble_binval(
            0,
            &[1],
            2,
            &[0u8; 44],
            &[],
            CharsetMode::Numeric,
            DEFAULT_SEGMENTS,
        );
        assert_eq!(
            n.len() % 12,
            0,
            "numeric-mode pad assembles to whole codewords"
        );
        let a = assemble_binval(
            0,
            &[1],
            2,
            &[0u8; 44],
            &[],
            CharsetMode::Alphanumeric,
            DEFAULT_SEGMENTS,
        );
        assert_eq!(
            a.len() % 12,
            0,
            "non-numeric pad assembles to whole codewords"
        );

        // ---- L1327:30 `- → +` / `- → /` in `encode_stacked` ----
        // `let data_chars = datalen - 1;` is immediately discarded by
        // `let _ = data_chars;` (L1343) and never read again. The value
        // is dead, so any arithmetic mutation is unobservable. Witness:
        // the public stacked encoder produces a stable fingerprint
        // regardless (covered by encode_stacked_* tests above).
        assert!(encode_stacked("(01)90012345678908", false).is_ok());

        // ---- L1353 `% → /`, `% → +`, `== → !=` in `encode_stacked` ----
        // The guard `if STACKED_SEGMENTS % 4 != 0 && r % 2 == 1`. With
        // STACKED_SEGMENTS == 4, `4 % 4 != 0` is `0 != 0` == false, so
        // the `&&` short-circuits BEFORE evaluating `r % 2 == 1`. The
        // mutated sub-expression (`r / 2`, `r + 2`, or `r % 2 != 1`) is
        // never reached → the leading-zero push never executes either
        // way. Closed-form dead-code equivalence.
        assert_eq!(
            STACKED_SEGMENTS % 4,
            0,
            "left conjunct is always false → RHS unreached"
        );

        // ---- L1410:16, L1412:33, L1415:33 in `encode_stacked` ----
        // The last-row padding block resizes `row_bits[last]` and
        // `row_seps[last]` UP TO `pixx` by appending zeros. The final
        // BitMatrix is allocated at width `pixx` and initialised to all
        // 0; each strip is written via `strip.iter().enumerate()`, which
        // only touches columns `0..strip.len()` and leaves the rest at
        // their 0 default. Padding a strip with 0s to width pixx is
        // therefore observationally identical to leaving it short.
        // Hence:
        //   * L1410 `numrows > 1` → `==`/`<`/`>=`: entering or skipping
        //     the block only ever triggers no-op zero-resizes (when
        //     numrows==1, last==0 and row_bits[0].len()==pixx already).
        //   * L1412/1415 `len < pixx` → `==`/`>`/`<=`: flipping whether
        //     the resize fires changes nothing because the resize pads
        //     with the same 0 the BitMatrix already holds.
        // Closed-form equivalence (no row-height term depends on the
        // resized length). Witness: a 2-row short-last-row symbol and a
        // 4-row symbol both encode to stable fingerprints (covered
        // above); height is driven by the fixed per-strip heights, not by
        // strip width.
        let bm = encode_stacked("(01)90012345678908(10)A", false).expect("ok");
        assert_eq!(
            bm.width(),
            102,
            "BitMatrix width is pixx; short last strip pads with 0 = default"
        );

        // ---- L1511:25 `> → >=` in `compute_row_sep` (`i > 0`) ----
        // The finder loop iterates `for i in p..=...` with p ∈ {19,68,...}
        // ≥ 19, so i ≥ 19 > 0 for every iteration. `i > 0` and `i >= 0`
        // are both unconditionally true here → identical. Witness: the
        // first finder position is 19.
        // ---- L1511:39 `- → /` in `compute_row_sep` (`row[i - 1]`) ----
        // In the branch we know `row[i] == 0`, so the original predicate
        // `row[i-1] == 1 || sep[i-1] == 0` and the mutant `row[i] == 1 ||
        // sep[i-1] == 0` differ only when `row[i-1] == 1 && sep[i-1] == 1`.
        // But whenever `row[i-1] == 1`, sep[i-1] is forced to 0: if i-1 is
        // in a finder region the `row==1 → 0` rule set it; if i-1 is
        // outside, sep[i-1] = complement = 1 - row[i-1] = 0. So
        // `row[i-1] == 1 ⟹ sep[i-1] == 0`, making the disagreement case
        // impossible. The mutant (`row[i]`, always false in this branch)
        // collapses to `sep[i-1] == 0`, exactly equal to the original.
        // Closed-form equivalence. Witness: the real 102-wide row sep
        // matches the oracle (asserted in encode_stacked_matches_oracle).
        let mut row = vec![0u8; 40];
        row[18] = 1;
        let sep = compute_row_sep(&row);
        assert_eq!(
            sep[19], 1,
            "i=19: row[18]=1 forces sep[18]=0, both predicates agree → sep[19]=1"
        );

        // ---- L1534:10 `< → <=` in `count_finder_positions` (`n < 13`) ----
        // The smallest n that yields any finder position is 32
        // (19 <= n - 13 ⟺ n >= 32). For every n <= 13 the count is 0
        // anyway: when n == 13, max = 0 and both 19 <= 0 and 68 <= 0 are
        // false → 0; the early `n < 13` return also gives 0. So `n < 13`
        // and `n <= 13` produce 0 on exactly the same inputs. Closed-form
        // equivalence. Witness: n=13 and n=12 both count 0.
        assert_eq!(count_finder_positions(13), 0, "n=13 counts 0 via the loop");
        assert_eq!(
            count_finder_positions(12),
            0,
            "n=12 counts 0 via early return"
        );
        assert_eq!(count_finder_positions(31), 0, "n=31: max=18 < 19, still 0");
        assert_eq!(count_finder_positions(32), 1, "n=32: first finder appears");
    }
}
