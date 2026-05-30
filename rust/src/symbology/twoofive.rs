//! The "2 of 5" family of linear barcodes.
//!
//! Six closely related variants — Standard (Code 2 of 5), Data Logic, IATA,
//! Industry, Matrix, and COOP. Each digit and the start/stop sentinels are
//! encoded as a *run-length* string in BWIPP's `code2of5_versions` table:
//! the digits alternate bar/space widths starting with a bar. Standard
//! variants (Industrial, IATA) emit 5 bars + 5 spaces per digit; Matrix,
//! COOP, and Data Logic emit 3 bars + 3 spaces.
//!
//! All run-length strings ported verbatim from `bwipp_code2of5`,
//! `bwipp_industrial2of5`, `bwipp_matrix2of5`, `bwipp_coop2of5` (and
//! Standard/Data Logic, sharing the Industrial/Matrix tables respectively).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// One variant's run-length tables: 10 digit encodings followed by start
/// and stop. Each string alternates bar/space widths starting with bar.
struct Variant {
    digits: [&'static str; 10],
    start: &'static str,
    stop: &'static str,
    name: &'static str,
}

#[rustfmt::skip]
const INDUSTRIAL: Variant = Variant {
    digits: [
        "1111313111", "3111111131", "1131111131", "3131111111", "1111311131",
        "3111311111", "1131311111", "1111113131", "3111113111", "1131113111",
    ],
    start: "313111",
    stop: "31113",
    name: "Code 2 of 5 (Industrial)",
};

// Standard 2 of 5 (Code 2 of 5) shares its digit/start/stop with Industrial
// (BWIPP's `code2of5 version=industrial`).
const STANDARD: Variant = INDUSTRIAL;

#[rustfmt::skip]
const IATA: Variant = Variant {
    digits: [
        "1111313111", "3111111131", "1131111131", "3131111111", "1111311131",
        "3111311111", "1131311111", "1111113131", "3111113111", "1131113111",
    ],
    start: "1111",
    stop: "311",
    name: "Code 2 of 5 IATA",
};

#[rustfmt::skip]
const MATRIX: Variant = Variant {
    digits: [
        "113311", "311131", "131131", "331111", "113131",
        "313111", "133111", "111331", "311311", "131311",
    ],
    start: "311111",
    stop: "31111",
    name: "Code 2 of 5 Matrix",
};

#[rustfmt::skip]
const COOP: Variant = Variant {
    digits: [
        "331111", "111331", "113131", "113311", "131131",
        "131311", "133111", "311131", "311311", "313111",
    ],
    start: "3131",
    stop: "133",
    name: "Code 2 of 5 COOP",
};

#[rustfmt::skip]
const DATALOGIC: Variant = Variant {
    digits: [
        "113311", "311131", "131131", "331111", "113131",
        "313111", "133111", "111331", "311311", "131311",
    ],
    start: "1111",
    stop: "311",
    name: "Code 2 of 5 Data Logic",
};

/// Append `enc`'s run-length digits into `bars`, alternating bar/space
/// starting from whichever polarity `bars` would continue with.
fn append_enc(bars: &mut Vec<u8>, enc: &str) {
    for c in enc.chars() {
        bars.push(c.to_digit(10).unwrap() as u8);
    }
}

/// GS1 mod-10 weighted check: rightmost digit gets weight 3, then 1,
/// 3, 1 …; sum modulo 10, complement to 10. Matches BWIPP's
/// `code2of5` check loop and is also what ITF-14 / EAN-13 use.
fn gs1_check(digits: &str) -> char {
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let n = c.to_digit(10).unwrap();
        sum += if i % 2 == 0 { n * 3 } else { n };
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

fn encode_variant(data: &str, opts: &Options, v: &Variant) -> Result<LinearPattern, Error> {
    let mut digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != data.chars().count() {
        return Err(Error::InvalidData(format!(
            "{} accepts digits only",
            v.name
        )));
    }
    if digits.is_empty() {
        return Err(Error::InvalidData(format!(
            "{} payload must not be empty",
            v.name
        )));
    }
    // `includecheck = "true"` appends the GS1 mod-10 weighted check
    // digit (×3, ×1 alternating from the right) — same algorithm as
    // ITF-14 and what BWIPP's `code2of5` (and every variant that
    // shares its check loop) emits.
    if opts.get("includecheck").is_some_and(|v| v == "true") {
        digits.push(gs1_check(&digits));
    }

    let mut bars: Vec<u8> = Vec::new();
    append_enc(&mut bars, v.start);
    for c in digits.chars() {
        let d = c.to_digit(10).unwrap() as usize;
        append_enc(&mut bars, v.digits[d]);
    }
    append_enc(&mut bars, v.stop);

    let text = if opts.include_text {
        Some(digits)
    } else {
        None
    };
    Ok(LinearPattern { bars, text })
}

/// Encode Code 2 of 5 (Standard / Industrial).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Non-interleaved 2 of 5: each digit becomes a 5-bar pattern; bars + spaces
/// // are emitted individually rather than packed two-digits-per-character like
/// // Interleaved 2 of 5 does. The Data Logic / IATA / Industrial / Matrix /
/// // COOP variants are sister encoders with different start/stop sentinels.
/// let svg = render_svg(Symbology::Code2of5, "12345", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_standard(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &STANDARD)
}

/// Encode Data Logic 2 of 5.
pub fn encode_datalogic(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &DATALOGIC)
}

/// Encode IATA 2 of 5.
pub fn encode_iata(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &IATA)
}

/// Encode Industrial 2 of 5.
pub fn encode_industrial(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &INDUSTRIAL)
}

/// Encode COOP 2 of 5.
pub fn encode_coop(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &COOP)
}

/// Encode Matrix 2 of 5.
pub fn encode_matrix(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_variant(data, opts, &MATRIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rejects_non_digits() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 109-112 of
        // twoofive.rs:
        //   1. `Code 2 of 5 (Industrial)` variant-name prefix (because
        //      STANDARD aliases INDUSTRIAL at line 40 — pinning this
        //      kills a mutation that swaps the alias to a different
        //      variant, e.g. IATA or Matrix)
        //   2. `accepts digits only` predicate (discriminates from
        //      the `payload must not be empty` sibling at line 114-118)
        match encode_standard("12A45", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 2 of 5 (Industrial)"),
                    "missing `Code 2 of 5 (Industrial)` variant-name prefix (STANDARD aliases INDUSTRIAL): {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "missing `accepts digits only` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into digits-only reject: {msg}"
                );
                // Cross-variant contamination guards: ensure
                // encode_standard hasn't been mis-aliased to a
                // different variant (IATA, Matrix, COOP, Data Logic).
                assert!(
                    !msg.contains("IATA"),
                    "wrong variant alias — IATA leaked into Standard diagnostic: {msg}"
                );
                assert!(
                    !msg.contains("Matrix"),
                    "wrong variant alias — Matrix leaked into Standard diagnostic: {msg}"
                );
                assert!(
                    !msg.contains("COOP"),
                    "wrong variant alias — COOP leaked into Standard diagnostic: {msg}"
                );
                assert!(
                    !msg.contains("Data Logic"),
                    "wrong variant alias — Data Logic leaked into Standard diagnostic: {msg}"
                );
            }
            other => panic!("\"12A45\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn every_variant_renders() {
        // Stage 11.A8c (cont) — bare `assert!(p.total_width() > 0)` in
        // a loop iterating over 6 variant encoders gives no info on
        // WHICH variant returned empty bars. Strengthen by naming
        // each variant via debug echo of the function pointer's
        // variant tag (use named tuples).
        let variants: &[(
            &str,
            fn(
                &str,
                &crate::options::Options,
            ) -> Result<crate::encoding::LinearPattern, crate::error::Error>,
        )] = &[
            ("encode_standard (alias for INDUSTRIAL)", encode_standard),
            ("encode_datalogic (Data Logic 2 of 5)", encode_datalogic),
            ("encode_iata (IATA 2 of 5)", encode_iata),
            (
                "encode_industrial (Industrial / Standard)",
                encode_industrial,
            ),
            ("encode_coop (COOP 2 of 5)", encode_coop),
            ("encode_matrix (Matrix 2 of 5)", encode_matrix),
        ];
        for &(name, f) in variants {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the variant + payload so
            // a regression points at which 2-of-5 family member broke.
            let p = f("12345", &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "{name}(\"12345\", default) (2-of-5 family variant smoke: 5-digit payload) must succeed: {e:?}",
                )
            });
            assert!(
                p.total_width() > 0,
                "{name}(\"12345\") (5-digit 2-of-5 family payload) must compose into non-empty symbol; got total_width={}",
                p.total_width()
            );
        }
    }

    #[test]
    fn variants_produce_different_outputs() {
        // The whole point of having multiple variants is that their bar
        // patterns differ. Confirm two arbitrary variants disagree on the
        // same input.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the 2-of-5 family Standard-vs-IATA distinctness path.
        let s = encode_standard("12345", &Options::default()).expect(
            "encode_standard(\"12345\", default) (2-of-5 family Standard variant for family-distinctness cross-check) must succeed",
        );
        let i = encode_iata("12345", &Options::default()).expect(
            "encode_iata(\"12345\", default) (2-of-5 family IATA variant; must differ from Standard) must succeed",
        );
        assert_ne!(s.bars, i.bars);
    }

    /// Goldens from `bwipp_code2of5` (all five `version`s plus Standard
    /// which shares Industrial's table). Each is the run-length output
    /// of `raw("code2of5", "12345", {version: "<v>"})[0].sbs`.
    #[test]
    fn variants_match_bwip_js_raw_sbs() {
        type Variant = (
            &'static str,
            fn(&str, &Options) -> Result<LinearPattern, Error>,
            &'static [u8],
        );
        let cases: &[Variant] = &[
            (
                "industrial",
                encode_industrial,
                &[
                    3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1,
                    3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1,
                    1, 1, 1, 1, 3, 1, 1, 1, 3,
                ],
            ),
            (
                "iata",
                encode_iata,
                &[
                    1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1,
                    3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1,
                    1, 1, 3, 1, 1,
                ],
            ),
            (
                "matrix",
                encode_matrix,
                &[
                    3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1,
                    3, 1, 3, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1,
                ],
            ),
            (
                "coop",
                encode_coop,
                &[
                    3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1,
                    3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 3,
                ],
            ),
            (
                "datalogic",
                encode_datalogic,
                &[
                    1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1,
                    3, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1,
                ],
            ),
        ];
        for &(name, f, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration variant name + input echo.
            let got = f("12345", &Options::default()).unwrap_or_else(|e| {
                panic!("{name} 2-of-5 encode(\"12345\") (default opts) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars.as_slice(),
                want,
                "{name} 2-of-5 bars mismatch vs bwip-js raw output"
            );
        }
    }

    /// Per-variant goldens for the `includecheck: true` path. The
    /// check digit is the GS1 mod-10 weighted check (×3, ×1
    /// alternating from the right). Without `includecheck` support
    /// the encoder was silently dropping the check digit option.
    #[test]
    fn variants_match_bwip_js_with_check() {
        type Variant = (
            &'static str,
            fn(&str, &Options) -> Result<LinearPattern, Error>,
            &'static [u8],
        );
        let cases: &[Variant] = &[
            (
                "industrial",
                encode_industrial,
                &[
                    3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1,
                    3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1,
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 1, 1, 3,
                ],
            ),
            (
                "iata",
                encode_iata,
                &[
                    1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1,
                    3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1,
                    1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 1,
                ],
            ),
            (
                "matrix",
                encode_matrix,
                &[
                    3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1,
                    3, 1, 3, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 1,
                ],
            ),
            (
                "coop",
                encode_coop,
                &[
                    3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1,
                    3, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 3,
                ],
            ),
            (
                "datalogic",
                encode_datalogic,
                &[
                    1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1,
                    3, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1,
                ],
            ),
        ];
        let opts = Options::default().with("includecheck", "true");
        for &(name, f, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration variant name + includecheck=true label.
            let got = f("12345", &opts).unwrap_or_else(|e| {
                panic!("{name} 2-of-5 encode(\"12345\", includecheck=true) (GS1 mod-10 check path) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars.as_slice(),
                want,
                "{name} 2-of-5 (with check) bars mismatch vs bwip-js raw output"
            );
        }
    }

    /// Kills the two `% with /` mutants on lines ~101 and ~103 of
    /// `gs1_check` (the weight selector and the final mod-10
    /// complement). The pre-existing corpus uses `"12345"`, which is
    /// a coincidence-resistant input: under both the
    /// `i % 2 == 0` → `i / 2 == 0` mutant on line 101 and the
    /// `sum % 10` → `sum / 10` mutant on line 103 the check digit
    /// happens to land on the same value (7) for that input. We pin
    /// the check digit for a *different* input where each mutant
    /// produces a distinct result, anchoring both formulas:
    ///
    /// gs1_check("13245"):
    ///   rev = [5, 4, 2, 3, 1]
    ///   original: 5·3 + 4·1 + 2·3 + 3·1 + 1·3 = 31 → (10 - 1) % 10 = 9
    ///   `i / 2 == 0` mutant: 5·3 + 4·3 + 2·1 + 3·1 + 1·1 = 33 → check = 7
    ///   `sum / 10` mutant   : 31, (10 - 31/10) % 10 = (10 - 3) % 10 = 7
    ///
    /// Both mutants land on '7'; the original lands on '9'. A direct
    /// equality on the returned char distinguishes all three states.
    #[test]
    fn gs1_check_distinguishes_weight_and_modulo_arithmetic() {
        assert_eq!(
            gs1_check("13245"),
            '9',
            "gs1_check('13245') should be 9; mutation in weight (i%2==0) \
             or mod-10 (sum%10) collapses it to '7'"
        );
    }

    /// Stage 11.A8c — pin the outer `% 10` fold on line ~103
    /// (`(10 - sum % 10) % 10`). The above `13245` and `12345` cases
    /// have `sum % 10 != 0` (1 and 3 respectively), so the outer
    /// `% 10` is a no-op — the mutant `drop the outer % 10` would
    /// produce 10 only when `sum` is a multiple of 10, and
    /// `from_digit(10, 10).unwrap()` panics. We pin the fold direction
    /// with hand-computed multiples-of-10 inputs.
    ///
    /// Hand-computed:
    ///   - "0" → rev="0", i=0 (0*3=0), sum=0, (10-0)%10=0 → '0'.
    ///   - "55" → rev="55": 5*3+5*1=20, (10-0)%10=0 → '0'. Outer wrap!
    ///   - "999" → rev="999": 9*3+9*1+9*3=63, (10-3)%10=7 → '7'.
    ///   - "100020" → rev="020001":
    ///       0*3+2*1+0*3+0*1+0*3+1*1 = 3, (10-3)%10=7 → '7'.
    ///   - "5005" → rev="5005": 5*3+0*1+0*3+5*1=20, (10-0)%10=0 → '0'.
    #[test]
    fn gs1_check_outer_mod_ten_folds_to_zero() {
        assert_eq!(gs1_check("0"), '0', "sum=0 → (10-0)%10 must fold to '0'");
        assert_eq!(gs1_check("55"), '0', "sum=20 → outer %10 fold");
        assert_eq!(gs1_check("999"), '7');
        assert_eq!(gs1_check("100020"), '7');
        assert_eq!(gs1_check("5005"), '0', "sum=20 → outer %10 fold");
        // Empty input → sum=0 → check '0'.
        assert_eq!(gs1_check(""), '0');
    }

    /// `append_enc(bars, enc)` parses each ASCII digit char in `enc`
    /// and pushes it as a `u8` onto `bars` in source order. Used by
    /// every twoofive variant (standard, datalogic, iata, industrial,
    /// coop, matrix) when stitching start/digit/stop encoding strings
    /// into the final bars vector. Never directly tested.
    ///
    /// Mutations to catch:
    /// * `to_digit(10)` → other base (e.g. 16 would accept 'A'..='F').
    /// * `bars.push(...)` → `bars.insert(0, ...)` (reverses order).
    /// * Iteration direction `pat.chars()` → `pat.chars().rev()`.
    /// * `as u8` cast preserved (digits 0..=9 always fit).
    /// * Accumulator semantics (preserves prior entries — must `push`,
    ///   not `clear` and rewrite).
    #[test]
    fn append_enc_pushes_digits_in_source_order() {
        // ---- Single-digit string into empty bars.
        let mut bars = vec![];
        append_enc(&mut bars, "0");
        assert_eq!(bars, vec![0], "single '0' → [0]");

        let mut bars = vec![];
        append_enc(&mut bars, "9");
        assert_eq!(bars, vec![9], "single '9' → [9]");

        // ---- Multi-digit string preserves source order.
        let mut bars = vec![];
        append_enc(&mut bars, "1234");
        assert_eq!(
            bars,
            vec![1, 2, 3, 4],
            "1234 → [1, 2, 3, 4] (forward iteration)"
        );

        // ---- Accumulator semantics: appends to existing bars.
        let mut bars = vec![9];
        append_enc(&mut bars, "12");
        assert_eq!(
            bars,
            vec![9, 1, 2],
            "prior [9] preserved + appended [1, 2] = [9, 1, 2]"
        );

        // ---- Empty pattern is a no-op.
        let mut bars = vec![5, 7];
        append_enc(&mut bars, "");
        assert_eq!(bars, vec![5, 7], "empty pattern preserves bars");

        // ---- Repeated digits — exercise the loop more than once.
        let mut bars = vec![];
        append_enc(&mut bars, "11122");
        assert_eq!(
            bars,
            vec![1, 1, 1, 2, 2],
            "repeated digits all pushed in source order"
        );

        // ---- Order discriminator: a palindrome catches no-op
        // direction mutations on its own, but an asymmetric string
        // (e.g. "1230") catches the `rev()` mutant explicitly.
        let mut bars = vec![];
        append_enc(&mut bars, "1230");
        assert_eq!(
            bars,
            vec![1, 2, 3, 0],
            "asymmetric '1230' → [1, 2, 3, 0]; reverse mutant would be [0, 3, 2, 1]"
        );

        // ---- Sequence: two append_enc calls accumulate correctly
        // (catches mutants that overwrite instead of push).
        let mut bars = vec![];
        append_enc(&mut bars, "12");
        append_enc(&mut bars, "34");
        assert_eq!(
            bars,
            vec![1, 2, 3, 4],
            "two calls accumulate: [1, 2] + [3, 4] = [1, 2, 3, 4]"
        );
    }
}
