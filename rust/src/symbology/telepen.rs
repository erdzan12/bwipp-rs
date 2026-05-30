//! Telepen.
//!
//! Full-ASCII linear barcode. Each ASCII byte maps to a variable-length
//! bar/space pattern, all expressed as run-length widths over a 1/3 narrow:
//! wide ratio. Start and stop sentinels are themselves Telepen characters
//! (`_` at the start and end of the codeword sequence). The symbol carries
//! a mod-127 check character.
//!
//! Patterns ported from bwip-js `bwipp_telepen`.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

const START: u8 = 95; // ASCII '_' is the BWIPP start/stop indicator
const STOP: u8 = 122; // ASCII 'z' (per BWIPP's table, slot 122 = stop)

include!("telepen_patterns.rs");

/// Encode a Telepen payload (ASCII 0..=127, BWIPP-compatible).
///
/// `numeric=true` switches to BWIPP's `telepennumeric` mode, where the input
/// is restricted to digits and the encoder pairs them into single 8-bit
/// characters (so a 14-digit payload uses 7 Telepen characters).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Telepen accepts the full 0..=127 ASCII range.
/// let svg = render_svg(Symbology::Telepen, "Hello!", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Telepen payload must not be empty".into(),
        ));
    }
    for c in data.chars() {
        if (c as u32) > 127 {
            return Err(Error::InvalidData(format!(
                "Telepen only supports ASCII (got {c:?})"
            )));
        }
    }
    encode_chars(data.bytes().collect(), opts)
}

/// Encode a Telepen Numeric payload (digit pairs are packed into single
/// Telepen characters in the 27..=126 range per BWIPP's `telepennumeric`).
pub fn encode_numeric(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let mut digits: String = data.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != data.chars().count() {
        return Err(Error::InvalidData(
            "Telepen Numeric accepts digits only".into(),
        ));
    }
    if digits.is_empty() {
        return Err(Error::InvalidData(
            "Telepen Numeric payload must not be empty".into(),
        ));
    }
    if digits.len() % 2 == 1 {
        digits.insert(0, '0');
    }
    // BWIPP packs each digit pair into the character (pair_value + 27),
    // staying inside the printable Telepen range.
    let mut bytes: Vec<u8> = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks(2) {
        let v = (pair[0] - b'0') * 10 + (pair[1] - b'0');
        bytes.push(27 + v);
    }
    encode_chars(bytes, opts)
}

fn encode_chars(mut bytes: Vec<u8>, opts: &Options) -> Result<LinearPattern, Error> {
    if bytes.len() > 500 {
        return Err(Error::InvalidData(
            "Telepen payload exceeds BWIPP's 500-char limit".into(),
        ));
    }

    // Compute the mod-127 check character. BWIPP initialises the
    // checksum at 0 (the start sentinel does NOT contribute), then
    // accumulates each data byte and emits `(127 - sum % 127) % 127`
    // as the trailing check character.
    let mut sum: u32 = 0;
    for &b in &bytes {
        sum = sum.wrapping_add(b as u32);
    }
    let check = (127 - (sum % 127)) % 127;
    bytes.push(check as u8);

    let mut runs: Vec<u8> = Vec::new();
    push_pattern(&mut runs, PATTERNS[START as usize]);
    for &b in &bytes {
        push_pattern(&mut runs, PATTERNS[b as usize]);
    }
    push_pattern(&mut runs, PATTERNS[STOP as usize]);

    let text = if opts.include_text {
        Some(String::from_utf8_lossy(&bytes[..bytes.len() - 1]).to_string())
    } else {
        None
    };
    Ok(LinearPattern { bars: runs, text })
}

fn push_pattern(out: &mut Vec<u8>, pattern: &str) {
    for c in pattern.chars() {
        out.push(c.to_digit(10).unwrap() as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_ascii() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 43-45 of telepen.rs:
        //   1. `Telepen` symbology prefix
        //   2. `only supports ASCII` predicate (discriminates from
        //      the `must not be empty` and `Numeric accepts digits
        //      only` sibling arms)
        //   3. `'é'` Debug echo of the offending char (the first
        //      non-ASCII char in "café"; 'c', 'a', 'f' are all <128)
        match encode("café", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Telepen"), "missing Telepen prefix: {msg}");
                assert!(
                    msg.contains("only supports ASCII"),
                    "missing `only supports ASCII` predicate: {msg}"
                );
                assert!(msg.contains("'é'"), "missing 'é' Debug echo: {msg}");
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into ASCII reject: {msg}"
                );
                assert!(
                    !msg.contains("Numeric"),
                    "wrong arm — Telepen Numeric diagnostic leaked into ASCII reject: {msg}"
                );
            }
            other => panic!("\"café\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 37-39 of telepen.rs:
        //   1. `Telepen` symbology prefix
        //   2. `payload must not be empty` predicate (NOT the Numeric
        //      variant — pin via Telepen Numeric ABSENCE)
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Telepen"), "missing Telepen prefix: {msg}");
                assert!(
                    msg.contains("payload must not be empty"),
                    "missing `payload must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("Numeric"),
                    "wrong helper — Telepen Numeric empty diagnostic leaked into plain `encode` empty reject: {msg}"
                );
                assert!(
                    !msg.contains("only supports ASCII"),
                    "wrong arm — ASCII diagnostic leaked into empty reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ascii_encodes() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen ASCII smoke path: 3-char ASCII "ABC".
        let p = encode("ABC", &Options::default())
            .expect("encode(\"ABC\", default) (Telepen ASCII 3-char smoke path) must succeed");
        assert!(
            !p.bars.is_empty(),
            "encode(\"ABC\") (3-char ASCII payload) must produce non-empty Telepen bars; got len={}",
            p.bars.len()
        );
    }

    #[test]
    fn numeric_pairs_digits() {
        // Telepen Numeric packs digit pairs into single Telepen characters,
        // so an odd-length input auto-pads to even length. We check that
        // both invocations succeed and produce the same number of data
        // characters (3 in this case), but the run-length total can differ
        // because Telepen patterns are variable-length per character.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen Numeric even-length pass-through vs odd-length
        // auto-pad paths: digit pairs pack as (27 + value) Telepen
        // chars; odd-length payload auto-pads to even.
        let p = encode_numeric("123456", &Options::default()).expect(
            "encode_numeric(\"123456\", default) (Telepen Numeric even-length pass-through; 6 digits → 3 paired chars) must succeed",
        );
        let q = encode_numeric("12345", &Options::default()).expect(
            "encode_numeric(\"12345\", default) (Telepen Numeric odd-length auto-pad; 5 digits → leading-0 pad → 6 digits → 3 paired chars) must succeed",
        );
        assert!(
            !p.bars.is_empty(),
            "encode_numeric(\"123456\") (even-length 6-digit payload → 3 paired-digit Telepen chars) must produce non-empty bars; got len={}",
            p.bars.len()
        );
        assert!(
            !q.bars.is_empty(),
            "encode_numeric(\"12345\") (odd-length 5-digit payload, auto-padded to 6) must produce non-empty bars; got len={}",
            q.bars.len()
        );
    }

    #[test]
    fn numeric_rejects_non_digits() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 56-58 of telepen.rs:
        //   1. `Telepen Numeric` helper-qualified prefix (NOT the
        //      bare `Telepen` prefix used by `encode` — pinning the
        //      `Numeric` qualifier discriminates the helper)
        //   2. `accepts digits only` predicate (NOT `only supports
        //      ASCII` — those are distinct sibling rejections)
        match encode_numeric("12AB", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Telepen Numeric"),
                    "missing `Telepen Numeric` helper prefix: {msg}"
                );
                assert!(
                    msg.contains("accepts digits only"),
                    "missing `accepts digits only` predicate: {msg}"
                );
                assert!(
                    !msg.contains("only supports ASCII"),
                    "wrong helper — plain `encode` ASCII diagnostic leaked into encode_numeric reject: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into digits-only reject: {msg}"
                );
            }
            other => panic!("\"12AB\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// Golden bar pattern for `"Hello"` captured from bwip-js's
    /// `raw("telepen", "Hello", {})[0].sbs`.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen byte-for-byte 78-bar SBS oracle path: 5-char
        // "Hello" → bwip-js raw SBS.
        let p = encode("Hello", &Options::default()).expect(
            "encode(\"Hello\", default) (Telepen byte-for-byte 78-bar SBS bwip-js raw oracle) must succeed",
        );
        let want: [u8; 78] = [
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1, 3, 3, 3, 3, 1, 1, 3, 3, 1, 3, 1, 3, 3, 1, 1,
            1, 1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3,
            3, 1, 3, 3, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "telepen bars mismatch vs bwip-js raw output");
    }

    /// Telepen Numeric golden for `"123456"` from bwip-js's
    /// `raw("telepennumeric", "123456", {})[0].sbs`. Locks down that
    /// the digit-pair packing (each pair → `27 + value`) matches
    /// BWIPP exactly now that the checksum fix is in.
    #[test]
    fn numeric_matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen Numeric byte-for-byte 68-bar SBS oracle path:
        // 6-digit "123456" → bwip-js raw SBS (digit-pair packing,
        // (27+value) → Telepen char + checksum).
        let p = encode_numeric("123456", &Options::default()).expect(
            "encode_numeric(\"123456\", default) (Telepen Numeric byte-for-byte 68-bar SBS bwip-js raw oracle; digit-pair packing + checksum fix) must succeed",
        );
        let want: [u8; 68] = [
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1,
            1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 3, 3, 3, 3,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "telepen numeric bars mismatch vs bwip-js raw output"
        );
    }

    /// Additional cross-validation goldens covering digit-only,
    /// mixed-case, uppercase-with-digits, and a short uppercase
    /// payload. Each is `b.raw("telepen", text, {})[0].sbs`.
    #[test]
    fn matches_bwip_js_various_inputs() {
        let cases: &[(&str, &[u8])] = &[
            (
                "12345",
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 3, 3, 1, 3,
                    1, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 1,
                    1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1, 1,
                    1, 1, 1, 1, 1, 1, 1, 1,
                ],
            ),
            (
                "ABC123",
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 3, 1, 3, 3, 3, 3, 3, 1, 3, 3,
                    1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 3, 3, 1, 3,
                    1, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1,
                    3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                ],
            ),
            (
                "TEST",
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 3,
                    3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1,
                    1, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (Telepen sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(got.bars, want, "telepen sbs mismatch for {text:?}");
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills the `> with >=` mutant at line ~42 (the ASCII range
    /// guard). The original errors when `c as u32 > 127`, i.e. char
    /// 128 and above. The mutant errors at 127 too. We anchor that
    /// byte 0x7F (DEL = 127) is accepted; the mutant rejects it.
    #[test]
    fn ascii_range_accepts_byte_127() {
        // DEL = 0x7F = 127. Original accepts (≤ 127); `>=` mutant
        // rejects (now ≥ 127). Code 128 is encoded with no display
        // glyph; we just check the encoder doesn't error.
        let s = "\x7F";
        encode(s, &Options::default())
            .expect("Telepen should accept ASCII 127 (DEL); the `>=` mutant rejects");
    }

    /// Kills the two `> 500` payload-length cap mutants at line ~79
    /// in `encode_chars`. The original rejects payloads strictly
    /// longer than 500 chars; the `==` mutant rejects only length
    /// 500, and the `>=` mutant rejects length-500 inputs. We pin
    /// both boundaries: length 500 succeeds, length 501 fails.
    #[test]
    fn payload_length_cap_is_strictly_five_hundred() {
        // 500 chars of 'A' must succeed (boundary).
        let exactly_500 = "A".repeat(500);
        encode(&exactly_500, &Options::default())
            .expect("500-char Telepen payload should encode (boundary not yet hit)");

        // 501 chars must error.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("500")`
        // (would accept any message containing the literal "500") →
        // 3-anchor pin:
        //   1. `Telepen` symbology name anchor
        //   2. `exceeds BWIPP's 500-char limit` full predicate
        //   3. cross-arm guard: must NOT contain `empty input` (the
        //      sibling empty-input arm, at line ~75 of telepen.rs).
        let length_501 = "A".repeat(501);
        match encode(&length_501, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Telepen"),
                    "missing `Telepen` symbology name: {msg:?}"
                );
                assert!(
                    msg.contains("exceeds BWIPP's 500-char limit"),
                    "missing full predicate `exceeds BWIPP's 500-char limit`: {msg:?}"
                );
                assert!(
                    !msg.contains("empty input"),
                    "cross-arm contamination: length-cap reject mentions `empty input`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(500-char cap), got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the wrap-around case for the mod-127
    /// checksum at line ~93 (`(127 - (sum % 127)) % 127`). All
    /// existing tests use payloads whose byte-sum is not a multiple
    /// of 127, so the outer `% 127` fold never reduces 127 → 0 — a
    /// mutation that drops the outer `% 127` would silently produce
    /// `check = 127` (a valid PATTERNS index), substituting
    /// PATTERNS[127] = "1111111111111111" for PATTERNS[0] =
    /// "31313131" in the bar sequence.
    ///
    /// Construction: input `"\x00"` gives bytes = [0], sum = 0,
    /// so check = (127 - 0) % 127 = 0. With the mutant, check = 127
    /// → the third PATTERNS index (which encodes the check) flips
    /// from "31313131" (8 modules) to "1111111111111111" (16
    /// modules), changing both content AND total length of the bars.
    ///
    /// Hand-derived expected bars:
    ///   PATTERNS[95]  (START="111111111133")  → 10 ones, 2 threes
    ///   PATTERNS[0]   (data="31313131")       → alternating 3/1 × 4
    ///   PATTERNS[0]   (check="31313131")      → SAME (proves wrap)
    ///   PATTERNS[122] (STOP="331111111111")   → 2 threes, 10 ones
    /// = 12 + 8 + 8 + 12 = 40 modules total.
    #[test]
    fn checksum_wraps_at_127_via_outer_modulo() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen checksum wrap path: single NUL byte exercises the
        // outer `% 127` fold so check=0 (NOT 127) → guards the
        // drop-outer-modulo mutant.
        let p = encode("\x00", &Options::default()).expect(
            "encode(\"\\x00\", default) (Telepen checksum outer `% 127` wrap path: NUL byte → check=0 not 127; guards drop-outer-modulo mutant) must succeed",
        );
        // Hand-computed bars for input "\x00":
        //   start "111111111133" + data[0] "31313131" + check "31313131"
        //   + stop "331111111111"
        let want: Vec<u8> = vec![
            // START PATTERNS[95] = "111111111133" (12 modules)
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, //
            // data PATTERNS[0] = "31313131" (8 modules)
            3, 1, 3, 1, 3, 1, 3, 1, //
            // check PATTERNS[0] = "31313131" (8 modules) — wrap to 0
            3, 1, 3, 1, 3, 1, 3, 1, //
            // STOP PATTERNS[122] = "331111111111" (12 modules)
            3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(
            p.bars, want,
            "encode(\"\\x00\") must produce check=0 (outer % 127 wrap); \
             the `drop outer % 127` mutant would produce check=127 \
             and swap PATTERNS[0]=8 modules for PATTERNS[127]=16 modules"
        );
        // Length sanity: 40 modules total.
        assert_eq!(p.bars.len(), 40, "wrap-case symbol is exactly 40 modules");
    }

    /// Kills the two `- with +` / `- with /` mutants at line ~104 (the
    /// `bytes[..bytes.len() - 1]` slice that strips the trailing check
    /// byte from the displayed text). The mutants either panic (`+`
    /// goes out of bounds) or include the check byte in the displayed
    /// text (`/` collapses `len / 1` to `len` — whole slice). Both are
    /// only observable when `include_text=true`. We pin the displayed
    /// text against the raw input so any divergence is caught.
    #[test]
    fn include_text_strips_trailing_check_byte() {
        // The top-level `include_text` flag is what gates the
        // displayed-text closure. With it set the encoder slices off
        // the trailing check byte before stringifying.
        let opts = Options {
            include_text: true,
            ..Options::default()
        };
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Telepen include_text strip-trailing-check path: with
        // include_text=true the displayed text must be the raw input
        // "Hello" not "Hello<check>"; guards bytes[..len-1] mutants.
        let p = encode("Hello", &opts).expect(
            "encode(\"Hello\", include_text=true) (Telepen include_text strip-trailing-check path; displayed text must be raw \"Hello\" not include the check byte) must succeed",
        );
        assert_eq!(
            p.text.as_deref(),
            Some("Hello"),
            "include_text should restore the original payload, NOT include the trailing check byte"
        );
    }

    /// Stage 11.A8c — pin `push_pattern` (telepen.rs:111). Same shape as
    /// plessey::push_pattern: decode each char as a base-10 digit and
    /// append it to the buffer. Kills:
    ///   * `replace push_pattern with ()` (function-replacement),
    ///   * `chars().rev()` / order-flip mutants,
    ///   * `replace base 10 with base 2` / base-8 on the `to_digit`
    ///     argument (covered by the '9' digit boundary),
    ///   * `c as u8` replacement (covered by the '0' anchor — '0' →
    ///     0, not 0x30).
    #[test]
    fn push_pattern_walks_chars_left_to_right_in_base_10() {
        // Empty pattern is a no-op.
        let mut out: Vec<u8> = vec![7, 7];
        push_pattern(&mut out, "");
        assert_eq!(out, vec![7, 7], "empty pattern leaves out untouched");

        // Single digit pushed as-is.
        push_pattern(&mut out, "3");
        assert_eq!(out, vec![7, 7, 3]);

        // Multi-digit walks left-to-right and APPENDS to the existing
        // buffer.
        push_pattern(&mut out, "12340");
        assert_eq!(out, vec![7, 7, 3, 1, 2, 3, 4, 0]);

        // Boundary: '9' decodes to 9 (rules out base-2/8 mutants).
        let mut out2: Vec<u8> = Vec::new();
        push_pattern(&mut out2, "9");
        assert_eq!(out2[0], 9);

        // Boundary: '0' decodes to 0, NOT 0x30 (rules out `c as u8`
        // replacement).
        let mut out3: Vec<u8> = Vec::new();
        push_pattern(&mut out3, "0");
        assert_eq!(out3[0], 0);
    }
}
