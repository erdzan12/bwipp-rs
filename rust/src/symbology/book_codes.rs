//! ISBN-13, ISMN, ISSN — book and serial publication codes.
//!
//! All three are renderered as EAN-13 with a specific prefix:
//!
//!   * **ISBN-13**: 13-digit ISBN starting with `978` or `979`. Input may be
//!     the 13-digit form (with optional hyphens) or the older 10-digit form,
//!     which we convert to ISBN-13 by prefixing `978` and recomputing the
//!     check digit.
//!   * **ISMN**: 13-digit ISMN starting with `9790`. Input may be the
//!     8-character ISMN-10 form (`M` prefix + 9 digits + check) which we
//!     translate to ISMN-13.
//!   * **ISSN**: 8-character serial number rendered as EAN-13 with prefix
//!     `977`, the first 7 ISSN digits, two extra digits (issue code, default
//!     `00`), and a recomputed check digit.
//!
//! Patterns ported from the BWIPP `isbn`, `ismn`, and `issn` encoders.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

use super::ean;

fn strip_separators(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c, '-' | ' ' | '_'))
        .collect()
}

fn gs1_check(digits: &str) -> char {
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let n = c.to_digit(10).unwrap();
        sum += if i % 2 == 0 { n * 3 } else { n };
    }
    char::from_digit((10 - sum % 10) % 10, 10).unwrap()
}

fn isbn10_check(digits: &str) -> char {
    // ISBN-10 mod-11 check digit: sum(d_i * (10 - i)) mod 11, 'X' if remainder 10.
    let mut sum: u32 = 0;
    for (i, c) in digits.chars().enumerate() {
        let n = c.to_digit(10).unwrap();
        sum += n * (10 - i as u32);
    }
    let r = (11 - sum % 11) % 11;
    if r == 10 {
        'X'
    } else {
        char::from_digit(r, 10).unwrap()
    }
}

/// Encode an ISBN. Accepts 10- or 13-digit input (with optional hyphens) and
/// renders as EAN-13.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // ISBN-13 with hyphens; the encoder strips them and validates the check
/// // digit.
/// let svg = render_svg(Symbology::Isbn, "978-0-13-468599-1", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_isbn(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let raw = strip_separators(data.trim());
    let body = match raw.len() {
        9 | 10 => {
            // Old ISBN-10. Take the first 9 digits as the body, prepend "978"
            // to get the first 12 ISBN-13 digits, recompute the check.
            let nine: String = raw.chars().take(9).collect();
            if !nine.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::InvalidData(
                    "ISBN-10: first 9 characters must be digits".into(),
                ));
            }
            if raw.len() == 10 {
                let supplied = raw.chars().last().unwrap();
                let supplied_upper = supplied.to_ascii_uppercase();
                let expected = isbn10_check(&nine);
                if supplied_upper != expected {
                    return Err(Error::InvalidData(format!(
                        "ISBN-10 check digit mismatch: supplied {supplied_upper}, expected {expected}"
                    )));
                }
            }
            let prefix = format!("978{nine}");
            format!("{prefix}{}", gs1_check(&prefix))
        }
        12 | 13 => {
            // ISBN-13 form. Validate the prefix is 978/979 and re-use the
            // EAN-13 normalize/check path.
            if !raw.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::InvalidData(
                    "ISBN-13: must be 12 or 13 digits".into(),
                ));
            }
            if !(raw.starts_with("978") || raw.starts_with("979")) {
                return Err(Error::InvalidData(format!(
                    "ISBN-13 must start with 978 or 979 (got {})",
                    &raw[..3.min(raw.len())]
                )));
            }
            if raw.len() == 12 {
                format!("{raw}{}", gs1_check(&raw))
            } else {
                let computed = gs1_check(&raw[..12]);
                let supplied = raw.chars().last().unwrap();
                if supplied != computed {
                    return Err(Error::InvalidData(format!(
                        "ISBN-13 check digit mismatch: supplied {supplied}, expected {computed}"
                    )));
                }
                raw
            }
        }
        n => {
            return Err(Error::InvalidData(format!(
                "ISBN: expected 9, 10, 12, or 13 digits, got {n}"
            )))
        }
    };
    ean::encode_ean13(&body, opts)
}

/// Encode an ISMN. Accepts the 13-digit ISMN-13 form (with optional hyphens)
/// or the older ISMN-10 form (`M` + 9 digits + check) and renders as EAN-13.
pub fn encode_ismn(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let raw_upper = data.trim().to_uppercase();
    let raw = strip_separators(&raw_upper);

    // ISMN-10 form: "M" + 8 body digits + 1 check digit (10 chars total),
    // or "M" + 8 body digits (9 chars, no check). Conversion to ISMN-13
    // prefixes "9790" to the 8 body digits and recomputes the GS1 check.
    if let Some(rest) = raw.strip_prefix('M') {
        if rest.len() != 8 && rest.len() != 9 {
            return Err(Error::InvalidData(format!(
                "ISMN-10: expected 8 or 9 characters after M, got {}",
                rest.len()
            )));
        }
        let body8: String = rest.chars().take(8).collect();
        if !body8.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidData(
                "ISMN-10: 8 characters after M must be digits".into(),
            ));
        }
        // The user-supplied check digit (if any) uses ISMN-10 mod-10
        // weighting which we don't currently validate; the new ISMN-13
        // check is recomputed below.
        let prefix = format!("9790{body8}");
        let body = format!("{prefix}{}", gs1_check(&prefix));
        return ean::encode_ean13(&body, opts);
    }

    // ISMN-13 form: must start with 9790 and be 12 or 13 digits long.
    if !raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidData(
            "ISMN: must be 12/13 digits or 10-char M-prefixed form".into(),
        ));
    }
    if !raw.starts_with("9790") && !raw.starts_with("9791") {
        return Err(Error::InvalidData(format!(
            "ISMN-13 must start with 9790 (or 9791) (got {})",
            &raw[..4.min(raw.len())]
        )));
    }
    let body = match raw.len() {
        12 => format!("{raw}{}", gs1_check(&raw)),
        13 => {
            let computed = gs1_check(&raw[..12]);
            let supplied = raw.chars().last().unwrap();
            if supplied != computed {
                return Err(Error::InvalidData(format!(
                    "ISMN-13 check digit mismatch: supplied {supplied}, expected {computed}"
                )));
            }
            raw
        }
        n => {
            return Err(Error::InvalidData(format!(
                "ISMN: expected 12 or 13 digits, got {n}"
            )))
        }
    };
    ean::encode_ean13(&body, opts)
}

/// Encode an ISSN as EAN-13 with the `977` prefix. Accepts the 8-character
/// ISSN form (with optional hyphen) and an optional 2-digit issue code via
/// the `issuecode` option (default `00`).
pub fn encode_issn(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let raw = strip_separators(data.trim().to_uppercase().as_str());
    if raw.len() != 8 && raw.len() != 7 {
        return Err(Error::InvalidData(format!(
            "ISSN: expected 7 or 8 characters, got {}",
            raw.len()
        )));
    }
    // Verify that the first 7 chars are digits and the optional 8th is a
    // digit or 'X'.
    let digits7: String = raw.chars().take(7).collect();
    if !digits7.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidData(
            "ISSN: first 7 characters must be digits".into(),
        ));
    }
    let issue_code = opts.get("issuecode").unwrap_or("00");
    if issue_code.len() != 2 || !issue_code.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidOption(format!(
            "issuecode must be exactly 2 digits (got {issue_code:?})"
        )));
    }
    let prefix = format!("977{digits7}{issue_code}");
    let body = format!("{prefix}{}", gs1_check(&prefix));
    ean::encode_ean13(&body, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `strip_separators`. Every ISBN/ISMN/ISSN
    /// entry point calls this helper to remove `-`, ` `, and `_`
    /// before parsing. None of the existing tests exercise the helper
    /// directly: the public-encoder tests only verify that hyphenated
    /// inputs *eventually* succeed, so a mutation that flips the
    /// matcher (e.g. dropping `'_'`, or `!matches!` → `matches!`) could
    /// survive on hyphen-only / space-only inputs.
    ///
    /// Mutations to catch:
    ///   - `!matches!(...)` → `matches!(...)`: inverted predicate —
    ///     only separators survive, everything else is dropped.
    ///   - Removal of any of `'-' | ' ' | '_'`: that separator stops
    ///     being stripped (regression for ISBN-10 with hyphens,
    ///     ISSN with spaces, or ISBN-13 with underscores).
    ///   - Replacing body with `s.to_string()`: no stripping at all.
    #[test]
    fn strip_separators_strips_dash_space_underscore() {
        // No separators: unchanged.
        assert_eq!(strip_separators("9780131103627"), "9780131103627");
        assert_eq!(strip_separators(""), "");
        assert_eq!(strip_separators("ABC"), "ABC");
        // Each separator removed individually.
        assert_eq!(
            strip_separators("978-0-13-110362-7"),
            "9780131103627",
            "hyphens stripped (ISBN-13 canonical form)"
        );
        assert_eq!(
            strip_separators("978 0 13 110362 7"),
            "9780131103627",
            "spaces stripped (alternate ISBN separator)"
        );
        assert_eq!(
            strip_separators("978_0_13_110362_7"),
            "9780131103627",
            "underscores stripped (regression net for `_`-only mutation)"
        );
        // Mixed separators in a single payload.
        assert_eq!(
            strip_separators("978-0 13_110362-7"),
            "9780131103627",
            "all three separator classes stripped from one input"
        );
        // Non-separator chars (e.g. dot, slash) MUST be preserved
        // — rejects a mutation that broadens the matcher.
        assert_eq!(
            strip_separators("1.2.3"),
            "1.2.3",
            "dots preserved (not in the separator set)"
        );
        assert_eq!(
            strip_separators("1/2/3"),
            "1/2/3",
            "slashes preserved (not in the separator set)"
        );
        // All-separator input collapses to empty.
        assert_eq!(strip_separators("---"), "");
        assert_eq!(strip_separators("   "), "");
        assert_eq!(strip_separators("___"), "");
    }

    /// Stage 11.A8c — pin `gs1_check` (a private mod-10 weighted-sum
    /// helper, separate copy from the one in `interleaved2of5` /
    /// `twoofive` / `ean`) with hand-computed values. The existing
    /// tests exercise the helper indirectly through encoding goldens
    /// for full ISBN-13 / ISMN / ISSN payloads, but a mutant in
    /// just this copy (e.g. `i % 2 == 0` → `!= 0`, or `sum / 10`
    /// inside the inner) could survive if the test inputs land on
    /// arithmetic coincidences.
    ///
    /// Hand-computed (book_codes::gs1_check is `(10 - sum%10) % 10`
    /// with right-to-left weights 3,1,3,1...):
    ///   - "" → sum=0 → '0'.
    ///   - "9" → rev="9", 9*3=27, sum=27, (10-7)%10=3 → '3'.
    ///   - "55" → rev="55": 5*3+5*1=20, (10-0)%10=0 → '0' (wrap).
    ///   - "977093178471" → ISSN prefix used in the codebase:
    ///       rev = "174178390779"
    ///       sum = 1*3+7*1+4*3+1*1+7*3+8*1+3*3+9*1+0*3+7*1+7*3+9*1
    ///           = 3+7+12+1+21+8+9+9+0+7+21+9 = 107
    ///       (10 - 7) % 10 = 3 → '3'.
    #[test]
    fn gs1_check_book_codes_known_values() {
        assert_eq!(gs1_check(""), '0');
        assert_eq!(gs1_check("9"), '3');
        // Wrap-around — kills the outer `% 10` fold.
        assert_eq!(gs1_check("55"), '0', "sum=20 → outer % 10 must fold to 0");
        // Full 12-digit ISSN body that book_codes builds for the
        // "977 + first 7 ISSN digits + 00 issue code" pattern.
        assert_eq!(
            gs1_check("977031784710"),
            // rev = "017487130779"
            //   0*3+1*1+7*3+4*1+8*3+7*1+1*3+3*1+0*3+7*1+7*3+9*1
            //   = 0+1+21+4+24+7+3+3+0+7+21+9 = 100
            //   (10 - 100%10) % 10 = (10-0) % 10 = 0
            '0',
            "977031784710 → wraparound check '0'"
        );
    }

    /// Stage 11.A8c — pin `isbn10_check`'s mod-11 arithmetic
    /// directly. The existing `isbn10_check_digit_is_actually_validated`
    /// goes through `encode_isbn` (which converts to ISBN-13 and
    /// computes a NEW check), so the ISBN-10 helper's exact return
    /// value isn't observable from end-to-end tests. Hand-computed
    /// against `sum(d_i * (10 - i)) mod 11`, then `(11 - r) % 11`:
    ///
    ///   - "000000000" → sum=0, r=0 → '0' (outer % 11 fold).
    ///   - "111111111" → sum=10+9+8+...+2 = 54, 54%11=10, r=1 → '1'.
    ///   - "020161622" → sum = 0+18+0+7+36+5+24+6+4 = 100, 100%11=1,
    ///     r=10 → 'X' (the special 'X' branch).
    ///   - "059600920" → 0+45+72+42+0+0+36+6+0 = 201, 201%11=3,
    ///     r=8 → '8'.
    #[test]
    fn isbn10_check_known_values() {
        // The outer % 11 fold: sum%11==0 → check '0', not '\u{0b}'.
        assert_eq!(
            isbn10_check("000000000"),
            '0',
            "sum=0 → (11-0) % 11 must fold to 0"
        );
        assert_eq!(isbn10_check("111111111"), '1');
        // The r==10 → 'X' branch.
        assert_eq!(
            isbn10_check("020161622"),
            'X',
            "sum=100, 100 % 11 = 1, r = 10 → 'X' (special branch)"
        );
        // Common pop-fiction ISBN-10 prefix.
        assert_eq!(
            isbn10_check("059600920"),
            '8',
            "Programming Ruby ISBN-10: check digit is '8'"
        );
    }

    #[test]
    fn isbn13_canonical() {
        // "978-1-56619-909-4" → 9781566199094, check should match.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the canonical ISBN-13 978-prefix path.
        let p = encode_isbn("978-1-56619-909-4", &Options::default()).expect(
            "encode_isbn(\"978-1-56619-909-4\", default) (canonical ISBN-13 with hyphens, 978-prefix + computed check) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_isbn(\"978-1-56619-909-4\") (canonical ISBN-13 with hyphens, 978-prefix) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn isbn13_rejects_wrong_prefix() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 4-anchor pin
        // matching the source diagnostic at line 101-104 of
        // book_codes.rs:
        //   1. `ISBN-13` symbology-flavor prefix
        //   2. `must start with 978 or 979` full predicate (pins both
        //      valid prefixes — kills a mutation that drops one half
        //      of the OR check at line 100)
        //   3. `got 123` first-3-chars echo (the `&raw[..3.min(raw.len())]`
        //      slice produces "123" for the 13-char input "1234567890123")
        //   4. cross-arm contamination guard: must NOT contain `check
        //      digit mismatch` (sibling at line 112-114) — kills
        //      mutations that route prefix-bad input through the
        //      check-digit-mismatch path.
        match encode_isbn("1234567890123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISBN-13"), "missing `ISBN-13` prefix: {msg}");
                assert!(
                    msg.contains("must start with 978 or 979"),
                    "missing `must start with 978 or 979` full predicate: {msg}"
                );
                assert!(
                    msg.contains("got 123"),
                    "missing `got 123` first-3-chars echo: {msg}"
                );
                assert!(
                    !msg.contains("check digit mismatch"),
                    "cross-arm contamination: prefix-reject mentions check digit: {msg}"
                );
            }
            other => panic!("non-978/979 ISBN-13 should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn isbn13_validates_check_digit() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 4-anchor pin
        // matching the source diagnostic at line 112-114 of
        // book_codes.rs:
        //   1. `ISBN-13` symbology-flavor prefix
        //   2. `check digit mismatch` predicate
        //   3. `supplied 9` supplied-value echo (last char of
        //      "9781566199099")
        //   4. `expected 4` computed-value echo (the correct check
        //      for 9781566199094)
        // The supplied vs expected echo pair pins BOTH the
        // gs1_check arithmetic AND the supplied-char extraction at
        // line 110 of book_codes.rs.
        // 9781566199094 has check 4; supply a wrong one and reject.
        match encode_isbn("9781566199099", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISBN-13"), "missing `ISBN-13` prefix: {msg}");
                assert!(
                    msg.contains("check digit mismatch"),
                    "missing `check digit mismatch` predicate: {msg}"
                );
                assert!(
                    msg.contains("supplied 9"),
                    "missing `supplied 9` supplied-value echo: {msg}"
                );
                assert!(
                    msg.contains("expected 4"),
                    "missing `expected 4` computed-value echo: {msg}"
                );
                assert!(
                    !msg.contains("must start with"),
                    "cross-arm contamination: check-digit reject mentions prefix-arm: {msg}"
                );
            }
            other => panic!("ISBN-13 with bad check should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn isbn10_converts_to_13() {
        // 0-13-110362-8 maps to 978-0-13-110362-x where x is the new check.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISBN-10 → ISBN-13 auto-conversion path.
        let p = encode_isbn("0131103628", &Options::default()).expect(
            "encode_isbn(\"0131103628\", default) (ISBN-10 → ISBN-13 conversion: 978-prefix auto-prepended + check recomputed) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_isbn(\"0131103628\") (10-digit ISBN-10 → 13-digit ISBN-13 via 978-prefix wrap) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn ismn_canonical_form() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the canonical ISMN-13 979-0 prefix path.
        let p = encode_ismn("979-0-1234-5678-5", &Options::default()).expect(
            "encode_ismn(\"979-0-1234-5678-5\", default) (canonical ISMN-13 with hyphens, 979-0 prefix) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_ismn(\"979-0-1234-5678-5\") (canonical ISMN-13 with hyphens, 979-0 prefix) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn ismn_m_prefix_form() {
        // "M123456785" is ISMN-10 (M + 9 digits + check)
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the legacy ISMN-10 M-prefix → ISMN-13 conversion path.
        let p = encode_ismn("M123456785", &Options::default()).expect(
            "encode_ismn(\"M123456785\", default) (legacy ISMN-10 M-prefix → ISMN-13 conversion: 979-0 wrap) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_ismn(\"M123456785\") (legacy ISMN-10 with M-prefix → ISMN-13 via 979-0 wrap) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn issn_with_default_issue_code() {
        // 0317-8471 → 977 + 0317847 + 00 + new check
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISSN default 00 issue-code path.
        let p = encode_issn("0317-8471", &Options::default()).expect(
            "encode_issn(\"0317-8471\", default) (ISSN default issue-code path: 977-prefix + ISSN + 00 placeholder + new check) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_issn(\"0317-8471\") (ISSN → EAN-13 via 977-prefix + default 00 issue code) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn issn_with_explicit_issue_code() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISSN explicit-issue-code override path (overrides 00).
        let p = encode_issn("0317-8471", &Options::default().with("issuecode", "13")).expect(
            "encode_issn(\"0317-8471\", issuecode=13) (ISSN explicit issuecode override: 977-prefix + ISSN + 13 issue code + new check) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_issn(\"0317-8471\", issuecode=\"13\") (ISSN → EAN-13 via 977-prefix + explicit 13 issue code) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn issn_rejects_bad_issue_code() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidOption(_)))` to 2-anchor pin
        // matching the source diagnostic at line 212-214 of
        // book_codes.rs:
        //   1. `issuecode must be exactly 2 digits` predicate
        //   2. `"X"` Debug echo of the offending option value
        // Note: the diagnostic does NOT include an `ISSN:` prefix
        // (the issuecode option-validation is generic).
        match encode_issn("0317-8471", &Options::default().with("issuecode", "X")) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("issuecode must be exactly 2 digits"),
                    "missing `issuecode must be exactly 2 digits` predicate: {msg}"
                );
                assert!(
                    msg.contains("\"X\""),
                    "missing `\"X\"` Debug echo of offending option: {msg}"
                );
            }
            other => panic!("issuecode=X should reject as InvalidOption, got {other:?}"),
        }
    }

    /// ISBN-13 bar-pattern golden from
    /// `raw("isbn", "978-1-56619-909-4", {})[0].sbs`.
    #[test]
    fn isbn_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISBN-13 byte-for-byte 59-bar bwip-js raw SBS oracle.
        let p = encode_isbn("978-1-56619-909-4", &Options::default()).expect(
            "encode_isbn(\"978-1-56619-909-4\", default) (ISBN-13 byte-for-byte 59-bar SBS bwip-js raw oracle) must succeed",
        );
        let want: [u8; 59] = [
            1, 1, 1, 1, 3, 1, 2, 3, 1, 2, 1, 1, 2, 2, 2, 1, 2, 3, 1, 4, 1, 1, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 2, 2, 2, 1, 3, 1, 1, 2, 3, 1, 1, 2, 3, 2, 1, 1, 3, 1, 1, 2, 1, 1, 3, 2, 1, 1,
            1,
        ];
        assert_eq!(p.bars, want, "isbn bars mismatch vs bwip-js raw output");
    }

    /// ISMN bar-pattern golden from
    /// `raw("ismn", "979-0-1234-5678-5", {})[0].sbs`.
    #[test]
    fn ismn_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISMN byte-for-byte 59-bar bwip-js raw SBS oracle.
        let p = encode_ismn("979-0-1234-5678-5", &Options::default()).expect(
            "encode_ismn(\"979-0-1234-5678-5\", default) (ISMN byte-for-byte 59-bar SBS bwip-js raw oracle; 979-0 prefix) must succeed",
        );
        let want: [u8; 59] = [
            1, 1, 1, 1, 3, 1, 2, 2, 1, 1, 3, 1, 1, 2, 3, 2, 2, 2, 1, 2, 2, 1, 2, 1, 4, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1, 1, 4, 1, 3, 1, 2, 1, 2, 1, 3, 1, 2, 3, 1, 1, 1,
            1,
        ];
        assert_eq!(p.bars, want, "ismn bars mismatch vs bwip-js raw output");
    }

    /// ISSN bar-pattern golden from
    /// `raw("issn", "0317-8471", {})[0].sbs`. (BWIPP wraps the ISSN
    /// number in `977<ISSN>00` and recomputes the EAN-13 check.)
    #[test]
    fn issn_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the ISSN byte-for-byte 59-bar bwip-js raw SBS oracle.
        let p = encode_issn("0317-8471", &Options::default()).expect(
            "encode_issn(\"0317-8471\", default) (ISSN byte-for-byte 59-bar SBS bwip-js raw oracle; BWIPP wraps ISSN in 977<ISSN>00 + EAN-13 check) must succeed",
        );
        let want: [u8; 59] = [
            1, 1, 1, 1, 3, 1, 2, 2, 1, 3, 1, 1, 1, 2, 3, 1, 4, 1, 1, 1, 2, 2, 2, 1, 3, 1, 2, 1, 1,
            1, 1, 1, 1, 2, 1, 3, 1, 1, 3, 2, 1, 3, 1, 2, 3, 2, 1, 1, 3, 2, 1, 1, 2, 2, 2, 1, 1, 1,
            1,
        ];
        assert_eq!(p.bars, want, "issn bars mismatch vs bwip-js raw output");
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests. Each test below pins a *specific*
    // cargo-mutants survivor surfaced by the baseline run.
    // ---------------------------------------------------------------------

    /// Kills `isbn10_check: replace * with /` (line ~44). The original
    /// test corpus had a single ISBN-10 input whose check digit
    /// coincidentally survived the `n / (10 - i)` mutation by integer-
    /// division collapse; the inputs here exercise distinct digit
    /// distributions so the mutated checksum diverges and the encoder
    /// rejects the symbol with `InvalidData`.
    #[test]
    fn isbn10_check_digit_is_actually_validated() {
        // Stage 11.A8c (cont) — upgrade both wrong-check arms from
        // discriminant-only `matches!(_, Err(Error::InvalidData(_)))`
        // to 4-anchor pin matching the source diagnostic at line
        // 84-86 of book_codes.rs:
        //   1. `ISBN-10` symbology-flavor prefix
        //   2. `check digit mismatch` predicate
        //   3. `supplied X` Debug-echo of offending check char (after
        //      uppercase normalization at line 81)
        //   4. `expected Y` computed-value echo
        // The supplied vs expected echo pair pins both the ISBN-10
        // mod-11 arithmetic and the supplied-char extraction.

        // "0596009208" — Programming Ruby. Correct check = '8' (mod-11
        // weighted sum = 201, 201 % 11 = 3, 11 - 3 = 8). Should accept.
        // Stage 11.A8c (cont) — descriptive label naming ISBN-10 happy path.
        assert!(
            encode_isbn("0596009208", &Options::default()).is_ok(),
            "encode_isbn(\"0596009208\") (ISBN-10 with computed check '8'; mod-11 sum=201 → check=11-3=8) must accept — kills `* with /` arithmetic mutant in isbn10_check"
        );
        // Same prefix, deliberately wrong check digit. Should reject.
        match encode_isbn("0596009207", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISBN-10"), "missing `ISBN-10` prefix: {msg}");
                assert!(
                    msg.contains("check digit mismatch"),
                    "missing `check digit mismatch` predicate: {msg}"
                );
                assert!(
                    msg.contains("supplied 7"),
                    "missing `supplied 7` value echo: {msg}"
                );
                assert!(
                    msg.contains("expected 8"),
                    "missing `expected 8` computed-value echo: {msg}"
                );
                assert!(
                    !msg.contains("ISBN-13"),
                    "wrong arm — ISBN-13 diagnostic leaked into ISBN-10 reject: {msg}"
                );
            }
            other => panic!("0596009207 should reject as InvalidData, got {other:?}"),
        }

        // ISBN-10 with 'X' as check (i.e. mod-11 remainder = 10).
        // "020161622X" — The Mythical Man-Month. Should accept.
        // Stage 11.A8c (cont) — descriptive label naming X-check special case.
        assert!(
            encode_isbn("020161622X", &Options::default()).is_ok(),
            "encode_isbn(\"020161622X\") (ISBN-10 with 'X' as check digit, mod-11 remainder = 10 special case) must accept — kills mutation that fails to handle 'X' as a valid check character"
        );
        // Same prefix, supply '0' instead of 'X'. Should reject.
        match encode_isbn("0201616220", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISBN-10"), "missing `ISBN-10` prefix: {msg}");
                assert!(
                    msg.contains("check digit mismatch"),
                    "missing `check digit mismatch` predicate: {msg}"
                );
                assert!(
                    msg.contains("supplied 0"),
                    "missing `supplied 0` value echo: {msg}"
                );
                assert!(
                    msg.contains("expected X"),
                    "missing `expected X` computed-value echo (the 'X' case pins remainder-10 → 'X' branch at line 47-48): {msg}"
                );
            }
            other => panic!("0201616220 should reject as InvalidData, got {other:?}"),
        }
    }

    /// Kills `encode_isbn: replace == with !=` (line ~79). The original
    /// test exercised the 10-digit branch with a *correct* check digit
    /// only; the mutant skipped validation, but every test passed
    /// anyway. This test feeds a 10-digit ISBN with a wrong check
    /// digit and asserts the encoder rejects it — the mutant would
    /// route through `if raw.len() != 10` (false), skip the check,
    /// and silently accept the wrong-check symbol.
    #[test]
    fn isbn10_with_wrong_check_digit_is_rejected() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 4-anchor pin
        // matching the source diagnostic at line 84-86 of
        // book_codes.rs (`ISBN-10 check digit mismatch: supplied
        // {supplied_upper}, expected {expected}`):
        //   1. `ISBN-10` prefix
        //   2. `check digit mismatch` predicate
        //   3. `supplied 7` value echo (supplied last char of
        //      "0131103627")
        //   4. `expected 8` computed-value echo (correct check for
        //      "013110362" via ISBN-10 mod-11)
        // 0131103627 → correct check would be 8, supplied is 7.
        match encode_isbn("0131103627", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISBN-10"), "missing `ISBN-10` prefix: {msg}");
                assert!(
                    msg.contains("check digit mismatch"),
                    "missing `check digit mismatch` predicate: {msg}"
                );
                assert!(
                    msg.contains("supplied 7"),
                    "missing `supplied 7` value echo: {msg}"
                );
                assert!(
                    msg.contains("expected 8"),
                    "missing `expected 8` computed-value echo: {msg}"
                );
            }
            other => panic!("0131103627 should reject as InvalidData, got {other:?}"),
        }
    }

    /// Kills `encode_ismn: replace != with ==` (line ~138). Original
    /// length check is `rest.len() != 8 && rest.len() != 9` (i.e. both
    /// 8 and 9 are valid). Mutation flips one half so the *other*
    /// length becomes invalid. We exercise *both* the 8-char and
    /// 9-char ISMN-10 paths so flipping either operand makes one of
    /// these tests fail.
    #[test]
    fn ismn_accepts_both_eight_and_nine_char_m_prefixed_form() {
        // 9-char form ("M" + 8 digits, no user-supplied check): the
        // encoder computes the ISMN-13 check.
        // Stage 11.A8c (cont) — descriptive labels naming both forms.
        let p = encode_ismn("M12345678", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_ismn(\"M12345678\") (9-char ISMN-10 form: M + 8 digits, no user check → encoder computes ISMN-13) must compose into non-empty symbol; got {}",
            p.total_width()
        );
        // 10-char form ("M" + 8 digits + ISMN-10 check digit). The
        // encoder ignores the legacy check and recomputes ISMN-13.
        let p = encode_ismn("M123456785", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_ismn(\"M123456785\") (10-char ISMN-10 form: M + 8 digits + legacy check, recomputed for ISMN-13) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    /// Kills `encode_ismn: delete !` (line ~164). Original check is
    /// `if !raw.starts_with("9790") && !raw.starts_with("9791")` —
    /// reject anything that doesn't start with one of those two
    /// prefixes. Mutation flips one of the `!` to a no-op, weakening
    /// the prefix guard so non-9790/9791 inputs slip past the check
    /// and trip a *different* error downstream (length / check-digit
    /// validation). The test must therefore inspect the error message
    /// to confirm the original `"must start with 9790"` reason fires,
    /// not just `matches!(_, InvalidData(_))` which accepts both
    /// original and mutant's downstream errors.
    #[test]
    fn ismn13_with_wrong_prefix_is_rejected_with_prefix_message() {
        // Pure-digit 13-char input not starting with 9790/9791 — only
        // the prefix guard should reject it. Under any of the
        // delete-`!` mutants the guard is bypassed and the downstream
        // length/check-digit branch handles it with a different
        // (non-"start with") error message.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("must
        // start with")` upgraded to 4-anchor pin:
        //   1. `ISMN-13` symbology-flavor prefix
        //   2. `must start with 9790 (or 9791)` full predicate
        //   3. `got 1234` first-4-chars value-echo (kills `&raw[..4]`
        //      slicing mutations in the format string at line 167)
        //   4. cross-arm contamination guard: must NOT contain
        //      `ISBN-13` (sibling at line 102) — kills body-swap
        //      mutations that would route ISMN-13 input through the
        //      ISBN-13 diagnostic.
        match encode_ismn("1234567890123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISMN-13"), "missing ISMN-13 prefix: {msg:?}");
                assert!(
                    msg.contains("must start with 9790 (or 9791)"),
                    "missing full prefix-predicate `must start with 9790 (or 9791)`: {msg:?}"
                );
                assert!(
                    msg.contains("got 1234"),
                    "missing `got 1234` first-4-chars echo: {msg:?}"
                );
                assert!(
                    !msg.contains("ISBN-13"),
                    "cross-arm contamination: ISMN-13 reject mentions ISBN-13: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(prefix), got {other:?}"),
        }
    }

    /// Kills `encode_issn: replace != with ==` (line ~196) and
    /// `encode_issn: replace || with &&` (line ~211). Original
    /// validation is `if raw.len() != 8 && raw.len() != 7` and
    /// `if issue_code.len() != 2 || !issue_code.chars().all(...)`.
    /// We exercise the 7-char ISSN form (the often-skipped path) and
    /// a 1-character issue_code (rejected by the `!= 2` half of the
    /// option-validation pair).
    #[test]
    fn issn_seven_char_form_and_short_issuecode_validation() {
        // Stage 11.A8c (cont) — upgrade all three reject arms from
        // discriminant-only to multi-anchor pin.
        //
        // 7-char form — the X-less prefix. Encoder should accept.
        // Stage 11.A8c (cont) — descriptive label naming 7-char form
        // (the often-skipped path that kills the `!= != → !=` ==
        // length-validation mutation).
        let p = encode_issn("0317847", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_issn(\"0317847\") (7-char ISSN form, X-less prefix, exercises != 7 half of length guard) must compose into non-empty symbol; got {}",
            p.total_width()
        );

        // 6-char form — should reject. Diagnostic at line 197-200:
        // `ISSN: expected 7 or 8 characters, got {n}`
        // Anchors: prefix + length-spec (pin both `7` and `8`) +
        // value echo + cross-arm guard against issuecode leakage.
        match encode_issn("031784", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("ISSN:"), "missing `ISSN:` prefix: {msg}");
                assert!(
                    msg.contains("expected 7 or 8 characters"),
                    "missing `expected 7 or 8 characters` length-spec: {msg}"
                );
                assert!(msg.contains("got 6"), "missing `got 6` value echo: {msg}");
                assert!(
                    !msg.contains("issuecode"),
                    "wrong arm — issuecode diagnostic leaked into 6-char ISSN reject: {msg}"
                );
            }
            other => panic!("6-char ISSN should reject as InvalidData, got {other:?}"),
        }

        // 1-character issue_code — must reject (length != 2).
        // Diagnostic at line 212-214: `issuecode must be exactly 2
        // digits (got {issue_code:?})`. The 1-char arm is the
        // length != 2 half of the predicate.
        match encode_issn("0317-8471", &Options::default().with("issuecode", "1")) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("issuecode must be exactly 2 digits"),
                    "1-char issuecode: missing predicate: {msg}"
                );
                assert!(
                    msg.contains("\"1\""),
                    "1-char issuecode: missing `\"1\"` Debug echo: {msg}"
                );
            }
            other => panic!("issuecode=1 should reject as InvalidOption, got {other:?}"),
        }

        // 3-character issue_code — must reject.
        match encode_issn("0317-8471", &Options::default().with("issuecode", "123")) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("issuecode must be exactly 2 digits"),
                    "3-char issuecode: missing predicate: {msg}"
                );
                assert!(
                    msg.contains("\"123\""),
                    "3-char issuecode: missing `\"123\"` Debug echo: {msg}"
                );
            }
            other => panic!("issuecode=123 should reject as InvalidOption, got {other:?}"),
        }
    }
}
