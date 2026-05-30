//! Australia Post 4-state customer barcode.
//!
//! Verified byte-for-byte against bwip-js's `bwipp_auspost` across all
//! four FCCs (11/45/59/62) and both customer-info modes:
//!
//! - **Character mode** (default): each suffix byte is looked up in the
//!   64-char `BARCHARS` alphabet and emitted as a 3-bar codeword.
//! - **Numeric mode** (`opts.extras["custinfoenc"] = "numeric"`): each
//!   suffix digit is emitted as a 2-bar codeword (same pattern used
//!   for FCC and DPID digits).
//!
//! Any leftover capacity in the custinfo region is padded with the
//! single-cell filler `"3"`, then the Reed-Solomon GF(64) check is
//! computed over `encstr[2..total-14]` and spliced in before the stop
//! sentinel. Patterns + RS construction ported from `bwipp_auspost` in
//! bwip-js v4.x (2026-03-31 vendor snapshot).

use crate::encoding::{Bar4State, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;
use crate::util::rs_gf64;

/// Bar-state strings, indexed by codeword value:
/// * `0..=63` — full 3-bar patterns for the 64-char customer-info alphabet;
/// * `64..=73` — compact 2-bar patterns for FCC and DPID digits;
/// * `74` — start sentinel `"13"` (Descender + Full);
/// * `75` — stop filler `"3"` (single Full).
///
/// In BWIPP, `encs[74]` is used for *both* the start and stop sentinel
/// (line 16624 and line 16811), while `encs[75]` is the per-cell filler
/// between customer info and the RS check section.
const ENCS: [&str; 76] = [
    "000", "001", "002", "010", "011", "012", "020", "021", "022", "100", "101", "102", "110",
    "111", "112", "120", "121", "122", "200", "201", "202", "210", "211", "212", "220", "221",
    "222", "300", "301", "302", "310", "311", "312", "320", "321", "322", "023", "030", "031",
    "032", "033", "103", "113", "123", "130", "131", "132", "133", "203", "213", "223", "230",
    "231", "232", "233", "303", "313", "323", "330", "331", "332", "333", "003", "013", "00", "01",
    "02", "10", "11", "12", "20", "21", "22", "30", "13", "3",
];

const START_END_IDX: usize = 74;
const FILLER_IDX: usize = 75;
const DIGIT_BASE: usize = 64;

/// Customer-info character mode alphabet. Each character maps to a 3-bar
/// codeword whose index is its position in this string (i.e. `'A'` →
/// `ENCS[0]`, `'B'` → `ENCS[1]`, …, `'#'` → `ENCS[63]`). Mirrors BWIPP's
/// `auspost_barchars`.
const BARCHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz #";

#[derive(Clone, Copy)]
enum CustinfoMode {
    Character,
    Numeric,
}

impl CustinfoMode {
    fn bars_per_input_char(self) -> usize {
        match self {
            CustinfoMode::Character => 3,
            CustinfoMode::Numeric => 2,
        }
    }
}

/// Total encstr length (= number of bars) for each FCC. Matches BWIPP's
/// `auspost_fcclen` map.
fn fcc_total_len(fcc: &str) -> Option<usize> {
    match fcc {
        "11" | "45" | "87" | "92" => Some(37),
        "59" => Some(52),
        "62" => Some(67),
        _ => None,
    }
}

/// Encode an Australia Post 4-state symbol with the full bwip-js layout:
/// start sentinel, FCC, DPID, customer-info, RS GF(64) check, stop.
///
/// `fcc` must be one of `"11" | "45" | "59" | "62"`. `data` is the 8-digit
/// DPID optionally followed by a customer-information suffix. The mode
/// used to encode the suffix is controlled by `opts.extras["custinfoenc"]`
/// (`"character"` (default) or `"numeric"`); any leftover capacity in the
/// custinfo region is padded with the single-cell filler `"3"`.
///
/// **Character mode**: each suffix byte must come from
/// `ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz #`
/// and consumes 3 bars. **Numeric mode**: each suffix byte must be an
/// ASCII digit and consumes 2 bars.
pub fn encode(fcc: &str, data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let total = fcc_total_len(fcc).ok_or_else(|| {
        Error::InvalidData(format!(
            "Australia Post: FCC must be one of 11, 45, 59, 62 (got {fcc:?})"
        ))
    })?;
    if data.len() < 8 || !data.as_bytes()[..8].iter().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidData(format!(
            "Australia Post: first 8 bytes of data must be the ASCII-digit DPID (got {data:?})"
        )));
    }
    let dpid = &data[..8];
    let custinfo = &data[8..];
    let mode = match opts.get("custinfoenc").unwrap_or("character") {
        "character" => CustinfoMode::Character,
        "numeric" => CustinfoMode::Numeric,
        other => {
            return Err(Error::InvalidOption(format!(
                "Australia Post: custinfoenc must be \"character\" or \"numeric\" (got {other:?})"
            )));
        }
    };
    let custinfo_region = total.saturating_sub(36); // total - (start+fcc+dpid+rs+stop) = total - 36
    let consumed = custinfo.len() * mode.bars_per_input_char();
    if consumed > custinfo_region {
        return Err(Error::InvalidData(format!(
            "Australia Post: customer info too long ({} bars needed, {custinfo_region} available)",
            consumed
        )));
    }

    // Build encstr as a fixed-length char string of '0'..='3' digits.
    let mut encstr = vec![b'0'; total];
    // [0..2]: start sentinel "13".
    splice_pattern(&mut encstr, 0, ENCS[START_END_IDX]);
    // [2..6]: FCC, 2 chars per digit.
    for (i, c) in fcc.chars().enumerate() {
        let d = c.to_digit(10).unwrap() as usize;
        splice_pattern(&mut encstr, 2 + i * 2, ENCS[DIGIT_BASE + d]);
    }
    // [6..22]: DPID, 2 chars per digit.
    for (i, c) in dpid.chars().enumerate() {
        let d = c.to_digit(10).unwrap() as usize;
        splice_pattern(&mut encstr, 6 + i * 2, ENCS[DIGIT_BASE + d]);
    }
    // [22..total-14]: customer-info region. First splice in the user's
    // custinfo per the selected mode, then pad any leftover bars with
    // the single-cell filler "3" (BWIPP's encs[75]).
    let mut pos = 22;
    match mode {
        CustinfoMode::Numeric => {
            for (idx, c) in custinfo.chars().enumerate() {
                if !c.is_ascii_digit() {
                    return Err(Error::InvalidData(format!(
                        "Australia Post (numeric mode): customer info byte {idx} {c:?} is not an ASCII digit"
                    )));
                }
                let d = c.to_digit(10).unwrap() as usize;
                splice_pattern(&mut encstr, pos, ENCS[DIGIT_BASE + d]);
                pos += 2;
            }
        }
        CustinfoMode::Character => {
            for (idx, c) in custinfo.chars().enumerate() {
                let bar_idx = BARCHARS.find(c).ok_or_else(|| {
                    Error::InvalidData(format!(
                        "Australia Post (character mode): customer info byte {idx} {c:?} is not in the barchar alphabet"
                    ))
                })?;
                splice_pattern(&mut encstr, pos, ENCS[bar_idx]);
                pos += 3;
            }
        }
    }
    while pos < total - 14 {
        splice_pattern(&mut encstr, pos, ENCS[FILLER_IDX]);
        pos += 1;
    }
    // [total-2..total]: stop sentinel "13" (same as start).
    splice_pattern(&mut encstr, total - 2, ENCS[START_END_IDX]);

    // RS over GF(64) on the data region [2..total-14], grouped into
    // 3-char (6-bit) codewords. BWIPP feeds the encoder the data in
    // reverse order; our `rs_gf64::encode_4` expects forward order and
    // reverses internally.
    let data_region = &encstr[2..total - 14];
    debug_assert_eq!(
        data_region.len() % 3,
        0,
        "data region must be a whole number of 3-char codewords"
    );
    let cw_count = data_region.len() / 3;
    let mut codewords: Vec<u8> = Vec::with_capacity(cw_count);
    for i in 0..cw_count {
        let c0 = (data_region[i * 3] - b'0') as u8;
        let c1 = (data_region[i * 3 + 1] - b'0') as u8;
        let c2 = (data_region[i * 3 + 2] - b'0') as u8;
        codewords.push(c0 * 16 + c1 * 4 + c2);
    }
    let check = rs_gf64::encode_4(&codewords);
    // Place the four check codewords at [total-14..total-2] in *reverse*
    // BWIPP order: cws[3] (k3) goes first, then k2, k1, k0.
    for (i, &k) in [check[3], check[2], check[1], check[0]].iter().enumerate() {
        let base = total - 14 + i * 3;
        let c0 = (k >> 4) & 0x3;
        let c1 = (k >> 2) & 0x3;
        let c2 = k & 0x3;
        encstr[base] = b'0' + c0;
        encstr[base + 1] = b'0' + c1;
        encstr[base + 2] = b'0' + c2;
    }

    // Convert the encstr digit chars to Postal4Pattern bars. BWIPP's
    // `bwipp_auspost` encstr-to-bar mapping is *different* from the
    // Japan Post / Postnet convention that `Bar4State::from_digit`
    // uses — for AusPost, '0' is Full, '1' is Ascender, '2' is
    // Descender, '3' is Tracker (see the `bbs`/`hunit` table around
    // bwipp_auspost.ps.src:16813). Inline the mapping here.
    let mut bars: Vec<Bar4State> = Vec::with_capacity(total);
    for &b in &encstr {
        let bar = match b {
            b'0' => Bar4State::Full,
            b'1' => Bar4State::Ascender,
            b'2' => Bar4State::Descender,
            b'3' => Bar4State::Tracker,
            other => {
                return Err(Error::InvalidData(format!(
                    "Australia Post: encstr digit out of range: {other}"
                )))
            }
        };
        bars.push(bar);
    }

    let text = if opts.include_text {
        Some(format!("{fcc}{dpid}{custinfo}"))
    } else {
        None
    };
    Ok(Postal4Pattern { bars, text })
}

/// Splice the `pat` string (each char `'0'..='3'`) into `out` at
/// `offset`. Caller is responsible for ensuring the slice is in bounds.
fn splice_pattern(out: &mut [u8], offset: usize, pat: &str) {
    for (i, c) in pat.bytes().enumerate() {
        out[offset + i] = c;
    }
}

/// Encode an Australia Post 4-state Customer (FCC 11) symbol.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 8-digit DPID — the basic Customer barcode form, no customer info.
/// let svg = render_svg(Symbology::AuspostCustomer, "12345678", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_customer(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode("11", data, opts)
}

/// Encode an Australia Post 4-state Reply Paid (FCC 45) symbol.
pub fn encode_reply_paid(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode("45", data, opts)
}

/// Encode an Australia Post 4-state Routing (FCC 59) symbol.
pub fn encode_routing(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode("59", data, opts)
}

/// Encode an Australia Post 4-state Redirection (FCC 62) symbol.
pub fn encode_redirection(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode("62", data, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a `Postal4Pattern` back to BWIPP's 0123 char string so tests
    /// can compare byte-for-byte against the captured encstr. AusPost
    /// uses the encstr convention `'0'=F`, `'1'=A`, `'2'=D`, `'3'=T`
    /// (inverse of Japan Post's; see the encode function).
    fn bars_to_encstr(p: &Postal4Pattern) -> String {
        p.bars
            .iter()
            .map(|b| match b {
                Bar4State::Full => '0',
                Bar4State::Ascender => '1',
                Bar4State::Descender => '2',
                Bar4State::Tracker => '3',
            })
            .collect()
    }

    /// Stage 11.A8c — pin `splice_pattern` writes pat-bytes at the
    /// correct offset (out[offset + i] = c for each i).
    ///
    /// Mutations caught:
    ///   * `offset + i` → `offset - i` (only writes at offset for
    ///     i=0 then OOB on i>=1) — caught by the "AB at offset 2"
    ///     check that pat[1] lands at index 3.
    ///   * `out[offset + i] = c` → `= 0` (would clear instead of
    ///     copy) — caught by reading back the expected bytes.
    ///   * `offset + i` → `i` (no offset) — pattern starts at index 0
    ///     instead of `offset`, leaving offset bytes unchanged.
    #[test]
    fn splice_pattern_writes_bytes_at_offset() {
        // Splice "AB" at offset 2 into a zeroed 7-byte buffer.
        let mut buf = [0u8; 7];
        splice_pattern(&mut buf, 2, "AB");
        assert_eq!(buf, [0, 0, b'A', b'B', 0, 0, 0]);

        // Single-char at end (offset = len-1).
        let mut buf = [0u8; 3];
        splice_pattern(&mut buf, 2, "Z");
        assert_eq!(buf, [0, 0, b'Z']);

        // Single-char at start (offset = 0).
        let mut buf = [1u8; 5];
        splice_pattern(&mut buf, 0, "X");
        assert_eq!(buf, [b'X', 1, 1, 1, 1]);

        // Empty pattern leaves buffer unchanged.
        let mut buf = [9u8; 4];
        splice_pattern(&mut buf, 1, "");
        assert_eq!(buf, [9, 9, 9, 9]);
    }

    /// Stage 11.A8c — pin every `fcc_total_len` arm + the default.
    ///
    /// Mutations caught:
    ///   * Any single FCC literal swap (e.g. `"11"` → `"12"`) flips
    ///     that key to the None default.
    ///   * Length-value swap (`Some(37)` ↔ `Some(52)` ↔ `Some(67)`)
    ///     misroutes the FCC-to-bar-count map.
    ///   * Default branch returning `Some(0)` or any literal would
    ///     fail the `None` assertion for unknown FCCs.
    #[test]
    fn fcc_total_len_per_arm_and_default() {
        // 37-bar FCCs (same arm grouped via `|`).
        assert_eq!(fcc_total_len("11"), Some(37));
        assert_eq!(fcc_total_len("45"), Some(37));
        assert_eq!(fcc_total_len("87"), Some(37));
        assert_eq!(fcc_total_len("92"), Some(37));
        // 52-bar (reply-paid) FCC.
        assert_eq!(fcc_total_len("59"), Some(52));
        // 67-bar (routing) FCC.
        assert_eq!(fcc_total_len("62"), Some(67));
        // Unknown FCCs → None (default branch).
        assert_eq!(fcc_total_len("99"), None);
        assert_eq!(fcc_total_len(""), None);
        assert_eq!(fcc_total_len("11 "), None, "trailing space breaks match");
        assert_eq!(fcc_total_len("1"), None, "single digit not in any arm");
        assert_eq!(fcc_total_len("111"), None, "3 digits not in any arm");
    }

    #[test]
    fn rejects_invalid_fcc() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the source diagnostic at line 93 of
        // auspost.rs:
        //   1. `Australia Post:` symbology prefix
        //   2. `FCC must be one of` predicate with valid list
        //   3. `"99"` Debug-echo of the offending FCC
        match encode("99", "12345678", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("FCC must be one of"),
                    "missing FCC must-be-one-of predicate: {msg}"
                );
                assert!(msg.contains("\"99\""), "missing \"99\" Debug echo: {msg}");
            }
            other => panic!("FCC=\"99\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_short_dpid() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the source diagnostic at line 98 of
        // auspost.rs:
        //   1. `Australia Post:` symbology prefix
        //   2. `first 8 bytes of data must be the ASCII-digit DPID`
        //      full predicate
        //   3. `"123"` Debug-echo of the offending short DPID
        match encode_customer("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("first 8 bytes of data must be the ASCII-digit DPID"),
                    "missing DPID predicate: {msg}"
                );
                assert!(msg.contains("\"123\""), "missing \"123\" Debug echo: {msg}");
            }
            other => panic!("short DPID should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn customer_renders_37_bars() {
        let p = encode_customer("12345678", &Options::default()).unwrap();
        assert_eq!(p.bars.len(), 37);
    }

    #[test]
    fn routing_renders_52_bars() {
        let p = encode_routing("12345678", &Options::default()).unwrap();
        assert_eq!(p.bars.len(), 52);
    }

    #[test]
    fn redirection_renders_67_bars() {
        let p = encode_redirection("12345678", &Options::default()).unwrap();
        assert_eq!(p.bars.len(), 67);
    }

    #[test]
    fn start_and_stop_are_both_ascender_tracker() {
        let p = encode_customer("12345678", &Options::default()).unwrap();
        // BWIPP `auspost_encs[74] = "13"` → Ascender, Tracker (under
        // AusPost's `'1'=A`, `'3'=T` encstr convention).
        assert_eq!(p.bars[0], Bar4State::Ascender);
        assert_eq!(p.bars[1], Bar4State::Tracker);
        // Stop sentinel (same "13").
        let n = p.bars.len();
        assert_eq!(p.bars[n - 2], Bar4State::Ascender);
        assert_eq!(p.bars[n - 1], Bar4State::Tracker);
    }

    #[test]
    fn different_fcc_produces_different_output() {
        let c = encode_customer("12345678", &Options::default()).unwrap();
        let rp = encode_reply_paid("12345678", &Options::default()).unwrap();
        assert_ne!(c.bars, rp.bars);
    }

    /// Golden encstrs from bwip-js's auspost encoder for DPID=12345678
    /// across each supported FCC. Captured via `tools/oracle-auspost.js`.
    /// Layout: start "13" + FCC (4 chars) + DPID (16 chars) + filler "3"s
    /// + 4×3-char RS check + stop "13".
    #[test]
    fn encstrs_match_bwip_js() {
        type EncFn = fn(&str, &Options) -> Result<Postal4Pattern, Error>;
        let cases: &[(EncFn, &str)] = &[
            (encode_customer, "1301010102101112202122312223010303313"),
            (encode_reply_paid, "1311120102101112202122331321222302113"),
            (
                encode_routing,
                "1312300102101112202122333333333333333332102021303213",
            ),
            (
                encode_redirection,
                "1320020102101112202122333333333333333333333333333333320020230030013",
            ),
        ];
        for &(f, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // input echo. The cases iterate over multiple fn pointers
            // (different AusPost variants); each must encode "12345678"
            // successfully and produce a unique encstr.
            let p = f("12345678", &Options::default()).unwrap_or_else(|e| {
                panic!("AusPost-variant encode(\"12345678\") (corpus item with expected encstr len {}) must succeed; got Err: {e}", want.len())
            });
            let got = bars_to_encstr(&p);
            assert_eq!(got, want, "encstr mismatch");
            assert_eq!(got.len(), want.len(), "length mismatch");
        }
    }

    /// Customer-info in **character** mode: each input char maps to a
    /// 3-bar codeword whose index is its position in `BARCHARS`.
    /// Goldens captured via `oracle-auspost.js <text> character`.
    #[test]
    fn custinfo_character_mode_matches_bwip_js() {
        let opts = Options::default();
        let want_routing = "1312300102101112202122000001002010011322120130021113";
        let got = bars_to_encstr(&encode_routing("12345678ABCDE", &opts).unwrap());
        assert_eq!(got, want_routing, "routing+ABCDE character mode");

        let want_redir = "1320020102101112202122000001002010011012020021022100330233210221013";
        let got = bars_to_encstr(&encode_redirection("12345678ABCDEFGHIJ", &opts).unwrap());
        assert_eq!(got, want_redir, "redirection+ABCDEFGHIJ character mode");
    }

    /// Customer-info in **numeric** mode: each input digit becomes a
    /// 2-bar codeword (the same pattern used for DPID digits).
    #[test]
    fn custinfo_numeric_mode_matches_bwip_js() {
        let opts = Options::default().with("custinfoenc", "numeric");
        let want_routing = "1312300102101112202122010210113333333301330233211313";
        let got = bars_to_encstr(&encode_routing("123456781234", &opts).unwrap());
        assert_eq!(got, want_routing, "routing+1234 numeric mode");

        let want_redir = "1320020102101112202122010210111233333333333333333333312310202120313";
        let got = bars_to_encstr(&encode_redirection("1234567812345", &opts).unwrap());
        assert_eq!(got, want_redir, "redirection+12345 numeric mode");
    }

    /// Custom info that overflows the available region must be rejected.
    #[test]
    fn rejects_overlong_custinfo() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin per
        // arm matching the source diagnostic at line 115-118 of
        // auspost.rs:
        //   1. `Australia Post:` symbology prefix
        //   2. `customer info too long` predicate
        //   3. specific `<consumed> bars needed, <region> available`
        //      echo — kills arithmetic mutations on `consumed =
        //      custinfo.len() * mode.bars_per_input_char()` (e.g. `*`
        //      → `+`) and on `custinfo_region = total.saturating_sub(36)`
        //      (e.g. `36` → `35`, or `sub` → `add`).
        // The discriminant-only check would survive any mutation that
        // still routes to InvalidData (e.g. a different validator
        // earlier in the pipeline).

        // FCC 11 has zero custinfo capacity (37 - 36 = 1 bar, which
        // can't even hold a single character-mode codeword).
        // "A" → character mode → 1 char × 3 bars/char = 3 bars > 1 available.
        match encode_customer("12345678A", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "FCC 11 arm: missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("customer info too long"),
                    "FCC 11 arm: missing customer-info-too-long predicate: {msg}"
                );
                assert!(
                    msg.contains("3 bars needed, 1 available"),
                    "FCC 11 arm: missing `3 bars needed, 1 available` echo: {msg}"
                );
            }
            other => panic!("FCC 11 + custinfo \"A\" should reject as InvalidData, got {other:?}"),
        }

        // FCC 59 holds 5 chars at most in character mode (16 / 3).
        // "ABCDEF" → character mode → 6 chars × 3 = 18 bars > 16 available.
        match encode_routing("12345678ABCDEF", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "FCC 59 arm: missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("customer info too long"),
                    "FCC 59 arm: missing customer-info-too-long predicate: {msg}"
                );
                assert!(
                    msg.contains("18 bars needed, 16 available"),
                    "FCC 59 arm: missing `18 bars needed, 16 available` echo: {msg}"
                );
            }
            other => {
                panic!("FCC 59 + custinfo \"ABCDEF\" should reject as InvalidData, got {other:?}")
            }
        }
    }

    /// Bad custinfoenc value is rejected as an InvalidOption.
    #[test]
    fn rejects_unknown_custinfoenc() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidOption(_)))` to 3-anchor pin
        // matching the source diagnostic at line 107-109 of auspost.rs:
        //   1. `Australia Post:` symbology prefix
        //   2. `custinfoenc must be "character" or "numeric"` predicate
        //   3. `"binary"` Debug echo of the offending option value
        // The discriminant-only check would survive a mutation that
        // routes to a different InvalidOption diagnostic, or that
        // accepts "binary" silently and fails downstream.
        let opts = Options::default().with("custinfoenc", "binary");
        match encode_routing("12345678AB", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("custinfoenc must be \"character\" or \"numeric\""),
                    "missing custinfoenc-must-be predicate: {msg}"
                );
                assert!(
                    msg.contains("\"binary\""),
                    "missing \"binary\" Debug echo: {msg}"
                );
            }
            other => panic!("custinfoenc=\"binary\" should reject as InvalidOption, got {other:?}"),
        }
    }

    fn fadt_string(p: &Postal4Pattern) -> String {
        p.bars
            .iter()
            .map(|b| match b {
                Bar4State::Full => 'F',
                Bar4State::Ascender => 'A',
                Bar4State::Descender => 'D',
                Bar4State::Tracker => 'T',
            })
            .collect()
    }

    /// End-to-end bar-shape cross-validation against `b.raw("auspost",
    /// "<fcc><payload>", {})[0]` by classifying each bar's `(bhs, bbs)`
    /// pair into F/A/D/T. Confirms the encstr→Bar4State mapping fix:
    /// without it, every bar was inverted (Descender↔Ascender,
    /// Full↔Tracker) versus the rendered bwip-js symbol even though
    /// the digit-encstr string matched byte-for-byte.
    #[test]
    fn bar_shapes_match_bwip_js() {
        type EncFn = fn(&str, &Options) -> Result<Postal4Pattern, Error>;
        let opts = Options::default();
        let cases: &[(EncFn, &str, &str)] = &[
            (
                encode_customer,
                "12345678",
                "ATFAFAFAFDAFAAADDFDADDTADDDTFAFTFTTAT",
            ),
            (
                encode_reply_paid,
                "12345678",
                "ATAAADFAFDAFAAADDFDADDTTATDADDDTFDAAT",
            ),
            (
                encode_routing,
                "12345678",
                "ATADTFFAFDAFAAADDFDADDTTTTTTTTTTTTTTTTTDAFDFDATFTDAT",
            ),
            (
                encode_redirection,
                "12345678",
                "ATDFFDFAFDAFAAADDFDADDTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTDFFDFDTFFTFFAT",
            ),
        ];
        for &(f, text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo + 4-state bar-shape rationale.
            let p = f(text, &opts).unwrap_or_else(|e| {
                panic!("AusPost-variant encode({text:?}) (corpus item with expected FADT 4-state bar shape of len {}) must succeed; got Err: {e}", want.len())
            });
            assert_eq!(
                fadt_string(&p),
                want,
                "auspost bar shape mismatch for {text:?}"
            );
        }
    }

    /// Kills the `> with >=` mutant at line ~114 (the
    /// `consumed > custinfo_region` overflow guard). The original
    /// rejects only when consumed is *strictly* greater; the mutant
    /// also rejects when they are equal — i.e. a payload that
    /// precisely saturates the custinfo region must succeed.
    ///
    /// For FCC=59 (routing, total=52, custinfo_region = 52-36 = 16),
    /// numeric mode consumes 2 bars per digit, so 8 digits of
    /// custinfo exactly saturate the region (8·2 = 16 bars). The
    /// boundary test pins both directions: 8-digit numeric custinfo
    /// must encode; 9 digits must error.
    ///
    /// The companion `< with <=` mutant at line ~164 (the filler
    /// loop bound) is observationally equivalent under the current
    /// pipeline: the extra filler byte the mutant writes lands at
    /// position `total-14`, which is then unconditionally
    /// overwritten by the first RS check codeword (the loop variable
    /// `pos` doesn't escape its scope, and the slice `&encstr[2..
    /// total-14]` used for the RS data region excludes the
    /// overwritten byte). The `+= with *=` mutant at line ~166 is
    /// a true infinite-loop and is correctly classified as TIMEOUT
    /// by cargo-mutants — not a missed mutant.
    #[test]
    fn custinfo_numeric_mode_saturates_region_at_exactly_eight_digits() {
        // Stage 11.A8c (cont) — upgrade the 9-digit reject arm from
        // discriminant-only `matches!(_, Err(Error::InvalidData(_)))`
        // to 3-anchor pin matching the source diagnostic at line
        // 115-118 of auspost.rs:
        //   1. `Australia Post:` symbology prefix
        //   2. `customer info too long` predicate
        //   3. `18 bars needed, 16 available` echo — pins both the
        //      `consumed = custinfo.len() * mode.bars_per_input_char()`
        //      arithmetic (9 × 2 = 18) and the `custinfo_region =
        //      total.saturating_sub(36)` arithmetic (52 - 36 = 16) for
        //      the numeric-mode + FCC-59 path specifically.
        // The 8-digit `.expect()` already pins the boundary; this
        // upgrade strengthens the 9-digit arm so a mutation to the
        // format string or to either arithmetic operand fails the
        // anchor instead of silently passing the discriminant check.
        let opts = Options::default().with("custinfoenc", "numeric");
        // 8 digits × 2 bars = 16 bars = entire custinfo region.
        encode_routing("1234567812345678", &opts)
            .expect("8-digit numeric custinfo should fit exactly (16 bars = region size)");
        // 9 digits = 18 bars > 16 → must error.
        match encode_routing("12345678123456789", &opts) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post:"),
                    "missing Australia Post prefix: {msg}"
                );
                assert!(
                    msg.contains("customer info too long"),
                    "missing customer-info-too-long predicate: {msg}"
                );
                assert!(
                    msg.contains("18 bars needed, 16 available"),
                    "missing `18 bars needed, 16 available` echo: {msg}"
                );
            }
            other => panic!("9-digit numeric custinfo should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin every `fcc_total_len` match arm directly.
    /// The encoder's public surface only exposes FCC 11 / 45 / 59 / 62
    /// via the dedicated wrappers, so the "87" and "92" arms at line
    /// ~70 are unreachable through the wrapper APIs and the
    /// `delete arm "87"` / `delete arm "92"` / `delete arm "59"` /
    /// `delete arm "62"` mutants would survive the encoder tests.
    /// Direct unit tests on the helper anchor all four arms.
    ///
    /// "11" / "45" / "87" / "92" share length 37 (`auspost_fcclen[*]`);
    /// "59" → 52; "62" → 67; everything else → None.
    #[test]
    fn fcc_total_len_all_arms() {
        assert_eq!(fcc_total_len("11"), Some(37), "FCC 11 → 37");
        assert_eq!(fcc_total_len("45"), Some(37), "FCC 45 → 37");
        assert_eq!(fcc_total_len("87"), Some(37), "FCC 87 → 37");
        assert_eq!(fcc_total_len("92"), Some(37), "FCC 92 → 37");
        assert_eq!(fcc_total_len("59"), Some(52), "FCC 59 → 52");
        assert_eq!(fcc_total_len("62"), Some(67), "FCC 62 → 67");
        // Invalid FCCs → None. Pin a representative sample around the
        // arms to catch off-by-one match-arm shifts (e.g. `"58"`,
        // `"60"`, `"63"`, neighbours of 59 and 62).
        assert_eq!(fcc_total_len("00"), None);
        assert_eq!(fcc_total_len("10"), None);
        assert_eq!(fcc_total_len("46"), None);
        assert_eq!(fcc_total_len("58"), None);
        assert_eq!(fcc_total_len("60"), None);
        assert_eq!(fcc_total_len("63"), None);
        assert_eq!(fcc_total_len("99"), None);
        assert_eq!(fcc_total_len(""), None);
        assert_eq!(fcc_total_len("11A"), None, "trailing junk");
    }

    /// Numeric mode must reject non-digit custinfo characters.
    #[test]
    fn numeric_mode_rejects_non_digits() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 143-145 of auspost.rs:
        //   1. `Australia Post (numeric mode):` parenthetical mode-
        //      qualified symbology prefix (distinct from the
        //      bare `Australia Post:` prefix used by FCC / DPID /
        //      length-overflow paths — pinning this discriminates the
        //      numeric-mode-only reject)
        //   2. `is not an ASCII digit` predicate
        //   3. `byte 0 'A'` index+char Debug echo — pins both the
        //      `idx` counter and the `c:?` Debug-echo of the offending
        //      byte. Specifically the 'A' is at idx=0 because custinfo
        //      starts at data[8..] (8-digit DPID).
        // The discriminant-only check would survive a mutation that
        // changes the format string or that re-routes the error to a
        // different diagnostic path.
        let opts = Options::default().with("custinfoenc", "numeric");
        match encode_routing("12345678AB", &opts) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post (numeric mode):"),
                    "missing `Australia Post (numeric mode):` parenthetical prefix: {msg}"
                );
                assert!(
                    msg.contains("is not an ASCII digit"),
                    "missing `is not an ASCII digit` predicate: {msg}"
                );
                assert!(
                    msg.contains("byte 0 'A'"),
                    "missing `byte 0 'A'` index+char Debug echo: {msg}"
                );
            }
            other => {
                panic!("numeric-mode custinfo \"AB\" should reject as InvalidData, got {other:?}")
            }
        }
    }

    /// Stage 11.A8c — character mode rejects custinfo bytes outside
    /// the BARCHARS alphabet (line 152-158 of `encode`). The numeric-
    /// mode rejection arm has a test; the character-mode arm doesn't
    /// — a mutant that drops the `.ok_or_else` rejection would let
    /// invalid bytes panic at the `splice_pattern` index, or worse,
    /// silently encode garbage.
    ///
    /// BARCHARS is `A-Z 0-9 a-z space and #` per the BWIPP alphabet.
    /// '@', '!', '~', '$' are NOT in the alphabet.
    ///
    /// Anchors:
    ///   - encode_routing("12345678@") → InvalidData (character mode
    ///     is the default — no custinfoenc set).
    ///   - encode_redirection("12345678~") → InvalidData.
    ///   - Diagnostic pins "character mode" + offending char + index.
    ///   - Sanity: alphabet chars (A, Z, a, z, 0, 9, ' ', '#') all
    ///     succeed (the lookup must not over-reject).
    #[test]
    fn character_mode_rejects_invalid_barchars_with_diagnostic() {
        // '@' is not in BARCHARS.
        //
        // Stage 11.A8c (cont) — bring this arm to anchor parity with
        // the '~' sibling at lines 654-666 by widening the first
        // anchor from `character mode` (which only matches the
        // parenthetical) to the full `Australia Post (character
        // mode):` symbology prefix. Kills mutations that drop
        // `Australia Post` from the format string at line 156 of
        // auspost.rs.
        match encode_routing("12345678@", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("Australia Post (character mode):")
                    && msg.contains("not in the barchar alphabet")
                    && msg.contains("'@'"),
                "expected character-mode rejection diagnostic, got: {msg}"
            ),
            other => panic!("'@' should reject in character mode, got {other:?}"),
        }

        // '~' is also outside the alphabet. Defense-in-depth with the
        // '@' anchor above — same line-156 diagnostic format
        // ("Australia Post (character mode): customer info byte {idx}
        // {c:?} is not in the barchar alphabet"); pin all 3 substrings
        // so the format-string survives mutations in BOTH tests.
        match encode_redirection("12345678~", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Australia Post (character mode):"),
                    "'~' rejection must carry the Australia Post (character mode) prefix; got {msg}"
                );
                assert!(
                    msg.contains("not in the barchar alphabet"),
                    "'~' rejection must carry the barchar-alphabet predicate; got {msg}"
                );
                assert!(
                    msg.contains("'~'"),
                    "'~' rejection must Debug-echo the offending char; got {msg}"
                );
            }
            other => panic!("'~' should reject in character mode, got {other:?}"),
        }

        // Sanity: every BARCHARS byte succeeds (alphabet membership
        // must be inclusive). Pick one of each class.
        for c in ['A', 'Z', 'a', 'z', '0', '9', ' ', '#'] {
            let payload: String = format!("12345678{c}");
            assert!(
                encode_routing(&payload, &Options::default()).is_ok(),
                "BARCHARS member {c:?} should succeed"
            );
        }
    }

    /// Stage 11.A8c — pin diagnostic substrings for the three weak
    /// `matches!(_, Err(_))` rejection tests in auspost:
    ///   * `rejects_invalid_fcc` — pins "FCC must be one of" diagnostic.
    ///   * `rejects_short_dpid` — pins "first 8 bytes" diagnostic.
    ///   * `rejects_overlong_custinfo` — pins "too long" diagnostic.
    ///   * `rejects_unknown_custinfoenc` — pins "custinfoenc must be"
    ///     diagnostic.
    ///
    /// Without these, mutations to the error message strings slip
    /// through unnoticed.
    #[test]
    fn auspost_rejection_diagnostic_substrings() {
        // Invalid FCC "99".
        match encode("99", "12345678", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("FCC must be one of")
                    && msg.contains("11")
                    && msg.contains("45")
                    && msg.contains("\"99\""),
                "FCC=99 should pin diagnostic, got: {msg}"
            ),
            other => panic!("FCC=99 should reject, got {other:?}"),
        }

        // Short DPID "123" (3 chars, need 8).
        match encode_customer("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("first 8 bytes") && msg.contains("ASCII-digit DPID"),
                "short DPID should pin diagnostic, got: {msg}"
            ),
            other => panic!("short DPID should reject, got {other:?}"),
        }

        // DPID with non-digit char at position < 8.
        match encode_customer("1234567A", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("first 8 bytes") && msg.contains("ASCII-digit DPID"),
                "non-digit DPID should pin diagnostic, got: {msg}"
            ),
            other => panic!("non-digit DPID should reject, got {other:?}"),
        }

        // Overlong custinfo. encode_customer has FCC=11 (total=37),
        // custinfo_region = 37 - 36 = 1. Character mode consumes 3
        // bars per char. So even 1 character of custinfo (3 bars) is
        // already > 1 bar region.
        //
        // Stage 11.A8c (cont) — add `Australia Post:` symbology prefix
        // anchor (matches the source diagnostic at line 116 of
        // auspost.rs and brings parity with the other arms in this
        // test that already require the `Australia Post (character
        // mode):` prefix).
        match encode_customer("12345678ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("Australia Post:") && msg.contains("too long") && msg.contains("bars"),
                "overlong custinfo should pin 'too long' diagnostic, got: {msg}"
            ),
            other => panic!("overlong custinfo should reject, got {other:?}"),
        }

        // Unknown custinfoenc.
        //
        // Stage 11.A8c (cont) — add `Australia Post:` symbology
        // prefix anchor (matches source diagnostic at line 108 of
        // auspost.rs).
        let opts = Options::default().with("custinfoenc", "magical");
        match encode_customer("12345678", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("Australia Post:")
                    && msg.contains("custinfoenc must be")
                    && msg.contains("character")
                    && msg.contains("numeric"),
                "custinfoenc=magical should pin diagnostic, got: {msg}"
            ),
            other => panic!("custinfoenc=magical should reject, got {other:?}"),
        }
    }
}
