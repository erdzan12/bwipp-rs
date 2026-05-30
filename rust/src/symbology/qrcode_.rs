//! QR Code.
//!
//! We delegate the actual matrix construction to the `qrcode` crate (a pure-
//! Rust QR encoder). The role of this module is to translate our `Options`
//! into the `qrcode` API and convert the result into a `BitMatrix` so it
//! plugs into our renderer pipeline.

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

use qrcode::{EcLevel, QrCode, Version};

/// Encode a QR Code payload.
///
/// Recognized options:
///   * `eclevel` = `L` | `M` | `Q` | `H`  (default: `M`)
///   * `version` = numeric 1..=40         (default: auto)
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Default: error-correction level M, auto-sized version.
/// let svg = render_svg(Symbology::QrCode, "https://example.com", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
///
/// // High error correction + forced version-5 grid.
/// let mut opts = Options::default();
/// opts.extras.push(("eclevel".into(), "H".into()));
/// opts.extras.push(("version".into(), "5".into()));
/// let svg = render_svg(Symbology::QrCode, "Hello", &opts).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
#[cfg_attr(feature = "prefer-native-qrcode", allow(dead_code))]
pub fn encode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let ec = match opts.get("eclevel").unwrap_or("M") {
        "L" => EcLevel::L,
        "M" => EcLevel::M,
        "Q" => EcLevel::Q,
        "H" => EcLevel::H,
        other => return Err(Error::InvalidOption(format!("eclevel={other}"))),
    };

    let code = if let Some(ver_str) = opts.get("version") {
        let ver: i16 = ver_str
            .parse()
            .map_err(|_| Error::InvalidOption(format!("version={ver_str}")))?;
        if !(1..=40).contains(&ver) {
            return Err(Error::InvalidOption(format!(
                "QR version must be 1..=40, got {ver}"
            )));
        }
        QrCode::with_version(data.as_bytes(), Version::Normal(ver), ec)
            .map_err(|e| Error::Backend(e.to_string()))?
    } else {
        QrCode::with_error_correction_level(data.as_bytes(), ec)
            .map_err(|e| Error::Backend(e.to_string()))?
    };

    let width = code.width();
    let mut matrix = BitMatrix::new(width, width);
    let modules = code.to_colors();
    for y in 0..width {
        for x in 0..width {
            let module = modules[y * width + x];
            // qrcode::Color::Dark is the foreground (black).
            matrix.set(x, y, module == qrcode::Color::Dark);
        }
    }
    Ok(matrix)
}

/// Encode a Micro QR Code payload.
///
/// Micro QR has four versions (M1–M4) with much tighter capacity than
/// full QR; the qrcode crate auto-selects the smallest one that fits.
///
/// Recognized options:
///   * `eclevel` = `L` | `M` | `Q`  (default: `L`; **M4 also supports Q**,
///     and **M1 ignores eclevel entirely** since it has no error
///     correction — anything other than L errors there.)
///   * `version` = numeric 1..=4  (default: auto)
///
/// Returns a square [`BitMatrix`] sized `2*v + 9` modules per side
/// (so 11×11 for M1, 13×13 for M2, 15×15 for M3, 17×17 for M4).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::MicroQrCode, "1234", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
#[cfg_attr(feature = "prefer-native-qrcode", allow(dead_code))]
pub fn encode_micro(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let ec = match opts.get("eclevel").unwrap_or("L") {
        "L" => EcLevel::L,
        "M" => EcLevel::M,
        "Q" => EcLevel::Q,
        // Micro QR has no H level; reject it up front rather than
        // bubbling up a Backend error.
        "H" => {
            return Err(Error::InvalidOption(
                "Micro QR does not support eclevel=H".into(),
            ));
        }
        other => return Err(Error::InvalidOption(format!("eclevel={other}"))),
    };

    let code = if let Some(ver_str) = opts.get("version") {
        let ver: i16 = ver_str
            .parse()
            .map_err(|_| Error::InvalidOption(format!("version={ver_str}")))?;
        if !(1..=4).contains(&ver) {
            return Err(Error::InvalidOption(format!(
                "Micro QR version must be 1..=4, got {ver}"
            )));
        }
        QrCode::with_version(data.as_bytes(), Version::Micro(ver), ec)
            .map_err(|e| Error::Backend(e.to_string()))?
    } else {
        // Try the four versions in ascending order with the requested
        // ec level; pick the first one that fits.
        let mut last_err: Option<Error> = None;
        let mut result = None;
        for ver in 1..=4 {
            match QrCode::with_version(data.as_bytes(), Version::Micro(ver), ec) {
                Ok(c) => {
                    result = Some(c);
                    break;
                }
                Err(e) => last_err = Some(Error::Backend(e.to_string())),
            }
        }
        result.ok_or_else(|| {
            last_err
                .unwrap_or_else(|| Error::Backend("Micro QR: no version fits the payload".into()))
        })?
    };

    let width = code.width();
    let mut matrix = BitMatrix::new(width, width);
    let modules = code.to_colors();
    for y in 0..width {
        for x in 0..width {
            let module = modules[y * width + x];
            matrix.set(x, y, module == qrcode::Color::Dark);
        }
    }
    Ok(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_small_payload() {
        // Stage 11.A8c (cont) — descriptive labels naming QR V1 size
        // floor + square-symbol invariant.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the QR substrate V1-floor smoke path: 5-byte "hello" must
        // produce ≥21-wide square (V1 minimum).
        let m = encode("hello", &Options::default()).expect(
            "encode(\"hello\", default) (QR substrate V1-floor smoke: 5-byte ASCII → ≥21-wide square symbol) must succeed",
        );
        assert!(
            m.width() >= 21,
            "encode(\"hello\") (5-char ASCII payload, smallest typical QR) must produce QR symbol ≥21 modules wide (V1 minimum); got {}×{}",
            m.width(),
            m.height()
        );
        assert_eq!(
            m.width(),
            m.height(),
            "QR symbol must be square (width == height); got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn respects_eclevel() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the QR substrate eclevel L/H paired path: same payload "hello"
        // at L vs H must produce L ≤ H matrix width (higher EC = more
        // codewords needed).
        let m_l = encode("hello", &Options::default().with("eclevel", "L")).expect(
            "encode(\"hello\", eclevel=L) (QR substrate eclevel-L path; lowest ECC overhead) must succeed",
        );
        let m_h = encode("hello", &Options::default().with("eclevel", "H")).expect(
            "encode(\"hello\", eclevel=H) (QR substrate eclevel-H path; highest ECC overhead → ≥L width) must succeed",
        );
        // Higher EC level => bigger matrix for the same payload.
        assert!(m_h.width() >= m_l.width());
    }

    #[test]
    fn invalid_eclevel_rejected() {
        // Stage 11.A8c (cont) — upgrade discriminant-only
        // `matches!(_, Err(Error::InvalidOption(_)))` to 1-anchor pin
        // matching the source diagnostic at line 43 of qrcode_.rs
        // (`eclevel=Z`). The format is unusual (no symbology prefix
        // because the eclevel arm is bare `format!("eclevel={other}")`),
        // so the value-echo is the strongest anchor available.
        match encode("hi", &Options::default().with("eclevel", "Z")) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("eclevel=Z"),
                    "missing `eclevel=Z` value echo: {msg}"
                );
                assert!(
                    !msg.contains("Micro QR"),
                    "wrong helper — Micro QR diagnostic leaked into Full QR eclevel reject: {msg}"
                );
            }
            other => panic!("eclevel=Z should reject as InvalidOption, got {other:?}"),
        }
    }

    #[test]
    fn micro_qr_encodes_small_payload() {
        // 5 numeric digits → fits M1 (11×11). The qrcode crate ranges
        // payload by alphanumeric/byte mode, so byte-mode "12345" may
        // promote to a larger version — we just assert it's <= M4.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Micro QR substrate auto-version path: 5-digit payload
        // must land in M1..=M4 (width 11..=17).
        let m = encode_micro("12345", &Options::default()).expect(
            "encode_micro(\"12345\", default) (Micro QR substrate auto-version path: 5-digit payload → M1..=M4 width 11..=17) must succeed",
        );
        assert!(m.width() >= 11 && m.width() <= 17);
        assert_eq!(m.width(), m.height());
    }

    #[test]
    fn micro_qr_forced_version_picks_size() {
        let opts = Options::default().with("version", "2");
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Micro QR substrate forced-version path: version=2 →
        // M2 size (2*2 + 9 = 13 modules).
        let m = encode_micro("AB", &opts).expect(
            "encode_micro(\"AB\", version=2) (Micro QR substrate forced-version M2 path: 13 modules wide = 2*2+9) must succeed",
        );
        assert_eq!(m.width(), 13); // M2 → 2*2 + 9 = 13
    }

    #[test]
    fn micro_qr_rejects_eclevel_h() {
        // Micro QR has no H level; should be a clean InvalidOption,
        // not a Backend error.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only to 2-anchor
        // pin matching the source diagnostic at line 106-108 of
        // qrcode_.rs (`Micro QR does not support eclevel=H`):
        //   1. `Micro QR` symbology prefix
        //   2. `does not support eclevel=H` predicate
        let opts = Options::default().with("eclevel", "H");
        match encode_micro("hi", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(msg.contains("Micro QR"), "missing `Micro QR` prefix: {msg}");
                assert!(
                    msg.contains("does not support eclevel=H"),
                    "missing predicate: {msg}"
                );
                assert!(
                    !msg.contains("version must be"),
                    "version-range arm leaked into eclevel reject: {msg}"
                );
            }
            other => panic!("eclevel=H on Micro QR should reject as InvalidOption, got {other:?}"),
        }
    }

    #[test]
    fn micro_qr_rejects_out_of_range_version() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 3-anchor
        // pin matching the source diagnostic at line 117-120 of
        // qrcode_.rs (`Micro QR version must be 1..=4, got 5`):
        //   1. `Micro QR` symbology prefix
        //   2. `version must be 1..=4` full range spec
        //   3. `got 5` value echo
        let opts = Options::default().with("version", "5");
        match encode_micro("hi", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(msg.contains("Micro QR"), "missing `Micro QR` prefix: {msg}");
                assert!(
                    msg.contains("version must be 1..=4"),
                    "missing `version must be 1..=4` range spec: {msg}"
                );
                assert!(msg.contains("got 5"), "missing `got 5` value echo: {msg}");
            }
            other => {
                panic!("Micro QR version=5 should reject as InvalidOption, got {other:?}")
            }
        }
    }

    /// Stage 11.A8c — pin `encode_micro`'s version-range guard +
    /// parse-failure branch (lines 113-121). Mirrors the full-QR
    /// `qr_version_unparseable_rejected_with_distinct_message` test
    /// for the Micro QR encoder.
    ///
    /// Anchors:
    ///   - "0" / "5" / "100" → out-of-range numeric (kills the
    ///     `(1..=4).contains` range check mutations).
    ///   - "abc" / "" / "1.5" → unparseable (kills the i16::parse
    ///     map_err branch).
    ///   - Boundary "1" and "4" → succeed (kills `(1..=3)` or
    ///     `(2..=4)` mutations).
    #[test]
    fn micro_qr_version_range_and_parse_branches() {
        // Out-of-range numeric: must hit the range error at line 119:
        //   "Micro QR version must be 1..=4, got {ver}"
        // `&&`-anchor both substrings so a mutation that drops EITHER
        // the "1..=4" range hint OR the `{ver}` echo would fail. Plus
        // pin the "Micro QR" prefix to keep the symbology tag honest.
        for bad in ["0", "5", "100"] {
            let opts = Options::default().with("version", bad);
            match encode_micro("hi", &opts) {
                Err(Error::InvalidOption(msg)) => {
                    assert!(
                        msg.contains("Micro QR version must be 1..=4"),
                        "{bad:?}: range diagnostic must carry Micro QR prefix + 1..=4 hint; got {msg:?}"
                    );
                    assert!(
                        msg.contains(&format!("got {bad}")),
                        "{bad:?}: range diagnostic must echo the parsed `got {bad}`; got {msg:?}"
                    );
                }
                other => panic!("expected InvalidOption(range) for {bad:?}, got {other:?}"),
            }
        }
        // Unparseable: must hit the parse-failure error (distinct
        // message format: "version=<v>"). Stage 11.A8c — add a
        // cross-arm contamination guard against the range arm's
        // `Micro QR version must be` prefix so a mutation that
        // routes parse failures through the range diagnostic would
        // fail this assertion (and vice versa).
        for bad in ["abc", "", "1.5"] {
            let opts = Options::default().with("version", bad);
            match encode_micro("hi", &opts) {
                Err(Error::InvalidOption(msg)) => {
                    assert!(
                        msg.contains(&format!("version={bad}")),
                        "expected 'version={bad}' parse-failure diagnostic, got: {msg:?}"
                    );
                    assert!(
                        !msg.contains("Micro QR version must be"),
                        "{bad:?}: parse-failure arm leaked range diagnostic `Micro QR version must be`: {msg:?}"
                    );
                    assert!(
                        !msg.contains("got "),
                        "{bad:?}: parse-failure arm leaked range diagnostic `got` echo: {msg:?}"
                    );
                }
                other => panic!("expected InvalidOption(parse) for {bad:?}, got {other:?}"),
            }
        }
        // Boundary good: 1 and 4 must succeed.
        for v in ["1", "4"] {
            let opts = Options::default().with("version", v);
            let res = encode_micro("1", &opts);
            assert!(
                res.is_ok(),
                "micro version={v:?} should succeed (boundary), got {res:?}"
            );
        }
    }

    /// QR Code substrate baseline for "HELLO" + EC level M.
    ///
    /// This pins the *current* `qrcode`-crate output as a regression
    /// baseline so a future substrate-version bump (which could
    /// change the mask-selection tie-break or segment-mode chooser)
    /// is caught at test time, not in production. The captured
    /// pattern is **not** byte-equivalent to bwip-js's
    /// `raw("qrcode","HELLO",{eclevel:"M"})[0].pixs` — the qrcode
    /// crate's spec-compliant mask scorer picks a different mask
    /// number than BWIPP on tied scores. Both symbols decode to
    /// "HELLO".
    ///
    /// This divergence is the same class as the documented
    /// `gs1qrcode` compatibility exception
    /// (`rust/COMPATIBILITY_EXCEPTIONS.md`); the exception applies to
    /// every catalog row that goes through the qrcode-crate substrate
    /// (`qrcode`, `qrcode_iso`, `microqrcode`, `swissqrcode`,
    /// `hibc_lic_qrcode`, `hibc_pas_qrcode`, `gs1qrcode`).
    #[test]
    fn substrate_baseline_pixs_for_hello() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the QR substrate V1 21×21 byte-for-byte pixs golden path:
        // 5-byte "HELLO" → 441-cell pixs (substrate-version drift
        // sentinel; documented compat exception for mask divergence).
        let m = encode("HELLO", &Options::default()).expect(
            "encode(\"HELLO\", default) (QR substrate V1 21×21 byte-for-byte pixs golden; 441-cell substrate-version drift sentinel) must succeed",
        );
        assert_eq!(m.width(), 21);
        assert_eq!(m.height(), 21);
        #[rustfmt::skip]
        let want: [u8; 441] = [
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1,
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1,
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1,
            0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0,
            1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0,
            0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1,
            1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0,
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0,
        ];
        let mut got = Vec::with_capacity(441);
        for y in 0..m.height() {
            for x in 0..m.width() {
                got.push(if m.get(x, y) { 1u8 } else { 0u8 });
            }
        }
        assert_eq!(
            got, want,
            "QR Code substrate pixs drift — `qrcode` crate may have \
             changed its mask scorer; investigate before updating."
        );
    }

    /// Kills the `delete !` mutant at line ~50 (the QR version range
    /// guard `if !(1..=40).contains(&ver)`). The original errors on
    /// out-of-range versions; the mutant inverts the guard so it
    /// errors on *valid* versions and accepts invalid ones. We pin
    /// both directions:
    ///   * version=5 (valid) → must succeed.
    ///   * version=0 / 41 (out of range) → must error with
    ///     InvalidOption mentioning "1..=40".
    #[test]
    fn qr_version_range_is_one_to_forty() {
        // In-range: must encode.
        let opts = Options::default().with("version", "5");
        encode("hi", &opts)
            .expect("version=5 should be accepted (in-range); the !-deleted mutant rejects it");

        // Below range: must reject.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("1..=40")` upgraded to 3-anchor pin:
        //   1. `QR version must be 1..=40` full predicate (kills
        //      mutations that swap the range bounds or substitute
        //      the variant ID — Micro QR sibling says "1..=4")
        //   2. `got 0` value-echo (kills `{ver}` interpolation drop)
        //   3. cross-arm contamination guard: must NOT contain `Micro
        //      QR version` (sibling at line 119 of qrcode_.rs)
        let opts_zero = Options::default().with("version", "0");
        match encode("hi", &opts_zero) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("QR version must be 1..=40"),
                    "missing full predicate `QR version must be 1..=40`: {msg:?}"
                );
                assert!(msg.contains("got 0"), "missing `got 0` value-echo: {msg:?}");
                assert!(
                    !msg.contains("Micro QR version"),
                    "cross-arm contamination: full-QR reject mentions Micro QR: {msg:?}"
                );
            }
            other => panic!("expected InvalidOption(range), got {other:?}"),
        }

        // Above range: must reject.
        let opts_big = Options::default().with("version", "41");
        match encode("hi", &opts_big) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("QR version must be 1..=40"),
                    "missing full predicate `QR version must be 1..=40`: {msg:?}"
                );
                assert!(
                    msg.contains("got 41"),
                    "missing `got 41` value-echo: {msg:?}"
                );
                assert!(
                    !msg.contains("Micro QR version"),
                    "cross-arm contamination: full-QR reject mentions Micro QR: {msg:?}"
                );
            }
            other => panic!("expected InvalidOption(range), got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the unparseable-version branch of
    /// `encode()` at line 47-49. The existing
    /// `qr_version_range_is_one_to_forty` test covers numeric
    /// out-of-range, but the `i16::parse` failure path (with its
    /// own distinct error message `"version=<v>"`) wasn't pinned.
    ///
    /// Mutations to catch:
    ///   - Drop the `parse` `map_err` → would propagate a `ParseIntError`
    ///     wrapped in a different error type.
    ///   - Change the error message format → "version=" wouldn't appear.
    ///   - Swap `i16` to a wider integer type → "9999999999" would
    ///     parse successfully and then hit the range check (different
    ///     error message: "1..=40" vs "version=").
    #[test]
    fn qr_version_unparseable_rejected_with_distinct_message() {
        // Stage 11.A8c — add cross-arm contamination guards against
        // the full-QR range arm's `QR Code version must be 1..=40`
        // and `got <N>` substrings. A mutation that routes parse
        // failures through the range diagnostic (or vice versa)
        // would fail these assertions.
        for bad in ["abc", "", "1.5", "0x5", " 5"] {
            let opts = Options::default().with("version", bad);
            let res = encode("hi", &opts);
            match res {
                Err(Error::InvalidOption(msg)) => {
                    assert!(
                        msg.contains(&format!("version={bad}")),
                        "expected 'version={bad}' in error msg, got: {msg:?}"
                    );
                    assert!(
                        !msg.contains("QR Code version must be"),
                        "{bad:?}: parse-failure arm leaked full-QR range diagnostic: {msg:?}"
                    );
                    assert!(
                        !msg.contains("1..=40"),
                        "{bad:?}: parse-failure arm leaked range hint `1..=40`: {msg:?}"
                    );
                }
                other => panic!("expected InvalidOption(version=<v>) for {bad:?}, got {other:?}"),
            }
        }
    }

    /// Kills the Micro QR pixel-extraction mutants at lines ~149-150:
    ///   * `* with +` and `* with /` in `modules[y * width + x]` —
    ///     these change the indexing math so the wrong pixel maps
    ///     to (x, y).
    ///   * `== with !=` in `module == qrcode::Color::Dark` — flips
    ///     foreground/background.
    ///
    /// We pin a full Micro QR baseline for `"12345"` at version=2 →
    /// 13×13 bitmap. The micro_qr_forced_version_picks_size test
    /// only checks width; this one anchors every pixel so any of
    /// the three mutants surfaces as a divergence.
    #[test]
    fn micro_substrate_baseline_pixs_for_12345() {
        let opts = Options::default().with("version", "2");
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Micro QR substrate M2 13×13 byte-for-byte pixs golden
        // path: 5-digit "12345" at forced version=2 → 169-cell pixs
        // (anchors `* with +` walker mutant + `== with !=` color flip).
        let m = encode_micro("12345", &opts).expect(
            "encode_micro(\"12345\", version=2) (Micro QR substrate M2 13×13 byte-for-byte pixs golden; 169-cell anchors walker + color-flip mutants) must succeed",
        );
        assert_eq!(m.width(), 13);
        assert_eq!(m.height(), 13);
        let mut got = Vec::with_capacity(169);
        for y in 0..m.height() {
            for x in 0..m.width() {
                got.push(if m.get(x, y) { 1u8 } else { 0u8 });
            }
        }
        // Pin the entire 13×13 bitmap exactly. The `qrcode` crate's
        // mask selection for "12345" at M2 produces a deterministic
        // bit pattern; any mutation that breaks the pixel-extraction
        // loop (wrong `*` indexing, `==!=` color flip) shifts a
        // material number of pixels and the array assertion catches
        // it. We dump the actual current pattern via the assert_eq
        // failure on first run; pinning the exact array lets the
        // `* with +` mutant (which indexes `modules[y + width + x]`,
        // re-reading earlier pixels) and the `== with !=` mutant
        // (which inverts dark/light) both surface as a mismatch.
        #[rustfmt::skip]
        let want: [u8; 169] = [
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1,
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1,
            1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0,
            0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0,
            1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0,
            0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1,
            1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1,
        ];
        assert_eq!(
            got, want,
            "Micro QR M2 \"12345\" pixs drift — the `qrcode` crate may have \
             changed its mask scorer, or the pixel-extraction loop in \
             `encode_micro` has been mutated (`*` indexing or `==` color check)"
        );
        // Backup invariant: a `*` → `+` index mutant computes
        // `modules[y + width + x]` instead of `modules[y * width +
        // x]`, walking re-reading the same row repeatedly. The
        // resulting `got` array won't match `want`, and the
        // assertion above catches it.
    }
}
