//! EAN / UPC with 2- or 5-digit add-on supplemental barcodes.
//!
//! BWIPP supports add-ons via a space-separated input format:
//!
//!   `<main_digits> <addon_digits>`
//!
//! The renderer encodes the main symbol and the add-on symbol side by side
//! with a quiet zone between them (9 modules by default). Both symbols use
//! their normal layouts; the only combination work is concatenating their
//! [`LinearPattern`]s with the appropriate inter-symbol gap.
//!
//! This module ships eight catalog entries (`ean13p2`, `ean13p5`, `ean8p2`,
//! `ean8p5`, `upcap2`, `upcap5`, `upcep2`, `upcep5`) plus two ISBN/ISSN
//! variants (`isbn13p5`, `issnp2`).

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

use super::book_codes;
use super::ean;
use super::ean_addons;

/// Modules of quiet space inserted between the main barcode and the
/// add-on. BWIPP defaults to 12 modules (its `addongap` option's
/// initial value); the GS1 spec calls for ≥ 7. We follow BWIPP for
/// byte-for-byte cross-validation.
const ADDON_GAP_MODULES: u8 = 12;

/// Split user input into `(main, addon)`, validating that the add-on is
/// exactly `expected_addon` digits long.
fn split_pair(data: &str, expected_addon: usize) -> Result<(&str, &str), Error> {
    let trimmed = data.trim();
    let mut parts = trimmed.split_whitespace();
    let main = parts
        .next()
        .ok_or_else(|| Error::InvalidData("EAN/UPC add-on: missing main barcode data".into()))?;
    let addon = parts
        .next()
        .ok_or_else(|| Error::InvalidData("EAN/UPC add-on: missing add-on digits".into()))?;
    if parts.next().is_some() {
        return Err(Error::InvalidData(
            "EAN/UPC add-on: expected exactly two whitespace-separated fields".into(),
        ));
    }
    if addon.chars().filter(|c| c.is_ascii_digit()).count() != expected_addon {
        return Err(Error::InvalidData(format!(
            "EAN/UPC add-on: expected {expected_addon}-digit add-on, got {addon:?}"
        )));
    }
    Ok((main, addon))
}

/// Concatenate `main` and `addon` LinearPatterns with a quiet-zone gap
/// between them. Handles bar/space alternation correctly so the resulting
/// pattern stays well-formed.
fn combine(main: LinearPattern, addon: LinearPattern, gap: u8) -> LinearPattern {
    let mut bars = main.bars;
    // bars[i] is a bar when i is even, space when i is odd.
    if bars.is_empty() {
        return LinearPattern {
            bars: addon.bars,
            text: addon.text,
        };
    }
    let last_is_bar = (bars.len() - 1) % 2 == 0;
    if last_is_bar {
        // Append a space of width `gap`, then addon's bars (which start with
        // a bar — so the alternation continues correctly).
        bars.push(gap);
    } else {
        // Last element is a space; extend it by `gap`. Then addon's bars
        // start with a bar, continuing alternation.
        *bars.last_mut().unwrap() += gap;
    }
    bars.extend_from_slice(&addon.bars);
    let text = match (main.text, addon.text) {
        (Some(m), Some(a)) => Some(format!("{m} {a}")),
        (Some(m), None) => Some(m),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    };
    LinearPattern { bars, text }
}

// ---------------------------------------------------------------------------
// EAN-13
// ---------------------------------------------------------------------------

/// Encode EAN-13 with a 2-digit add-on.
pub fn encode_ean13_p2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 2)?;
    let m = ean::encode_ean13(main, opts)?;
    let a = ean_addons::encode_ean2(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

/// Encode EAN-13 with a 5-digit add-on.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Main barcode + add-on are whitespace-separated. The EAN-13 form below
/// // is "Hello, world!"'s ISBN-13 followed by an MSRP 5-digit add-on.
/// let svg = render_svg(Symbology::Ean13P5, "9780131103627 52250", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_ean13_p5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 5)?;
    let m = ean::encode_ean13(main, opts)?;
    let a = ean_addons::encode_ean5(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

// ---------------------------------------------------------------------------
// EAN-8
// ---------------------------------------------------------------------------

/// Encode EAN-8 with a 2-digit add-on.
pub fn encode_ean8_p2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 2)?;
    let m = ean::encode_ean8(main, opts)?;
    let a = ean_addons::encode_ean2(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

/// Encode EAN-8 with a 5-digit add-on.
pub fn encode_ean8_p5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 5)?;
    let m = ean::encode_ean8(main, opts)?;
    let a = ean_addons::encode_ean5(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

// ---------------------------------------------------------------------------
// UPC-A
// ---------------------------------------------------------------------------

/// Encode UPC-A with a 2-digit add-on.
pub fn encode_upca_p2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 2)?;
    let m = ean::encode_upca(main, opts)?;
    let a = ean_addons::encode_ean2(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

/// Encode UPC-A with a 5-digit add-on.
pub fn encode_upca_p5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 5)?;
    let m = ean::encode_upca(main, opts)?;
    let a = ean_addons::encode_ean5(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

// ---------------------------------------------------------------------------
// UPC-E
// ---------------------------------------------------------------------------

/// Encode UPC-E with a 2-digit add-on.
pub fn encode_upce_p2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 2)?;
    let m = ean::encode_upce(main, opts)?;
    let a = ean_addons::encode_ean2(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

/// Encode UPC-E with a 5-digit add-on.
pub fn encode_upce_p5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 5)?;
    let m = ean::encode_upce(main, opts)?;
    let a = ean_addons::encode_ean5(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

// ---------------------------------------------------------------------------
// ISBN with 5-digit add-on, ISSN with 2-digit add-on
// ---------------------------------------------------------------------------

/// Encode ISBN-13 with a 5-digit add-on (the standard book-pricing add-on).
pub fn encode_isbn_p5(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 5)?;
    let m = book_codes::encode_isbn(main, opts)?;
    let a = ean_addons::encode_ean5(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

/// Encode ISSN with a 2-digit add-on.
pub fn encode_issn_p2(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (main, addon) = split_pair(data, 2)?;
    let m = book_codes::encode_issn(main, opts)?;
    let a = ean_addons::encode_ean2(addon, opts)?;
    Ok(combine(m, a, ADDON_GAP_MODULES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_requires_exact_addon_length() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin matching the source diagnostic at line
        // 47-49 of ean_combined.rs (`EAN/UPC add-on: expected
        // {expected_addon}-digit add-on, got {addon:?}`):
        //   1. `EAN/UPC add-on:` prefix
        //   2. `expected 2-digit add-on` predicate (pins the
        //      `{expected_addon}` interpolation)
        //   3. `"1"` Debug echo of the supplied addon
        match split_pair("012345678905 1", 2) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN/UPC add-on:"),
                    "missing `EAN/UPC add-on:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected 2-digit add-on"),
                    "missing `expected 2-digit add-on` predicate: {msg}"
                );
                assert!(msg.contains("\"1\""), "missing `\"1\"` Debug echo: {msg}");
                assert!(
                    !msg.contains("missing main"),
                    "wrong arm — missing-main diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("exactly two"),
                    "wrong arm — extra-tokens diagnostic leaked: {msg}"
                );
            }
            other => {
                panic!("\"012345678905 1\" should reject as InvalidData, got {other:?}")
            }
        }
    }

    #[test]
    fn split_rejects_extra_tokens() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 2-anchor pin matching the source diagnostic at line
        // 42-44 of ean_combined.rs (`EAN/UPC add-on: expected
        // exactly two whitespace-separated fields`):
        //   1. `EAN/UPC add-on:` prefix
        //   2. `expected exactly two whitespace-separated fields`
        //      predicate
        match split_pair("012345678905 12 99", 2) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("EAN/UPC add-on:"),
                    "missing `EAN/UPC add-on:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected exactly two whitespace-separated fields"),
                    "missing `expected exactly two whitespace-separated fields` predicate: {msg}"
                );
                assert!(
                    !msg.contains("digit add-on"),
                    "wrong arm — addon-length diagnostic leaked: {msg}"
                );
            }
            other => panic!("\"012345678905 12 99\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `combine` bar/space alternation logic:
    ///   * Empty main → returns addon untouched.
    ///   * Main ends on a bar (even-index last) → push `gap` as new
    ///     space, then extend with addon bars.
    ///   * Main ends on a space (odd-index last) → extend that space
    ///     width by `gap`, then extend with addon bars.
    ///   * Text combination rules: Some/Some → "{m} {a}"; Some/None
    ///     → main; None/Some → addon; None/None → None.
    ///
    /// Mutations caught:
    ///   * `(bars.len() - 1) % 2 == 0` → `!= 0` swaps the two
    ///     branches, producing wrong bar count.
    ///   * `bars.push(gap)` vs `*bars.last_mut().unwrap() += gap`
    ///     swap → end-on-bar test would get wrong widths.
    ///   * `is_empty` guard removed would push gap before addon when
    ///     main is empty (extra spurious bar).
    #[test]
    fn combine_alternation_branches_pin_layout() {
        // End-on-bar: main bars [3, 2, 5] (bar/space/bar), addon
        // [1, 1, 1] (bar/space/bar), gap=12.
        // last index 2, even → bar → push gap as space.
        let main = LinearPattern {
            bars: vec![3, 2, 5],
            text: Some("M".into()),
        };
        let addon = LinearPattern {
            bars: vec![1, 1, 1],
            text: Some("A".into()),
        };
        let got = combine(main, addon, 12);
        assert_eq!(
            got.bars,
            vec![3, 2, 5, 12, 1, 1, 1],
            "end-on-bar → push gap as new space"
        );
        assert_eq!(got.text, Some("M A".into()));

        // End-on-space: main [3, 2, 5, 1] (bar/space/bar/space),
        // addon [1, 1, 1], gap=12. last index 3, odd → space →
        // extend last width.
        let main = LinearPattern {
            bars: vec![3, 2, 5, 1],
            text: None,
        };
        let addon = LinearPattern {
            bars: vec![1, 1, 1],
            text: Some("A".into()),
        };
        let got = combine(main, addon, 12);
        assert_eq!(
            got.bars,
            vec![3, 2, 5, 13, 1, 1, 1],
            "end-on-space → extend last space width"
        );
        assert_eq!(got.text, Some("A".into()), "None+Some → addon text");

        // Empty main → addon returned as-is.
        let main = LinearPattern {
            bars: vec![],
            text: Some("M".into()),
        };
        let addon = LinearPattern {
            bars: vec![1, 2, 3],
            text: None,
        };
        let got = combine(main, addon, 12);
        assert_eq!(got.bars, vec![1, 2, 3]);
        assert_eq!(got.text, None, "empty-main branch keeps addon.text only");

        // None/None text combination.
        let got = combine(
            LinearPattern {
                bars: vec![1],
                text: None,
            },
            LinearPattern {
                bars: vec![1],
                text: None,
            },
            12,
        );
        assert_eq!(got.text, None);
    }

    #[test]
    fn ean13_with_2_digit_addon_combines_widths() {
        let m_only = ean::encode_ean13("012345678905", &Options::default())
            .unwrap()
            .total_width();
        let a_only = ean_addons::encode_ean2("12", &Options::default())
            .unwrap()
            .total_width();
        let combined = encode_ean13_p2("012345678905 12", &Options::default())
            .unwrap()
            .total_width();
        // Combined width = main + gap + addon (no overlap).
        assert_eq!(combined, m_only + u32::from(ADDON_GAP_MODULES) + a_only);
    }

    #[test]
    fn ean13_with_5_digit_addon_renders() {
        // Stage 11.A8c (cont) — strengthen bare width assert with
        // descriptive label naming input. A mutation that returns an
        // empty bars vec from encode_ean13_p5 would otherwise fail
        // with just "assertion failed: p.total_width() > 0".
        let p = encode_ean13_p5("012345678905 12345", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_ean13_p5(\"012345678905 12345\").total_width() must be > 0 — main EAN-13 + 5-digit add-on must compose into a non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn ean8_with_addon() {
        let p2 = encode_ean8_p2("1234567 12", &Options::default()).unwrap();
        let p5 = encode_ean8_p5("1234567 12345", &Options::default()).unwrap();
        assert!(p5.total_width() > p2.total_width());
    }

    #[test]
    fn upca_with_addons() {
        encode_upca_p2("01234567890 12", &Options::default()).unwrap();
        encode_upca_p5("01234567890 12345", &Options::default()).unwrap();
    }

    #[test]
    fn upce_with_addons() {
        encode_upce_p2("01234565 12", &Options::default()).unwrap();
        encode_upce_p5("01234565 12345", &Options::default()).unwrap();
    }

    #[test]
    fn isbn_p5_combines() {
        // Stage 11.A8c (cont) — descriptive label naming input.
        let p = encode_isbn_p5("978-1-56619-909-4 50995", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_isbn_p5(\"978-1-56619-909-4 50995\") must compose ISBN-13 + 5-digit price add-on into non-empty symbol; got {}",
            p.total_width()
        );
    }

    #[test]
    fn issn_p2_combines() {
        // Stage 11.A8c (cont) — descriptive label naming input.
        let p = encode_issn_p2("0317-8471 13", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode_issn_p2(\"0317-8471 13\") must compose ISSN + 2-digit issue-code add-on into non-empty symbol; got {}",
            p.total_width()
        );
    }

    /// ISBN-13 + 5-digit add-on cross-validation against
    /// `b.raw("isbn", "<isbn> <addon>", {})[0].sbs`. The 12-module
    /// gap between the main and add-on is the BWIPP default
    /// (`ADDON_GAP_MODULES`) — a previous checkpoint fixed it from
    /// the spec's "≥9 modules" to BWIPP's "exactly 12".
    #[test]
    fn isbn_p5_matches_bwipp_sbs() {
        let cases: &[(&str, &[u8])] = &[
            (
                "978-1-56619-909-4 50995",
                &[
                    1, 1, 1, 1, 3, 1, 2, 3, 1, 2, 1, 1, 2, 2, 2, 1, 2, 3, 1, 4, 1, 1, 1, 1, 1, 1,
                    4, 1, 1, 1, 1, 1, 2, 2, 2, 1, 3, 1, 1, 2, 3, 1, 1, 2, 3, 2, 1, 1, 3, 1, 1, 2,
                    1, 1, 3, 2, 1, 1, 1, 12, 1, 1, 2, 1, 2, 3, 1, 1, 1, 1, 1, 2, 3, 1, 1, 3, 1, 1,
                    2, 1, 1, 3, 1, 1, 2, 1, 1, 1, 3, 2, 1,
                ],
            ),
            (
                "978-1-873671-00-9 12345",
                &[
                    1, 1, 1, 1, 3, 1, 2, 3, 1, 2, 1, 1, 2, 2, 2, 1, 2, 1, 3, 2, 1, 3, 1, 1, 4, 1,
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 4, 1, 3, 1, 2, 2, 2, 2, 1, 3, 2, 1, 1, 3, 2, 1, 1,
                    3, 1, 1, 2, 1, 1, 1, 12, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 4,
                    1, 1, 1, 1, 1, 3, 2, 1, 1, 1, 2, 3, 1,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo.
            let got = encode_isbn_p5(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode_isbn_p5({text:?}) (ISBN-13 + 5-digit add-on corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(got.bars, want, "ISBN+P5 sbs mismatch for {text:?}");
        }
    }

    /// ISSN + 2-digit add-on cross-validation against
    /// `b.raw("issn", "<issn> 00 <addon>", {})[0].sbs`. BWIPP's
    /// `issn` uses a 3-part syntax `<issn> <seqvar> <addon>` where
    /// `seqvar` is a text-only sequence/issue field; we accept the
    /// simpler 2-part `<issn> <addon>` form (matching common
    /// industry usage) but the resulting bar pattern is identical
    /// to BWIPP's `<issn> 00 <addon>` output.
    #[test]
    fn issn_p2_matches_bwipp_sbs() {
        let cases: &[(&str, &[u8])] = &[
            (
                "0317-8471 13",
                &[
                    1, 1, 1, 1, 3, 1, 2, 2, 1, 3, 1, 1, 1, 2, 3, 1, 4, 1, 1, 1, 2, 2, 2, 1, 3, 1,
                    2, 1, 1, 1, 1, 1, 1, 2, 1, 3, 1, 1, 3, 2, 1, 3, 1, 2, 3, 2, 1, 1, 3, 2, 1, 1,
                    2, 2, 2, 1, 1, 1, 1, 12, 1, 1, 2, 2, 2, 2, 1, 1, 1, 1, 1, 4, 1,
                ],
            ),
            (
                "1144-875X 99",
                &[
                    1, 1, 1, 1, 3, 1, 2, 2, 1, 3, 1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 3, 1, 1, 1, 1, 3,
                    2, 1, 1, 1, 1, 1, 1, 2, 1, 3, 1, 3, 1, 2, 1, 2, 3, 1, 3, 2, 1, 1, 3, 2, 1, 1,
                    1, 3, 1, 2, 1, 1, 1, 12, 1, 1, 2, 2, 1, 1, 3, 1, 1, 2, 1, 1, 3,
                ],
            ),
        ];
        for &(text, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo.
            let got = encode_issn_p2(text, &Options::default()).unwrap_or_else(|e| {
                panic!("encode_issn_p2({text:?}) (ISSN + 2-digit add-on corpus item) must succeed; got Err: {e}")
            });
            assert_eq!(got.bars, want, "ISSN+P2 sbs mismatch for {text:?}");
        }
    }

    #[test]
    fn combine_preserves_alternation() {
        // Synthetic minimal LinearPatterns: just verify combine() doesn't
        // collapse adjacent same-kind elements.
        let m = LinearPattern {
            bars: vec![3, 2, 3],
            text: None,
        };
        let a = LinearPattern {
            bars: vec![1, 1, 1],
            text: None,
        };
        let c = combine(m, a, 9);
        // m ended with a bar (index 2 = even); a gap-space was appended:
        //   [3, 2, 3, 9, 1, 1, 1]
        assert_eq!(c.bars, vec![3, 2, 3, 9, 1, 1, 1]);
    }

    #[test]
    fn combine_extends_trailing_space() {
        let m = LinearPattern {
            bars: vec![3, 2],
            text: None,
        };
        let a = LinearPattern {
            bars: vec![1, 1, 1],
            text: None,
        };
        let c = combine(m, a, 9);
        // m ended with a space; gap is added to the trailing space:
        //   [3, 11, 1, 1, 1]
        assert_eq!(c.bars, vec![3, 11, 1, 1, 1]);
    }

    /// EAN-13 + 2-digit add-on golden from
    /// `raw("ean13", "0123456789012 12", {})[0].sbs`.
    #[test]
    fn ean13_p2_matches_bwip_js_raw_sbs() {
        let p = encode_ean13_p2("012345678901 12", &Options::default()).unwrap();
        let want: [u8; 73] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 1,
            1, 12, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 2,
        ];
        assert_eq!(p.bars, want, "ean13+p2 bars mismatch vs bwip-js raw output");
    }

    /// EAN-13 + 5-digit add-on golden from
    /// `raw("ean13", "0123456789012 12345", {})[0].sbs`.
    #[test]
    fn ean13_p5_matches_bwip_js_raw_sbs() {
        let p = encode_ean13_p5("012345678901 12345", &Options::default()).unwrap();
        let want: [u8; 91] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 1,
            1, 12, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 3, 2, 1, 1,
            1, 2, 3, 1,
        ];
        assert_eq!(p.bars, want, "ean13+p5 bars mismatch vs bwip-js raw output");
    }

    /// UPC-A + 2-digit add-on golden from
    /// `raw("upca", "01234567890 12", {})[0].sbs`.
    #[test]
    fn upca_p2_matches_bwip_js_raw_sbs() {
        let p = encode_upca_p2("01234567890 12", &Options::default()).unwrap();
        let want: [u8; 73] = [
            1, 1, 1, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 4, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 1, 2, 3, 1, 1, 1,
            1, 12, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 2,
        ];
        assert_eq!(p.bars, want, "upca+p2 bars mismatch vs bwip-js raw output");
    }

    /// UPC-E + 2-digit add-on golden from
    /// `raw("upce", "01234565 12", {})[0].sbs`.
    #[test]
    fn upce_p2_matches_bwip_js_raw_sbs() {
        let p = encode_upce_p2("01234565 12", &Options::default()).unwrap();
        let want: [u8; 47] = [
            1, 1, 1, 1, 2, 2, 2, 2, 1, 2, 2, 1, 4, 1, 1, 2, 3, 1, 1, 1, 3, 2, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1, 12, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 2,
        ];
        assert_eq!(p.bars, want, "upce+p2 bars mismatch vs bwip-js raw output");
    }

    /// EAN-8 + 2-digit add-on golden, captured from bwip-js
    /// `raw("ean8", "1234567 12", { permitaddon: true })[0].sbs`. (bwip-js
    /// requires `permitaddon` because the EAN-8 + add-on combination is
    /// non-standard; the Rust crate's catalog has explicit `ean8p2` and
    /// `ean8p5` ids so it accepts the combination unconditionally.)
    /// Regenerate via `node rust/tools/oracle-eanupc-addons.js`.
    #[test]
    fn ean8_p2_matches_bwip_js_raw_sbs() {
        let p = encode_ean8_p2("1234567 12", &Options::default()).unwrap();
        let want: [u8; 57] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1,
            1, 1, 4, 1, 3, 1, 2, 3, 2, 1, 1, 1, 1, 1, 12, 1, 1, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 2,
        ];
        assert_eq!(p.bars, want, "ean8+p2 bars mismatch vs bwip-js raw output");
    }

    /// EAN-8 + 5-digit add-on golden, captured from
    /// `raw("ean8", "1234567 12345", { permitaddon: true })[0].sbs`.
    #[test]
    fn ean8_p5_matches_bwip_js_raw_sbs() {
        let p = encode_ean8_p5("1234567 12345", &Options::default()).unwrap();
        let want: [u8; 75] = [
            1, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 1, 1, 1, 1, 1, 2, 3, 1, 1,
            1, 1, 4, 1, 3, 1, 2, 3, 2, 1, 1, 1, 1, 1, 12, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1,
            1, 1, 1, 4, 1, 1, 1, 1, 1, 3, 2, 1, 1, 1, 2, 3, 1,
        ];
        assert_eq!(p.bars, want, "ean8+p5 bars mismatch vs bwip-js raw output");
    }

    /// UPC-A + 5-digit add-on golden, captured from
    /// `raw("upca", "01234567890 12345", {})[0].sbs`.
    #[test]
    fn upca_p5_matches_bwip_js_raw_sbs() {
        let p = encode_upca_p5("01234567890 12345", &Options::default()).unwrap();
        let want: [u8; 91] = [
            1, 1, 1, 3, 2, 1, 1, 2, 2, 2, 1, 2, 1, 2, 2, 1, 4, 1, 1, 1, 1, 3, 2, 1, 2, 3, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 4, 1, 3, 1, 2, 1, 2, 1, 3, 3, 1, 1, 2, 3, 2, 1, 1, 1, 2, 3, 1, 1, 1,
            1, 12, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 3, 2, 1, 1,
            1, 2, 3, 1,
        ];
        assert_eq!(p.bars, want, "upca+p5 bars mismatch vs bwip-js raw output");
    }

    /// UPC-E + 5-digit add-on golden, captured from
    /// `raw("upce", "01234565 12345", {})[0].sbs`.
    #[test]
    fn upce_p5_matches_bwip_js_raw_sbs() {
        let p = encode_upce_p5("01234565 12345", &Options::default()).unwrap();
        let want: [u8; 65] = [
            1, 1, 1, 1, 2, 2, 2, 2, 1, 2, 2, 1, 4, 1, 1, 2, 3, 1, 1, 1, 3, 2, 1, 1, 1, 1, 4, 1, 1,
            1, 1, 1, 1, 12, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2, 1, 2, 2, 1, 1, 1, 1, 4, 1, 1, 1, 1, 1, 3,
            2, 1, 1, 1, 2, 3, 1,
        ];
        assert_eq!(p.bars, want, "upce+p5 bars mismatch vs bwip-js raw output");
    }

    /// Stage 11.A8c — pin `split_pair(data, expected_addon)`. The
    /// helper trims input, splits on whitespace, and returns
    /// `(main, addon)` when:
    ///   * exactly 2 whitespace-separated tokens present;
    ///   * the addon's *digit count* (filter + count) equals
    ///     `expected_addon` (non-digit chars in addon don't count).
    ///
    /// Five branches; no direct test until now — only exercised
    /// through every encode_ean*_p2/p5 entry, so per-branch mutations
    /// only surface end-to-end.
    ///
    /// Mutations killed:
    ///   * missing main / missing addon / extra token early-returns;
    ///   * `chars().filter(...).count() != expected_addon` count drift;
    ///   * `filter(...)` removal (would count non-digits and accept
    ///     "X1" as 2-digit when expected_addon=2);
    ///   * `!= expected_addon` → `< expected_addon` (would accept
    ///     longer addons).
    #[test]
    fn split_pair_two_token_split_and_digit_count_guard() {
        // Happy: 2 tokens, exact digit count.
        assert_eq!(split_pair("1234567 12", 2).unwrap(), ("1234567", "12"));
        assert_eq!(
            split_pair("1234567 12345", 5).unwrap(),
            ("1234567", "12345")
        );

        // Trim happens before split.
        assert_eq!(split_pair("  1234567 12  ", 2).unwrap(), ("1234567", "12"));

        // The 6 rejection inputs below cover FOUR distinct arms in
        // split_pair. Each per-input pin anchors the path-specific
        // diagnostic + (where applicable) Debug-echo of the offending
        // value, with cross-arm contamination guards.

        // Missing-main arm (line 37): empty and all-whitespace inputs.
        for empty in ["", "   "] {
            match split_pair(empty, 2).unwrap_err() {
                Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("EAN/UPC add-on:")
                            && msg.contains("missing main barcode data"),
                        "{empty:?}: missing-main diagnostic; got {msg}"
                    );
                    assert!(
                        !msg.contains("missing add-on digits"),
                        "{empty:?}: must NOT leak missing-addon wording; got {msg}"
                    );
                }
                other => panic!("{empty:?}: expected InvalidData, got {other:?}"),
            }
        }

        // Missing-addon arm (line 40): single token.
        match split_pair("1234567", 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN/UPC add-on:") && msg.contains("missing add-on digits"),
                    "single-token diagnostic; got {msg}"
                );
                assert!(
                    !msg.contains("missing main barcode data"),
                    "single-token must NOT leak missing-main wording; got {msg}"
                );
            }
            other => panic!("single token: expected InvalidData, got {other:?}"),
        }

        // Three-token arm (line 41-44).
        match split_pair("a b c", 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN/UPC add-on:")
                        && msg.contains("expected exactly two whitespace-separated fields"),
                    "three-token diagnostic; got {msg}"
                );
            }
            other => panic!("three tokens: expected InvalidData, got {other:?}"),
        }

        // Wrong-digit-count arm (line 46-49): too few (1) and too many (3).
        // Pins the `{expected_addon}` interpolation (=2) + the
        // `{addon:?}` Debug-echo of the actual addon string.
        match split_pair("abc 1", 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("EAN/UPC add-on:")
                        && msg.contains("expected 2-digit add-on")
                        && msg.contains("got \"1\""),
                    "1-digit addon diagnostic must echo expected=2 + got=\"1\"; got {msg}"
                );
            }
            other => panic!("1-digit addon: expected InvalidData, got {other:?}"),
        }
        match split_pair("abc 123", 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("expected 2-digit add-on") && msg.contains("got \"123\""),
                    "3-digit addon diagnostic must echo expected=2 + got=\"123\" (kills \
                     `!=` → `<` since `<` would silently ACCEPT 3 ≥ 2 and never \
                     reach this arm); got {msg}"
                );
            }
            other => panic!("3-digit addon: expected InvalidData, got {other:?}"),
        }

        // Non-digit interior in addon is FILTERED before count — so
        // "12a" has 2 digits → passes when expected_addon=2 (this is
        // documented behavior of the helper; the caller validates the
        // addon further). Kills `filter` removal (which would count
        // all 3 chars and fail).
        assert_eq!(
            split_pair("abc 12a", 2).unwrap(),
            ("abc", "12a"),
            "addon digit count uses filter — non-digits ignored \
             (kills `.filter(...)` removal)"
        );
    }
}
