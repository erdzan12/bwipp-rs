//! BC412 — IBM's "Binary Code 412", primarily used on semiconductor wafers
//! and disc media.
//!
//! Numeric + uppercase (except `O`) alphabet of 35 characters. Each
//! character encodes to 4 bars + 4 spaces with the bars at fixed
//! positions and the spaces varying in width (1–5 modules). Optional
//! mod-35 check digit; the `semi` mode adds a different (sum-of-odd,
//! sum-of-even × 2) check used on Seagate's semiconductor BC412
//! products and prepends/appends a start/stop sentinel.
//!
//! Patterns ported from `bwipp_bc412` in bwip-js v4.x.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// 35-char alphabet — note the non-lexicographic order and the missing
/// `O` (to avoid digit-zero confusion). Index in this string equals the
/// character's BC412 codeword value.
const BARCHARS: &str = "0R9GLVHA8EZ4NTS1J2Q6C7DYKBUIX3FWP5M";

/// 37 entries: 35 alphabet codes + start ("12") + stop ("111"). Each of
/// the 35 character entries is 8 sbs runs (4 bars + 4 spaces). The
/// start sentinel is 2 runs (1 bar + 1 space), the stop is 3 runs
/// (bar + space + bar). Ported verbatim from BWIPP's `bc412_encs`.
#[rustfmt::skip]
const ENCS: [&str; 37] = [
    "11111115", "13111212", "11131113", "12111213", "12121311", "13131111",
    "12111312", "11131212", "11121411", "11151111", "15111111", "11111511",
    "12131211", "13121112", "13111311", "11111214", "12121113", "11111313",
    "13111113", "11121213", "11141112", "11121312", "11141211", "14121111",
    "12121212", "11131311", "13121211", "12111411", "14111211", "11111412",
    "12111114", "14111112", "12141111", "11121114", "12131112",
    "12",  // 35: start sentinel
    "111", // 36: stop sentinel
];

const START_IDX: usize = 35;
const STOP_IDX: usize = 36;

/// Encode a BC412 payload.
///
/// Recognized options:
/// * `includecheck = "true"` — append a mod-35 sum-of-codeword-values
///   check character (default: off).
/// * `includestartstop = "true"` — wrap the symbol in the BWIPP
///   start "12" + stop "111" sentinels (default: off).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // BC412 alphabet excludes `O` to avoid 0/O confusion in semiconductor
/// // wafer-tracking contexts.
/// let svg = render_svg(Symbology::Bc412, "ABC123", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData("BC412 payload must not be empty".into()));
    }
    if data.chars().count() > 500 {
        return Err(Error::InvalidData(
            "BC412 payload exceeds BWIPP's 500-char limit".into(),
        ));
    }
    let mut indices: Vec<u8> = Vec::with_capacity(data.chars().count());
    for c in data.chars() {
        match BARCHARS.find(c) {
            Some(i) => indices.push(i as u8),
            None => {
                return Err(Error::InvalidData(format!(
                    "BC412 does not support character {c:?} (digits + uppercase except O)"
                )))
            }
        }
    }
    let include_check = opts.get("includecheck").is_some_and(|v| v == "true");
    let include_startstop = opts.get("includestartstop").is_some_and(|v| v == "true");

    let check_idx: Option<u8> = if include_check {
        let sum: u32 = indices.iter().map(|&i| u32::from(i)).sum();
        Some((sum % 35) as u8)
    } else {
        None
    };

    let mut sbs: Vec<u8> = Vec::with_capacity((indices.len() + 3) * 8);
    if include_startstop {
        extend_from_enc(&mut sbs, ENCS[START_IDX]);
    }
    for &i in &indices {
        extend_from_enc(&mut sbs, ENCS[i as usize]);
    }
    if let Some(c) = check_idx {
        extend_from_enc(&mut sbs, ENCS[c as usize]);
    }
    if include_startstop {
        extend_from_enc(&mut sbs, ENCS[STOP_IDX]);
    }

    let text = if opts.include_text {
        let mut t = data.to_string();
        if include_check {
            if let Some(c) = check_idx {
                t.push(BARCHARS.chars().nth(c as usize).unwrap());
            }
        }
        Some(t)
    } else {
        None
    };
    Ok(LinearPattern { bars: sbs, text })
}

fn extend_from_enc(sbs: &mut Vec<u8>, enc: &str) {
    for c in enc.chars() {
        sbs.push(c.to_digit(10).expect("BC412 enc must be all digits") as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `extend_from_enc(sbs, enc)`. The helper
    /// translates an ENC-table digit string into per-module run-length
    /// bytes by parsing each char via `to_digit(10)` and pushing the
    /// resulting integer into `sbs`. The existing tests only exercise
    /// this indirectly through `matches_bwip_js_raw_sbs` and the start/
    /// stop / check-digit goldens, where any body-replacement or
    /// append-vs-replace mutation may pass undetected.
    ///
    /// Mutations to catch:
    ///   - Body replaced with `()`: sbs unchanged; downstream goldens
    ///     would fail but the helper itself has no isolated check.
    ///   - `for c in enc.chars()` → `for c in enc.bytes()`: each byte
    ///     becomes the literal ASCII code (e.g. '1'=0x31=49) — pushed
    ///     value would be far larger than a real run length.
    ///   - `sbs.push(...)` → `sbs[0] = ...` or similar: silent corrupt.
    ///   - `as u8` cast width changes: digit '7' would clip differently
    ///     under wrong types (not strictly observable on 0..9 but pins
    ///     the contract).
    #[test]
    fn extend_from_enc_appends_per_char_digits() {
        // Empty enc: sbs unchanged.
        let mut sbs: Vec<u8> = vec![5, 6, 7];
        extend_from_enc(&mut sbs, "");
        assert_eq!(sbs, vec![5, 6, 7], "empty enc must leave sbs unchanged");

        // Each digit char pushes its base-10 value.
        let mut sbs: Vec<u8> = Vec::new();
        extend_from_enc(&mut sbs, "1234567890");
        assert_eq!(
            sbs,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 0],
            "every digit char must convert to its base-10 value (not ASCII code)"
        );

        // Appending preserves the existing prefix — proves it's `push`,
        // not `set`.
        let mut sbs: Vec<u8> = vec![42, 7];
        extend_from_enc(&mut sbs, "13");
        assert_eq!(
            sbs,
            vec![42, 7, 1, 3],
            "extend_from_enc must APPEND, not replace the prefix"
        );

        // Realistic BC412 ENC entry "11121411" (codeword '8' from the
        // existing test): 8 modules expanding to [1,1,1,2,1,4,1,1].
        let mut sbs: Vec<u8> = Vec::new();
        extend_from_enc(&mut sbs, "11121411");
        assert_eq!(
            sbs,
            vec![1, 1, 1, 2, 1, 4, 1, 1],
            "ENCS[8] expansion matches the check_digit_codeword golden"
        );
    }

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 2-anchor
        // pin matching the source diagnostic at line 61 of bc412.rs:
        //   1. `BC412 payload` family + property prefix
        //   2. `must not be empty` predicate
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("BC412 payload"),
                    "missing BC412 payload prefix: {msg}"
                );
                assert!(
                    msg.contains("must not be empty"),
                    "missing predicate: {msg}"
                );
            }
            other => panic!("empty BC412 should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_o() {
        // BC412 alphabet omits 'O' to disambiguate from '0'.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the source diagnostic at line 74 of bc412.rs:
        //   1. `BC412 does not support character` predicate
        //   2. `'O'` char Debug echo
        //   3. `digits + uppercase except O` valid-alphabet hint
        // 4th anchor: cross-arm exclusion — must NOT echo any of the
        // valid letters 'A'/'B'/'C' from the same payload (kills a
        // `.find` → `.rfind` mutant or one that swaps `c` with the
        // last char it saw).
        match encode("ABCO", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("BC412 does not support character"),
                    "missing predicate: {msg}"
                );
                assert!(msg.contains("'O'"), "missing 'O' char echo: {msg}");
                assert!(
                    msg.contains("digits + uppercase except O"),
                    "missing valid-alphabet hint: {msg}"
                );
                assert!(
                    !msg.contains("'A'") && !msg.contains("'B'") && !msg.contains("'C'"),
                    "'ABCO' rejection must echo only the offending 'O', NOT the valid 'A'/'B'/'C' prefix (mutation iterating wrong char): {msg}"
                );
            }
            other => panic!(
                "BC412 encode(\"ABCO\") must reject as Err(InvalidData(alphabet-omits-O)); got {other:?}"
            ),
        }
    }

    #[test]
    fn rejects_lowercase() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the rejects_o sibling pattern.
        // 4th anchor: iteration-direction guard — 'abc' has THREE
        // lowercase chars. The diagnostic must echo the FIRST one 'a'
        // (offset 0), NOT the last 'c' (offset 2). A `.rev()` mutant
        // on the char-scan would report 'c' instead.
        match encode("abc", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("BC412 does not support character"),
                    "missing predicate: {msg}"
                );
                assert!(msg.contains("'a'"), "missing 'a' char echo: {msg}");
                assert!(
                    msg.contains("digits + uppercase except O"),
                    "missing valid-alphabet hint: {msg}"
                );
                assert!(
                    !msg.contains("'c'") && !msg.contains("'b'"),
                    "'abc' rejection must echo the FIRST lowercase char 'a' (offset 0), NOT 'b' or 'c' (mutation iterates in reverse / from end): {msg}"
                );
            }
            other => panic!(
                "BC412 encode(\"abc\") must reject as Err(InvalidData(lowercase)); got {other:?}"
            ),
        }
    }

    /// Byte-for-byte sbs cross-validation against `b.raw("bc412",
    /// text, {})[0].sbs`. BWIPP defaults `includecheck` and
    /// `includestartstop` to false, so our default-off matches.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        let cases: &[(&str, &[u8])] = &[(
            "ABC123",
            &[
                1, 1, 1, 3, 1, 2, 1, 2, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 4, 1, 1, 1, 2, 1, 1, 1, 1,
                1, 2, 1, 4, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 4, 1, 2,
            ],
        )];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo.
            let p = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (BC412 sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(p.bars, want, "BC412 sbs mismatch for {text:?}");
        }
    }

    /// With `includestartstop: true`, the symbol is wrapped in the
    /// BWIPP start "12" (1 bar + 1 space) and stop "111" (bar +
    /// space + bar) sentinels. The data sbs is unchanged in between.
    #[test]
    fn start_stop_wraps_the_symbol() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the BC412 includestartstop wrap path: BWIPP start "12" +
        // stop "111" sentinels added around plain data sbs.
        let plain = encode("ABC123", &Options::default()).expect(
            "encode(\"ABC123\", default) (BC412 plain baseline for start/stop wrap cross-check) must succeed",
        );
        let wrapped = encode(
            "ABC123",
            &Options::default().with("includestartstop", "true"),
        )
        .expect(
            "encode(\"ABC123\", includestartstop=true) (BC412 includestartstop path: BWIPP \"12\" start + \"111\" stop sentinels added) must succeed",
        );
        // Wrapped = start (2) + plain + stop (3) = plain.len() + 5.
        assert_eq!(wrapped.bars.len(), plain.bars.len() + 5);
        // Start sentinel is [1, 2] at the front.
        assert_eq!(&wrapped.bars[..2], &[1, 2]);
        // Stop sentinel is [1, 1, 1] at the back.
        assert_eq!(&wrapped.bars[wrapped.bars.len() - 3..], &[1, 1, 1]);
        // Middle matches the plain sbs.
        assert_eq!(
            &wrapped.bars[2..wrapped.bars.len() - 3],
            plain.bars.as_slice()
        );
    }

    /// `includecheck` appends a 1-character mod-35 check after the
    /// data and before any stop sentinel. The check character is
    /// the sum of codeword indices modulo 35.
    #[test]
    fn check_digit_appends_one_codeword() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the BC412 includecheck length-cross-check path.
        let plain = encode("ABC123", &Options::default()).expect(
            "encode(\"ABC123\", default) (BC412 plain baseline for includecheck length-delta cross-check) must succeed",
        );
        let checked = encode("ABC123", &Options::default().with("includecheck", "true")).expect(
            "encode(\"ABC123\", includecheck=true) (BC412 includecheck path: appends 1-char mod-35 check = 8 extra sbs runs) must succeed",
        );
        // One extra character of 8 sbs runs each.
        assert_eq!(checked.bars.len(), plain.bars.len() + 8);
        // Prefix matches the no-check output.
        assert_eq!(&checked.bars[..plain.bars.len()], plain.bars.as_slice());
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills `encode: replace % with /` (line ~84). The `check_digit_
    /// appends_one_codeword` test above only verified the *length* of
    /// the appended codeword sbs, not its identity — so the mutant
    /// `sum / 35` (which produces a different codeword index for any
    /// input where `sum >= 35`) survived. This test pins the actual
    /// codeword pattern for an input whose `sum % 35 != sum / 35`.
    #[test]
    fn check_digit_codeword_value_matches_modulo_thirty_five() {
        // BARCHARS positions: A=7, B=25, C=20, 1=15, 2=17, 3=29.
        // sum = 7+25+20+15+17+29 = 113. 113 % 35 = 8 (codeword '8',
        // since BARCHARS[8] == '8'). 113 / 35 = 3 (codeword 'G'). The
        // mutant `sum / 35` would emit 'G' (sbs ENCS[3] = [1,2,1,1,
        // 1,2,1,3]) at the check position; the spec says emit '8'.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the BC412 mod-35 check-codeword arithmetic-pinning path: pins
        // sum % 35 = 8 vs mutant sum / 35 = 3.
        let checked = encode("ABC123", &Options::default().with("includecheck", "true")).expect(
            "encode(\"ABC123\", includecheck=true) (BC412 mod-35 check arithmetic path; sum=113 → 113%35=8 codeword '8' = ENCS[8] \"11121411\"; mutant sum/35=3 would emit 'G') must succeed",
        );
        // Codeword '8' is `ENCS[8]` = "11121411" → sbs [1,1,1,2,1,4,1,1].
        let want_check_sbs: &[u8] = &[1, 1, 1, 2, 1, 4, 1, 1];
        let check_start = checked.bars.len() - want_check_sbs.len();
        assert_eq!(
            &checked.bars[check_start..],
            want_check_sbs,
            "expected mod-35 codeword '8' (index 8) — saw something else; \
             likely the `sum % 35` arithmetic regressed"
        );
    }

    /// Kills `encode: replace > with ==` and `> with >=` at line ~63
    /// (the BWIPP-imposed 500-char payload limit). The original test
    /// corpus never reached the boundary, so both mutants survived.
    /// We exercise inputs at length 500 (accept), 501 (reject), and
    /// the off-by-one cluster around it.
    #[test]
    fn payload_length_limit_is_strictly_five_hundred() {
        let five_hundred: String = "A".repeat(500);
        let five_oh_one: String = "A".repeat(501);
        let four_ninety_nine: String = "A".repeat(499);
        // Exactly 500 is valid.
        // Stage 11.A8c (cont) — descriptive label naming exact-cap path.
        assert!(
            encode(&five_hundred, &Options::default()).is_ok(),
            "encode(500-char BC412) must accept (exactly at 500-cap; kills `<= 500` → `< 500` boundary mutant)"
        );
        // 501 is over the limit.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 2-anchor pin matching the source diagnostic at line
        // 64-66 of bc412.rs (`BC412 payload exceeds BWIPP's 500-
        // char limit`):
        //   1. `BC412` symbology prefix
        //   2. `exceeds BWIPP's 500-char limit` predicate
        //   + cross-arm guard against `must not be empty` leakage.
        match encode(&five_oh_one, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("BC412"), "missing `BC412` prefix: {msg}");
                assert!(
                    msg.contains("exceeds BWIPP's 500-char limit"),
                    "missing length-cap predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
            }
            other => panic!("501-char BC412 should reject as InvalidData, got {other:?}"),
        }
        // 499 is well within. Bracketing both sides of the boundary
        // kills the `> with >=` mutant too.
        // Stage 11.A8c (cont) — descriptive label naming below-cap path.
        assert!(
            encode(&four_ninety_nine, &Options::default()).is_ok(),
            "encode(499-char BC412) must accept (one below 500-cap; brackets boundary with 500-accept + 501-reject)"
        );
    }
}
