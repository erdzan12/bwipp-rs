//! Pharmacode (One-Track) and Pharmacode Two-Track.
//!
//! Pharmacode One-Track represents a positive integer in `3..=131070` as a
//! sequence of thin (1-module) and thick (3-module) bars separated by
//! 2-module spaces. The encoding is `(value + 1)` in binary with the
//! leading `1` bit dropped; each remaining bit is emitted MSB-first as
//! one bar (`0` → thin, `1` → thick).
//!
//! Pharmacode Two-Track (`pharmacode2`) encodes the same value but each
//! bar carries vertical information instead of width: descender (bottom
//! half), ascender (top half), or full. We model it as a
//! [`Postal4Pattern`] with [`Bar4State::Descender`] / [`Bar4State::Ascender`]
//! / [`Bar4State::Full`] (Tracker is unused). Algorithm matches BWIPP's
//! `pharmacode2_base3sub` / `pharmacode2_base3map` / `pharmacode2_htmult`
//! tables byte-for-byte.
//!
//! Reference: BWIPP `pharmacode.ps.src` and `pharmacode2.ps.src`.

use crate::encoding::{Bar4State, LinearPattern, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;

/// Encode Pharmacode One-Track. The payload is an integer in `3..=131070`.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Pharmacode, "12345", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_one_track(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let value: u32 = parse_int(data, 3, 131_070, "Pharmacode One-Track")?;

    // BWIPP's algorithm: convert `(value + 1)` to binary, drop the
    // leading `1` (always present since adding 1 carries past the
    // top bit), then emit each remaining bit MSB-first as one bar —
    // `0` → narrow (width 1), `1` → wide (width 3). Bars are
    // separated by a single space of width `swidth = 2`.
    let bits = format!("{:b}", value + 1);
    let bits = &bits[1..]; // drop the leading '1'

    // Each bar gets a trailing `swidth = 2` space, including the
    // last one (BWIPP writes `sbs = [bar, space] * barlen`).
    let mut runs: Vec<u8> = Vec::new();
    for c in bits.chars() {
        runs.push(if c == '1' { 3 } else { 1 });
        runs.push(2);
    }

    let text = if opts.include_text {
        Some(value.to_string())
    } else {
        None
    };
    Ok(LinearPattern { bars: runs, text })
}

/// Encode Pharmacode Two-Track. The payload is an integer in
/// `4..=64570080` (BWIPP's stated range). Output is one
/// [`Bar4State`] per ternary digit, MSB-first:
///
///   * **Descender** — bar in the lower half of the symbol;
///   * **Ascender**  — bar in the upper half;
///   * **Full**      — bar spanning both halves.
///
/// [`Bar4State::Tracker`] is unused for Two-Track Pharmacode.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Pharmacode2, "100", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_two_track(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let value: u32 = parse_int(data, 4, 64_570_080, "Pharmacode Two-Track")?;

    // BWIPP's exact encoding: the residue `r = v % 3` picks a bar phase
    // via `base3map[r]` and the next `v` is `(v - base3sub[r]) / 3`.
    // Digits are emitted MSB-first by filling a fixed-size buffer from
    // the right and then taking the tail. Mirrors `bwipp_pharmacode2`.
    const BASE3_SUB: [u32; 3] = [3, 1, 2];
    const BASE3_MAP: [u8; 3] = [2, 0, 1]; // 0=D, 1=A, 2=F (see Bar4State below)

    let mut digits_rev: Vec<u8> = Vec::with_capacity(16);
    let mut v = value;
    while v > 0 {
        let r = (v % 3) as usize;
        v = (v - BASE3_SUB[r]) / 3;
        digits_rev.push(BASE3_MAP[r]);
    }
    digits_rev.reverse(); // MSB-first

    let bars: Vec<Bar4State> = digits_rev
        .into_iter()
        .map(|d| match d {
            0 => Bar4State::Descender,
            1 => Bar4State::Ascender,
            2 => Bar4State::Full,
            _ => unreachable!("BASE3_MAP only produces 0..=2"),
        })
        .collect();

    let text = if opts.include_text {
        Some(value.to_string())
    } else {
        None
    };
    Ok(Postal4Pattern { bars, text })
}

fn parse_int(data: &str, min: u32, max: u32, name: &str) -> Result<u32, Error> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidData(format!("{name}: payload is empty")));
    }
    let value: u32 = trimmed.parse().map_err(|_| {
        Error::InvalidData(format!(
            "{name}: payload {trimmed:?} is not a positive integer"
        ))
    })?;
    if value < min || value > max {
        return Err(Error::InvalidData(format!(
            "{name}: value must be in [{min}, {max}] (got {value})"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_track_minimum_value_three() {
        // value=3 → (3+1) = 4 = 0b100, drop leading 1 → "00" →
        // bars [narrow, swidth, narrow, swidth] = [1, 2, 1, 2].
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Pharmacode One-Track minimum-value path: value=3 →
        // (3+1)=4=0b100 → drop leading-1 → "00" → 4-bar [1,2,1,2].
        let p = encode_one_track("3", &Options::default()).expect(
            "encode_one_track(\"3\", default) (Pharmacode One-Track min value 3 boundary; (3+1)=0b100 drop-leading-1 → \"00\" → bars [1,2,1,2]) must succeed",
        );
        assert_eq!(p.bars, vec![1, 2, 1, 2]);
    }

    #[test]
    fn one_track_value_four() {
        // value=4 → (4+1) = 5 = 0b101, drop leading 1 → "01" →
        // bars [narrow, swidth, wide, swidth] = [1, 2, 3, 2].
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Pharmacode One-Track value-4 narrow/wide-bit path:
        // (4+1)=0b101 → "01" → bars [1,2,3,2] = narrow + swidth + wide.
        let p = encode_one_track("4", &Options::default()).expect(
            "encode_one_track(\"4\", default) (Pharmacode One-Track value=4; (4+1)=0b101 drop-leading-1 → \"01\" → bars [1,2,3,2] narrow/wide bit distinction) must succeed",
        );
        assert_eq!(p.bars, vec![1, 2, 3, 2]);
    }

    #[test]
    fn one_track_rejects_below_minimum() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 126-128
        // (`Pharmacode One-Track: value must be in [3, 131070] (got 2)`).
        // Cross-arm guard against the empty-payload + non-numeric arms.
        match encode_one_track("2", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Pharmacode One-Track:"),
                    "missing `Pharmacode One-Track:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be in [3, 131070]"),
                    "missing range hint `[3, 131070]`: {msg}"
                );
                assert!(
                    msg.contains("got 2"),
                    "missing offending-value echo `got 2`: {msg}"
                );
                assert!(
                    !msg.contains("payload is empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("not a positive integer"),
                    "wrong arm — non-numeric diagnostic leaked: {msg}"
                );
            }
            other => panic!("`2` (below min) should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn one_track_rejects_above_maximum() {
        // Stage 11.A8c — sibling of `one_track_rejects_below_minimum`;
        // pins the above-max arm (131071 = max+1).
        match encode_one_track("131071", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Pharmacode One-Track:"),
                    "missing `Pharmacode One-Track:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be in [3, 131070]"),
                    "missing range hint `[3, 131070]`: {msg}"
                );
                assert!(
                    msg.contains("got 131071"),
                    "missing offending-value echo `got 131071` (max+1): {msg}"
                );
                assert!(
                    !msg.contains("not a positive integer"),
                    "wrong arm — non-numeric diagnostic leaked: {msg}"
                );
            }
            other => panic!("`131071` (above max) should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn one_track_rejects_non_numeric() {
        // Stage 11.A8c — upgrade to 4-anchor pin matching the
        // non-numeric source diagnostic at line 120-124
        // (`Pharmacode One-Track: payload "hello" is not a positive
        // integer`). Cross-arm guard against the range arm.
        match encode_one_track("hello", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Pharmacode One-Track:"),
                    "missing `Pharmacode One-Track:` prefix: {msg}"
                );
                assert!(
                    msg.contains("not a positive integer"),
                    "missing non-numeric predicate: {msg}"
                );
                assert!(
                    msg.contains("\"hello\""),
                    "missing payload echo `\"hello\"`: {msg}"
                );
                assert!(
                    !msg.contains("must be in"),
                    "wrong arm — range diagnostic leaked into non-numeric arm: {msg}"
                );
            }
            other => panic!("`hello` should reject as non-numeric InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn two_track_minimum_value_four() {
        // value=4: bwip-js classifies as "DD" — two descender bars.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Pharmacode Two-Track minimum value path: value=4 →
        // 2 Descender bars (cross-validated against bwip-js).
        let p = encode_two_track("4", &Options::default()).expect(
            "encode_two_track(\"4\", default) (Pharmacode Two-Track min value 4 boundary; bwip-js classifies as \"DD\" = 2 Descender bars) must succeed",
        );
        assert_eq!(p.bars, vec![Bar4State::Descender, Bar4State::Descender]);
    }

    #[test]
    fn two_track_rejects_below_minimum() {
        // Stage 11.A8c — 4-anchor pin matching the Two-Track range
        // diagnostic (`Pharmacode Two-Track: value must be in
        // [4, 64570080] (got 3)`), with cross-symbology guard against
        // the One-Track family.
        match encode_two_track("3", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Pharmacode Two-Track:"),
                    "missing `Pharmacode Two-Track:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be in [4, 64570080]"),
                    "missing range hint `[4, 64570080]`: {msg}"
                );
                assert!(
                    msg.contains("got 3"),
                    "missing offending-value echo `got 3`: {msg}"
                );
                assert!(
                    !msg.contains("Pharmacode One-Track"),
                    "cross-symbology contamination: Two-Track arm mentions One-Track: {msg}"
                );
            }
            other => {
                panic!("`3` (Two-Track below min) should reject as InvalidData, got {other:?}")
            }
        }
    }

    /// Pharmacode Two-Track bar sequences classified from
    /// `b.raw("pharmacode2", text, {})` by mapping
    /// `(bhs, bbs)`-pair to D/A/F:
    ///
    ///   * `bhs ≈ 0.315`, `bbs = 0`  → Full
    ///   * `bhs ≈ 0.157`, `bbs = 0`  → Descender
    ///   * `bhs ≈ 0.157`, `bbs > 0`  → Ascender
    ///
    /// Covers minimum, mid-range, and a span up to the 5-digit
    /// upper end so the LSB/MSB ordering is fixed.
    #[test]
    fn two_track_matches_bwip_js() {
        fn dafstr(bars: &[Bar4State]) -> String {
            bars.iter()
                .map(|b| match b {
                    Bar4State::Descender => 'D',
                    Bar4State::Ascender => 'A',
                    Bar4State::Full => 'F',
                    Bar4State::Tracker => 'T',
                })
                .collect()
        }
        let cases: &[(&str, &str)] = &[
            ("4", "DD"),
            ("5", "DA"),
            ("100", "FDFD"),
            ("1234", "DDAFDFD"),
            ("16384", "DFFFFFADD"),
            ("64570", "AFADDDFDDD"),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`
            // with per-iteration input echo.
            let p = encode_two_track(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode_two_track({text:?}) (Pharmacode Two-Track corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                dafstr(&p.bars),
                want,
                "pharmacode2 bar sequence mismatch for {text:?}"
            );
        }
    }

    /// Pharmacode One-Track golden from
    /// `raw("pharmacode", "117", {})[0].sbs`.
    #[test]
    fn one_track_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Pharmacode One-Track byte-for-byte 12-bar SBS oracle.
        let p = encode_one_track("117", &Options::default()).expect(
            "encode_one_track(\"117\", default) (Pharmacode One-Track byte-for-byte 12-bar SBS bwip-js raw oracle) must succeed",
        );
        let want: [u8; 12] = [3, 2, 3, 2, 1, 2, 3, 2, 3, 2, 1, 2];
        assert_eq!(
            p.bars, want,
            "pharmacode bars mismatch vs bwip-js raw output"
        );
    }

    /// Kills `parse_int: replace > with >=` at line ~125. The existing
    /// `one_track_rejects_above_maximum` test only checks
    /// `max + 1` (131071); the mutant `value >= max` would also reject
    /// `value == max` (131070), which is the accepted boundary. We pin
    /// both `max - 1` (accept) and `max` (accept) and `max + 1`
    /// (reject), so the boundary is bracketed in all directions.
    #[test]
    fn one_track_accepts_exactly_max_value() {
        // value == 131070 — must accept (the upper bound is inclusive).
        encode_one_track("131070", &Options::default()).expect("max value 131070 must encode");
        // value == 131069 — must accept (just under the bound).
        encode_one_track("131069", &Options::default()).expect("131069 must encode");
        // value == 3 — must accept (the lower bound is also inclusive).
        encode_one_track("3", &Options::default()).expect("min value 3 must encode");
    }

    /// Companion of the test above for the `two_track` decoder, which
    /// uses parse_int with a different (max=64_570_080) bound.
    #[test]
    fn two_track_accepts_exactly_max_value() {
        encode_two_track("64570080", &Options::default()).expect("max value 64570080 must encode");
        // Stage 11.A8c — pin the max+1 rejection with the same anchors
        // as `two_track_rejects_below_minimum`. Both boundary
        // direction sites now strictly assert the diagnostic, so a
        // mutant that drops only one arm of the `value < min ||
        // value > max` check would be caught.
        match encode_two_track("64570081", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Pharmacode Two-Track:"),
                    "missing `Pharmacode Two-Track:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be in [4, 64570080]"),
                    "missing range hint `[4, 64570080]`: {msg}"
                );
                assert!(
                    msg.contains("got 64570081"),
                    "missing offending-value echo `got 64570081` (max+1): {msg}"
                );
            }
            other => panic!(
                "`64570081` (Two-Track above max) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// Stage 11.A8c — pin `parse_int` boundary behavior. Kills
    /// `< min` / `> max` boundary mutations and the empty-trim
    /// rejection on lines 116-129.
    #[test]
    fn parse_int_boundaries() {
        // In-range value accepted.
        assert_eq!(parse_int("5", 1, 10, "test").unwrap(), 5);
        // Exact min and max accepted (the `< min` and `> max` are
        // strict, so equality passes).
        assert_eq!(parse_int("1", 1, 10, "test").unwrap(), 1);
        assert_eq!(parse_int("10", 1, 10, "test").unwrap(), 10);
        // Stage 11.A8c — upgrade these range-arm checks to pin the
        // range-bound diagnostic + value echo. parse_int's range arm
        // at line 125-129 produces:
        //   "{name}: value must be in [min, max] (got V)"
        for (input, want_value) in [("0", "0"), ("11", "11")] {
            let err = parse_int(input, 1, 10, "test").unwrap_err();
            let Error::InvalidData(msg) = err else {
                panic!("parse_int({input:?}) must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("test:")
                    && msg.contains("must be in [1, 10]")
                    && msg.contains(&format!("(got {want_value})")),
                "range arm for {input:?} must pin name prefix + [1, 10] bound + value echo; \
                 got {msg:?}"
            );
        }
        // Whitespace trimmed.
        assert_eq!(parse_int("  5  ", 1, 10, "test").unwrap(), 5);
        // Empty rejected with the line-118 diagnostic:
        //   "{name}: payload is empty"
        // 3-anchor pin upgrades the previous single-substring check
        // (which would accept any message merely mentioning "empty"):
        let err = parse_int("", 1, 10, "test").unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("test:"),
                    "empty diagnostic must echo `{{name}}` ('test'); got {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "empty diagnostic must carry the predicate; got {msg}"
                );
                // Cross-arm contamination guard: empty arm must NOT
                // pick up the range-arm wording (commit 5eab0e8).
                assert!(
                    !msg.contains("must be in"),
                    "empty diagnostic must NOT leak the range-arm wording; got {msg}"
                );
            }
            // Stage 11.A8c — input + variant echo so a regression
            // pinpoints what variant fired in place of InvalidData.
            other => panic!(
                "empty `parse_int(\"\", 1, 10, \"test\")` should yield InvalidData, got {other:?}"
            ),
        }
        // Whitespace-only also rejected as empty.
        //
        // Stage 11.A8c — upgrade from matches!(_, InvalidData(_)) to pin
        // the same "empty" substring as the truly-empty case above. A
        // mutant that:
        //   * skips the `trim()` before the `is_empty()` check
        //     (so "   " falls through to integer-parse and surfaces
        //     a different error message),
        //   * routes whitespace-only through a different arm,
        // would survive the variant-only assertion. Pinning "empty"
        // proves both `""` and `"   "` hit the SAME guard arm.
        let err = parse_int("   ", 1, 10, "test").unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("whitespace-only must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("empty"),
            "whitespace-only diagnostic must call out 'empty' (matching the \
             truly-empty arm — proves the trim() runs before is_empty()); \
             got {msg:?}"
        );
        // Stage 11.A8c — upgrade these non-integer parse-fail checks
        // to pin the path-specific diagnostic. parse_int's parse arm
        // at line 120-124 produces:
        //   "{name}: payload \"X\" is not a positive integer"
        for input in ["abc", "-5", "1.5"] {
            let err = parse_int(input, 1, 10, "test").unwrap_err();
            let Error::InvalidData(msg) = err else {
                panic!("parse_int({input:?}) must yield InvalidData; got {err:?}");
            };
            let want_echo = format!("{input:?}");
            assert!(
                msg.contains("test:")
                    && msg.contains("not a positive integer")
                    && msg.contains(&want_echo),
                "parse-fail arm for {input:?} must pin name prefix + 'not a positive integer' \
                 + {want_echo:?} echo; got {msg:?}"
            );
            // ABSENCE of "empty" or "must be in [" — kills mutants that
            // route parse-fails through the empty or range arms.
            assert!(
                !msg.contains("payload is empty") && !msg.contains("must be in"),
                "parse-fail for {input:?} must not leak the empty / range arms; got {msg:?}"
            );
        }
    }
}
