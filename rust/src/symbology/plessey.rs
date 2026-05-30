//! Plessey.
//!
//! Hexadecimal payload (0-9, A-F). Each character is encoded as 4 bits LSB
//! first; each bit becomes a (wide-bar, narrow-space) for `1` or
//! (narrow-bar, wide-space) for `0`. The symbol layout is:
//!
//!   start (8 runs) + barlen × digit (8 runs each)
//!     + checksum1 (8 runs) + checksum2 (8 runs) + terminator (9 runs)
//!
//! The two checksum digits are derived from a CRC-8 over the data bits
//! using BWIPP's polynomial salt `[1, 1, 1, 1, 0, 1, 0, 0, 1]`.
//! Patterns + CRC algorithm ported from bwip-js `bwipp_plessey`. The ratio
//! is 1:3 for bars and 2:4 for spaces (BWIPP's `1, 2, 3, 4` widths).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// Per-character run-length pattern. 8 elements alternating bar/space:
///   `1` = narrow bar (width 1), `3` = wide bar (width 3),
///   `2` = narrow space (width 2), `4` = wide space (width 4).
///
/// Index 0..=15 maps to '0'..='F' (BCD bits). Index 16 is the start
/// pattern; index 17 is the bidirectional terminator BWIPP uses after
/// the two CRC digits.
const PATTERNS: [&str; 18] = [
    "14141414",  // 0 = 0000
    "32141414",  // 1 = 0001 (bit 0 set)
    "14321414",  // 2 = 0010
    "32321414",  // 3 = 0011
    "14143214",  // 4 = 0100
    "32143214",  // 5 = 0101
    "14323214",  // 6 = 0110
    "32323214",  // 7 = 0111
    "14141432",  // 8 = 1000
    "32141432",  // 9 = 1001
    "14321432",  // A = 1010
    "32321432",  // B = 1011
    "14143232",  // C = 1100
    "32143232",  // D = 1101
    "14323232",  // E = 1110
    "32323232",  // F = 1111
    "32321432",  // 16: start pattern
    "541412323", // 17: terminator (bidirectional default)
];

const ALPHABET: &str = "0123456789ABCDEF";

const START_IDX: usize = 16;
const TERM_IDX: usize = 17;

/// BWIPP's CRC polynomial salt for Plessey. Each `1` bit in the data
/// stream XORs this 9-bit pattern into positions `[i..i+9]` of the
/// extended `checkbits` array.
const CHECKSALT: [u8; 9] = [1, 1, 1, 1, 0, 1, 0, 0, 1];

/// Encode a Plessey payload (hex digits 0-9, A-F).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Plessey supports hex characters; the encoder appends the CRC check.
/// let svg = render_svg(Symbology::Plessey, "01234ABCD", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData(
            "Plessey payload must not be empty".into(),
        ));
    }
    for c in payload.chars() {
        if !ALPHABET.contains(c) {
            return Err(Error::InvalidData(format!(
                "Plessey: invalid character {c:?} (allowed: 0-9, A-F)"
            )));
        }
    }

    let digits: Vec<u8> = payload
        .chars()
        .map(|c| ALPHABET.find(c).unwrap() as u8)
        .collect();
    let (checksum1, checksum2) = checksum_pair(&digits);

    let mut runs: Vec<u8> = Vec::new();
    push_pattern(&mut runs, PATTERNS[START_IDX]);
    for &d in &digits {
        push_pattern(&mut runs, PATTERNS[d as usize]);
    }
    push_pattern(&mut runs, PATTERNS[checksum1 as usize]);
    push_pattern(&mut runs, PATTERNS[checksum2 as usize]);
    push_pattern(&mut runs, PATTERNS[TERM_IDX]);

    let text = if opts.include_text {
        Some(payload)
    } else {
        None
    };
    Ok(LinearPattern { bars: runs, text })
}

/// Compute the two Plessey CRC nibbles (checksum1 = low 4 bits,
/// checksum2 = high 4 bits) over an array of hex-digit values 0..=15.
/// Mirrors BWIPP's CRC-8 with polynomial salt `[1,1,1,1,0,1,0,0,1]`.
fn checksum_pair(digits: &[u8]) -> (u8, u8) {
    let n_data_bits = digits.len() * 4;
    let mut bits = vec![0u8; n_data_bits + 8];
    for (i, &d) in digits.iter().enumerate() {
        bits[i * 4] = d & 1;
        bits[i * 4 + 1] = (d >> 1) & 1;
        bits[i * 4 + 2] = (d >> 2) & 1;
        bits[i * 4 + 3] = (d >> 3) & 1;
    }
    for i in 0..n_data_bits {
        if bits[i] == 1 {
            for (j, &s) in CHECKSALT.iter().enumerate() {
                bits[i + j] ^= s;
            }
        }
    }
    let mut checkval: u32 = 0;
    for i in 0..8 {
        checkval += (1 << i) * bits[n_data_bits + i] as u32;
    }
    ((checkval & 0x0F) as u8, ((checkval >> 4) & 0x0F) as u8)
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
    fn rejects_invalid_character() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 77-78
        // (`Plessey: invalid character 'G' (allowed: 0-9, A-F)`).
        // 'G' is outside the hex alphabet. Cross-arm guard against
        // the empty arm.
        match encode("12G", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Plessey:"), "missing `Plessey:` prefix: {msg}");
                assert!(
                    msg.contains("invalid character"),
                    "missing `invalid character` predicate: {msg}"
                );
                assert!(msg.contains("'G'"), "missing 'G' char Debug echo: {msg}");
                assert!(
                    msg.contains("0-9, A-F"),
                    "missing `0-9, A-F` allowed-alphabet hint: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
            }
            other => panic!("`12G` should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 71-72
        // (`Plessey payload must not be empty`). Cross-arm guard
        // against the invalid-character arm.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Plessey"), "missing `Plessey` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-char diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Plessey payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn lowercase_accepted() {
        let p1 = encode("ABCD", &Options::default()).unwrap();
        let p2 = encode("abcd", &Options::default()).unwrap();
        assert_eq!(p1.bars, p2.bars);
    }

    #[test]
    fn digit_zero_produces_expected_runs() {
        // Pattern for '0' is "14141414" (BCD 0000 = all-zero bits).
        // For a 1-digit payload, layout is:
        //   start (8) + digit (8) + check1 (8) + check2 (8) + term (9) = 41.
        let p = encode("0", &Options::default()).unwrap();
        assert_eq!(p.bars.len(), 8 + 8 + 8 + 8 + 9);
    }

    /// Golden bar pattern for `"DEADBEEF"` captured from bwip-js's
    /// `raw("plessey", "DEADBEEF", {})[0].sbs`.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        let p = encode("DEADBEEF", &Options::default()).unwrap();
        let want: [u8; 97] = [
            3, 2, 3, 2, 1, 4, 3, 2, 3, 2, 1, 4, 3, 2, 3, 2, 1, 4, 3, 2, 3, 2, 3, 2, 1, 4, 3, 2, 1,
            4, 3, 2, 3, 2, 1, 4, 3, 2, 3, 2, 3, 2, 3, 2, 1, 4, 3, 2, 1, 4, 3, 2, 3, 2, 3, 2, 1, 4,
            3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 1, 4, 1, 4, 1, 4, 3, 2, 3, 2, 3, 2, 1, 4, 3,
            2, 5, 4, 1, 4, 1, 2, 3, 2, 3,
        ];
        assert_eq!(p.bars, want, "plessey bars mismatch vs bwip-js raw output");
    }

    /// Kills `checksum_pair: replace & with |` at line ~116 (the bit-3
    /// extraction `(d >> 3) & 1`). The existing `matches_bwip_js_raw_
    /// sbs` row uses `"DEADBEEF"` where every hex digit is ≥ 8, so
    /// `(d >> 3)` always evaluates to 1 — the mutant `(d >> 3) | 1`
    /// produces the same 1 and the bar sequence doesn't change. This
    /// test uses `"01234567"`, where every digit is < 8 and
    /// `(d >> 3)` is 0 — the mutant flips bit 3 to 1 for every digit
    /// and the bar sequence diverges.
    #[test]
    fn bit_three_extraction_handles_digits_below_eight() {
        let p = encode("01234567", &Options::default()).unwrap();
        // Length is deterministic: 8 (start) + 8 × 8 (digits) +
        // 8 (check1) + 8 (check2) + 9 (term) = 97.
        assert_eq!(p.bars.len(), 97);
        // Anchor the full 97-bar sequence. Pinning only the data
        // prefix (start + 8 digits) was insufficient: the mutant
        // `(d >> 3) & 1 -> (d >> 3) | 1` flips bit-3 to 1 for every
        // digit in 0..=7, but `(d >> 3) | 1` doesn't change the
        // *digit-pattern* emission (which is PATTERNS[d as usize]
        // verbatim) — it changes the CRC nibbles fed into
        // PATTERNS[checksum1/checksum2]. The mutant therefore only
        // affects the trailing 16 bars (checksum1 + checksum2).
        // Pinning the full 97-byte sequence catches the mutation;
        // the corpus uses digits 0..=7 so every digit hits the
        // `(d >> 3) = 0` branch and the mutant has a real effect.
        // Section layout for the 97-byte pin:
        //   bytes  0..  8 → start sentinel    (PATTERNS[16] = "32321432")
        //   bytes  8.. 16 → digit '0'         (PATTERNS[0]  = "14141414")
        //   bytes 16.. 24 → digit '1'         (PATTERNS[1]  = "32141414")
        //   bytes 24.. 32 → digit '2'         (PATTERNS[2]  = "14321414")
        //   bytes 32.. 40 → digit '3'         (PATTERNS[3]  = "32321414")
        //   bytes 40.. 48 → digit '4'         (PATTERNS[4]  = "14143214")
        //   bytes 48.. 56 → digit '5'         (PATTERNS[5]  = "32143214")
        //   bytes 56.. 64 → digit '6'         (PATTERNS[6]  = "14323214")
        //   bytes 64.. 72 → digit '7'         (PATTERNS[7]  = "32323214")
        //   bytes 72.. 80 → checksum1 nibble  (CRC low)
        //   bytes 80.. 88 → checksum2 nibble  (CRC high)
        //   bytes 88.. 97 → terminator        (PATTERNS[17] = "541412323")
        #[rustfmt::skip]
        let want: [u8; 97] = [
            3, 2, 3, 2, 1, 4, 3, 2,
            1, 4, 1, 4, 1, 4, 1, 4,
            3, 2, 1, 4, 1, 4, 1, 4,
            1, 4, 3, 2, 1, 4, 1, 4,
            3, 2, 3, 2, 1, 4, 1, 4,
            1, 4, 1, 4, 3, 2, 1, 4,
            3, 2, 1, 4, 3, 2, 1, 4,
            1, 4, 3, 2, 3, 2, 1, 4,
            3, 2, 3, 2, 3, 2, 1, 4,
            1, 4, 1, 4, 1, 4, 1, 4,
            1, 4, 3, 2, 1, 4, 3, 2,
            5, 4, 1, 4, 1, 2, 3, 2, 3,
        ];
        assert_eq!(
            p.bars, want,
            "plessey('01234567') bars regressed; \
             the `(d >> 3) & 1` bit-3 extraction at line ~116 may have flipped \
             to `| 1` — the mutant changes the CRC nibbles, so the trailing 16 \
             bars (checksum1 + checksum2) of the symbol diverge"
        );
    }

    /// Stage 11.A8c — pin `push_pattern`. Decodes each char as a base-10
    /// digit and appends it as a `u8`. Anchors:
    ///
    ///   * empty string → no change.
    ///   * single digit "5" → push 5.
    ///   * multi-digit "31415" → walks chars in order, each mapped via
    ///     `to_digit(10).unwrap()`.
    ///   * appends to the existing buffer (not replace).
    ///
    /// Kills `replace push_pattern with ()`, `replace base 10 with
    /// base 2` mutations on the `to_digit` arg, and order-flip mutants
    /// like `chars().rev()` in the iterator.
    #[test]
    fn push_pattern_walks_chars_left_to_right_in_base_10() {
        let mut out: Vec<u8> = Vec::new();
        push_pattern(&mut out, "");
        assert!(out.is_empty(), "empty pattern leaves out empty");

        push_pattern(&mut out, "5");
        assert_eq!(out, vec![5], "single digit pushed as-is");

        push_pattern(&mut out, "31415");
        assert_eq!(
            out,
            vec![5, 3, 1, 4, 1, 5],
            "multi-digit appends in chars() order to existing buffer"
        );

        // Boundary: '9' → 9 (top of base-10).
        let mut out2: Vec<u8> = Vec::new();
        push_pattern(&mut out2, "9");
        assert_eq!(out2[0], 9, "'9' decodes to 9 (rules out base 2/8 mutant)");

        // Boundary: '0' → 0 (bottom of base-10, distinct from `c as u8`
        // which would give 0x30).
        let mut out3: Vec<u8> = Vec::new();
        push_pattern(&mut out3, "0");
        assert_eq!(
            out3[0], 0,
            "'0' decodes to 0 (rules out `c as u8` replacement)"
        );
    }
}
