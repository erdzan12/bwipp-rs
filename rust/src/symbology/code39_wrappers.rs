//! Symbologies that are thin wrappers over Code 39.
//!
//! * **VIN** — 17-character vehicle ID; we validate the character set (no
//!   `I`, `O`, `Q`) and delegate to Code 39.
//! * **LOGMARS** — US DoD MIL-STD-1189B; Code 39 with the mod-43 check
//!   digit and the start/stop characters rendered as text below the bars.
//! * **PZN7 / PZN8** — Pharmazentralnummer; data is prefixed with `-` and
//!   gets a special mod-10 (or mod-11 for PZN8) check digit, then encoded
//!   as Code 39.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// Encode a VIN.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 17-character vehicle identification number (no I/O/Q allowed).
/// let svg = render_svg(Symbology::Vin, "1HGCM82633A123456", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_vin(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.to_uppercase();
    if payload.chars().count() != 17 {
        return Err(Error::InvalidData(format!(
            "VIN must be exactly 17 characters (got {})",
            payload.chars().count()
        )));
    }
    for c in payload.chars() {
        if !c.is_ascii_alphanumeric() {
            return Err(Error::InvalidData(format!("VIN: invalid character {c:?}")));
        }
        if matches!(c, 'I' | 'O' | 'Q') {
            return Err(Error::InvalidData(format!(
                "VIN cannot contain {c:?} (ambiguous with 1, 0, and 0)"
            )));
        }
    }
    super::code39::encode(&payload, opts)
}

/// Encode LOGMARS. Same encoding as Code 39 with `includecheck=true` always.
pub fn encode_logmars(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData(
            "LOGMARS payload must not be empty".into(),
        ));
    }
    let mut overridden = opts.clone();
    // BWIPP's `logmars` derivative always includes the mod-43 check digit.
    // Force the option regardless of what the user passed.
    overridden.extras.retain(|(k, _)| k != "includecheck");
    overridden
        .extras
        .push(("includecheck".to_string(), "true".to_string()));
    super::code39::encode(&payload, &overridden)
}

/// Encode a 7-digit PZN (Pharmazentralnummer) as Code 39 with `-`
/// prefix. PZN7 uses weights 2..=7 over the 6 data digits — BWIPP's
/// `_L = 2`.
pub fn encode_pzn7(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_pzn(data, opts, 6, 2)
}

/// Encode an 8-digit PZN8. PZN8 uses weights 1..=7 over the 7 data
/// digits — BWIPP's `_L = 1`.
pub fn encode_pzn8(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_pzn(data, opts, 7, 1)
}

fn encode_pzn(
    data: &str,
    opts: &Options,
    body_len: usize,
    weight_offset: u32,
) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != body_len && digits.len() != body_len + 1 {
        return Err(Error::InvalidData(format!(
            "PZN: expected {body_len} or {} digits, got {}",
            body_len + 1,
            digits.len()
        )));
    }
    let body: String = digits.chars().take(body_len).collect();
    let check = pzn_check(&body, weight_offset);
    let supplied = digits.chars().nth(body_len);
    if let Some(c) = supplied {
        if c != check {
            return Err(Error::InvalidData(format!(
                "PZN: supplied check {c} does not match computed {check}"
            )));
        }
    }
    let payload = format!("-{body}{check}");
    super::code39::encode(&payload, opts)
}

/// PZN check digit: BWIPP's `sum(digit[i] * (i + weight_offset))`
/// mod 11. PZN7 uses `weight_offset = 2` (weights 2..=7); PZN8 uses
/// `weight_offset = 1` (weights 1..=7). If the result is 10 there's
/// no valid check digit per the spec — return `'?'` so the Code 39
/// layer rejects the payload.
fn pzn_check(body: &str, weight_offset: u32) -> char {
    let mut sum: u32 = 0;
    for (i, c) in body.chars().enumerate() {
        let weight = i as u32 + weight_offset;
        sum += c.to_digit(10).unwrap() * weight;
    }
    let check = sum % 11;
    if check == 10 {
        // BWIPP rejects this; we mark it visually so callers can detect it.
        // Returning '?' lets the Code 39 layer reject it cleanly.
        '?'
    } else {
        char::from_digit(check, 10).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vin_requires_17_chars() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 29-32
        // (`VIN must be exactly 17 characters (got 3)`). Cross-arm
        // guard against the forbidden-letter / invalid-char arms.
        match encode_vin("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("VIN must be exactly 17 characters"),
                    "missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("got 3"),
                    "missing `got 3` count echo (3 chars in `ABC`): {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — non-alphanumeric diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("cannot contain"),
                    "wrong arm — forbidden-letter diagnostic leaked: {msg}"
                );
            }
            other => panic!("3-char VIN should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn vin_rejects_forbidden_letters() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 38-42
        // (`VIN cannot contain 'I' (ambiguous with 1, 0, and 0)`).
        // The input ends with 'I' at position 16 — proves the
        // forbidden-letter check is reached. Cross-arm guard against
        // length and non-alphanumeric arms.
        match encode_vin("1HGCM82633A12345I", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("VIN cannot contain"),
                    "missing `VIN cannot contain` predicate: {msg}"
                );
                assert!(msg.contains("'I'"), "missing 'I' char Debug echo: {msg}");
                assert!(
                    msg.contains("ambiguous"),
                    "missing `ambiguous` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must be exactly 17"),
                    "wrong arm — length diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — non-alphanumeric diagnostic leaked: {msg}"
                );
            }
            other => panic!("VIN with 'I' should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin VIN's two rejection arms separately:
    ///   1. Non-alphanumeric character → "invalid character" diagnostic
    ///      (line 35-37). Existing `vin_rejects_forbidden_letters`
    ///      tests 'I' (a forbidden letter) but never a non-alphanumeric.
    ///   2. Forbidden letter I/O/Q → "cannot contain" + "ambiguous"
    ///      diagnostic (line 38-42). Existing test checks `is_err()`
    ///      only, doesn't pin which letter or message.
    ///   3. Length-mismatch diagnostic content ("VIN must be exactly
    ///      17 characters" + "got N").
    ///
    /// Mutations killed:
    ///   * `!c.is_ascii_alphanumeric()` predicate negation/removal.
    ///   * `matches!(c, 'I' | 'O' | 'Q')` arm-drop (each of I/O/Q).
    ///   * Error message string drift on any of the 3 arms.
    ///   * Length-check `!= 17` mutated to `< 17` or `> 17` (would
    ///     accept some bad lengths).
    #[test]
    fn vin_rejection_arms_pin_each_diagnostic() {
        // Arm 1: non-alphanumeric character '!' at position 16.
        match encode_vin("1HGCM82633A12345!", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("VIN: invalid character") && msg.contains("'!'"),
                "non-alphanumeric should hit 'invalid character' arm with offending char, got: {msg}"
            ),
            other => panic!("'!' in VIN should reject as invalid, got {other:?}"),
        }
        // Non-alphanumeric '$' anywhere — also rejects.
        //
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `Err(Error::InvalidData(_))` to 2-anchor pin matching the
        // sibling '!' arm at line 168:
        //   1. `VIN: invalid character` predicate
        //   2. `'$'` char Debug echo
        // A variant-only check passes any mutation that swaps the
        // format string at VIN's invalid-char arm to e.g. the
        // ambiguous-letter arm's wording.
        match encode_vin("$HGCM82633A123456", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("VIN: invalid character") && msg.contains("'$'"),
                "'$' should reject with invalid-character arm; got: {msg}"
            ),
            other => panic!("'$' should reject, got {other:?}"),
        }

        // Arm 2: each of I/O/Q rejects with the 'ambiguous' diagnostic.
        for forbidden in ['I', 'O', 'Q'] {
            // 16-char base + forbidden letter at the end.
            let payload: String = format!("1HGCM82633A12345{forbidden}");
            match encode_vin(&payload, &Options::default()) {
                Err(Error::InvalidData(msg)) => assert!(
                    msg.contains("VIN cannot contain")
                        && msg.contains(&format!("'{forbidden}'"))
                        && msg.contains("ambiguous"),
                    "VIN with {forbidden:?} should reject as ambiguous, got: {msg}"
                ),
                other => panic!("VIN with {forbidden:?} should reject, got {other:?}"),
            }
        }

        // Arm 3: length-mismatch diagnostic pins "exactly 17" + got count.
        match encode_vin("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("VIN must be exactly 17 characters") && msg.contains("got 3"),
                "length mismatch should pin '17 characters' + got count, got: {msg}"
            ),
            other => panic!("3-char VIN should reject with length error, got {other:?}"),
        }
        // 18-char input also rejects.
        let too_long = format!("{}A", "1HGCM82633A123456");
        match encode_vin(&too_long, &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("VIN must be exactly 17 characters") && msg.contains("got 18"),
                "18-char VIN should reject with 'got 18' count, got: {msg}"
            ),
            other => panic!("18-char VIN should reject, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin LOGMARS empty-payload rejection at line 50-54.
    /// Existing tests cover happy paths + check-digit forcing but not
    /// the empty-input arm.
    ///
    /// Mutations killed:
    ///   * `payload.is_empty()` predicate negation/removal.
    ///   * Error message string drift ("must not be empty").
    #[test]
    fn logmars_rejects_empty_payload() {
        match encode_logmars("", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("LOGMARS") && msg.contains("must not be empty"),
                "empty LOGMARS should pin 'LOGMARS' + 'must not be empty', got: {msg}"
            ),
            other => panic!("empty LOGMARS should reject, got {other:?}"),
        }
    }

    #[test]
    fn vin_accepts_canonical() {
        // Stage 11.A8c (cont) — descriptive label naming input.
        // "1HGCM82633A123456" is a 17-char VIN sample exercising the
        // canonical 17-char acceptance path.
        let p = encode_vin("1HGCM82633A123456", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_vin(\"1HGCM82633A123456\") (canonical 17-char VIN) must compose into non-empty Code 39 VIN symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn logmars_forces_check_digit() {
        // LOGMARS always adds the mod-43 check, so its symbol is wider than
        // Code 39 with the same payload and no check.
        let logmars = encode_logmars("HELLO", &Options::default()).unwrap();
        let plain = super::super::code39::encode("HELLO", &Options::default()).unwrap();
        assert!(logmars.total_width() > plain.total_width());
    }

    #[test]
    fn pzn7_round_trip() {
        // Use a known PZN7: 1234567 -> check should be deterministic
        // Stage 11.A8c (cont) — descriptive label naming input + path.
        let p = encode_pzn7("123456", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_pzn7(\"123456\") (6-digit PZN7 body → 7-char with computed check digit) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn pzn7_rejects_wrong_length() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 86-90
        // (`PZN: expected 6 or 7 digits, got 3`). Cross-helper guard
        // against the VIN diagnostics and the supplied-check arm.
        match encode_pzn7("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("PZN:"), "missing `PZN:` prefix: {msg}");
                assert!(
                    msg.contains("expected 6 or 7 digits"),
                    "missing length predicate (PZN7 expects 6 body or 6+1 supplied digits): {msg}"
                );
                assert!(msg.contains("got 3"), "missing `got 3` count echo: {msg}");
                assert!(
                    !msg.contains("VIN"),
                    "wrong helper — VIN diagnostic leaked into PZN7: {msg}"
                );
                assert!(
                    !msg.contains("supplied check"),
                    "wrong arm — supplied-check diagnostic leaked: {msg}"
                );
            }
            other => panic!("3-digit PZN7 should reject as InvalidData, got {other:?}"),
        }
    }

    /// VIN golden from `raw("code39", "1HGCM82633A123456", {})[0].sbs`.
    /// VIN doesn't have a dedicated BWIPP bcid — it's just Code 39
    /// over a 17-char VIN-validated payload — so this verifies that
    /// our wrapper produces the same bars as plain `code39` for the
    /// same string.
    #[test]
    fn vin_matches_bwip_js_code39() {
        let p = encode_vin("1HGCM82633A123456", &Options::default()).unwrap();
        let want: [u8; 190] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1,
            1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3,
            1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 3, 1, 1,
            1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3,
            1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3,
            3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "vin bars mismatch vs bwip-js raw output");
    }

    /// LOGMARS golden — Code 39 + appended mod-43 check character.
    /// `raw("code39", "LOGMARS123", {includecheck: true})[0].sbs`.
    #[test]
    fn logmars_matches_bwip_js_code39_with_check() {
        let p = encode_logmars("LOGMARS123", &Options::default()).unwrap();
        let want: [u8; 130] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1,
            1, 1, 1, 1, 1, 1, 3, 3, 1, 3, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1,
            3, 1, 3, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1,
            1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1,
            1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "logmars bars mismatch vs bwip-js raw output");
    }

    /// PZN7 golden from `raw("pzn", "123456", {})[0].sbs`. PZN7 is
    /// Code 39 with a `-` prefix and a mod-11 check digit (BWIPP
    /// computes both before delegating to code39, weights 2..=7).
    #[test]
    fn pzn7_matches_bwip_js_raw_sbs() {
        let p = encode_pzn7("123456", &Options::default()).unwrap();
        let want: [u8; 100] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3,
            1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1,
            3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1,
            1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "pzn7 bars mismatch vs bwip-js raw output");
    }

    /// PZN8 golden from `raw("pzn", "1234567", {pzn8: true})[0].sbs`.
    /// PZN8 is the same Code 39 wrapper as PZN7 but with one extra
    /// data digit and a different weight base (1..=8 instead of
    /// 2..=7 — see `pzn_check` for the math).
    #[test]
    fn pzn8_matches_bwip_js_raw_sbs() {
        let p = encode_pzn8("1234567", &Options::default()).unwrap();
        let want: [u8; 110] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3,
            1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1,
            3, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3,
            1, 3, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "pzn8 bars mismatch vs bwip-js raw output");
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests for code39_wrappers.
    // ---------------------------------------------------------------------

    /// Kills `encode_pzn: replace + with -` and `+ with *` at line ~85
    /// (the `body_len + 1` half of `if digits.len() != body_len &&
    /// digits.len() != body_len + 1`). Mutants reject the
    /// "with-supplied-check" path (body_len + 1 digits); the
    /// existing tests only exercised `body_len`-digit input
    /// (computed-check path).
    #[test]
    fn pzn_accepts_full_length_with_supplied_check_digit() {
        // PZN7: body_len=6, accepts 6 *or* 7 digits.
        // Compute the correct check digit and supply it explicitly.
        let body6 = "123456";
        let check = pzn_check(body6, 2);
        let with_check = format!("{body6}{check}");
        assert_eq!(
            with_check.len(),
            7,
            "PZN7 with-check input should be 7 digits"
        );
        encode_pzn7(&with_check, &Options::default())
            .expect("PZN7 should accept body_len + 1 digits when the supplied check matches");
        // PZN8: body_len=7, accepts 7 *or* 8 digits.
        let body7 = "1234567";
        let check = pzn_check(body7, 1);
        let with_check = format!("{body7}{check}");
        assert_eq!(
            with_check.len(),
            8,
            "PZN8 with-check input should be 8 digits"
        );
        encode_pzn8(&with_check, &Options::default())
            .expect("PZN8 should accept body_len + 1 digits when the supplied check matches");
    }

    /// Stage 11.A8c — pin the `check == 10 -> '?'` branch in
    /// `pzn_check` (line ~118). Inputs where `sum % 11 == 10` have no
    /// valid PZN check digit per spec; BWIPP emits `'?'` and lets the
    /// Code 39 layer reject the symbol. The existing tests never hit
    /// this branch, so the `if check == 10 { '?' } else { ... }`
    /// arm-swap mutant survives.
    ///
    /// Hand-computed: PZN7("000003") has weighted sum
    /// 0*2 + 0*3 + 0*4 + 0*5 + 0*6 + 3*7 = 21, 21 % 11 = 10 → '?'.
    /// PZN8("0000003") with weights 1..=7: 0+0+0+0+0+0+3*7=21 → '?'.
    ///
    /// We verify `pzn_check` directly returns '?' AND that
    /// `encode_pzn7` / `encode_pzn8` reject the resulting payload
    /// (because Code 39 can't encode '?').
    #[test]
    fn pzn_check_returns_question_mark_when_sum_mod_eleven_is_ten() {
        // Direct helper assertion.
        assert_eq!(
            pzn_check("000003", 2),
            '?',
            "PZN7 sum=21, 21%11=10 should map to '?'"
        );
        assert_eq!(
            pzn_check("0000003", 1),
            '?',
            "PZN8 sum=21, 21%11=10 should map to '?'"
        );
        // Encoder rejects the symbol because Code 39 can't carry '?'.
        assert!(
            matches!(
                encode_pzn7("000003", &Options::default()),
                Err(Error::InvalidData(_))
            ),
            "PZN7('000003') must error — pzn_check returns '?' \
             and Code 39 can't encode the question mark"
        );
        assert!(
            matches!(
                encode_pzn8("0000003", &Options::default()),
                Err(Error::InvalidData(_))
            ),
            "PZN8('0000003') must error — pzn_check returns '?' \
             and Code 39 can't encode the question mark"
        );
    }

    /// Kills `encode_pzn: replace != with ==` at line ~96 (the
    /// supplied-check-digit equality check). Original rejects a
    /// mismatched supplied check; the mutant inverts the comparison
    /// and rejects *matching* check digits instead.
    #[test]
    fn pzn_rejects_supplied_check_digit_mismatch() {
        // PZN7: body "123456", correct check would be... let pzn_check
        // compute it, then supply the *opposite* digit so the validator
        // *must* trigger.
        let body6 = "123456";
        let correct = pzn_check(body6, 2);
        let wrong = if correct == '0' { '1' } else { '0' };
        let bad = format!("{body6}{wrong}");
        assert!(
            matches!(
                encode_pzn7(&bad, &Options::default()),
                Err(Error::InvalidData(_))
            ),
            "PZN7 should reject a mismatched supplied check digit ({wrong} vs computed {correct})"
        );
    }

    /// Stage 11.A8c — pin `pzn_check` valid (non-`?`) arithmetic for
    /// both `weight_offset` values, exercising both ends of the body.
    /// Existing tests cover only the `?` arm and the round-trip via
    /// `encode_pzn7`/`encode_pzn8` — direct value assertions for the
    /// computed digit at each position were missing. Mutations to
    /// catch:
    ///   - `i as u32 + weight_offset` → `* weight_offset`: would
    ///     produce wildly different sums.
    ///   - `* weight` → `+ weight`: weighted sum becomes positional sum.
    ///   - `% 11` → `% 10` or `% 12`: wrong modulus.
    ///   - `weight_offset` arg ignored: PZN7 and PZN8 collapse to same
    ///     output for distinguishing inputs.
    ///
    /// Hand-computed (PZN7 weights 2..=7 for 6-digit body; PZN8
    /// weights 1..=7 for 7-digit body; sum mod 11 → digit):
    ///   - PZN7 "100000": 1*2 = 2 → '2'.
    ///   - PZN7 "000001": 1*7 = 7 → '7'.
    ///   - PZN7 "000010": 1*6 = 6 → '6'.
    ///   - PZN8 "1000000": 1*1 = 1 → '1'.
    ///   - PZN8 "0000001": 1*7 = 7 → '7'.
    ///   - Distinguishing case: PZN7 "100000" = '2' but PZN8 with the
    ///     same prefix shifted differently. Use "0000100" for PZN8:
    ///     1*5 = 5 → '5'.
    #[test]
    fn pzn_check_per_position_weights() {
        // PZN7: weight_offset=2, weights 2..=7 for 6-digit body.
        assert_eq!(
            pzn_check("100000", 2),
            '2',
            "PZN7 pos 0: weight=2; 1*2=2 → '2'"
        );
        assert_eq!(
            pzn_check("000001", 2),
            '7',
            "PZN7 pos 5: weight=7; 1*7=7 → '7'"
        );
        assert_eq!(
            pzn_check("000010", 2),
            '6',
            "PZN7 pos 4: weight=6; 1*6=6 → '6'"
        );
        // PZN8: weight_offset=1, weights 1..=7 for 7-digit body.
        assert_eq!(
            pzn_check("1000000", 1),
            '1',
            "PZN8 pos 0: weight=1; 1*1=1 → '1'"
        );
        assert_eq!(
            pzn_check("0000001", 1),
            '7',
            "PZN8 pos 6: weight=7; 1*7=7 → '7'"
        );
        // Distinguishing PZN7 vs PZN8: same input "100000" (6 digits)
        // produces '2' under offset=2 but '1' under offset=1. If
        // `weight_offset` were ignored, the two would collide.
        assert_ne!(
            pzn_check("100000", 2),
            pzn_check("100000", 1),
            "weight_offset must actually affect the sum (2 vs 1 → '2' vs '1')"
        );
        // Empty body: sum=0, 0%11=0 → '0' for both offsets.
        assert_eq!(pzn_check("", 2), '0');
        assert_eq!(pzn_check("", 1), '0');
    }

    /// Stage 11.A8c — pin `encode_pzn`'s non-digit filter.
    ///
    /// The function strips every non-ASCII-digit char before length /
    /// check validation. Existing tests (`pzn7_round_trip`,
    /// `pzn7_matches_bwip_js_raw_sbs`, `pzn8_matches_bwip_js_raw_sbs`)
    /// all pass clean digit-only payloads, so they would silently pass
    /// a mutant that *dropped* the filter (`data.chars().collect()`).
    /// This test feeds a payload with embedded dashes / spaces / a
    /// "PZN-" prefix and verifies it produces the same encoded output
    /// as the cleaned digit-only equivalent.
    #[test]
    fn encode_pzn_filters_non_digits_before_length_validation() {
        // PZN7 body length = 6. Both "123456" and "12-34-56" must
        // produce the same encoded LinearPattern (the filter strips the
        // dashes, leaving 6 digits = body_len). A mutant that dropped
        // the filter would see "12-34-56" as 8 chars → length check
        // would error.
        let clean = encode_pzn7("123456", &Options::default()).unwrap();
        let dashed = encode_pzn7("12-34-56", &Options::default()).unwrap();
        assert_eq!(
            clean.bars, dashed.bars,
            "encode_pzn7 must filter dashes → same bars as clean digits"
        );

        // "PZN-123456" with the "PZN-" prefix: filter strips letters
        // and dash, leaving "123456" → accepted.
        let prefixed = encode_pzn7("PZN-123456", &Options::default()).unwrap();
        assert_eq!(
            clean.bars, prefixed.bars,
            "encode_pzn7 must filter letter+dash prefix"
        );

        // Whitespace inside the digit stream: also filtered.
        let spaced = encode_pzn7(" 1 2 3 4 5 6 ", &Options::default()).unwrap();
        assert_eq!(
            clean.bars, spaced.bars,
            "encode_pzn7 must filter interior whitespace"
        );

        // Mutant discriminator: an all-non-digit payload (length 0 after
        // filter) must error. A mutant that dropped the filter would
        // see "abc-def" as 7 chars → would match body_len+1 (=7) and
        // try to validate "abc" against the check digit → would error
        // anyway via to_digit panic. So this is more documentation
        // than a discriminator, but pin the empty-after-filter error
        // path explicitly.
        let err = encode_pzn7("abc-def", &Options::default()).unwrap_err();
        assert!(
            matches!(err, Error::InvalidData(_)),
            "all-non-digits → 0 digits → InvalidData; got {err:?}"
        );

        // PZN8 (body_len=7) sanity: same filter, different length.
        let clean8 = encode_pzn8("1234567", &Options::default()).unwrap();
        let dashed8 = encode_pzn8("123-4567", &Options::default()).unwrap();
        assert_eq!(
            clean8.bars, dashed8.bars,
            "encode_pzn8 also filters non-digits"
        );
    }
}
