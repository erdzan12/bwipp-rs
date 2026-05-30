//! Japan Post 4-state.
//!
//! Encodes a 20-character postal code field plus a 1-character mod-19
//! check digit into a 67-bar 4-state symbol. The alphabet is
//! `0123456789-ABCDEFGHIJKLMNOPQRSTUVWXYZ` (37 characters). Digits and the
//! dash encode directly (one input char = 3 bars). Letters expand to
//! **two** encoded slots via a prefix-then-digit pattern:
//!
//!   `letter index ∈ 11..=36 → first slot = ((idx-1)/10) + 10`
//!                          `→ second slot = (idx-1) % 10`
//!
//! Slots short of 20 are padded with `encs[14]` ("012" → Tracker, Descender,
//! Ascender). The check digit value is `(19 - sum mod 19) mod 19` and is
//! encoded with `encs[check]`. Start sentinel is `encs[19] = "31"` (Full,
//! Descender) and stop is `encs[20] = "13"` (Descender, Full).
//!
//! Patterns and algorithm ported from `bwipp_japanpost` in bwip-js v4.x.

use crate::encoding::{Bar4State, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;

const BARCHARS: &str = "0123456789-ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 21 patterns: 18 data patterns + start + stop. Indices 19/20 are the
/// 2-bar start/stop sentinels; all others are 3 bars wide.
const ENCS: [&str; 21] = [
    "300", "330", "312", "132", "321", "303", "123", "231", "213", "033", "030", "120", "102",
    "210", "012", "201", "021", "003", "333", "31", "13",
];

const PAD_SLOT_INDEX: usize = 14; // encs[14] used to pad short inputs.

/// Encode a Japan Post 4-state payload.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Postal code + address (alphanumeric; the encoder pads to 20 slots).
/// let svg = render_svg(Symbology::JapanPost, "1500001ABC", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let payload = data.trim().to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData("Japan Post: payload is empty".into()));
    }

    // Build the slot sequence (each slot is an index into ENCS).
    let mut slots: Vec<usize> = Vec::with_capacity(20);
    for c in payload.chars() {
        let idx = BARCHARS
            .find(c)
            .ok_or_else(|| Error::InvalidData(format!("Japan Post: invalid character {c:?}")))?;
        if idx < 11 {
            // Digit or dash: one slot.
            if slots.len() >= 20 {
                return Err(Error::InvalidData(
                    "Japan Post: payload too long (>20 slots after expansion)".into(),
                ));
            }
            slots.push(idx);
        } else {
            // Letter: expand into two slots.
            if slots.len() + 2 > 20 {
                return Err(Error::InvalidData(
                    "Japan Post: payload too long (>20 slots after letter expansion)".into(),
                ));
            }
            let prefix = (idx - 1) / 10 + 10; // 11..=13
            let suffix = (idx - 1) % 10; // 0..=9
            slots.push(prefix);
            slots.push(suffix);
        }
    }
    // Pad to exactly 20 slots with PAD_SLOT_INDEX.
    while slots.len() < 20 {
        slots.push(PAD_SLOT_INDEX);
    }

    // Compute mod-19 check.
    let checksum: u32 = slots.iter().map(|&s| s as u32).sum();
    let check = (19 - checksum % 19) % 19;

    // Build the bar sequence:
    //   start (2 bars) + 20 * 3 bars + check (3 bars) + stop (2 bars) = 67 bars.
    let mut bars: Vec<Bar4State> = Vec::with_capacity(67);
    push_pattern(&mut bars, ENCS[19])?; // start
    for &slot in &slots {
        push_pattern(&mut bars, ENCS[slot])?;
    }
    push_pattern(&mut bars, ENCS[check as usize])?; // check
    push_pattern(&mut bars, ENCS[20])?; // stop

    Ok(Postal4Pattern {
        bars,
        text: if opts.include_text {
            Some(payload)
        } else {
            None
        },
    })
}

fn push_pattern(out: &mut Vec<Bar4State>, pat: &str) -> Result<(), Error> {
    for c in pat.chars() {
        let d = c
            .to_digit(10)
            .ok_or_else(|| Error::InvalidData(format!("Japan Post pattern bad digit {c:?}")))?
            as u8;
        let bar = Bar4State::from_digit(d).ok_or_else(|| {
            Error::InvalidData(format!("Japan Post pattern digit out of range: {d}"))
        })?;
        out.push(bar);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `push_pattern` happy/error branches.
    ///
    /// Mutations caught:
    ///   * `to_digit(10)` → `to_digit(16)` would accept `'A'..='F'`
    ///     as 10..=15 instead of rejecting — caught by the bad-char
    ///     case.
    ///   * `Bar4State::from_digit(d)` returning None for d≥4 must
    ///     surface as Err, not silently drop — caught by digit "4".
    ///   * Reordering the two `ok_or_else` calls would swap which
    ///     error message gets emitted, but the discriminant
    ///     (Err(InvalidData)) stays the same — message-substring
    ///     assertions pin that.
    ///   * Happy path: pat "0123" → [Tracker, Descender, Ascender,
    ///     Full] in order pins digit-to-Bar4State mapping and the
    ///     order-preservation of the push loop.
    #[test]
    fn push_pattern_happy_and_error_branches() {
        // Happy: 0,1,2,3 → Tracker, Descender, Ascender, Full in order.
        let mut bars: Vec<Bar4State> = Vec::new();
        push_pattern(&mut bars, "0123").unwrap();
        assert_eq!(
            bars,
            vec![
                Bar4State::Tracker,
                Bar4State::Descender,
                Bar4State::Ascender,
                Bar4State::Full,
            ]
        );

        // Empty pattern: no error, no push.
        // Stage 11.A8c (cont) — descriptive label naming no-op invariant.
        let mut bars: Vec<Bar4State> = Vec::new();
        push_pattern(&mut bars, "").unwrap();
        assert!(
            bars.is_empty(),
            "push_pattern(\"\") must be a no-op (empty pattern string → no Bar4State pushed); got len={}",
            bars.len()
        );

        // Bad character (non-digit) → InvalidData with "bad digit"
        // message.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("bad
        // digit")` upgraded to 4-anchor pin:
        //   1. `Japan Post pattern` symbology prefix
        //   2. `bad digit` predicate
        //   3. char Debug echo `'A'`
        //   4. cross-arm contamination guard: must NOT contain
        //      `out of range` (the other arm's wording)
        let mut bars: Vec<Bar4State> = Vec::new();
        let err = push_pattern(&mut bars, "A").unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Japan Post pattern"),
                    "missing Japan Post prefix: {msg:?}"
                );
                assert!(
                    msg.contains("bad digit"),
                    "missing `bad digit` predicate: {msg:?}"
                );
                assert!(msg.contains("'A'"), "missing char Debug echo 'A': {msg:?}");
                assert!(
                    !msg.contains("out of range"),
                    "cross-arm contamination: bad-digit msg mentions `out of range`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }

        // Digit out of range (4..=9): to_digit succeeds → returns
        // Some(4)..; Bar4State::from_digit returns None → "out of
        // range" branch fires.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("out of
        // range")` upgraded to 4-anchor pin:
        //   1. `Japan Post pattern` symbology prefix
        //   2. `out of range` predicate
        //   3. digit-value echo `4`
        //   4. cross-arm contamination guard: must NOT contain `bad
        //      digit` (the other arm's wording).
        let mut bars: Vec<Bar4State> = Vec::new();
        let err = push_pattern(&mut bars, "4").unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Japan Post pattern"),
                    "missing Japan Post prefix: {msg:?}"
                );
                assert!(
                    msg.contains("out of range"),
                    "missing `out of range` predicate: {msg:?}"
                );
                assert!(
                    msg.contains(": 4"),
                    "missing digit-value echo `: 4`: {msg:?}"
                );
                assert!(
                    !msg.contains("bad digit"),
                    "cross-arm contamination: out-of-range msg mentions `bad digit`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 48 of japan_post.rs:
        //   1. `Japan Post:` symbology prefix (with colon — discriminates
        //      from the `Japan Post pattern` prefix used by the push_pattern
        //      helper diagnostics at lines 111 / 114)
        //   2. `payload is empty` predicate
        // Empty-payload diagnostic carries no value to echo; cross-arm
        // contamination guards strengthen the signal.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Japan Post:"),
                    "missing `Japan Post:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "missing `payload is empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-character diagnostic leaked into empty reject: {msg}"
                );
                assert!(
                    !msg.contains("payload too long"),
                    "wrong arm — overflow diagnostic leaked into empty reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_character() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 56 of japan_post.rs:
        //   1. `Japan Post:` symbology prefix
        //   2. `invalid character` predicate (distinct from the
        //      `payload is empty` and `payload too long` siblings)
        //   3. `'!'` Debug echo of the offending char — note the
        //      payload "HELLO!" is uppercased so H/E/L/L/O are all
        //      valid BARCHARS entries; the '!' at idx 5 is the first
        //      lookup miss.
        match encode("HELLO!", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Japan Post:"),
                    "missing `Japan Post:` prefix: {msg}"
                );
                assert!(
                    msg.contains("invalid character"),
                    "missing `invalid character` predicate: {msg}"
                );
                assert!(msg.contains("'!'"), "missing `'!'` Debug echo: {msg}");
                assert!(
                    !msg.contains("payload is empty"),
                    "wrong arm — empty-payload diagnostic leaked into invalid-char reject: {msg}"
                );
                assert!(
                    !msg.contains("payload too long"),
                    "wrong arm — overflow diagnostic leaked into invalid-char reject: {msg}"
                );
            }
            other => panic!("\"HELLO!\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn digit_only_payload_fills_20_slots_and_renders_67_bars() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Japan Post digit-only path: 12-char "123-4567-890" with
        // dashes → 12 digit slots + 8 padding = 20 slots; total bars
        // = 2 (start) + 20×3 + 3 (check) + 2 (stop) = 67.
        let p = encode("123-4567-890", &Options::default()).expect(
            "encode(\"123-4567-890\", default) (Japan Post digit-only path: 12 digit chars + 8 pad → 20 slots → 67 bars total) must succeed",
        );
        // Total bars = 2 (start) + 20*3 (slots) + 3 (check) + 2 (stop) = 67.
        assert_eq!(p.bars.len(), 67);
        // First two bars are the start pattern "31" -> Full, Descender.
        assert_eq!(p.bars[0], Bar4State::Full);
        assert_eq!(p.bars[1], Bar4State::Descender);
        // Last two bars are the stop pattern "13" -> Descender, Full.
        assert_eq!(p.bars[65], Bar4State::Descender);
        assert_eq!(p.bars[66], Bar4State::Full);
    }

    #[test]
    fn letters_expand_into_two_slots() {
        // "A" has barchars index 11; expansion: prefix = 11, suffix = 0.
        // Plus 18 padding slots of index 14 => 20 slots total.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Japan Post letter-expansion-vs-pure-pad path: "A" → 2
        // slots (prefix+suffix) + 18 pad; "00" → 2 digit slots + 18
        // pad. Both yield 67 bars but the bar patterns differ.
        let with_letter = encode("A", &Options::default()).expect(
            "encode(\"A\", default) (Japan Post letter-expansion path: 'A' → 2 slots (prefix 11, suffix 0) + 18 pad) must succeed",
        );
        let pure_pad = encode("00", &Options::default()).expect(
            "encode(\"00\", default) (Japan Post pure-digit path for letter-vs-digit equivalence; 2 zero slots + 18 pad) must succeed",
        );
        // Both produce 67 bars (the slot layout is identical-length).
        assert_eq!(with_letter.bars.len(), 67);
        assert_eq!(pure_pad.bars.len(), 67);
        // But the actual bar sequences differ — letter expansion changes
        // which patterns appear in slots 0 and 1.
        assert_ne!(with_letter.bars, pure_pad.bars);
    }

    #[test]
    fn check_digit_is_deterministic() {
        // Same input twice → identical output.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Japan Post determinism path: same input must produce
        // same bars — guards against any non-determinism mutant.
        let p1 = encode("123456", &Options::default()).expect(
            "encode(\"123456\", default) (Japan Post determinism baseline; first call must succeed) must succeed",
        );
        let p2 = encode("123456", &Options::default()).expect(
            "encode(\"123456\", default) (Japan Post determinism duplicate; second call must produce identical bars) must succeed",
        );
        assert_eq!(p1.bars, p2.bars);
    }

    #[test]
    fn rejects_overflow_beyond_20_slots() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 4-anchor pin
        // matching the source diagnostic at line 68-69 of japan_post.rs:
        //   1. `Japan Post:` symbology prefix
        //   2. `payload too long` predicate
        //   3. `>20 slots` value echo
        //   4. `after letter expansion` parenthetical (discriminates
        //      from the digit-overflow path at line 60-61 which uses
        //      the bare `after expansion` text)
        // 26 letters: 10 letters fill 20 slots (each letter pushes 2);
        // the 11th letter trips slots+2=22 > 20.
        let payload = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        match encode(payload, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Japan Post:"),
                    "missing `Japan Post:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload too long"),
                    "missing `payload too long` predicate: {msg}"
                );
                assert!(
                    msg.contains(">20 slots"),
                    "missing `>20 slots` value echo: {msg}"
                );
                assert!(
                    msg.contains("after letter expansion"),
                    "missing `after letter expansion` parenthetical: {msg}"
                );
                assert!(
                    !msg.contains("payload is empty"),
                    "wrong arm — empty-payload diagnostic leaked into overflow reject: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-character diagnostic leaked into overflow reject: {msg}"
                );
            }
            other => panic!(
                "26-letter payload should reject as InvalidData with overflow diagnostic, got {other:?}"
            ),
        }
    }

    #[test]
    fn lowercase_uppercased() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Japan Post case-folding path: lowercase inputs must
        // normalize to uppercase via to_ascii_uppercase().
        let upper = encode("ABC", &Options::default()).expect(
            "encode(\"ABC\", default) (Japan Post uppercase baseline for case-fold equivalence) must succeed",
        );
        let lower = encode("abc", &Options::default()).expect(
            "encode(\"abc\", default) (Japan Post lowercase auto-upper path; must equal \"ABC\" bars via to_ascii_uppercase()) must succeed",
        );
        assert_eq!(upper.bars, lower.bars);
    }

    fn classify(bars: &[Bar4State]) -> String {
        bars.iter()
            .map(|b| match b {
                Bar4State::Full => 'F',
                Bar4State::Tracker => 'T',
                Bar4State::Ascender => 'A',
                Bar4State::Descender => 'D',
            })
            .collect()
    }

    /// Byte-for-byte cross-validation against bwip-js. The expected
    /// strings below come from classifying each bar by its `(bhs,
    /// bbs)` pair from `b.raw("japanpost", text, {})[0]`:
    ///
    ///   * `bhs = 0.175`  → Full
    ///   * `bhs = 0.04375` → Tracker
    ///   * `bbs = 0`, `bhs = 0.109` → Descender
    ///   * `bbs = 0.109`, `bhs = 0.109` → Ascender
    ///
    /// Covers digit-only, digit + dash, alphabetic (letter expansion),
    /// and full-payload-no-padding cases.
    #[test]
    fn bar_sequence_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            (
                "123-4567-890",
                "FDFFTFDADFATFTFADFTFDAFAFDTFTADFTFFFTTTDATDATDATDATDATDATDATDAADTDF",
            ),
            (
                "ABC",
                "FDDATFTTDATFFTDATFDATDATDATDATDATDATDATDATDATDATDATDATDATDATDAATDDF",
            ),
            (
                "13-4567-890",
                "FDFFTDFATFTFADFTFDAFAFDTFTADFTFFFTTTDATDATDATDATDATDATDATDATDAFFTDF",
            ),
            (
                "12300000000000000000",
                "FDFFTFDADFAFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTFTTADTDF",
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the Japan Post bar-
            // sequence corpus row (4 cross-validated cases: digit-
            // dash, alphabetic letter-expansion, dash-prefix, full-
            // no-pad).
            let got = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode({text:?}, default) (Japan Post bar-sequence corpus row: digit-only / letter-expansion / dash / full-payload-no-pad) must succeed: {e:?}",
                )
            });
            assert_eq!(
                classify(&got.bars),
                want,
                "Japan Post bar sequence mismatch for {text:?}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills `encode: replace > with ==` and `> with >=` at line ~67
    /// (the slot-overflow check for letter expansion). Original
    /// existing tests use payloads either far below or far above the
    /// boundary; the mutants fire only at exactly the boundary. We
    /// add tests at slots.len() == 18 + letter (fits exactly to 20
    /// — must accept) and slots.len() == 19 + letter (overflows to 21
    /// — must reject).
    #[test]
    fn letter_expansion_at_slot_boundary() {
        // 18 digits + 1 letter ⇒ 18 + 2 = 20 slots; pads to 20, no overflow.
        // Stage 11.A8c (cont) — descriptive label naming exact-fit boundary.
        let exact_fit = "1".repeat(18) + "A";
        assert!(
            encode(&exact_fit, &Options::default()).is_ok(),
            "encode(18 digits + letter 'A') (slot expansion 18 + 2 = 20 = exact-fit boundary) must accept — kills `> 20` → `>= 20` mutant that would reject the boundary input"
        );
        // 19 digits + 1 letter ⇒ 19 + 2 = 21 slots; must reject.
        // Stage 11.A8c (cont) — upgrade the overflow arm from
        // discriminant-only `matches!(_, Err(Error::InvalidData(_)))`
        // to 4-anchor pin matching line 68-69 of japan_post.rs:
        //   1. `Japan Post:` prefix
        //   2. `payload too long` predicate
        //   3. `>20 slots` value echo
        //   4. `after letter expansion` parenthetical (the letter at
        //      idx 19 trips slots+2=21 > 20 via the letter-expansion
        //      branch at line 67-71, NOT the digit-overflow branch
        //      at line 60-62; this anchor pins that)
        let overflow = "1".repeat(19) + "A";
        match encode(&overflow, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Japan Post:"),
                    "missing `Japan Post:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload too long"),
                    "missing `payload too long` predicate: {msg}"
                );
                assert!(
                    msg.contains(">20 slots"),
                    "missing `>20 slots` value echo: {msg}"
                );
                assert!(
                    msg.contains("after letter expansion"),
                    "missing `after letter expansion` parenthetical (must hit letter-expansion branch, not digit-overflow): {msg}"
                );
            }
            other => panic!("19-digit+'A' should reject as InvalidData, got {other:?}"),
        }
    }

    /// Kills `encode: replace - with +` and `- with /` at line ~72
    /// (`prefix = (idx - 1) / 10 + 10`). Original tests pin the bar
    /// sequence for letter inputs ('A', 'B', 'C'); but those happen to
    /// have idx==11, 12, 13 — small values for which the mutant
    /// `(idx + 1) / 10 + 10` produces the same `10 + 1 == 11` /
    /// `10 + 1 == 11` / `10 + 1 == 11` and the `/` mutant gives `idx /
    /// 10 + 10` which is wrong. The existing 'A','B','C' golden does
    /// catch these — but only via `classify` of the bar sequence, not
    /// via slot-index assertions. We pin a letter with `idx >= 20`
    /// (e.g. 'J' = idx 20) so the integer arithmetic forks across the
    /// mutants.
    #[test]
    fn letter_slot_prefix_arithmetic_matches_spec() {
        // 'J' is BARCHARS[20] (letters start at index 11 — 'A'=11, ...,
        // 'J' = 20). prefix = (20-1)/10 + 10 = 11. Mutant -+: (20+1)/10
        // + 10 = 12. Mutant -/: (20/1)/10 + 10 = 12. The bar sequence
        // for the 'J'-prefix slot 11 differs from slot 12.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Japan Post letter-prefix arithmetic mutation paths: 'J'
        // (idx 20) and 'K' (idx 21) must produce DIFFERENT bars,
        // killing `-+` / `-/` arithmetic mutants on prefix formula.
        let with_j = encode("J", &Options::default()).expect(
            "encode(\"J\", default) (Japan Post letter-expansion 'J' idx=20: prefix=(20-1)/10+10=11; pins `-+` / `-/` arithmetic mutants) must succeed",
        );
        let with_k = encode("K", &Options::default()).expect(
            "encode(\"K\", default) (Japan Post letter-expansion 'K' idx=21: must differ from 'J' bars; pins prefix-arithmetic mutants) must succeed",
        );
        assert_ne!(
            with_j.bars, with_k.bars,
            "encode(\"J\") and encode(\"K\") should diverge — \
             likely a regression in the letter-expansion prefix arithmetic"
        );
        // Pin the full bar sequence for the boundary letter 'J' so any
        // arithmetic shift on line 72 makes this assertion fail.
        let got = classify(&with_j.bars);
        let want = "FDDATTFFTDATDATDATDATDATDATDATDATDATDATDATDATDATDATDATDATDATDAADTDF";
        assert_eq!(
            got, want,
            "encode(\"J\") bar sequence regressed; check the (idx-1)/10+10 prefix formula"
        );
    }
}
