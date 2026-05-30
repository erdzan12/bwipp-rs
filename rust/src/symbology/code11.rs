//! Code 11.
//!
//! Numeric (`0..=9` and `-`) with optional one or two check digits using the
//! mod-11 ("C") and mod-9 ("K") algorithms. Each character is a 5-element
//! bar/space pattern (3 bars + 2 spaces) at a 3:1 wide:narrow ratio.
//!
//! The 6-element entries in [`ENCS`] include the trailing inter-character
//! space (always one narrow module) so concatenating runs builds the
//! whole symbol without further joining logic — same convention as
//! BWIPP's `code11_encs`.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

const ALPHABET: &str = "0123456789-";

/// Per-character sbs run-length encodings, taken verbatim from BWIPP's
/// `code11_encs`. Indexed parallel to [`ALPHABET`]; index 11 holds the
/// `*` start/stop sentinel. Each entry is `[bar, space, bar, space,
/// bar, inter-char-space]`. Narrow = 1 module, wide = 3 modules.
#[rustfmt::skip]
const ENCS: [[u8; 6]; 12] = [
    [1, 1, 1, 1, 3, 1], // '0'
    [3, 1, 1, 1, 3, 1], // '1'
    [1, 3, 1, 1, 3, 1], // '2'
    [3, 3, 1, 1, 1, 1], // '3'
    [1, 1, 3, 1, 3, 1], // '4'
    [3, 1, 3, 1, 1, 1], // '5'
    [1, 3, 3, 1, 1, 1], // '6'
    [1, 1, 1, 3, 3, 1], // '7'
    [3, 1, 1, 3, 1, 1], // '8'
    [3, 1, 1, 1, 1, 1], // '9'
    [1, 1, 3, 1, 1, 1], // '-'
    [1, 1, 3, 3, 1, 1], // '*'  (start/stop)
];

const SENTINEL_INDEX: usize = 11;

fn value(c: char) -> Option<u32> {
    ALPHABET.find(c).map(|i| i as u32)
}

/// Encode a Code 11 payload.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Code 11 accepts digits + `-`.
/// let svg = render_svg(Symbology::Code11, "1234-5678", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Code 11 payload must not be empty".into(),
        ));
    }
    for c in data.chars() {
        if value(c).is_none() {
            return Err(Error::InvalidData(format!(
                "Code 11 does not support character {c:?}"
            )));
        }
    }

    let include_check = opts
        .get("includecheck")
        .map(|v| v == "true")
        .unwrap_or(true);

    let mut full = data.to_string();
    if include_check {
        full.push(check_c(data));
        // K check digit is added when payload is 10 chars or more.
        if data.len() >= 10 {
            full.push(check_k(&full));
        }
    }

    // Concatenate sbs runs: start + each char + stop. Each enc already
    // carries its trailing inter-char gap, so no further joiners are
    // needed — the alternation invariant (bar, space, bar, space, …)
    // is preserved across the concatenation because every enc has 3
    // bars and 3 spaces.
    let mut bars: Vec<u8> = Vec::with_capacity((full.chars().count() + 2) * 6);
    bars.extend_from_slice(&ENCS[SENTINEL_INDEX]);
    for c in full.chars() {
        let idx = value(c).unwrap() as usize;
        bars.extend_from_slice(&ENCS[idx]);
    }
    bars.extend_from_slice(&ENCS[SENTINEL_INDEX]);

    let text = if opts.include_text { Some(full) } else { None };
    Ok(LinearPattern { bars, text })
}

fn check_c(data: &str) -> char {
    weighted_check(data, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 11)
}

fn check_k(data: &str) -> char {
    weighted_check(data, &[1, 2, 3, 4, 5, 6, 7, 8, 9], 9)
}

fn weighted_check(data: &str, weights: &[u32], modulus: u32) -> char {
    let mut sum: u32 = 0;
    for (i, c) in data.chars().rev().enumerate() {
        let w = weights[i % weights.len()];
        sum += value(c).unwrap() * w;
    }
    ALPHABET.chars().nth((sum % modulus) as usize).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_digits() {
        // Stage 11.A8c (cont) — descriptive label naming input. Code 11
        // covers digits 0..9 plus '-'; full digit span exercises the
        // entire 0..=9 lookup table.
        let p = encode("0123456789", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode(\"0123456789\") (full digit span 0..9) must compose into non-empty Code 11 symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn rejects_letters() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 63-65 of code11.rs (`Code 11 does not support character
        // {c:?}`):
        //   1. `Code 11` symbology prefix
        //   2. `does not support character` predicate
        //   3. `'A'` Debug echo (first non-valid char in "ABC")
        match encode("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 11"), "missing `Code 11` prefix: {msg}");
                assert!(
                    msg.contains("does not support character"),
                    "missing `does not support character` predicate: {msg}"
                );
                assert!(msg.contains("'A'"), "missing 'A' Debug echo: {msg}");
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
            }
            other => panic!("\"ABC\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// Byte-for-byte cross-validation against bwip-js. The sbs arrays
    /// below come from
    ///   `b.raw("code11", text, {includecheck: false})[0].sbs`
    /// captured with the v4 vendor snapshot. BWIPP defaults
    /// `includecheck` to `false`, so we drop our own default-on
    /// behaviour for this comparison.
    #[test]
    fn sbs_matches_bwipp_without_check() {
        let cases: &[(&str, &[u8])] = &[
            (
                "123",
                &[
                    1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1,
                    3, 3, 1, 1,
                ],
            ),
            (
                "0123456789",
                &[
                    1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3,
                    3, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1,
                ],
            ),
            (
                "1-2",
                &[
                    1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1,
                    3, 3, 1, 1,
                ],
            ),
        ];
        let opts = Options::default().with("includecheck", "false");
        for &(text, want) in cases {
            let got = encode(text, &opts).unwrap();
            assert_eq!(got.bars, want, "Code 11 sbs mismatch for {text:?}");
        }
    }

    /// The default-on check digit path appends one C-check (≤9 input
    /// chars) or two C/K checks (≥10 input chars). Anchors those
    /// codeword positions against bwip-js with `includecheck: true`.
    #[test]
    fn sbs_matches_bwipp_with_check() {
        let cases: &[(&str, &[u8])] = &[
            (
                "123",
                &[
                    1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1,
                    3, 1, 1, 1, 1, 1, 3, 3, 1, 1,
                ],
            ),
            (
                "0123456789",
                &[
                    1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3,
                    3, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1,
                    1, 1, 3, 3, 1, 1,
                ],
            ),
            (
                "1-2",
                &[
                    1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 3, 3, 1, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            let got = encode(text, &Options::default()).unwrap();
            assert_eq!(
                got.bars, want,
                "Code 11 sbs mismatch (with check) for {text:?}"
            );
        }
    }

    /// Stage 11.A8c — pin `value` lookup for every ALPHABET position
    /// and rejection of out-of-alphabet chars. Kills the `position`
    /// predicate and `as u32` cast mutations on lines 40-42.
    #[test]
    fn value_lookup_every_char() {
        // Digits → indices 0..=9.
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(value(c), Some(i as u32), "value({c:?})");
        }
        // '-' → index 10.
        assert_eq!(value('-'), Some(10));
        // Non-members.
        assert_eq!(value('A'), None);
        assert_eq!(value('a'), None);
        assert_eq!(value(' '), None);
        assert_eq!(value('*'), None); // sentinel only — not in value lookup
        assert_eq!(value('/'), None); // just below '0'
        assert_eq!(value(':'), None); // just above '9'
    }

    /// Stage 11.A8c — pin `check_c` / `check_k` mod-11 / mod-9
    /// weighted checks for short canonical inputs. Hand-computed:
    ///   - "123": sum = 3*1 + 2*2 + 1*3 = 10, 10 % 11 = 10 → '-'.
    ///   - "1": sum = 1*1 = 1, 1 % 11 = 1 → '1'.
    /// Kills the `* w` arithmetic, `% modulus` reduction, and the
    /// `chars().rev()` order mutations on lines 107-114.
    #[test]
    fn check_c_known_values() {
        assert_eq!(check_c("1"), '1');
        // "123" via check_c: rev = "321".
        //   sum = value('3')*1 + value('2')*2 + value('1')*3 = 3+4+3 = 10.
        //   10 % 11 = 10. ALPHABET[10] = '-'.
        assert_eq!(check_c("123"), '-');
        // "0": sum = 0, 0 % 11 = 0 → '0'.
        assert_eq!(check_c("0"), '0');
        // Empty data → sum 0 → '0'.
        assert_eq!(check_c(""), '0');
    }

    /// Stage 11.A8c — pin `check_k` (mod-9, weights 1..=9). Previously
    /// only `check_c` had a direct test, leaving body-swap mutations
    /// between the two functions free to slip (e.g. swapping weights
    /// or moduli). Also adds a weight-wraparound case (>10 chars for
    /// check_c) that exercises the `i % weights.len()` cycling logic
    /// neither the existing short-input tests nor the goldens hit.
    ///
    /// Hand-computed anchors:
    ///   - check_k(""): sum=0, 0 % 9 = 0 → '0'.
    ///   - check_k("9"): sum=9, 9 % 9 = 0 → '0'.  (Distinguishes
    ///     from check_c("9") which returns '9' since 9 % 11 = 9.)
    ///   - check_k("123"): sum=10, 10 % 9 = 1 → '1'.  (Distinguishes
    ///     from check_c("123") which returns '-'.)
    ///   - check_c("12345678901") (11 chars): weight at pos 10
    ///     cycles back to weights[0]=1. Sum = 246, 246 % 11 = 4 → '4'.
    ///   - check_k("0123456789") (10 chars > 9 weights): weight at
    ///     pos 9 cycles back to weights[0]=1. Sum = 165, 165 % 9 = 3
    ///     → '3'.
    ///
    /// Mutations to catch:
    ///   - swap of check_c / check_k bodies (different weights/mod).
    ///   - `[1..=9]` → `[1..=10]` in check_k (drops the wrap edge).
    ///   - `11` → `9` (or vice-versa) in either modulus.
    ///   - `weights[i % weights.len()]` → `weights[i]` (panics at the
    ///     wraparound positions).
    #[test]
    fn check_k_and_weight_wraparound() {
        // check_k mod-9 short inputs.
        assert_eq!(check_k(""), '0', "empty → sum 0 → '0'");
        assert_eq!(check_k("0"), '0', "single 0 → 0 % 9 = 0 → '0'");
        assert_eq!(
            check_k("9"),
            '0',
            "single 9 → 9 % 9 = 0 → '0' (distinct from check_c → '9')"
        );
        assert_eq!(
            check_k("123"),
            '1',
            "'123' → sum 10, 10 % 9 = 1 → '1' (vs check_c '-')"
        );

        // Long inputs that trigger the `i % weights.len()` wraparound.
        // check_c with 11 chars: weight at position 10 cycles to 1.
        assert_eq!(
            check_c("12345678901"),
            '4',
            "11-char input exercises weights[10 % 10] = weights[0] wraparound"
        );
        // check_k with 10 chars: weight at position 9 cycles to 1.
        assert_eq!(
            check_k("0123456789"),
            '3',
            "10-char input exercises weights[9 % 9] = weights[0] wraparound"
        );
    }

    /// Stage 11.A8c — pin `weighted_check(data, weights, modulus)`
    /// directly with custom (weights, modulus) combinations. The
    /// existing `check_c` and `check_k` anchors exercise it only with
    /// their fixed parameter sets ([1..=10], 11) and ([1..=9], 9).
    /// Pin the helper's three observable behaviors with synthetic
    /// arguments to lock the algorithm:
    ///   * `data.chars().rev()` order (right-to-left walk);
    ///   * `weights[i % weights.len()]` wraparound at the weights tail;
    ///   * `sum % modulus` index into ALPHABET.
    ///
    /// Mutations killed:
    ///   * `data.chars().rev()` → `data.chars()` (order flip);
    ///   * `i % weights.len()` → `i` direct index (would panic on
    ///     overflow);
    ///   * `weights[...]` removed: weight always 1;
    ///   * `sum % modulus` → other mod;
    ///   * `value(c).unwrap() * w` → `+ w` (would shift base sum).
    #[test]
    fn weighted_check_walks_right_to_left_with_weight_wraparound() {
        // Empty data → sum=0 → ALPHABET[0] = '0'.
        assert_eq!(weighted_check("", &[1, 2, 3], 5), '0');

        // Single digit "5" with weights [7], mod 11:
        //   rev "5", i=0, w=weights[0]=7, value('5')=5, sum=5*7=35,
        //   35 % 11 = 2 → ALPHABET[2] = '2'.
        assert_eq!(
            weighted_check("5", &[7], 11),
            '2',
            "single digit applies first weight"
        );

        // Walk-direction anchor: "01" vs "10" with weights=[1, 2].
        //   "01" rev "10": i=0 c='1' w=1 → 1; i=1 c='0' w=2 → 0. sum=1.
        //   "10" rev "01": i=0 c='0' w=1 → 0; i=1 c='1' w=2 → 2. sum=2.
        // → distinct results, kills `.rev()` removal.
        let s01 = weighted_check("01", &[1, 2], 11);
        let s10 = weighted_check("10", &[1, 2], 11);
        assert_eq!(s01, '1');
        assert_eq!(s10, '2');
        assert_ne!(s01, s10, "right-to-left walk must matter");

        // Weight-cycling anchor: "123" with weights [1, 2] (len 2).
        //   rev "321": i=0 c='3' w=weights[0]=1 → 3
        //             i=1 c='2' w=weights[1]=2 → 4
        //             i=2 c='1' w=weights[0]=1 → 1 (wraparound!)
        //   sum = 3+4+1 = 8 → ALPHABET[8] = '8'.
        assert_eq!(
            weighted_check("123", &[1, 2], 11),
            '8',
            "weights[i % len] wraparound at i=2 → weights[0]"
        );

        // Modulus anchor: same data + weights, different mod.
        //   "9" with weights [3], mod 11: sum = 9*3 = 27, 27 % 11 = 5 → '5'.
        //   "9" with weights [3], mod 7:  sum = 27, 27 % 7 = 6 → '6'.
        assert_eq!(weighted_check("9", &[3], 11), '5');
        assert_eq!(weighted_check("9", &[3], 7), '6');
    }
}
