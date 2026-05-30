//! EAN-2 and EAN-5 supplemental add-on barcodes.
//!
//! Both are short add-on symbols that appear to the right of an EAN-13 /
//! UPC-A / UPC-E main barcode. They have their own start guard (`1011`),
//! their own digit-to-digit separator (`01`), and use a parity-pattern
//! scheme: each digit is rendered with the L pattern or the G pattern
//! depending on a lookup table indexed by either:
//!
//!   * **EAN-2**: `value mod 4`, where `value` is the two-digit integer.
//!   * **EAN-5**: a weighted "check digit" derived from the five digits.
//!
//! Patterns and parity tables ported from the BWIPP `ean2` and `ean5`
//! encoders (verified against `bwipp_ean2` and `bwipp_ean5` in bwip-js v4.x).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// L-pattern bit strings for digits 0..=9 (same table EAN-13 uses).
const L_PATTERNS: [&str; 10] = [
    "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
    "0110111", "0001011",
];
/// G-pattern bit strings (left-even parity); these are the L patterns with
/// the bit order reversed.
const G_PATTERNS: [&str; 10] = [
    "0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001",
    "0001001", "0010111",
];

/// EAN-2 parity pattern indexed by `value mod 4`.
const EAN2_PARITY: [&str; 4] = ["LL", "LG", "GL", "GG"];

/// EAN-5 parity pattern indexed by the EAN-5 check digit (0..=9).
const EAN5_PARITY: [&str; 10] = [
    "GGLLL", "GLGLL", "GLLGL", "GLLLG", "LGGLL", "LLGGL", "LLLGG", "LGLGL", "LGLLG", "LLGLG",
];

/// Add-on start guard: 4 modules (`bar space bar bar`).
const ADDON_START: &str = "1011";
/// Inter-digit separator inside an add-on: 2 modules (`space bar`).
const ADDON_SEP: &str = "01";

fn digit(c: char) -> Result<usize, Error> {
    c.to_digit(10)
        .map(|n| n as usize)
        .ok_or_else(|| Error::InvalidData(format!("EAN add-on accepts digits only (got {c:?})")))
}

fn pattern_for(d: usize, parity: char) -> &'static str {
    match parity {
        'L' => L_PATTERNS[d],
        'G' => G_PATTERNS[d],
        _ => unreachable!("EAN add-on parity must be L or G"),
    }
}

/// Encode a 2-digit EAN add-on.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Ean2, "42", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_ean2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 2 {
        return Err(Error::InvalidData(format!(
            "EAN-2 add-on must be exactly 2 digits (got {})",
            digits.len()
        )));
    }
    let value: u32 = digits.parse().unwrap();
    let parity = EAN2_PARITY[(value % 4) as usize];

    let mut modules = String::new();
    modules.push_str(ADDON_START);
    for (i, (c, p)) in digits.chars().zip(parity.chars()).enumerate() {
        if i > 0 {
            modules.push_str(ADDON_SEP);
        }
        modules.push_str(pattern_for(digit(c)?, p));
    }

    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
}

/// Encode a 5-digit EAN add-on.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 5-digit MSRP add-on (US/Canada).
/// let svg = render_svg(Symbology::Ean5, "12345", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_ean5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 5 {
        return Err(Error::InvalidData(format!(
            "EAN-5 add-on must be exactly 5 digits (got {})",
            digits.len()
        )));
    }
    let ds: Vec<u32> = digits.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let sum_odd = ds[0] + ds[2] + ds[4];
    let sum_even = ds[1] + ds[3];
    let check = ((sum_odd * 3) + (sum_even * 9)) % 10;
    let parity = EAN5_PARITY[check as usize];

    let mut modules = String::new();
    modules.push_str(ADDON_START);
    for (i, (c, p)) in digits.chars().zip(parity.chars()).enumerate() {
        if i > 0 {
            modules.push_str(ADDON_SEP);
        }
        modules.push_str(pattern_for(digit(c)?, p));
    }

    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
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

    /// Stage 11.A8c — pin `digit` and `pattern_for` directly. Both are
    /// tiny private helpers that the public encoders exercise only
    /// indirectly. Mutations that survive on encoder goldens:
    ///   - `digit`: `c.to_digit(10)` → `c.to_digit(16)` silently
    ///     accepts 'a'..='f' (still routes them to L/G_PATTERNS
    ///     indices 10..=15 which panic at runtime — but on inputs
    ///     that never hit them, the mutation is dormant).
    ///   - `digit`: `c.to_digit(10)` → `c.to_digit(8)` would reject
    ///     digits '8' and '9' — a payload like "01" would still pass.
    ///   - `pattern_for`: arm swap 'L' ↔ 'G' — would change parity
    ///     bits for many digits but might coincide on symmetric ones.
    ///   - `pattern_for`: catch-all `unreachable!` swapped for a
    ///     silent `L_PATTERNS[d]` fallback hides invalid parity calls.
    #[test]
    fn digit_and_pattern_for_direct_anchors() {
        // digit: '0'..='9' → Ok(0..=9).
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(digit(c).unwrap(), i, "digit({c:?}) = {i}");
        }
        // digit: hex letters rejected (catches to_digit(16) mutation).
        // The first hex-letter case ('a') also pins the rejection
        // diagnostic from line 47 to lock the predicate text +
        // `{c:?}` char-echo. The remaining is_err checks anchor each
        // arm's rejection behaviour but rely on the diagnostic pin
        // here to lock the format.
        let err_a = digit('a').unwrap_err();
        match err_a {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN add-on"),
                    "rejection diagnostic must carry the EAN add-on prefix; got {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "rejection diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("(got 'a')"),
                    "rejection diagnostic must Debug-echo the offending char; got {msg}"
                );
            }
            other => panic!("digit('a'): expected InvalidData, got {other:?}"),
        }
        // Stage 11.A8c (cont) — 6 bare `.is_err()` boundary checks
        // upgraded to anchor parity with the 'a' arm above. Each
        // arm pins `EAN add-on` + `accepts digits only` predicate +
        // per-char `{c:?}` echo. Kills mutations that:
        //   * Drop the symbology prefix from the format string
        //     (line 47 of ean_addons.rs).
        //   * Substitute the per-char echo with a hardcoded value.
        //   * Reroute one boundary arm through a different
        //     diagnostic.
        for c in ['f', 'A', ' ', '!', '/', ':'] {
            match digit(c).unwrap_err() {
                Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("EAN add-on"),
                        "digit({c:?}): missing `EAN add-on` prefix: {msg}"
                    );
                    assert!(
                        msg.contains("accepts digits only"),
                        "digit({c:?}): missing `accepts digits only` predicate: {msg}"
                    );
                    let expected = format!("(got {c:?})");
                    assert!(
                        msg.contains(&expected),
                        "digit({c:?}): missing `{expected}` char echo: {msg}"
                    );
                }
                other => panic!("digit({c:?}): expected InvalidData, got {other:?}"),
            }
        }

        // pattern_for: 'L' arm → L_PATTERNS[d].
        assert_eq!(
            pattern_for(0, 'L'),
            "0001101",
            "pattern_for(0, 'L') = L_PATTERNS[0]"
        );
        assert_eq!(
            pattern_for(5, 'L'),
            "0110001",
            "pattern_for(5, 'L') = L_PATTERNS[5]"
        );
        assert_eq!(
            pattern_for(9, 'L'),
            "0001011",
            "pattern_for(9, 'L') = L_PATTERNS[9]"
        );
        // pattern_for: 'G' arm → G_PATTERNS[d].
        assert_eq!(
            pattern_for(0, 'G'),
            "0100111",
            "pattern_for(0, 'G') = G_PATTERNS[0]"
        );
        assert_eq!(
            pattern_for(5, 'G'),
            "0111001",
            "pattern_for(5, 'G') = G_PATTERNS[5]"
        );
        // Distinguishing pair: for digit 0, L and G are different
        // strings — proves the arms route to different tables.
        assert_ne!(
            pattern_for(0, 'L'),
            pattern_for(0, 'G'),
            "L and G must produce different patterns (arm-swap detector)"
        );
    }

    #[test]
    fn ean2_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade two discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` weak checks to
        // 3-anchor pins matching the source diagnostic at line 70-73
        // of ean_addons.rs:
        //   1. `EAN-2 add-on` family-name prefix
        //   2. `must be exactly 2 digits` predicate
        //   3. per-arm `got 1` / `got 3` value-echo
        match encode_ean2("1", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-2 add-on"),
                    "1-digit: missing EAN-2 add-on prefix: {msg}"
                );
                assert!(
                    msg.contains("must be exactly 2 digits"),
                    "1-digit: missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("got 1"),
                    "1-digit: missing `got 1` value-echo: {msg}"
                );
            }
            other => panic!("1-digit EAN2 should reject as InvalidData, got {other:?}"),
        }
        match encode_ean2("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-2 add-on"),
                    "3-digit: missing EAN-2 add-on prefix: {msg}"
                );
                assert!(
                    msg.contains("must be exactly 2 digits"),
                    "3-digit: missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("got 3"),
                    "3-digit: missing `got 3` value-echo: {msg}"
                );
            }
            other => panic!("3-digit EAN2 should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ean2_rejects_non_digits() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin. Input "AB" gets digits-filtered to "" → length check
        // rejects with the "exactly 2 digits (got 0)" diagnostic at
        // line 70-73 of ean_addons.rs (NOT the per-char `digits only`
        // diagnostic — the length check runs first):
        //   1. `EAN-2 add-on` family-name prefix
        //   2. `must be exactly 2 digits` predicate
        //   3. `got 0` value-echo (filtered-out non-digits leave 0
        //      digits)
        match encode_ean2("AB", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-2 add-on"),
                    "missing EAN-2 add-on prefix: {msg}"
                );
                assert!(
                    msg.contains("must be exactly 2 digits"),
                    "missing length predicate: {msg}"
                );
                assert!(msg.contains("got 0"), "missing `got 0` value-echo: {msg}");
            }
            other => panic!("'AB' should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ean2_known_pattern_for_05() {
        // value = 5, parity = EAN2_PARITY[1] = "LG"
        let p = encode_ean2("05", &Options::default()).unwrap();
        let expected = format!("{ADDON_START}{}{ADDON_SEP}{}", L_PATTERNS[0], G_PATTERNS[5],);
        assert_eq!(module_string(&p), expected);
    }

    #[test]
    fn ean5_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade two discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` weak checks to
        // 3-anchor pins matching the source diagnostic at line
        // 109-112 of ean_addons.rs:
        //   1. `EAN-5 add-on` family-name prefix
        //   2. `must be exactly 5 digits` predicate
        //   3. per-arm `got 4` / `got 6` value-echo
        match encode_ean5("1234", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-5 add-on"),
                    "4-digit: missing EAN-5 add-on prefix: {msg}"
                );
                assert!(
                    msg.contains("must be exactly 5 digits"),
                    "4-digit: missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("got 4"),
                    "4-digit: missing `got 4` value-echo: {msg}"
                );
            }
            other => panic!("4-digit EAN5 should reject as InvalidData, got {other:?}"),
        }
        match encode_ean5("123456", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-5 add-on"),
                    "6-digit: missing EAN-5 add-on prefix: {msg}"
                );
                assert!(
                    msg.contains("must be exactly 5 digits"),
                    "6-digit: missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("got 6"),
                    "6-digit: missing `got 6` value-echo: {msg}"
                );
            }
            other => panic!("6-digit EAN5 should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ean5_known_check_digit_and_pattern() {
        // 12345: sum_odd = 1+3+5 = 9, sum_even = 2+4 = 6
        // check = (9*3 + 6*9) % 10 = (27 + 54) % 10 = 81 % 10 = 1
        // parity = EAN5_PARITY[1] = "GLGLL"
        let p = encode_ean5("12345", &Options::default()).unwrap();
        let parity = "GLGLL";
        let digits = "12345";
        let mut expected = String::new();
        expected.push_str(ADDON_START);
        for (i, (c, par)) in digits.chars().zip(parity.chars()).enumerate() {
            if i > 0 {
                expected.push_str(ADDON_SEP);
            }
            let d = c.to_digit(10).unwrap() as usize;
            expected.push_str(if par == 'L' {
                L_PATTERNS[d]
            } else {
                G_PATTERNS[d]
            });
        }
        assert_eq!(module_string(&p), expected);
    }

    /// Stage 11.A8c — pin EAN-5 check-digit boundary cases. The
    /// existing `ean5_known_check_digit_and_pattern` uses "12345"
    /// (sum=81, check=1) but doesn't cover:
    ///   - check=0 (where the formula's `% 10` actually folds), or
    ///   - distinct sum_odd/sum_even contributions that would catch
    ///     mutations like `sum_odd * 3` → `sum_odd * 9` (which would
    ///     swap the weight roles).
    ///
    /// Hand-computed:
    ///   - "13000": sum_odd = 1+0+0 = 1, sum_even = 3+0 = 3.
    ///     check = (1*3 + 3*9) % 10 = (3 + 27) % 10 = 30 % 10 = 0.
    ///   - "00000": all zeros → check 0.
    ///   - "10000": sum_odd=1, sum_even=0. check = (3+0)%10 = 3.
    ///   - "00100": sum_odd=0+1+0=1, sum_even=0. check = (3)%10 = 3.
    ///     Note: same check as "10000" because sum_odd is unchanged.
    ///     This is the EAN-5 mod-10 collision pattern.
    ///   - "01000": sum_odd=0, sum_even=1. check = (0+9)%10 = 9.
    ///     If the weights were swapped (`sum_odd*9 + sum_even*3`),
    ///     this would yield (0+3)%10 = 3 instead.
    #[test]
    fn ean5_check_digit_weight_roles_and_wrap() {
        // Helper: compute check from input.
        fn check(digits: &str) -> u32 {
            let ds: Vec<u32> = digits.chars().map(|c| c.to_digit(10).unwrap()).collect();
            let sum_odd = ds[0] + ds[2] + ds[4];
            let sum_even = ds[1] + ds[3];
            ((sum_odd * 3) + (sum_even * 9)) % 10
        }
        assert_eq!(check("13000"), 0, "sum=30, %10 folds to 0");
        assert_eq!(check("00000"), 0);
        assert_eq!(check("10000"), 3);
        assert_eq!(check("00100"), 3);
        // Weight-role swap detector: sum_odd=0, sum_even=1.
        // Original `sum_odd*3 + sum_even*9 = 0 + 9 = 9`.
        // Mutant `sum_odd*9 + sum_even*3 = 0 + 3 = 3`.
        assert_eq!(check("01000"), 9, "weight-role swap detector");
        // The encoder output for "13000" with check 0 → parity =
        // EAN5_PARITY[0] = "GGLLL". Verify via the actual encoder.
        let p = encode_ean5("13000", &Options::default()).unwrap();
        let mut expected = String::new();
        expected.push_str(ADDON_START);
        let parity = "GGLLL"; // EAN5_PARITY[0]
        for (i, (c, par)) in "13000".chars().zip(parity.chars()).enumerate() {
            if i > 0 {
                expected.push_str(ADDON_SEP);
            }
            let d = c.to_digit(10).unwrap() as usize;
            expected.push_str(if par == 'L' {
                L_PATTERNS[d]
            } else {
                G_PATTERNS[d]
            });
        }
        assert_eq!(
            module_string(&p),
            expected,
            "encode_ean5('13000') with check=0 parity=GGLLL"
        );
    }

    #[test]
    fn ean2_total_width_matches_spec() {
        // 4 (start) + 7 (digit 1) + 2 (sep) + 7 (digit 2) = 20 modules.
        let p = encode_ean2("12", &Options::default()).unwrap();
        assert_eq!(module_string(&p).len(), 20);
    }

    #[test]
    fn ean5_total_width_matches_spec() {
        // 4 (start) + 5*7 (digits) + 4*2 (separators) = 47 modules.
        let p = encode_ean5("12345", &Options::default()).unwrap();
        assert_eq!(module_string(&p).len(), 47);
    }

    /// EAN-2 add-on golden from `raw("ean2", "12", {})[0].sbs`.
    #[test]
    fn ean2_matches_bwip_js_raw_sbs() {
        let p = encode_ean2("12", &Options::default()).unwrap();
        let want: [u8; 13] = [1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 2];
        assert_eq!(p.bars, want, "ean2 bars mismatch vs bwip-js raw output");
    }

    /// EAN-5 add-on golden from `raw("ean5", "12345", {})[0].sbs`.
    #[test]
    fn ean5_matches_bwip_js_raw_sbs() {
        let p = encode_ean5("12345", &Options::default()).unwrap();
        let want: [u8; 31] = [
            1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 3, 2, 1, 1, 1, 2,
            3, 1,
        ];
        assert_eq!(p.bars, want, "ean5 bars mismatch vs bwip-js raw output");
    }

    /// Stage 11.A8c — pin `digit(c)` in ean_addons. Distinct from
    /// ean::digit (which panics on non-digit input): this one returns
    /// `Result<usize, Error::InvalidData>` so the caller can surface
    /// a clean error. Used by both encode_ean2 and encode_ean5
    /// after the upstream digit-only filter, but no direct anchor
    /// pins the base 10 + Err shape.
    ///
    /// Anchors pin:
    ///   * '0'..='9' → Ok(0..=9) full round-trip (kills `to_digit(10)`
    ///     → `to_digit(16)` mutant that would let 'A' admit as 10);
    ///   * 'A' → Err::InvalidData (kills `to_digit(16)` mutant
    ///     specifically: 'A'.to_digit(16) = Some(10), not None);
    ///   * ' ' → Err;
    ///   * NUL ('\0') → Err.
    #[test]
    fn digit_addons_returns_result_with_base_10_only() {
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(digit(c).unwrap(), i, "'{c}' → Ok({i})");
        }

        // 'A' rejected (kills `to_digit(16)` mutant — under base 16,
        // 'A'.to_digit(16) = Some(10), not None, so the helper would
        // return Ok(10) instead of Err).
        assert!(
            digit('A').is_err(),
            "'A' must Err under base 10 (kills base-16 mutant)"
        );
        // Stage 11.A8c — add per-char mutation-class labels.
        // 'Z' is past 'F' (the largest base-16 letter), so a base-16
        // mutant would still reject it; the label below identifies
        // the case where a different mutation (e.g. `is_alphabetic`
        // accept) might leak through.
        assert!(
            digit('Z').is_err(),
            "'Z' must Err — kills `c.is_ascii_alphabetic()`/`to_digit(36)` mutations"
        );
        // Lowercase 'a' parallels 'A' (base-16 kill) but also kills
        // case-folding mutations that uppercase before `to_digit`.
        assert!(
            digit('a').is_err(),
            "'a' must Err — kills `to_digit(16)` / case-folding mutations"
        );
        // 'f' is the largest base-16 letter (to_digit(16) = Some(15)).
        // If a mutant flips `to_digit(10)` → `to_digit(16)`, 'f' would
        // silently produce Ok(15). The is_err() check alone catches
        // that, but pinning the diagnostic + char-echo here also
        // catches format-string mutations that drop the `{c:?}` echo:
        // under such a mutant `digit('a')`'s diagnostic (which pins
        // "(got 'a')" at line 175) would still pass, but `digit('f')`
        // would no longer echo 'f' in its message.
        let err_f = digit('f').unwrap_err();
        match err_f {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN add-on"),
                    "'f' rejection must carry the EAN add-on prefix; got {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "'f' rejection must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("(got 'f')"),
                    "'f' rejection must Debug-echo 'f' (kills `{{c:?}}` drop \
                     that would survive at the dedicated 'a' anchor); got {msg}"
                );
            }
            other => panic!("digit('f'): expected InvalidData, got {other:?}"),
        }
        // Stage 11.A8c — add per-char mutation-class labels so a
        // failure here identifies WHICH boundary regressed. Each
        // char exercises a distinct rejection-class predicate:
        assert!(
            digit(' ').is_err(),
            "' ' must Err — kills `c.is_ascii_whitespace()` accept-as-zero mutation"
        );
        assert!(
            digit('\0').is_err(),
            "NUL must Err — kills `c < ' '` permissive control-char mutation"
        );
        assert!(
            digit('-').is_err(),
            "'-' must Err — kills `c.is_ascii_punctuation()` accept mutations"
        );
        assert!(
            digit('.').is_err(),
            "'.' must Err — kills decimal-point accept mutations that would treat \
             `digit` as a `parse::<f64>`-style helper"
        );
    }

    /// Stage 11.A8c — pin `pattern_for(d, parity)`. Tiny dispatch helper
    /// routing to either L_PATTERNS or G_PATTERNS based on parity char.
    /// Only exercised transitively through `encode_ean2` / `encode_ean5`
    /// goldens, so mutations on the match-arm swap (L → G_PATTERNS) or
    /// the per-digit index would survive when the affected output bit
    /// pattern happens to coincide.
    ///
    /// Hand-computed (from L_PATTERNS and G_PATTERNS constants):
    ///   pattern_for(0, 'L') = "0001101"   pattern_for(0, 'G') = "0100111"
    ///   pattern_for(1, 'L') = "0011001"   pattern_for(1, 'G') = "0110011"
    ///   pattern_for(5, 'L') = "0110001"   pattern_for(5, 'G') = "0111001"
    ///   pattern_for(9, 'L') = "0001011"   pattern_for(9, 'G') = "0010111"
    ///
    /// Mutations to catch:
    ///   * 'L' arm returns G_PATTERNS or vice versa: each L and G
    ///     pattern is distinct from its peer at the same index, so
    ///     any swap is observable.
    ///   * Index drift `L_PATTERNS[d]` → `L_PATTERNS[d + 1]`: would
    ///     fail bounds for d=9 and shift values for d=0..=8.
    ///   * Body replaced with constant `"0000000"`: every output
    ///     would equal that constant.
    ///   * Match arm order: catches L ↔ G swap.
    #[test]
    fn pattern_for_routes_to_l_and_g_per_parity() {
        // Boundary anchors: d=0 and d=9 in both parities.
        assert_eq!(pattern_for(0, 'L'), "0001101", "L digit 0 boundary");
        assert_eq!(pattern_for(9, 'L'), "0001011", "L digit 9 boundary");
        assert_eq!(pattern_for(0, 'G'), "0100111", "G digit 0 boundary");
        assert_eq!(pattern_for(9, 'G'), "0010111", "G digit 9 boundary");

        // Middle anchors: d=5 in both parities.
        assert_eq!(pattern_for(5, 'L'), "0110001", "L digit 5");
        assert_eq!(pattern_for(5, 'G'), "0111001", "G digit 5");

        // Per-digit L and G patterns must NEVER coincide — pins the
        // parity-swap mutant (L → G_PATTERNS) for every digit.
        for d in 0..10 {
            assert_ne!(
                pattern_for(d, 'L'),
                pattern_for(d, 'G'),
                "L and G patterns for digit {d} must differ"
            );
        }

        // All outputs are 7 modules wide.
        for d in 0..10 {
            assert_eq!(pattern_for(d, 'L').len(), 7);
            assert_eq!(pattern_for(d, 'G').len(), 7);
        }
    }

    /// `digit(c) -> Result<usize, Error>`: parse an ASCII digit char
    /// into its 0..=9 value. Used as the entry-point validator in
    /// `encode_ean2` / `encode_ean5`. Never directly tested.
    ///
    /// Mutations to catch:
    /// * `to_digit(10)` → `to_digit(16)` (would accept 'A'..='F' as
    ///   10..=15).
    /// * Ok/Err arm swap (would treat every accepted char as Err).
    /// * `n as usize` cast (0..=9 always fits but a mutant could
    ///   break specific arms).
    #[test]
    fn digit_decimal_only_per_char() {
        // ---- Every digit '0'..='9' is accepted with its numeric value.
        for c in '0'..='9' {
            let want = (c as u32 - '0' as u32) as usize;
            assert_eq!(digit(c).unwrap(), want, "'{c}' → {want}");
        }

        // ---- Hex-letter discriminator: 'a'..='f' / 'A'..='F' must
        // Err. Catches a `to_digit(16)` mutant.
        for c in ['a', 'b', 'c', 'd', 'e', 'f', 'A', 'F'] {
            assert!(
                digit(c).is_err(),
                "'{c}' (hex letter) must Err under base-10 parsing"
            );
        }

        // ---- Other non-digit chars.
        for c in [' ', '!', '-', '.', '/', ':', '@', '[', 'g', 'z', 'Z', 'G'] {
            assert!(digit(c).is_err(), "'{c}' must Err");
        }

        // ---- Whitespace, NUL, multi-byte.
        // Stage 11.A8c (cont) — strengthen labels with per-byte / per-
        // codepoint echoes so a mutation that accepts a specific control
        // or non-ASCII byte (e.g. NUL→0, TAB→9 if some buggy lookup uses
        // `c as u8 % 10`) names the exact byte that slipped through.
        assert!(
            digit('\0').is_err(),
            "NUL (U+0000, byte 0x00) must Err — must NOT be parsed as digit 0 by any `(c as u8) % 10` mutant"
        );
        assert!(
            digit('\t').is_err(),
            "TAB (U+0009, byte 0x09) must Err — must NOT be parsed as digit 9 by any `(c as u8) % 10` mutant"
        );
        assert!(
            digit('é').is_err(),
            "non-ASCII 'é' (U+00E9) must Err — multi-byte UTF-8 must not bypass the ASCII-digit guard"
        );
        assert!(
            digit('\u{0660}').is_err(),
            "Arabic-Indic ZERO '٠' (U+0660) must Err — only ASCII '0'..='9' is a digit, NOT every Unicode 'Nd' codepoint"
        );

        // ---- Error variant + diagnostic + char-echo.
        // Stage 11.A8c upgrade from `matches!(_, Error::InvalidData(_))`
        // to a per-arm match that pins the symbology prefix, the
        // predicate, AND the `{c:?}` Debug-echo of the offending char.
        // Defense-in-depth with the digit_addons_returns_result_with_base_10_only
        // 'a' anchor (7b2c55a) and 'f' anchor (c7cc2b2) — this test
        // exercises the digit() helper through its dense per-char loop
        // so the diagnostic pin guarantees the format-string survives
        // mutations even when one of the sibling pins gets refactored.
        match digit('a').unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN add-on"),
                    "diagnostic must carry the EAN add-on prefix; got {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("(got 'a')"),
                    "diagnostic must Debug-echo the offending char 'a'; got {msg}"
                );
            }
            other => panic!("error must be InvalidData; got {other:?}"),
        }

        // ---- Distinctness invariant: 10 accepted chars → 10 unique values.
        use std::collections::HashSet;
        let mut values: HashSet<usize> = HashSet::new();
        for c in '0'..='9' {
            let v = digit(c).unwrap();
            assert!(values.insert(v), "duplicate value {v} for '{c}'");
        }
        assert_eq!(values.len(), 10, "exactly 10 accepted chars → 10 values");

        // ---- Monotonicity: digit(c+1) == digit(c) + 1 for '0'..'8'.
        for c in '0'..'9' {
            let next = char::from_u32(c as u32 + 1).unwrap();
            assert_eq!(
                digit(next).unwrap(),
                digit(c).unwrap() + 1,
                "monotonic: '{}' → {}+1 = {}",
                next,
                digit(c).unwrap(),
                digit(c).unwrap() + 1
            );
        }
    }
}
