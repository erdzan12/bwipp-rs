//! 4-state postal codes.
//!
//! All symbologies in this module share the [`Postal4Pattern`] representation:
//! each character emits a fixed number of bars (typically 4) and each bar
//! is one of `Tracker | Descender | Ascender | Full`. We unify them in a
//! single file because their per-character encoding logic is closely
//! related.
//!
//! Implemented here:
//!   * **DAFT** — literal D/A/F/T input, one bar per character.
//!   * **KIX (Klant Index)** — Dutch postal code, 4 bars per character, no
//!     start/stop, no check digit.
//!   * **Royal Mail RM4SCC** — UK postal code, 4 bars per character plus
//!     start/stop sentinels, with a 6×6 modular check digit.
//!
//! Patterns ported from the BWIPP `daft`, `kix`, and `royalmail` encoders
//! (verified against `bwipp_daft`, `bwipp_kix`, and `bwipp_royalmail` in
//! bwip-js v4.x).

use crate::encoding::{Bar4State, Postal4Pattern};
use crate::error::Error;
use crate::options::Options;

/// The 36-pattern table shared by KIX and (under a different alphabet
/// permutation) Royal Mail RM4SCC. Each entry is 4 digits representing one
/// bar each (0..=3).
const ENCS_36: [&str; 36] = [
    "0033", "0123", "0132", "1023", "1032", "1122", "0213", "0303", "0312", "1203", "1212", "1302",
    "0231", "0321", "0330", "1221", "1230", "1320", "2013", "2103", "2112", "3003", "3012", "3102",
    "2031", "2121", "2130", "3021", "3030", "3120", "2211", "2301", "2310", "3201", "3210", "3300",
];

/// KIX alphabet (digits + uppercase letters, lexicographic order).
const KIX_ALPHA: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Royal Mail RM4SCC alphabet — the permutation BWIPP uses; each character's
/// index in this string equals the 6×6 check-digit grid position
/// `(index / 6, index % 6)`.
const RM4SCC_ALPHA: &str = "ZUVWXY501234B6789AHCDEFGNIJKLMTOPQRS";

/// Start sentinel pattern for Royal Mail (single ascending bar).
const RM4SCC_START: &str = "2";
/// Stop sentinel pattern for Royal Mail (single full bar).
const RM4SCC_STOP: &str = "3";

fn pattern_to_bars(pat: &str, out: &mut Vec<Bar4State>) -> Result<(), Error> {
    for c in pat.chars() {
        let d = c.to_digit(10).ok_or_else(|| {
            Error::InvalidData(format!("postal4 pattern contains non-digit {c:?}"))
        })? as u8;
        let bar = Bar4State::from_digit(d).ok_or_else(|| {
            Error::InvalidData(format!("postal4 pattern digit must be 0..=3 (got {d})"))
        })?;
        out.push(bar);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DAFT
// ---------------------------------------------------------------------------

/// Encode a DAFT payload.
///
/// Input is a string of `D`/`A`/`F`/`T` characters (case-insensitive). Each
/// character maps directly to one bar:
/// `D` = Descender, `A` = Ascender, `F` = Full, `T` = Tracker.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // DAFT is a 4-state pass-through: each character is one bar.
/// let svg = render_svg(Symbology::Daft, "DAFTDAFT", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_daft(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData("DAFT payload must not be empty".into()));
    }
    let mut bars = Vec::with_capacity(data.len());
    for c in data.chars() {
        let bar = match c.to_ascii_uppercase() {
            'D' => Bar4State::Descender,
            'A' => Bar4State::Ascender,
            'F' => Bar4State::Full,
            'T' => Bar4State::Tracker,
            other => {
                return Err(Error::InvalidData(format!(
                    "DAFT accepts only D, A, F, T (got {other:?})"
                )))
            }
        };
        bars.push(bar);
    }
    Ok(Postal4Pattern {
        bars,
        text: if opts.include_text {
            Some(data.to_string())
        } else {
            None
        },
    })
}

// ---------------------------------------------------------------------------
// KIX
// ---------------------------------------------------------------------------

/// Encode a KIX payload. Accepts digits and uppercase letters (lowercase is
/// silently uppercased). No start, stop, or check digit per the Dutch spec.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Dutch KIX: postcode + street/house identifier.
/// let svg = render_svg(Symbology::Kix, "1234AB12", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_kix(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let payload = data.to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData("KIX payload must not be empty".into()));
    }
    let mut bars = Vec::with_capacity(payload.len() * 4);
    for c in payload.chars() {
        let idx = KIX_ALPHA
            .find(c)
            .ok_or_else(|| Error::InvalidData(format!("KIX: invalid character {c:?}")))?;
        pattern_to_bars(ENCS_36[idx], &mut bars)?;
    }
    Ok(Postal4Pattern {
        bars,
        text: if opts.include_text {
            Some(payload)
        } else {
            None
        },
    })
}

// ---------------------------------------------------------------------------
// Royal Mail RM4SCC
// ---------------------------------------------------------------------------

/// Encode a Royal Mail RM4SCC payload. Accepts digits and uppercase letters
/// (lowercase auto-uppercased). Prepends start sentinel, appends mod-6 check
/// digit and stop sentinel per the BS-7732 spec.
///
/// Options:
///   * `validatecheck` = `true` to interpret the last input character as a
///     user-supplied check digit and verify it. Default `false` (computes).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Royal Mail RM4SCC: the encoder adds start/stop sentinels and the
/// // mod-6 check digit automatically.
/// let svg = render_svg(Symbology::RoyalMail, "SN34RD1A", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_royalmail(data: &str, opts: &Options) -> Result<Postal4Pattern, Error> {
    let payload = data.to_uppercase();
    if payload.is_empty() {
        return Err(Error::InvalidData(
            "Royal Mail RM4SCC payload must not be empty".into(),
        ));
    }
    for c in payload.chars() {
        if RM4SCC_ALPHA.find(c).is_none() {
            return Err(Error::InvalidData(format!(
                "Royal Mail RM4SCC: invalid character {c:?}"
            )));
        }
    }

    let validate = opts.get("validatecheck").is_some_and(|v| v == "true");
    let (body, supplied_check) = if validate {
        let mut chars: Vec<char> = payload.chars().collect();
        if chars.len() < 2 {
            return Err(Error::InvalidData(
                "Royal Mail RM4SCC validatecheck: payload too short".into(),
            ));
        }
        let last = chars.pop().unwrap();
        (chars.into_iter().collect::<String>(), Some(last))
    } else {
        (payload.clone(), None)
    };

    let computed = compute_rm4scc_check(&body);
    if let Some(s) = supplied_check {
        if s != computed {
            return Err(Error::InvalidData(format!(
                "Royal Mail RM4SCC: supplied check {s} does not match computed {computed}"
            )));
        }
    }
    let full_body = format!("{body}{computed}");

    // ENCS_36 is indexed by *lexicographic* alphabet position
    // (`KIX_ALPHA`), not by RM4SCC's permuted ordering. BWIPP's
    // `royalmail_encs` is just the same patterns reordered to match
    // `royalmail_barchars`, so looking up KIX_ALPHA gives the same
    // result with one table. `RM4SCC_ALPHA` is still used for the
    // mod-6 check digit math.
    let mut bars = Vec::with_capacity(full_body.len() * 4 + 2);
    pattern_to_bars(RM4SCC_START, &mut bars)?;
    for c in full_body.chars() {
        let idx = KIX_ALPHA.find(c).unwrap();
        pattern_to_bars(ENCS_36[idx], &mut bars)?;
    }
    pattern_to_bars(RM4SCC_STOP, &mut bars)?;

    Ok(Postal4Pattern {
        bars,
        text: if opts.include_text {
            Some(full_body)
        } else {
            None
        },
    })
}

/// Compute the RM4SCC mod-6 check character.
///
/// Each input character contributes a `top` value (`idx / 6`) and a `bot`
/// value (`idx % 6`). The check character's `top` value is `sum_top mod 6`
/// and its `bot` value is `sum_bot mod 6`; the check character itself is
/// the one at `top * 6 + bot` in `RM4SCC_ALPHA`.
fn compute_rm4scc_check(body: &str) -> char {
    let mut sum_top = 0u32;
    let mut sum_bot = 0u32;
    for c in body.chars() {
        let idx = RM4SCC_ALPHA.find(c).unwrap() as u32;
        sum_top += idx / 6;
        sum_bot += idx % 6;
    }
    let check_idx = (sum_top % 6) * 6 + (sum_bot % 6);
    RM4SCC_ALPHA.chars().nth(check_idx as usize).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_string(p: &Postal4Pattern) -> String {
        p.bars
            .iter()
            .map(|b| match b {
                Bar4State::Tracker => 'T',
                Bar4State::Descender => 'D',
                Bar4State::Ascender => 'A',
                Bar4State::Full => 'F',
            })
            .collect()
    }

    // ---- DAFT -------------------------------------------------------------

    #[test]
    fn daft_each_letter_maps_correctly() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DAFT 1:1 char→bar mapping smoke path.
        let p = encode_daft("DAFT", &Options::default()).expect(
            "encode_daft(\"DAFT\", default) (DAFT 1:1 char→bar mapping smoke: D→Descender, A→Ascender, F→Full, T→Tracker) must succeed",
        );
        assert_eq!(bar_string(&p), "DAFT");
    }

    #[test]
    fn daft_is_case_insensitive() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DAFT case-fold path: lowercase input must equal uppercase.
        let lower = encode_daft("daft", &Options::default()).expect(
            "encode_daft(\"daft\", default) (DAFT lowercase case-fold path: must equal uppercase via to_ascii_uppercase()) must succeed",
        );
        let upper = encode_daft("DAFT", &Options::default()).expect(
            "encode_daft(\"DAFT\", default) (DAFT uppercase baseline for case-fold cross-check) must succeed",
        );
        assert_eq!(lower.bars, upper.bars);
    }

    #[test]
    fn daft_rejects_other_letters() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 90-91 of postal4.rs:
        //   1. `DAFT` symbology prefix (distinct from the KIX sibling
        //      at line 126 / 132 sharing the postal4.rs module)
        //   2. `accepts only D, A, F, T` predicate (pins the full
        //      4-char alphabet — a mutation that drops one letter
        //      fails the substring check)
        //   3. `'X'` Debug echo of the offending char (D and A are
        //      valid; X at idx 2 is the first lookup miss)
        match encode_daft("DAX", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("DAFT"), "missing DAFT prefix: {msg}");
                assert!(
                    msg.contains("accepts only D, A, F, T"),
                    "missing `accepts only D, A, F, T` predicate: {msg}"
                );
                assert!(msg.contains("'X'"), "missing 'X' Debug echo: {msg}");
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into letter reject: {msg}"
                );
                assert!(
                    !msg.contains("KIX"),
                    "wrong helper — KIX diagnostic leaked into DAFT reject: {msg}"
                );
            }
            other => panic!("\"DAX\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn daft_rejects_empty() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 80 of postal4.rs:
        //   1. `DAFT` symbology prefix (NOT `KIX` — kills a mutation
        //      that misroutes encode_daft to encode_kix)
        //   2. `payload must not be empty` predicate
        match encode_daft("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("DAFT"), "missing DAFT prefix: {msg}");
                assert!(
                    msg.contains("payload must not be empty"),
                    "missing `payload must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("accepts only D, A, F, T"),
                    "wrong arm — letter diagnostic leaked into empty reject: {msg}"
                );
                assert!(
                    !msg.contains("KIX"),
                    "wrong helper — KIX diagnostic leaked into DAFT empty reject: {msg}"
                );
            }
            other => panic!("empty DAFT payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Cross-validation against bwip-js: `b.raw("daft", text, {})` returns
    /// per-bar `(bhs, bbs)` pairs that classify (using the same
    /// height/offset thresholds we use for Japan Post) as:
    ///
    ///   * `bhs = 0.175`, `bbs = 0`      → Full
    ///   * `bhs = 0.109`, `bbs = 0`      → Descender
    ///   * `bhs = 0.109`, `bbs ≈ 0.0656` → Ascender
    ///   * `bhs = 0.04375`, `bbs ≈ 0.0656` → Tracker
    ///
    /// Since the DAFT encoder is a literal 1:1 char→bar mapping, the
    /// classified bwip-js sequence is identical to the input — but we
    /// still anchor the equivalence so a model change can't drift.
    #[test]
    fn daft_matches_bwip_js() {
        for text in ["DAFT", "DDDD", "AAAA", "FFFF", "TTTT", "DAFTDAFTDAFT"] {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the DAFT bar-sequence
            // corpus row so a divergence points at which case
            // regressed.
            let p = encode_daft(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode_daft({text:?}, default) (DAFT bar-sequence corpus row, 4-char identity, single-letter run, or 3×repetition) must succeed: {e:?}",
                )
            });
            assert_eq!(
                bar_string(&p),
                text,
                "DAFT bar sequence mismatch for {text:?}"
            );
        }
    }

    // ---- KIX --------------------------------------------------------------

    #[test]
    fn kix_known_pattern_for_zero() {
        // ENCS_36[0] = "0033" -> TTFF
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the KIX digit-0 path: KIX_ALPHA[0]='0' → ENCS_36[0]="0033"
        // → 4-bar TTFF.
        let p = encode_kix("0", &Options::default()).expect(
            "encode_kix(\"0\", default) (KIX digit-0 path: KIX_ALPHA[0]='0' → ENCS_36[0]=\"0033\" → 4-bar TTFF) must succeed",
        );
        assert_eq!(bar_string(&p), "TTFF");
    }

    #[test]
    fn kix_two_chars_concatenate() {
        // "01" -> ENCS_36[0] + ENCS_36[1] = "0033" + "0123" -> TTFF TDAF
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the KIX 2-digit concatenation path.
        let p = encode_kix("01", &Options::default()).expect(
            "encode_kix(\"01\", default) (KIX 2-digit concatenation; ENCS_36[0]+ENCS_36[1] → TTFFTDAF) must succeed",
        );
        assert_eq!(bar_string(&p), "TTFFTDAF");
    }

    #[test]
    fn kix_rejects_lowercase_that_is_not_in_alpha() {
        // Lowercase is uppercased; an actual invalid like '#' should reject.
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 132
        // (`KIX: invalid character '#'`), with cross-arm guard
        // against the empty arm.
        match encode_kix("AB#", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("KIX:"), "missing `KIX:` prefix: {msg}");
                assert!(
                    msg.contains("invalid character"),
                    "missing `invalid character` predicate: {msg}"
                );
                assert!(msg.contains("'#'"), "missing '#' char Debug echo: {msg}");
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
            }
            other => panic!("`AB#` should reject as InvalidData, got {other:?}"),
        }
    }

    // ---- Royal Mail -------------------------------------------------------

    #[test]
    fn rm4scc_canonical_payload_with_computed_check() {
        // "LE28HS9Z" is the BWIPP example; encoder appends a computed check
        // and wraps in start/stop sentinels.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the RM4SCC canonical-payload path: BWIPP example
        // "LE28HS9Z" → auto-check + start/stop sentinels → 38 bars
        // (1 + 9×4 + 1).
        let p = encode_royalmail("LE28HS9Z", &Options::default()).expect(
            "encode_royalmail(\"LE28HS9Z\", default) (RM4SCC BWIPP canonical example; auto-check + start/stop sentinels → 38 bars) must succeed",
        );
        // 1 start bar + (8 data + 1 check) * 4 + 1 stop bar = 38 bars total.
        assert_eq!(p.bars.len(), 1 + 9 * 4 + 1);
        assert_eq!(p.bars[0], Bar4State::Ascender); // start "2"
        assert_eq!(p.bars[p.bars.len() - 1], Bar4State::Full); // stop "3"
    }

    /// Stage 11.A8c — pin `pattern_to_bars` digit-to-Bar4State
    /// conversion + both error paths directly. The helper is used
    /// by KIX and RM4SCC encoders through the ENCS_36 / sentinel
    /// patterns; the patterns never contain out-of-range chars in
    /// production, so the `to_digit()` + `from_digit()` error
    /// branches are unreachable from the public API. Direct unit
    /// tests pin all 4 success arms + both error paths.
    ///
    /// Bar4State digit mapping (from src/encoding.rs):
    ///   - '0' → Tracker
    ///   - '1' → Descender
    ///   - '2' → Ascender
    ///   - '3' → Full
    ///   - '4'..'9' → InvalidData "digit must be 0..=3"
    ///   - non-digit → InvalidData "non-digit"
    #[test]
    fn pattern_to_bars_covers_all_arms() {
        // All 4 valid digits in one pattern → accumulates in order.
        let mut out = Vec::new();
        pattern_to_bars("0123", &mut out).unwrap();
        assert_eq!(
            out,
            vec![
                Bar4State::Tracker,
                Bar4State::Descender,
                Bar4State::Ascender,
                Bar4State::Full,
            ],
            "pattern '0123' must map to T,D,A,F in order"
        );

        // Appends rather than overwrites.
        pattern_to_bars("3", &mut out).unwrap();
        assert_eq!(out.len(), 5, "pattern_to_bars must APPEND, not replace");
        assert_eq!(out[4], Bar4State::Full);

        // Out-of-range digit '4'..'9' → InvalidData with the
        // from_digit-specific message.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("must
        // be 0..=3")` upgraded to per-iteration 4-anchor pin:
        //   1. `postal4 pattern` symbology-area prefix
        //   2. `digit must be 0..=3` full predicate (kills truncation
        //      mutations that drop `digit` or the range)
        //   3. per-iteration `got {d}` value echo
        //   4. cross-arm guard: must NOT contain `non-digit` (sibling
        //      arm at line 49 of postal4.rs)
        let mut bad = Vec::new();
        for d in '4'..='9' {
            let pat = d.to_string();
            match pattern_to_bars(&pat, &mut bad) {
                Err(Error::InvalidData(msg)) => {
                    assert!(
                        msg.contains("postal4 pattern"),
                        "missing `postal4 pattern` prefix for '{d}': {msg:?}"
                    );
                    assert!(
                        msg.contains("digit must be 0..=3"),
                        "missing full predicate for '{d}': {msg:?}"
                    );
                    assert!(
                        msg.contains(&format!("got {d}")),
                        "missing per-iteration value echo `got {d}`: {msg:?}"
                    );
                    assert!(
                        !msg.contains("non-digit"),
                        "cross-arm contamination: out-of-range msg mentions `non-digit` for '{d}': {msg:?}"
                    );
                }
                other => panic!("expected InvalidData for '{d}', got {other:?}"),
            }
        }
        // Empty pattern is a no-op success — no bars added.
        // Stage 11.A8c (cont) — descriptive label naming no-op invariant.
        let mut empty_out = Vec::new();
        pattern_to_bars("", &mut empty_out).unwrap();
        assert!(
            empty_out.is_empty(),
            "pattern_to_bars(\"\") must be a no-op (empty input → no bars pushed); got len={}",
            empty_out.len()
        );

        // Non-digit char → "non-digit" error.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("non-
        // digit")` upgraded to 4-anchor pin:
        //   1. `postal4 pattern` symbology-area prefix
        //   2. `contains non-digit` full predicate
        //   3. `'A'` char Debug echo
        //   4. cross-arm guard: must NOT contain `must be 0..=3`
        //      (sibling arm at line 52 of postal4.rs)
        let mut nondigit_out = Vec::new();
        match pattern_to_bars("A", &mut nondigit_out) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("postal4 pattern"),
                    "missing `postal4 pattern` prefix: {msg:?}"
                );
                assert!(
                    msg.contains("contains non-digit"),
                    "missing full predicate `contains non-digit`: {msg:?}"
                );
                assert!(msg.contains("'A'"), "missing char Debug echo 'A': {msg:?}");
                assert!(
                    !msg.contains("must be 0..=3"),
                    "cross-arm contamination: non-digit msg mentions `must be 0..=3`: {msg:?}"
                );
            }
            other => panic!("expected InvalidData(non-digit), got {other:?}"),
        }
    }

    #[test]
    fn rm4scc_check_digit_known_vector() {
        // Indices in RM4SCC_ALPHA ("ZUVWXY501234B6789AHCDEFGNIJKLMTOPQRS"):
        //   S=35, N=24, 3=10, 4=11, R=34, D=20, 1=8, A=17
        // sum_top = 5 + 4 + 1 + 1 + 5 + 3 + 1 + 2 = 22 -> 22 % 6 = 4
        // sum_bot = 5 + 0 + 4 + 5 + 4 + 2 + 2 + 5 = 27 -> 27 % 6 = 3
        // check idx = 4*6 + 3 = 27 -> RM4SCC_ALPHA[27] = 'K'
        assert_eq!(compute_rm4scc_check("SN34RD1A"), 'K');
    }

    /// Stage 11.A8c — pin the validatecheck supplied-check mismatch
    /// arm (line 199-201 of encode_royalmail). The full diagnostic is:
    ///   "Royal Mail RM4SCC: supplied check X does not match computed K"
    ///
    /// encode_royalmail has FOUR InvalidData arms (empty, invalid-char,
    /// validatecheck-too-short, check-mismatch). Variant-only assertion
    /// can't distinguish — a mutant that swaps the mismatch arm body
    /// with the empty or invalid-char message survives.
    #[test]
    fn rm4scc_validate_check_rejects_mismatch() {
        let err = encode_royalmail(
            "SN34RD1AX",
            &Options::default().with("validatecheck", "true"),
        )
        .unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("expected InvalidData for check mismatch; got {err:?}");
        };
        assert!(
            msg.contains("Royal Mail RM4SCC:"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("supplied check X"),
            "diagnostic must echo the supplied check 'X'; got {msg:?}"
        );
        assert!(
            msg.contains("does not match computed K"),
            "diagnostic must echo the computed check 'K'; got {msg:?}"
        );
        assert!(
            !msg.contains("invalid character")
                && !msg.contains("must not be empty")
                && !msg.contains("too short"),
            "mismatch diagnostic must not leak other arms' substrings; got {msg:?}"
        );
    }

    /// Stage 11.A8c — pin the invalid-character arm (line 175-178)
    /// with diagnostic + char-echo pins. The arm produces:
    ///   "Royal Mail RM4SCC: invalid character '!'"
    #[test]
    fn rm4scc_rejects_invalid_character() {
        let err = encode_royalmail("HELLO!", &Options::default()).unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("expected InvalidData for invalid char; got {err:?}");
        };
        assert!(
            msg.contains("Royal Mail RM4SCC:"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("invalid character"),
            "diagnostic must call out 'invalid character'; got {msg:?}"
        );
        assert!(
            msg.contains("'!'"),
            "diagnostic must echo the offending char via {{c:?}}; got {msg:?}"
        );
        assert!(
            !msg.contains("supplied check") && !msg.contains("must not be empty"),
            "invalid-char diagnostic must not leak other arms' substrings; got {msg:?}"
        );
    }

    #[test]
    fn rm4scc_rejects_empty() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 170-172
        // (`Royal Mail RM4SCC payload must not be empty`). Cross-arm
        // guard against the invalid-character arm.
        match encode_royalmail("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Royal Mail RM4SCC"),
                    "missing `Royal Mail RM4SCC` prefix: {msg}"
                );
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("invalid character"),
                    "wrong arm — invalid-character diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Royal Mail RM4SCC should reject as InvalidData, got {other:?}"),
        }
    }

    /// Cross-validation against `b.raw("royalmail", text, {})[0].bhs`
    /// classifying each bar as F/A/D/T from the height/offset pair.
    /// Anchors the start sentinel, 8-char data run, computed check
    /// digit slot, and stop sentinel.
    #[test]
    fn rm4scc_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            ("LE28HS9Z", "AFTTFTFFTTDFATFDADFATFTFTDATFFFTTDATFF"),
            ("SN12AA1A", "AFTFTFDTATDAFTDFADADADADATDAFDADAAFDTF"),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`.
            let p = encode_royalmail(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode_royalmail({text:?}) (Royal Mail 4-State corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                bar_string(&p),
                want,
                "Royal Mail bar sequence mismatch for {text:?}"
            );
        }
    }

    /// Kills the three boundary mutants on line ~185 of `encode_royalmail`:
    ///   - `< 2 -> == 2` (would accept length-1 input and reject length-2),
    ///   - `< 2 -> > 2` (would reject every length ≥ 3 as "too short"),
    ///   - `< 2 -> <= 2` (would reject the minimum length-2 input).
    ///
    /// The `validatecheck=true` branch needs at least 2 chars (1 body
    /// char + 1 supplied check char). For a single-body-char payload
    /// the check digit equals the body char itself (since both `top` =
    /// `idx / 6` and `bot` = `idx % 6` reduce to the same nibble pair
    /// after the mod-6 collapse). We use body="A" → check 'A' (RM4SCC_
    /// ALPHA index 17), so the canonical 2-char validate payload "AA"
    /// must succeed. Then we assert length-1 input is rejected as too
    /// short, and length-3 ("AAW", where 'W' is the computed check for
    /// "AA") is accepted.
    #[test]
    fn rm4scc_validatecheck_length_boundary_is_exactly_two() {
        // Length 2: body="A" + check='A' → must succeed (computed check
        // for body "A" is 'A', because idx_A = 17, top = 2, bot = 5;
        // check_idx = 2*6 + 5 = 17 = 'A' in RM4SCC_ALPHA).
        let opts = Options::default().with("validatecheck", "true");
        encode_royalmail("AA", &opts)
            .expect("validatecheck should accept length-2 payload (1 body + 1 check)");

        // Length 1: error "too short" — the validatecheck branch
        // requires at least 2 chars. The `< 2 -> == 2` mutant would
        // accept this length (returning Ok); the original returns Err.
        // Stage 11.A8c (cont) — upgrade from single-anchor
        // `msg.contains("too short")` to 3-anchor pin matching the
        // source diagnostic at line 187 of postal4.rs:
        //   1. `Royal Mail RM4SCC` symbology prefix (distinguishes
        //      from DAFT / KIX siblings in the same module)
        //   2. `validatecheck` qualifier (specifies the option-mode
        //      that triggered this rejection — not the standalone
        //      length validator)
        //   3. `payload too short` predicate
        match encode_royalmail("A", &opts) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Royal Mail RM4SCC"),
                    "missing `Royal Mail RM4SCC` prefix: {msg}"
                );
                assert!(
                    msg.contains("validatecheck"),
                    "missing `validatecheck` mode qualifier: {msg}"
                );
                assert!(
                    msg.contains("payload too short"),
                    "missing `payload too short` predicate: {msg}"
                );
            }
            other => panic!("expected InvalidData(too short), got {other:?}"),
        }

        // Length 3: body="AA" + check='W'. The `< 2 -> > 2` mutant
        // would reject every length > 2 as "too short"; the original
        // accepts. compute_rm4scc_check("AA"):
        //   idx_A = 17 ⇒ top += 17/6=2, bot += 17%6=5 (×2 for both A's)
        //   sum_top = 4, sum_bot = 10
        //   check_idx = (4%6)*6 + (10%6) = 24 + 4 = 28
        //   RM4SCC_ALPHA[28] = 'L' (verified below)
        assert_eq!(compute_rm4scc_check("AA"), 'L');
        encode_royalmail("AAL", &opts)
            .expect("validatecheck should accept length-3 payload (2 body + 1 check)");
    }

    /// Cross-validation for KIX. Unlike RM4SCC, KIX has no start/stop
    /// sentinels or check digit — each input char maps to 4 bars and
    /// the bars are concatenated directly. Anchors that pure
    /// pass-through behaviour against bwip-js's `raw("kix", ...)`.
    #[test]
    fn kix_matches_bwip_js() {
        let cases: &[(&str, &str)] = &[
            ("1231GA1RS", "TDAFTDFADTAFTDAFDAFTDADATDAFFTADFTFT"),
            ("ABC123", "DADADFTATAFDTDAFTDFADTAF"),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else`.
            let p = encode_kix(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode_kix({text:?}) (KIX 4-State corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(
                bar_string(&p),
                want,
                "KIX bar sequence mismatch for {text:?}"
            );
        }
    }
}
