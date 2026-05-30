//! USPS PostNet and PLANET.
//!
//! Each digit is encoded as **5 bars**, two of which are "tall" (full
//! height) and three "short" (half height from the baseline). The symbol
//! is bracketed by a single tall bar at start and end. A mod-10 sum check
//! digit follows the data.
//!
//! Geometry (modelled with [`Bar4State`]):
//!   * "tall" (BWIPP code `5`) → [`Bar4State::Full`] — full-height bar.
//!   * "short" (BWIPP code `2`) → [`Bar4State::Descender`] — bar from the
//!     vertical midpoint to the baseline. In the standard the short bars
//!     are exactly half the tall height; our renderer uses a slightly
//!     taller "descender" geometry that still scans correctly and reads
//!     naturally next to 4-state postal codes.
//!
//! Patterns ported from bwip-js `bwipp_postnet` and `bwipp_planet`.

use crate::encoding::{Bar4State, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;

/// Per-digit 5-bar pattern for PostNet. `5` = tall, `2` = short.
const POSTNET_ENCS: [&str; 10] = [
    "55222", "22255", "22525", "22552", "25225", "25252", "25522", "52225", "52252", "52522",
];

/// Per-digit 5-bar pattern for PLANET (inverse-density of PostNet).
const PLANET_ENCS: [&str; 10] = [
    "22555", "55522", "55252", "55225", "52552", "52525", "52255", "25552", "25525", "25255",
];

/// Both PostNet and PLANET frame the data with a single tall bar.
const FRAME_BAR: char = '5';

/// Encode a USPS PostNet payload.
///
/// PostNet accepts the following digit-count variants per the USPS spec:
/// * 5 digits — ZIP
/// * 9 digits — ZIP+4
/// * 11 digits — ZIP+4 + 2-digit delivery point
///
/// In every case the encoder appends a mod-10 check digit, producing a
/// 6/10/12-bar-count symbol once the framing bars and check are added.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 9-digit ZIP+4.
/// let svg = render_svg(Symbology::Postnet, "555551237", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_postnet(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode_5bar(data, opts, &POSTNET_ENCS, "PostNet")
}

/// Encode a USPS PLANET payload.
///
/// PLANET accepts 11 digits (Confirm Service ID + tracking) or 13 digits
/// (with mailpiece ID); the encoder appends a mod-10 check.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Planet, "12345678901", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_planet(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    encode_5bar(data, opts, &PLANET_ENCS, "PLANET")
}

fn encode_5bar(
    data: &str,
    opts: &Options,
    encs: &[&str; 10],
    name: &'static str,
) -> Result<Postal4Pattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != data.chars().count() {
        return Err(Error::InvalidData(format!(
            "{name}: digits only (got {data:?})"
        )));
    }
    if digits.is_empty() {
        return Err(Error::InvalidData(format!(
            "{name}: payload must not be empty"
        )));
    }
    // The published PostNet variants are 5, 9, 11 digit data lengths (which
    // expand to 6/10/12 bars after the check). PLANET allows 11 or 13.
    // We accept any sensible length and let downstream scanners validate.
    if digits.len() > 30 {
        return Err(Error::InvalidData(format!(
            "{name}: payload too long ({} digits)",
            digits.len()
        )));
    }

    let check = sum_check(&digits);
    let full = format!("{digits}{check}");

    let mut bars = Vec::with_capacity(2 + full.len() * 5);
    push_bar(&mut bars, FRAME_BAR)?;
    for c in full.chars() {
        let d = c.to_digit(10).unwrap() as usize;
        for ch in encs[d].chars() {
            push_bar(&mut bars, ch)?;
        }
    }
    push_bar(&mut bars, FRAME_BAR)?;

    Ok(Postal4Pattern {
        bars,
        text: if opts.include_text { Some(full) } else { None },
    })
}

fn push_bar(out: &mut Vec<Bar4State>, height: char) -> Result<(), Error> {
    match height {
        '5' => out.push(Bar4State::Full),
        '2' => out.push(Bar4State::Descender),
        other => {
            return Err(Error::InvalidData(format!(
                "PostNet/PLANET pattern has unexpected height code {other:?}"
            )))
        }
    }
    Ok(())
}

/// PostNet / PLANET check digit: sum of digits, mod 10, complement.
fn sum_check(digits: &str) -> char {
    let mut sum: u32 = 0;
    for c in digits.chars() {
        sum += c.to_digit(10).unwrap();
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postnet_check_digit_known() {
        // ZIP "12345" -> sum = 1+2+3+4+5 = 15 -> check = (10-5)%10 = 5
        assert_eq!(sum_check("12345"), '5');
    }

    #[test]
    fn postnet_zip_5_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the PostNet ZIP-5 path: 5-digit body → check digit appended;
        // total 1 frame + 6 chars × 5 bars + 1 frame = 32 bars.
        let p = encode_postnet("12345", &Options::default()).expect(
            "encode_postnet(\"12345\", default) (PostNet ZIP-5 path: 5-digit body + auto-check; 1+6×5+1=32 bars) must succeed",
        );
        // 1 frame + 6 chars * 5 bars + 1 frame = 32 bars
        assert_eq!(p.bars.len(), 1 + 6 * 5 + 1);
        assert_eq!(p.bars.first().copied(), Some(Bar4State::Full));
        assert_eq!(p.bars.last().copied(), Some(Bar4State::Full));
    }

    #[test]
    fn postnet_zip_plus_4_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the PostNet ZIP+4 path: 9-digit body → 1+10×5+1=52 bars.
        let p = encode_postnet("123456789", &Options::default()).expect(
            "encode_postnet(\"123456789\", default) (PostNet ZIP+4 path: 9-digit body + auto-check; 1+10×5+1=52 bars) must succeed",
        );
        // 1 + 10*5 + 1 = 52 bars
        assert_eq!(p.bars.len(), 1 + 10 * 5 + 1);
    }

    #[test]
    fn postnet_rejects_letters() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 83-85 of postnet.rs:
        //   1. `PostNet:` symbology prefix (the encode_5bar helper's
        //      `name` arg is "PostNet" for postnet; "PLANET" for the
        //      planet sibling)
        //   2. `digits only` predicate (discriminates from the
        //      `payload must not be empty` and `payload too long`
        //      sibling arms at lines 87-90 and 95-99)
        //   3. `"ABC"` Debug echo of the offending payload (the
        //      `{data:?}` format renders the full string with quotes)
        match encode_postnet("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("PostNet:"), "missing `PostNet:` prefix: {msg}");
                assert!(
                    msg.contains("digits only"),
                    "missing `digits only` predicate: {msg}"
                );
                assert!(msg.contains("\"ABC\""), "missing \"ABC\" Debug echo: {msg}");
                assert!(
                    !msg.contains("payload must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into digits-only reject: {msg}"
                );
                assert!(
                    !msg.contains("payload too long"),
                    "wrong arm — too-long diagnostic leaked into digits-only reject: {msg}"
                );
                assert!(
                    !msg.contains("PLANET:"),
                    "wrong symbology — PLANET prefix leaked into PostNet diagnostic: {msg}"
                );
            }
            other => panic!("\"ABC\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn planet_check_digit_known() {
        // "123456789012" -> sum = 1+2+3+4+5+6+7+8+9+0+1+2 = 48
        // check = (10 - 48%10) % 10 = (10 - 8) % 10 = 2
        assert_eq!(sum_check("123456789012"), '2');
    }

    #[test]
    fn planet_12_digit_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the PLANET 11-digit path: 11-digit body + auto-check →
        // 1+12×5+1=62 bars.
        let p = encode_planet("12345678901", &Options::default()).expect(
            "encode_planet(\"12345678901\", default) (PLANET 11-digit path: 11-digit body + auto-check; 1+12×5+1=62 bars) must succeed",
        );
        // 1 + 12*5 + 1 = 62 bars (11 data + 1 check = 12 char)
        assert_eq!(p.bars.len(), 1 + 12 * 5 + 1);
    }

    #[test]
    fn postnet_and_planet_differ_on_digit_zero() {
        // PostNet "0" = "55222" (FFDDD); PLANET "0" = "22555" (DDFFF).
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the PostNet vs PLANET digit-0 inversion paths.
        let pn = encode_postnet("0", &Options::default()).expect(
            "encode_postnet(\"0\", default) (PostNet single-digit zero: FFDDD pattern \"55222\"; inverted from PLANET) must succeed",
        );
        let pl = encode_planet("0", &Options::default()).expect(
            "encode_planet(\"0\", default) (PLANET single-digit zero: DDFFF pattern \"22555\"; inverted from PostNet) must succeed",
        );
        assert_ne!(pn.bars, pl.bars);
    }

    /// Stage 11.A8c — pin the wrap-around case `sum_check` where
    /// `sum % 10 == 0`. The existing anchors hit non-wrap cases
    /// (12345 → check 5, 123456789012 → check 2), leaving the outer
    /// `% 10` fold mutation (`(10 - sum%10) % 10` → `10 - sum%10`)
    /// uncaught. The mutation would produce `char::from_digit(10, 10)`
    /// = None → panic at unwrap.
    ///
    /// Also pin both `push_bar` arms directly: '5' → Full, '2' →
    /// Descender, plus a third (invalid) height returning Err.
    #[test]
    fn sum_check_wraparound_and_push_bar_arms() {
        // sum_check("19"): sum=10, 10%10=0, (10-0)%10=0 → '0'.
        // Without the outer % 10 fold, the result would be 10 →
        // char::from_digit(10, 10) → None → panic.
        assert_eq!(sum_check("19"), '0', "sum=10 wrap → outer % 10 must fold");
        // sum_check("28"): symmetric: sum=10 → '0'.
        assert_eq!(sum_check("28"), '0');
        // sum_check("100"): sum=1, (10-1)%10=9 → '9'. Non-wrap anchor.
        assert_eq!(sum_check("100"), '9');
        // sum_check(""): empty → sum=0 → (10-0)%10=0 → '0'. Edge case.
        assert_eq!(sum_check(""), '0');

        // push_bar '5' → Full (height code BWIPP uses for "tall" bar).
        let mut bars = Vec::new();
        push_bar(&mut bars, '5').expect("'5' arm should succeed");
        assert_eq!(bars, vec![Bar4State::Full]);

        // push_bar '2' → Descender (height code for "short" bar).
        let mut bars = Vec::new();
        push_bar(&mut bars, '2').expect("'2' arm should succeed");
        assert_eq!(bars, vec![Bar4State::Descender]);

        // push_bar with any other height returns an Error::InvalidData.
        // Stage 11.A8c — upgrade from `matches!(_, InvalidData(_))` to
        // diagnostic substring + char-echo pins. The catch-all arm at
        // line 125-128 produces:
        //   "PostNet/PLANET pattern has unexpected height code '9'"
        //
        // Mutation: removing the `_ =>` arm or replacing it with Ok(())
        // would silently swallow invalid input (kills variant check);
        // a mutant that drops `{other:?}` echo or swaps the predicate
        // text survives the old assertion.
        // Stage 11.A8c (cont) — switch let-else → match so the failing
        // variant is echoed when push_bar's catch-all arm gets re-routed
        // to a non-InvalidData error variant by a mutation.
        let mut bars = Vec::new();
        let msg = match push_bar(&mut bars, '9').unwrap_err() {
            Error::InvalidData(m) => m,
            other => panic!(
                "push_bar(&mut bars, '9') must reject as InvalidData via the catch-all height-code arm; got {other:?}"
            ),
        };
        assert!(
            msg.contains("PostNet/PLANET"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("unexpected height code"),
            "diagnostic must call out 'unexpected height code'; got {msg:?}"
        );
        assert!(
            msg.contains("'9'"),
            "diagnostic must echo the offending char ({{other:?}}); got {msg:?}"
        );
        assert!(bars.is_empty(), "errored push_bar must NOT mutate output");
    }

    fn bar_string(bars: &[Bar4State]) -> String {
        bars.iter()
            .map(|b| match b {
                Bar4State::Full => 'F',
                Bar4State::Descender => 'D',
                Bar4State::Ascender => 'A',
                Bar4State::Tracker => 'T',
            })
            .collect()
    }

    /// Cross-validation against bwip-js: `b.raw("postnet", text, {})[0]`
    /// returns per-bar `bhs` (heights). Each bar is either `0.125`
    /// (tall = Full) or `0.05` (short = Descender). The classified
    /// sequence is anchored here.
    #[test]
    fn postnet_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            ("12345", "FDDDFFDDFDFDDFFDDFDDFDFDFDDFDFDF"),
            (
                "123456789",
                "FDDDFFDDFDFDDFFDDFDDFDFDFDDFFDDFDDDFFDDFDFDFDDDFDFDF",
            ),
            (
                "12345678901",
                "FDDDFFDDFDFDDFFDDFDDFDFDFDDFFDDFDDDFFDDFDFDFDDFFDDDDDDFFDFDDFF",
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the corpus row so a
            // PostNet bar-sequence regression points at the row.
            let p = encode_postnet(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode_postnet({text:?}, default) (PostNet bar-sequence corpus row, ZIP-5/+4/12-digit) must succeed: {e:?}",
                )
            });
            assert_eq!(
                bar_string(&p.bars),
                want,
                "PostNet bar sequence mismatch for {text:?}"
            );
        }
    }

    /// Kills both `> 30 -> >=` and `> 30 -> ==` mutants at line ~95 of
    /// `encode_5bar` (the `digits.len() > 30` payload cap).
    ///
    /// The original returns Err for `digits.len() > 30` (length ≥ 31).
    /// The `>= 30` mutant returns Err for length ≥ 30, so a length-30
    /// payload that the spec accepts is incorrectly rejected.
    /// The `== 30` mutant returns Err for length exactly 30 only, so
    /// a length-31 payload that the original rejects is incorrectly
    /// accepted.
    ///
    /// We pin both boundaries: a 30-digit payload must succeed and a
    /// 31-digit payload must error.
    #[test]
    fn payload_length_cap_is_strictly_thirty() {
        // Length 30: accepted (boundary). Kills `>= 30` mutant.
        let exactly_30 = "123456789012345678901234567890";
        assert_eq!(exactly_30.len(), 30);
        encode_postnet(exactly_30, &Options::default())
            .expect("30-digit payload should encode (boundary not yet hit)");

        // Length 31: rejected. Kills `== 30` mutant (which would accept).
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("too
        // long")` upgraded to 3-anchor pin:
        //   1. `PostNet:` symbology prefix
        //   2. `payload too long` full predicate
        //   3. `31 digits` value-echo (kills `digits.len() → 0` or
        //      hardcoded-literal mutations in the format string)
        let length_31 = "1234567890123456789012345678901";
        assert_eq!(length_31.len(), 31);
        match encode_postnet(length_31, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("PostNet:"),
                    "missing PostNet symbology prefix: {msg:?}"
                );
                assert!(
                    msg.contains("payload too long"),
                    "missing `payload too long` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("31 digits"),
                    "missing `31 digits` value-echo: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(too long), got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the wrap-around case for `sum_check` at line
    /// ~140 (`(10 - sum % 10) % 10`). The existing `postnet_check_digit_
    /// known` and `planet_check_digit_known` only hit cases where
    /// `sum % 10 != 0`, so the **outer** `% 10` (which folds 10 → 0
    /// when sum is a multiple of 10) was untested. The mutant
    /// `delete the outer % 10` (or `% 10 -> * 10` etc.) would produce
    /// `'\u{0a}'` instead of `'0'` for any sum that's a multiple of 10,
    /// and `char::from_digit(10, 10).unwrap()` would panic.
    ///
    /// Hand-computed:
    /// - "55" → sum = 10 → (10 - 10%10) = 10 → 10 % 10 = 0 → '0'.
    /// - "0" → sum = 0 → (10 - 0) = 10 → 10 % 10 = 0 → '0'.
    /// - "19" → sum = 10 → 0.
    /// - "5555555555" → sum = 50 → 0.
    #[test]
    fn sum_check_wraps_when_total_is_multiple_of_ten() {
        // The outer % 10 must fold 10 → 0 when sum % 10 == 0.
        assert_eq!(sum_check("55"), '0', "(10 - 10%10) %10 must be 0");
        assert_eq!(sum_check("0"), '0', "(10 - 0) %10 must be 0");
        assert_eq!(sum_check("19"), '0');
        assert_eq!(sum_check("5555555555"), '0', "sum=50 → check='0'");
        // Empty payload sums to 0 → check '0' as well.
        assert_eq!(sum_check(""), '0');
    }

    /// Stage 11.A8c — pin the `push_bar` match arms at line ~121.
    /// The encoder only ever calls `push_bar` with '5' or '2' (from
    /// the patterns + FRAME_BAR), so the error path is unreachable
    /// from the public API. Direct unit tests on the helper pin the
    /// '5' → Full, '2' → Descender, and other → InvalidData mappings.
    /// Kills `match arm deletion` and the `'5' -> Descender` /
    /// `'2' -> Full` swap mutants.
    #[test]
    fn push_bar_maps_height_codes() {
        let mut out: Vec<Bar4State> = Vec::new();
        push_bar(&mut out, '5').unwrap();
        assert_eq!(out, vec![Bar4State::Full], "'5' must map to Full");
        push_bar(&mut out, '2').unwrap();
        assert_eq!(
            out,
            vec![Bar4State::Full, Bar4State::Descender],
            "'2' must map to Descender"
        );
        // Any other char must error with the expected message tag.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("unexpected height code")` upgraded to 3-anchor pin:
        //   1. `PostNet/PLANET pattern` symbology prefix
        //   2. `unexpected height code` full predicate
        //   3. `'3'` char Debug echo (matches `{other:?}` form)
        // Stage 11.A8c (cont) — echo the actual returned Result variant
        // when the catch-all is taken, so a mutant that re-routes '3' to
        // Ok or to a non-InvalidData Err is named in the failure msg.
        let bad = push_bar(&mut out, '3');
        match bad {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("PostNet/PLANET pattern"),
                    "missing PostNet/PLANET pattern prefix: {msg:?}"
                );
                assert!(
                    msg.contains("unexpected height code"),
                    "expected 'unexpected height code' in error msg, got: {msg:?}"
                );
                assert!(msg.contains("'3'"), "missing char Debug echo '3': {msg:?}");
            }
            other => panic!(
                "push_bar(&mut out, '3') must Err with InvalidData via the height-code catch-all; got {other:?}"
            ),
        }
        // Stage 11.A8c — strengthen the previously-weak `is_err()` checks
        // to pin the offending-char substring in `{other:?}` AND anchor
        // sentinel adjacency. Kills:
        //   - `{other:?}` drop / replacement by a fixed char,
        //   - widening mutants like `'4' | '5' => Full` (would accept '4'),
        //   - widening mutants like `'1' | '2' => Descender` (would accept '1' / '3').
        for bad in ['0', '9', 'F', '1', '3', '4', '6', '/', ':'] {
            let err = push_bar(&mut out, bad).expect_err(&format!("push_bar({bad:?}) must reject"));
            let Error::InvalidData(msg) = err else {
                panic!("push_bar({bad:?}) must return InvalidData; got other error variant");
            };
            assert!(
                msg.contains("unexpected height code"),
                "diagnostic for {bad:?} must mention 'unexpected height code'; got {msg:?}"
            );
            let needle = format!("{bad:?}");
            assert!(
                msg.contains(&needle),
                "diagnostic for {bad:?} must contain its debug form {needle:?}; got {msg:?}"
            );
        }
    }

    /// Same shape of cross-validation for PLANET. The bar set is
    /// inverted vs PostNet (2 short + 3 tall per digit instead of
    /// 2 tall + 3 short), so the same `FFFDD` / `DDFFF` etc.
    /// classification distinguishes the encoders cleanly.
    #[test]
    fn planet_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            (
                "12345678901",
                "FFFFDDFFDFDFFDDFFDFFDFDFDFFDDFFDFFFDDFFDFDFDFFDDFFFFFFDDFDFFDF",
            ),
            (
                "1234567890123",
                "FFFFDDFFDFDFFDDFFDFFDFDFDFFDDFFDFFFDDFFDFDFDFFDDFFFFFFDDFFDFDFFDDFDFDFFF",
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the PLANET corpus row.
            let p = encode_planet(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode_planet({text:?}, default) (PLANET bar-sequence corpus row, 11/13-digit) must succeed: {e:?}",
                )
            });
            assert_eq!(
                bar_string(&p.bars),
                want,
                "PLANET bar sequence mismatch for {text:?}"
            );
        }
    }
}
