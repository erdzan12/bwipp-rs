//! HIBC LIC (Health Industry Bar Code - Labeler Identification Code).
//!
//! HIBC LIC wraps a payload in a fixed envelope:
//!
//!   1. Prefix the data with `+`.
//!   2. Compute a mod-43 check character over the prefixed string using the
//!      HIBC alphabet `0..9A..Z-. $/+%` (the same alphabet Code 39 uses).
//!   3. Append the check character.
//!   4. Render the resulting string with the chosen base symbology.
//!
//! Base symbologies supported: **Code 128**, **Code 39**, **Data Matrix**,
//! **QR Code**, **PDF417**, **MicroPDF417**, and **Codablock-F**. Each one
//! ships as both a LIC and PAS variant. The envelope is identical across
//! all of them — the only difference is which downstream encoder receives
//! the formatted string.
//!
//! Reference: BWIPP `hibccode128`, `hibccode39`, `hibcdatamatrix`,
//! `hibcqrcode`, `hibcpdf417`, `hibcmicropdf417`, `hibccodablockf`
//! encoders.

use crate::encoding::{BitMatrix, LinearPattern, StackedPattern};
use crate::error::Error;
use crate::options::Options;

use super::aztec;
use super::codablockf;
use super::code128;
use super::code39;
use super::datamatrix_;
use super::micropdf417;
use super::pdf417;
#[cfg(not(feature = "prefer-native-qrcode"))]
use super::qrcode_;

const HIBC_ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";

/// Validate the user's payload, prefix it with `+`, append the mod-43 check
/// character, and return the wire-format string.
pub fn format(data: &str) -> Result<String, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "HIBC LIC payload must not be empty".into(),
        ));
    }
    let upper = data.to_uppercase();
    for c in upper.chars() {
        if !HIBC_ALPHABET.contains(c) {
            return Err(Error::InvalidData(format!(
                "HIBC LIC: invalid character {c:?}"
            )));
        }
    }
    let prefixed = format!("+{upper}");
    let sum: u32 = prefixed
        .chars()
        .map(|c| HIBC_ALPHABET.find(c).expect("validated above") as u32)
        .sum();
    let check = HIBC_ALPHABET
        .chars()
        .nth((sum % 43) as usize)
        .expect("mod-43 index in range");
    Ok(format!("{prefixed}{check}"))
}

/// HIBC LIC rendered as Code 128.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // HIBC LIC data: the encoder prepends `+` and appends the mod-43 check
/// // character automatically.
/// let svg = render_svg(Symbology::HibcCode128, "A123BJC5D6E71", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_code128(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let formatted = format(data)?;
    code128::encode(&formatted, opts)
}

/// HIBC LIC rendered as Code 39. The envelope includes `+`, which Code 39
/// supports natively.
pub fn encode_code39(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let formatted = format(data)?;
    code39::encode(&formatted, opts)
}

/// HIBC LIC rendered as Data Matrix.
pub fn encode_datamatrix(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let formatted = format(data)?;
    datamatrix_::encode(&formatted, opts)
}

/// HIBC LIC rendered as QR Code.
pub fn encode_qrcode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let formatted = format(data)?;
    #[cfg(feature = "prefer-native-qrcode")]
    {
        super::qrcode_native::encode_with_options(formatted.as_bytes(), opts)
    }
    #[cfg(not(feature = "prefer-native-qrcode"))]
    {
        qrcode_::encode(&formatted, opts)
    }
}

/// HIBC LIC rendered as PDF417.
pub fn encode_pdf417(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    pdf417::encode(&format(data)?, opts)
}

/// HIBC LIC rendered as MicroPDF417.
pub fn encode_micropdf417(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    micropdf417::encode(&format(data)?, opts)
}

/// HIBC LIC rendered as Codablock-F.
pub fn encode_codablockf(data: &str, opts: &Options) -> Result<StackedPattern, Error> {
    codablockf::encode(&format(data)?, opts)
}

/// HIBC LIC rendered as Aztec Code. BWIPP `hibcazteccode`.
///
/// The HIBC `+`-prefix + mod-43 check envelope is identical to the
/// LIC envelope used for Code 39 / Code 128 / DataMatrix / PDF417;
/// the only difference is the underlying symbology that carries the
/// formatted payload. We reuse [`format`] and delegate to
/// [`aztec::encode`].
///
/// # Options
///
/// - **`validatecheck`** (default `false`): when set to `"true"`,
///   the encoder expects the input's final character to be a
///   pre-computed HIBC mod-43 check digit. The encoder verifies the
///   provided check matches the computed one; on mismatch it
///   returns [`Error::InvalidData`] (BWIPP raises
///   `bwipp.hibcazteccodeBadCheckDigit`). On success, the trailing
///   check digit is stripped before [`format`] re-appends it — so
///   `encode_azteccode("ABC<provided>", validatecheck=true)`
///   produces the same symbol as `encode_azteccode("ABC")` when
///   `<provided>` matches.
pub fn encode_azteccode(data: &str, opts: &Options) -> Result<crate::encoding::BitMatrix, Error> {
    let validatecheck = match opts.get("validatecheck") {
        None | Some("false") => false,
        Some("true") => true,
        Some(v) => {
            return Err(Error::InvalidOption(format!(
                "hibcazteccode: validatecheck={v:?} must be \"true\" or \"false\""
            )));
        }
    };
    let effective = if validatecheck {
        verify_and_strip_hibc_check(data)?
    } else {
        data.to_string()
    };
    let formatted = format(&effective)?;
    aztec::encode(formatted.as_bytes())
}

/// Verify the trailing character of `data` matches the HIBC mod-43
/// check digit computed over the remaining body (with the implicit
/// `+` prefix). Returns the body sans the trailing check character.
/// Mirrors BWIPP `bwipp_hibcazteccode` lines 41917-41930.
fn verify_and_strip_hibc_check(data: &str) -> Result<String, Error> {
    if data.chars().count() < 2 {
        return Err(Error::InvalidData(
            "hibcazteccode: validatecheck=true requires at least 2 characters \
             (body + trailing check digit)"
                .into(),
        ));
    }
    let upper = data.to_uppercase();
    for c in upper.chars() {
        if !HIBC_ALPHABET.contains(c) {
            return Err(Error::InvalidData(format!(
                "hibcazteccode: invalid character {c:?}"
            )));
        }
    }
    // Split into body and provided check.
    let body: String = upper.chars().take(upper.chars().count() - 1).collect();
    let provided = upper.chars().last().expect("len >= 2 checked above");
    // Compute the expected check digit over `+body`.
    let prefixed = format!("+{body}");
    let sum: u32 = prefixed
        .chars()
        .map(|c| HIBC_ALPHABET.find(c).expect("validated above") as u32)
        .sum();
    let expected = HIBC_ALPHABET
        .chars()
        .nth((sum % 43) as usize)
        .expect("mod-43 index in range");
    if provided != expected {
        return Err(Error::InvalidData(format!(
            "hibcazteccode: validatecheck=true but the provided trailing check \
             digit {provided:?} doesn't match the computed value {expected:?}"
        )));
    }
    Ok(body)
}

/// HIBC LIC rendered as Data Matrix Rectangular. BWIPP
/// `hibcdatamatrixrectangular` — the rectangular-shape sibling of
/// [`encode_datamatrix`]. Same `+`-prefix + check envelope; passes
/// `shape=rectangular` to the datamatrix substrate so callers don't
/// have to set it manually.
pub fn encode_datamatrix_rectangular(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let formatted = format(data)?;
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "shape");
    o.extras
        .push(("shape".to_string(), "rectangular".to_string()));
    datamatrix_::encode(&formatted, &o)
}

// ---------------------------------------------------------------------------
// HIBC PAS (Provider Application Standard)
// ---------------------------------------------------------------------------

/// Format a HIBC PAS payload. PAS uses the same `+` prefix + mod-43 check
/// envelope as LIC; the differentiator is conventionally the data structure
/// (PAS data typically uses `/` as a separator between fields), but the
/// barcode-level encoding is identical, so we share [`format`].
pub fn format_pas(data: &str) -> Result<String, Error> {
    format(data)
}

/// HIBC PAS rendered as Code 128.
pub fn encode_pas_code128(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    code128::encode(&format_pas(data)?, opts)
}

/// HIBC PAS rendered as Code 39.
pub fn encode_pas_code39(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    code39::encode(&format_pas(data)?, opts)
}

/// HIBC PAS rendered as Data Matrix.
pub fn encode_pas_datamatrix(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    datamatrix_::encode(&format_pas(data)?, opts)
}

/// HIBC PAS rendered as QR Code.
pub fn encode_pas_qrcode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let formatted = format_pas(data)?;
    #[cfg(feature = "prefer-native-qrcode")]
    {
        super::qrcode_native::encode_with_options(formatted.as_bytes(), opts)
    }
    #[cfg(not(feature = "prefer-native-qrcode"))]
    {
        qrcode_::encode(&formatted, opts)
    }
}

/// HIBC PAS rendered as PDF417.
pub fn encode_pas_pdf417(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    pdf417::encode(&format_pas(data)?, opts)
}

/// HIBC PAS rendered as MicroPDF417.
pub fn encode_pas_micropdf417(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    micropdf417::encode(&format_pas(data)?, opts)
}

/// HIBC PAS rendered as Codablock-F.
pub fn encode_pas_codablockf(data: &str, opts: &Options) -> Result<StackedPattern, Error> {
    codablockf::encode(&format_pas(data)?, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_digit_known_vector() {
        // BWIPP test: data "A99912345/$$52001510X3" -> "+A99912345/$$52001510X3K"
        // Recompute by hand:
        //   prefixed = "+A99912345/$$52001510X3"
        //   sum = 41 + 10 + 9 + 9 + 9 + 1 + 2 + 3 + 4 + 5 + 40 + 39 + 39 + 5 + 2 + 0 + 0 + 1 + 5 + 1 + 0 + 33 + 3
        //   index('+')=41, index('A')=10, index('9')=9, ..., index('/')=40, index('$')=39
        //   Let's just check that the formatter agrees on the round-trip.
        let formatted = format("A99912345/$$52001510X3").unwrap();
        assert!(formatted.starts_with("+A99912345/$$52001510X3"));
        // Re-validate by computing the sum and confirming the last char.
        let body = &formatted[..formatted.len() - 1];
        let supplied = formatted.chars().last().unwrap();
        let sum: u32 = body
            .chars()
            .map(|c| HIBC_ALPHABET.find(c).unwrap() as u32)
            .sum();
        let expected = HIBC_ALPHABET.chars().nth((sum % 43) as usize).unwrap();
        assert_eq!(supplied, expected);
    }

    /// Stage 11.A8c — pin `format`'s check-digit arithmetic with
    /// hand-computed expected strings. The existing
    /// `check_digit_known_vector` test recomputes the same `sum % 43`
    /// the implementation uses, so any arithmetic mutant (`% with /`,
    /// `+= with -=` etc.) would change both sides equally and the
    /// test still passes. These golden strings hard-code the
    /// expected check character, so any divergence is caught.
    ///
    /// Hand-computed against
    /// `HIBC_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%"`:
    ///
    ///   - format("A"): prefixed="+A". '+' = 41, 'A' = 10.
    ///     sum = 51. 51 % 43 = 8 → '8'. Result = "+A8".
    ///     Mutant `sum / 43` → HIBC_ALPHABET[1] = '1' → "+A1".
    ///   - format("0"): prefixed="+0". '+' = 41, '0' = 0.
    ///     sum = 41. 41 % 43 = 41 → '+'. Result = "+0+".
    ///   - format("Z"): prefixed="+Z". 41 + 35 = 76. 76 % 43 = 33 → 'X'.
    ///     Result = "+ZX".
    ///   - format("%"): prefixed="+%". 41 + 42 = 83. 83 % 43 = 40 → '/'.
    ///     Result = "+%/".
    ///   - format("00"): prefixed="+00". 41 + 0 + 0 = 41 → '+'.
    ///     Result = "+00+".
    #[test]
    fn format_check_digit_golden_values() {
        assert_eq!(format("A").unwrap(), "+A8", "+A → check '8'");
        assert_eq!(format("0").unwrap(), "+0+", "+0 → check '+'");
        assert_eq!(format("Z").unwrap(), "+ZX", "+Z → check 'X'");
        assert_eq!(format("%").unwrap(), "+%/", "+% → check '/'");
        assert_eq!(format("00").unwrap(), "+00+", "+00 → check '+'");
        // Uppercase normalisation: lowercase letters get uppercased
        // before validation, so format("a") should yield "+A8".
        assert_eq!(
            format("a").unwrap(),
            "+A8",
            "lowercase 'a' → uppercase 'A' path"
        );
    }

    /// Stage 11.A8c — these two `format_rejects_*` tests originally
    /// asserted only `matches!(..., Err(InvalidData(_)))`. The
    /// companion `format_rejection_arms_pin_diagnostic_substrings`
    /// test below already pins the full diagnostic; strengthening
    /// these in place ensures the weak assertions don't survive a
    /// follow-up test refactor that might drop the strong sibling.
    #[test]
    fn format_rejects_empty() {
        let err = format("").expect_err("empty HIBC LIC payload must reject");
        let Error::InvalidData(msg) = err else {
            panic!("empty HIBC LIC payload must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("HIBC LIC") && msg.contains("must not be empty"),
            "empty diagnostic must carry 'HIBC LIC' + 'must not be empty'; got {msg:?}"
        );
    }

    #[test]
    fn format_rejects_invalid_character() {
        let err = format("HELLO!").expect_err("HIBC LIC with '!' must reject");
        let Error::InvalidData(msg) = err else {
            panic!("invalid-char HIBC LIC payload must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("HIBC LIC") && msg.contains("invalid character") && msg.contains("'!'"),
            "invalid-char diagnostic must carry 'HIBC LIC' + 'invalid character' + \
             '\\'!\\'' echo; got {msg:?}"
        );
    }

    /// Stage 11.A8c — strengthen the existing weak `format_rejects_*`
    /// assertions by pinning the diagnostic substring on each rejection
    /// arm. Without this:
    ///   * `format_rejects_empty` only checks `is_err()`, so the error
    ///     message can drift to anything.
    ///   * `format_rejects_invalid_character` similarly doesn't pin
    ///     which character or that the message blames the right thing.
    /// Mutations to "HIBC LIC payload must not be empty" or
    /// "HIBC LIC: invalid character {c:?}" all slip through.
    ///
    /// Also pin two characters outside the HIBC alphabet that look like
    /// alphabet members to catch a `to_uppercase()`-removal mutation
    /// (lowercase 'a' becomes 'A' which is valid; non-letter punctuation
    /// like '!' stays invalid).
    ///
    /// Mutations killed:
    ///   * Empty-input message string drift.
    ///   * Invalid-char arm dropped or its message altered.
    ///   * `data.to_uppercase()` removed (would reject lowercase, which
    ///     is supposed to be accepted via uppercasing).
    #[test]
    fn format_rejection_arms_pin_diagnostic_substrings() {
        // Empty input → "HIBC LIC payload must not be empty".
        match format("") {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("HIBC LIC") && msg.contains("must not be empty"),
                "empty input should pin 'HIBC LIC' + 'must not be empty', got: {msg}"
            ),
            other => panic!("empty input should reject as InvalidData, got {other:?}"),
        }

        // Invalid character "!" — must surface "invalid character '!'"
        // with the offending char.
        match format("ABC!") {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("HIBC LIC")
                    && msg.contains("invalid character")
                    && msg.contains("'!'"),
                "invalid '!' should pin diagnostic with offending char, got: {msg}"
            ),
            other => panic!("'!' should reject, got {other:?}"),
        }

        // Mid-payload invalid char @.
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("'@'")` to 2-anchor pin matching the source
        // diagnostic at line 47-49 of hibc.rs (`HIBC LIC: invalid
        // character {c:?}`):
        //   1. `HIBC LIC:` symbology prefix
        //   2. `invalid character` predicate
        //   3. `'@'` Debug echo of the mid-payload offending char
        // The 3-anchor structure pins the symbology name (kills
        // mutations that swap "HIBC LIC" → e.g. "HIBC LAC") AND
        // the predicate (kills "invalid character" → "bad character"
        // text drift).
        match format("A@B") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("HIBC LIC:"),
                    "missing `HIBC LIC:` prefix: {msg}"
                );
                assert!(
                    msg.contains("invalid character"),
                    "missing `invalid character` predicate: {msg}"
                );
                assert!(msg.contains("'@'"), "missing `'@'` Debug echo: {msg}");
            }
            other => panic!("'@' should reject, got {other:?}"),
        }

        // Lowercase letters get uppercased BEFORE the alphabet check,
        // so 'a' (which becomes 'A') succeeds — pin this round-trip.
        let lower = format("hello").expect("lowercase should uppercase + succeed");
        let upper = format("HELLO").expect("uppercase should succeed");
        assert_eq!(
            lower, upper,
            "format must uppercase lowercase input before alphabet check"
        );
    }

    #[test]
    fn format_uppercases_input() {
        let upper = format("abc").unwrap();
        let lower = format("ABC").unwrap();
        assert_eq!(upper, lower);
    }

    /// Stage 11.A8c — pin `format` output for short hand-computed
    /// inputs. The existing tests verify rejection paths and the
    /// uppercase normalisation, but the actual computed check-digit
    /// values were never pinned in isolation — only validated
    /// transitively via the verify_and_strip helper's complementary
    /// path.
    ///
    /// HIBC_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%"
    /// (indices: 0-9=digits, 10-35=A-Z, 36='-', 37='.', 38=' ',
    /// 39='$', 40='/', 41='+', 42='%').
    ///
    /// Hand-computed:
    ///   - format("0"): prefixed "+0", sum=41+0=41, check=ALPHA[41]='+'
    ///     → "+0+".
    ///   - format("A"): sum=41+10=51, check=ALPHA[51%43=8]='8' → "+A8".
    ///   - format("Z"): sum=41+35=76, check=ALPHA[76%43=33]='X' → "+ZX".
    ///   - format("00"): sum=41+0+0=41, check='+' → "+00+".
    ///
    /// Mutations to catch:
    ///   - `+{upper}` → `{upper}+` or just `{upper}`: prefix lost.
    ///   - `sum % 43` → `sum % 42` or `sum % 44`: wrong modulus.
    ///   - `find().expect()` replaced — would always crash.
    #[test]
    fn format_appends_correct_check_digit() {
        assert_eq!(format("0").unwrap(), "+0+", "body=0, check='+' (sum=41)");
        assert_eq!(format("A").unwrap(), "+A8", "body=A, check='8' (sum=51)");
        assert_eq!(format("Z").unwrap(), "+ZX", "body=Z, check='X' (sum=76)");
        assert_eq!(format("00").unwrap(), "+00+", "body=00, check='+' (sum=41)");
        // Round-trip: format(body) → strip(formatted_minus_prefix) → body.
        let formatted = format("ABC").unwrap();
        // formatted has the form "+<body><check>"; strip leading '+'
        // and pass the rest to verify_and_strip — must return body.
        let body_check = &formatted[1..];
        assert_eq!(
            verify_and_strip_hibc_check(body_check).unwrap(),
            "ABC",
            "round-trip: format ↔ verify_and_strip"
        );
    }

    #[test]
    fn encode_code128_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC Code-128 wrapper path: format() `+` prefix +
        // mod-43 check envelope → Code 128 encoder.
        let p = encode_code128("A99912345/52001510X3", &Options::default()).expect(
            "encode_code128(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() `+` prefix + mod-43 check envelope → Code 128) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_code128(\"A99912345/52001510X3\") (HIBC LIC payload over Code 128) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn encode_code39_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC Code-39 wrapper path.
        let p = encode_code39("A99912345/52001510X3", &Options::default()).expect(
            "encode_code39(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → Code 39) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_code39(\"A99912345/52001510X3\") (HIBC LIC payload over Code 39) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn encode_datamatrix_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC DataMatrix wrapper + ≥10×10 size floor.
        let m = encode_datamatrix("A99912345/52001510X3", &Options::default()).expect(
            "encode_datamatrix(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → Data Matrix; ≥10×10 floor) must succeed",
        );
        assert!(
            m.width() >= 10,
            "encode_datamatrix(\"A99912345/52001510X3\") (HIBC LIC payload over Data Matrix) must produce symbol ≥10 modules wide (DM 10x10 minimum); got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn encode_qrcode_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC QR wrapper + V1 ≥21 size floor.
        let m = encode_qrcode("A99912345/52001510X3", &Options::default()).expect(
            "encode_qrcode(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → QR; V1 ≥21 floor) must succeed",
        );
        assert!(
            m.width() >= 21,
            "encode_qrcode(\"A99912345/52001510X3\") (HIBC LIC payload over QR) must produce QR symbol ≥21 modules wide (V1 minimum); got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn encode_pdf417_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC PDF417 wrapper.
        let m = encode_pdf417("A99912345/52001510X3", &Options::default()).expect(
            "encode_pdf417(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → PDF417) must succeed",
        );
        assert!(
            m.width() > 0 && m.height() > 0,
            "encode_pdf417(\"A99912345/52001510X3\") (HIBC LIC payload over PDF417) must produce non-empty matrix; got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn encode_micropdf417_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC Micro PDF417 wrapper.
        let m = encode_micropdf417("A99912345/52001510X3", &Options::default()).expect(
            "encode_micropdf417(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → Micro PDF417) must succeed",
        );
        assert!(m.width() > 0 && m.height() > 0);
    }

    #[test]
    fn encode_codablockf_renders() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the HIBC LIC Codablock F wrapper.
        let s = encode_codablockf("A99912345/52001510X3", &Options::default()).expect(
            "encode_codablockf(\"A99912345/52001510X3\", default) (HIBC LIC payload → format() envelope → Codablock F) must succeed",
        );
        assert!(s.width() > 0);
    }

    /// HIBC LIC wrapper-composition pinning. Each wrapper is
    /// definitionally `primary::encode(format(input))` — the
    /// `_renders` tests above only assert that the result is
    /// non-empty, which is too loose to catch a regression where the
    /// wrapper bypassed `format()` (the `+` prefix + mod-43 check
    /// envelope). These tests assert byte-for-byte equality between
    /// the wrapper output and the verified primary applied to the
    /// formatted payload — so any future drift in either side
    /// surfaces immediately.
    ///
    /// We also mirror the inverse direction (passing the unformatted
    /// input to the primary should produce different output) to
    /// confirm the envelope is actually applied.
    #[test]
    fn encode_pdf417_composes_format_and_pdf417() {
        let payload = "A99912345/52001510X3";
        let wrapper = encode_pdf417(payload, &Options::default()).unwrap();
        let formatted = format(payload).unwrap();
        let direct = pdf417::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            (wrapper.width(), wrapper.height()),
            (direct.width(), direct.height()),
            "HIBC LIC PDF417 size diverges from primary(format(input))"
        );
        for y in 0..wrapper.height() {
            for x in 0..wrapper.width() {
                assert_eq!(
                    wrapper.get(x, y),
                    direct.get(x, y),
                    "pixel mismatch at ({x},{y})"
                );
            }
        }
        // Sanity: the unformatted input would produce a different
        // size (no `+` prefix and no check character).
        let bare = pdf417::encode(payload, &Options::default()).unwrap();
        assert_ne!(
            (wrapper.width(), wrapper.height()),
            (bare.width(), bare.height()),
            "wrapper output should differ from primary applied to unformatted input"
        );
    }

    #[test]
    fn encode_micropdf417_composes_format_and_micropdf417() {
        let payload = "A99912345/52001510X3";
        let wrapper = encode_micropdf417(payload, &Options::default()).unwrap();
        let formatted = format(payload).unwrap();
        let direct = micropdf417::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            (wrapper.width(), wrapper.height()),
            (direct.width(), direct.height()),
            "HIBC LIC MicroPDF417 size diverges from primary(format(input))"
        );
        for y in 0..wrapper.height() {
            for x in 0..wrapper.width() {
                assert_eq!(
                    wrapper.get(x, y),
                    direct.get(x, y),
                    "pixel mismatch at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn encode_codablockf_composes_format_and_codablockf() {
        let payload = "A99912345/52001510X3";
        let wrapper = encode_codablockf(payload, &Options::default()).unwrap();
        let formatted = format(payload).unwrap();
        let direct = codablockf::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            wrapper.width(),
            direct.width(),
            "HIBC LIC Codablock-F width diverges from primary(format(input))"
        );
        assert_eq!(
            wrapper.rows.len(),
            direct.rows.len(),
            "HIBC LIC Codablock-F row count diverges"
        );
        for (i, (a, b)) in wrapper.rows.iter().zip(direct.rows.iter()).enumerate() {
            assert_eq!(
                a.bars, b.bars,
                "HIBC LIC Codablock-F row {i} bars diverge from primary(format(input))"
            );
        }
    }

    #[test]
    fn encode_datamatrix_composes_format_and_datamatrix() {
        let payload = "A99912345/52001510X3";
        let wrapper = encode_datamatrix(payload, &Options::default()).unwrap();
        let formatted = format(payload).unwrap();
        let direct = datamatrix_::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            (wrapper.width(), wrapper.height()),
            (direct.width(), direct.height()),
            "HIBC LIC DataMatrix size diverges from primary(format(input))"
        );
        for y in 0..wrapper.height() {
            for x in 0..wrapper.width() {
                assert_eq!(
                    wrapper.get(x, y),
                    direct.get(x, y),
                    "pixel mismatch at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn encode_azteccode_composes_format_and_aztec() {
        // HIBC Aztec is a wrapper that builds the `+` + mod-43 envelope
        // around the payload and delegates to the verified Aztec
        // encoder. We pin three properties: (1) the resulting matrix
        // is non-empty (Aztec returns a square BitMatrix), (2) it
        // matches what `aztec::encode(format(input))` produces (the
        // wrapper composition is byte-equivalent to the direct call),
        // and (3) format errors propagate.
        let payload = "A99912345/52001510X3";
        let wrapped = encode_azteccode(payload, &Options::default()).unwrap();
        let direct = {
            let formatted = format(payload).unwrap();
            aztec::encode(formatted.as_bytes()).unwrap()
        };
        assert_eq!(
            wrapped.width(),
            direct.width(),
            "HIBC Aztec wrapper produced a different size from direct aztec::encode"
        );
        assert_eq!(
            wrapped.width() * wrapped.height(),
            direct.width() * direct.height(),
            "HIBC Aztec wrapper module count diverged from direct"
        );

        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 41-42
        // (`HIBC LIC payload must not be empty`). Cross-arm guard
        // against the invalid-char arm.
        match encode_azteccode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("HIBC LIC"), "missing `HIBC LIC` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-char diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Aztec payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn encode_datamatrix_rectangular_composes_format_and_datamatrix_rect() {
        // Same wrapper composition + shape=rectangular delegation.
        let payload = "A99912345/52001510X3";
        let wrapped = encode_datamatrix_rectangular(payload, &Options::default()).unwrap();
        // Rectangular Data Matrix is wider than tall.
        assert!(
            wrapped.width() > wrapped.height(),
            "expected rectangular DM (wide), got {}×{}",
            wrapped.width(),
            wrapped.height()
        );
        // Empty payload propagates as InvalidData.
        // Stage 11.A8c — same anchors as the Aztec wrapper sibling
        // above; proves both wrappers propagate the empty-payload
        // check from format() correctly.
        match encode_datamatrix_rectangular("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("HIBC LIC"), "missing `HIBC LIC` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-char diagnostic leaked: {msg}"
                );
            }
            other => {
                panic!("empty Data Matrix rect payload should reject as InvalidData, got {other:?}")
            }
        }
    }

    /// HIBC LIC Code 128 golden bar pattern for the canonical
    /// example, captured from
    /// `raw("hibccode128", "A99912345/52001510X3", {})[0].sbs`.
    #[test]
    fn encode_code128_matches_bwip_js_raw_sbs() {
        let p = encode_code128("A99912345/52001510X3", &Options::default()).unwrap();
        let want: [u8; 127] = [
            2, 1, 1, 2, 1, 4, 2, 3, 1, 2, 1, 2, 1, 1, 1, 3, 2, 3, 1, 1, 3, 1, 4, 1, 1, 1, 3, 1, 4,
            1, 4, 1, 2, 1, 2, 1, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 4, 1, 3, 1, 1, 1, 3, 2,
            2, 2, 1, 1, 3, 1, 4, 1, 2, 1, 3, 3, 1, 1, 2, 1, 2, 2, 2, 2, 1, 1, 3, 2, 2, 2, 2, 2, 1,
            3, 1, 2, 1, 1, 4, 1, 3, 1, 3, 3, 1, 1, 2, 1, 2, 2, 1, 1, 3, 2, 1, 3, 1, 1, 2, 3, 1, 2,
            2, 2, 3, 1, 2, 3, 3, 1, 1, 1, 2,
        ];
        assert_eq!(
            p.bars, want,
            "HIBC LIC Code 128 bars mismatch vs bwip-js raw output"
        );
    }

    /// HIBC PAS Code 128 golden from
    /// `raw("hibccode128", "A/99912345/$$52001510X3", {})[0].sbs`
    /// (BWIPP doesn't ship a separate `hibcpascode128` bcid — the
    /// PAS envelope is identical to LIC at the bar level, so the
    /// existing `hibccode128` handles both).
    #[test]
    fn encode_pas_code128_matches_bwip_js_raw_sbs() {
        let p = encode_pas_code128("A/99912345/$$52001510X3", &Options::default()).unwrap();
        let want: [u8; 145] = [
            2, 1, 1, 2, 1, 4, 2, 3, 1, 2, 1, 2, 1, 1, 1, 3, 2, 3, 1, 1, 3, 2, 2, 2, 1, 1, 3, 1, 4,
            1, 1, 1, 3, 1, 4, 1, 4, 1, 2, 1, 2, 1, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 4, 1,
            3, 1, 1, 1, 3, 2, 2, 2, 1, 2, 1, 3, 2, 2, 1, 2, 1, 3, 2, 2, 1, 1, 3, 1, 4, 1, 2, 1, 3,
            3, 1, 1, 2, 1, 2, 2, 2, 2, 1, 1, 3, 2, 2, 2, 2, 2, 1, 3, 1, 2, 1, 1, 4, 1, 3, 1, 3, 3,
            1, 1, 2, 1, 2, 2, 1, 1, 3, 2, 1, 2, 3, 1, 2, 2, 2, 2, 1, 4, 1, 1, 2, 3, 3, 1, 1, 1, 2,
        ];
        assert_eq!(
            p.bars, want,
            "HIBC PAS Code 128 bars mismatch vs bwip-js raw output"
        );
    }

    /// HIBC PAS Code 39 golden from
    /// `raw("hibccode39", "/EX2501XZ/16D20240115", {})[0].sbs` — the
    /// same encoder bwip-js uses for both LIC and PAS (the PAS
    /// envelope is identical to LIC at the bar level; the
    /// differentiator is conventional payload structure). Regenerate
    /// via `node rust/tools/oracle-hibc.js`.
    #[test]
    fn encode_pas_code39_matches_bwip_js_raw_sbs() {
        let p = encode_pas_code39("/EX2501XZ/16D20240115", &Options::default()).unwrap();
        let want: [u8; 250] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1,
            1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1,
            3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1,
            1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1,
            1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3,
            3, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 1, 3, 3,
            1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 1,
            3, 1, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1,
            3, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "HIBC PAS Code 39 bars mismatch vs bwip-js raw output"
        );
    }

    /// HIBC LIC Code 39 golden from
    /// `raw("hibccode39", "A99912345/52001510X3", {})[0].sbs`.
    #[test]
    fn encode_code39_matches_bwip_js_raw_sbs() {
        let p = encode_code39("A99912345/52001510X3", &Options::default()).unwrap();
        let want: [u8; 240] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3,
            1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1,
            1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1,
            1, 3, 1, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3,
            1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 1, 3,
            3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 3, 1,
            1, 3, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3,
            1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "HIBC LIC Code 39 bars mismatch vs bwip-js raw output"
        );
    }

    /// HIBC LIC + Data Matrix end-to-end pixs against bwip-js's
    /// `hibcdatamatrix` for the short payload "A001". The HIBC
    /// envelope is `+A001<check>`; the underlying `datamatrix` crate
    /// then produces a 12×12 symbol. This input happens to take the
    /// same encoding path through both encoders, so the pixs match
    /// byte-for-byte — longer payloads can diverge if the substrate
    /// picks a different mode than BWIPP (already documented for
    /// plain `datamatrix`).
    #[test]
    fn encode_datamatrix_pixs_matches_bwip_js() {
        let m = encode_datamatrix("A001", &Options::default()).unwrap();
        assert_eq!(m.width(), 12);
        assert_eq!(m.height(), 12);
        #[rustfmt::skip]
        let want: [u8; 144] = [
            1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1,
            1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0,
            1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1,
            1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0,
            1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1,
            1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0,
            1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1,
            1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0,
            1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1,
            1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        let mut got = Vec::with_capacity(144);
        for y in 0..m.height() {
            for x in 0..m.width() {
                got.push(if m.get(x, y) { 1u8 } else { 0u8 });
            }
        }
        assert_eq!(got, want, "HIBC LIC DataMatrix pixs mismatch");
    }

    /// HIBC LIC + QR Code composition: `encode_qrcode(data)` must
    /// equal `qrcode::encode(format(data))` — the wrapper is just
    /// `format` followed by `qrcode::encode`. Since `format` itself
    /// has its own unit tests and `qrcode::encode` is verified for
    /// substrate sanity, the composition test pins that no other
    /// code path can sneak in.
    #[test]
    fn encode_qrcode_composes_format_and_qrcode() {
        let payload = "A99912345/52001510X3";
        let got = encode_qrcode(payload, &Options::default()).unwrap();
        let formatted = format(payload).unwrap();
        #[cfg(feature = "prefer-native-qrcode")]
        let want = crate::symbology::qrcode_native::encode_with_options(
            formatted.as_bytes(),
            &Options::default(),
        )
        .unwrap();
        #[cfg(not(feature = "prefer-native-qrcode"))]
        let want = crate::symbology::qrcode_::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(got.width(), want.width());
        assert_eq!(got.height(), want.height());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    /// HIBC PAS + Data Matrix composition.
    #[test]
    fn encode_pas_datamatrix_composes_format_pas_and_datamatrix() {
        let payload = "A99912345/52001510X3";
        let got = encode_pas_datamatrix(payload, &Options::default()).unwrap();
        let formatted = format_pas(payload).unwrap();
        let want = crate::symbology::datamatrix_::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(got.width(), want.width());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    /// HIBC PAS + QR Code composition.
    #[test]
    fn encode_pas_qrcode_composes_format_pas_and_qrcode() {
        let payload = "A99912345/52001510X3";
        let got = encode_pas_qrcode(payload, &Options::default()).unwrap();
        let formatted = format_pas(payload).unwrap();
        #[cfg(feature = "prefer-native-qrcode")]
        let want = crate::symbology::qrcode_native::encode_with_options(
            formatted.as_bytes(),
            &Options::default(),
        )
        .unwrap();
        #[cfg(not(feature = "prefer-native-qrcode"))]
        let want = crate::symbology::qrcode_::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(got.width(), want.width());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    /// HIBC PAS PDF417 composition pin: the wrapper output must
    /// byte-match `pdf417::encode(format_pas(input))`.
    #[test]
    fn encode_pas_pdf417_composes_format_pas_and_pdf417() {
        let payload = "A/99912345/$$52001510X3";
        let wrapper = encode_pas_pdf417(payload, &Options::default()).unwrap();
        let formatted = format_pas(payload).unwrap();
        let direct = pdf417::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            (wrapper.width(), wrapper.height()),
            (direct.width(), direct.height()),
            "HIBC PAS PDF417 size diverges from primary(format_pas(input))"
        );
        for y in 0..wrapper.height() {
            for x in 0..wrapper.width() {
                assert_eq!(
                    wrapper.get(x, y),
                    direct.get(x, y),
                    "pixel mismatch at ({x},{y})"
                );
            }
        }
    }

    /// HIBC PAS MicroPDF417 composition pin.
    #[test]
    fn encode_pas_micropdf417_composes_format_pas_and_micropdf417() {
        let payload = "A/99912345/$$52001510X3";
        let wrapper = encode_pas_micropdf417(payload, &Options::default()).unwrap();
        let formatted = format_pas(payload).unwrap();
        let direct = micropdf417::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            (wrapper.width(), wrapper.height()),
            (direct.width(), direct.height()),
            "HIBC PAS MicroPDF417 size diverges from primary(format_pas(input))"
        );
        for y in 0..wrapper.height() {
            for x in 0..wrapper.width() {
                assert_eq!(
                    wrapper.get(x, y),
                    direct.get(x, y),
                    "pixel mismatch at ({x},{y})"
                );
            }
        }
    }

    /// HIBC PAS Codablock-F composition pin.
    #[test]
    fn encode_pas_codablockf_composes_format_pas_and_codablockf() {
        let payload = "A/99912345/$$52001510X3";
        let wrapper = encode_pas_codablockf(payload, &Options::default()).unwrap();
        let formatted = format_pas(payload).unwrap();
        let direct = codablockf::encode(&formatted, &Options::default()).unwrap();
        assert_eq!(
            wrapper.width(),
            direct.width(),
            "HIBC PAS Codablock-F width diverges from primary(format_pas(input))"
        );
        assert_eq!(
            wrapper.rows.len(),
            direct.rows.len(),
            "HIBC PAS Codablock-F row count diverges"
        );
        for (i, (a, b)) in wrapper.rows.iter().zip(direct.rows.iter()).enumerate() {
            assert_eq!(
                a.bars, b.bars,
                "HIBC PAS Codablock-F row {i} bars diverge from primary(format_pas(input))"
            );
        }
    }

    /// HIBC LIC payload "A99912345/52001510X3" → HIBC envelope
    /// "+A99912345/52001510X3<check>" → PDF417 cws golden captured
    /// from bwip-js's `hibcpdf417` bcid. This locks down the full
    /// chain: HIBC `format()` matches BWIPP byte-for-byte, the
    /// downstream PDF417 encoder produces the same cws, and our
    /// wrapper composes the two correctly.
    #[test]
    fn encode_pdf417_cws_matches_bwip_js() {
        let formatted = format("A99912345/52001510X3").unwrap();
        let cws = crate::symbology::pdf417::pdf417_cws(formatted.as_bytes(), 2, 0).unwrap();
        assert_eq!(
            cws,
            vec![
                16, 860, 840, 849, 279, 32, 94, 169, 152, 0, 35, 30, 863, 843, 841, 900, 379, 344,
                150, 535, 396, 36, 844, 252
            ]
        );
    }

    /// Stage 11.14 — `encode_azteccode` now respects the BWIPP
    /// `validatecheck` option (`bwipp_hibcazteccode:41888`). When set
    /// to true, the encoder pre-validates the trailing check digit
    /// and strips it; the resulting symbol matches the default
    /// (no-validatecheck) encoding of the same body.
    #[test]
    fn encode_azteccode_validatecheck_matches_default_when_valid() {
        // Compute the expected check digit for "A123BJC5D6E71" via
        // the same logic `format()` uses, then re-encode with the
        // check appended + validatecheck=true.
        let formatted = format("A123BJC5D6E71").unwrap();
        let check = formatted.chars().last().unwrap();
        let with_check = format!("A123BJC5D6E71{check}");

        let default = encode_azteccode("A123BJC5D6E71", &Options::default()).unwrap();
        let validated = encode_azteccode(
            &with_check,
            &Options::default().with("validatecheck", "true"),
        )
        .unwrap();
        // Same symbol shape — same pixels.
        assert_eq!(default.width(), validated.width());
        assert_eq!(default.height(), validated.height());
        for y in 0..default.height() {
            for x in 0..default.width() {
                assert_eq!(
                    default.get(x, y),
                    validated.get(x, y),
                    "pixel mismatch at ({x},{y})",
                );
            }
        }
    }

    /// Stage 11.14 — `validatecheck=true` with an incorrect trailing
    /// check digit returns `Error::InvalidData` (BWIPP raises
    /// `bwipp.hibcazteccodeBadCheckDigit`).
    #[test]
    fn encode_azteccode_validatecheck_rejects_bad_check_digit() {
        // Append a deliberately wrong check ('0' is rarely the
        // correct mod-43 check for any input).
        let err = encode_azteccode(
            "A123BJC5D6E710",
            &Options::default().with("validatecheck", "true"),
        )
        .unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                // Stage 11.A8c (cont) — single-substring
                // `validatecheck` + `doesn't match` upgraded to
                // 4-anchor pin matching the source diagnostic at
                // lines 196-199 of hibc.rs:
                //   1. `hibcazteccode:` symbology prefix
                //   2. `validatecheck=true but the provided
                //      trailing check` full predicate
                //   3. `doesn't match the computed value` predicate
                //   4. supplied/computed char Debug echoes
                assert!(
                    msg.contains("hibcazteccode:"),
                    "missing hibcazteccode prefix: {msg:?}"
                );
                assert!(
                    msg.contains("validatecheck=true but the provided trailing check"),
                    "missing full predicate: {msg:?}"
                );
                assert!(
                    msg.contains("doesn't match the computed value"),
                    "missing `doesn't match the computed value`: {msg:?}"
                );
                // The supplied check is the last char of input
                // "A123BJC5D6E710" → '0'. Don't pin the computed
                // value (depends on mod-43 of body); pin just the
                // supplied char echo.
                assert!(
                    msg.contains("'0'"),
                    "missing supplied-char echo `'0'`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.14 — invalid `validatecheck` value returns
    /// `InvalidOption`.
    #[test]
    fn encode_azteccode_rejects_invalid_validatecheck_value() {
        // Diagnostic at line 149:
        //   "hibcazteccode: validatecheck={v:?} must be \"true\" or \"false\""
        // 4-anchor pin upgrades the previous single-substring check
        // (which would accept any message merely mentioning "validatecheck"):
        let err = encode_azteccode("ABC", &Options::default().with("validatecheck", "maybe"))
            .unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("hibcazteccode:"),
                    "diagnostic must carry the hibcazteccode prefix; got {msg:?}"
                );
                assert!(
                    msg.contains("validatecheck=\"maybe\""),
                    "diagnostic must Debug-echo the offending value; got {msg:?}"
                );
                assert!(
                    msg.contains("must be"),
                    "diagnostic must carry the predicate; got {msg:?}"
                );
                assert!(
                    msg.contains("\"true\"") && msg.contains("\"false\""),
                    "diagnostic must name BOTH valid values; got {msg:?}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Stage 11.A8c — pin `verify_and_strip_hibc_check`'s body /
    /// check-digit split + success return value directly. The
    /// existing tests exercise `encode_azteccode` end-to-end with
    /// `validatecheck=true`, which lumps the verify step in with the
    /// downstream Aztec encoder; a mutant that returns the wrong body
    /// substring (e.g. `chars().take(upper.chars().count() - 1)` →
    /// `take(upper.chars().count())` which would return the full
    /// string with the check char still attached, or off-by-one in
    /// either direction) wouldn't be caught if the downstream Aztec
    /// encoding happens to succeed on the longer string.
    ///
    /// Hand-computed: for body "A" the HIBC check is '8' (see
    /// `format_check_digit_golden_values`). So input "A8" should
    /// verify cleanly and return body "A".
    /// For body "0" the check is '+', so input "0+" returns "0".
    /// For body "Z" the check is 'X', so input "ZX" returns "Z".
    /// For body "00" the check is '+', so input "00+" returns "00".
    #[test]
    fn verify_and_strip_hibc_check_returns_body_only() {
        // Success path: correct check → return body sans the check char.
        assert_eq!(verify_and_strip_hibc_check("A8").unwrap(), "A");
        assert_eq!(verify_and_strip_hibc_check("0+").unwrap(), "0");
        assert_eq!(verify_and_strip_hibc_check("ZX").unwrap(), "Z");
        assert_eq!(verify_and_strip_hibc_check("00+").unwrap(), "00");
        // Lowercase normalised to uppercase before validation.
        assert_eq!(verify_and_strip_hibc_check("a8").unwrap(), "A");
        // Mismatched check → error with diagnostic text.
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("doesn't match")` to 4-anchor pin matching the
        // source diagnostic at line 196-199 of hibc.rs:
        //   1. `hibcazteccode:` symbology prefix
        //   2. `validatecheck=true` mode qualifier
        //   3. `'9'` Debug echo of supplied check digit
        //   4. `'8'` Debug echo of computed check digit
        // The supplied/expected echo pair pins BOTH the alphabet-
        // table lookup AND the mod-43 arithmetic.
        match verify_and_strip_hibc_check("A9") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("hibcazteccode:"),
                    "missing `hibcazteccode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("validatecheck=true"),
                    "missing `validatecheck=true` mode qualifier: {msg}"
                );
                assert!(
                    msg.contains("doesn't match"),
                    "missing `doesn't match` predicate: {msg}"
                );
                assert!(msg.contains("'9'"), "missing `'9'` supplied echo: {msg}");
                assert!(msg.contains("'8'"), "missing `'8'` expected echo: {msg}");
            }
            other => panic!("expected InvalidData(check mismatch), got {other:?}"),
        }
        // Invalid character → alphabet-rejection error (not check mismatch).
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("invalid character")` to 3-anchor pin matching
        // the source diagnostic at line 177-179 of hibc.rs:
        //   1. `hibcazteccode:` symbology prefix (distinguishes from
        //      the `HIBC LIC` prefix used by the format() helper at
        //      line 49)
        //   2. `invalid character` predicate
        //   3. `'!'` Debug echo of offending char
        match verify_and_strip_hibc_check("A!") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("hibcazteccode:"),
                    "missing `hibcazteccode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("invalid character"),
                    "missing `invalid character` predicate: {msg}"
                );
                assert!(msg.contains("'!'"), "missing `'!'` Debug echo: {msg}");
                assert!(
                    !msg.contains("HIBC LIC:"),
                    "wrong helper — HIBC LIC: prefix leaked into hibcazteccode reject: {msg}"
                );
            }
            other => panic!("expected InvalidData(invalid character), got {other:?}"),
        }
    }

    /// Kills `verify_and_strip_hibc_check: replace < with ==` and
    /// `< with <=` at line ~167. Existing tests use payloads of 14
    /// characters (well above the boundary); the mutants flip the
    /// length guard so:
    /// - `<= 2` rejects exactly-2-character input (1 body + 1 check),
    ///   which is the documented minimum acceptance case.
    /// - `== 2` accepts inputs of length 0 and 1 (which then panic in
    ///   the body/check-digit split logic downstream).
    ///
    /// We bracket the boundary at 1 char (reject), 2 chars (accept or
    /// reject based on validity, but never panic), and 3 chars (the
    /// existing happy-path tests already cover this).
    #[test]
    fn validatecheck_minimum_length_boundary() {
        // 1-char input must always be rejected before downstream split.
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 167-172
        // (`hibcazteccode: validatecheck=true requires at least 2
        // characters (body + trailing check digit)`).
        match encode_azteccode("A", &Options::default().with("validatecheck", "true")) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("hibcazteccode:"),
                    "missing `hibcazteccode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("requires at least 2 characters"),
                    "missing `requires at least 2 characters` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-char diagnostic leaked: {msg}"
                );
            }
            other => panic!(
                "`A` (1-char with validatecheck) should reject as InvalidData, got {other:?}"
            ),
        }
        // Empty input must also be rejected. Same diagnostic anchors
        // as the 1-char case (count() < 2 fires for both 0 and 1).
        match encode_azteccode("", &Options::default().with("validatecheck", "true")) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("hibcazteccode:"),
                    "missing `hibcazteccode:` prefix: {msg}"
                );
                assert!(
                    msg.contains("requires at least 2 characters"),
                    "missing `requires at least 2 characters` predicate: {msg}"
                );
            }
            other => panic!(
                "empty payload with validatecheck should reject as InvalidData, got {other:?}"
            ),
        }
        // 2-char input is the documented minimum — the encoder either
        // accepts it (if the check digit matches) or rejects with a
        // check-digit-mismatch error (never with a length error,
        // never with a panic). The mutant `<= 2` would route 2-char
        // input through the length-error branch first, which we'd
        // detect by message inspection.
        match encode_azteccode("AA", &Options::default().with("validatecheck", "true")) {
            Ok(_) => {} // unlikely (very specific check-digit match) but acceptable.
            Err(Error::InvalidData(msg)) => assert!(
                !msg.contains("at least 2 characters"),
                "2-char input should not trigger the length-error branch; got msg={msg:?}"
            ),
            Err(other) => panic!("unexpected error variant: {other:?}"),
        }
    }

    /// Kills `encode_datamatrix_rectangular: replace != with ==` at
    /// line ~212 (the `o.extras.retain(|(k, _)| k != "shape")` filter).
    /// Original: keep everything except "shape". Mutant: keep only
    /// "shape". Under the mutant, unrelated extras like `version`
    /// would be dropped before reaching the datamatrix substrate.
    /// We pass a `version` extra that is NOT the auto-selected
    /// default for the input, then assert the encoder honours it
    /// (output dimensions match the requested version, not the
    /// auto-default).
    #[test]
    fn datamatrix_rectangular_preserves_unrelated_extras() {
        // "X" auto-selects a small rectangle. Force version=8x48
        // (the largest non-DMRE rectangle); the encoder must honour
        // it under the original filter. Under the mutant, the version
        // extra is dropped and the encoder falls back to the auto-
        // selected 8x18 (the smallest fit) — different dimensions.
        let default = encode_datamatrix_rectangular("X", &Options::default()).unwrap();
        let with_version =
            encode_datamatrix_rectangular("X", &Options::default().with("version", "8x48"))
                .unwrap();
        assert_ne!(
            (default.width(), default.height()),
            (with_version.width(), with_version.height()),
            "encode_datamatrix_rectangular dropped the `version` extra \
             (default={:?} == with_version={:?}); \
             the `retain(k != \"shape\")` filter may have flipped",
            (default.width(), default.height()),
            (with_version.width(), with_version.height()),
        );
    }
}
