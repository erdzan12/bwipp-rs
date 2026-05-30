//! Interleaved 2 of 5 (ITF) and ITF-14.
//!
//! Each pair of digits is encoded together: the first digit's pattern as five
//! bars, the second as five spaces, interleaved. Each "digit pattern" has
//! exactly two wide and three narrow elements (2-of-5). Symbol starts with
//! "1010" and ends with "11101".

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// `0..=9`: which of the 5 positions are wide. (BWIPP `i2of5` patterns.)
///
/// `1` is wide, `0` is narrow. Position 0 is the first bar/space in the pair.
const DIGIT_PATTERNS: &[&str] = &[
    "00110", // 0
    "10001", // 1
    "01001", // 2
    "11000", // 3
    "00101", // 4
    "10100", // 5
    "01100", // 6
    "00011", // 7
    "10010", // 8
    "01010", // 9
];

const START: &str = "1010";
/// Stop pattern: wide bar + narrow space + narrow bar + narrow space.
/// With BWIPP's 1:2 default this is `b2 s1 b1 s1` = `"11010"`. The
/// trailing space matches BWIPP's `interleaved2of5_encs[11] = "2111"`
/// so the final sbs ends with a quiet-zone-ish space module rather
/// than a bar — same byte-for-byte convention BWIPP uses.
const STOP: &str = "11010";

/// Encode an Interleaved 2 of 5 payload.
///
/// Recognized options:
/// * `includecheck = "true"` — append the GS1 mod-10 weighted check
///   digit (× 3, × 1 alternating from the right). BWIPP defaults
///   `includecheck` to `false`.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Interleaved2of5, "1234567890", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let mut digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != data.chars().count() {
        return Err(Error::InvalidData(
            "Interleaved 2 of 5 accepts digits only".into(),
        ));
    }
    if digits.is_empty() {
        return Err(Error::InvalidData(
            "Interleaved 2 of 5 needs at least 2 digits".into(),
        ));
    }
    if opts.get("includecheck").is_some_and(|v| v == "true") {
        digits.push(gs1_check(&digits));
    }
    // Auto-pad to even length with a leading zero (BWIPP behavior).
    if digits.len() % 2 == 1 {
        digits.insert(0, '0');
    }

    let mut modules = String::new();
    modules.push_str(START);
    let chars: Vec<char> = digits.chars().collect();
    for pair in chars.chunks(2) {
        let d1 = pair[0].to_digit(10).unwrap() as usize;
        let d2 = pair[1].to_digit(10).unwrap() as usize;
        let bars = DIGIT_PATTERNS[d1];
        let spaces = DIGIT_PATTERNS[d2];
        for (b, s) in bars.chars().zip(spaces.chars()) {
            // BWIPP defaults to a 1:2 narrow:wide ratio (its `bwipp_
            // interleaved2of5` source hard-codes `nwidth=1` / `wwidth
            // =2`). The historical 1:3 ratio is configurable through
            // BWIPP's `ratio` option but isn't surfaced through our
            // dispatch yet — we follow BWIPP's default for byte-for-
            // byte cross-validation.
            let bar_width = if b == '1' { 2 } else { 1 };
            let space_width = if s == '1' { 2 } else { 1 };
            for _ in 0..bar_width {
                modules.push('1');
            }
            for _ in 0..space_width {
                modules.push('0');
            }
        }
    }
    modules.push_str(STOP);

    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
}

/// Encode an ITF-14: exactly 13 data digits (a 14th check digit is computed
/// when missing).
pub fn encode_itf14(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    let len = digits.len();
    if len != 13 && len != 14 {
        return Err(Error::InvalidData(
            "ITF-14 must be 13 digits (check computed) or 14 digits".into(),
        ));
    }
    let final_digits = if len == 13 {
        format!("{digits}{}", gs1_check(&digits))
    } else {
        digits
    };
    encode(&final_digits, opts)
}

fn gs1_check(digits: &str) -> char {
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let n = c.to_digit(10).unwrap();
        sum += if i % 2 == 0 { n * 3 } else { n };
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_odd_length() {
        // Stage 11.A8c (cont) — descriptive label naming input + path.
        // ITF requires even payload length; "12345" is odd (5 digits),
        // so the encoder must auto-pad with a leading "0" (→ "012345").
        let p = encode("12345", &Options::default()).expect(
            "encode(\"12345\", default) (Interleaved 2 of 5 odd-length auto-pad path: 5-digit payload → leading-0 pad → 6-digit \"012345\") must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode(\"12345\") (odd-length payload, auto-padded to \"012345\") must compose into non-empty Interleaved 2 of 5 symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn itf14_computes_check_digit() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ITF-14 auto-check path: 13-digit body → 14-char with
        // computed mod-10 check digit.
        let p = encode_itf14("1234567890123", &Options::default()).expect(
            "encode_itf14(\"1234567890123\", default) (ITF-14 13-digit body → 14-char via auto-computed mod-10 check) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_itf14(\"1234567890123\") (13-digit body → 14-char with computed check) must compose into non-empty ITF-14 symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn itf14_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 112-114 of
        // interleaved2of5.rs:
        //   1. `ITF-14` symbology prefix (distinct from the
        //      `Interleaved 2 of 5` sibling)
        //   2. `must be 13 digits` predicate (pins the 13-digit
        //      valid-length spec)
        //   3. `14 digits` complementary length spec (the OR-arm of
        //      the length predicate — kills a mutation that drops
        //      either `13` or `14` from the diagnostic)
        match encode_itf14("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ITF-14"), "missing `ITF-14` prefix: {msg}");
                assert!(
                    msg.contains("must be 13 digits"),
                    "missing `must be 13 digits` predicate: {msg}"
                );
                assert!(
                    msg.contains("14 digits"),
                    "missing `14 digits` complementary length spec: {msg}"
                );
                assert!(
                    !msg.contains("Interleaved 2 of 5"),
                    "wrong helper — Interleaved 2 of 5 diagnostic leaked into ITF-14 reject: {msg}"
                );
            }
            other => panic!("3-digit ITF-14 should reject as InvalidData, got {other:?}"),
        }
    }

    /// Interleaved 2 of 5 golden from
    /// `raw("interleaved2of5", "12345678", {})[0].sbs`.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Interleaved 2 of 5 byte-for-byte 48-bar SBS oracle path:
        // 8-digit "12345678" (even, no auto-pad) → bwip-js raw SBS.
        let p = encode("12345678", &Options::default()).expect(
            "encode(\"12345678\", default) (Interleaved 2 of 5 byte-for-byte 48-bar SBS bwip-js raw oracle; 8-digit even-length no-pad) must succeed",
        );
        let want: [u8; 48] = [
            1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 2, 2,
            2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 2, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "interleaved2of5 bars mismatch vs bwip-js raw output"
        );
    }

    /// Additional cross-validation: odd-length payload (auto-padded
    /// with a leading "0" per BWIPP) and the `includecheck: true`
    /// path which appends a mod-10 weighted check digit.
    #[test]
    fn matches_bwip_js_more_payloads() {
        let cases: &[(&str, &str, &[u8])] = &[
            (
                "12345",
                "",
                &[
                    1, 1, 1, 1, 1, 2, 1, 1, 2, 1, 2, 1, 1, 2, 1, 2, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2,
                    1, 1, 2, 2, 1, 1, 2, 1, 2, 1, 1, 1,
                ],
            ),
            (
                "12345678",
                "true",
                &[
                    1, 1, 1, 1, 1, 2, 1, 1, 2, 1, 2, 1, 1, 2, 1, 2, 2, 2, 1, 1, 1, 1, 2, 1, 1, 2,
                    1, 1, 2, 2, 1, 1, 2, 1, 1, 1, 2, 1, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 2, 2, 1,
                    1, 2, 2, 1, 1, 1,
                ],
            ),
        ];
        for &(text, check, want) in cases {
            let opts = if check.is_empty() {
                Options::default()
            } else {
                Options::default().with("includecheck", check)
            };
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the corpus row (input +
            // includecheck flag) so a regression points at which case
            // diverged.
            let got = encode(text, &opts).unwrap_or_else(|e| {
                panic!(
                    "encode({text:?}, includecheck={check:?}) (Interleaved 2 of 5 odd-length-pad / includecheck-true corpus row) must succeed: {e:?}",
                )
            });
            assert_eq!(
                got.bars, want,
                "interleaved2of5 sbs mismatch for {text:?} check={check}"
            );
        }
    }

    /// Stage 11.A8c — pin `gs1_check` (the private mod-10 weighted-sum
    /// helper) on a hand-computed corpus. Kills several mutations on
    /// lines 124-131:
    ///
    /// * `chars().rev() -> chars()` — reverses the weight assignment.
    /// * `i % 2 == 0 -> != 0` — swaps the `*3` / `*1` weighting.
    /// * `n * 3 -> n + 3` / `n / 3` / `n - 3` — wrong weight arithmetic.
    /// * `(10 - sum % 10) % 10` — outer `% 10` fold mutants (drop the
    ///   outer % 10 ⇒ panics with `from_digit(10, 10).unwrap()` when
    ///   sum is a multiple of 10).
    ///
    /// Hand-computed:
    ///
    /// * "0" → rev="0", i=0 (n=0, 0*3=0), sum=0, (10-0)%10=0 → '0'.
    /// * "5" → rev="5", i=0 (5*3=15), sum=15, (10-5)%10=5 → '5'.
    /// * "10" → rev="01": i=0 (0*3=0), i=1 (1*1=1), sum=1, (10-1)%10=9 → '9'.
    /// * "55" → rev="55": 5*3+5*1=20, (10-0)%10=0 → '0'. (Wrap-around!)
    /// * "73" → rev="37": 3*3+7*1=16, (10-6)%10=4 → '4'.
    /// * "12345" → rev="54321":
    ///     i=0: 5*3=15
    ///     i=1: 4*1=4
    ///     i=2: 3*3=9
    ///     i=3: 2*1=2
    ///     i=4: 1*3=3
    ///     sum = 33, (10-3)%10 = 7 → '7'.
    /// * "1234567890123" → rev="3210987654321":
    ///     odd-indexed (i=0,2,4,...): 3,1,9,7,5,3,1 → ×3 = 9,3,27,21,15,9,3 = 87
    ///     even-indexed (i=1,3,5,...): 2,0,8,6,4,2 → ×1 = 22
    ///     sum = 109, (10-9)%10 = 1 → '1'.
    #[test]
    fn gs1_check_known_values_with_wraparound() {
        assert_eq!(gs1_check("0"), '0');
        assert_eq!(gs1_check("5"), '5');
        assert_eq!(gs1_check("10"), '9');
        // Wrap-around: sum=20 → (10 - 0) % 10 = 0 (NOT '\u{0a}').
        assert_eq!(
            gs1_check("55"),
            '0',
            "sum=20, (10 - 0) %10 must fold to 0 — outer % 10 mutant"
        );
        assert_eq!(gs1_check("73"), '4');
        assert_eq!(gs1_check("12345"), '7');
        assert_eq!(gs1_check("1234567890123"), '1');
        // Another wrap-around test for variety: "999" → rev="999",
        // i=0:9*3=27, i=1:9*1=9, i=2:9*3=27, sum=63, (10-3)%10=7 → '7'.
        assert_eq!(gs1_check("999"), '7');
        // "37" → rev="73": 7*3+3*1=24, (10-4)%10=6 → '6'.
        assert_eq!(gs1_check("37"), '6');
    }

    /// ITF-14 golden from `raw("itf14", "1234567890123", {})[0].sbs`.
    #[test]
    fn itf14_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ITF-14 byte-for-byte 78-bar SBS oracle path: 13-digit
        // body auto-check → bwip-js raw SBS.
        let p = encode_itf14("1234567890123", &Options::default()).expect(
            "encode_itf14(\"1234567890123\", default) (ITF-14 byte-for-byte 78-bar SBS bwip-js raw oracle; 13-digit body auto-check) must succeed",
        );
        let want: [u8; 78] = [
            1, 1, 1, 1, 2, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 2, 2,
            2, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 2, 1, 1, 2, 2, 2, 1, 1, 2, 1, 1, 2,
            1, 1, 1, 1, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 2, 2, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "itf14 bars mismatch vs bwip-js raw output");
    }
}
