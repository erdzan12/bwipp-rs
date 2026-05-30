//! EAN-13, EAN-8, UPC-A, UPC-E.
//!
//! All four are variants of the GS1 retail family. Each digit is encoded as
//! a 7-module pattern from one of three sets: L (left-odd), G (left-even),
//! or R (right). The leading digit of EAN-13 selects which mix of L and G
//! patterns is used for the next six digits. Symbols are bracketed by a
//! 3-module guard at each end, with a 5-module center guard between left
//! and right halves.
//!
//! UPC-E is **not** a simple compression of UPC-A: it has its own bar
//! layout with a special 6-module end guard and a parity pattern selected
//! from the number-system + check-digit combination.
//!
//! Patterns and parity tables verified against the BWIPP `ean13`, `ean8`,
//! `upca`, and `upce` encoders (and `bwipp_upce` in bwip-js v4.x).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

const L_PATTERNS: [&str; 10] = [
    "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
    "0110111", "0001011",
];
const G_PATTERNS: [&str; 10] = [
    "0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001",
    "0001001", "0010111",
];
const R_PATTERNS: [&str; 10] = [
    "1110010", "1100110", "1101100", "1000010", "1011100", "1001110", "1010000", "1000100",
    "1001000", "1110100",
];

/// Parity pattern for the six left digits of EAN-13, indexed by the leading
/// digit. `L` = use L_PATTERNS, `G` = use G_PATTERNS.
const EAN13_PARITY: [&str; 10] = [
    "LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL",
    "LGGLGL",
];

/// Parity pattern for the six data digits of UPC-E, indexed by the check
/// digit when the number-system digit is 0. For NS=1, every `O` becomes
/// `E` and vice versa.
const UPCE_PARITY_NS0: [&str; 10] = [
    "EEEOOO", "EEOEOO", "EEOOEO", "EEOOOE", "EOEEOO", "EOOEEO", "EOOOEE", "EOEOEO", "EOEOOE",
    "EOOEOE",
];

const NORMAL_GUARD: &str = "101";
const CENTER_GUARD: &str = "01010";
/// UPC-E end guard: `0 1 0 1 0 1` (the bars/spaces alternate, starting with
/// space). Six modules, narrower than EAN/UPC-A's stop guard.
const UPCE_END_GUARD: &str = "010101";

/// Compute the GS1 mod-10 check digit for a string of data digits (the
/// rightmost digit is weighted 3, alternating with 1).
fn gs1_check(digits: &str) -> char {
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let n = c.to_digit(10).unwrap();
        sum += if i % 2 == 0 { n * 3 } else { n };
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

/// Strip non-digits from `data`. Returns the digit-only string.
fn digits_only(data: &str) -> String {
    data.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Accept either `exact` digits (with the user-supplied check digit, which
/// we validate) or `exact - 1` digits (we compute the check digit).
fn normalize(data: &str, exact: usize, family: &'static str) -> Result<String, Error> {
    let digits = digits_only(data);
    match digits.len() {
        n if n == exact - 1 => Ok(format!("{digits}{}", gs1_check(&digits))),
        n if n == exact => {
            let body = &digits[..exact - 1];
            let supplied = digits.chars().last().unwrap();
            let computed = gs1_check(body);
            if supplied != computed {
                return Err(Error::InvalidData(format!(
                    "{family}: supplied check digit {supplied} does not match computed {computed}"
                )));
            }
            Ok(digits)
        }
        n => Err(Error::InvalidData(format!(
            "{family}: expected {} or {} digits, got {n}",
            exact - 1,
            exact
        ))),
    }
}

fn digit(c: char) -> usize {
    c.to_digit(10).expect("validated digit") as usize
}

/// Encode an EAN-13 payload (12 or 13 digits; check is computed if missing).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 12 digits: encoder computes the check digit.
/// let svg = render_svg(Symbology::Ean13, "590123412345", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_ean13(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits = normalize(data, 13, "EAN-13")?;
    let leading = digit(digits.chars().next().unwrap());
    let parity = EAN13_PARITY[leading];

    let mut modules = String::new();
    modules.push_str(NORMAL_GUARD);
    for (c, parity_kind) in digits.chars().skip(1).take(6).zip(parity.chars()) {
        let table = match parity_kind {
            'L' => &L_PATTERNS,
            'G' => &G_PATTERNS,
            _ => unreachable!(),
        };
        modules.push_str(table[digit(c)]);
    }
    modules.push_str(CENTER_GUARD);
    for c in digits.chars().skip(7) {
        modules.push_str(R_PATTERNS[digit(c)]);
    }
    modules.push_str(NORMAL_GUARD);

    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
}

/// Encode an EAN-8 payload (7 or 8 digits).
/// Encode a Marks & Spencer 7-digit code. BWIPP `mands`.
///
/// M&S is a UK retailer-specific EAN-8 variant: input is 7 or 8 chars
/// (we prepend a leading `0` to 7-char input to make 8 chars), then the
/// symbol is encoded as EAN-8. BWIPP additionally tweaks the trailing
/// two bars' visual height to match bar 2 (a cosmetic shortening that
/// makes the rightmost two bars look like check-digit guard bars) and
/// adds 'M'/'S' annotations under the leading guard when `includetext`
/// is on. Our `LinearPattern` model carries only bar widths (a uniform
/// height across all bars), so the per-bar height tweak is **not**
/// preserved — the encoded bar pattern (sbs) is byte-identical to
/// BWIPP's M&S output, only the cosmetic bar-tail height differs.
/// Scanners read M&S as a standard EAN-8.
pub fn encode_mands(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 7 && digits.len() != 8 {
        return Err(Error::InvalidData(format!(
            "M&S: expected 7 or 8 digits, got {}",
            digits.len()
        )));
    }
    let padded = if digits.len() == 7 {
        format!("0{digits}")
    } else {
        digits
    };
    encode_ean8(&padded, opts)
}

pub fn encode_ean8(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits = normalize(data, 8, "EAN-8")?;
    let mut modules = String::new();
    modules.push_str(NORMAL_GUARD);
    for c in digits.chars().take(4) {
        modules.push_str(L_PATTERNS[digit(c)]);
    }
    modules.push_str(CENTER_GUARD);
    for c in digits.chars().skip(4) {
        modules.push_str(R_PATTERNS[digit(c)]);
    }
    modules.push_str(NORMAL_GUARD);
    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
}

/// Encode a UPC-A payload (11 or 12 digits). UPC-A is structurally an
/// EAN-13 with leading `0`; we prefix the input and delegate.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::UpcA, "01234567890", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_upca(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits = normalize(data, 12, "UPC-A")?;
    let padded = format!("0{digits}");
    encode_ean13(&padded, opts)
}

/// Encode a UPC-E payload (7 or 8 digits, including the user-supplied or
/// computed mod-10 check digit derived from the **expanded** UPC-A form).
///
/// The renderer uses UPC-E's native 6-data-digit + special end-guard layout,
/// not a UPC-A expansion. The parity of the six data digits is selected
/// from a lookup table keyed by `(number_system, check_digit)`.
pub fn encode_upce(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits = digits_only(data);
    let body = match digits.len() {
        7 => {
            // User omitted the check digit. Compute it via the UPC-A expansion.
            let upca = upce_to_upca(&digits)?;
            let check = gs1_check(&upca);
            format!("{digits}{check}")
        }
        8 => {
            // User supplied the check digit; validate via UPC-A expansion.
            let upca = upce_to_upca(&digits[..7])?;
            let computed = gs1_check(&upca);
            let supplied = digits.chars().last().unwrap();
            if supplied != computed {
                return Err(Error::InvalidData(format!(
                    "UPC-E: supplied check digit {supplied} does not match computed {computed} (via UPC-A expansion {upca}{computed})"
                )));
            }
            digits.clone()
        }
        n => {
            return Err(Error::InvalidData(format!(
                "UPC-E: expected 7 or 8 digits, got {n}"
            )))
        }
    };

    let bytes = body.as_bytes();
    let ns = bytes[0];
    if ns != b'0' && ns != b'1' {
        return Err(Error::InvalidData(
            "UPC-E: number system digit must be 0 or 1".into(),
        ));
    }
    let check = (bytes[7] - b'0') as usize;
    let parity = parity_for_upce(ns, check);

    let mut modules = String::new();
    modules.push_str(NORMAL_GUARD);
    for (c, parity_kind) in body[1..7].chars().zip(parity.chars()) {
        let table = match parity_kind {
            'O' => &L_PATTERNS,
            'E' => &G_PATTERNS,
            _ => unreachable!(),
        };
        modules.push_str(table[digit(c)]);
    }
    modules.push_str(UPCE_END_GUARD);

    let text = if opts.include_text { Some(body) } else { None };
    Ok(LinearPattern::from_modules(&modules, text))
}

/// Return the 6-character `O`/`E` parity pattern for a UPC-E body.
fn parity_for_upce(number_system: u8, check_digit: usize) -> String {
    let base = UPCE_PARITY_NS0[check_digit];
    if number_system == b'0' {
        base.to_string()
    } else {
        base.chars()
            .map(|c| match c {
                'O' => 'E',
                'E' => 'O',
                other => other,
            })
            .collect()
    }
}

/// Expand a 7-digit UPC-E body to its 11-digit UPC-A body (no check digit).
fn upce_to_upca(upce7: &str) -> Result<String, Error> {
    if upce7.len() != 7 {
        return Err(Error::InvalidData(
            "UPC-E expansion requires a 7-digit body".into(),
        ));
    }
    let bytes = upce7.as_bytes();
    if !bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidData("UPC-E: non-digit in body".into()));
    }
    let ns = bytes[0];
    if ns != b'0' && ns != b'1' {
        return Err(Error::InvalidData(
            "UPC-E: number system digit must be 0 or 1".into(),
        ));
    }
    let last = bytes[6];
    let mfr = &bytes[1..6];
    Ok(match last {
        b'0' | b'1' | b'2' => format!(
            "{}{}{}{}0000{}{}{}",
            ns as char,
            mfr[0] as char,
            mfr[1] as char,
            last as char,
            mfr[2] as char,
            mfr[3] as char,
            mfr[4] as char,
        ),
        b'3' => format!(
            "{}{}{}{}00000{}{}",
            ns as char,
            mfr[0] as char,
            mfr[1] as char,
            mfr[2] as char,
            mfr[3] as char,
            mfr[4] as char,
        ),
        b'4' => format!(
            "{}{}{}{}{}00000{}",
            ns as char,
            mfr[0] as char,
            mfr[1] as char,
            mfr[2] as char,
            mfr[3] as char,
            mfr[4] as char,
        ),
        b'5'..=b'9' => format!(
            "{}{}{}{}{}{}0000{}",
            ns as char,
            mfr[0] as char,
            mfr[1] as char,
            mfr[2] as char,
            mfr[3] as char,
            mfr[4] as char,
            last as char,
        ),
        _ => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_string(p: &LinearPattern) -> String {
        let mut s = String::new();
        let mut bar = true;
        for &w in &p.bars {
            let m = if bar { '1' } else { '0' };
            for _ in 0..w {
                s.push(m);
            }
            bar = !bar;
        }
        s
    }

    #[test]
    fn ean13_validates_supplied_check_digit() {
        // 0123456789012: the correct check for the first 12 digits is...
        let correct = gs1_check("012345678901");
        let valid = format!("01234567890{}{}", '1', correct); // 12 digits + check
                                                              // Construct an explicitly-wrong-check variant:
        let wrong_check = if correct == '0' { '1' } else { '0' };
        let bad = format!("012345678901{wrong_check}");
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic from `normalize`
        // at line 82-84 (`EAN-13: supplied check digit X does not
        // match computed Y`). Cross-family guard against UPC-A/UPC-E.
        match encode_ean13(&bad, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-13:"),
                    "missing `EAN-13:` family prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit"),
                    "missing `supplied check digit` predicate: {msg}"
                );
                assert!(
                    msg.contains("does not match computed"),
                    "missing `does not match computed` predicate: {msg}"
                );
                assert!(
                    !msg.contains("UPC-A") && !msg.contains("UPC-E") && !msg.contains("M&S"),
                    "cross-family contamination: EAN-13 reject mentions sibling family: {msg}"
                );
            }
            other => panic!("bad-check EAN-13 should reject as InvalidData, got {other:?}"),
        }
        let good = format!("012345678901{correct}");
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-13 happy-path accept-after-validation: same 12-digit
        // payload with the *computed* check digit must succeed.
        encode_ean13(&good, &Options::default()).expect(
            "encode_ean13(<12-digit + computed check>) (EAN-13 accept-after-validation smoke; mirror-check that the supplied-check rejection is value-sensitive, not blanket) must succeed",
        );
        let _ = valid; // silence
    }

    #[test]
    fn ean13_computes_check_digit_when_missing() {
        // 12 digits → check appended. We build the expected module string by
        // computing the check digit on the fly, so the test stays a real
        // pattern check and not a hard-coded literal.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-13 12-digit auto-check path: 12-digit input must
        // trigger automatic check-digit computation/append.
        let p = encode_ean13("012345678901", &Options::default()).expect(
            "encode_ean13(\"012345678901\", default) (EAN-13 12-digit auto-check path: check digit must be computed + appended) must succeed",
        );
        let check = digit(gs1_check("012345678901"));
        let expected = format!(
            "{NORMAL_GUARD}{}{}{}{}{}{}{CENTER_GUARD}{}{}{}{}{}{}{NORMAL_GUARD}",
            L_PATTERNS[1],
            L_PATTERNS[2],
            L_PATTERNS[3],
            L_PATTERNS[4],
            L_PATTERNS[5],
            L_PATTERNS[6],
            R_PATTERNS[7],
            R_PATTERNS[8],
            R_PATTERNS[9],
            R_PATTERNS[0],
            R_PATTERNS[1],
            R_PATTERNS[check],
        );
        assert_eq!(module_string(&p), expected);
    }

    #[test]
    fn upca_validates_supplied_check_digit() {
        let bad = "012345678901"; // 12 digits, last digit is the check
                                  // 012345678901's check for first 11 = ?  Let's compute:
        let computed = gs1_check("01234567890");
        let supplied = '1';
        if supplied == computed {
            // Unlikely; in this case construct a different invalid example.
            return;
        }
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the `UPC-A:` family diagnostic.
        match encode_upca(bad, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("UPC-A:"),
                    "missing `UPC-A:` family prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit"),
                    "missing `supplied check digit` predicate: {msg}"
                );
                assert!(
                    msg.contains("does not match computed"),
                    "missing `does not match computed` predicate: {msg}"
                );
                assert!(
                    !msg.contains("EAN-13") && !msg.contains("UPC-E") && !msg.contains("M&S"),
                    "cross-family contamination: UPC-A reject mentions sibling family: {msg}"
                );
            }
            other => panic!("bad-check UPC-A should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ean8_computes_and_renders_known_pattern() {
        // 1234567 → check = 0 → 12345670
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-8 7-digit auto-check pattern-cross-check path: the
        // module string is built from L/R patterns + computed check
        // = 0, then asserted equal.
        let p = encode_ean8("1234567", &Options::default()).expect(
            "encode_ean8(\"1234567\", default) (EAN-8 7-digit auto-check; computed check=0; module-string pattern cross-check) must succeed",
        );
        let expected = format!(
            "{NORMAL_GUARD}{}{}{}{}{CENTER_GUARD}{}{}{}{}{NORMAL_GUARD}",
            L_PATTERNS[1],
            L_PATTERNS[2],
            L_PATTERNS[3],
            L_PATTERNS[4],
            R_PATTERNS[5],
            R_PATTERNS[6],
            R_PATTERNS[7],
            R_PATTERNS[0],
        );
        assert_eq!(module_string(&p), expected);
    }

    #[test]
    fn upce_native_rendering_smoke() {
        // 01234565 is the standard UPC-E test vector. Expansion is 010000005678+0 ?
        // We test that:
        //   1. it does not error
        //   2. the symbol uses the UPC-E end guard (6 modules, "010101"),
        //      not the UPC-A 3-module guard.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the UPC-E native rendering path: 8-digit "01234565" must
        // produce the UPC-E 6-module end guard (not the UPC-A 3-module).
        let p = encode_upce("01234565", &Options::default()).expect(
            "encode_upce(\"01234565\", default) (UPC-E native rendering smoke; UPC-E 6-module end guard distinguishes from UPC-A 3-module guard) must succeed",
        );
        let s = module_string(&p);
        // The symbol should end with the UPC-E end guard.
        assert!(
            s.ends_with(UPCE_END_GUARD),
            "got tail: {}",
            &s[s.len() - 12..]
        );
        // Total width: normal guard (3) + 6 data * 7 + end guard (6) = 51 modules.
        assert_eq!(s.len(), 3 + 6 * 7 + 6);
    }

    #[test]
    fn upce_parity_pattern_ns0_check0_is_eeeooo() {
        let p = parity_for_upce(b'0', 0);
        assert_eq!(p, "EEEOOO");
    }

    #[test]
    fn upce_parity_pattern_ns1_check0_is_oooeee() {
        let p = parity_for_upce(b'1', 0);
        assert_eq!(p, "OOOEEE");
    }

    /// Stage 11.A8c — extend `parity_for_upce` coverage beyond
    /// check=0. Existing tests pin check=0 for NS=0 (returns "EEEOOO")
    /// and NS=1 (returns "OOOEEE"). The check=0 row is palindromic
    /// under O↔E inversion (3 E's then 3 O's), so an `'O' => 'E'`
    /// arm-deletion mutant that only flips one direction would still
    /// produce "OOOEEE" from "EEEOOO" — both arms collapse to the
    /// same swap. Inputs where the O/E count is uneven distinguish
    /// the arms.
    ///
    /// Hand-computed (UPCE_PARITY_NS0[check]):
    ///   - check=1 → "EEOEOO" (4 E, 2 O). NS=1 → "OOEOEE" (2 E, 4 O).
    ///   - check=5 → "EOOEEO" (3 E, 3 O — symmetric, but ordering
    ///     differs: NS=1 must produce "OEEOOE", not "EEOOEE").
    ///   - check=9 → "EOOEOE" (3 E, 3 O). NS=1 → "OEEOEO".
    #[test]
    fn parity_for_upce_covers_nonzero_check_digits() {
        // NS=0: direct table lookup.
        assert_eq!(parity_for_upce(b'0', 1), "EEOEOO");
        assert_eq!(parity_for_upce(b'0', 5), "EOOEEO");
        assert_eq!(parity_for_upce(b'0', 9), "EOOEOE");
        // NS=1: per-char O↔E inversion.
        assert_eq!(parity_for_upce(b'1', 1), "OOEOEE");
        assert_eq!(
            parity_for_upce(b'1', 5),
            "OEEOOE",
            "ordering matters — a mutant that drops one swap arm \
             would produce a different perm"
        );
        assert_eq!(parity_for_upce(b'1', 9), "OEEOEO");
    }

    #[test]
    fn upce_rejects_bad_check_digit() {
        // 01234560: 0123456 -> UPC-A expansion -> mod-10 check ≠ 0 in
        // general, so this should be rejected.
        let computed = gs1_check(&upce_to_upca("0123456").unwrap());
        let wrong = if computed == '0' { '1' } else { '0' };
        let bad = format!("0123456{wrong}");
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 228-229
        // (`UPC-E: supplied check digit X does not match computed Y
        // (via UPC-A expansion ZZZ...)`). Cross-family guard against
        // EAN-13/UPC-A.
        match encode_upce(&bad, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("UPC-E:"),
                    "missing `UPC-E:` family prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit"),
                    "missing `supplied check digit` predicate: {msg}"
                );
                assert!(
                    msg.contains("does not match computed"),
                    "missing `does not match computed` predicate: {msg}"
                );
                assert!(
                    msg.contains("via UPC-A expansion"),
                    "missing UPC-E-specific `via UPC-A expansion` echo: {msg}"
                );
                assert!(
                    !msg.contains("EAN-13") && !msg.contains("M&S"),
                    "cross-family contamination: UPC-E reject mentions EAN-13 or M&S: {msg}"
                );
            }
            other => panic!("bad-check UPC-E should reject as InvalidData, got {other:?}"),
        }
    }

    /// EAN-13 golden bar pattern captured from bwip-js's
    /// `raw("ean13", "0123456789012", {})[0].sbs`.
    #[test]
    fn ean13_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-13 byte-for-byte 59-bar SBS oracle path: 13-digit
        // pre-checked payload "0123456789012" → bwip-js raw SBS.
        let p = encode_ean13("0123456789012", &Options::default()).expect(
            "encode_ean13(\"0123456789012\", default) (EAN-13 byte-for-byte 59-bar SBS bwip-js raw oracle, pre-checked 13-digit payload) must succeed",
        );
        let want: [u8; 59] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 1,
            1,
        ];
        assert_eq!(p.bars, want, "ean13 bars mismatch vs bwip-js raw output");
    }

    /// EAN-8 golden from `raw("ean8", "1234567", {})[0].sbs`.
    #[test]
    fn ean8_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-8 byte-for-byte 43-bar SBS oracle path: 7-digit
        // payload "1234567" auto-checked → bwip-js raw SBS.
        let p = encode_ean8("1234567", &Options::default()).expect(
            "encode_ean8(\"1234567\", default) (EAN-8 byte-for-byte 43-bar SBS bwip-js raw oracle, 7-digit payload + auto-check digit 0) must succeed",
        );
        let want: [u8; 43] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1,
            1, 1, 4, 1, 3, 1, 2, 3, 2, 1, 1, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "ean8 bars mismatch vs bwip-js raw output");
    }

    /// UPC-A golden from `raw("upca", "01234567890", {})[0].sbs`.
    #[test]
    fn upca_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the UPC-A byte-for-byte 59-bar SBS oracle path: 11-digit
        // payload "01234567890" auto-checked → bwip-js raw SBS.
        let p = encode_upca("01234567890", &Options::default()).expect(
            "encode_upca(\"01234567890\", default) (UPC-A byte-for-byte 59-bar SBS bwip-js raw oracle, 11-digit payload auto-checked) must succeed",
        );
        let want: [u8; 59] = [
            1, 1, 1, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 4, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 1, 2, 3, 1, 1, 1,
            1,
        ];
        assert_eq!(p.bars, want, "upca bars mismatch vs bwip-js raw output");
    }

    /// M&S byte-for-byte golden from `raw("mands", "12345670", {})[0].sbs`.
    /// "12345670" is the 8-digit (data + check) form so we exercise the
    /// pass-through branch; the 7-char form is exercised by the prepend
    /// test below.
    #[test]
    fn mands_8_digit_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the M&S 8-digit pass-through path: "12345670" (data+check)
        // bypasses the leading-0 pad → bars identical to EAN-8 for
        // "1234567".
        let p = encode_mands("12345670", &Options::default()).expect(
            "encode_mands(\"12345670\", default) (M&S 8-digit pass-through path, equivalent to EAN-8 \"1234567\" oracle) must succeed",
        );
        // Identical to the ean8 oracle for "1234567" because M&S is
        // structurally just an EAN-8 with a leading 0 padded onto
        // 7-char input — and "12345670" is the same 7-data-digit
        // payload "0123456" with the computed check "7" appended.
        let want: [u8; 43] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1,
            1, 1, 4, 1, 3, 1, 2, 3, 2, 1, 1, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "mands bars mismatch vs bwip-js raw output");
    }

    #[test]
    fn mands_7_and_8_digit_forms_match() {
        // BWIPP M&S accepts a 7-char input only when after the leading
        // zero pad the result is itself a valid EAN-8 (data + check
        // both supplied). The trivial "0000000" → prepend → "00000000"
        // satisfies that (EAN-8 check of "0000000" is 0). The 8-char
        // form should produce identical bars to the 7-char form's
        // post-prepend output.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the M&S 7-vs-8-char equivalence path: 7-char "0000000" goes
        // through the leading-0 pad → "00000000" which equals the
        // 8-char form's bars.
        let p7 = encode_mands("0000000", &Options::default()).expect(
            "encode_mands(\"0000000\", default) (M&S 7-char prepend-0 path: \"0000000\" → \"00000000\" via leading-0 pad) must succeed",
        );
        let p8 = encode_mands("00000000", &Options::default()).expect(
            "encode_mands(\"00000000\", default) (M&S 8-char pass-through path; equivalent target for 7-char prepend-0 form) must succeed",
        );
        assert_eq!(
            p7.bars, p8.bars,
            "M&S 7-char (pad-prepended) and 8-char forms must produce identical bars"
        );
    }

    #[test]
    fn mands_7_digit_with_bad_post_prepend_check_rejects() {
        // BWIPP behaviour: 7-char input that, after the `0` pad,
        // doesn't form a valid EAN-8 surfaces an EAN-8 check error.
        // Mirrors `bwipp.raw("mands", "1234567", {})` failing with
        // `bwipp.ean8badCheckDigit`.
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin. M&S delegates the EAN-8 check failure through
        // the normalize helper; the diagnostic carries the `EAN-8:`
        // family prefix (M&S is a thin wrapper).
        match encode_mands("1234567", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-8:") || msg.contains("M&S"),
                    "missing EAN-8 or M&S family prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit") || msg.contains("does not match"),
                    "missing check-digit predicate: {msg}"
                );
                assert!(
                    !msg.contains("expected 7 or 8 digits"),
                    "wrong arm — length diagnostic leaked into check-digit arm: {msg}"
                );
            }
            other => panic!(
                "`1234567` with post-prepend bad check should reject as InvalidData, got {other:?}"
            ),
        }
    }

    #[test]
    fn mands_rejects_wrong_length() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 157-159
        // (`M&S: expected 7 or 8 digits, got 5`).
        match encode_mands("12345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("M&S:"), "missing `M&S:` family prefix: {msg}");
                assert!(
                    msg.contains("expected 7 or 8 digits"),
                    "missing length predicate: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` length echo: {msg}");
                assert!(
                    !msg.contains("supplied check digit"),
                    "wrong arm — check-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("5-digit M&S should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `normalize` (the private helper shared by
    /// encode_ean13 / encode_ean8 / encode_upca) directly. It accepts
    /// either `exact` digits (with user-supplied check, validated) or
    /// `exact - 1` digits (encoder computes check). The end-to-end
    /// EAN tests pin specific payloads through the public encoders;
    /// mutants in the inner logic (`exact - 1`, `body_len - 1`,
    /// `supplied != computed`) can sometimes survive if a specific
    /// input coincidentally produces a valid output anyway.
    ///
    /// Hand-computed for `normalize(input, 8, "EAN-8")`:
    ///   - "1234567" (7 digits): gs1_check("1234567") = '0'.
    ///     normalize returns "12345670".
    ///   - "12345670" (8 digits, correct check): returned as-is.
    ///   - "12345671" (8 digits, WRONG check): InvalidData.
    ///   - "123" (3 digits): InvalidData ("expected 7 or 8").
    ///   - "" (empty): InvalidData ("expected 7 or 8").
    ///   - "abc1234567" (10 chars, only 7 digits): digits_only filters
    ///     down to 7, so normalize returns "12345670".
    ///
    /// Hand-computed gs1_check("1234567"):
    ///   rev = "7654321"
    ///   i=0: 7*3=21
    ///   i=1: 6*1=6
    ///   i=2: 5*3=15
    ///   i=3: 4*1=4
    ///   i=4: 3*3=9
    ///   i=5: 2*1=2
    ///   i=6: 1*3=3
    ///   sum=60, (10-0)%10=0 → '0'.
    /// Stage 11.A8c — pin `digits_only` directly. The helper is the
    /// gateway through which every EAN/UPC payload passes before
    /// `normalize`; existing tests exercise it indirectly only.
    ///
    /// Mutations caught:
    ///   * `c.is_ascii_digit()` → `c.is_alphanumeric()` would let
    ///     letters through, breaking the digit-count check downstream.
    ///   * The whole filter dropped (`data.chars().collect()`) would
    ///     keep separators — caught by the dashes/spaces case.
    #[test]
    fn digits_only_strips_all_non_digits() {
        assert_eq!(digits_only(""), "");
        assert_eq!(digits_only("12345"), "12345");
        assert_eq!(digits_only("012-345-678-905"), "012345678905");
        assert_eq!(digits_only(" 12 34 "), "1234");
        assert_eq!(digits_only("abc1234567"), "1234567");
        assert_eq!(digits_only("ABCDEF"), "");
        // Punctuation, control chars, high-bit bytes all stripped.
        assert_eq!(digits_only("1!2@3#4$5%6^7&8*9(0)"), "1234567890");
        assert_eq!(digits_only("\t\n\r1\t"), "1");
        // Boundary chars adjacent to digit range.
        assert_eq!(
            digits_only("/0123456789:"),
            "0123456789",
            "'/' (just below '0') and ':' (just above '9') must be stripped"
        );
    }

    #[test]
    fn normalize_accepts_both_arities() {
        // exact-1 path: compute the check.
        assert_eq!(normalize("1234567", 8, "EAN-8").unwrap(), "12345670");
        // exact path with correct check: pass-through.
        assert_eq!(normalize("12345670", 8, "EAN-8").unwrap(), "12345670");
        // exact path with wrong check: InvalidData (specific message).
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("does not match")` upgraded to 4-anchor pin:
        //   1. `EAN-8:` family-name prefix
        //   2. `supplied check digit 1` value echo (last digit of
        //      "12345671" is 1; correct check would be 0)
        //   3. `does not match computed 0` value-echo for computed
        //      check (kills `{computed}` interpolation drop in the
        //      format string at line 83 of ean.rs)
        //   4. cross-arm guard: must NOT contain `expected 7 or 8`
        //      (the sibling length-mismatch arm)
        match normalize("12345671", 8, "EAN-8") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-8:"),
                    "missing EAN-8 family prefix: {msg:?}"
                );
                assert!(
                    msg.contains("supplied check digit 1"),
                    "missing supplied-check-digit echo `supplied check digit 1`: {msg:?}"
                );
                assert!(
                    msg.contains("does not match computed 0"),
                    "missing computed-check echo `does not match computed 0`: {msg:?}"
                );
                assert!(
                    !msg.contains("expected 7 or 8"),
                    "cross-arm contamination: check-mismatch reject mentions length arm: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(check), got {other:?}"),
        }
        // Wrong length: InvalidData with "expected 7 or 8" message.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("expected 7 or 8")` upgraded to 3-anchor pin (both 3-digit
        // and empty arms):
        //   1. `EAN-8:` family-name prefix (kills `{family}` drop or
        //      substitution mutations in the format string at line
        //      88-92 of ean.rs)
        //   2. `expected 7 or 8 digits` full predicate (kills `{exact
        //      - 1}` / `{exact}` boundary swaps; original was just
        //      "7 or 8" with no `digits`)
        //   3. per-arm `got N` value echo: `got 3` (3-digit input)
        //      and `got 0` (empty input — filtered to 0 digits)
        match normalize("123", 8, "EAN-8") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-8:"),
                    "missing EAN-8 family prefix for 3-digit: {msg:?}"
                );
                assert!(
                    msg.contains("expected 7 or 8 digits"),
                    "missing full predicate for 3-digit: {msg:?}"
                );
                assert!(msg.contains("got 3"), "missing `got 3` value-echo: {msg:?}");
            }
            other => panic!("expected InvalidData(length), got {other:?}"),
        }
        match normalize("", 8, "EAN-8") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-8:"),
                    "missing EAN-8 family prefix for empty: {msg:?}"
                );
                assert!(
                    msg.contains("expected 7 or 8 digits"),
                    "missing full predicate for empty: {msg:?}"
                );
                assert!(msg.contains("got 0"), "missing `got 0` value-echo: {msg:?}");
            }
            other => panic!("expected InvalidData on empty, got {other:?}"),
        }
        // Non-digit chars are filtered out via digits_only — letters
        // are removed silently and the encoder sees only the digit
        // subsequence. "abc1234567" → digits "1234567" (7 chars) →
        // exact-1 path produces "12345670".
        assert_eq!(normalize("abc1234567", 8, "EAN-8").unwrap(), "12345670");
        // Verify the family name appears in the error message
        // (catches a mutant that prefixes the wrong family).
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("EAN-13")` to 3-anchor pin matching the
        // source diagnostic at line 88-92 of ean.rs:
        //   1. `EAN-13` family-name prefix
        //   2. `expected 12 or 13 digits` full predicate (pins both
        //      `exact - 1 = 12` and `exact = 13`)
        //   3. `got 3` value echo ("123" has 3 digits)
        // The supplied digit-count echo pins the `format!(..., n)`
        // arg interpolation.
        match normalize("123", 13, "EAN-13") {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("EAN-13"), "missing `EAN-13` prefix: {msg}");
                assert!(
                    msg.contains("expected 12 or 13 digits"),
                    "missing `expected 12 or 13 digits` length-spec: {msg}"
                );
                assert!(msg.contains("got 3"), "missing `got 3` value echo: {msg}");
                assert!(
                    !msg.contains("supplied check digit"),
                    "wrong arm — check-digit-mismatch diagnostic leaked into length reject: {msg}"
                );
            }
            other => panic!("expected InvalidData with EAN-13, got {other:?}"),
        }
    }

    /// UPC-E golden from `raw("upce", "01234565", {})[0].sbs`.
    #[test]
    fn upce_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the UPC-E byte-for-byte 33-bar SBS oracle path: standard
        // UPC-E test vector "01234565" → bwip-js raw SBS.
        let p = encode_upce("01234565", &Options::default()).expect(
            "encode_upce(\"01234565\", default) (UPC-E byte-for-byte 33-bar SBS bwip-js raw oracle, standard test vector) must succeed",
        );
        let want: [u8; 33] = [
            1, 1, 1, 1, 2, 2, 2, 2, 1, 2, 2, 1, 4, 1, 1, 2, 3, 1, 1, 1, 3, 2, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "upce bars mismatch vs bwip-js raw output");
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests for the UPC-E expansion paths.
    // ---------------------------------------------------------------------

    /// Kills `upce_to_upca: delete match arm b'3'` and `delete match
    /// arm b'4'`. The original UPC-E corpus only exercised the
    /// `b'5'..=b'9'` arm (via "01234565"); deleting either of the
    /// b'3'/b'4' arms in the match would silently fall through to
    /// `_ => unreachable!()` for UPC-E inputs ending in 3 or 4 — but
    /// no existing test triggered that code path, so the mutants
    /// survived. The test below covers every last-digit class
    /// (0,1,2 share an arm, then 3, 4, 5..=9). We don't pre-compute
    /// the exact UPC-A expansion (the arm-specific zero-fill pattern
    /// is what the encoder is being tested for); instead we round-
    /// trip through the encoder by computing the GS1 check digit on
    /// whatever `upce_to_upca` returns, and assert that:
    ///   - the call itself does not panic (catches `delete match arm`),
    ///   - `encode_upce` accepts the result (catches downstream
    ///     drift in the expansion).
    #[test]
    fn upce_to_upca_covers_every_last_digit_branch() {
        for last in b'0'..=b'9' {
            // Build a clean 7-char UPC-E body: ns=0, mfr=12345, then the
            // variable last digit selecting which `upce_to_upca` match arm
            // we cover.
            let upce7 = format!("0{}{}{}{}{}{}", '1', '2', '3', '4', '5', last as char);
            assert_eq!(upce7.len(), 7);
            // upce_to_upca must not panic for any digit 0..=9.
            let upca = upce_to_upca(&upce7).unwrap_or_else(|e| {
                panic!("upce_to_upca({upce7}) failed: {e:?}");
            });
            // Sanity: UPC-A body is always 11 digits.
            assert_eq!(upca.len(), 11);
            // Round-trip via encode_upce with the freshly-computed
            // check digit. Asserts the expansion is encoder-valid.
            let check = gs1_check(&upca);
            let full_upce = format!("{upce7}{check}");
            assert!(
                encode_upce(&full_upce, &Options::default()).is_ok(),
                "encode_upce rejected a valid UPC-E ({full_upce}); \
                 upce_to_upca may have dropped a match arm"
            );
        }
    }

    /// Stage 11.A8c — pin the *exact* UPC-A expansion per
    /// `upce_to_upca` arm. The existing
    /// `upce_to_upca_covers_every_last_digit_branch` only checks that
    /// the call doesn't panic + `encode_upce` accepts the round-trip.
    /// Mutants that swap arm bodies (e.g. b'3' uses b'4''s template)
    /// can survive because the swapped expansion still has 11 digits
    /// and still encodes as some UPC-A.
    ///
    /// Hand-computed expansions for ns='0', mfr="12345":
    ///   - last='0': "{ns}{mfr0}{mfr1}{last}0000{mfr2..4}" →
    ///       "0" + "12" + "0" + "0000" + "345" = "01200000345"
    ///   - last='1': "01210000345"
    ///   - last='2': "01220000345"
    ///   - last='3': "{ns}{mfr0..2}00000{mfr3..4}" →
    ///       "0" + "123" + "00000" + "45" = "01230000045"
    ///   - last='4': "{ns}{mfr0..3}00000{mfr4}" →
    ///       "0" + "1234" + "00000" + "5" = "01234000005"
    ///   - last='5'..='9': "{ns}{mfr0..4}0000{last}" →
    ///       "012345" + "0000" + last
    ///     so last='5' → "01234500005",
    ///        last='8' → "01234500008".
    #[test]
    fn upce_to_upca_arm_specific_expansions() {
        // Arm 1: b'0' | b'1' | b'2' → zero-fill in middle, last char in pos 3.
        assert_eq!(upce_to_upca("0123450").unwrap(), "01200000345");
        assert_eq!(upce_to_upca("0123451").unwrap(), "01210000345");
        assert_eq!(upce_to_upca("0123452").unwrap(), "01220000345");
        // Arm 2: b'3' → 5-zero-fill in middle, last char NOT in output
        // (it tells which arm to use, the remaining 5 mfr digits form
        // the body).
        assert_eq!(upce_to_upca("0123453").unwrap(), "01230000045");
        // Arm 3: b'4' → 5-zero-fill after mfr[3], last char NOT in output.
        assert_eq!(upce_to_upca("0123454").unwrap(), "01234000005");
        // Arm 4: b'5'..=b'9' → 4-zero-fill, last char preserved at end.
        assert_eq!(upce_to_upca("0123455").unwrap(), "01234500005");
        assert_eq!(upce_to_upca("0123458").unwrap(), "01234500008");
        assert_eq!(upce_to_upca("0123459").unwrap(), "01234500009");
        // NS=1 variant — same per-arm shape, just ns char differs.
        assert_eq!(upce_to_upca("1234560").unwrap(), "12300000456");
    }

    /// Kills `encode_upce: replace != with ==` at line ~243 and
    /// `upce_to_upca: replace != with ==` at line ~295 — both are
    /// the `ns != b'0' && ns != b'1'` number-system-digit checks.
    /// Existing tests only exercised ns='0'; this asserts inputs
    /// with ns='2'..'9' are rejected.
    #[test]
    fn upce_rejects_non_zero_non_one_number_system_digit() {
        // Both rejection arms (upce_to_upca line 297 + encode_upce line
        // 245) share the same static diagnostic:
        //   "UPC-E: number system digit must be 0 or 1"
        // The loop covers all 8 invalid ns values; each iteration pins
        // the diagnostic substring (proves the format text hasn't been
        // mutated to e.g. "must be 0, 1, or 2" or the predicate dropped).
        for ns in b'2'..=b'9' {
            let body = format!("{}123456", ns as char);
            // upce_to_upca should reject with the shared diagnostic.
            match upce_to_upca(&body) {
                Err(Error::InvalidData(msg)) => assert!(
                    msg.contains("UPC-E:")
                        && msg.contains("number system digit")
                        && msg.contains("must be 0 or 1"),
                    "upce_to_upca(ns={}): diagnostic missing prefix/predicate/values; got {msg}",
                    ns as char
                ),
                other => panic!(
                    "upce_to_upca should reject ns={} as InvalidData, got {other:?}",
                    ns as char
                ),
            }
            // encode_upce should also reject 8-char inputs with bad ns.
            let with_check = format!("{}1234560", ns as char);
            match encode_upce(&with_check, &Options::default()) {
                Err(Error::InvalidData(msg)) => assert!(
                    msg.contains("UPC-E:")
                        && msg.contains("number system digit")
                        && msg.contains("must be 0 or 1"),
                    "encode_upce(ns={}): diagnostic missing prefix/predicate/values; got {msg}",
                    ns as char
                ),
                other => panic!(
                    "encode_upce should reject ns={} as InvalidData, got {other:?}",
                    ns as char
                ),
            }
        }
    }

    /// Stage 11.A8c — pin `digit(c)`. Single-line panic-on-non-digit
    /// helper used by every EAN/UPC encoder to convert an already-
    /// validated ASCII digit char to its numeric usize. The body is
    /// `c.to_digit(10).expect(...)` — pinning the base 10 anchor
    /// prevents `to_digit(16)` mutants that would silently admit
    /// `A..F` with wrong values.
    ///
    /// Mutations killed:
    ///   * `to_digit(10)` → `to_digit(16)`: 'A' would convert to 10
    ///     instead of panicking (callers expect 0..=9 only);
    ///   * `to_digit(10)` → `to_digit(2)`: '2' would panic;
    ///   * `as usize` (already trivial; behaviour preserved);
    ///   * `.expect(...)` → `.unwrap_or(0)`: would silently zero non-
    ///     digits — pin via per-digit-character round-trip.
    #[test]
    fn digit_all_ascii_digits_round_trip_to_their_usize_value() {
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(digit(c), i, "digit({c:?}) must equal {i}");
        }
    }

    /// Stage 11.A8c — pin `gs1_check(digits)`. The GS1 mod-10
    /// weighted-sum check-digit: right-to-left walk, odd positions
    /// (after `rev`) ×3, even positions ×1, sum mod 10, then
    /// `(10 - sum%10) % 10`. Used by ean::normalize across EAN-13 /
    /// EAN-8 / UPC-A / UPC-E paths, but pinned only indirectly via
    /// `ean13_validates_supplied_check_digit` which constructs both
    /// happy and wrong inputs from the helper's own output (so it
    /// can't catch identity mutants on the helper itself).
    ///
    /// Anchors pin all three observable behaviors with hand-computed
    /// values:
    ///   * empty: sum=0 → (10-0)%10 = 0 → '0';
    ///   * "0" → '0' (0×3 = 0);
    ///   * "1" → '7' (1×3 = 3; (10-3)%10 = 7);
    ///   * "9" → '3' (9×3 = 27; (10-7)%10 = 3) — covers wraparound
    ///     past 10 in the weighted sum;
    ///   * "12" → '3' (rev "21": 2×3 + 1 = 7; (10-7)%10 = 3) — pins
    ///     parity alternation;
    ///   * "123" → '6' (rev "321": 9+2+3 = 14; (10-14%10)%10 = 6);
    ///   * "012345678901" → '2' (standard 12-digit EAN-13 body);
    ///   * all-zeros: → '0' (wraparound check).
    ///
    /// Kills `.rev()` removal, weight-arm swap (3↔1), and
    /// `(10 - sum%10) % 10` → `sum % 10` shortcut.
    #[test]
    fn gs1_check_hand_computed_anchors() {
        // Boundary: empty input.
        assert_eq!(gs1_check(""), '0');

        // Single digit (only odd-position weight=3 applies).
        assert_eq!(gs1_check("0"), '0', "0×3 = 0 → '0'");
        assert_eq!(gs1_check("1"), '7', "1×3 = 3; (10-3)%10 = 7");
        assert_eq!(gs1_check("9"), '3', "9×3 = 27; (10-7)%10 = 3");

        // Two digits: rev exercises the weight alternation.
        assert_eq!(gs1_check("12"), '3', "rev 21: 2×3 + 1 = 7; (10-7)%10 = 3");

        // Three digits: both weights cycle.
        assert_eq!(gs1_check("123"), '6', "rev 321: 9+2+3 = 14; (10-4)%10 = 6");

        // Standard 12-digit EAN-13 body anchor.
        assert_eq!(
            gs1_check("012345678901"),
            '2',
            "12-digit GTIN body → check digit 2 (sum=98, 98%10=8, (10-8)%10=2)"
        );

        // All zeros wraparound.
        assert_eq!(gs1_check("0000000000000"), '0');
    }
}
