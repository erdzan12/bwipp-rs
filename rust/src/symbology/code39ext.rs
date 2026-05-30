//! Code 39 Full ASCII (a.k.a. Code 39 Extended, `code39ext`).
//!
//! Extends the 43-character Code 39 alphabet to all 128 ASCII characters by
//! encoding each non-base character as a **pair** of base Code 39 characters
//! (one of `$`, `%`, `/`, `+` followed by an uppercase letter or digit).
//!
//! The translation table is the one used by BWIPP `code39ext` and is
//! documented in ISO/IEC 16388.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// Map ASCII -> the Code 39 string that encodes it.
/// Index = ASCII codepoint (0..=127). Each entry is at most 2 characters.
const ASCII_TO_CODE39: &[&str] = &[
    "%U", "$A", "$B", "$C", "$D", "$E", "$F", "$G", "$H", "$I", // 0..9
    "$J", "$K", "$L", "$M", "$N", "$O", "$P", "$Q", "$R", "$S", // 10..19
    "$T", "$U", "$V", "$W", "$X", "$Y", "$Z", "%A", "%B", "%C", // 20..29
    "%D", "%E", // 30..31
    " ", "/A", "/B", "/C", "/D", "/E", "/F", "/G", "/H", "/I", // 32..41 (' '..)
    "/J", "/K", "/L", "-", ".", "/O", // 42..47
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", // 48..57
    "/Z", "%F", "%G", "%H", "%I", "%J", // 58..63
    "%V", // 64 '@'
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", // 65..74
    "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", // 75..84
    "U", "V", "W", "X", "Y", "Z", // 85..90
    "%K", "%L", "%M", "%N", "%O", // 91..95
    "%W", // 96 '`'
    "+A", "+B", "+C", "+D", "+E", "+F", "+G", "+H", "+I", "+J", // 97..106
    "+K", "+L", "+M", "+N", "+O", "+P", "+Q", "+R", "+S", "+T", // 107..116
    "+U", "+V", "+W", "+X", "+Y", "+Z", // 117..122
    "%P", "%Q", "%R", "%S", "%T", // 123..127
];

/// Encode a Code 39 Full ASCII payload. Each ASCII character is translated
/// via the BWIPP-compatible shift table and the resulting Code 39 string is
/// then encoded with the regular Code 39 encoder.
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Code 39 Full ASCII payload must not be empty".into(),
        ));
    }
    let mut translated = String::with_capacity(data.len() * 2);
    for c in data.chars() {
        let codepoint = c as u32;
        if codepoint > 127 {
            return Err(Error::InvalidData(format!(
                "Code 39 Full ASCII only supports ASCII; got {c:?}"
            )));
        }
        translated.push_str(ASCII_TO_CODE39[codepoint as usize]);
    }
    super::code39::encode(&translated, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_lowercase() {
        // 'a' -> "+A", 'b' -> "+B", ..., 'z' -> "+Z"
        for (i, c) in ('a'..='z').enumerate() {
            let expected = format!("+{}", (b'A' + i as u8) as char);
            assert_eq!(ASCII_TO_CODE39[c as usize], expected);
        }
    }

    #[test]
    fn translates_control_characters() {
        // NUL is `%U`, SOH..SUB (1..=26) follow `$` + letter (A..Z).
        // ESC..US (27..=31) follow `%` + letter (A..E).
        assert_eq!(ASCII_TO_CODE39[0], "%U");
        assert_eq!(ASCII_TO_CODE39[1], "$A");
        assert_eq!(ASCII_TO_CODE39[26], "$Z");
        assert_eq!(ASCII_TO_CODE39[27], "%A");
    }

    #[test]
    fn translates_at_sign_and_backtick() {
        // '@' -> "%V"; '`' -> "%W"
        assert_eq!(ASCII_TO_CODE39['@' as usize], "%V");
        assert_eq!(ASCII_TO_CODE39['`' as usize], "%W");
    }

    #[test]
    fn round_trip_simple_ascii_payload() {
        let p = encode("Hello", &Options::default()).unwrap();
        // Translation: 'H' (base) + 'e' -> "+E" + 'l' -> "+L" + 'l' -> "+L" + 'o' -> "+O"
        // So the inner string is "H+E+L+L+O" — encoded as Code 39.
        let direct = super::super::code39::encode("H+E+L+L+O", &Options::default()).unwrap();
        assert_eq!(p.bars, direct.bars);
    }

    #[test]
    fn rejects_non_ascii() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 50-51 of code39ext.rs:
        //   1. `Code 39 Full ASCII` symbology prefix
        //   2. `only supports ASCII` predicate (discriminates from
        //      the `must not be empty` sibling at line 42-43)
        //   3. `'é'` Debug echo of the offending char (first >127
        //      char in "café"; 'c'/'a'/'f' are all valid ASCII)
        match encode("café", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 39 Full ASCII"),
                    "missing `Code 39 Full ASCII` prefix: {msg}"
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
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 42-43 of code39ext.rs:
        //   1. `Code 39 Full ASCII` symbology prefix
        //   2. `payload must not be empty` predicate
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 39 Full ASCII"),
                    "missing `Code 39 Full ASCII` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload must not be empty"),
                    "missing `payload must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("only supports ASCII"),
                    "wrong arm — ASCII diagnostic leaked into empty-payload reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Byte-for-byte sbs cross-validation against
    /// `b.raw("code39ext", text, {})[0].sbs`. Each golden exercises a
    /// different region of the ASCII translation table: uppercase
    /// `H` (base, no escape), lowercase letters (`+`-escape),
    /// control-ish punctuation `!` (`/`-escape), digits, the SPACE
    /// special case, and the comma (`/L`).
    #[test]
    fn sbs_matches_bwipp() {
        let cases: &[(&str, &[u8])] = &[
            (
                "Hello",
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3,
                    1, 3, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1,
                    3, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3,
                    3, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1,
                    3, 1, 3, 1, 1, 1,
                ],
            ),
            (
                "abc",
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3,
                    1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3,
                    1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1,
                    1, 1,
                ],
            ),
            (
                "123 ABC",
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1,
                    1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 3, 1,
                    1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 3, 1, 1, 3, 1, 1,
                    1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
                ],
            ),
            (
                "abc!",
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3,
                    1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3,
                    1, 1, 1, 3, 1, 3, 1, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 3,
                    1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`
            // with per-iteration input echo + path label.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (Code 39 Full ASCII sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars, want,
                "Code 39 Full ASCII sbs mismatch for {text:?}"
            );
        }
    }

    /// Kills `encode: replace > with >=` at line ~49 (the ASCII-range
    /// guard `codepoint > 127`). The mutant `>= 127` rejects the DEL
    /// character (codepoint 127) — but DEL is a valid Full-ASCII
    /// input (translates to `%T` in BWIPP). We pin codepoint 127
    /// (DEL) explicitly so the boundary is locked.
    #[test]
    fn accepts_del_codepoint_127() {
        // DEL = 0x7F = 127; ASCII_TO_CODE39[127] = "%T".
        let del = "\x7f";
        let p = encode(del, &Options::default()).unwrap();
        let direct = super::super::code39::encode("%T", &Options::default()).unwrap();
        assert_eq!(
            p.bars, direct.bars,
            "encode(\"\\x7f\") must equal encode(\"%T\") — \
             the DEL (127) boundary is part of Full-ASCII"
        );
    }
}
