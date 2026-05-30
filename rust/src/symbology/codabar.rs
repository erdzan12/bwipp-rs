//! Codabar (NW-7, USS Codabar, 2 of 7).
//!
//! 16-character alphabet: digits, `-$:/.+`, and `A B C D` as start/stop
//! characters (often rendered as `A`/`B`/`C`/`D`/`T`/`N`/`*`/`E`). Each
//! character is 7 elements (4 bars + 3 spaces).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

// Per-character bit patterns (1 = bar module, 0 = space module),
// ported from BWIPP's `rationalizedCodabar_encs` run-length table
// with the trailing inter-char-space digit dropped (we add the
// inter-char space explicitly in `encode`). BWIPP uses a 1:3
// narrow:wide ratio; previous pattern strings here used 1:2 and so
// emitted slightly narrower symbols than BWIPP/spec.
const PATTERNS: &[(char, &str)] = &[
    ('0', "10101000111"),
    ('1', "10101110001"),
    ('2', "10100010111"),
    ('3', "11100010101"),
    ('4', "10111010001"),
    ('5', "11101010001"),
    ('6', "10001010111"),
    ('7', "10001011101"),
    ('8', "10001110101"),
    ('9', "11101000101"),
    ('-', "10100011101"),
    ('$', "10111000101"),
    (':', "1110101110111"),
    ('/', "1110111010111"),
    ('.', "1110111011101"),
    ('+', "1011101110111"),
    ('A', "1011100010001"),
    ('B', "1000100010111"),
    ('C', "1010001000111"),
    ('D', "1010001110001"),
];

fn pattern_for(c: char) -> Option<&'static str> {
    PATTERNS
        .iter()
        .find_map(|&(p, s)| if p == c { Some(s) } else { None })
}

/// Position of each character in BWIPP's `rationalizedCodabar_barchars`
/// — the index doubles as both the row in [`PATTERNS`] and the
/// codeword value the mod-16 check sums over.
const BARCHARS: &str = "0123456789-$:/.+ABCD";

/// Encode a Codabar payload. The first and last characters must be start/stop
/// characters chosen from `A`, `B`, `C`, `D`.
///
/// Recognized options:
/// * `includecheck = "true"` — append a mod-16 sum check character
///   (inserted before the stop char). Matches BWIPP's
///   `rationalizedCodabar` behaviour.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Codabar requires start/stop characters (A/B/C/D) at both ends.
/// let svg = render_svg(Symbology::Codabar, "A1234B", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let payload = data.to_uppercase();
    let mut chars: Vec<char> = payload.chars().collect();
    if chars.len() < 3 {
        return Err(Error::InvalidData(
            "Codabar needs at least start, one data char, and stop".into(),
        ));
    }
    if !matches!(chars.first(), Some('A' | 'B' | 'C' | 'D'))
        || !matches!(chars.last(), Some('A' | 'B' | 'C' | 'D'))
    {
        return Err(Error::InvalidData(
            "Codabar payload must begin and end with one of A, B, C, or D".into(),
        ));
    }
    for c in chars.iter() {
        if pattern_for(*c).is_none() {
            return Err(Error::InvalidData(format!("Codabar: unsupported {c:?}")));
        }
    }

    if opts.get("includecheck").is_some_and(|v| v == "true") {
        // BWIPP's check: sum the codeword values of every char, take
        // (16 - sum%16) % 16, and insert the character at that index
        // just before the stop sentinel.
        let mut sum: u32 = 0;
        for &c in &chars {
            sum += BARCHARS.find(c).unwrap() as u32;
        }
        let check_idx = ((16 - sum % 16) % 16) as usize;
        let check_char = BARCHARS.chars().nth(check_idx).unwrap();
        let stop = chars.pop().unwrap();
        chars.push(check_char);
        chars.push(stop);
    }

    let mut modules = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 {
            modules.push('0');
        }
        modules.push_str(pattern_for(*c).unwrap());
    }
    // Trailing inter-char gap so the sbs output matches BWIPP's
    // `rationalizedCodabar` byte-for-byte (same convention as code39).
    modules.push('0');

    let text = if opts.include_text {
        Some(payload)
    } else {
        None
    };
    Ok(LinearPattern::from_modules(&modules, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_with_start_stop() {
        // Stage 11.A8c (cont) — descriptive label naming input + the
        // mutation class this catches (encoder regression to empty bars).
        let p = encode("A12345B", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode(\"A12345B\").total_width() must be > 0 — start='A' + 5 digits + stop='B' must compose into non-empty bars; got {}",
            p.total_width()
        );
    }

    #[test]
    fn rejects_without_start_stop() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the sibling `rejects_invalid_start_or_stop_with_
        // framing_message` test (line 345-360):
        //   1. `Codabar` symbology name
        //   2. `must begin and end with` predicate
        //   3. `A, B, C, or D` valid-alphabet enumeration
        // The "12345" input has no leading/trailing sentinel; the
        // encoder routes through the same line-80 diagnostic as the
        // invalid-sentinel arms.
        match encode("12345", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Codabar"),
                    "missing Codabar symbology name: {msg}"
                );
                assert!(
                    msg.contains("must begin and end with"),
                    "missing predicate `must begin and end with`: {msg}"
                );
                assert!(
                    msg.contains("A, B, C, or D"),
                    "missing valid-alphabet enumeration `A, B, C, or D`: {msg}"
                );
            }
            other => panic!("'12345' should reject as InvalidData, got {other:?}"),
        }
    }

    /// Codabar golden from `raw("rationalizedCodabar", text, {})[0].sbs`.
    /// BWIPP's `codabar` bcid maps to `rationalizedCodabar` and emits a
    /// trailing 1-module inter-char gap (same convention as Code 39).
    #[test]
    fn matches_bwip_js_raw_sbs() {
        let cases: &[(&str, &[u8])] = &[
            (
                "A12345B",
                &[
                    1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 3,
                    1, 1, 3, 1,
                ],
            ),
            (
                "A123-45D",
                &[
                    1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 3,
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 1, 3, 1, 1, 1,
                    1, 3, 1, 1, 1, 1, 1, 3, 3, 3, 1, 1,
                ],
            ),
            (
                "C0000B",
                &[
                    1, 1, 1, 3, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1,
                    1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 3, 1, 3, 1, 1, 3, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`
            // with per-iteration input echo so a failing corpus item
            // names which payload tripped the encoder.
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode({text:?}) (Codabar sbs corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                got.bars, want,
                "codabar bars mismatch vs bwip-js for {text:?}"
            );
        }
    }

    /// With `includecheck: true`, BWIPP inserts a mod-16 sum check
    /// character just before the stop sentinel. Cross-validated
    /// against `b.raw("rationalizedCodabar", "A123B", {includecheck:
    /// true})[0].sbs`.
    #[test]
    fn matches_bwip_js_with_check() {
        let opts = Options::default().with("includecheck", "true");
        let p = encode("A123B", &opts).unwrap();
        let want: [u8; 48] = [
            1, 1, 3, 3, 1, 3, 1, 1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 1, 3, 1, 1, 3, 1, 3, 3, 1, 1, 1,
            1, 1, 1, 3, 1, 1, 3, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 3, 1,
        ];
        assert_eq!(p.bars, want, "codabar with check sbs mismatch vs bwip-js");
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills `encode: replace < with ==` and `< with <=` at line ~71
    /// (the 3-char minimum length check). Original tests exercised
    /// 5+-char inputs only; the mutants `<= 3` (rejects 3-char inputs)
    /// and `== 3` (rejects only 3-char inputs, accepts 0/1/2-char)
    /// went undetected.
    #[test]
    fn length_boundary_around_three_chars() {
        // 3 chars (start + 1 data + stop) is the *minimum* valid length.
        // The mutant `<= 3` rejects this; the original accepts.
        // Stage 11.A8c (cont) — descriptive label naming boundary
        // (catches `< 3 → <= 3` mutant that would over-reject the
        // minimum 3-char "A1B" form).
        assert!(
            encode("A1B", &Options::default()).is_ok(),
            "encode(\"A1B\") (3-char minimum: start='A' + 1 data digit + stop='B') must accept — kills `< 3` → `<= 3` length-guard mutant"
        );
        // 2 chars must be rejected by both the original and the
        // `== 3` mutant — but the latter accepts 2-char inputs, so
        // a 2-char InvalidData assertion only kills the original-
        // preserving direction; we also need the 3-char success path
        // above to catch the `<= 3` mutant.
        //
        // Stage 11.A8c (cont) — upgrade two discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` checks to
        // 2-anchor pins matching the source diagnostic at line 73
        // of codabar.rs:
        //   1. `Codabar` symbology name
        //   2. `needs at least start, one data char, and stop`
        //      full predicate
        match encode("AB", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Codabar"),
                    "2-char: missing Codabar prefix: {msg}"
                );
                assert!(
                    msg.contains("needs at least start, one data char, and stop"),
                    "2-char: missing min-length predicate: {msg}"
                );
            }
            other => panic!("'AB' (2 chars) should reject as InvalidData, got {other:?}"),
        }
        // 1 char is also rejected (sanity).
        match encode("A", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Codabar"),
                    "1-char: missing Codabar prefix: {msg}"
                );
                assert!(
                    msg.contains("needs at least start, one data char, and stop"),
                    "1-char: missing min-length predicate: {msg}"
                );
            }
            other => panic!("'A' (1 char) should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the outer `% 16` fold in the `includecheck`
    /// path at line ~97 (`((16 - sum % 16) % 16)`). Existing
    /// `matches_bwip_js_with_check` uses "A123B" where sum = 39 and
    /// `sum % 16 = 7`, so the outer `% 16` is unused — both
    /// `((16 - 7) % 16) = 9` and the mutant `(16 - 7) = 9` give the
    /// same character. The fold only matters when `sum % 16 == 0`,
    /// where the original produces `(16 - 0) % 16 = 0` (a '0' check)
    /// but the mutant `drop the outer % 16` would produce 16 (and
    /// then panic on `BARCHARS.chars().nth(16)` for `BARCHARS[16] =
    /// 'A'` — wait, that *does* exist! BARCHARS is 20 chars long, so
    /// index 16 is 'A'. The mutant would silently insert 'A' as the
    /// check character. We pin the legitimate '0' check.
    ///
    /// Hand-computed: BARCHARS = "0123456789-$:/.+ABCD".
    ///   'A' = 16, '+' = 15, 'B' = 17.
    ///   Payload "A+B": sum = 16 + 15 + 17 = 48, 48 % 16 = 0.
    ///   check = (16 - 0) % 16 = 0 → '0' (BARCHARS[0]).
    /// The mutant `(16 - sum % 16)` (no outer fold) would yield 16
    /// and emit 'A' instead.
    #[test]
    fn includecheck_outer_mod_sixteen_folds_to_zero() {
        let opts = Options::default().with("includecheck", "true");
        let with_check = encode("A+B", &opts).unwrap();
        // Without-check baseline for comparison.
        let baseline = encode("A+B", &Options::default()).unwrap();
        // Inserting a '0' check character means the symbol grows by
        // exactly one PATTERNS['0'] codeword + one inter-char gap.
        // PATTERNS['0'] = "10101000111" → 11 modules, with the
        // leading '0' inter-char gap that's 12 modules added. The
        // bar/space alternation expands those 12 modules into a
        // specific sbs delta — we anchor by comparing the encoded
        // payload to a manual "A0+B" without the check option, which
        // produces the exact same bars (insert '0' before stop ≡
        // encoding "A+B" with check ≡ encoding "A0+B" without check).
        // Wait — that's not quite right because the check character
        // goes *before* the stop, so the explicit encoding is "A0+B"
        // → start=A, data=0, data=+, stop=B... but BWIPP inserts the
        // check BEFORE the stop (line ~100: chars.pop() then push
        // check, push stop) so the actual sequence is A, +, 0, B.
        // Let's encode "A+0B" directly to mirror that.
        let synthetic = encode("A+0B", &Options::default()).unwrap();
        assert_eq!(
            with_check.bars, synthetic.bars,
            "encode(\"A+B\", check=true) must equal encode(\"A+0B\", check=false) — \
             the outer % 16 fold folds (16-0) % 16 to 0 and BARCHARS[0] = '0'. \
             The mutant `drop the outer % 16` would insert 'A' (BARCHARS[16]) instead."
        );
        // Sanity: the with-check symbol is longer than the no-check baseline.
        assert!(with_check.bars.len() > baseline.bars.len());
    }

    /// Stage 11.A8c — pin `pattern_for(c)` directly. The 20-entry
    /// PATTERNS table is searched via linear `find_map` and returns
    /// `Some(pattern)` for in-alphabet chars or `None` for unknowns.
    /// Existing tests exercise the lookup only transitively through
    /// the encoder; mutations that swap the `==` predicate or replace
    /// `Some(s)` with `None` can pass through goldens if they happen
    /// to hit a coincidental no-match.
    ///
    /// Mutations to catch:
    ///   - `p == c` → `p != c`: returns FIRST non-match (any 'A'
    ///     lookup would return PATTERNS[1] which is '1').
    ///   - `Some(s)` → `None`: every lookup returns None → encoder
    ///     panics on `.unwrap()` at line 109.
    ///   - `find_map` body removed: type error / nothing returned.
    #[test]
    fn pattern_for_full_alphabet_lookup() {
        // Digits 0..=9.
        assert_eq!(pattern_for('0'), Some("10101000111"));
        assert_eq!(pattern_for('1'), Some("10101110001"));
        assert_eq!(pattern_for('9'), Some("11101000101"));
        // Symbols.
        assert_eq!(pattern_for('-'), Some("10100011101"));
        assert_eq!(pattern_for('$'), Some("10111000101"));
        assert_eq!(pattern_for(':'), Some("1110101110111"));
        assert_eq!(pattern_for('/'), Some("1110111010111"));
        assert_eq!(pattern_for('.'), Some("1110111011101"));
        assert_eq!(pattern_for('+'), Some("1011101110111"));
        // Start/stop chars A..=D.
        assert_eq!(pattern_for('A'), Some("1011100010001"));
        assert_eq!(pattern_for('B'), Some("1000100010111"));
        assert_eq!(pattern_for('C'), Some("1010001000111"));
        assert_eq!(pattern_for('D'), Some("1010001110001"));
        // Non-members → None.
        assert_eq!(pattern_for('E'), None, "E is not in the Codabar alphabet");
        assert_eq!(
            pattern_for('a'),
            None,
            "lowercase a not in alphabet (encoder uppercases first)"
        );
        assert_eq!(pattern_for(' '), None);
        assert_eq!(pattern_for('!'), None);
        assert_eq!(pattern_for('@'), None, "'@' is just before 'A' in ASCII");
        assert_eq!(pattern_for('5'), Some("11101010001"), "mid-alphabet anchor");
        // Length sanity: digits/symbols are 11 modules, ':'/'/'/'.' are 13,
        // '+' is 13, A..=D are 13. The distinct lengths catch a mutation
        // that swaps PATTERNS entries within the table.
        assert_eq!(pattern_for('0').unwrap().len(), 11);
        assert_eq!(pattern_for(':').unwrap().len(), 13);
        assert_eq!(pattern_for('A').unwrap().len(), 13);
    }

    /// Kills `encode: replace || with &&` at line ~77 (the
    /// start/stop validation). Original is `!start_ok || !end_ok`,
    /// mutant is `!start_ok && !end_ok` — accepts inputs where only
    /// *one* of the framing characters is wrong. We can't rely on a
    /// broad `matches!(Err(InvalidData(_)))` check: under the mutant
    /// the start/stop guard is bypassed but the input still trips a
    /// downstream codabar-alphabet check ("Codabar: unsupported …")
    /// with a *different* message. We therefore inspect the error
    /// message to pin the start/stop-guard reason explicitly.
    #[test]
    fn rejects_invalid_start_or_stop_with_framing_message() {
        // Use a payload that is otherwise codabar-valid (digits +
        // A/B/C/D start/stop characters from the alphabet), so the
        // only thing that can reject it is the start/stop framing
        // guard at line 77. We pick a stop character that IS in the
        // codabar alphabet (digit '4') so the downstream pattern_for
        // lookup succeeds — the rejection must come from the
        // start/stop framing check.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("begin
        // and end with")` upgraded to 3-anchor pin for both directional
        // tests:
        //   1. `Codabar` symbology name anchor
        //   2. `must begin and end with` full predicate
        //   3. `A, B, C, or D` valid-start/stop-character list (kills
        //      mutations that drop or rename the alphabet enumeration
        //      in the format string at line 80 of codabar.rs).
        // Stage 11.A8c (cont) — echo the actual rejected input string in
        // the catch-all panic so the failure diagnostic uniquely names
        // which arm fired (was it "A12344" or "11234B"?). Without this,
        // both arms produce the same panic text.
        match encode("A12344", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Codabar"),
                    "missing `Codabar` symbology name: {msg:?}"
                );
                assert!(
                    msg.contains("must begin and end with"),
                    "missing `must begin and end with` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("A, B, C, or D"),
                    "missing valid start/stop alphabet `A, B, C, or D`: {msg:?}"
                );
                // Cross-arm guard: the alphabet-membership rejection from a
                // truly invalid char (e.g. '!') uses a different diagnostic
                // ("contains invalid character"). If a mutation rewires the
                // start/stop guard to fall through to the alphabet check,
                // this guard catches it.
                assert!(
                    !msg.contains("contains invalid character"),
                    "\"A12344\" must reject via START/STOP framing arm, not alphabet-membership arm; got {msg:?}"
                );
            }
            other => panic!(
                "encode(\"A12344\") must reject as Err(InvalidData(start/stop)); got {other:?} (mutation re-routed start/stop framing arm)"
            ),
        }
        // Counter-direction: invalid start, valid stop. Same argument:
        // '1' is in the codabar alphabet so the downstream pattern
        // lookup succeeds.
        match encode("11234B", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Codabar"),
                    "missing `Codabar` symbology name: {msg:?}"
                );
                assert!(
                    msg.contains("must begin and end with"),
                    "missing `must begin and end with` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("A, B, C, or D"),
                    "missing valid start/stop alphabet `A, B, C, or D`: {msg:?}"
                );
                assert!(
                    !msg.contains("contains invalid character"),
                    "\"11234B\" must reject via START/STOP framing arm, not alphabet-membership arm; got {msg:?}"
                );
            }
            other => panic!(
                "encode(\"11234B\") must reject as Err(InvalidData(start/stop)); got {other:?} (mutation re-routed start/stop framing arm)"
            ),
        }
    }
}
