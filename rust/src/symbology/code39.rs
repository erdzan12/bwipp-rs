//! Code 39 (3 of 9 Code, USS-39).
//!
//! 43-character alphabet (digits, A-Z, space, $%+-./), 9-element bar/space
//! pattern per character (5 bars + 4 spaces alternating BSBSBSBSB), exactly
//! three "wide" elements per pattern. Bracketed by `*` start/stop characters,
//! separated by a single-module space.
//!
//! Reference: ANSI/AIM BC1-1995 and the BWIPP `code39` encoder.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

const WIDE: u8 = 3;
const NARROW: u8 = 1;

/// Bar/space pattern for each Code 39 character.
///
/// Each entry is 9 digits (`1` = narrow, `3` = wide, by convention),
/// ordered B S B S B S B S B. Start/stop is `*`.
const PATTERNS: &[(char, &[u8; 9])] = &[
    ('0', b"111331311"),
    ('1', b"311311113"),
    ('2', b"113311113"),
    ('3', b"313311111"),
    ('4', b"111331113"),
    ('5', b"311331111"),
    ('6', b"113331111"),
    ('7', b"111311313"),
    ('8', b"311311311"),
    ('9', b"113311311"),
    ('A', b"311113113"),
    ('B', b"113113113"),
    ('C', b"313113111"),
    ('D', b"111133113"),
    ('E', b"311133111"),
    ('F', b"113133111"),
    ('G', b"111113313"),
    ('H', b"311113311"),
    ('I', b"113113311"),
    ('J', b"111133311"),
    ('K', b"311111133"),
    ('L', b"113111133"),
    ('M', b"313111131"),
    ('N', b"111131133"),
    ('O', b"311131131"),
    ('P', b"113131131"),
    ('Q', b"111111333"),
    ('R', b"311111331"),
    ('S', b"113111331"),
    ('T', b"111131331"),
    ('U', b"331111113"),
    ('V', b"133111113"),
    ('W', b"333111111"),
    ('X', b"131131113"),
    ('Y', b"331131111"),
    ('Z', b"133131111"),
    ('-', b"131111313"),
    ('.', b"331111311"),
    (' ', b"133111311"),
    ('$', b"131313111"),
    ('/', b"131311131"),
    ('+', b"131113131"),
    ('%', b"111313131"),
    ('*', b"131131311"),
];

/// Index for check-digit calculation (mod-43).
fn check_index(c: char) -> Option<u8> {
    PATTERNS.iter().position(|&(p, _)| p == c).and_then(|i| {
        // Exclude '*' (last entry) from the check-digit alphabet.
        if i < 43 {
            Some(i as u8)
        } else {
            None
        }
    })
}

fn pattern_for(c: char) -> Option<&'static [u8; 9]> {
    PATTERNS
        .iter()
        .find_map(|&(p, pat)| if p == c { Some(pat) } else { None })
}

/// Encode a Code 39 payload.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Code39, "HELLO-123", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.to_uppercase();
    let include_check = opts.get("includecheck").is_some_and(|v| v == "true");

    // Validate every character is in the alphabet (excluding `*`).
    for c in payload.chars() {
        if c == '*' || pattern_for(c).is_none() {
            return Err(Error::InvalidData(format!(
                "Code 39 does not support character {c:?}"
            )));
        }
    }

    // Optional mod-43 check digit.
    let mut full = payload.clone();
    if include_check {
        let sum: u32 = payload
            .chars()
            .map(|c| u32::from(check_index(c).unwrap()))
            .sum();
        let check = PATTERNS[(sum % 43) as usize].0;
        full.push(check);
    }

    // Build the module string: *<payload>* with a 1-module inter-char
    // gap between every pair of consecutive characters AND a trailing
    // 1-module gap after the last character (which BWIPP emits as the
    // final element of `sbs` — see `b.raw("code39", text, {})`).
    let mut modules = String::new();
    let framed = format!("*{full}*");
    for (i, c) in framed.chars().enumerate() {
        let pat = pattern_for(c).expect("validated above");
        if i > 0 {
            modules.push('0'); // inter-character gap (1 narrow space)
        }
        let mut bar = true;
        for &width_byte in pat {
            let width = if width_byte == b'3' { WIDE } else { NARROW };
            let module = if bar { '1' } else { '0' };
            for _ in 0..width {
                modules.push(module);
            }
            bar = !bar;
        }
    }
    modules.push('0'); // trailing inter-char gap, matches BWIPP's sbs output

    let text = if opts.include_text { Some(full) } else { None };
    Ok(LinearPattern::from_modules(&modules, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_digit_zero() {
        // '*' (start) gap '0' gap '*' (stop)
        // start *: 1 0 1 0 0 0 1 0 1 0 0 0 1 -> bars=[1,1,1,3,1,1,3,1,1] in alt
        // Stage 11.A8c (cont) — strengthen bare asserts with descriptive
        // labels naming the input + expected lower bound. Code 39 emits
        // 9 sbs entries per char plus a 1-element inter-char gap, so
        // 3 chars + 2 gaps = 29 entries minimum. The `>= 27` lower
        // bound (kept loose for golden-table tolerance) catches the
        // worst case where the encoder regresses to outputting only
        // the data char without start/stop framing (9 entries).
        let p = encode("0", &Options::default()).unwrap();
        assert!(
            p.bars.len() >= 27,
            "encode(\"0\") must emit ≥27 sbs entries (start * + '0' + stop * + 2 gaps); got {} entries",
            p.bars.len()
        );
        assert!(
            p.total_width() > 0,
            "encode(\"0\").total_width() must be > 0 (kills mutation returning empty bars vec); got {}",
            p.total_width()
        );
    }

    #[test]
    fn rejects_lowercase_with_unknown_char() {
        // Lowercase gets uppercased, but a character like '!' is invalid.
        //
        // Stage 11.A8c — upgrade from matches!(_, InvalidData(_)) to
        // pin the diagnostic substring + char echo. The encode() loop
        // has ONE InvalidData rejection arm (line 102-106):
        //   "Code 39 does not support character '?'"
        // A mutant that drops `{c:?}` or swaps the predicate text
        // survives the variant-only check.
        let err = encode("HELLO!", &Options::default()).unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("unknown-char must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("Code 39"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("does not support character"),
            "diagnostic must include the predicate phrase; got {msg:?}"
        );
        assert!(
            msg.contains("'!'"),
            "diagnostic must echo the offending char via {{c:?}}; got {msg:?}"
        );
    }

    /// Stage 11.A8c — pin the `c == '*'` rejection arm at line 102.
    /// The existing `rejects_lowercase_with_unknown_char` test uses
    /// `'!'` which triggers the `pattern_for(c).is_none()` half of
    /// the OR. The `'*'` half is uncovered: pattern_for('*') IS
    /// Some (since PATTERNS[43] is the start/stop sentinel), so
    /// without the explicit `c == '*' ||` check, the encoder would
    /// accept `'*'` as a data character and emit duplicate start
    /// sentinels — corrupt symbol.
    ///
    /// Anchors:
    ///   1. "ABC*" → rejected with diagnostic mentioning '*'.
    ///   2. "*"   → rejected.
    ///   3. "*ABC*" (looks like already-bracketed) → rejected
    ///      (the leading '*' is inside the payload at this point).
    ///   4. Sanity: "ABC" → accepts (the `c == '*' ||` short-circuit
    ///      must NOT reject normal letters).
    ///
    /// Mutations killed:
    ///   * `c == '*' ||` removed → "*" payloads would proceed to
    ///     pattern_for('*') = Some, encode '*' as data, emit
    ///     duplicate sentinels in the symbol.
    ///   * `'*'` → `'#'` (different literal) → asterisk slips
    ///     through, '#' becomes the spurious rejection.
    ///   * `||` → `&&` collapses the OR — would only reject when
    ///     BOTH c == '*' AND pattern_for is None, which is never
    ///     true (anything in pattern_for is Some).
    #[test]
    fn rejects_asterisk_sentinel_in_payload() {
        // Single '*' rejects.
        //
        // Stage 11.A8c (cont) — anchor parity with sibling `ABC*`
        // arm at lines 232-236: add `Code 39` symbology-prefix anchor
        // (the sibling already requires the full `Code 39 does not
        // support character` predicate). The original 2-anchor pin
        // here only required the predicate + char-echo; a mutation
        // that drops the symbology name from the format string at
        // line 104 of code39.rs would pass.
        match encode("*", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 39"), "missing `Code 39` prefix: {msg}");
                assert!(
                    msg.contains("does not support character") && msg.contains("'*'"),
                    "expected '*' rejection diagnostic, got: {msg}"
                );
            }
            other => panic!("'*' should reject, got {other:?}"),
        }
        // '*' anywhere in payload rejects. Defense-in-depth with the
        // single-'*' anchor above — both inputs must produce the same
        // line-104 diagnostic ("Code 39 does not support character {c:?}").
        match encode("ABC*", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Code 39 does not support character"),
                    "'ABC*' rejection must carry the Code 39 prefix + predicate; got {msg}"
                );
                assert!(
                    msg.contains("'*'"),
                    "'ABC*' rejection must echo the offending char as `'*'`; got {msg}"
                );
            }
            other => panic!("'ABC*' should reject, got {other:?}"),
        }
        // Pre-bracketed (looks like already-framed) → still rejects.
        // The encoder always wraps in `*...*` internally; passing
        // `*ABC*` would double-bracket.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only
        // `Err(Error::InvalidData(_))` to a 3-anchor pin that matches
        // the sibling arms — kills variant-only mutations that drop
        // the `Code 39 does not support character {c:?}` format
        // string in the asterisk rejection arm at line 104:
        //   1. `Code 39` symbology tag
        //   2. `does not support character` predicate
        //   3. `'*'` char-echo (first encountered '*' at offset 0)
        match encode("*ABC*", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Code 39"), "missing `Code 39` prefix: {msg}");
                assert!(
                    msg.contains("does not support character"),
                    "missing predicate: {msg}"
                );
                assert!(msg.contains("'*'"), "missing char-echo `'*'`: {msg}");
            }
            other => panic!("'*ABC*' should reject, got {other:?}"),
        }
        // Sanity: "ABC" still encodes (the `c == '*' ||` short-circuit
        // must not break the normal path).
        assert!(
            encode("ABC", &Options::default()).is_ok(),
            "ABC must still encode"
        );
    }

    #[test]
    fn lowercase_is_uppercased() {
        let p1 = encode("CODE39", &Options::default()).unwrap();
        let p2 = encode("code39", &Options::default()).unwrap();
        assert_eq!(p1.bars, p2.bars);
    }

    #[test]
    fn check_digit_changes_output() {
        let p1 = encode("CODE39", &Options::default()).unwrap();
        let p2 = encode("CODE39", &Options::default().with("includecheck", "true")).unwrap();
        assert_ne!(p1.bars, p2.bars);
    }

    /// Golden bar pattern for `"HELLO"` captured from bwip-js's
    /// `raw("code39", "HELLO", {})[0].sbs`. Includes the trailing
    /// inter-char gap BWIPP emits after the final `*` stop.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        let p = encode("HELLO", &Options::default()).unwrap();
        let want: [u8; 70] = [
            1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1,
            1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3,
            1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
        ];
        assert_eq!(p.bars, want, "code39 bars mismatch vs bwip-js raw output");
    }

    /// Stage 11.A8c — pin `check_index` boundary behaviour at the `*`
    /// sentinel. The `if i < 43` exclusion (line ~72) keeps `'*'` out
    /// of the check-digit alphabet — without it, including `'*'` as
    /// codeword 43 would offset the mod-43 sum on any payload
    /// containing the sentinel. The encoder rejects `'*'` upstream
    /// (line ~102) so the `<` boundary is unreachable from the
    /// public API; without a direct helper test the
    /// `i < 43 -> i <= 43` mutant survives.
    ///
    /// Verifies the full ordering: digits 0..=9 at indices 0..=9,
    /// letters A..=Z at 10..=35, dash/period/space at 36..=38,
    /// $/+///% at 39..=42, and the `'*'` sentinel at index 43
    /// returning None.
    #[test]
    fn check_index_excludes_star_sentinel() {
        // Digits (0..=9).
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(check_index(c), Some(i as u8), "check_index({c:?})");
        }
        // Letters (10..=35).
        for (i, c) in ('A'..='Z').enumerate() {
            assert_eq!(check_index(c), Some((i + 10) as u8), "check_index({c:?})");
        }
        // Punctuation per PATTERNS ordering:
        //   '-' = 36, '.' = 37, ' ' = 38, '$' = 39, '/' = 40, '+' = 41,
        //   '%' = 42.
        assert_eq!(check_index('-'), Some(36));
        assert_eq!(check_index('.'), Some(37));
        assert_eq!(check_index(' '), Some(38));
        assert_eq!(check_index('$'), Some(39));
        assert_eq!(check_index('/'), Some(40));
        assert_eq!(check_index('+'), Some(41));
        assert_eq!(check_index('%'), Some(42));
        // '*' is at PATTERNS index 43 — must be excluded from the
        // check-digit alphabet (returns None). The `i < 43 -> i <= 43`
        // mutant would return Some(43) here.
        assert_eq!(
            check_index('*'),
            None,
            "'*' (PATTERNS[43]) must be excluded from check-digit alphabet"
        );
        // Lowercase and other non-alphabet → None.
        assert_eq!(check_index('a'), None);
        assert_eq!(check_index('?'), None);
        assert_eq!(check_index('\0'), None);
    }

    /// Stage 11.A8c — pin `pattern_for(c)` directly. The 44-entry
    /// PATTERNS table (10 digits + 26 letters + 7 punctuation +
    /// '*' sentinel) is only searched transitively by encode(); a
    /// mutation that swaps the `p == c` predicate or replaces `Some`
    /// with `None` could survive if the encoder's downstream
    /// `expect("validated above")` panics differently than expected.
    ///
    /// Mutations to catch:
    ///   - `p == c` → `p != c`: returns FIRST non-match (any 'A'
    ///     lookup returns PATTERNS[0] = '0' pattern).
    ///   - `Some(pat)` → `None`: every lookup returns None.
    ///   - `find_map` body removed: type error.
    ///
    /// Anchors at the start, mid, and end of each region.
    #[test]
    fn pattern_for_table_lookup() {
        // Digit 0 and 9 (start and end of digit region).
        assert_eq!(pattern_for('0'), Some(b"111331311"));
        assert_eq!(pattern_for('9'), Some(b"113311311"));
        // Letter A and Z (start and end of letter region).
        assert_eq!(pattern_for('A'), Some(b"311113113"));
        assert_eq!(pattern_for('Z'), Some(b"133131111"));
        // Mid-region letter.
        assert_eq!(pattern_for('M'), Some(b"313111131"));
        // Punctuation: -, ., space, $, /, +, % (all 7).
        assert_eq!(pattern_for('-'), Some(b"131111313"));
        assert_eq!(pattern_for('.'), Some(b"331111311"));
        assert_eq!(pattern_for(' '), Some(b"133111311"));
        assert_eq!(pattern_for('$'), Some(b"131313111"));
        assert_eq!(pattern_for('/'), Some(b"131311131"));
        assert_eq!(pattern_for('+'), Some(b"131113131"));
        assert_eq!(pattern_for('%'), Some(b"111313131"));
        // Start/stop sentinel '*' is in PATTERNS (unlike check_index
        // which excludes it).
        assert_eq!(pattern_for('*'), Some(b"131131311"));
        // Non-members.
        assert_eq!(pattern_for('a'), None, "lowercase rejected");
        assert_eq!(pattern_for('?'), None);
        assert_eq!(pattern_for('@'), None);
        assert_eq!(pattern_for(';'), None);
        assert_eq!(pattern_for('\0'), None);
        // Length sanity: every pattern is exactly 9 bytes.
        assert_eq!(pattern_for('0').unwrap().len(), 9);
        assert_eq!(pattern_for('*').unwrap().len(), 9);
    }

    /// Goldens that exercise the optional mod-43 check-digit path
    /// (`includecheck: true`) and the dash special character. The
    /// check appends one extra Code 39 character (= 10 sbs runs incl
    /// the inter-char gap) compared to the same input without
    /// `includecheck`.
    #[test]
    fn matches_bwipp_with_check_and_punct() {
        let cases: &[(&str, bool, &[u8])] = &[
            (
                "HELLO",
                true,
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1,
                    1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 3, 1,
                    1, 1,
                ],
            ),
            (
                "12345",
                false,
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1,
                    1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1,
                    1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
                ],
            ),
            (
                "12345",
                true,
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1,
                    1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 3, 1, 3, 1,
                    1, 3, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 1,
                    1, 1,
                ],
            ),
            (
                "ABC-123",
                false,
                &[
                    1, 3, 1, 1, 3, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 1, 3,
                    1, 1, 3, 1, 3, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1,
                    1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 3, 1, 1, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 1, 1,
                    1, 1, 1, 3, 1, 1, 3, 1, 3, 1, 1, 1,
                ],
            ),
        ];
        for &(text, check, want) in cases {
            let opts = if check {
                Options::default().with("includecheck", "true")
            } else {
                Options::default()
            };
            // Stage 11.A8c (cont) — `.unwrap()` would panic with only
            // the inner Error::Display on failure; `.expect(...)` adds
            // per-iteration input + check-flag echo so the failing
            // corpus item is named.
            let got = encode(text, &opts).unwrap_or_else(|e| {
                panic!("encode({text:?}, includecheck={check}) (Code 39 sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars, want,
                "code39 sbs mismatch for {text:?} (check={check})"
            );
        }
    }
}
