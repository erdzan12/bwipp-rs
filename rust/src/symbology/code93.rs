//! Code 93.
//!
//! 43-character primary alphabet (digits, A-Z, space, `$%+-./`) plus four
//! shift codes that enable a full-ASCII extension. Each character is a
//! 9-module pattern composed of 3 bars and 3 spaces (alternating, bar
//! first). The symbol layout is:
//!
//!   `*` data... (`C` `K` if includecheck) `*` `1` (terminator bar)
//!
//! Patterns ported from the BWIPP `code93` encoder (verified against the
//! `bwipp_code93` function in bwip-js v4.x).
//!
//! Recognized options:
//!   * `includecheck` = `true` to append the mod-47 `C` and `K` check digits
//!     (default `false`, matching BWIPP).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// 43-character primary alphabet. Index into this string == the character's
/// numeric value used in the check-digit math (0..=42).
const ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";

/// 9-module bit patterns for the 47 Code 93 symbols (43 alphabet + 4 shifts).
///
/// Index in this array == numeric value passed through the encoder.
/// `'1'` is a bar module, `'0'` is a space module. All entries are 9 modules.
const PATTERNS: [&str; 47] = [
    "100010100", // 0
    "101001000", // 1
    "101000100", // 2
    "101000010", // 3
    "100101000", // 4
    "100100100", // 5
    "100100010", // 6
    "101010000", // 7
    "100010010", // 8
    "100001010", // 9
    "110101000", // A
    "110100100", // B
    "110100010", // C
    "110010100", // D
    "110010010", // E
    "110001010", // F
    "101101000", // G
    "101100100", // H
    "101100010", // I
    "100110100", // J
    "100011010", // K
    "101011000", // L
    "101001100", // M
    "101000110", // N
    "100101100", // O
    "100010110", // P
    "110110100", // Q
    "110110010", // R
    "110101100", // S
    "110100110", // T
    "110010110", // U
    "110011010", // V
    "101101100", // W
    "101100110", // X
    "100110110", // Y
    "100111010", // Z
    "100101110", // -
    "111010100", // .
    "111010010", // space
    "111001010", // $
    "101101110", // /
    "101110110", // +
    "110101110", // %
    "100100110", // SFT1 (43)
    "111011010", // SFT2 (44)
    "111010110", // SFT3 (45)
    "100110010", // SFT4 (46)
];

/// 9-module bit pattern for the start/stop sentinel `*`. (Index 47 in the
/// upstream encoding array - it lives outside the data alphabet so we treat
/// it as a separate constant.)
const START_STOP: &str = "101011110";
/// Trailing terminator bar that closes the stop sentinel.
const TERMINATOR_BAR: &str = "1";

/// Numeric value of `c` in the Code 93 alphabet (or `None` if not in it).
fn value(c: char) -> Option<u32> {
    ALPHABET.find(c).map(|i| i as u32)
}

/// Encode a Code 93 payload.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Code93, "CODE93", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Code 93 payload must not be empty".into(),
        ));
    }
    if data.chars().count() > 500 {
        return Err(Error::InvalidData(
            "Code 93 payload exceeds BWIPP's 500-char limit".into(),
        ));
    }
    let payload = data.to_uppercase();
    let mut indices: Vec<u32> = Vec::with_capacity(payload.chars().count());
    for c in payload.chars() {
        match value(c) {
            Some(v) => indices.push(v),
            None => {
                return Err(Error::InvalidData(format!(
                    "Code 93 does not support character {c:?}"
                )))
            }
        }
    }

    let include_check = opts.get("includecheck").is_some_and(|v| v == "true");
    let text = if opts.include_text {
        Some(payload)
    } else {
        None
    };
    encode_indices(&indices, include_check, text)
}

/// Encode a pre-resolved sequence of Code 93 codeword indices (0..=46).
///
/// This is the lower-level entry point used by `code93ext` so the extended
/// encoder can emit the four `SFT` shift codewords (indices 43..=46) — they
/// have no character form in [`ALPHABET`] and so can't be passed through
/// the string-based [`encode`] entry point.
///
/// Indices are validated by the caller; this function trusts them.
pub(crate) fn encode_indices(
    indices: &[u32],
    include_check: bool,
    text: Option<String>,
) -> Result<LinearPattern, Error> {
    let mut full: Vec<u32> = indices.to_vec();
    if include_check {
        full.push(check_c_indices(&full));
        full.push(check_k_indices(&full));
    }

    let mut modules = String::new();
    modules.push_str(START_STOP);
    for &i in &full {
        modules.push_str(PATTERNS[i as usize]);
    }
    modules.push_str(START_STOP);
    modules.push_str(TERMINATOR_BAR);

    Ok(LinearPattern::from_modules(&modules, text))
}

/// First check digit (`C`): weights `1..=20` cycling right-to-left over the
/// codeword stream. Modulus 47.
fn check_c_indices(indices: &[u32]) -> u32 {
    weighted_check_indices(indices, 20)
}

/// Second check digit (`K`): weights `1..=15` cycling over the codeword
/// stream including the just-appended `C` check.
fn check_k_indices(indices_with_c: &[u32]) -> u32 {
    weighted_check_indices(indices_with_c, 15)
}

fn weighted_check_indices(indices: &[u32], max_weight: u32) -> u32 {
    let mut sum: u32 = 0;
    for (i, &v) in indices.iter().rev().enumerate() {
        let weight = (i as u32 % max_weight) + 1;
        sum += v * weight;
    }
    sum % 47
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
    fn known_pattern_zero() {
        // "0" without includecheck: start + '0' + stop + terminator.
        let p = encode("0", &Options::default()).unwrap();
        let expected = format!(
            "{}{}{}{}",
            START_STOP, PATTERNS[0], START_STOP, TERMINATOR_BAR
        );
        assert_eq!(module_string(&p), expected);
    }

    #[test]
    fn known_pattern_payload_with_check_digits() {
        // BWIPP spec example: "TEST93" with includecheck=true.
        // Verified by hand against bwip-js's bwipp_code93:
        //   C(TEST93) = ALPHABET[(T*6 + E*5 + S*4 + T*3 + 9*2 + 3*1) % 47]
        //   K(TEST93C) = ALPHABET[(T*7 + E*6 + S*5 + T*4 + 9*3 + 3*2 + C*1) % 47]
        let p = encode("TEST93", &Options::default().with("includecheck", "true")).unwrap();

        let t = value('T').unwrap();
        let e = value('E').unwrap();
        let s = value('S').unwrap();
        let n9 = value('9').unwrap();
        let n3 = value('3').unwrap();
        let sum_c = t * 6 + e * 5 + s * 4 + t * 3 + n9 * 2 + n3;
        let check_c_val = sum_c % 47;
        let check_c = ALPHABET.chars().nth(check_c_val as usize).unwrap();
        let sum_k = t * 7 + e * 6 + s * 5 + t * 4 + n9 * 3 + n3 * 2 + check_c_val;
        let check_k_val = sum_k % 47;
        let check_k = ALPHABET.chars().nth(check_k_val as usize).unwrap();

        let mut expected = String::new();
        expected.push_str(START_STOP);
        for c in ['T', 'E', 'S', 'T', '9', '3', check_c, check_k] {
            expected.push_str(PATTERNS[value(c).unwrap() as usize]);
        }
        expected.push_str(START_STOP);
        expected.push_str(TERMINATOR_BAR);

        assert_eq!(module_string(&p), expected);
    }

    #[test]
    fn includecheck_default_false() {
        // Default (no includecheck): no check digits in the output.
        // Length should match the "no check digits" branch of BWIPP:
        //   payload_len * 9 + 9 (start) + 9 (stop) + 1 (terminator) = 9N + 19
        let p = encode("ABCDE", &Options::default()).unwrap();
        let expected_modules = 5 * 9 + 9 + 9 + 1;
        assert_eq!(module_string(&p).len(), expected_modules);
    }

    #[test]
    fn includecheck_adds_two_chars() {
        let p = encode("ABCDE", &Options::default().with("includecheck", "true")).unwrap();
        // With two check digits: 7*9 + 9 + 9 + 1 = 82
        let expected_modules = 7 * 9 + 9 + 9 + 1;
        assert_eq!(module_string(&p).len(), expected_modules);
    }

    #[test]
    fn rejects_unknown_character() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 118-120
        // (`Code 93 does not support character {c:?}`). Input "hello!"
        // is uppercased to "HELLO!" — '!' is the first unknown char.
        // Cross-arm guards against the empty + too-long arms.
        match encode("hello!", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 93"), "missing `Code 93` prefix: {msg}");
                assert!(
                    msg.contains("does not support character"),
                    "missing `does not support character` predicate: {msg}"
                );
                assert!(
                    msg.contains("'!'"),
                    "missing '!' char Debug echo (first unknown char in HELLO!): {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("exceeds BWIPP"),
                    "wrong arm — length-cap diagnostic leaked: {msg}"
                );
            }
            other => panic!("\"hello!\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_payload() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 103-105
        // (`Code 93 payload must not be empty`). Cross-arm guards
        // against unknown-character + length-cap arms.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 93"), "missing `Code 93` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("does not support character"),
                    "wrong arm — unknown-character diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("exceeds BWIPP"),
                    "wrong arm — length-cap diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Code 93 payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_too_long_payload() {
        let payload = "A".repeat(501);
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 108-110
        // (`Code 93 payload exceeds BWIPP's 500-char limit`).
        // Cross-arm guards against empty + unknown-character arms.
        match encode(&payload, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 93"), "missing `Code 93` prefix: {msg}");
                assert!(
                    msg.contains("exceeds BWIPP's 500-char limit"),
                    "missing length-cap predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("does not support character"),
                    "wrong arm — unknown-character diagnostic leaked: {msg}"
                );
            }
            other => panic!("501-char Code 93 payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the `> 500` payload-length boundary
    /// (rust/src/symbology/code93.rs line 107). The existing
    /// `rejects_too_long_payload` test only exercises length 501,
    /// which is rejected under both the original `> 500` and the
    /// mutated `>= 500` — so the boundary mutation slips through.
    ///
    /// Anchors:
    ///   - length 499 → accepts (the original accepts, and so does
    ///     the `>= 500` mutant; this anchor just sanity-checks the
    ///     non-boundary case).
    ///   - length 500 → ACCEPTS under original but REJECTS under
    ///     `>= 500` mutant. Bracket-killer for that mutation.
    ///   - length 501 → rejects (covered by existing test; we
    ///     re-anchor here to lock the close-side of the bracket too).
    ///   - Also pin the diagnostic substring "500-char limit" so a
    ///     mutation that swaps the error message survives the
    ///     length-only assertion above.
    #[test]
    fn rejects_unknown_character_and_length_boundary() {
        // Length boundary: 500 accepts, 501 rejects.
        let p500 = "A".repeat(500);
        assert!(
            encode(&p500, &Options::default()).is_ok(),
            "500-char payload should encode (kills `> 500` → `>= 500` mutant)"
        );
        let p499 = "A".repeat(499);
        assert!(
            encode(&p499, &Options::default()).is_ok(),
            "499-char payload should encode"
        );
        // 501 rejects with the length-cap diagnostic from line 109:
        //   "Code 93 payload exceeds BWIPP's 500-char limit"
        // Both "500-char limit" and "BWIPP" are always present in this
        // exact string; the previous `||` accepted either substring.
        // 3-anchor `&&` pin replaces the weak disjunction:
        let p501 = "A".repeat(501);
        match encode(&p501, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 93 payload"),
                    "length-cap diagnostic must carry the Code 93 prefix; got {msg}"
                );
                assert!(
                    msg.contains("BWIPP"),
                    "length-cap diagnostic must attribute the cap to BWIPP; got {msg}"
                );
                assert!(
                    msg.contains("500-char limit"),
                    "length-cap diagnostic must name the 500-char cap; got {msg}"
                );
            }
            other => panic!("501-char payload should reject, got {other:?}"),
        }

        // Unknown character: pin the diagnostic substring so the
        // catch-all mutation that swaps the error message doesn't
        // slip through. "!" is not in the Code 93 alphabet.
        //
        // Stage 11.A8c (cont) — add `Code 93` symbology-prefix
        // anchor matching the source diagnostic at line 119 of
        // code93.rs:
        //   1. `Code 93` symbology name (kills mutations that drop
        //      or substitute the symbology prefix in the format
        //      string)
        //   2. `does not support character` predicate (preserved)
        //   3. `'!'` char Debug echo (preserved)
        match encode("HELLO!", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 93"), "missing `Code 93` prefix: {msg}");
                assert!(
                    msg.contains("does not support character") && msg.contains("'!'"),
                    "expected unknown-character diagnostic with the offending char, got: {msg}"
                );
                // Stage 11.A8c (cont) — cross-arm contamination guards.
                // Code 93 has 3 sibling InvalidData arms (empty / 500-char
                // length cap / unknown-character). A mutation that re-routes
                // unknown-character to one of the other arms would still
                // produce InvalidData but with the WRONG predicate; these
                // ABSENCE checks make such a mutation visible.
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into unknown-char reject: {msg}"
                );
                assert!(
                    !msg.contains("exceeds") && !msg.contains("500-char"),
                    "wrong arm — length-cap diagnostic leaked into unknown-char reject: {msg}"
                );
                // Iteration-position guard: 'H', 'E', 'L', 'O' are valid
                // Code 93 chars; the diagnostic must echo ONLY the '!'
                // sentinel, NOT any of the valid prefix chars (a mutation
                // that reports the FIRST char instead of the OFFENDING
                // char would echo 'H' here).
                assert!(
                    !msg.contains("'H'") && !msg.contains("'E'") && !msg.contains("'L'") && !msg.contains("'O'"),
                    "iteration-direction — diagnostic must echo only the offending '!' (offset 5), NOT the valid prefix 'HELLO': {msg}"
                );
            }
            other => panic!(
                "encode(\"HELLO!\") must reject as Err(InvalidData(unknown-char)) via the catch-all alphabet arm; got {other:?}"
            ),
        }
    }

    #[test]
    fn slash_plus_percent_are_distinct() {
        // Regression: pre-fix, '/' and '+' had identical bit patterns and
        // '%' was wrong. Verify they now match the BWIPP table.
        assert_eq!(PATTERNS[value('/').unwrap() as usize], "101101110");
        assert_eq!(PATTERNS[value('+').unwrap() as usize], "101110110");
        assert_eq!(PATTERNS[value('%').unwrap() as usize], "110101110");
    }

    /// Golden bar pattern for `"CODE93"` captured from bwip-js's
    /// `raw("code93", "CODE93", {})[0].sbs`.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        let p = encode("CODE93", &Options::default()).unwrap();
        let want: [u8; 49] = [
            1, 1, 1, 1, 4, 1, 2, 1, 1, 3, 1, 1, 1, 2, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 1, 2, 1,
            1, 1, 4, 1, 1, 1, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 1, 4, 1, 1,
        ];
        assert_eq!(p.bars, want, "code93 bars mismatch vs bwip-js raw output");
    }

    /// Cross-validation for various Code 93 inputs covering the
    /// full base alphabet: SPACE, punctuation (`$`, `+`, `/`, `.`),
    /// and the `/CODE/93` mixed pattern.
    #[test]
    fn matches_bwip_js_various_inputs() {
        let cases: &[(&str, &[u8])] = &[
            (
                "HELLO WORLD",
                &[
                    1, 1, 1, 1, 4, 1, 1, 1, 2, 2, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1,
                    1, 1, 2, 3, 1, 2, 1, 1, 2, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 2, 2, 1, 2, 1, 1,
                    2, 2, 2, 1, 2, 2, 1, 1, 1, 1, 1, 1, 2, 3, 2, 2, 1, 1, 1, 2, 1, 1, 1, 1, 4, 1,
                    1,
                ],
            ),
            (
                "TEST 99",
                &[
                    1, 1, 1, 1, 4, 1, 2, 1, 1, 2, 2, 1, 2, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 2, 1,
                    1, 2, 2, 1, 3, 1, 1, 2, 1, 1, 1, 4, 1, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 1, 1, 1,
                    4, 1, 1,
                ],
            ),
            (
                "$10.95+",
                &[
                    1, 1, 1, 1, 4, 1, 3, 2, 1, 1, 1, 1, 1, 1, 1, 2, 1, 3, 1, 3, 1, 1, 1, 2, 3, 1,
                    1, 1, 1, 2, 1, 4, 1, 1, 1, 1, 1, 2, 1, 2, 1, 2, 1, 1, 3, 1, 2, 1, 1, 1, 1, 1,
                    4, 1, 1,
                ],
            ),
            (
                "/CODE/93",
                &[
                    1, 1, 1, 1, 4, 1, 1, 1, 2, 1, 3, 1, 2, 1, 1, 3, 1, 1, 1, 2, 1, 1, 2, 2, 2, 2,
                    1, 1, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 2, 1, 3, 1, 1, 4, 1, 1, 1, 1, 1, 1, 1, 4,
                    1, 1, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (Code 93 sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(got.bars, want, "code93 sbs mismatch for {text:?}");
        }
    }

    /// Cross-validation for the C+K check-digit path
    /// (`b.raw("code93", text, {includecheck: true})[0].sbs`). The
    /// two check digits sit between the last data char and the stop
    /// sentinel, so a wrong weighted-sum computation would change
    /// exactly two 6-element chunks vs. the no-check golden above.
    #[test]
    fn matches_bwip_js_with_check() {
        let cases: &[(&str, &[u8])] = &[
            (
                "CODE93",
                &[
                    1, 1, 1, 1, 4, 1, 2, 1, 1, 3, 1, 1, 1, 2, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2,
                    1, 2, 1, 1, 1, 4, 1, 1, 1, 1, 1, 1, 1, 4, 1, 1, 1, 3, 1, 1, 2, 1, 2, 2, 2, 1,
                    1, 1, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
            (
                "ABC 123",
                &[
                    1, 1, 1, 1, 4, 1, 2, 1, 1, 1, 1, 3, 2, 1, 1, 2, 1, 2, 2, 1, 1, 3, 1, 1, 3, 1,
                    1, 2, 1, 1, 1, 1, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 1, 1, 1, 4, 1, 1, 2, 1, 1, 2,
                    2, 1, 1, 1, 3, 1, 2, 1, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
            (
                "TEST",
                &[
                    1, 1, 1, 1, 4, 1, 2, 1, 1, 2, 2, 1, 2, 2, 1, 2, 1, 1, 2, 1, 1, 1, 2, 2, 2, 1,
                    1, 2, 2, 1, 1, 3, 1, 2, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
        ];
        let opts = Options::default().with("includecheck", "true");
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo.
            let got = encode(text, &opts).unwrap_or_else(|e| {
                panic!("encode({text:?}, includecheck=true) (Code 93 C+K check-digit corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars, want,
                "code93 (with check) sbs mismatch for {text:?}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Stage 11.A8c — pin `weighted_check_indices` past its wrap
    /// boundary directly. Existing tests use ≤7-char payloads, so the
    /// `(i as u32 % max_weight) + 1` wrap at line ~179 is never
    /// engaged (positions 0..=14 stay strictly below both max_weight
    /// values 15 and 20). The mutant `i % max_weight` → `i / max_weight`
    /// silently agrees on short inputs (both produce 0 for i<max_weight),
    /// but diverges for long inputs.
    ///
    /// Hand-computed for all-'A' (codeword = 10) inputs:
    ///
    ///   max_weight = 20:
    ///     - 20 A's: sum = 10*(1+2+...+20) = 2100. 2100 % 47 = 32.
    ///     - 21 A's: position 20 wraps to weight 1.
    ///                sum = 2100 + 10*1 = 2110. 2110 % 47 = 42.
    ///
    ///   max_weight = 15:
    ///     - 15 A's: sum = 10*(1+...+15) = 1200. 1200 % 47 = 25.
    ///     - 16 A's: position 15 wraps to weight 1.
    ///                sum = 1200 + 10 = 1210. 1210 % 47 = 35.
    ///
    /// The `% max_weight` → `/ max_weight` mutant at position 15
    /// would set weight = (15/15) + 1 = 2 (instead of 1), producing
    /// sum = 1200 + 20 = 1220 → 1220 % 47 = 45 (not 35).
    #[test]
    fn weighted_check_indices_wraps_at_max_weight() {
        let twenty_a = [10u32; 20];
        let twenty_one_a = [10u32; 21];
        assert_eq!(
            weighted_check_indices(&twenty_a, 20),
            32,
            "20 A's, max=20: no wrap → sum=2100, 2100 %47 = 32"
        );
        assert_eq!(
            weighted_check_indices(&twenty_one_a, 20),
            42,
            "21 A's, max=20: position 20 wraps → sum=2110, 2110 %47 = 42"
        );

        let fifteen_a = [10u32; 15];
        let sixteen_a = [10u32; 16];
        assert_eq!(
            weighted_check_indices(&fifteen_a, 15),
            25,
            "15 A's, max=15: no wrap → sum=1200, 1200 %47 = 25"
        );
        assert_eq!(
            weighted_check_indices(&sixteen_a, 15),
            35,
            "16 A's, max=15: position 15 wraps → sum=1210, 1210 %47 = 35"
        );

        // Empty input → sum=0 → check=0.
        assert_eq!(weighted_check_indices(&[], 20), 0);
        // Single-codeword input → no chance to wrap.
        assert_eq!(weighted_check_indices(&[5], 20), 5);
    }

    /// Stage 11.A8c — pin `check_c_indices` and `check_k_indices`
    /// using the constants 20 and 15 respectively (not each other,
    /// not some other value).
    ///
    /// For `indices = [1; 20]`:
    ///   * check_c uses max_weight=20 → weights 1..=20 → sum = 210 →
    ///     210 % 47 = 22.
    ///   * check_k uses max_weight=15 → weights cycle as
    ///     1,2,…,15,1,2,3,4,5 → sum = 120 + 15 = 135 → 135 % 47 = 41.
    ///
    /// Mutations caught:
    ///   * `check_c_indices` calling `weighted_check_indices(_, 15)`
    ///     instead of 20 — would return 41 not 22.
    ///   * `check_k_indices` calling 20 instead of 15 — would return
    ///     22 not 41.
    ///   * Either wrapper using a different constant (10, 30, etc.)
    ///     produces a third value that fails both expected values.
    #[test]
    fn check_c_uses_20_and_check_k_uses_15_weights() {
        let twenty_ones = [1u32; 20];
        assert_eq!(
            check_c_indices(&twenty_ones),
            22,
            "check_c must use max_weight=20: sum(1..=20)=210, 210%47=22"
        );
        assert_eq!(
            check_k_indices(&twenty_ones),
            41,
            "check_k must use max_weight=15: \
             cycle weights → sum 135, 135%47=41"
        );
        // Sanity: the two wrappers MUST disagree on this input.
        assert_ne!(
            check_c_indices(&twenty_ones),
            check_k_indices(&twenty_ones),
            "check_c and check_k must not collapse onto the same constant"
        );
    }

    /// Stage 11.A8c — pin `encode_indices` include_check branch +
    /// fixed prefix/suffix structure.
    ///
    /// Structural invariants:
    ///   * Output always begins with START_STOP (9 modules) and ends
    ///     with `START_STOP + TERMINATOR_BAR` (9 + 1 = 10 modules).
    ///   * include_check=true appends 2 codewords (C and K).
    ///   * include_check=false leaves the index stream unchanged.
    ///   * text propagates as the LinearPattern.text.
    ///
    /// Mutations caught:
    ///   * `if include_check` branch flip → 2 cws missing/extra.
    ///   * Drop START_STOP or TERMINATOR_BAR → width changes.
    ///   * `text` swapped to None → propagation breaks.
    #[test]
    fn encode_indices_structure_and_check_branch() {
        // Empty + no check: just sentinels.
        let p = encode_indices(&[], false, None).unwrap();
        assert_eq!(p.total_width(), 19, "2 START_STOP (9) + TERM (1) = 19");
        assert_eq!(p.text, None);

        // Single index + no check: 3 patterns × 9 + 1 = 28.
        let p = encode_indices(&[0], false, Some("M".into())).unwrap();
        assert_eq!(p.total_width(), 28);
        assert_eq!(p.text.as_deref(), Some("M"));

        // Same index WITH check: adds 2 codewords → +18 modules.
        let p_check = encode_indices(&[0], true, None).unwrap();
        assert_eq!(
            p_check.total_width(),
            28 + 18,
            "include_check appends C + K check codewords (2 × 9)"
        );

        // Multiple indices, no check: 5 patterns × 9 + 1 = 46.
        let p3 = encode_indices(&[1, 2, 3], false, None).unwrap();
        assert_eq!(p3.total_width(), 46);
    }

    /// Kills `encode: replace > with >=` at line ~107 (the BWIPP
    /// 500-char payload-length cap). Existing tests used short
    /// payloads; the mutant `>= 500` rejects exactly-500-char inputs
    /// (which the spec accepts). We bracket the boundary at 499, 500,
    /// and 501 to lock the cap in.
    #[test]
    fn payload_length_cap_is_strictly_five_hundred() {
        let four_99: String = "A".repeat(499);
        let five_00: String = "A".repeat(500);
        let five_01: String = "A".repeat(501);
        // Stage 11.A8c (cont) — descriptive labels naming Code 93 500-cap
        // boundary direction (brackets the `<= 500` ↔ `< 500` mutation pair).
        assert!(
            encode(&four_99, &Options::default()).is_ok(),
            "encode(499-char Code 93) must accept (one below 500-cap; kills `< 500` → `<= 499` boundary mutant)"
        );
        // Exactly 500 must succeed.
        assert!(
            encode(&five_00, &Options::default()).is_ok(),
            "encode(500-char Code 93) must accept (exactly at 500-cap; kills `<= 500` → `< 500` boundary mutant)"
        );
        // 501 must be rejected. Stage 11.A8c — upgrade discriminant-only
        // `matches!` to a 2-anchor pin so the boundary-cap test
        // co-asserts the diagnostic content (same anchors as
        // `rejects_too_long_payload`), with cross-arm guard against
        // the unknown-character arm.
        match encode(&five_01, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 93"),
                    "501-cap diagnostic missing `Code 93` prefix: {msg}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-char limit"),
                    "501-cap diagnostic missing length-cap predicate: {msg}"
                );
                assert!(
                    !msg.contains("does not support character"),
                    "501-cap arm leaked unknown-character diagnostic: {msg}"
                );
            }
            other => panic!("501-char Code 93 payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `value(c)` direct lookup into ALPHABET =
    /// `"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%"`. The helper is
    /// only exercised transitively through `encode` (which calls
    /// `value(c)` per input char) and the existing
    /// `slash_plus_percent_are_distinct` test (which only pins values
    /// 40, 41, 42 indirectly via PATTERNS). Boundary and rejection
    /// arithmetic survives in mutations on the `find(c).map(|i| i as
    /// u32)` body when end-to-end goldens don't probe the affected
    /// codeword.
    ///
    /// Hand-computed:
    ///   * digits '0'..='9' → 0..=9
    ///   * uppercase 'A'..='Z' → 10..=35
    ///   * '-' → 36, '.' → 37, ' ' → 38, '$' → 39
    ///   * '/' → 40, '+' → 41, '%' → 42
    ///   * non-alphabet chars (lowercase, '*', '!', high bytes) → None
    ///
    /// Mutations to catch:
    ///   * Body replaced with `Some(0)`: every char returns 0; pins
    ///     by checking value('A') = 10.
    ///   * `find(c).map(|i| i as u32)` → `Some(i as u32 + 1)`:
    ///     shifts everything by 1; value('0') would be 1 not 0.
    ///   * `find(c).map(...)` → `find('0').map(...)`: always returns
    ///     Some(0) regardless of input; value('A') = 0 ≠ 10.
    ///   * ALPHABET truncation (drop the punctuation tail): value('+')
    ///     would return None instead of Some(41).
    ///   * ALPHABET re-ordering (swap two adjacent chars): boundary
    ///     anchors at digit→alpha (9 vs A) and alpha→punct (Z vs -)
    ///     would diverge.
    #[test]
    fn value_full_alphabet_lookup() {
        // Digit boundaries.
        assert_eq!(value('0'), Some(0), "digit boundary low");
        assert_eq!(value('9'), Some(9), "digit boundary high");
        assert_eq!(value('5'), Some(5), "digit middle");

        // Uppercase boundaries.
        assert_eq!(value('A'), Some(10), "alpha boundary low (after '9')");
        assert_eq!(value('Z'), Some(35), "alpha boundary high");
        assert_eq!(value('M'), Some(22), "alpha middle");

        // Punctuation in alphabet order:
        //   '-' .  ' '  '$'  '/'  '+'  '%'
        //    36 37  38  39    40   41   42
        assert_eq!(value('-'), Some(36), "after 'Z'");
        assert_eq!(value('.'), Some(37));
        assert_eq!(value(' '), Some(38));
        assert_eq!(value('$'), Some(39));
        assert_eq!(value('/'), Some(40));
        assert_eq!(value('+'), Some(41));
        assert_eq!(value('%'), Some(42), "last alphabet entry");

        // Non-alphabet rejection.
        assert_eq!(value('*'), None, "'*' is start/stop, not in data alphabet");
        assert_eq!(value('a'), None, "lowercase");
        assert_eq!(value('z'), None, "lowercase z");
        assert_eq!(value('!'), None);
        assert_eq!(value('@'), None, "just below 'A' in ASCII");
        assert_eq!(value('['), None, "just above 'Z' in ASCII");
        assert_eq!(value(':'), None, "just above '9' in ASCII");
        assert_eq!(value(' '), Some(38), "regression: space IS in alphabet");

        // Non-ASCII rejection.
        assert_eq!(value('é'), None, "non-ASCII char");
        assert_eq!(value('\u{0}'), None, "NUL");
        assert_eq!(value('\u{7F}'), None, "DEL");

        // Distinctness invariant: every alphabet member returns a
        // unique 0..=42 value.
        let alphabet_chars: Vec<char> = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%"
            .chars()
            .collect();
        assert_eq!(alphabet_chars.len(), 43, "alphabet has exactly 43 chars");
        for (i, &c) in alphabet_chars.iter().enumerate() {
            assert_eq!(
                value(c),
                Some(i as u32),
                "char {c:?} at position {i} must map to value {i}"
            );
        }
    }

    /// Stage 11.A8c — pin the `max_weight` constant baked into each
    /// of `check_c_indices` (= 20) and `check_k_indices` (= 15).
    /// Without a discriminator-anchor test, a mutant that swaps the
    /// two constants (check_c=15, check_k=20) survives on every
    /// payload short enough that both windows stay in their linear
    /// regime — i.e. lengths < 15.
    ///
    /// The 16-element input [1, 2, …, 16] crosses the 15-wrap
    /// boundary used by check_k but stays below the 20-wrap used by
    /// check_c, producing distinct results:
    ///
    ///   check_c (max=20): every position uses weight = i+1
    ///   for i in 0..=15, sum = Σ (16-i) * (i+1) = 816, 816 % 47 = 17.
    ///
    ///   check_k (max=15): position i=15 wraps to weight = (15%15)+1 = 1
    ///   instead of 16, sum = 816 - (1*16) + (1*1) = 801, 801 % 47 = 2.
    ///
    /// A swap (check_c=15, check_k=20) yields the same values flipped
    /// (check_c=2, check_k=17), so the asserts catch it. A combined
    /// mutation that drops both constants to a common value (e.g. both
    /// = 15 or both = 20) makes the two functions agree and is caught
    /// by the distinctness assertion at the bottom.
    #[test]
    fn check_c_and_k_indices_use_correct_max_weights_20_and_15() {
        // The discriminator input — 16 distinct values that span the
        // 15-wrap boundary.
        let input: Vec<u32> = (1..=16).collect();

        // Pin both functions directly with hand-computed expected values.
        assert_eq!(
            check_c_indices(&input),
            17,
            "check_c_indices uses max_weight=20: sum=816, 816 % 47 = 17"
        );
        assert_eq!(
            check_k_indices(&input),
            2,
            "check_k_indices uses max_weight=15: wrap at i=15 → sum=801, 801 % 47 = 2"
        );

        // Distinctness invariant: the two helpers MUST produce
        // different results on the discriminator input. If they
        // agree on this input, both max_weight constants have
        // collapsed to the same value (catches any mutation that
        // mass-replaces 20 ↔ 15 ↔ any other constant).
        assert_ne!(
            check_c_indices(&input),
            check_k_indices(&input),
            "check_c (max=20) and check_k (max=15) MUST diverge on a \
             16-element input that crosses the 15-wrap boundary"
        );

        // Length-1 sanity: both functions reduce to v % 47.
        assert_eq!(
            check_c_indices(&[5]),
            5,
            "check_c([5]): 5 * 1 = 5, 5 % 47 = 5"
        );
        assert_eq!(
            check_k_indices(&[5]),
            5,
            "check_k([5]): 5 * 1 = 5, 5 % 47 = 5"
        );
        // Empty input → both return 0.
        assert_eq!(check_c_indices(&[]), 0);
        assert_eq!(check_k_indices(&[]), 0);

        // Short-input agreement: on payloads of length < 15, both
        // functions agree (the wrap boundary isn't crossed). This
        // pins that the wrap is the SOLE difference between them —
        // a mutant that swaps the actual weighting formula
        // (e.g. `i % max + 1` → `i + 1`) would survive otherwise.
        let short: Vec<u32> = (1..=14).collect();
        assert_eq!(
            check_c_indices(&short),
            check_k_indices(&short),
            "for length 14 (< both wrap boundaries), check_c and check_k \
             MUST agree because no position hits its wrap"
        );
    }
}
