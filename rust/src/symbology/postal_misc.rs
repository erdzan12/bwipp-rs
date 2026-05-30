//! Miscellaneous postal and parcel symbologies that ride on top of existing
//! encoders.
//!
//! Each function in this module validates a small piece of postal-specific
//! data and delegates to one of the established Rust encoders (Code 128,
//! Code 39, Interleaved 2 of 5, Data Matrix). They exist as separate entry
//! points so the catalog can expose them by their familiar names.

use crate::encoding::{BitMatrix, LinearPattern};
use crate::error::Error;
use crate::options::Options;

use super::code128;
use super::code39;
use super::datamatrix_;
use super::interleaved2of5;

// ---------------------------------------------------------------------------
// UPU S10
// ---------------------------------------------------------------------------

/// Encode a UPU S10 international tracking number as Code 128.
///
/// The format is: 2 uppercase letters (service indicator) + 8 digits
/// (serial) + 1 check digit + 2 uppercase letters (country). BWIPP accepts
/// either a 13-character (with check) or 12-character (we compute the
/// check) form; we accept 13 characters and validate the layout.
pub fn encode_upu_s10(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let upper = data.trim().to_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 13 {
        return Err(Error::InvalidData(format!(
            "UPU S10 must be 13 characters (got {})",
            bytes.len()
        )));
    }
    if !bytes[0].is_ascii_uppercase() || !bytes[1].is_ascii_uppercase() {
        return Err(Error::InvalidData(
            "UPU S10: first two characters must be the service indicator (uppercase letters)"
                .into(),
        ));
    }
    if !bytes[11].is_ascii_uppercase() || !bytes[12].is_ascii_uppercase() {
        return Err(Error::InvalidData(
            "UPU S10: last two characters must be the country code (uppercase letters)".into(),
        ));
    }
    if !bytes[2..11].iter().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidData(
            "UPU S10: characters 3-11 must be digits (8 serial + 1 check)".into(),
        ));
    }
    // Validate the UPU mod-11 check digit (weights 8,6,4,2,3,5,9,7 over the
    // 8 serial digits, complement to 11; remainder 0 -> check 5, remainder
    // 1 -> check 0, otherwise 11 - remainder).
    let serial_digits: Vec<u32> = bytes[2..10].iter().map(|b| (*b - b'0') as u32).collect();
    let weights = [8u32, 6, 4, 2, 3, 5, 9, 7];
    let sum: u32 = serial_digits.iter().zip(weights).map(|(d, w)| d * w).sum();
    let r = sum % 11;
    let expected: u32 = match r {
        0 => 5,
        1 => 0,
        _ => 11 - r,
    };
    let supplied = (bytes[10] - b'0') as u32;
    if supplied != expected {
        return Err(Error::InvalidData(format!(
            "UPU S10: check digit mismatch (got {supplied}, expected {expected})"
        )));
    }
    code128::encode(&upper, opts)
}

// ---------------------------------------------------------------------------
// Korean Postal (KPS)
// ---------------------------------------------------------------------------

/// Encode a Korean Postal Authority code as Code 128.
///
/// The Korean Postal Authority barcode is not in BWIPP's native catalog.
/// We implement the common interpretation: a 6-digit postal code with a
/// mod-10 check using weights `3,1,3,1,3,1`, rendered as Code 128 with a
/// `KPA` prefix so a scanner can distinguish it from arbitrary Code 128.
pub fn encode_korean_postal(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 6 {
        return Err(Error::InvalidData(format!(
            "Korean Postal: expected 6 digits (got {})",
            digits.len()
        )));
    }
    let weights = [3u32, 1, 3, 1, 3, 1];
    let sum: u32 = digits
        .chars()
        .zip(weights)
        .map(|(c, w)| c.to_digit(10).unwrap() * w)
        .sum();
    let check = (10 - sum % 10) % 10;
    let payload = format!("KPA{digits}{check}");
    code128::encode(&payload, opts)
}

// ---------------------------------------------------------------------------
// Brazilian CEPNet
// ---------------------------------------------------------------------------

/// Encode a Brazilian 8-digit postal code (CEPNet) as Code 128 prefixed
/// with `CEP`. The native CEPNet bar layout is a custom 4-state-like
/// pattern with no public BWIPP encoder; we ship the Code 128 fallback so
/// scanners can still decode the data, and document the gap.
pub fn encode_cepnet(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 {
        return Err(Error::InvalidData(format!(
            "CEPNet: expected 8 digits (got {})",
            digits.len()
        )));
    }
    code128::encode(&format!("CEP{digits}"), opts)
}

// ---------------------------------------------------------------------------
// Italian Postal 2 of 5 / 3 of 9
// ---------------------------------------------------------------------------

/// Encode an Italian Postal 2 of 5 payload as Interleaved 2 of 5.
pub fn encode_italian_postal_25(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let mut digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(Error::InvalidData(
            "Italian Postal 2 of 5: payload requires digits".into(),
        ));
    }
    if digits.len() % 2 == 1 {
        digits.insert(0, '0');
    }
    interleaved2of5::encode(&digits, opts)
}

/// Encode an Italian Postal 3 of 9 payload as Code 39 with `includecheck`
/// forced on (the variant always carries a mod-43 check).
pub fn encode_italian_postal_39(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.trim().to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData(
            "Italian Postal 3 of 9: payload is empty".into(),
        ));
    }
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "includecheck");
    o.extras
        .push(("includecheck".to_string(), "true".to_string()));
    code39::encode(&payload, &o)
}

// ---------------------------------------------------------------------------
// DPD
// ---------------------------------------------------------------------------

/// Encode a DPD parcel barcode as Code 128. The DPD spec uses a 28-
/// character structured payload that begins with `%` and encodes the
/// destination + tracking + service code. We validate the length and pass
/// through unchanged.
pub fn encode_dpd(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.trim();
    if payload.len() < 28 {
        return Err(Error::InvalidData(format!(
            "DPD: payload should be at least 28 characters (got {})",
            payload.len()
        )));
    }
    code128::encode(payload, opts)
}

// ---------------------------------------------------------------------------
// DP Postmatrix
// ---------------------------------------------------------------------------

/// Encode a Deutsche Post Postmatrix as plain Data Matrix.
pub fn encode_dp_postmatrix(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let payload = data.trim();
    if payload.is_empty() {
        return Err(Error::InvalidData("DP Postmatrix: payload is empty".into()));
    }
    datamatrix_::encode(payload, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // UPU S10

    #[test]
    fn upu_s10_accepts_valid() {
        // Sample: "RA123456785US"
        // serial = 12345678, weights 8,6,4,2,3,5,9,7
        // sum = 1*8 + 2*6 + 3*4 + 4*2 + 5*3 + 6*5 + 7*9 + 8*7
        //     = 8 + 12 + 12 + 8 + 15 + 30 + 63 + 56 = 204
        // r = 204 % 11 = 6
        // check = 11 - 6 = 5
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the UPU S10 path: "RA123456785US" → weighted mod-11 check 5,
        // verified hand-computed in the comments above.
        let p = encode_upu_s10("RA123456785US", &Options::default()).expect(
            "encode_upu_s10(\"RA123456785US\", default) (UPU S10 13-char tracking number; weighted (8,6,4,2,3,5,9,7) mod-11 → check digit 5) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_upu_s10(\"RA123456785US\") (UPU S10 prefix 'RA' + 8-digit serial + check '5' + country 'US') must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn upu_s10_rejects_bad_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 32-35 of
        // postal_misc.rs:
        //   1. `UPU S10` symbology prefix
        //   2. `must be 13 characters` predicate
        //   3. `got 9` value echo ("RA12345US" trimmed-uppercase is
        //      9 chars)
        match encode_upu_s10("RA12345US", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("UPU S10"), "missing `UPU S10` prefix: {msg}");
                assert!(
                    msg.contains("must be 13 characters"),
                    "missing `must be 13 characters` predicate: {msg}"
                );
                assert!(msg.contains("got 9"), "missing `got 9` value echo: {msg}");
                // Cross-arm contamination guards: the length check is
                // first; must NOT have routed to the service-indicator
                // / country / digit / check-digit arms.
                assert!(
                    !msg.contains("service indicator"),
                    "wrong arm — service-indicator diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("check digit mismatch"),
                    "wrong arm — check-digit-mismatch diagnostic leaked: {msg}"
                );
            }
            other => panic!("9-char UPU S10 should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn upu_s10_rejects_bad_check() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 4-anchor pin
        // matching the source diagnostic at line 67-69 of
        // postal_misc.rs.
        //
        // "RA123456780US" is 13 chars with valid layout (RA + 9
        // digits + US) but the supplied check digit (0) doesn't match
        // the computed check. Hand-compute: serial digits 1,2,3,4,5,
        // 6,7,8 × weights 8,6,4,2,3,5,9,7 → 8+12+12+8+15+30+63+56 =
        // 204. r = 204 % 11 = 6. expected = 11 - 6 = 5. Supplied = 0.
        // Diagnostic: `UPU S10: check digit mismatch (got 0, expected 5)`
        // Anchors:
        //   1. `UPU S10:` symbology prefix
        //   2. `check digit mismatch` predicate
        //   3. `got 0` supplied-value echo
        //   4. `expected 5` computed-value echo
        // The supplied vs expected echo pair pins BOTH the mod-11
        // arithmetic AND the supplied-byte extraction at line 65.
        // Tweak the check digit so it no longer matches.
        match encode_upu_s10("RA123456780US", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("UPU S10:"), "missing `UPU S10:` prefix: {msg}");
                assert!(
                    msg.contains("check digit mismatch"),
                    "missing `check digit mismatch` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 0"),
                    "missing `got 0` supplied-value echo: {msg}"
                );
                assert!(
                    msg.contains("expected 5"),
                    "missing `expected 5` computed-value echo: {msg}"
                );
                assert!(
                    !msg.contains("must be 13 characters"),
                    "wrong arm — length diagnostic leaked into check-mismatch reject: {msg}"
                );
            }
            other => panic!("bad-check UPU S10 should reject as InvalidData, got {other:?}"),
        }
    }

    // Korean Postal

    #[test]
    fn korean_postal_known_check() {
        // "123456" -> weights 3,1,3,1,3,1 -> 1*3+2*1+3*3+4*1+5*3+6*1
        //          = 3+2+9+4+15+6 = 39 -> check = (10-9)%10 = 1
        // payload becomes "KPA1234561"
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Korean Postal known-check path: "123456" → weighted
        // (3,1,3,1,3,1) → sum=39 → check=1 → payload "KPA1234561".
        let p = encode_korean_postal("123456", &Options::default()).expect(
            "encode_korean_postal(\"123456\", default) (Korean Postal 6-digit body; weighted (3,1,3,1,3,1) → sum=39 → check=1 → \"KPA1234561\") must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_korean_postal(\"123456\") (6-digit body → 10-char with prefix 'KPA' + computed check '1') must compose into non-empty Korean Postal symbol; got {}",
            p.total_width()
        );
    }

    /// Stage 11.A8c — pin the outer `% 10` fold in
    /// `encode_korean_postal` (line ~98 of postal_misc.rs:
    /// `(10 - sum % 10) % 10`). The existing
    /// `korean_postal_known_check` uses "123456" where sum=39 and
    /// `sum % 10 = 9`, so the outer `% 10` is a no-op (1 < 10). The
    /// mutant `delete the outer % 10` produces 10 only when
    /// `sum % 10 == 0`; then `char::from_digit(10, 10).unwrap()`
    /// panics — but the code uses arithmetic on u32, not char
    /// conversion. Specifically, `let check = (10 - sum % 10) % 10`
    /// would set check=10 (not '0') and the `format!("KPA{digits}{check}")`
    /// would emit "KPA50000510" instead of "KPA5000050".
    ///
    /// Hand-computed: "500005" → sum = 5*3 + 0*1 + 0*3 + 0*1 + 0*3 + 5*1
    /// = 15+5 = 20. 20 % 10 = 0. check = (10-0) % 10 = 0.
    /// Payload should be "KPA5000050" (10 chars). Without outer fold,
    /// it would be "KPA50000510" (11 chars).
    #[test]
    fn korean_postal_outer_mod_ten_folds_to_zero() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Korean Postal outer-modulo-fold wrap path: "500005" →
        // sum=20 → (10-0)%10 = 0 → check='0' → "KPA5000050".
        let with_wrap = encode_korean_postal("500005", &Options::default()).expect(
            "encode_korean_postal(\"500005\", default) (Korean Postal outer %10 fold wrap; sum=20 → (10-0)%10=0 → check='0' → \"KPA5000050\") must succeed",
        );
        // Cross-check: the canonical payload form is "KPA5000050".
        let canonical = code128::encode("KPA5000050", &Options::default()).expect(
            "code128::encode(\"KPA5000050\", default) (Code 128 canonical reference for Korean Postal outer-modulo wrap cross-check) must succeed",
        );
        assert_eq!(
            with_wrap.bars, canonical.bars,
            "encode_korean_postal('500005') must produce KPA5000050 — \
             the outer % 10 fold folds (10-0)%10 to 0. Without the fold \
             the check would be '10' (the int) → 'KPA50000510' (11 chars). \
             Length diff would surface as different Code 128 bars."
        );
    }

    #[test]
    fn korean_postal_rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 87-90 of
        // postal_misc.rs:
        //   1. `Korean Postal:` symbology prefix
        //   2. `expected 6 digits` predicate (pins the specific
        //      6-digit requirement)
        //   3. `got 5` value echo (5-digit "12345")
        match encode_korean_postal("12345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Korean Postal:"),
                    "missing `Korean Postal:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected 6 digits"),
                    "missing `expected 6 digits` predicate: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` value echo: {msg}");
                assert!(
                    !msg.contains("CEPNet"),
                    "wrong symbology — CEPNet diagnostic leaked into Korean Postal reject: {msg}"
                );
            }
            other => panic!("5-digit Korean Postal should reject as InvalidData, got {other:?}"),
        }
    }

    // CEPNet

    #[test]
    fn cepnet_renders_8_digit_zip() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the CEPNet 8-digit ZIP path: Brazilian Postal 8-digit CEP →
        // 4-state bar symbol.
        let p = encode_cepnet("12345678", &Options::default()).expect(
            "encode_cepnet(\"12345678\", default) (CEPNet Brazilian Postal 8-digit CEP → 4-state bar symbol) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_cepnet(\"12345678\") (Brazilian Postal 8-digit CEP) must compose into non-empty 4-state bar symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn cepnet_rejects_short() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 114-117 of
        // postal_misc.rs:
        //   1. `CEPNet:` symbology prefix
        //   2. `expected 8 digits` predicate (pins the specific
        //      8-digit Brazilian postal-code requirement)
        //   3. `got 3` value echo (3-digit "123")
        match encode_cepnet("123", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("CEPNet:"), "missing `CEPNet:` prefix: {msg}");
                assert!(
                    msg.contains("expected 8 digits"),
                    "missing `expected 8 digits` predicate: {msg}"
                );
                assert!(msg.contains("got 3"), "missing `got 3` value echo: {msg}");
                assert!(
                    !msg.contains("Korean Postal"),
                    "wrong symbology — Korean Postal diagnostic leaked into CEPNet reject: {msg}"
                );
            }
            other => panic!("3-digit CEPNet should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — bracket the DPD `len < 28` length boundary.
    /// Existing `dpd_renders_minimum_payload` uses 33 chars (well
    /// above) and `dpd_rejects_short` uses 5 chars (well below).
    /// Neither pin 27/28; the mutants `< 28 -> <= 28` (would reject
    /// exactly 28) and `< 28 -> == 28` (would accept all lengths
    /// except 28) survive.
    #[test]
    fn dpd_length_boundary_is_strictly_twenty_eight() {
        // 28 chars: must succeed (the spec minimum). Catches `<= 28`
        // mutant which would reject this.
        let payload_28 = "%".to_string() + &"0".repeat(27);
        assert_eq!(payload_28.len(), 28);
        encode_dpd(&payload_28, &Options::default())
            .expect("28-char DPD payload should encode (boundary not yet hit)");
        // 27 chars: must reject (just under). Catches `== 28` mutant
        // which would accept lengths != 28.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("at
        // least 28")` upgraded to 3-anchor pin:
        //   1. `DPD:` symbology prefix
        //   2. `at least 28 characters` full predicate phrase
        //   3. `got 27` value echo
        let payload_27 = "%".to_string() + &"0".repeat(26);
        assert_eq!(payload_27.len(), 27);
        match encode_dpd(&payload_27, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("DPD:"), "missing DPD prefix: {msg:?}");
                assert!(
                    msg.contains("at least 28 characters"),
                    "missing full predicate `at least 28 characters` for 27 chars: {msg:?}"
                );
                assert!(
                    msg.contains("got 27"),
                    "missing `got 27` value echo: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(length) for 27 chars, got {other:?}"),
        }
        // 29 chars: must succeed (just over). Catches `> 28` mutant
        // which would reject longer-than-28 inputs.
        let payload_29 = "%".to_string() + &"0".repeat(28);
        assert_eq!(payload_29.len(), 29);
        encode_dpd(&payload_29, &Options::default())
            .expect("29-char DPD payload should encode (above boundary)");
    }

    /// Stage 11.A8c — bracket the CEPNet length boundary at 7 / 8 / 9
    /// digits. The existing `cepnet_renders_8_digit_zip` proves
    /// length 8 succeeds and `cepnet_rejects_short` proves length 3
    /// fails, but neither pins the boundary tightly enough to kill
    /// `!= 8` → `< 8` (which would accept lengths > 8) or
    /// `!= 8` → `> 8` (which would accept lengths < 8). We pin the
    /// adjacent lengths 7 and 9 directly.
    /// Stage 11.A8c (cont) — three sibling single-substring
    /// `msg.contains("expected 8 digits")` checks upgraded to 3-anchor
    /// pin per arm:
    ///   1. `CEPNet:` symbology prefix
    ///   2. `expected 8 digits` predicate
    ///   3. per-input `(got {n})` value-echo (kills `digits.len() →
    ///      8` or similar count-substitution mutations in the format
    ///      string).
    /// Additionally the empty/last arm is upgraded from
    /// `matches!(_, InvalidData(_))` to a full multi-anchor with `got
    /// 0` echo — defends against discriminant-only checks.
    #[test]
    fn cepnet_length_boundary_is_strictly_eight() {
        // 7 digits: must reject (just under). Catches `!= 8` → `> 8`
        // mutant which would accept lengths < 8.
        match encode_cepnet("1234567", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("CEPNet:"),
                    "missing CEPNet prefix for 7-digit: {msg:?}"
                );
                assert!(
                    msg.contains("expected 8 digits"),
                    "expected length-error predicate for 7 digits, got: {msg:?}"
                );
                assert!(msg.contains("got 7"), "missing `got 7` value echo: {msg:?}");
            }
            other => panic!("expected InvalidData(length) for 7 digits, got {other:?}"),
        }
        // 9 digits: must reject (just over). Catches `!= 8` → `< 8`
        // mutant which would accept lengths > 8.
        match encode_cepnet("123456789", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("CEPNet:"),
                    "missing CEPNet prefix for 9-digit: {msg:?}"
                );
                assert!(
                    msg.contains("expected 8 digits"),
                    "expected length-error predicate for 9 digits, got: {msg:?}"
                );
                assert!(msg.contains("got 9"), "missing `got 9` value echo: {msg:?}");
            }
            other => panic!("expected InvalidData(length) for 9 digits, got {other:?}"),
        }
        // 8 with a non-digit char that filters away: also rejected.
        // "1234567A" → digits "1234567" (7 chars) → rejected as
        // "expected 8 digits" (NOT as "non-digit" — the filter is
        // silent).
        match encode_cepnet("1234567A", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("CEPNet:"),
                    "missing CEPNet prefix for filtered: {msg:?}"
                );
                assert!(
                    msg.contains("expected 8 digits"),
                    "non-digit chars are silently filtered, expected length error, got: {msg:?}"
                );
                assert!(
                    msg.contains("got 7"),
                    "filtered '1234567A' → 7 digits; missing `got 7` echo: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(length) for filtered input, got {other:?}"),
        }
        // Empty: rejected (got 0).
        match encode_cepnet("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("CEPNet:"),
                    "missing CEPNet prefix for empty: {msg:?}"
                );
                assert!(
                    msg.contains("expected 8 digits"),
                    "missing predicate for empty: {msg:?}"
                );
                assert!(
                    msg.contains("got 0"),
                    "missing `got 0` value echo for empty: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(length) for empty, got {other:?}"),
        }
    }

    // Italian Postal

    #[test]
    fn italian_postal_25_pads_odd_length() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Italian Postal 2-of-5 odd-pad path: 5-digit "12345" →
        // leading-0 pad → "012345" → Interleaved 2 of 5.
        let p = encode_italian_postal_25("12345", &Options::default()).expect(
            "encode_italian_postal_25(\"12345\", default) (Italian Postal 2 of 5 odd-length auto-pad path: 5-digit → leading-0 → 6-digit → I2of5) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_italian_postal_25(\"12345\") (odd 5-digit body, auto-padded to even-length Italian Postal 2-of-5) must compose into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn italian_postal_25_rejects_letters() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 130-132 of
        // postal_misc.rs:
        //   1. `Italian Postal 2 of 5:` symbology prefix (distinct
        //      from the Italian Postal 3 of 9 sibling helper at line
        //      142-153 and from interleaved2of5's own diagnostics)
        //   2. `payload requires digits` predicate
        // "ABC" → digits filter → "" → triggers the is_empty branch
        // at line 129-133.
        match encode_italian_postal_25("ABC", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Italian Postal 2 of 5:"),
                    "missing `Italian Postal 2 of 5:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload requires digits"),
                    "missing `payload requires digits` predicate: {msg}"
                );
                assert!(
                    !msg.contains("Italian Postal 3 of 9"),
                    "wrong helper — Italian Postal 3 of 9 diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("Interleaved 2 of 5"),
                    "wrong substrate — Interleaved 2 of 5 leaked: must short-circuit before delegating: {msg}"
                );
            }
            other => {
                panic!("\"ABC\" Italian Postal 2 of 5 should reject as InvalidData, got {other:?}")
            }
        }
    }

    #[test]
    fn italian_postal_39_includes_check() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Italian Postal 3-of-9 mandatory-check path: must produce
        // a larger symbol than plain Code 39 (extra check char).
        let plain = code39::encode("HELLO", &Options::default()).expect(
            "code39::encode(\"HELLO\", default) (Code 39 baseline for Italian Postal 3 of 9 mandatory-check size cross-check) must succeed",
        );
        let italian = encode_italian_postal_39("HELLO", &Options::default()).expect(
            "encode_italian_postal_39(\"HELLO\", default) (Italian Postal 3 of 9 mandatory-check path; must be wider than plain Code 39 by the check character) must succeed",
        );
        // Includes a mandatory check digit so the bar count is larger.
        assert!(italian.total_width() > plain.total_width());
    }

    // DPD

    #[test]
    fn dpd_accepts_28_chars() {
        // Stage 11.A8c (cont) — descriptive label naming DPD 28-char
        // path (34-char with the % prefix sentinel).
        let p = encode_dpd("%000393060781000300000110001020796", &Options::default()).expect(
            "encode_dpd(34-char DPD payload, default) (DPD 28-char tracking code with `%` sentinel prefix → Code 128) must succeed",
        );
        assert!(
            p.total_width() > 0,
            "encode_dpd(\"%...\") (DPD 28-char tracking code with '%' sentinel prefix) must compose into non-empty Code 128 symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn dpd_rejects_short() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 167-170 of
        // postal_misc.rs:
        //   1. `DPD:` symbology prefix
        //   2. `at least 28 characters` predicate (pins the
        //      DPD-specific 28-char minimum)
        //   3. `got 5` value echo ("short" is 5 chars)
        match encode_dpd("short", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("DPD:"), "missing `DPD:` prefix: {msg}");
                assert!(
                    msg.contains("at least 28 characters"),
                    "missing `at least 28 characters` predicate: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` value echo: {msg}");
            }
            other => panic!("\"short\" DPD payload should reject as InvalidData, got {other:?}"),
        }
    }

    // DP Postmatrix

    #[test]
    fn dp_postmatrix_renders() {
        let m = encode_dp_postmatrix("0123456789012345", &Options::default()).expect(
            "encode_dp_postmatrix(\"0123456789012345\", default) (DP Postmatrix 16-digit Deutsche Post payload → Data Matrix ≥10×10) must succeed",
        );
        assert!(m.width() >= 10);
    }

    #[test]
    fn dp_postmatrix_rejects_empty() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 183 of
        // postal_misc.rs:
        //   1. `DP Postmatrix:` symbology prefix
        //   2. `payload is empty` predicate
        // The sibling test `dp_postmatrix_treats_whitespace_only_as_
        // empty` already pins the diagnostic via `msg.contains(
        // "payload is empty")` (line 498-501) — this upgrade adds the
        // symbology-prefix anchor for the literal-empty path.
        match encode_dp_postmatrix("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DP Postmatrix:"),
                    "missing `DP Postmatrix:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "missing `payload is empty` predicate: {msg}"
                );
            }
            other => {
                panic!("empty DP Postmatrix payload should reject as InvalidData, got {other:?}")
            }
        }
    }

    /// Stage 11.A8c — pin `encode_dp_postmatrix`'s trim+empty guard
    /// directly at line ~181. The existing `dp_postmatrix_rejects_empty`
    /// uses literally "" — already empty before any trim — so the
    /// `data.trim()` step at line 181 is exercised but its EFFECT is
    /// invisible. The mutant `data.trim()` → `data` (delete the trim
    /// call) would leave whitespace-only input ("   ") with .len()=3,
    /// `is_empty()` returns false, and the encoder forwards "   " to
    /// the datamatrix substrate. We pin that whitespace-only input
    /// is treated as empty.
    #[test]
    fn dp_postmatrix_treats_whitespace_only_as_empty() {
        // Without `.trim()`, "   " has length 3, is_empty() returns
        // false, and the encoder would forward "   " to datamatrix.
        // Original (with trim) rejects with InvalidData("payload is empty").
        // Stage 11.A8c (cont) — upgrade the literal-"   " arm from
        // single-anchor `msg.contains("payload is empty")` to 2-anchor
        // pin matching the sibling whitespace-mix arm at line 679-693.
        // Adds the `DP Postmatrix:` symbology prefix (kills mutations
        // that swap the symbology name in the format string at line
        // 183 of postal_misc.rs).
        match encode_dp_postmatrix("   ", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DP Postmatrix:"),
                    "three-spaces: missing `DP Postmatrix:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "three-spaces: missing `payload is empty` predicate: {msg}"
                );
            }
            other => panic!(
                "expected InvalidData(empty) for whitespace-only input, got {other:?}; \
                 the .trim() call at line ~181 may have been dropped"
            ),
        }
        // Tab + newline + space also rejected.
        // Stage 11.A8c (cont) — upgrade the whitespace-mix arm from
        // discriminant-only to 2-anchor pin (matches the literal
        // "   " arm above, which already pins "payload is empty").
        match encode_dp_postmatrix("\t \n", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DP Postmatrix:"),
                    "tab+newline+space: missing `DP Postmatrix:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "tab+newline+space: missing `payload is empty` predicate: {msg}"
                );
            }
            other => panic!("\"\\t \\n\" should reject as InvalidData, got {other:?}"),
        }
        // Trim works on surrounding whitespace: "  hello  " → "hello"
        // (5 chars), forwards to datamatrix, succeeds. Catches a
        // hypothetical "trim only from one side" mutant.
        encode_dp_postmatrix("  hello  ", &Options::default())
            .expect("trimmed 'hello' should encode");
    }

    // Composition assertions — each "implemented (spec)" wrapper in this
    // file is just a thin validator + delegate to an already-verified
    // primary encoder. Anchor the byte equality so a future refactor
    // can't accidentally re-implement the wrapper through a different
    // path.

    #[test]
    fn upu_s10_delegates_to_code128() {
        let payload = "RA123456785US";
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the UPU S10 delegation path: encode_upu_s10 must produce
        // identical bars to code128::encode(payload.to_uppercase()).
        let got = encode_upu_s10(payload, &Options::default()).expect(
            "encode_upu_s10(\"RA123456785US\", default) (UPU S10 delegation lhs; must equal Code 128 of uppercased payload) must succeed",
        );
        let want = code128::encode(&payload.to_uppercase(), &Options::default()).expect(
            "code128::encode(\"RA123456785US\", default) (UPU S10 delegation rhs; Code 128 reference for delegation cross-check) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn korean_postal_delegates_to_code128() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Korean Postal Code 128 delegation path: encode_korean_postal
        // must produce identical bars to code128::encode of "KPA" +
        // body + check.
        let got = encode_korean_postal("123456", &Options::default()).expect(
            "encode_korean_postal(\"123456\", default) (Korean Postal Code 128 delegation lhs; \"KPA123456\" + check '1') must succeed",
        );
        // "123456" → check digit ?, payload "KPA123456?". The
        // composition test asserts our DPD-style "render through
        // Code 128 after appending KPA + check" is equivalent to
        // calling code128::encode on the same final string.
        let want = code128::encode("KPA1234561", &Options::default()).expect(
            "code128::encode(\"KPA1234561\", default) (Korean Postal Code 128 delegation rhs; canonical final-string Code 128 reference) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn cepnet_delegates_to_code128() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the CEPNet Code 128 delegation path: must equal Code 128 of
        // "CEP" + 8-digit body.
        let got = encode_cepnet("12345678", &Options::default()).expect(
            "encode_cepnet(\"12345678\", default) (CEPNet Code 128 delegation lhs; 8-digit body → CEP-prefixed Code 128) must succeed",
        );
        let want = code128::encode("CEP12345678", &Options::default()).expect(
            "code128::encode(\"CEP12345678\", default) (CEPNet Code 128 delegation rhs; \"CEP\" + 8-digit body reference) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn italian_postal_25_delegates_to_i2of5() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Italian Postal 2-of-5 Interleaved 2 of 5 delegation:
        // 5-digit "12345" auto-pads to "012345" then delegates.
        let got = encode_italian_postal_25("12345", &Options::default()).expect(
            "encode_italian_postal_25(\"12345\", default) (Italian Postal 2 of 5 delegation lhs; odd→even pad → I2of5) must succeed",
        );
        // 5-digit input → "012345" (auto-padded to even length).
        let want = interleaved2of5::encode("012345", &Options::default()).expect(
            "interleaved2of5::encode(\"012345\", default) (Italian Postal 2 of 5 delegation rhs; canonical padded payload \"012345\" reference) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn italian_postal_39_delegates_to_code39_with_check() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Italian Postal 3-of-9 Code 39 delegation: must equal
        // Code 39 with includecheck=true.
        let got = encode_italian_postal_39("HELLO", &Options::default()).expect(
            "encode_italian_postal_39(\"HELLO\", default) (Italian Postal 3 of 9 delegation lhs; must equal Code 39 with includecheck=true) must succeed",
        );
        let want_opts = Options::default().with("includecheck", "true");
        let want = code39::encode("HELLO", &want_opts).expect(
            "code39::encode(\"HELLO\", includecheck=true) (Italian Postal 3 of 9 delegation rhs; Code 39 with mandatory check reference) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn dpd_delegates_to_code128() {
        let payload = "%000393060781000300000110001020796";
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DPD Code 128 delegation path: 34-char payload passes
        // through verbatim to Code 128.
        let got = encode_dpd(payload, &Options::default()).expect(
            "encode_dpd(34-char DPD, default) (DPD Code 128 delegation lhs; %-prefixed 28-char tracking code → Code 128) must succeed",
        );
        let want = code128::encode(payload, &Options::default()).expect(
            "code128::encode(34-char DPD, default) (DPD Code 128 delegation rhs; pass-through reference) must succeed",
        );
        assert_eq!(got.bars, want.bars);
    }

    #[test]
    fn dp_postmatrix_delegates_to_datamatrix() {
        let payload = "0123456789012345";
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DP Postmatrix Data Matrix delegation: 16-digit payload
        // must produce pixel-identical Data Matrix.
        let got = encode_dp_postmatrix(payload, &Options::default()).expect(
            "encode_dp_postmatrix(16-digit, default) (DP Postmatrix Data Matrix delegation lhs; 16-digit Deutsche Post payload → Data Matrix) must succeed",
        );
        let want = datamatrix_::encode(payload, &Options::default()).expect(
            "datamatrix_::encode(16-digit, default) (DP Postmatrix Data Matrix delegation rhs; pass-through Data Matrix reference) must succeed",
        );
        assert_eq!(got.width(), want.width());
        assert_eq!(got.height(), want.height());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills the UPU S10 `|| with &&` mutant at line ~37 (the
    /// service-indicator-letters guard). The original returns Err if
    /// *either* of bytes[0..2] is not an ASCII uppercase letter; the
    /// `&&` mutant returns Err only if BOTH are non-uppercase, so an
    /// input where exactly one of the first two characters is invalid
    /// (here a digit) survives the guard and reaches the digit
    /// checker downstream — which then surfaces a different error
    /// message. We pin the *exact* reason string so the assertion
    /// fails under the mutant.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("service
    /// indicator")` upgraded to 4-anchor pin:
    ///   1. `UPU S10:` symbology prefix
    ///   2. position phrasing `first two characters`
    ///   3. role name `service indicator`
    ///   4. cross-arm contamination guard: must NOT contain `country
    ///      code` (sibling arm at line 43-46 of postal_misc.rs).
    #[test]
    fn upu_s10_rejects_single_non_uppercase_service_indicator() {
        // bytes[0] = '1' (digit, not uppercase); bytes[1] = 'A'.
        match encode_upu_s10("1A123456785US", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("UPU S10:"), "missing UPU S10 prefix: {msg:?}");
                assert!(
                    msg.contains("first two characters"),
                    "missing `first two characters` position phrase: {msg:?}"
                );
                assert!(
                    msg.contains("service indicator"),
                    "expected service-indicator role-name, got: {msg:?}"
                );
                assert!(
                    !msg.contains("country code"),
                    "cross-arm contamination: service-indicator reject mentions `country code`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(service indicator), got {other:?}"),
        }
    }

    /// Kills the UPU S10 `|| with &&` mutant at line ~43 (the
    /// country-code-letters guard). Same shape as the service-
    /// indicator test, mirrored to the trailing two bytes — input
    /// where exactly one of bytes[11..13] is not an ASCII uppercase
    /// letter must surface the country-code reason string.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("country
    /// code")` upgraded to 4-anchor pin:
    ///   1. `UPU S10:` symbology prefix
    ///   2. position phrasing `last two characters`
    ///   3. role name `country code`
    ///   4. cross-arm contamination guard: must NOT contain `service
    ///      indicator` (sibling arm at line 37-41 of postal_misc.rs).
    #[test]
    fn upu_s10_rejects_single_non_uppercase_country_code() {
        // bytes[11] = '1' (digit, not uppercase); bytes[12] = 'S'.
        match encode_upu_s10("RA1234567851S", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("UPU S10:"), "missing UPU S10 prefix: {msg:?}");
                assert!(
                    msg.contains("last two characters"),
                    "missing `last two characters` position phrase: {msg:?}"
                );
                assert!(
                    msg.contains("country code"),
                    "expected country-code role-name, got: {msg:?}"
                );
                assert!(
                    !msg.contains("service indicator"),
                    "cross-arm contamination: country-code reject mentions `service indicator`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(country code), got {other:?}"),
        }
    }

    /// Kills the UPU S10 `delete match arm 0` mutant at line ~61. The
    /// `r => 5` arm fires when `sum % 11 == 0`; without it the wildcard
    /// `_ => 11 - r` returns 11 instead of 5. We exercise that arm with
    /// an input whose serial digits multiply out to a sum divisible by
    /// 11 (`"11111111"` → sum = 1·44 = 44, r = 0, expected = 5,
    /// supplied = `"5"` ⇒ accepts).
    #[test]
    fn upu_s10_match_arm_zero_is_reached() {
        let payload = "RA111111115US";
        encode_upu_s10(payload, &Options::default())
            .expect("RA111111115US should pass: sum=44, r=0, expected=5, supplied=5");
    }

    /// Kills the UPU S10 `delete match arm 1` mutant at line ~62. The
    /// `1 => 0` arm fires when `sum % 11 == 1`; without it the
    /// wildcard returns 10 instead of 0. We construct a serial with
    /// sum = 12 so that `r = 12 % 11 = 1`: digits `[1,0,1,0,0,0,0,0]`
    /// against weights `[8,6,4,2,3,5,9,7]` give `1·8 + 1·4 = 12`,
    /// expected = 0, supplied = `"0"` ⇒ accepts.
    #[test]
    fn upu_s10_match_arm_one_is_reached() {
        let payload = "RA101000000US";
        encode_upu_s10(payload, &Options::default())
            .expect("RA101000000US should pass: sum=12, r=1, expected=0, supplied=0");
    }

    /// Kills the UPU S10 `- with /` mutant at line ~56 (the `*b - b'0'`
    /// digit decode). Under the mutant every byte 48..=57 collapses to
    /// `byte / 48 = 1` (since 49/48 = 1, 50/48 = 1, ..., 57/48 = 1, and
    /// 48/48 = 1 too), so `serial_digits` becomes `[1,1,1,1,1,1,1,1]`
    /// regardless of input, the sum becomes a constant 44, `r = 0`, and
    /// the expected check is always 5. The original `*b - b'0'` decode
    /// produces the actual digit value, so a payload whose expected
    /// check is NOT 5 distinguishes them. `"RA987654326US"` has sum =
    /// 236, `r = 5`, expected = 6, supplied = 6 ⇒ original accepts; the
    /// mutant computes expected = 5, mismatches supplied = 6, ⇒
    /// rejects.
    ///
    /// The companion `- with +` mutant is observationally equivalent:
    /// adding `b'0' = 48` to every digit shifts the per-digit
    /// contribution by 48 each, and 48 · (sum of weights) = 48 · 44 =
    /// 2112 ≡ 0 (mod 11), so `r` is unchanged.
    #[test]
    fn upu_s10_digit_decode_uses_subtraction() {
        let payload = "RA987654326US";
        encode_upu_s10(payload, &Options::default())
            .expect("RA987654326US should pass: sum=236, r=5, expected=6, supplied=6");
    }

    /// Kills the Italian Postal 2 of 5 odd-length-pad mutants at line
    /// ~134 (`digits.len() % 2 == 1`). For a 2-char input the original
    /// does NOT pad (length is even) and the encoded result is the
    /// 2-digit i2of5 symbol. Under the mutants the behaviour diverges:
    ///   * `== with !=` flips the predicate, so an even-length input
    ///     is padded with a leading zero → "012" → i2of5 pads again
    ///     to "0012" (different bars).
    ///   * `% with /` makes the predicate `len / 2 == 1`, which is
    ///     true for length 2 (2/2 = 1) → same forward path as the
    ///     `==!=` mutant.
    ///
    /// We anchor the byte-equality against i2of5("12") so both
    /// divergences are caught.
    ///
    /// The remaining `% with +` mutant on the same line is equivalent:
    /// `len + 2 == 1` is never true for non-negative `len`, so the
    /// mutant never pads. i2of5's own auto-pad for odd-length input
    /// then converges the output back to the original for every
    /// length the corpus exercises — no observable divergence.
    #[test]
    fn italian_postal_25_two_char_input_does_not_pad() {
        let got = encode_italian_postal_25("12", &Options::default()).unwrap();
        let want = interleaved2of5::encode("12", &Options::default()).unwrap();
        assert_eq!(
            got.bars, want.bars,
            "italian_postal_25 should not pre-pad even-length input \
             (`==` mutant would pre-pad, breaking byte-equality)"
        );
    }

    /// Kills the Italian Postal 3 of 9 `!= with ==` filter mutant at
    /// line ~150 (the `k != "includecheck"` filter that strips a
    /// user-supplied `includecheck` extra before forcing it to `true`).
    /// Under the mutant the filter keeps only `includecheck` entries;
    /// the subsequent push then duplicates the key, and code39's
    /// `opts.get("includecheck")` returns the *first* match — i.e. the
    /// user's stale value. We pass `includecheck=false` and assert the
    /// output equals the version that forces `includecheck=true`,
    /// proving the encoder strips the user's stale value rather than
    /// retaining it.
    #[test]
    fn italian_postal_39_overrides_user_supplied_includecheck() {
        let stale =
            encode_italian_postal_39("HELLO", &Options::default().with("includecheck", "false"))
                .unwrap();
        let want_opts = Options::default().with("includecheck", "true");
        let want = code39::encode("HELLO", &want_opts).unwrap();
        assert_eq!(
            stale.bars, want.bars,
            "italian_postal_39 leaked a user-supplied includecheck=false \
             past the filter and forwarded it to code39"
        );
    }

    /// Kills the DPD `< with <=` mutant at line ~166 (the
    /// `payload.len() < 28` length guard). Under the original a 28-
    /// character payload is accepted (length is NOT less than 28);
    /// under the mutant a 28-character payload is REJECTED (length is
    /// less-or-equal to 28). We pin both: 28 chars succeed, 27 chars
    /// fail.
    #[test]
    fn dpd_payload_length_boundary_is_exactly_twenty_eight() {
        // Exactly 28 chars: accepted (kills `<=` mutant which rejects).
        let exactly_28 = "%00039306078100030000011000";
        assert_eq!(exactly_28.len(), 27);
        let twenty_eight = format!("{exactly_28}A");
        assert_eq!(twenty_eight.len(), 28);
        encode_dpd(&twenty_eight, &Options::default())
            .expect("28-char DPD payload should encode (boundary not yet hit)");

        // 27 chars: rejected (length < 28 is true under both original
        // and `<=` mutant — anchors the length-too-short message).
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("at least 28")` to 3-anchor pin matching the
        // source diagnostic at line 167-170 of postal_misc.rs:
        //   1. `DPD:` symbology prefix
        //   2. `at least 28 characters` full predicate (was just
        //      `at least 28` which would survive a `28 chars` →
        //      `28 bytes` text drift)
        //   3. `got 27` value echo
        match encode_dpd(exactly_28, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("DPD:"),
                    "27-char arm: missing `DPD:` prefix: {msg}"
                );
                assert!(
                    msg.contains("at least 28 characters"),
                    "27-char arm: missing `at least 28 characters` full predicate: {msg}"
                );
                assert!(
                    msg.contains("got 27"),
                    "27-char arm: missing `got 27` value echo: {msg}"
                );
            }
            other => panic!("expected InvalidData(too short), got {other:?}"),
        }
    }
}
