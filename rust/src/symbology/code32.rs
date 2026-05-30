//! Code 32 (Italian Pharmacode / "PZN").
//!
//! 8-digit input → 6-character Code 39 barcode. The 6 characters are the
//! base-32 representation of the 9-digit number formed by the input + a
//! mod-10 check digit, using BWIPP's `0..9 B C D F..H J..N P..T U V W X Y Z`
//! alphabet (digits 0..9 then A..V mapped past the vowels A, E, I, O). The
//! human-readable text below the bars conventionally carries an "A" prefix
//! and the full 9-digit value; the bars themselves don't encode the "A".

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

const ALPHABET: &[u8; 32] = b"0123456789BCDFGHJKLMNPQRSTUVWXYZ";

/// Encode a Code 32 payload.
///
/// Input is 8 numeric characters (or 9 if the user supplies the check
/// digit themselves). The bars encode the 6-character base-32 form of
/// the 9-digit value, matching BWIPP / bwip-js byte-for-byte.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Italian Pharmacode (AIC) — 8 digits; the encoder appends the mod-10
/// // check and renders Code 39 bars over the base-32 expansion.
/// let svg = render_svg(Symbology::Code32, "01234567", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 && digits.len() != 9 {
        return Err(Error::InvalidData(
            "Code 32 requires 8 digits (check computed) or 9 digits (with check)".into(),
        ));
    }

    let nine = if digits.len() == 8 {
        let check = compute_check(&digits);
        format!("{digits}{check}")
    } else {
        digits
    };

    let value: u64 = nine
        .parse()
        .map_err(|_| Error::InvalidData("bad number".into()))?;
    let mut encoded = [b' '; 6];
    let mut v = value;
    for slot in encoded.iter_mut().rev() {
        *slot = ALPHABET[(v % 32) as usize];
        v /= 32;
    }
    let code32_string = std::str::from_utf8(&encoded).unwrap().to_string();

    super::code39::encode(&code32_string, opts)
}

fn compute_check(digits: &str) -> char {
    // Italian pharmaceutical mod-10: odd positions ×1, even ×2; if the
    // doubled product is >= 10, add the digits; sum % 10.
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().enumerate() {
        let n = c.to_digit(10).unwrap();
        let weighted = if i % 2 == 0 { n } else { n * 2 };
        sum += if weighted >= 10 {
            weighted - 9 // equivalent to summing the two digits
        } else {
            weighted
        };
    }
    let check = sum % 10;
    char::from_digit(check, 10).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_input() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin:
        //   1. `Code 32` symbology name
        //   2. `requires 8 digits` predicate
        //   3. `9 digits` complementary valid-length anchor (matches
        //      the source diagnostic at line 36 of code32.rs)
        // A variant-only check would survive a mutation that swaps
        // the format string to a different InvalidData message.
        match encode("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 32"),
                    "missing Code 32 symbology name: {msg}"
                );
                assert!(
                    msg.contains("requires 8 digits"),
                    "missing `requires 8 digits` predicate: {msg}"
                );
                assert!(
                    msg.contains("9 digits"),
                    "missing complementary `9 digits` valid-length anchor: {msg}"
                );
            }
            other => panic!("'123' should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn accepts_8_digits() {
        // Stage 11.A8c (cont) — descriptive label naming input. Code 32
        // is the Italian Pharmacode, accepting 8 or 9 digits. This
        // test exercises the lower-bound 8-digit boundary.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Code 32 lower-bound 8-digit path: Italian Pharmacode
        // accepts 8 or 9 digits; this exercises the 8-digit boundary.
        let p = encode("01234567", &Options::default()).expect(
            "encode(\"01234567\", default) (Code 32 Italian Pharmacode 8-digit lower boundary) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode(\"01234567\") (8-digit lower boundary) must compose into non-empty Code 32 symbol; got {}",
            p.total_width()
        );
    }

    /// Golden bar pattern for `"01234567"` captured from bwip-js's
    /// `raw("code32", "01234567", {})[0].sbs`. Code 32 is the Italian
    /// Pharmacode — internally a Code 39 encoding of the 6-char
    /// base-32 form of `data + mod-10 check digit`. The bars don't
    /// include an "A" prefix; that's a human-readable convention.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Code 32 8-digit "01234567" → 80-bar byte-for-byte oracle
        // path: Code 32 internally re-codes as Code 39 of the 6-char
        // base-32 (data + mod-10 check digit) representation.
        let p = encode("01234567", &Options::default()).expect(
            "encode(\"01234567\", default) (Code 32 → Code 39 base-32 re-coding; 80-bar bwip-js oracle) must succeed",
        );
        let want: [u8; 80] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1,
            1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1,
            3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "code32 bars mismatch vs bwip-js raw output");
    }

    /// Kills `compute_check: replace * with +` at line ~67. The
    /// existing `matches_bwip_js_raw_sbs` row "01234567" produced the
    /// same check digit (6) under both the original `n * 2` and the
    /// mutant `n + 2` arithmetic by a digit-by-digit coincidence
    /// (sums diverge but both hit 26/36 ≡ 6 mod 10). This test pins
    /// "12345678" — the digit weights diverge into 38 vs 35 ≡ 2 vs
    /// 5 mod 10, so the mutated check digit changes the entire
    /// base-32 conversion and the bar sequence diverges.
    #[test]
    fn weighted_arithmetic_diverges_on_input_with_distinguishing_check() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Code 32 `* vs +` mutation-killing arithmetic-divergence
        // input "12345678": digit weights diverge (38 vs 35 ≡ 2 vs 5
        // mod 10), so `n*2` vs `n+2` mutation produces visibly
        // different check digits → different base-32 conversion → bar
        // sequence diverges.
        let p = encode("12345678", &Options::default()).expect(
            "encode(\"12345678\", default) (Code 32 mutation-killing arithmetic-divergence input: n*2 vs n+2 produces check=2 vs check=5) must succeed",
        );
        // Full bar sequence captured from the *current* (original)
        // `compute_check` output. The mutant `n + 2` produces check=5
        // instead of check=2, which shifts the last base-32 codeword
        // and the bar sequence diverges (Code 39's character→bars
        // map is non-uniform, so even a single codeword change
        // perturbs multiple bar runs).
        let want: [u8; 80] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1,
            1, 3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1,
            1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "code32(\"12345678\") bars regressed; \
             the `n * 2` weighted-sum arithmetic at line ~67 may have flipped to `n + 2`"
        );
    }

    /// Stage 11.A8c — pin `compute_check` Italian Pharmacode mod-10
    /// with hand-computed expected values for boundary inputs.
    /// Kills `% 10` / `* 2` / `>= 10` / `- 9` arithmetic mutations
    /// on lines 67-74.
    #[test]
    fn compute_check_known_values() {
        // Empty digits → sum=0, 0%10=0 → '0'.
        assert_eq!(compute_check(""), '0');

        // "0000000000" → all zeros, sum 0 → '0'.
        assert_eq!(compute_check("0000000000"), '0');

        // "1": single char, i=0 even, weighted=1, sum=1, 1%10=1 → '1'.
        assert_eq!(compute_check("1"), '1');

        // "5": i=0 even, n=5, weighted=5, sum=5, 5%10=5 → '5'.
        assert_eq!(compute_check("5"), '5');

        // "11": (i=0 1*1) + (i=1 1*2) = 1+2 = 3 → '3'.
        assert_eq!(compute_check("11"), '3');

        // "15": (1*1) + (5*2=10, sum digits = 1) = 1+1 = 2 → '2'.
        // Pins the >= 10 + sum-of-digits branch.
        assert_eq!(compute_check("15"), '2');

        // "99": (9*1) + (9*2=18, sum digits = 9) = 9+9 = 18, 18%10=8 → '8'.
        assert_eq!(compute_check("99"), '8');

        // "1234567": odd-length input.
        //   i=0: 1*1=1.
        //   i=1: 2*2=4.
        //   i=2: 3*1=3.
        //   i=3: 4*2=8.
        //   i=4: 5*1=5.
        //   i=5: 6*2=12, >=10 → 12-9=3.
        //   i=6: 7*1=7.
        //   sum = 1+4+3+8+5+3+7 = 31, 31%10=1 → '1'.
        assert_eq!(compute_check("1234567"), '1');
    }
}
