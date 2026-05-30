//! MSI / MSI Plessey.
//!
//! Numeric-only (0..9). Each digit is encoded as 4 bits BCD, MSB first,
//! where each bit becomes a (wide-bar, narrow-space) for `1` or a
//! (narrow-bar, wide-space) for `0`. The symbol is bracketed by a
//! start guard (`21` — wide-bar + narrow-space) and a stop guard
//! (`121` — narrow-bar + wide-space + narrow-bar).
//!
//! Optional check digits (mod-10 and/or mod-11) are appended when
//! requested via `checktype` (`none` | `mod10` | `mod1010` | `mod11` |
//! `mod1110`). The default in BWIPP is `unset` which means no check.
//!
//! Patterns ported from bwip-js `bwipp_msi` digit encoding table.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// Per-digit run-length pattern. Each entry is 8 elements alternating
/// bar/space; digits map to BCD bit patterns. Width units: 1 = narrow,
/// 2 = wide.
const DIGIT_PATTERNS: [&str; 10] = [
    "12121212", "12121221", "12122112", "12122121", "12211212", "12211221", "12212112", "12212121",
    "21121212", "21121221",
];

/// Start pattern (`21`) — wide bar + narrow space.
const START: &str = "21";
/// Stop pattern (`121`) — narrow bar + wide space + narrow bar.
const STOP: &str = "121";

/// Encode an MSI payload.
///
/// Recognized options:
///   * `checktype` = `none` | `mod10` | `mod1010` | `mod11` | `mod1110`
///     (default `none`).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Default: no check digit.
/// let svg = render_svg(Symbology::Msi, "123456", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
///
/// // With a mod-10 check digit appended.
/// let mut opts = Options::default();
/// opts.extras.push(("checktype".into(), "mod10".into()));
/// let svg = render_svg(Symbology::Msi, "123456", &opts).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData("MSI payload must not be empty".into()));
    }
    for c in data.chars() {
        if !c.is_ascii_digit() {
            return Err(Error::InvalidData(format!(
                "MSI accepts digits only (got {c:?})"
            )));
        }
    }
    if data.chars().count() > 254 {
        return Err(Error::InvalidData(
            "MSI payload exceeds BWIPP's 254-char limit".into(),
        ));
    }

    // `includecheck: "true"` is BWIPP's high-level switch — when set,
    // a check digit is appended according to `checktype` (which
    // defaults to `mod10` per BWIPP). When `includecheck` is absent
    // we still honour an explicit `checktype` (legacy of this
    // module's pre-`includecheck` interface).
    let include_check = opts.get("includecheck").is_some_and(|v| v == "true");
    let check_type = if include_check {
        opts.get("checktype").unwrap_or("mod10")
    } else {
        opts.get("checktype").unwrap_or("none")
    };
    let mut full = data.to_string();
    match check_type {
        "none" => {}
        "mod10" => {
            full.push(mod10(&full));
        }
        "mod1010" => {
            full.push(mod10(&full));
            full.push(mod10(&full));
        }
        "mod11" => {
            full.push(mod11(&full));
        }
        "mod1110" => {
            full.push(mod11(&full));
            full.push(mod10(&full));
        }
        other => {
            return Err(Error::InvalidOption(format!(
                "MSI checktype must be none, mod10, mod1010, mod11, or mod1110 (got {other})"
            )));
        }
    }

    let mut runs: Vec<u8> = Vec::new();
    push_runs(&mut runs, START);
    for c in full.chars() {
        let d = c.to_digit(10).unwrap() as usize;
        push_runs(&mut runs, DIGIT_PATTERNS[d]);
    }
    push_runs(&mut runs, STOP);

    let text = if opts.include_text { Some(full) } else { None };
    Ok(LinearPattern { bars: runs, text })
}

fn push_runs(out: &mut Vec<u8>, pattern: &str) {
    // MSI run-length strings use digit `1` = narrow, `2` = wide. The very
    // first run is always a bar (the start guard begins with a wide bar).
    for c in pattern.chars() {
        out.push(c.to_digit(10).unwrap() as u8);
    }
}

/// MSI mod-10 check digit (Luhn-style: split into odd-position digits,
/// double those, sum, then sum even-position digits, total mod 10, complement).
fn mod10(digits: &str) -> char {
    // Build the "odd" sequence by taking every other char starting from the
    // rightmost. BWIPP's algorithm:
    //   1) Concatenate odd-indexed digits (1-based from right), double the
    //      resulting integer, then sum the digits of that doubled number.
    //   2) Add the sum of the even-indexed digits.
    //   3) check = (10 - (total mod 10)) mod 10.
    let chars: Vec<char> = digits.chars().collect();
    let mut odd_concat = String::new();
    let mut even_sum = 0u32;
    for (i, c) in chars.iter().rev().enumerate() {
        if i % 2 == 0 {
            odd_concat.push(*c);
        } else {
            even_sum += c.to_digit(10).unwrap();
        }
    }
    let odd_reversed: String = odd_concat.chars().rev().collect();
    let doubled: u32 = odd_reversed.parse::<u32>().unwrap_or(0) * 2;
    let mut odd_sum = 0u32;
    for d in doubled.to_string().chars() {
        odd_sum += d.to_digit(10).unwrap();
    }
    let total = odd_sum + even_sum;
    char::from_digit((10 - total % 10) % 10, 10).unwrap()
}

/// MSI mod-11 check digit (IBM variant: weights cycle 2..=7, sum mod 11,
/// complement). If the result is 10, BWIPP renders it as `0`.
fn mod11(digits: &str) -> char {
    let mut sum: u32 = 0;
    let mut weight = 2u32;
    for c in digits.chars().rev() {
        sum += c.to_digit(10).unwrap() * weight;
        weight = if weight == 7 { 2 } else { weight + 1 };
    }
    let check = (11 - sum % 11) % 11;
    if check == 10 {
        '0'
    } else {
        char::from_digit(check, 10).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_digits() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 59-61 of msi.rs:
        //   1. `MSI` symbology prefix
        //   2. `accepts digits only` predicate (NOT the empty-payload
        //      or 254-char-limit phrasing — discriminates the
        //      per-char digit-check path from the two sibling
        //      InvalidData branches at lines 55 and 65)
        //   3. `'A'` Debug echo of the offending char (custinfo "12A5"
        //      → the 'A' at idx 2 is the first non-digit; the loop
        //      hits it and bails)
        match encode("12A5", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("MSI"), "missing MSI prefix: {msg}");
                assert!(
                    msg.contains("accepts digits only"),
                    "missing `accepts digits only` predicate: {msg}"
                );
                assert!(msg.contains("'A'"), "missing 'A' Debug echo: {msg}");
                // Cross-arm contamination guard: must NOT carry the
                // empty-payload or length-cap phrasing.
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into per-char reject: {msg}"
                );
                assert!(
                    !msg.contains("254-char limit"),
                    "wrong arm — length-cap diagnostic leaked into per-char reject: {msg}"
                );
            }
            other => panic!("\"12A5\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_payload() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 55 of msi.rs:
        //   1. `MSI` symbology prefix
        //   2. `must not be empty` predicate (discriminates from the
        //      `accepts digits only` and `254-char limit` arms)
        // The empty-payload diagnostic carries no value to echo, so
        // 2 anchors is the maximum content available; the
        // cross-arm-contamination guards strengthen the signal.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("MSI"), "missing MSI prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("accepts digits only"),
                    "wrong arm — digits-only diagnostic leaked into empty-payload reject: {msg}"
                );
                assert!(
                    !msg.contains("254-char limit"),
                    "wrong arm — length-cap diagnostic leaked into empty-payload reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn encodes_digit_zero_with_known_run_lengths() {
        // START "21" (2 runs) + digit '0' "12121212" (8 runs) + STOP "121"
        // (3 runs) = 13 run-length elements total.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MSI digit-0 known-run-lengths path: START "21" + '0'
        // "12121212" + STOP "121" = 13 runs.
        let p = encode("0", &Options::default()).expect(
            "encode(\"0\", default) (MSI digit-0 known-run-lengths path: START + '0' + STOP = 13 runs) must succeed",
        );
        let expected: Vec<u8> = "2112121212121"
            .chars()
            .map(|c| c.to_digit(10).unwrap() as u8)
            .collect();
        assert_eq!(p.bars, expected);
    }

    #[test]
    fn mod10_known_vector() {
        // BWIPP test: msi mod-10 of "1234567" should be 4
        // (Luhn-style: 1234567 reversed → odd ["7","5","3","1"], even sum = 6+4+2 = 12;
        //  reversed odd "1357" doubled = 2714 → digit-sum 2+7+1+4 = 14; total = 26; check = 4).
        assert_eq!(mod10("1234567"), '4');
    }

    #[test]
    fn mod11_known_vector() {
        // For "1234567", BWIPP mod-11 with weights 2..=7 cycling:
        // 7*2 + 6*3 + 5*4 + 4*5 + 3*6 + 2*7 + 1*2 = 14+18+20+20+18+14+2 = 106
        // check = (11 - 106 % 11) % 11 = (11 - 7) % 11 = 4
        assert_eq!(mod11("1234567"), '4');
    }

    /// Stage 11.A8c — pin `mod11`'s `check == 10` special-case branch
    /// at line 164-165. BWIPP renders check=10 as `'0'` (not '10', not
    /// '0'..='9' via char::from_digit which panics for value 10 at
    /// radix 10). The existing `mod11_known_vector` uses "1234567"
    /// whose check is 4 — the special case never fires.
    ///
    /// Find an input where weighted-sum mod 11 == 1 so check==10:
    ///   * "06" → reversed ['6','0'] with weights [2, 3]
    ///         → sum = 6*2 + 0*3 = 12
    ///         → check = (11 - 12 % 11) % 11 = (11 - 1) % 11 = 10
    ///         → branch hits → returns '0'.
    ///
    /// Also pin the `check % 11 == 0` normal-path '0' as a distinct
    /// anchor:
    ///   * "0" → sum = 0 → check = (11 - 0) % 11 = 0
    ///         → falls through to `char::from_digit(0, 10).unwrap()`
    ///         → returns '0'. Mutation removing the special-case
    ///         branch would still pass "0" but panic on "06".
    ///
    /// Mutations killed:
    ///   * `if check == 10` → `if check == 11` (or arm removed):
    ///     "06" path falls through to `char::from_digit(10, 10).unwrap()`
    ///     which panics — distinct failure vs `assert_eq!`.
    ///   * Branch body `'0'` → `'9'` / `'A'` / any other char: caught
    ///     directly.
    ///   * `(11 - sum % 11) % 11` → `(11 - sum % 11)`: for sum=12 →
    ///     check=10 still, but for sum=0 (input "0"), `(11 - 0)` = 11
    ///     which is not 10, falls through to char::from_digit(11)
    ///     panicking — caught by "0" anchor.
    #[test]
    fn mod11_check_ten_special_case_returns_zero() {
        // Special-case branch: weighted sum mod 11 == 1 → check=10 → '0'.
        assert_eq!(
            mod11("06"),
            '0',
            "mod11(\"06\") triggers check==10 special-case → '0'"
        );

        // Normal-path '0': weighted sum mod 11 == 0 → check=0 → '0'.
        // Distinguishes the special-case branch from `% 11` math
        // mutations.
        assert_eq!(mod11("0"), '0', "mod11(\"0\") → check=0 normal-path → '0'");

        // Another sum%11==0 anchor with a longer input: "076" reversed
        // ['6','7','0'] weights [2,3,4]: sum=12+21+0=33; 33%11=0;
        // check=(11-0)%11=0 → '0'. Cross-checks the normal-path '0'
        // is reachable via a non-trivial weighting sequence.
        assert_eq!(
            mod11("076"),
            '0',
            "mod11(\"076\") → check=0 (sum=33, 33%11=0) → '0'"
        );
    }

    #[test]
    fn checktype_mod10_appends_one_digit() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MSI checktype=mod10 length-delta path: 7-digit payload
        // adds 1 mod-10 check digit (61 → 69 runs).
        let plain = encode("1234567", &Options::default()).expect(
            "encode(\"1234567\", default) (MSI plain 7-digit baseline for checktype=mod10 length-delta cross-check; 7×8+5=61 runs) must succeed",
        );
        let with_check = encode("1234567", &Options::default().with("checktype", "mod10")).expect(
            "encode(\"1234567\", checktype=mod10) (MSI mod10 path: appends 1 mod-10 check digit; 8×8+5=69 runs) must succeed",
        );
        // 8 elements per digit + 2-element start + 3-element stop:
        //   plain: 7 * 8 + 2 + 3 = 61
        //   with_check: 8 * 8 + 2 + 3 = 69
        assert_eq!(plain.bars.len(), 61);
        assert_eq!(with_check.bars.len(), 69);
    }

    #[test]
    fn invalid_checktype_rejected() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidOption(_)))` to 3-anchor pin
        // matching the source diagnostic at line 99-101 of msi.rs:
        //   1. `MSI checktype must be` prefix
        //   2. `none, mod10, mod1010, mod11, or mod1110` full alphabet
        //      list — pins every valid checktype name so a mutation
        //      that drops one alphabet entry fails the substring
        //      check
        //   3. `mod99` value echo of the offending checktype (note
        //      bare `{other}` not `{other:?}` — no quotes around the
        //      echo)
        match encode("123", &Options::default().with("checktype", "mod99")) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("MSI checktype must be"),
                    "missing `MSI checktype must be` prefix: {msg}"
                );
                assert!(
                    msg.contains("none, mod10, mod1010, mod11, or mod1110"),
                    "missing full checktype alphabet list: {msg}"
                );
                assert!(msg.contains("mod99"), "missing `mod99` value echo: {msg}");
            }
            other => panic!("checktype=mod99 should reject as InvalidOption, got {other:?}"),
        }
    }

    /// Golden bar pattern for `"12345"` captured from bwip-js's
    /// `raw("msi", "12345", {})[0].sbs`. MSI uses a 1:2 narrow:wide
    /// ratio in both BWIPP and our default — no config needed.
    #[test]
    fn matches_bwip_js_raw_sbs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MSI byte-for-byte bwip-js SBS oracle path: 5-digit
        // "12345" with default 1:2 narrow:wide ratio.
        let p = encode("12345", &Options::default()).expect(
            "encode(\"12345\", default) (MSI byte-for-byte SBS bwip-js raw oracle; 5-digit + 1:2 narrow:wide ratio) must succeed",
        );
        let want: [u8; 45] = [
            2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1, 1, 2, 2,
            1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 1,
        ];
        assert_eq!(p.bars, want, "msi bars mismatch vs bwip-js raw output");
    }

    /// Each `checktype` option appends a different mod-10 / mod-11
    /// check digit (or two, for the doubled variants), exercising the
    /// four distinct check paths. Goldens captured from
    /// `b.raw("msi", "1234567", {checktype, includecheck: true})[0].sbs`.
    #[test]
    fn checktype_variants_match_bwipp() {
        let cases: &[(&str, &[u8])] = &[
            (
                "mod10",
                &[
                    2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1,
                    1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2,
                    2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 1,
                ],
            ),
            (
                "mod1010",
                &[
                    2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1,
                    1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2,
                    2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1,
                ],
            ),
            (
                "mod11",
                &[
                    2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1,
                    1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2,
                    2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 1,
                ],
            ),
            (
                "mod1110",
                &[
                    2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1,
                    1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2,
                    2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 2, 1, 1, 2, 1,
                ],
            ),
        ];
        for &(checktype, want) in cases {
            let opts = Options::default().with("checktype", checktype);
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the MSI checktype
            // corpus row so a regression points at the variant.
            let got = encode("1234567", &opts).unwrap_or_else(|e| {
                panic!(
                    "encode(\"1234567\", checktype={checktype:?}) (MSI checktype corpus: none/mod10/mod1010/mod11/mod1110 algorithm row) must succeed: {e:?}",
                )
            });
            assert_eq!(
                got.bars, want,
                "MSI sbs mismatch for checktype={checktype:?}"
            );
        }
    }

    /// BWIPP also accepts `includecheck: true` (without `checktype`)
    /// and defaults the check algorithm to `mod10`. Match that
    /// behaviour: `{includecheck: true}` alone should produce the
    /// same output as `{checktype: "mod10"}`.
    #[test]
    fn includecheck_defaults_to_mod10() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MSI includecheck-defaults-to-mod10 equivalence path.
        let a = encode("1234567", &Options::default().with("includecheck", "true")).expect(
            "encode(\"1234567\", includecheck=true) (MSI includecheck=true: must default to mod10 algorithm) must succeed",
        );
        let b = encode("1234567", &Options::default().with("checktype", "mod10")).expect(
            "encode(\"1234567\", checktype=mod10) (MSI checktype=mod10 explicit reference for includecheck-defaults cross-check) must succeed",
        );
        assert_eq!(a.bars, b.bars);
    }

    /// Kills `encode: replace > with ==` and `> with >=` at line ~64
    /// (the BWIPP-inherited 254-char MSI payload-length cap). The
    /// original test corpus used short payloads only; the mutants
    /// flipped the inequality so either exactly-254 (accepted by spec)
    /// is rejected, or only exactly-254 is rejected. We bracket the
    /// boundary at 253/254/255.
    #[test]
    fn payload_length_cap_is_strictly_two_hundred_fifty_four() {
        // Stage 11.A8c (cont) — upgrade the 255-char reject arm from
        // discriminant-only `matches!(_, Err(Error::InvalidData(_)))`
        // to multi-anchor pin matching the source diagnostic at line
        // 65-67 of msi.rs:
        //   1. `MSI` symbology prefix
        //   2. `254-char limit` predicate (pins the specific length
        //      cap — a mutation that changes 254 to 253 or 255 would
        //      shift the boundary; the 253/254 `.is_ok()` assertions
        //      already pin the lower side of the boundary)
        //   3. Cross-arm contamination guards against the empty-
        //      payload and per-char-digit InvalidData arms.
        let two_53 = "1".repeat(253);
        let two_54 = "1".repeat(254);
        let two_55 = "1".repeat(255);
        // Stage 11.A8c (cont) — descriptive labels naming MSI 254-cap
        // boundary direction (brackets `<= 254` ↔ `< 254` mutation pair).
        assert!(
            encode(&two_53, &Options::default()).is_ok(),
            "encode(253-char MSI) must accept (one below 254-cap; kills `< 254` → `<= 253` boundary mutant)"
        );
        assert!(
            encode(&two_54, &Options::default()).is_ok(),
            "encode(254-char MSI) must accept (exactly at 254-cap; kills `<= 254` → `< 254` boundary mutant)"
        );
        match encode(&two_55, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("MSI"), "missing MSI prefix: {msg}");
                assert!(
                    msg.contains("254-char limit"),
                    "missing `254-char limit` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked into length-cap reject: {msg}"
                );
                assert!(
                    !msg.contains("accepts digits only"),
                    "wrong arm — digits-only diagnostic leaked into length-cap reject: {msg}"
                );
            }
            other => panic!(
                "255-char payload should reject as InvalidData with length-cap diagnostic, got {other:?}"
            ),
        }
    }

    /// Stage 11.A8c — pin `mod10` IBM Luhn variant + `mod11` weighted
    /// variant for hand-computed inputs. Kills `* 2` doubling / `% 10`
    /// / `* weight` / `weight cycle 2..7` arithmetic mutations on
    /// lines 138-151 / 156-161.
    #[test]
    fn mod10_and_mod11_known_values() {
        // mod10 BWIPP IBM variant: digits = "12345":
        //   right-to-left chars: '5','4','3','2','1'.
        //   i=0 '5' odd-concat; i=1 '4' even_sum=4; i=2 '3' odd-concat;
        //   i=3 '2' even_sum=6; i=4 '1' odd-concat.
        //   odd_concat="531" reversed="135".
        //   doubled = 135 * 2 = 270. digit sum = 2+7+0 = 9.
        //   total = 9 + 6 = 15. check = (10 - 15%10) % 10 = (10-5)%10 = 5.
        assert_eq!(mod10("12345"), '5');

        // mod10("0") = ?
        //   chars rev: '0'. i=0 odd. odd_concat="0". reversed="0".
        //   doubled = 0*2 = 0. odd_sum = 0. even_sum = 0. total = 0.
        //   check = (10-0)%10 = 0 → '0'.
        assert_eq!(mod10("0"), '0');

        // mod11("123") IBM variant weights 2..=7 cycle, weighted from right:
        //   right '3'*2 = 6, '2'*3=6, '1'*4=4. sum=16.
        //   check = (11 - 16%11) % 11 = (11-5) %11 = 6 → '6'.
        // Wait — check the implementation: total = sum, check = (11 - sum%11) %11.
        // Let me look back. Actually mod11 is `(11 - sum%11) % 11`, but if = 10
        // it renders as '0'. Let me just verify our impl.
        let got = mod11("123");
        // Just pin the result rather than recompute every arithmetic.
        // The value should be a digit char '0'..='9'.
        assert!(got.is_ascii_digit(), "mod11 must return digit, got {got:?}");

        // mod11("0") = (11 - 0%11) %11 = 11%11 = 0 → '0'.
        assert_eq!(mod11("0"), '0');

        // mod11("") = empty: sum=0 → '0'.
        assert_eq!(mod11(""), '0');
    }

    /// Stage 11.A8c — strengthen `mod11` by replacing the weak
    /// `is_ascii_digit()` assertion on "123" with the actual hand-
    /// computed digit, and exercise the `check == 10 → '0'` collapse
    /// branch which the existing tests never trigger (all their
    /// `sum % 11 == 0` cases collapse via the outer `% 11`, not via
    /// the explicit `if check == 10`).
    ///
    /// Hand-computed anchors:
    ///   - mod11("123"): rev "321"; 3*2+2*3+1*4 = 16; (11-16%11)%11
    ///     = (11-5)%11 = 6 → '6'.
    ///   - mod11("6"): single digit; sum=6*2=12; (11-12%11)%11
    ///     = (11-1)%11 = 10 → branches into the `check == 10` arm
    ///     which renders '0'.
    ///   - mod11("17"): rev "71"; 7*2+1*3 = 17; (11-17%11)%11
    ///     = (11-6)%11 = 5 → '5'. Distinct two-digit anchor.
    ///
    /// Mutations to catch:
    ///   - `check == 10` → `check == 11` or `check == 0`: the "6"
    ///     anchor flips from '0' to a different char.
    ///   - `weight + 1` → `weight - 1` or `weight + 2`: the "17"
    ///     anchor changes (the existing "123" anchor pins the same
    ///     mutation, just from a different angle).
    ///   - removal of the `if check == 10` branch: returns whatever
    ///     `char::from_digit(10, 10)` does (None → panic).
    #[test]
    fn mod11_explicit_anchors_including_check10_branch() {
        assert_eq!(
            mod11("123"),
            '6',
            "rev=321: 3*2+2*3+1*4=16; (11-16%11)%11=6"
        );
        assert_eq!(
            mod11("6"),
            '0',
            "sum=12, check=10 must render as '0' (explicit collapse arm)"
        );
        assert_eq!(mod11("17"), '5', "rev=71: 7*2+1*3=17; (11-17%11)%11=5");
    }
}
