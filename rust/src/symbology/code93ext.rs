//! Code 93 Full ASCII (a.k.a. Code 93 Extended, `code93ext`).
//!
//! Extends Code 93's 48-character base alphabet to the full 7-bit
//! ASCII range. Each non-base character expands into **one of the
//! four `SFT1..=SFT4` shift codewords** (codeword indices 43..=46 —
//! they have no representation in the printable alphabet) followed
//! by an uppercase letter. BWIPP's `code93ext_extencs` encodes the
//! shifts as the magic strings `^SFT$`, `^SFT%`, `^SFT/`, `^SFT+`,
//! which it then resolves via the FNC-parser to the actual SFT
//! codewords; we resolve them upfront to plain codeword indices
//! since we have a lower-level `code93::encode_indices` entry
//! point.
//!
//! Reference: BWIPP `code93ext` (`bwipp_code93ext.code93ext_extencs`
//! in the 2026-03-31 vendor snapshot).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

// ----- Code 93 codeword indices used by this module ------------------------
//
// These mirror the index positions in `code93::PATTERNS`. They live here as
// short constants so the translation table below reads naturally.

const C0: u32 = 0; // '0'..'9' live at indices 0..=9
const CA: u32 = 10; // 'A'..'Z' live at indices 10..=35
const DASH: u32 = 36;
const DOT: u32 = 37;
const SPC: u32 = 38;
const DOLLAR: u32 = 39;
const SLASH: u32 = 40;
const PLUS: u32 = 41;
const PERCENT: u32 = 42;
const SFT1: u32 = 43; // ^SFT$
const SFT2: u32 = 44; // ^SFT%
const SFT3: u32 = 45; // ^SFT/
const SFT4: u32 = 46; // ^SFT+

const fn digit(d: u8) -> u32 {
    C0 + d as u32
}

const fn letter(l: u8) -> u32 {
    CA + (l - b'A') as u32
}

/// Map ASCII (0..=127) to the sequence of Code 93 codeword indices that
/// encodes it. Each entry is 1 or 2 indices. Indices are positions in
/// [`super::code93::PATTERNS`].
#[rustfmt::skip]
const ASCII_TO_CODE93: &[&[u32]] = &[
    // 0..=31: control codes — every one is a 2-codeword shift escape.
    &[SFT2, letter(b'U')], &[SFT1, letter(b'A')], &[SFT1, letter(b'B')], &[SFT1, letter(b'C')],
    &[SFT1, letter(b'D')], &[SFT1, letter(b'E')], &[SFT1, letter(b'F')], &[SFT1, letter(b'G')],
    &[SFT1, letter(b'H')], &[SFT1, letter(b'I')], &[SFT1, letter(b'J')], &[SFT1, letter(b'K')],
    &[SFT1, letter(b'L')], &[SFT1, letter(b'M')], &[SFT1, letter(b'N')], &[SFT1, letter(b'O')],
    &[SFT1, letter(b'P')], &[SFT1, letter(b'Q')], &[SFT1, letter(b'R')], &[SFT1, letter(b'S')],
    &[SFT1, letter(b'T')], &[SFT1, letter(b'U')], &[SFT1, letter(b'V')], &[SFT1, letter(b'W')],
    &[SFT1, letter(b'X')], &[SFT1, letter(b'Y')], &[SFT1, letter(b'Z')], &[SFT2, letter(b'A')],
    &[SFT2, letter(b'B')], &[SFT2, letter(b'C')], &[SFT2, letter(b'D')], &[SFT2, letter(b'E')],

    // 32 ' '  — base
    &[SPC],
    // 33..=35: !, ", # → SFT3 + A/B/C
    &[SFT3, letter(b'A')], &[SFT3, letter(b'B')], &[SFT3, letter(b'C')],
    // 36 '$', 37 '%'  — base
    &[DOLLAR], &[PERCENT],
    // 38..=42: &, ', (, ), * → SFT3 + F/G/H/I/J
    &[SFT3, letter(b'F')], &[SFT3, letter(b'G')], &[SFT3, letter(b'H')], &[SFT3, letter(b'I')],
    &[SFT3, letter(b'J')],
    // 43 '+'  — base
    &[PLUS],
    // 44 ',' → SFT3 + L. 45 '-', 46 '.' — base. 47 '/' — base.
    &[SFT3, letter(b'L')], &[DASH], &[DOT], &[SLASH],

    // 48..=57: digits — base
    &[digit(0)], &[digit(1)], &[digit(2)], &[digit(3)], &[digit(4)],
    &[digit(5)], &[digit(6)], &[digit(7)], &[digit(8)], &[digit(9)],

    // 58..=63: :, ;, <, =, >, ? → SFT3 + Z, SFT2 + F/G/H/I/J
    &[SFT3, letter(b'Z')], &[SFT2, letter(b'F')], &[SFT2, letter(b'G')],
    &[SFT2, letter(b'H')], &[SFT2, letter(b'I')], &[SFT2, letter(b'J')],
    // 64 '@' → SFT2 + V
    &[SFT2, letter(b'V')],

    // 65..=90: A..Z — base
    &[letter(b'A')], &[letter(b'B')], &[letter(b'C')], &[letter(b'D')], &[letter(b'E')],
    &[letter(b'F')], &[letter(b'G')], &[letter(b'H')], &[letter(b'I')], &[letter(b'J')],
    &[letter(b'K')], &[letter(b'L')], &[letter(b'M')], &[letter(b'N')], &[letter(b'O')],
    &[letter(b'P')], &[letter(b'Q')], &[letter(b'R')], &[letter(b'S')], &[letter(b'T')],
    &[letter(b'U')], &[letter(b'V')], &[letter(b'W')], &[letter(b'X')], &[letter(b'Y')],
    &[letter(b'Z')],

    // 91..=95: [, \, ], ^, _ → SFT2 + K/L/M/N/O
    &[SFT2, letter(b'K')], &[SFT2, letter(b'L')], &[SFT2, letter(b'M')],
    &[SFT2, letter(b'N')], &[SFT2, letter(b'O')],
    // 96 '`' → SFT2 + W
    &[SFT2, letter(b'W')],

    // 97..=122: a..z → SFT4 + A..Z
    &[SFT4, letter(b'A')], &[SFT4, letter(b'B')], &[SFT4, letter(b'C')], &[SFT4, letter(b'D')],
    &[SFT4, letter(b'E')], &[SFT4, letter(b'F')], &[SFT4, letter(b'G')], &[SFT4, letter(b'H')],
    &[SFT4, letter(b'I')], &[SFT4, letter(b'J')], &[SFT4, letter(b'K')], &[SFT4, letter(b'L')],
    &[SFT4, letter(b'M')], &[SFT4, letter(b'N')], &[SFT4, letter(b'O')], &[SFT4, letter(b'P')],
    &[SFT4, letter(b'Q')], &[SFT4, letter(b'R')], &[SFT4, letter(b'S')], &[SFT4, letter(b'T')],
    &[SFT4, letter(b'U')], &[SFT4, letter(b'V')], &[SFT4, letter(b'W')], &[SFT4, letter(b'X')],
    &[SFT4, letter(b'Y')], &[SFT4, letter(b'Z')],

    // 123..=127: {, |, }, ~, DEL → SFT2 + P/Q/R/S/T
    &[SFT2, letter(b'P')], &[SFT2, letter(b'Q')], &[SFT2, letter(b'R')],
    &[SFT2, letter(b'S')], &[SFT2, letter(b'T')],
];

/// Encode a Code 93 Full ASCII payload. Each input character expands
/// to one or two Code 93 codeword indices (the second being the SFT
/// shift for non-base characters); we resolve those upfront and hand
/// the full index vector to `code93::encode_indices`.
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Code 93 Full ASCII payload must not be empty".into(),
        ));
    }
    let mut indices: Vec<u32> = Vec::with_capacity(data.len() * 2);
    for c in data.chars() {
        let codepoint = c as u32;
        if codepoint > 127 {
            return Err(Error::InvalidData(format!(
                "Code 93 Full ASCII only supports ASCII; got {c:?}"
            )));
        }
        indices.extend_from_slice(ASCII_TO_CODE93[codepoint as usize]);
    }
    let include_check = opts.get("includecheck").is_some_and(|v| v == "true");
    let text = if opts.include_text {
        Some(data.to_string())
    } else {
        None
    };
    super::code93::encode_indices(&indices, include_check, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin the tiny `digit(d)` and `letter(l)` const
    /// helpers used to build ASCII_TO_CODE93.
    ///
    /// digit(d) = C0 + d (C0 = 0).
    /// letter(l) = CA + (l - 'A') (CA = 10).
    ///
    /// Mutations caught:
    ///   * `C0 + d` → `C0 * d` would shift digit(0)=0 vs digit(N)=0.
    ///   * `(l - b'A')` → `(l - b'a')` shifts uppercase to lowercase
    ///     base, producing wildly wrong indices.
    ///   * Constants CA=10 vs other → letter('A')=10 sentinel.
    #[test]
    fn digit_and_letter_const_helpers_compute_correct_indices() {
        // digit(0..9) → 0..9.
        for d in 0u8..10 {
            assert_eq!(digit(d), u32::from(d), "digit({d})");
        }
        // letter('A'..'Z') → 10..35.
        for (i, c) in (b'A'..=b'Z').enumerate() {
            assert_eq!(letter(c), 10 + i as u32, "letter({c:?})");
        }
        // Boundary anchors.
        assert_eq!(digit(0), 0);
        assert_eq!(digit(9), 9);
        assert_eq!(letter(b'A'), 10);
        assert_eq!(letter(b'Z'), 35);
    }

    #[test]
    fn translates_lowercase() {
        // 'a'..'z' all expand to `SFT4 + <letter>` (codeword 46 +
        // 10..=35).
        for (i, c) in ('a'..='z').enumerate() {
            let entry = ASCII_TO_CODE93[c as usize];
            assert_eq!(entry, &[SFT4, 10 + i as u32], "lowercase {c:?}");
        }
    }

    #[test]
    fn translates_control_characters() {
        assert_eq!(ASCII_TO_CODE93[0], &[SFT2, 30]); // NUL → SFT2 + 'U'
        assert_eq!(ASCII_TO_CODE93[1], &[SFT1, 10]); // SOH → SFT1 + 'A'
        assert_eq!(ASCII_TO_CODE93[26], &[SFT1, 35]); // SUB → SFT1 + 'Z'
        assert_eq!(ASCII_TO_CODE93[27], &[SFT2, 10]); // ESC → SFT2 + 'A'
        assert_eq!(ASCII_TO_CODE93[127], &[SFT2, 29]); // DEL → SFT2 + 'T'
    }

    /// The four Code 93 base-shift characters appear as themselves
    /// (not as shift pairs like Code 39 Full ASCII would emit).
    #[test]
    fn base_alphabet_extras_are_direct() {
        assert_eq!(ASCII_TO_CODE93[b'$' as usize], &[DOLLAR]);
        assert_eq!(ASCII_TO_CODE93[b'%' as usize], &[PERCENT]);
        assert_eq!(ASCII_TO_CODE93[b'+' as usize], &[PLUS]);
        assert_eq!(ASCII_TO_CODE93[b'/' as usize], &[SLASH]);
    }

    #[test]
    fn rejects_non_ascii() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 129-131 of code93ext.rs (`Code 93 Full ASCII only supports
        // ASCII; got {c:?}`):
        //   1. `Code 93 Full ASCII` prefix
        //   2. `only supports ASCII` predicate
        //   3. `'é'` Debug echo (first >127 char in "café")
        match encode("café", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 93 Full ASCII"),
                    "missing `Code 93 Full ASCII` prefix: {msg}"
                );
                assert!(
                    msg.contains("only supports ASCII"),
                    "missing `only supports ASCII` predicate: {msg}"
                );
                assert!(msg.contains("'é'"), "missing 'é' Debug echo: {msg}");
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into non-ASCII reject: {msg}"
                );
            }
            other => panic!("\"café\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_input() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 2-anchor pin matching the source diagnostic at line
        // 121-122 of code93ext.rs (`Code 93 Full ASCII payload must
        // not be empty`):
        //   1. `Code 93 Full ASCII` prefix
        //   2. `payload must not be empty` predicate
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 93 Full ASCII"),
                    "missing `Code 93 Full ASCII` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload must not be empty"),
                    "missing `payload must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("only supports ASCII"),
                    "wrong arm — ASCII diagnostic leaked into empty reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Byte-for-byte sbs cross-validation against
    /// `b.raw("code93ext", text, {})[0].sbs`. Covers:
    ///
    ///   * mixed lowercase + uppercase + punctuation (`Hello`, `Hello!`),
    ///   * digit + letter expansion (`abc123`),
    ///   * the four shift-prefix base characters appearing literally
    ///     (`$%/+`), which is where Code 93 Full ASCII diverges from
    ///     Code 39 Full ASCII.
    #[test]
    fn sbs_matches_bwipp() {
        let cases: &[(&str, &[u8])] = &[
            (
                "Hello",
                &[
                    1, 1, 1, 1, 4, 1, 1, 1, 2, 2, 1, 2, 1, 2, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 1, 2,
                    2, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 2, 2, 2,
                    1, 1, 1, 2, 1, 1, 2, 2, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
            (
                "Hello!",
                &[
                    1, 1, 1, 1, 4, 1, 1, 1, 2, 2, 1, 2, 1, 2, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 1, 2,
                    2, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 2, 2, 2,
                    1, 1, 1, 2, 1, 1, 2, 2, 3, 1, 1, 1, 2, 1, 2, 1, 1, 1, 1, 3, 1, 1, 1, 1, 4, 1,
                    1,
                ],
            ),
            (
                "abc123",
                &[
                    1, 1, 1, 1, 4, 1, 1, 2, 2, 2, 1, 1, 2, 1, 1, 1, 1, 3, 1, 2, 2, 2, 1, 1, 2, 1,
                    1, 2, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 1, 3, 1, 1, 1, 1, 1, 2, 1, 3, 1, 1, 1, 3,
                    1, 2, 1, 1, 1, 4, 1, 1, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
            (
                "$%/+",
                &[
                    1, 1, 1, 1, 4, 1, 3, 2, 1, 1, 1, 1, 2, 1, 1, 1, 3, 1, 1, 1, 2, 1, 3, 1, 1, 1,
                    3, 1, 2, 1, 1, 1, 1, 1, 4, 1, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`
            // with per-iteration input echo.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (Code 93 Full ASCII sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars, want,
                "Code 93 Full ASCII sbs mismatch for {text:?}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills `encode: replace > with >=` at line ~128 (the ASCII-range
    /// guard `codepoint > 127`). The mutant `>= 127` rejects DEL
    /// (codepoint 127), which is a valid Full-ASCII input. We pin
    /// codepoint 127 explicitly so the boundary is locked.
    #[test]
    fn accepts_del_codepoint_127() {
        let del = "\x7f";
        // Should not error.
        encode(del, &Options::default()).unwrap();
    }

    /// Kills `encode: replace == with !=` at line ~135 (the
    /// `includecheck` option-value comparator). The existing
    /// `sbs_matches_bwipp` corpus uses only `Options::default()`, where
    /// `opts.get("includecheck") == None` so the closure never
    /// executes — the mutant survives. This test pins the output for
    /// an explicit `includecheck=true` payload so the flipped
    /// comparator changes the include_check value and the bar sequence
    /// diverges.
    #[test]
    fn includecheck_true_appends_check_codeword() {
        let with_check = encode("Hello", &Options::default().with("includecheck", "true")).unwrap();
        let without_check = encode("Hello", &Options::default()).unwrap();
        // Code 93 appends 2 check codewords (C and K) when includecheck
        // is true; each codeword expands to 6 modules of the bar
        // pattern. The with-check output must be strictly longer.
        assert!(
            with_check.bars.len() > without_check.bars.len(),
            "encode(\"Hello\", includecheck=true) ({}) should be longer than \
             encode(\"Hello\", default) ({}); the `v == \"true\"` comparator may have flipped",
            with_check.bars.len(),
            without_check.bars.len(),
        );
        // Symmetric counter-test: explicit `includecheck=false` must
        // equal the default (no check appended).
        let explicit_false =
            encode("Hello", &Options::default().with("includecheck", "false")).unwrap();
        assert_eq!(
            explicit_false.bars, without_check.bars,
            "encode(includecheck=false) should equal default; \
             the `v == \"true\"` comparator may have flipped"
        );
    }
}
