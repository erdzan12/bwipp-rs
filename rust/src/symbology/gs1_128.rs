//! GS1-128 (and its UCC-128 alias, plus SSCC-18 / NVE-18 and UPC Coupon
//! wrappers).
//!
//! GS1-128 is Code 128 carrying GS1 Application Identifier data. The symbol
//! layout is:
//!
//!   Start (A/B/C) | FNC1 | encoded AI elements with internal FNC1
//!   separators after variable-length AIs | mod-103 check | Stop
//!
//! We delegate the bar-level encoding to [`super::code128::encode_tokens`]
//! and only handle the GS1 layer here: parse the user's parenthesised
//! element string, emit the appropriate token sequence.
//!
//! Reference: BWIPP `gs1-128.ps.src` and `gs1northamericancoupon.ps.src`.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

use super::code128::{encode_tokens, Token};
use crate::util::gs1;

/// Encode a GS1-128 payload. Input is a GS1 element string of the form
/// `(AI)data(AI)data...`.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // GTIN (AI 01) + serial number (AI 21).
/// let svg = render_svg(Symbology::Gs1_128, "(01)09501101530003(21)ABC123", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    encode_with_linkage(data, opts, Linkage::None)
}

/// Composite-linkage marker appended to the GS1-128 codeword stream
/// to signal that the symbol carries a 2-D companion. Used by the
/// `composite_gs1_128_*` symbologies — `LinkageA` for CC-A / CC-B
/// (which both render via MicroPDF417), `LinkageC` for CC-C (PDF417).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Linkage {
    /// Standalone GS1-128 — no linkage codeword.
    None,
    /// CC-A or CC-B companion (BWIPP `^LNKA`).
    A,
    /// CC-C companion (BWIPP `^LNKC`).
    C,
}

/// Encode a GS1-128 payload, optionally with a trailing linkage codeword
/// for composite barcodes. See [`Linkage`] for the link types.
///
/// `Linkage::None` produces the same output as [`encode`]; the other
/// variants append a single subset-switch codeword right before the
/// check character, per BWIPP `code128_lka` / `code128_lkc`.
pub(crate) fn encode_with_linkage(
    data: &str,
    opts: &Options,
    linkage: Linkage,
) -> Result<LinearPattern, Error> {
    let elements = gs1::parse(data).map_err(|e| Error::InvalidData(e.to_string()))?;
    let mut tokens: Vec<Token> = Vec::new();
    tokens.push(Token::Fnc1); // Mandatory leading FNC1 for GS1-128.

    let bytes = gs1::encode_with_fnc1(&elements);
    // `bytes` starts with a leading FNC1 byte (which we just emitted as a
    // token) followed by the AI / data sequence with inter-element FNC1
    // bytes. Translate each byte to a Token, mapping FNC1 (0x1D) to
    // Token::Fnc1.
    for &b in bytes.iter().skip(1) {
        if b == gs1::FNC1 {
            tokens.push(Token::Fnc1);
        } else {
            tokens.push(Token::Ascii(b));
        }
    }

    match linkage {
        Linkage::None => {}
        Linkage::A => tokens.push(Token::LinkA),
        Linkage::C => tokens.push(Token::LinkC),
    }

    encode_tokens(&tokens, opts).map(|mut p| {
        // Display the user's original element string under the bars when
        // include_text is set.
        if opts.include_text {
            p.text = Some(data.to_string());
        }
        p
    })
}

/// Encode an SSCC-18 payload (18-digit Serial Shipping Container Code).
///
/// Input is the 18-digit SSCC; we wrap it in AI `(00)` and delegate.
pub fn encode_sscc18(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 18 {
        return Err(Error::InvalidData(format!(
            "SSCC-18: expected 18 digits, got {}",
            digits.len()
        )));
    }
    encode(&format!("(00){digits}"), opts)
}

/// Encode a UPC Coupon (GS1 North American Coupon, AI 8110). Input is the
/// raw coupon data string (variable length per the AI 8110 spec); we wrap
/// it in `(8110)` and delegate.
pub fn encode_coupon(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let body = data.trim();
    if body.is_empty() {
        return Err(Error::InvalidData("UPC Coupon: payload is empty".into()));
    }
    encode(&format!("(8110){body}"), opts)
}

/// Encode EAN-14 / GTIN-14 (BWIPP `bwipp_ean14`). Input is either 13
/// digits (we compute the check digit) or 14 digits (we verify it).
/// Optional `(01)` AI prefix is accepted but stripped before length
/// checks. We then delegate to GS1-128 with input `(01){14-digit gtin}`,
/// which produces the canonical FNC1 + AI 01 + GTIN encoding BWIPP's
/// `bwipp_ean14` emits as `^FNC101{gtin}` to Code 128.
pub fn encode_ean14(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    // Strip an optional `(01)` AI prefix; the BWIPP encoder requires it,
    // but most callers pass just the digits. We deliberately do NOT
    // strip a bare leading `01` — many real GTINs (e.g.
    // `01234567890128`) start with those digits and a heuristic strip
    // would silently corrupt them.
    let body = data.strip_prefix("(01)").unwrap_or(data);
    let digits: String = body.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 13 && digits.len() != 14 {
        return Err(Error::InvalidData(format!(
            "EAN-14: expected 13 or 14 digits, got {}",
            digits.len()
        )));
    }

    // Mod-10 check digit over the first 13 digits, alternating 3/1
    // weights starting from the leftmost position (BWIPP `bwipp_ean14`
    // lines 9883-9888: `(checksum % 2 == 0) ? digit * 3 : digit`).
    let mut sum: u32 = 0;
    for (i, ch) in digits.chars().take(13).enumerate() {
        let d = ch.to_digit(10).unwrap();
        sum += if i % 2 == 0 { d * 3 } else { d };
    }
    let check = (10 - (sum % 10)) % 10;

    let gtin = if digits.len() == 13 {
        format!("{digits}{check}")
    } else {
        let given_check = digits.chars().nth(13).unwrap().to_digit(10).unwrap();
        if given_check != check {
            return Err(Error::InvalidData(format!(
                "EAN-14: incorrect check digit (expected {check}, got {given_check})"
            )));
        }
        digits
    };

    encode(&format!("(01){gtin}"), opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_simple_gtin() {
        // Stage 11.A8c (cont) — descriptive label naming canonical GTIN
        // path (AI 01 + 16-digit GTIN-14 incl. check, GS1-128 over Code 128
        // subset C).
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the canonical GS1-128 AI=01 GTIN-14 smoke path.
        let p = encode("(01)04012345123456", &Options::default()).expect(
            "encode(\"(01)04012345123456\", default) (canonical AI=01 GTIN-14 → GS1-128 over Code 128 subset C) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode(\"(01)04012345123456\") (canonical AI=01 GTIN-14 over GS1-128 Code 128 subset C) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn rejects_invalid_ai_format() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin.
        // Delegates to gs1::parse (line 64 of gs1_128.rs); the
        // ParseError::MissingOpenParen Display impl at lines 61-63 of
        // util/gs1.rs renders:
        //   `GS1 parse: expected '(' at position 0`
        // Anchors:
        //   1. `GS1 parse:` prefix (discriminates from SSCC-18 /
        //      EAN-14 / UPC Coupon top-level arms that short-circuit
        //      before gs1::parse)
        //   2. `expected '('` predicate
        //   3. `at position 0` — pins the position counter (input
        //      starts with '0' so the missing-paren is at index 0)
        // Missing parenthesis.
        match encode("01)04012345123456", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 parse:"),
                    "missing `GS1 parse:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected '('"),
                    "missing `expected '('` predicate: {msg}"
                );
                assert!(
                    msg.contains("at position 0"),
                    "missing `at position 0` value echo: {msg}"
                );
            }
            other => panic!(
                "missing-paren input should reject as InvalidData with GS1 parse diagnostic, got {other:?}"
            ),
        }
    }

    #[test]
    fn rejects_bad_ai_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin.
        // Delegates to gs1::parse; the ParseError::BadLength Display
        // impl at lines 73-76 of util/gs1.rs renders:
        //   `GS1 parse: AI (01) requires data length 14, got 13`
        // Anchors:
        //   1. `GS1 parse:` prefix
        //   2. `AI (01)` AI-token echo
        //   3. `data length 14, got 13` length predicate + value echo
        //      — pins both the expected-length spec (AI 01 → 14
        //      digits) and the supplied-length echo (13 digits).
        // AI (01) requires 14 digits; supply 13.
        match encode("(01)0401234512345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 parse:"),
                    "missing `GS1 parse:` prefix: {msg}"
                );
                assert!(
                    msg.contains("AI (01)"),
                    "missing `AI (01)` AI-token echo: {msg}"
                );
                assert!(
                    msg.contains("data length 14, got 13"),
                    "missing `data length 14, got 13` predicate + value echo: {msg}"
                );
            }
            other => panic!(
                "13-digit AI 01 should reject as InvalidData with GS1 parse BadLength, got {other:?}"
            ),
        }
    }

    #[test]
    fn sscc18_wraps_with_ai_00() {
        // Stage 11.A8c (cont) — descriptive label naming SSCC-18 wrapper
        // path (auto-prepends AI 00 to the 18-digit serial).
        let p = encode_sscc18("106141411234567897", &Options::default()).expect(
            "encode_sscc18(\"106141411234567897\", default) (SSCC-18 18-digit serial → GS1-128 via AI=00 auto-wrap path) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_sscc18(\"106141411234567897\") (18-digit SSCC → GS1-128 via AI=00 auto-wrap) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn sscc18_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 103-106 of gs1_128.rs:
        //   1. `SSCC-18:` symbology prefix
        //   2. `expected 18 digits` predicate (pins the specific
        //      18-digit requirement — a mutation that changes 18 to
        //      another constant fails)
        //   3. `got 5` value echo for the supplied 5-digit input
        match encode_sscc18("12345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("SSCC-18:"), "missing `SSCC-18:` prefix: {msg}");
                assert!(
                    msg.contains("expected 18 digits"),
                    "missing `expected 18 digits` predicate: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` value echo: {msg}");
            }
            other => panic!("\"12345\" SSCC-18 input should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn coupon_wraps_with_ai_8110() {
        // Stage 11.A8c (cont) — descriptive label naming coupon wrapper
        // path (auto-prepends AI 8110 to the coupon payload).
        let p = encode_coupon("106141416543213500110000310123196000", &Options::default()).expect(
            "encode_coupon(36-char coupon payload, default) (UPC Coupon → GS1-128 via AI=8110 auto-wrap path) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_coupon(\"...\") (36-char coupon payload → GS1-128 via AI=8110 auto-wrap) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn coupon_rejects_empty() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 117 of gs1_128.rs:
        //   1. `UPC Coupon:` symbology prefix (distinguishes from the
        //      SSCC-18 / EAN-14 / GS1 parse arms)
        //   2. `payload is empty` predicate
        // Empty-payload diagnostic carries no value to echo; cross-
        // arm contamination guards strengthen the signal.
        match encode_coupon("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("UPC Coupon:"),
                    "missing `UPC Coupon:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "missing `payload is empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("SSCC-18"),
                    "wrong arm — SSCC-18 diagnostic leaked into UPC Coupon empty reject: {msg}"
                );
                assert!(
                    !msg.contains("EAN-14"),
                    "wrong arm — EAN-14 diagnostic leaked into UPC Coupon empty reject: {msg}"
                );
                assert!(
                    !msg.contains("GS1 parse"),
                    "wrong arm — GS1 parse leaked into UPC Coupon empty reject (must short-circuit before delegating): {msg}"
                );
            }
            other => panic!("empty UPC Coupon payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ean14_with_13_digit_input_matches_bwip_js_raw_sbs() {
        // bwip-js: `raw("ean14", "(01)0401234512345", {})[0].sbs`. The
        // encoder computes check digit 6 over "040123451234" → final
        // GTIN "04012345123456", then emits GS1-128 of "(01)<gtin>".
        // Pinning the full 73-element sbs proves bwipp-rs's `ean14`
        // path produces byte-identical bars to BWIPP for this input.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-14 13-digit auto-check + 73-bar oracle path: encoder
        // computes check 6 over "040123451234" → "04012345123456",
        // then emits GS1-128 of "(01)<gtin>".
        let p = encode_ean14("(01)0401234512345", &Options::default()).expect(
            "encode_ean14(\"(01)0401234512345\", default) (EAN-14 13-digit auto-check + 73-bar bwip-js raw SBS oracle) must succeed",
        );
        let want: [u8; 73] = [
            2, 1, 1, 2, 3, 2, 4, 1, 1, 1, 3, 1, 2, 2, 2, 1, 2, 2, 1, 2, 1, 3, 2, 2, 2, 2, 2, 1, 2,
            2, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 2, 2, 3, 2, 1, 3, 1, 1, 2, 3, 3, 3, 1, 1,
            2, 1, 3, 2, 2, 1, 1, 2, 2, 3, 3, 1, 1, 1, 2,
        ];
        assert_eq!(p.bars.as_slice(), want.as_slice());
    }

    #[test]
    fn ean14_accepts_14_digit_input_with_correct_check() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-14 13/14-digit equivalence path: 13-digit form auto-
        // computes check, 14-digit form passes through; bars must match.
        let p13 = encode_ean14("0401234512345", &Options::default()).expect(
            "encode_ean14(\"0401234512345\", default) (EAN-14 13-digit auto-check baseline; check=6 computed) must succeed",
        );
        let p14 = encode_ean14("04012345123456", &Options::default()).expect(
            "encode_ean14(\"04012345123456\", default) (EAN-14 14-digit pass-through; check=6 already present) must succeed",
        );
        assert_eq!(
            p13.bars, p14.bars,
            "13/14-digit forms must produce same bars"
        );
    }

    #[test]
    fn ean14_rejects_wrong_check_digit() {
        // Correct check is 6; supplying 5 must error.
        // Stage 11.A8c — upgrade single-anchor `msg.contains("check
        // digit")` to a 4-anchor pin matching the source diagnostic
        // at line 158-159 (`EAN-14: incorrect check digit (expected
        // 6, got 5)`). Cross-arm guard against the wrong-length arm.
        match encode_ean14("04012345123455", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN-14:"),
                    "missing `EAN-14:` family prefix: {msg}"
                );
                assert!(
                    msg.contains("incorrect check digit"),
                    "missing `incorrect check digit` predicate: {msg}"
                );
                assert!(
                    msg.contains("expected 6"),
                    "missing `expected 6` computed-check echo: {msg}"
                );
                assert!(
                    msg.contains("got 5"),
                    "missing `got 5` supplied-check echo: {msg}"
                );
                assert!(
                    !msg.contains("expected 13 or 14 digits"),
                    "wrong arm — length-mismatch diagnostic leaked: {msg}"
                );
            }
            other => {
                panic!("`04012345123455` (bad check 5) should reject as InvalidData, got {other:?}")
            }
        }
    }

    #[test]
    fn ean14_rejects_short_input() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 137-140 of gs1_128.rs:
        //   1. `EAN-14:` symbology prefix
        //   2. `expected 13 or 14 digits` predicate (pins the dual
        //      acceptable-length spec — a mutation that drops the
        //      "13 or" or the "14" half fails)
        //   3. `got 5` value echo for the supplied 5-digit input
        match encode_ean14("12345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("EAN-14:"), "missing `EAN-14:` prefix: {msg}");
                assert!(
                    msg.contains("expected 13 or 14 digits"),
                    "missing `expected 13 or 14 digits` predicate: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` value echo: {msg}");
            }
            other => panic!("\"12345\" EAN-14 input should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the outer `% 10` fold in `encode_ean14`'s
    /// check-digit arithmetic at line ~151 (`(10 - (sum % 10)) % 10`).
    /// The existing goldens (sum=64, check=6) and the
    /// `ean14_rejects_wrong_check_digit` test all use inputs where
    /// `sum % 10 != 0`, so the outer `% 10` (which folds 10 → 0 when
    /// `sum % 10 == 0`) is never exercised. A mutant that drops the
    /// outer fold (e.g. `(10 - (sum % 10))`) would compute check=10
    /// instead of check=0; `format!("{check}")` renders "10" (two
    /// chars), making the gtin 15 digits long and tripping the
    /// downstream AI 01 length guard in `gs1::parse`.
    ///
    /// Anchors (hand-computed, BWIPP weights `3,1,3,1,...,3` over
    /// 13 digits):
    ///   * "0000000000000" → all zeros, sum=0 → outer fold gives 0 →
    ///     gtin "00000000000000" (14 zeros, valid). Under the mutant
    ///     check=10 → gtin "000000000000010" (15 chars) → reject.
    ///   * "5000000000005" → 5*3 + 5*3 = 30, sum%10=0 → outer fold
    ///     gives 0 → gtin "50000000000050" (14 chars, valid). Same
    ///     mutant rejects with 15-char gtin.
    ///
    /// Mutations to catch (in addition to outer-fold removal):
    ///   * `(10 - (sum % 10))` → `(10 + (sum % 10))`: at sum%10=0
    ///     gives 10 (mutant rejects); at sum%10=5 gives 15 (mutant
    ///     produces invalid char). Both anchors catch.
    ///   * `i % 2 == 0` → `i % 2 != 0`: swaps weights; the "5...5"
    ///     anchor would change sum from 30 (which yields check 0) to
    ///     5+5 = 10 (which also yields check 0). The "00000…000"
    ///     anchor is unaffected (all zeros). So we also pin a
    ///     **non-trivial wrap** that distinguishes weight-swap:
    ///     "9999999999991" with weights `3,1,3,1,…,3`:
    ///       * even positions (0,2,4,6,8,10,12) digits are 9,9,9,9,9,9,1
    ///         → 6×(9×3) + 1×3 = 162 + 3 = 165
    ///       * odd positions (1,3,5,7,9,11) digits are all 9
    ///         → 6×(9×1) = 54
    ///       * sum = 219; 219%10 = 9; check = (10-9)%10 = 1.
    ///     The weight-swap mutant (weights `1,3,1,3,…,1`) computes
    ///       * even positions: 6×(9×1) + 1×1 = 55
    ///       * odd positions: 6×(9×3) = 162
    ///       * sum = 217; 217%10 = 7; check = (10-7)%10 = 3.
    ///     So the original accepts trailing '1', the mutant accepts
    ///     trailing '3' — both directions tested below.
    #[test]
    fn ean14_check_digit_outer_modulo_fold_at_wraparound() {
        // Wraparound anchor 1: all zeros.
        let p_zeros = encode_ean14("0000000000000", &Options::default())
            .expect("13 zeros should encode (check = 0 via outer % 10 fold)");
        assert!(
            p_zeros.total_width() > 0,
            "encoded symbol must have non-zero width"
        );
        // Cross-check: 14-digit form with the explicit check '0' must
        // produce identical bars (pins both the computed and supplied
        // check paths).
        let p_14_zeros = encode_ean14("00000000000000", &Options::default())
            .expect("14 zeros with explicit check '0' must accept");
        assert_eq!(
            p_zeros.bars, p_14_zeros.bars,
            "13/14-digit forms must agree when check folds to 0"
        );

        // Wraparound anchor 2: non-trivial sum that wraps.
        // sum = 5*3 + 5*3 = 30; 30%10=0; check = (10-0)%10 = 0.
        let p_55 = encode_ean14("5000000000005", &Options::default())
            .expect("13 digits 5000000000005 should encode (sum=30, check=0 via wrap)");
        let p_14_55 = encode_ean14("50000000000050", &Options::default())
            .expect("14-digit form with check '0' must agree");
        assert_eq!(
            p_55.bars, p_14_55.bars,
            "13/14-digit forms must agree at the second wrap anchor"
        );

        // Discriminator anchor: weight-swap (i % 2 == 0 → !=) would
        // change the check digit. Verify the 14-digit form's check
        // digit holds against a deliberate wrong-digit reject.
        let p_9 = encode_ean14("9999999999991", &Options::default())
            .expect("13 9s + trailing 1 → check 1 (sum=219, 219%10=9, (10-9)%10=1)");
        // 14-digit form with explicit check '1' must agree.
        let p_14_9 = encode_ean14("99999999999911", &Options::default())
            .expect("14-digit form with check '1' must accept");
        assert_eq!(
            p_9.bars, p_14_9.bars,
            "13/14-digit forms must agree at the discriminator anchor"
        );
        // The weight-swap mutant would produce check=3 instead of 1 →
        // explicit check '3' would be silently accepted under the
        // mutant and rejected by the original. Pin the rejection.
        assert!(
            matches!(
                encode_ean14("99999999999913", &Options::default()),
                Err(Error::InvalidData(_))
            ),
            "the weight-swap mutant's check value '3' must be rejected by the original"
        );
    }

    #[test]
    fn ean14_accepts_parenthesized_and_unprefixed_forms() {
        // `(01)`-prefixed and bare-digits forms should both resolve to
        // the same encoded GS1-128.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the EAN-14 paren-vs-bare equivalence path.
        let p = encode_ean14("(01)04012345123456", &Options::default()).expect(
            "encode_ean14(\"(01)04012345123456\", default) (EAN-14 paren-prefixed 14-digit form; must equal bare form) must succeed",
        );
        let q = encode_ean14("04012345123456", &Options::default()).expect(
            "encode_ean14(\"04012345123456\", default) (EAN-14 bare-digit 14-form; must equal paren form) must succeed",
        );
        assert_eq!(p.bars, q.bars);
    }

    #[test]
    fn encode_with_linkage_a_matches_bwip_js() {
        // bwip-js oracle (oracle-gs1-128-link.js for "(01)04012345123456" with
        // linkagea=true) emits this 79-element sbs:
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the GS1-128 Linkage::A 79-element SBS oracle path.
        let p = encode_with_linkage("(01)04012345123456", &Options::default(), Linkage::A).expect(
            "encode_with_linkage(\"(01)04012345123456\", Linkage::A) (GS1-128 Linkage::A 79-element SBS bwip-js oracle; pushes Token::LinkA after data) must succeed",
        );
        let want_sbs: [u8; 79] = [
            2, 1, 1, 2, 3, 2, 4, 1, 1, 1, 3, 1, 2, 2, 2, 1, 2, 2, 1, 2, 1, 3, 2, 2, 2, 2, 2, 1, 2,
            2, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 2, 2, 3, 2, 1, 3, 1, 1, 2, 3, 3, 3, 1, 1,
            2, 1, 3, 1, 1, 1, 4, 1, 1, 3, 2, 2, 1, 2, 2, 3, 3, 1, 1, 1, 2,
        ];
        assert_eq!(p.bars.len(), want_sbs.len());
        for (i, (got, want)) in p.bars.iter().zip(want_sbs.iter()).enumerate() {
            assert_eq!(*got, *want, "sbs[{i}] mismatch");
        }
    }

    #[test]
    fn encode_with_linkage_none_matches_plain_encode() {
        // Linkage::None should be identical to plain encode().
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the GS1-128 Linkage::None equivalence-to-plain-encode path:
        // both must produce identical bars.
        let plain = encode("(01)04012345123456", &Options::default()).expect(
            "encode(\"(01)04012345123456\", default) (GS1-128 plain-encode baseline for Linkage::None equivalence) must succeed",
        );
        let linked = encode_with_linkage("(01)04012345123456", &Options::default(), Linkage::None)
            .expect(
                "encode_with_linkage(\"(01)04012345123456\", Linkage::None) (GS1-128 Linkage::None must be identical to plain encode; no Token::LinkA/LinkC pushed) must succeed",
            );
        assert_eq!(plain.bars, linked.bars);
    }

    /// Stage 11.A8c — pin `encode_with_linkage(Linkage::C)` directly.
    /// The existing `encode_with_linkage_a_matches_bwip_js` pins the
    /// `Linkage::A` arm and `encode_with_linkage_none_matches_plain_encode`
    /// pins the `Linkage::None` arm — but `Linkage::C` is only
    /// exercised transitively through `composite::encode_gs1_128_ccc`.
    ///
    /// The 3-arm match at line 81-85 maps:
    ///   Linkage::None → no push
    ///   Linkage::A → Token::LinkA
    ///   Linkage::C → Token::LinkC
    ///
    /// Mutations to catch:
    ///   * `Linkage::C => tokens.push(Token::LinkC)` → `push(Token::
    ///     LinkA)`: C arm would silently produce the A output. The
    ///     three-way distinctness assertion below catches this.
    ///   * `Linkage::C => {}`: C-arm becomes a no-op → C output
    ///     would equal None output.
    ///   * Match-arm omission entirely: C silently routes to default
    ///     → equals None.
    ///
    /// All three linkage variants must produce DISTINCT bar outputs
    /// (different LinkA / LinkC / None Token sequences).
    #[test]
    fn encode_with_linkage_c_is_distinct_from_a_and_none() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the GS1-128 3-arm linkage distinctness path. The 3-arm match
        // at line 81-85 of gs1_128.rs maps None→no-push / A→LinkA /
        // C→LinkC. All three must produce DISTINCT bar outputs.
        let none = encode_with_linkage("(01)04012345123456", &Options::default(), Linkage::None)
            .expect(
                "encode_with_linkage(..., Linkage::None) (GS1-128 3-arm distinctness baseline: no Token::Link pushed) must succeed",
            );
        let link_a = encode_with_linkage("(01)04012345123456", &Options::default(), Linkage::A)
            .expect(
                "encode_with_linkage(..., Linkage::A) (GS1-128 3-arm distinctness: pushes Token::LinkA; must differ from None and C) must succeed",
            );
        let link_c = encode_with_linkage("(01)04012345123456", &Options::default(), Linkage::C)
            .expect(
                "encode_with_linkage(..., Linkage::C) (GS1-128 3-arm distinctness: pushes Token::LinkC; transitively via composite::encode_gs1_128_ccc; must differ from None and A) must succeed",
            );

        // Linkage::C output must be longer than None (one extra
        // linkage codeword = 6 modules of bar/space widths).
        assert!(
            link_c.bars.len() > none.bars.len(),
            "Linkage::C must add a linkage codeword vs None ({} <= {})",
            link_c.bars.len(),
            none.bars.len()
        );

        // Linkage::C must produce DIFFERENT bars from Linkage::A
        // (LinkA = code128 codeword 100, LinkC = codeword 99 — they
        // shift to a different subset and have different bar patterns).
        assert_ne!(
            link_c.bars, link_a.bars,
            "Linkage::A and Linkage::C must emit different codewords"
        );

        // Linkage::C must produce DIFFERENT bars from Linkage::None
        // (the extra codeword changes the symbol length).
        assert_ne!(
            link_c.bars, none.bars,
            "Linkage::C must NOT equal Linkage::None — the linkage \
             codeword must actually be appended"
        );

        // Sanity: Linkage::A and Linkage::C have the same total
        // codeword count (start + AI/data + linkage + check + stop),
        // just with different linkage codeword identities. So their
        // bar lengths must match.
        assert_eq!(
            link_c.bars.len(),
            link_a.bars.len(),
            "Linkage::A and Linkage::C must produce same-length \
             symbols (same codeword count)"
        );
    }

    #[test]
    fn multi_ai_payload_with_fnc1_separator() {
        // (10) is variable-length, followed by another element → FNC1 inserted.
        // Stage 11.A8c (cont) — descriptive label naming the multi-AI
        // path: variable-length (10) batch ID, FNC1 separator inserted
        // before the (01) GTIN that follows.
        let p = encode("(10)A1B2(01)04012345123456", &Options::default()).expect(
            "encode(\"(10)A1B2(01)04012345123456\", default) (multi-AI: variable-length AI=10 batch ID + FNC1 separator + AI=01 GTIN-14) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode(\"(10)A1B2(01)04012345123456\") (variable-length AI 10 + FNC1 separator + AI 01 GTIN) must compose into non-empty GS1-128 symbol; got {}",
            p.total_width()
        );
    }

    /// GS1-128 golden bar pattern from
    /// `raw("gs1-128", "(01)04012345123456", {})[0].sbs`. Locks down
    /// the fix to `pick_initial_subset`: the leading FNC1 must be
    /// transparent so the encoder picks Subset C for the 16 trailing
    /// digit pairs (otherwise we'd emit Subset B + a redundant
    /// C-switch, costing one extra codeword).
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the GS1-128 byte-for-byte 73-bar SBS oracle path: locks down
        // pick_initial_subset's leading-FNC1 transparency so the
        // encoder picks Subset C directly for 16 trailing digit pairs.
        let p = encode("(01)04012345123456", &Options::default()).expect(
            "encode(\"(01)04012345123456\", default) (GS1-128 byte-for-byte 73-bar SBS bwip-js raw oracle; pins pick_initial_subset FNC1-transparency → Subset C) must succeed",
        );
        let want: [u8; 73] = [
            2, 1, 1, 2, 3, 2, 4, 1, 1, 1, 3, 1, 2, 2, 2, 1, 2, 2, 1, 2, 1, 3, 2, 2, 2, 2, 2, 1, 2,
            2, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 2, 2, 3, 2, 1, 3, 1, 1, 2, 3, 3, 3, 1, 1,
            2, 1, 3, 2, 2, 1, 1, 2, 2, 3, 3, 1, 1, 1, 2,
        ];
        assert_eq!(p.bars, want, "gs1-128 bars mismatch vs bwip-js raw output");
    }

    /// Multi-AI cross-validation: a payload combining GTIN + net
    /// weight (`(3103)`) exercises the FNC1 *separator* between
    /// elements as well as the FNC1 *header*. Also covers a different
    /// AI pair (`(01)` + `(17)` best-before date) and a payload long
    /// enough to span more than 80 sbs runs.
    #[test]
    fn matches_bwip_js_multi_ai() {
        let cases: &[(&str, &[u8])] = &[
            (
                "(01)90012345678908(3103)001750",
                &[
                    2, 1, 1, 2, 3, 2, 4, 1, 1, 1, 3, 1, 2, 2, 2, 1, 2, 2, 2, 1, 4, 1, 2, 1, 2, 2,
                    2, 1, 2, 2, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 4, 1, 1, 2, 2, 2, 1, 2, 1,
                    4, 1, 1, 3, 2, 2, 1, 2, 2, 1, 2, 3, 2, 1, 1, 2, 1, 2, 2, 3, 2, 1, 2, 2, 2, 2,
                    1, 2, 3, 2, 2, 1, 2, 3, 1, 1, 3, 1, 2, 1, 2, 1, 4, 1, 2, 3, 3, 1, 1, 1, 2,
                ],
            ),
            (
                "(01)04012345123456(17)211231",
                &[
                    2, 1, 1, 2, 3, 2, 4, 1, 1, 1, 3, 1, 2, 2, 2, 1, 2, 2, 1, 2, 1, 3, 2, 2, 2, 2,
                    2, 1, 2, 2, 3, 1, 2, 1, 3, 1, 1, 1, 3, 1, 2, 3, 1, 1, 2, 2, 3, 2, 1, 3, 1, 1,
                    2, 3, 3, 3, 1, 1, 2, 1, 1, 2, 3, 2, 2, 1, 2, 1, 3, 2, 1, 2, 1, 1, 2, 2, 3, 2,
                    2, 1, 2, 3, 2, 1, 2, 1, 1, 3, 3, 1, 2, 3, 3, 1, 1, 1, 2,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the multi-AI corpus row.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode({text:?}, default) (GS1-128 multi-AI corpus row: AI 01 + 3103 or AI 01 + 17 with FNC1 separator) must succeed: {e:?}",
                )
            });
            assert_eq!(got.bars, want, "gs1-128 sbs mismatch for {text:?}");
        }
    }

    /// Kills `encode_ean14: replace % with +` at line ~149 (the
    /// `i % 2 == 0` check that selects the 3× weighting in the
    /// EAN-14 check-digit calculation). The existing test rows used
    /// inputs where the original and mutant arithmetic coincidentally
    /// computed the same check digit; this row picks "1234567890123"
    /// where original=1 and mutant=9 diverge.
    #[test]
    fn ean14_weighting_arithmetic_distinguishes_check_digit() {
        // Compute via the encoder: the encoder appends the original-
        // arithmetic check digit (1), so "1234567890123" encodes
        // without error. Under the mutant the loop always picks the
        // "else d" branch, computing check=9; encode_ean14 with the
        // 13-digit input would still succeed but the bars would
        // change because the appended check digit shifts.
        //
        // To distinguish, feed the 14-digit form with the
        // *original*-computed check 1: the encoder validates it.
        // Under the mutant the computed check is 9, the validator
        // sees 1, and returns InvalidData.
        let result = encode_ean14("12345678901231", &Options::default());
        assert!(
            result.is_ok(),
            "encode_ean14(\"12345678901231\") should accept the supplied check digit '1' \
             which is the BWIPP-spec mod-10 result; the encoder's `i % 2` weighting may have flipped"
        );
        // Symmetric counter: with the mutant-computed check '9'
        // (which is invalid under the original), the encoder must
        // reject.
        assert!(
            matches!(
                encode_ean14("12345678901239", &Options::default()),
                Err(Error::InvalidData(_))
            ),
            "encode_ean14(\"12345678901239\") should reject — '9' is the mutant's check digit"
        );
    }
}
