//! Deutsche Post Identcode and Leitcode.
//!
//! Both are fixed-length numeric identifiers encoded as Interleaved 2 of 5
//! with a Deutsche Post mod-10 check digit (weights `4` and `9` alternating).
//!
//! * **Identcode**: 11 data digits + 1 check digit = 12 digits total.
//! * **Leitcode**: 13 data digits + 1 check digit = 14 digits total.
//!
//! Algorithm verified against `bwipp_identcode` and `bwipp_leitcode` in
//! bwip-js v4.x.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

use super::interleaved2of5;

/// Encode a Deutsche Post Identcode (11 digits + computed check, or 12 with
/// supplied check).
pub fn encode_identcode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_dp(data, opts, 11, "Identcode")
}

/// Encode a Deutsche Post Leitcode (13 digits + computed check, or 14 with
/// supplied check).
pub fn encode_leitcode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_dp(data, opts, 13, "Leitcode")
}

fn encode_dp(
    data: &str,
    opts: &Options,
    body_len: usize,
    name: &'static str,
) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != data.chars().count() {
        return Err(Error::InvalidData(format!(
            "Deutsche Post {name}: digits only (got {data:?})"
        )));
    }
    let full = match digits.len() {
        n if n == body_len => {
            let check = dp_check(&digits);
            format!("{digits}{check}")
        }
        n if n == body_len + 1 => {
            let body = &digits[..body_len];
            let supplied = digits.chars().last().unwrap();
            let computed = dp_check(body);
            if supplied != computed {
                return Err(Error::InvalidData(format!(
                    "Deutsche Post {name}: supplied check digit {supplied} \
                     does not match computed {computed}"
                )));
            }
            digits
        }
        n => {
            return Err(Error::InvalidData(format!(
                "Deutsche Post {name}: expected {body_len} or {} digits, got {n}",
                body_len + 1
            )));
        }
    };
    interleaved2of5::encode(&full, opts)
}

/// Compute the Deutsche Post mod-10 check digit. Weights `4` and `9` alternate
/// starting with `4` at position 0.
fn dp_check(body: &str) -> char {
    let mut sum: u32 = 0;
    for (i, c) in body.chars().enumerate() {
        let n = c.to_digit(10).unwrap();
        let weight = if i % 2 == 0 { 4 } else { 9 };
        sum += n * weight;
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identcode_check_digit_known() {
        // BWIPP example: "34567890123" -> compute the check.
        // weights 4,9,4,9,4,9,4,9,4,9,4
        // 3*4 + 4*9 + 5*4 + 6*9 + 7*4 + 8*9 + 9*4 + 0*9 + 1*4 + 2*9 + 3*4
        // = 12 + 36 + 20 + 54 + 28 + 72 + 36 + 0 + 4 + 18 + 12 = 292
        // check = (10 - 292%10) % 10 = (10 - 2) % 10 = 8
        assert_eq!(dp_check("34567890123"), '8');
    }

    #[test]
    fn leitcode_check_digit_known() {
        // weights 4,9,4,9,...,4 over 13 digits.
        // For "1234567890123":
        // 1*4 + 2*9 + 3*4 + 4*9 + 5*4 + 6*9 + 7*4 + 8*9 + 9*4 + 0*9 + 1*4 + 2*9 + 3*4
        // = 4 + 18 + 12 + 36 + 20 + 54 + 28 + 72 + 36 + 0 + 4 + 18 + 12 = 314
        // check = (10 - 314%10) % 10 = (10 - 4) % 10 = 6
        assert_eq!(dp_check("1234567890123"), '6');
    }

    #[test]
    fn identcode_renders_with_computed_check() {
        // Stage 11.A8c (cont) — descriptive label naming input + path.
        // "34567890123" is the 11-digit Identcode body; the encoder
        // computes the 12th check digit and produces a 12-char symbol.
        let p = encode_identcode("34567890123", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_identcode(\"34567890123\") (11-digit body → 12-char with computed check) must compose into non-empty Deutsche Post Identcode symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn identcode_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 60-62 of identleitcode.rs (`Deutsche Post Identcode:
        // expected 11 or 12 digits, got 4`).
        match encode_identcode("1234", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Deutsche Post Identcode:"),
                    "missing `Deutsche Post Identcode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected 11 or 12 digits"),
                    "missing `expected 11 or 12 digits` length-spec: {msg}"
                );
                assert!(msg.contains("got 4"), "missing `got 4` echo: {msg}");
                assert!(
                    !msg.contains("Leitcode"),
                    "wrong helper — Leitcode diagnostic leaked into Identcode reject: {msg}"
                );
            }
            other => panic!("4-digit Identcode should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn identcode_validates_supplied_check_digit() {
        // Append a wrong check; should error.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 52-55 of identleitcode.rs (`Deutsche Post Identcode:
        // supplied check digit X does not match computed Y`).
        // Input "345678901230" is 12 digits; the last char '0' is
        // the supplied check.
        match encode_identcode("345678901230", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Deutsche Post Identcode:"),
                    "missing `Deutsche Post Identcode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit 0"),
                    "missing `supplied check digit 0` value echo: {msg}"
                );
                assert!(
                    msg.contains("does not match computed"),
                    "missing `does not match computed` predicate: {msg}"
                );
                // Stage 11.A8c (cont) — symmetric cross-helper guard
                // (the Leitcode sibling test already guards against
                // Identcode leakage; this test was missing the inverse
                // guard). Kills a mutation that re-routes the
                // Identcode check-mismatch through the Leitcode
                // diagnostic helper.
                assert!(
                    !msg.contains("Leitcode"),
                    "wrong helper — Leitcode diagnostic leaked into Identcode reject: {msg}"
                );
            }
            other => panic!(
                "encode_identcode(\"345678901230\") must reject as Err(InvalidData(check-mismatch)); got {other:?}"
            ),
        }
    }

    #[test]
    fn leitcode_renders_with_computed_check() {
        // Stage 11.A8c (cont) — descriptive label naming input + path.
        // "1234567890123" is the 13-digit Leitcode body; the encoder
        // computes the 14th check digit and produces a 14-char symbol.
        let p = encode_leitcode("1234567890123", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_leitcode(\"1234567890123\") (13-digit body → 14-char with computed check) must compose into non-empty Deutsche Post Leitcode symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn leitcode_validates_supplied_check_digit() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 52-55 of identleitcode.rs (`Deutsche Post Leitcode:
        // supplied check digit X does not match computed Y`).
        // Input "12345678901230" is 14 digits; the last char '0' is
        // the supplied check.
        match encode_leitcode("12345678901230", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Deutsche Post Leitcode:"),
                    "missing `Deutsche Post Leitcode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("supplied check digit 0"),
                    "missing `supplied check digit 0` value echo: {msg}"
                );
                assert!(
                    msg.contains("does not match computed"),
                    "missing `does not match computed` predicate: {msg}"
                );
                assert!(
                    !msg.contains("Identcode"),
                    "wrong helper — Identcode diagnostic leaked into Leitcode reject: {msg}"
                );
            }
            other => panic!("wrong-check Leitcode should reject as InvalidData, got {other:?}"),
        }
    }

    /// Identcode golden from `raw("identcode", "34567890123", {})[0].sbs`.
    #[test]
    fn identcode_matches_bwip_js_raw_sbs() {
        let p = encode_identcode("34567890123", &Options::default()).unwrap();
        let want: [u8; 68] = [
            1, 1, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 2, 1, 1, 1,
            1, 2, 2, 2, 1, 1, 1, 2, 1, 1, 2, 2, 2, 1, 1, 2, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 2, 2, 1,
            1, 1, 1, 2, 1, 1, 2, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "identcode bars mismatch vs bwip-js raw output"
        );
    }

    /// Leitcode golden from `raw("leitcode", "1234567890123", {})[0].sbs`.
    #[test]
    fn leitcode_matches_bwip_js_raw_sbs() {
        let p = encode_leitcode("1234567890123", &Options::default()).unwrap();
        let want: [u8; 78] = [
            1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 2, 2,
            2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 2, 1, 1, 2, 2, 2, 1, 1, 2, 1, 1, 2,
            1, 1, 1, 1, 2, 2, 2, 1, 2, 2, 1, 2, 1, 1, 1, 1, 2, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "leitcode bars mismatch vs bwip-js raw output");
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills three mutants on line ~47 of `encode_dp` (the
    /// `match n if n == body_len + 1` arm — replaced with `false` (the
    /// arm never triggers), `+ with -` (arm matches body_len-1 instead),
    /// or `+ with *` (arm matches body_len instead, same as the first
    /// arm)). Each existing test only exercises the body_len-digit path
    /// or the body_len+1-with-WRONG-check path (which falls through to
    /// the catch-all error). This test brute-force-scans for the
    /// correct check digit by trying all 10 possibilities; one of them
    /// must succeed — under any of the three mutants the body_len+1
    /// path is never reached so every digit fails.
    #[test]
    fn dp_codes_accept_body_len_plus_one_with_correct_check() {
        // identcode: body_len=11. Probe all 10 possible trailing
        // digits; exactly one is the right check digit and the
        // encoder must accept it via the body_len+1 path.
        let mut accepted_identcode = false;
        for d in '0'..='9' {
            let candidate = format!("12345678901{d}");
            if encode_identcode(&candidate, &Options::default()).is_ok() {
                accepted_identcode = true;
                break;
            }
        }
        assert!(
            accepted_identcode,
            "encode_identcode rejected every 12-digit candidate; \
             the body_len+1 match arm in encode_dp may have been disabled"
        );
        // leitcode: body_len=13. Same scan.
        let mut accepted_leitcode = false;
        for d in '0'..='9' {
            let candidate = format!("1234567890123{d}");
            if encode_leitcode(&candidate, &Options::default()).is_ok() {
                accepted_leitcode = true;
                break;
            }
        }
        assert!(
            accepted_leitcode,
            "encode_leitcode rejected every 14-digit candidate; \
             the body_len+1 match arm in encode_dp may have been disabled"
        );
    }

    /// Stage 11.A8c — direct `dp_check` killer pinning weight-pattern
    /// asymmetry + wrap-around `(10 - sum%10) % 10 == 0`.
    ///
    /// Existing anchors `identcode_check_digit_known("34567890123") → '8'`
    /// and `leitcode_check_digit_known("1234567890123") → '6'` together
    /// catch a coarse weight-swap mutant only by coincidence: the
    /// 11-digit identcode anchor produces '8' under BOTH `[4,9,4,9,…]`
    /// and the swapped `[9,4,9,4,…]` weight pattern (sum mod 10
    /// collides at 2 → check 8). A weight-swap mutant pruned through the
    /// 13-digit leitcode anchor alone, leaving the identcode anchor as
    /// false reassurance.
    ///
    /// This test pins each weight position independently with single-
    /// and double-digit anchors that DO distinguish `[4,9]` from
    /// `[9,4]`, and it pins the wrap-around `(10 - 0) % 10 == 0` arm
    /// directly. Hand-computed expecteds below.
    #[test]
    fn dp_check_weight_pattern_and_wrap_around_pins() {
        // ----- wrap-around arm: sum % 10 == 0 → check '0' -----
        // ""   → sum 0           → (10 - 0) % 10 = 0
        // "0"  → 0*4 = 0          → 0
        // "5"  → 5*4 = 20         → (10 - 20%10) % 10 = (10-0)%10 = 0
        // "32" → 3*4 + 2*9 = 30   → (10 - 30%10) % 10 = (10-0)%10 = 0
        assert_eq!(dp_check(""), '0', "empty body must wrap to '0'");
        assert_eq!(dp_check("0"), '0', "single '0' must wrap to '0'");
        assert_eq!(
            dp_check("5"),
            '0',
            "single '5' (sum=20) must wrap; if mutant uses weight 9 at \
             position 0, sum=45 → '5' (distinct)"
        );
        assert_eq!(
            dp_check("32"),
            '0',
            "multi-digit wrap: 3*4 + 2*9 = 30 → '0'; weight-swap would \
             yield 3*9 + 2*4 = 35 → '5' (distinct)"
        );

        // ----- non-wrap anchors that catch weight-pattern swap -----
        // "9"  → 9*4 = 36         → (10 - 6) % 10 = 4
        // "12" → 1*4 + 2*9 = 22   → (10 - 2) % 10 = 8
        // "10" → 1*4 + 0*9 = 4    → (10 - 4) % 10 = 6
        // "23" → 2*4 + 3*9 = 35   → (10 - 5) % 10 = 5
        assert_eq!(dp_check("9"), '4');
        assert_eq!(dp_check("12"), '8');
        assert_eq!(dp_check("10"), '6');
        assert_eq!(dp_check("23"), '5');

        // Weight-swap discriminator: "10" with weights [9,4] would be
        // 1*9 + 0*4 = 9 → (10-9)%10 = 1 (not '6'). The "10" anchor
        // alone kills `replace 4 with 9`, `replace 9 with 4`, and
        // `replace i % 2 == 0 with !=` mutants.
        //
        // Position-order discriminator: "12" vs "21" must differ
        // because the weight at each position depends on i % 2.
        //   "12" → 1*4 + 2*9 = 22 → '8' (computed above)
        //   "21" → 2*4 + 1*9 = 17 → (10-7)%10 = 3
        // Equal under any mutant that drops the i-dependent weight
        // (e.g. constant 4 or constant 9).
        assert_eq!(dp_check("21"), '3');
        assert_ne!(
            dp_check("12"),
            dp_check("21"),
            "asymmetric digit pair must produce distinct checks; \
             equality means the i-dependent weight selector was \
             replaced by a constant"
        );

        // Final-modulus discriminator: replacing `% 10` with `% 11`
        // changes the "0" result only for sums where (10 - sum%10) ≥ 10
        // — i.e. sum%10 == 0. The wrap anchors above already pin
        // that, but add an explicit check that the result is always
        // a decimal digit (0..=9) across a sweep.
        for c in '0'..='9' {
            let r = dp_check(&c.to_string());
            assert!(
                r.is_ascii_digit(),
                "dp_check({c:?}) returned {r:?}, not a decimal digit"
            );
        }
    }
}
