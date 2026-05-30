//! Flattermarken.
//!
//! A printer's mark used to verify page ordering in folded brochures. Each
//! input digit selects one of 10 4-element run-length patterns; BWIPP's
//! alphabet is `"1234567890"`, so `'1'` indexes pattern 0, `'2'` → 1, …,
//! `'9'` → 8, and `'0'` → 9 (the last pattern). The symbol has no
//! start/stop sentinel.
//!
//! Patterns ported from bwip-js `bwipp_flattermarken`.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// 10 per-position run-length patterns, indexed by the input digit's
/// position in BWIPP's `"1234567890"` alphabet (so `'1'` → 0, `'0'`
/// → 9). Each is 4 modules: `[0, d, 1, 9-d-1]`.
const PATTERNS: [&str; 10] = [
    "0018", "0117", "0216", "0315", "0414", "0513", "0612", "0711", "0810", "0900",
];

/// BWIPP's alphabet ordering: `'1'..='9'` then `'0'`.
const BARCHARS: &str = "1234567890";

/// Encode a Flattermarken payload.
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Flattermarken payload must not be empty".into(),
        ));
    }
    if !data.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidData(
            "Flattermarken accepts digits only".into(),
        ));
    }
    if data.chars().count() > 500 {
        return Err(Error::InvalidData(
            "Flattermarken payload exceeds 500 chars".into(),
        ));
    }

    let mut runs: Vec<u8> = Vec::new();
    for c in data.chars() {
        let idx = BARCHARS.find(c).unwrap();
        for ch in PATTERNS[idx].chars() {
            runs.push(ch.to_digit(10).unwrap() as u8);
        }
    }

    let text = if opts.include_text {
        Some(data.to_string())
    } else {
        None
    };
    Ok(LinearPattern { bars: runs, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_letters() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 33-35
        // (`Flattermarken accepts digits only`). Cross-arm guards
        // against the empty-payload + length-cap arms.
        match encode("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Flattermarken"),
                    "missing `Flattermarken` prefix: {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "missing `accepts digits only` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("exceeds 500 chars"),
                    "wrong arm — length-cap diagnostic leaked: {msg}"
                );
            }
            other => panic!("\"ABC\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c — upgrade to 3-anchor pin matching the source
        // diagnostic at line 28-30 (`Flattermarken payload must not
        // be empty`). Cross-arm guards against the other two arms.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Flattermarken"),
                    "missing `Flattermarken` prefix: {msg}"
                );
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("accepts digits only"),
                    "wrong arm — non-digit diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("exceeds 500 chars"),
                    "wrong arm — length-cap diagnostic leaked: {msg}"
                );
            }
            other => {
                panic!("empty Flattermarken payload should reject as InvalidData, got {other:?}")
            }
        }
    }

    #[test]
    fn encodes_digit_zero() {
        // '0' is index 9 in BWIPP's "1234567890" → pattern[9] = "0900"
        // → runs [0, 9, 0, 0].
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken digit-0 path: BARCHARS[9] → PATTERNS[9]
        // = "0900" → runs [0,9,0,0].
        let p = encode("0", &Options::default()).expect(
            "encode(\"0\", default) (Flattermarken digit-0 path: BARCHARS[9] → PATTERNS[9] \"0900\" → runs [0,9,0,0]) must succeed",
        );
        assert_eq!(p.bars, vec![0, 9, 0, 0]);
    }

    #[test]
    fn encodes_concatenated_digits() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken concatenation path: "01" → PATTERNS[9]
        // ("0900") + PATTERNS[0] ("0018") → 8-run vector.
        let p = encode("01", &Options::default()).expect(
            "encode(\"01\", default) (Flattermarken 2-digit concatenation; PATTERNS[9]+PATTERNS[0] → 8 runs) must succeed",
        );
        // '0' → pattern[9] = "0900"; '1' → pattern[0] = "0018".
        assert_eq!(p.bars, vec![0, 9, 0, 0, 0, 0, 1, 8]);
    }

    /// Flattermarken golden from `raw("flattermarken", "1234567", {})[0].sbs`.
    /// Each digit `d` maps to a 4-element run-length pattern; concatenated
    /// for the whole payload.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken byte-for-byte 28-run SBS oracle path:
        // 7-digit "1234567" → each digit's 4-run pattern concatenated.
        let p = encode("1234567", &Options::default()).expect(
            "encode(\"1234567\", default) (Flattermarken byte-for-byte 28-run SBS bwip-js raw oracle; 7 digits × 4-run patterns) must succeed",
        );
        let want: [u8; 28] = [
            0, 0, 1, 8, 0, 1, 1, 7, 0, 2, 1, 6, 0, 3, 1, 5, 0, 4, 1, 4, 0, 5, 1, 3, 0, 6, 1, 2,
        ];
        assert_eq!(
            p.bars, want,
            "flattermarken bars mismatch vs bwip-js raw output"
        );
    }

    /// Stage 11.A8c — pin every digit in PATTERNS directly. The
    /// existing tests cover digits '0', '1', '2', '3', '4', '5',
    /// '6', '7' (via the "0", "01", "1234567" goldens) but leave
    /// digits '8' and '9' (PATTERNS indices 7 and 8) directly
    /// untested. The `matches_bwip_js_raw_sbs` golden only goes up
    /// through "1234567" — so a mutant like `PATTERNS[7] = "0810"
    /// -> "0710"` or `PATTERNS[8] = "0810" -> "0710"` would survive.
    ///
    /// Hand-computed expansions per the BWIPP `[0, d, 1, 9-d-1]` rule:
    ///   * '8' is BARCHARS[7] → PATTERNS[7] = "0711" → runs [0,7,1,1].
    ///   * '9' is BARCHARS[8] → PATTERNS[8] = "0810" → runs [0,8,1,0].
    ///   * '0' is BARCHARS[9] → PATTERNS[9] = "0900" → runs [0,9,0,0].
    ///
    /// Also verifies the include_text branch on/off (the existing
    /// tests don't toggle `include_text`).
    #[test]
    fn all_ten_patterns_and_include_text() {
        // Digit 8: indices 7 in PATTERNS, pattern "0711".
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken digit-8 and digit-9 paths: PATTERNS[7]
        // "0711" / PATTERNS[8] "0810" — guards against index-
        // permutation mutants in PATTERNS[7..=8].
        let p8 = encode("8", &Options::default()).expect(
            "encode(\"8\", default) (Flattermarken digit-8 path: BARCHARS[7] → PATTERNS[7] \"0711\" → runs [0,7,1,1]) must succeed",
        );
        assert_eq!(p8.bars, vec![0, 7, 1, 1], "digit '8' must map to [0,7,1,1]");
        // Digit 9: indices 8 in PATTERNS, pattern "0810".
        let p9 = encode("9", &Options::default()).expect(
            "encode(\"9\", default) (Flattermarken digit-9 path: BARCHARS[8] → PATTERNS[8] \"0810\" → runs [0,8,1,0]) must succeed",
        );
        assert_eq!(p9.bars, vec![0, 8, 1, 0], "digit '9' must map to [0,8,1,0]");
        // Pin the full "1234567890" expansion to lock down all 10 patterns
        // in a single check. This is the most direct way to kill a single
        // index-permutation mutant.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken full-PATTERNS expansion path: pins every
        // pattern index 0..=9 in a single 40-run check.
        let all = encode("1234567890", &Options::default()).expect(
            "encode(\"1234567890\", default) (Flattermarken full-PATTERNS index 0..=9 expansion; 40-run vector pins entire table) must succeed",
        );
        let want: [u8; 40] = [
            0, 0, 1, 8, // '1' → PATTERNS[0]
            0, 1, 1, 7, // '2' → PATTERNS[1]
            0, 2, 1, 6, // '3' → PATTERNS[2]
            0, 3, 1, 5, // '4' → PATTERNS[3]
            0, 4, 1, 4, // '5' → PATTERNS[4]
            0, 5, 1, 3, // '6' → PATTERNS[5]
            0, 6, 1, 2, // '7' → PATTERNS[6]
            0, 7, 1, 1, // '8' → PATTERNS[7]
            0, 8, 1, 0, // '9' → PATTERNS[8]
            0, 9, 0, 0, // '0' → PATTERNS[9]
        ];
        assert_eq!(all.bars, want, "every digit's pattern must match");
        // include_text branch: default omits text, explicit on populates it.
        assert_eq!(all.text, None, "default include_text=false → text=None");
        let opts_on = Options {
            include_text: true,
            ..Options::default()
        };
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Flattermarken include_text=true path.
        let with_text = encode("42", &opts_on).expect(
            "encode(\"42\", include_text=true) (Flattermarken include_text branch; must populate text with raw payload \"42\") must succeed",
        );
        assert_eq!(
            with_text.text.as_deref(),
            Some("42"),
            "include_text=true must populate text with the original payload"
        );
    }

    /// Kills `encode: replace > with ==` and `> with >=` at line ~37
    /// (the 500-char payload-length cap). The original test corpus
    /// used short payloads only; the mutants flipped the inequality so
    /// either exactly-500 (accepted by spec) is rejected, or only
    /// exactly-500 is rejected. We bracket the boundary at 499/500/501.
    #[test]
    fn payload_length_cap_is_strictly_five_hundred() {
        let four_99 = "1".repeat(499);
        let five_00 = "1".repeat(500);
        let five_01 = "1".repeat(501);
        // Stage 11.A8c (cont) — descriptive labels naming Flattermarken
        // 500-char length-cap boundary (499/500 accept, 501 reject).
        assert!(
            encode(&four_99, &Options::default()).is_ok(),
            "encode(499-char Flattermarken) must accept (one below 500-cap; kills `< 500` → `<= 499` boundary mutant)"
        );
        assert!(
            encode(&five_00, &Options::default()).is_ok(),
            "encode(500-char Flattermarken) must accept (exactly at 500-cap; kills `<= 500` → `< 500` boundary mutant)"
        );
        // Stage 11.A8c — upgrade discriminant-only to 3-anchor pin
        // matching the length-cap diagnostic, with cross-arm guards.
        match encode(&five_01, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Flattermarken"),
                    "missing `Flattermarken` prefix: {msg}"
                );
                assert!(
                    msg.contains("exceeds 500 chars"),
                    "missing `exceeds 500 chars` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("accepts digits only"),
                    "wrong arm — non-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("501-digit Flattermarken should reject as InvalidData, got {other:?}"),
        }
    }
}
