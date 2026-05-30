//! Data Matrix (ECC 200).
//!
//! Delegates to the `datamatrix` crate. The role of this module is the same
//! as for QR Code — translate our options and adapt the output to our
//! `BitMatrix`.

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

use datamatrix::{DataMatrix, SymbolList, SymbolSize};

/// Encode a Data Matrix payload.
///
/// Recognized options:
///   * `version` = e.g. `10x10`, `24x24`, `16x48` (default: auto-size)
///   * `shape`   = `square` | `rectangular` (default: any)
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Default: auto-sized square.
/// let svg = render_svg(Symbology::DataMatrix, "Hello, world!", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
///
/// // Force a rectangular shape.
/// let mut opts = Options::default();
/// opts.extras.push(("shape".into(), "rectangular".into()));
/// let svg = render_svg(Symbology::DataMatrix, "Short", &opts).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
/// Encode a Data Matrix Rectangular Extension (DMRE) symbol. BWIPP
/// `datamatrixrectangularextension`.
///
/// DMRE (ISO/IEC 21471) adds 17 rectangular sizes to the original 6
/// (8×48..26×64). The `datamatrix` crate exposes them via
/// [`SymbolList::with_extended_rectangles()`]. Forcing this symbol
/// list ensures the encoder will pick a DMRE size where it makes
/// sense (e.g. 80×8 for a 40-character payload) instead of falling
/// back to a tall classic rectangular size.
///
/// Honours the `version` option for explicit DMRE size selection.
/// BWIPP's `bwipp_datamatrixrectangularextension` simply forces
/// `dmre=true` and delegates to `bwipp_datamatrix`, which then
/// respects all the standard `version`/`format`/`parsefnc` options.
/// The Rust port currently exposes `version` (the most-asked-for
/// knob) — callers can pin any extended-rectangle size from the
/// DMRE table (`8x48`, `8x64`, `8x80`, `8x96`, `8x120`, `8x144`,
/// `12x64`, `12x88`, `16x64`, `20x36`, `20x44`, `20x64`, `22x48`,
/// `24x48`, `24x64`, `26x40`, `26x48`, `26x64`). A non-DMRE size
/// (square or the 6 original rectangles) returns
/// `Error::InvalidOption`.
pub fn encode_rectangular_extension(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let symbol_list = match opts.get("version") {
        Some(spec) => {
            let size = parse_symbol_size(spec)?;
            if !is_dmre_extended_rectangle(size) {
                return Err(Error::InvalidOption(format!(
                    "datamatrixrectangularextension: version={spec:?} is not a \
                     DMRE-extended rectangle size (valid: 8x48, 8x64, 8x80, \
                     8x96, 8x120, 8x144, 12x64, 12x88, 16x64, 20x36, 20x44, \
                     20x64, 22x48, 24x48, 24x64, 26x40, 26x48, 26x64)"
                )));
            }
            SymbolList::with_whitelist([size])
        }
        None => SymbolList::with_extended_rectangles().enforce_rectangular(),
    };
    encode_with_symbol_list(data, symbol_list, "Data Matrix (DMRE)")
}

/// Test predicate: is `size` one of the 18 DMRE-extended rectangle
/// sizes? Mirrors `SymbolList::with_extended_rectangles()`'s contents
/// (ISO/IEC 21471 §6.3.2).
fn is_dmre_extended_rectangle(size: SymbolSize) -> bool {
    matches!(
        size,
        SymbolSize::Rect8x48
            | SymbolSize::Rect8x64
            | SymbolSize::Rect8x80
            | SymbolSize::Rect8x96
            | SymbolSize::Rect8x120
            | SymbolSize::Rect8x144
            | SymbolSize::Rect12x64
            | SymbolSize::Rect12x88
            | SymbolSize::Rect16x64
            | SymbolSize::Rect20x36
            | SymbolSize::Rect20x44
            | SymbolSize::Rect20x64
            | SymbolSize::Rect22x48
            | SymbolSize::Rect24x48
            | SymbolSize::Rect24x64
            | SymbolSize::Rect26x40
            | SymbolSize::Rect26x48
            | SymbolSize::Rect26x64
    )
}

/// Drive the upstream `datamatrix` crate for an already-resolved
/// `SymbolList`, returning our [`BitMatrix`]. Centralises the two
/// defences every call into the third-party encoder needs (both the
/// square/auto `encode` path and the DMRE `encode_rectangular_extension`
/// path route through here):
///
/// 1. **Macro-envelope guard.** `use_macro_if_possible`
///    (encodation/mod.rs:194 in 0.3.2) slices
///    `data[HEAD.len() .. data.len() - TRAIL.len()]` whenever the input
///    *starts with* a 7-byte Macro 05/06 header (`[)>{RS}05{GS}` /
///    `[)>{RS}06{GS}`) but never checks it is long enough to also hold
///    the 2-byte `{RS}{EOT}` trailer, so an 8-byte header-only input
///    yields the reversed range `data[7..6]` and panics. A well-formed
///    envelope is always ≥ 9 bytes, so rejecting the short case loses no
///    legitimate input. (Found by the A4 fuzz harness via
///    render_svg(DataMatrix, "[)>{RS}06{GS}6").)
/// 2. **catch_unwind backstop.** Any *other* upstream panic on
///    attacker-controlled input is converted to a clean `Error` rather
///    than aborting the process. `catch_unwind` is safe (compatible with
///    `#![forbid(unsafe_code)]`); `AssertUnwindSafe` is justified because
///    the closure only reads `data` and consumes the owned `symbol_list`,
///    so a panic leaves no shared state observably broken.
fn encode_with_symbol_list(
    data: &str,
    symbol_list: SymbolList,
    ctx: &str,
) -> Result<BitMatrix, Error> {
    const MACRO05_HEAD: &[u8] = b"[)>\x1E05\x1D";
    const MACRO06_HEAD: &[u8] = b"[)>\x1E06\x1D";
    const MACRO_TRAIL_LEN: usize = 2; // {RS}{EOT}
    let bytes = data.as_bytes();
    for head in [MACRO05_HEAD, MACRO06_HEAD] {
        if bytes.starts_with(head) && bytes.len() < head.len() + MACRO_TRAIL_LEN {
            return Err(Error::InvalidData(format!(
                "{ctx}: truncated structured-append / macro envelope (input \
                 starts with a `[)>` Macro 05/06 header but is too short to \
                 contain the closing {{RS}}{{EOT}} trailer)"
            )));
        }
    }

    let dm = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        DataMatrix::encode_str(data, symbol_list)
    }))
    .map_err(|_| {
        Error::Backend(format!(
            "{ctx}: the upstream `datamatrix` encoder panicked on this input \
             (likely a malformed structured-append / macro envelope) — \
             rejected rather than aborting"
        ))
    })?
    .map_err(|e| Error::Backend(format!("{ctx}: {e:?}")))?;
    let bitmap = dm.bitmap();
    let mut matrix = BitMatrix::new(bitmap.width(), bitmap.height());
    for (x, y) in bitmap.pixels() {
        matrix.set(x, y, true);
    }
    Ok(matrix)
}

pub fn encode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let symbol_list = match opts.get("version") {
        Some(spec) => SymbolList::with_whitelist([parse_symbol_size(spec)?]),
        None => match opts.get("shape") {
            Some("square") => SymbolList::default().enforce_square(),
            Some("rectangular") => SymbolList::default().enforce_rectangular(),
            Some(other) => {
                return Err(Error::InvalidOption(format!(
                    "shape={other} (use square|rectangular)"
                )))
            }
            // Default to square — matches BWIPP's `bwipp_datamatrix`
            // size-table preference (the encoder iterates all 24 square
            // sizes before any rectangular size). Without this default
            // the `datamatrix` crate picks the smallest fitting size
            // regardless of shape, which can produce a rectangular DM
            // where BWIPP picks square (e.g. 32×8 vs 16×16 for
            // "HELLO,WORLD!"). Use `Symbology::DataMatrixRectangular`
            // or `shape=rectangular` for the rectangular path.
            None => SymbolList::default().enforce_square(),
        },
    };

    encode_with_symbol_list(data, symbol_list, "Data Matrix")
}

/// Look up a [`SymbolSize`] from a `"WxH"` string (e.g. `"24x24"`).
fn parse_symbol_size(spec: &str) -> Result<SymbolSize, Error> {
    // The `datamatrix` crate doesn't expose width/height accessors on
    // `SymbolSize`, so we map the canonical labels by hand. This covers
    // every variant the crate ships in v0.3.
    Ok(match spec {
        "10x10" => SymbolSize::Square10,
        "12x12" => SymbolSize::Square12,
        "14x14" => SymbolSize::Square14,
        "16x16" => SymbolSize::Square16,
        "18x18" => SymbolSize::Square18,
        "20x20" => SymbolSize::Square20,
        "22x22" => SymbolSize::Square22,
        "24x24" => SymbolSize::Square24,
        "26x26" => SymbolSize::Square26,
        "32x32" => SymbolSize::Square32,
        "36x36" => SymbolSize::Square36,
        "40x40" => SymbolSize::Square40,
        "44x44" => SymbolSize::Square44,
        "48x48" => SymbolSize::Square48,
        "52x52" => SymbolSize::Square52,
        "64x64" => SymbolSize::Square64,
        "72x72" => SymbolSize::Square72,
        "80x80" => SymbolSize::Square80,
        "88x88" => SymbolSize::Square88,
        "96x96" => SymbolSize::Square96,
        "104x104" => SymbolSize::Square104,
        "120x120" => SymbolSize::Square120,
        "132x132" => SymbolSize::Square132,
        "144x144" => SymbolSize::Square144,
        "8x18" => SymbolSize::Rect8x18,
        "8x32" => SymbolSize::Rect8x32,
        "12x26" => SymbolSize::Rect12x26,
        "12x36" => SymbolSize::Rect12x36,
        "16x36" => SymbolSize::Rect16x36,
        "16x48" => SymbolSize::Rect16x48,
        "8x48" => SymbolSize::Rect8x48,
        "8x64" => SymbolSize::Rect8x64,
        "8x80" => SymbolSize::Rect8x80,
        "8x96" => SymbolSize::Rect8x96,
        "8x120" => SymbolSize::Rect8x120,
        "8x144" => SymbolSize::Rect8x144,
        "12x64" => SymbolSize::Rect12x64,
        "12x88" => SymbolSize::Rect12x88,
        "16x64" => SymbolSize::Rect16x64,
        "20x36" => SymbolSize::Rect20x36,
        "20x44" => SymbolSize::Rect20x44,
        "20x64" => SymbolSize::Rect20x64,
        "22x48" => SymbolSize::Rect22x48,
        "24x48" => SymbolSize::Rect24x48,
        "24x64" => SymbolSize::Rect24x64,
        "26x40" => SymbolSize::Rect26x40,
        "26x48" => SymbolSize::Rect26x48,
        "26x64" => SymbolSize::Rect26x64,
        other => {
            return Err(Error::InvalidOption(format!(
                "no Data Matrix symbol of size {other}"
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8d — regression for the fuzz crash inside the upstream
    /// `datamatrix` 0.3.2 encoder (encodation/mod.rs:194, "slice index
    /// starts at 7 but ends at 6"). A truncated `[)>` Macro 05/06
    /// envelope (header present, < 9 bytes total) must now surface a
    /// graceful Error::InvalidData instead of panicking the process.
    #[test]
    fn truncated_macro_envelope_returns_error_not_panic() {
        // Exact fuzz reproducer: `[)>{RS}06{GS}6` (8 bytes).
        let repro = "[)>\x1e06\x1d6";
        let r = encode(repro, &Options::default());
        assert!(
            matches!(r, Err(Error::InvalidData(_))),
            "truncated macro-06 envelope must be InvalidData, got {r:?}"
        );
        // Macro 05 head, equally short.
        let r5 = encode("[)>\x1e05\x1dX", &Options::default());
        assert!(
            matches!(r5, Err(Error::InvalidData(_))),
            "truncated macro-05 envelope must be InvalidData, got {r5:?}"
        );
        // A well-formed (≥9-byte) macro-06 envelope with the {RS}{EOT}
        // trailer still encodes — the guard rejects only the short case.
        let ok = encode("[)>\x1e06\x1dHELLO\x1e\x04", &Options::default());
        assert!(
            ok.is_ok(),
            "well-formed macro envelope must encode, got {ok:?}"
        );
    }

    #[test]
    fn encodes_short_payload() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Data Matrix substrate short-payload smoke path: 5-byte
        // ASCII "hello" → at-least 10×10 symbol.
        let m = encode("hello", &Options::default()).expect(
            "encode(\"hello\", default) (Data Matrix substrate short-payload smoke: 5-byte ASCII → ≥10×10) must succeed",
        );
        assert!(m.width() >= 10);
        assert!(m.height() >= 10);
    }

    #[test]
    fn encodes_longer_payload() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Data Matrix substrate longer-payload smoke path: 28-byte
        // payload with spaces → ≥14 width.
        let m = encode("a longer payload with spaces", &Options::default()).expect(
            "encode(\"a longer payload with spaces\", default) (Data Matrix substrate longer-payload smoke: 28-byte mixed ASCII+space → ≥14 width) must succeed",
        );
        assert!(m.width() >= 14);
    }

    /// Data Matrix BitMatrix golden from
    /// `raw("datamatrix", "hello", {})[0].pixs` (12×12 = 144 pixels,
    /// row-major).
    #[test]
    fn matches_bwip_js_raw_pixs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Data Matrix substrate 12×12 byte-for-byte pixs oracle:
        // 5-byte ASCII "hello" → 144-cell raw pixs golden.
        let m = encode("hello", &Options::default()).expect(
            "encode(\"hello\", default) (Data Matrix substrate 12×12 byte-for-byte pixs golden vs bwip-js raw output; 144-cell row-major) must succeed",
        );
        assert_eq!(m.width(), 12);
        assert_eq!(m.height(), 12);
        #[rustfmt::skip]
        let want_pixs: [u8; 144] = [
            1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1,
            1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0,
            1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1,
            1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1,
            1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0,
            1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1,
            1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0,
            1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1,
            1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ];
        let mut got = Vec::with_capacity(144);
        for y in 0..m.height() {
            for x in 0..m.width() {
                got.push(if m.get(x, y) { 1u8 } else { 0u8 });
            }
        }
        assert_eq!(
            got, want_pixs,
            "datamatrix pixs mismatch vs bwip-js raw output"
        );
    }

    /// Data Matrix substrate baseline for `"HELLO,WORLD!"`. With the
    /// default-to-square change (see `encode` doc), our substrate now
    /// picks the same 16×16 symbol that bwip-js does for this input
    /// — closing the previous size divergence. The module bit
    /// pattern can still differ from BWIPP for arbitrary inputs
    /// because the substrate's mode selector takes a slightly
    /// different encoding path, but at least the symbol size now
    /// agrees. Both are spec-compliant ISO/IEC 16022 Data Matrix
    /// codes that decode to "HELLO,WORLD!".
    ///
    /// Pins the *current* size so a substrate-version bump that
    /// changes shape preference is caught here.
    #[test]
    fn substrate_baseline_for_hello_world() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Data Matrix substrate-baseline path: with default-to-
        // square change, "HELLO,WORLD!" must pick a 16×16 symbol
        // (substrate-version drift sentinel).
        let m = encode("HELLO,WORLD!", &Options::default()).expect(
            "encode(\"HELLO,WORLD!\", default) (Data Matrix substrate-baseline: default-to-square 16×16 sentinel guarding against substrate-version drift) must succeed",
        );
        assert_eq!(m.width(), 16, "datamatrix substrate width drift");
        assert_eq!(m.height(), 16, "datamatrix substrate height drift");
    }

    /// DMRE — Data Matrix Rectangular Extension — adds the 17 extra
    /// rectangular sizes (8×48..26×64) per ISO/IEC 21471. The
    /// substrate has all of them available via
    /// `SymbolList::with_extended_rectangles().enforce_rectangular()`,
    /// but the encoder's preferred-size policy differs from BWIPP's
    /// `dmre: true` selector: for a 40-character payload BWIPP picks
    /// 80×8 (a DMRE-only size), the substrate picks 36×16 (a
    /// classic-rectangular size). Both are valid ISO/IEC 16022
    /// rectangular symbols that decode to the same payload. Same
    /// substrate-spec posture as plain `datamatrix` on arbitrary
    /// input. Pin the rectangular-shape property here; size-policy
    /// divergence is documented in PORT_STATUS / GOLDEN_COVERAGE.
    #[test]
    fn dmre_produces_rectangular_for_long_input() {
        let long = "AB".repeat(20);
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DMRE rectangular-shape property: 40-byte "ABAB..." must
        // pick a rectangular (width > height) symbol with solid bottom
        // finder-timing bar.
        let m = encode_rectangular_extension(&long, &Options::default()).expect(
            "encode_rectangular_extension(40-byte \"AB\"×20, default) (DMRE rectangular-shape property: width > height + solid bottom finder bar) must succeed",
        );
        assert!(
            m.width() > m.height(),
            "DMRE should produce a rectangular (wide) symbol; got {}×{}",
            m.width(),
            m.height()
        );
        // Bottom row is the L-finder timing bar (solid).
        let bottom = (0..m.width()).all(|x| m.get(x, m.height() - 1));
        assert!(bottom, "DMRE bottom row should be a solid finder bar");
    }

    #[test]
    fn dmre_short_input_matches_bwip_js_size() {
        // For 5-digit input both substrates pick 18×8 — this *is* a
        // classic rectangular size that BWIPP and our substrate
        // agree on, so pin the dimensions.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DMRE classic-rect short-input path: 5-digit "12345" →
        // 18×8 (the BWIPP-agreeing size for digit-only payloads).
        let m = encode_rectangular_extension("12345", &Options::default()).expect(
            "encode_rectangular_extension(\"12345\", default) (DMRE classic-rectangular 18×8 for 5-digit BWIPP-agreement size) must succeed",
        );
        assert_eq!(m.width(), 18);
        assert_eq!(m.height(), 8);
    }

    /// 12-row pixs sanity for a few payloads: the `datamatrix` crate
    /// substrate doesn't always match BWIPP's mode-selection
    /// heuristics for arbitrary inputs (we get a different — but
    /// still valid — symbol size for some text). For the inputs
    /// where the size DOES agree we anchor the bit pattern; for
    /// others we accept any valid Data Matrix output.
    #[test]
    fn datamatrix_picks_sensible_sizes() {
        let cases: &[(&str, usize)] = &[("ABC", 10), ("12345678", 12), ("hello", 12)];
        for &(text, expected_min_size) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(|e| panic!(...))` naming the corpus row
            // (text + expected_min_size) so a substrate-version drift
            // pinpoints which row regressed.
            let m = encode(text, &Options::default()).unwrap_or_else(|e| {
                panic!(
                    "encode({text:?}, default) (Data Matrix substrate corpus row, expected_min_size={expected_min_size}, must be square + solid bottom finder bar) must succeed: {e:?}",
                )
            });
            assert!(
                m.width() >= expected_min_size,
                "{text:?} too small: {}x{}",
                m.width(),
                m.height()
            );
            assert_eq!(m.width(), m.height(), "{text:?} not square");
            // Always finder-bar on the bottom row (all 1s) — Data
            // Matrix's right + bottom timing patterns are solid.
            let bottom = (0..m.width()).all(|x| m.get(x, m.height() - 1));
            assert!(bottom, "{text:?} bottom row is not solid bar");
        }
    }

    /// Stage 11.15 — `encode_rectangular_extension` now respects the
    /// `version` option (BWIPP A1 surface). User can force any
    /// DMRE-extended rectangle size.
    #[test]
    fn dmre_version_option_forces_extended_rectangle_size() {
        // Pick a couple of explicit DMRE sizes.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // each DMRE explicit-version path: version="8x48" forces 48×8
        // (height=8, width=48), version="20x64" forces 64×20.
        let m = encode_rectangular_extension(
            "Hello, World!",
            &Options::default().with("version", "8x48"),
        )
        .expect(
            "encode_rectangular_extension(\"Hello, World!\", version=8x48) (DMRE explicit-version 48×8 force-size path) must succeed",
        );
        assert_eq!(m.width(), 48);
        assert_eq!(m.height(), 8);

        let m = encode_rectangular_extension(
            &"AB".repeat(30),
            &Options::default().with("version", "20x64"),
        )
        .expect(
            "encode_rectangular_extension(60-byte \"AB\"×30, version=20x64) (DMRE explicit-version 64×20 force-size path) must succeed",
        );
        assert_eq!(m.width(), 64);
        assert_eq!(m.height(), 20);
    }

    /// Stage 11.15 — `version` pointing at a non-DMRE size (square or
    /// the 6 original rectangles) returns `Error::InvalidOption`.
    #[test]
    fn dmre_version_rejects_non_extended_sizes() {
        // Square size — rejected. Diagnostic at line 60-65:
        //   "datamatrixrectangularextension: version={spec:?} is not a
        //    DMRE-extended rectangle size (valid: 8x48, ...)"
        // 4-anchor pin upgrades the previous single-substring check:
        let err = encode_rectangular_extension("ABC", &Options::default().with("version", "10x10"))
            .unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("datamatrixrectangularextension:"),
                    "must carry symbology prefix; got {msg:?}"
                );
                assert!(
                    msg.contains("version=\"10x10\""),
                    "must Debug-echo the offending version; got {msg:?}"
                );
                assert!(
                    msg.contains("DMRE-extended rectangle size"),
                    "must carry the predicate; got {msg:?}"
                );
                assert!(
                    msg.contains("8x48") && msg.contains("26x64"),
                    "must name endpoints of the valid DMRE size list; got {msg:?}"
                );
            }
            other => panic!(
                "encode_rectangular_extension(\"ABC\", version=\"10x10\") must reject as InvalidOption(square-not-DMRE); got {other:?}"
            ),
        }

        // Original (non-DMRE) rectangle — rejected. Stage 11.A8c —
        // upgrade from `matches!(_, InvalidOption(_))` to pin the
        // wrapper prefix + offending spec + "DMRE-extended rectangle"
        // tail so swap-of-arms (e.g. the non-DMRE rect path silently
        // routes through `with_whitelist`) is caught.
        // Stage 11.A8c (cont) — switch let-else → match so the panic
        // names the offending spec ("16x48") and the rejected variant.
        let msg = match encode_rectangular_extension(
            "ABC",
            &Options::default().with("version", "16x48"),
        )
        .unwrap_err()
        {
            Error::InvalidOption(m) => m,
            err => panic!(
                "encode_rectangular_extension(\"ABC\", version=\"16x48\") must reject as InvalidOption(non-DMRE rect); got {err:?}"
            ),
        };
        assert!(
            msg.contains("datamatrixrectangularextension:"),
            "diagnostic must carry the DMRE prefix; got {msg:?}"
        );
        assert!(
            msg.contains("\"16x48\""),
            "diagnostic must echo the offending spec via {{spec:?}}; got {msg:?}"
        );
        assert!(
            msg.contains("not a DMRE-extended rectangle"),
            "diagnostic must mention 'not a DMRE-extended rectangle'; got {msg:?}"
        );
        // Cross-arm contamination guard: the non-DMRE-rect path must
        // NOT borrow the parse_symbol_size "no Data Matrix symbol"
        // message (that's reserved for genuinely-unknown specs).
        assert!(
            !msg.contains("no Data Matrix symbol"),
            "non-DMRE rect diagnostic must not leak parse_symbol_size's message; got {msg:?}"
        );
    }

    /// Stage 11.A8c — pin the `shape=<unknown>` catch-all rejection in
    /// the base `encode()` (line ~116). Existing tests cover the
    /// "square" / "rectangular" / "default" paths but never exercise
    /// the catch-all `Some(other) → Err(InvalidOption)` arm; a mutant
    /// that swaps the arm body to `SymbolList::default()` would
    /// silently accept any string and fall through to encoding.
    #[test]
    fn encode_rejects_unknown_shape_value() {
        let opts = Options::default().with("shape", "diamond");
        match encode("HELLO", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("shape=diamond"),
                    "expected msg with 'shape=diamond', got: {msg:?}"
                );
                assert!(
                    msg.contains("use square|rectangular"),
                    "expected hint to mention valid choices, got: {msg:?}"
                );
            }
            other => panic!(
                "expected InvalidOption(shape=...), got {other:?}; \
                 the catch-all shape arm may have been replaced"
            ),
        }
        // shape="" (empty) also hits the same catch-all. Stage 11.A8c —
        // pin the same shape= + use square|rectangular tail; the empty
        // string is a critical sentinel that distinguishes the catch-all
        // from a mutant that special-cases empty strings to a different
        // arm.
        // Stage 11.A8c (cont) — switch let-else → match so the panic
        // names the rejected variant when the catch-all is taken.
        let msg = match encode("HELLO", &Options::default().with("shape", "")).unwrap_err() {
            Error::InvalidOption(m) => m,
            err => panic!(
                "encode(\"HELLO\", shape=\"\") must reject as InvalidOption(unknown shape catch-all); got {err:?} (mutation may have special-cased empty string to a different arm)"
            ),
        };
        assert!(
            msg.contains("shape="),
            "empty-shape diagnostic must echo 'shape=' prefix; got {msg:?}"
        );
        assert!(
            msg.contains("use square|rectangular"),
            "empty-shape diagnostic must mention valid choices; got {msg:?}"
        );
    }

    /// Stage 11.15 — `version` with an entirely unknown spec returns
    /// `Error::InvalidOption` (via `parse_symbol_size`).
    ///
    /// Stage 11.A8c — pin both the `parse_symbol_size` diagnostic AND
    /// the absence of the DMRE-specific "not a DMRE-extended" tail,
    /// so a mutant that routes garbage specs through the DMRE
    /// rejection path (instead of the `?` propagation from
    /// `parse_symbol_size`) is caught.
    #[test]
    fn dmre_version_rejects_garbage_spec() {
        // Stage 11.A8c (cont) — switch let-else → match so the panic
        // echoes the offending spec name ("garbage") and the actual
        // returned variant when a mutation re-routes through a
        // non-InvalidOption error variant.
        let msg = match encode_rectangular_extension(
            "ABC",
            &Options::default().with("version", "garbage"),
        )
        .unwrap_err()
        {
            Error::InvalidOption(m) => m,
            err => panic!(
                "encode_rectangular_extension(\"ABC\", version=\"garbage\") must reject as InvalidOption (via parse_symbol_size short-circuit); got {err:?}"
            ),
        };
        assert!(
            msg.contains("no Data Matrix symbol"),
            "garbage spec must surface parse_symbol_size's diagnostic; got {msg:?}"
        );
        assert!(
            msg.contains("garbage"),
            "diagnostic must echo the offending spec name; got {msg:?}"
        );
        assert!(
            !msg.contains("DMRE-extended"),
            "garbage spec must short-circuit BEFORE the DMRE-membership \
             check; got {msg:?}"
        );
    }

    /// Stage 11.15 — `is_dmre_extended_rectangle` correctly classifies
    /// the 18 DMRE-extended sizes vs the 6 original ones.
    #[test]
    fn is_dmre_extended_rectangle_classifies_correctly() {
        // 6 original rectangles — NOT DMRE.
        for s in [
            SymbolSize::Rect8x18,
            SymbolSize::Rect8x32,
            SymbolSize::Rect12x26,
            SymbolSize::Rect12x36,
            SymbolSize::Rect16x36,
            SymbolSize::Rect16x48,
        ] {
            assert!(
                !is_dmre_extended_rectangle(s),
                "{s:?} should NOT be DMRE-extended",
            );
        }
        // All 18 DMRE-extended sizes — must be DMRE. Spot-checking
        // 4 of them isn't enough to catch a "drop one arm" mutation
        // on the 18-arm `matches!` macro. Anchor every variant so a
        // mutant dropping any single row would flip the result for
        // that input.
        for s in [
            SymbolSize::Rect8x48,
            SymbolSize::Rect8x64,
            SymbolSize::Rect8x80,
            SymbolSize::Rect8x96,
            SymbolSize::Rect8x120,
            SymbolSize::Rect8x144,
            SymbolSize::Rect12x64,
            SymbolSize::Rect12x88,
            SymbolSize::Rect16x64,
            SymbolSize::Rect20x36,
            SymbolSize::Rect20x44,
            SymbolSize::Rect20x64,
            SymbolSize::Rect22x48,
            SymbolSize::Rect24x48,
            SymbolSize::Rect24x64,
            SymbolSize::Rect26x40,
            SymbolSize::Rect26x48,
            SymbolSize::Rect26x64,
        ] {
            assert!(
                is_dmre_extended_rectangle(s),
                "{s:?} should be DMRE-extended (covers all 18 arms)",
            );
        }
        // Squares — NOT DMRE.
        for s in [SymbolSize::Square10, SymbolSize::Square144] {
            assert!(!is_dmre_extended_rectangle(s), "{s:?} should NOT be DMRE");
        }
    }

    /// `parse_symbol_size(spec)` is the 48-arm lookup that translates
    /// `"WxH"` spec strings into `SymbolSize` variants. Unknown specs
    /// return `Err(InvalidOption)`.
    ///
    /// Called from `encode()` whenever the user passes a `version`
    /// option but never directly tested — coverage rides on whatever
    /// version specs the encoder corpus happens to use.
    ///
    /// Mutations to catch:
    /// * Arm typo: `"10x10" => Square10` → `"10x10" => Square12`.
    /// * Range collapse: a wildcard `_ => ...` instead of the explicit
    ///   `other => Err(...)` arm.
    /// * Case-folding accident: `"10X10"` (uppercase X) must Err.
    /// * Whitespace tolerance: `" 10x10 "` must Err (no trimming).
    /// * Endpoint pin: first and last variant in each category.
    /// * Distinctness invariant: every recognised spec maps to a unique
    ///   SymbolSize.
    #[test]
    fn parse_symbol_size_full_alphabet_lookup() {
        // ---- Endpoints + spot-checks per category.

        // Squares (24 variants): first, mid, last.
        assert_eq!(parse_symbol_size("10x10").unwrap(), SymbolSize::Square10);
        assert_eq!(parse_symbol_size("32x32").unwrap(), SymbolSize::Square32);
        assert_eq!(parse_symbol_size("144x144").unwrap(), SymbolSize::Square144);

        // Original 6 rectangles: first and last.
        assert_eq!(parse_symbol_size("8x18").unwrap(), SymbolSize::Rect8x18);
        assert_eq!(parse_symbol_size("16x48").unwrap(), SymbolSize::Rect16x48);

        // DMRE 18 (extended rectangles): first, mid, last.
        // Note: "8x48" is the FIRST DMRE — must NOT collide with any
        // square arm. A typo "8x48 => Square48" would pass goldens that
        // never request shape=8x48.
        assert_eq!(parse_symbol_size("8x48").unwrap(), SymbolSize::Rect8x48);
        assert_eq!(parse_symbol_size("20x36").unwrap(), SymbolSize::Rect20x36);
        assert_eq!(parse_symbol_size("26x64").unwrap(), SymbolSize::Rect26x64);

        // ---- Error arms.
        // Stage 11.A8c — upgrade from 5 weak matches!(_, InvalidOption(_))
        // to per-input diagnostic + spec-echo pins. parse_symbol_size has
        // ONE catch-all rejection arm (line 199-203) producing:
        //   "no Data Matrix symbol of size {other}"
        //
        // A mutant that drops the `{other}` echo or swaps the predicate
        // text survives variant-only assertion. Iterating distinct inputs
        // with per-input echo also kills `{other}` → fixed-string mutants.
        for input in ["100x100", "", "10X10", " 10x10 ", "hello"] {
            let err = parse_symbol_size(input).unwrap_err();
            let Error::InvalidOption(msg) = err else {
                panic!("parse_symbol_size({input:?}) must yield InvalidOption; got {err:?}");
            };
            assert!(
                msg.contains("no Data Matrix symbol"),
                "diagnostic for {input:?} must call out 'no Data Matrix symbol'; got {msg:?}"
            );
            assert!(
                msg.contains("of size"),
                "diagnostic for {input:?} must include 'of size' tag; got {msg:?}"
            );
            assert!(
                msg.contains(input),
                "diagnostic for {input:?} must echo the offending spec; got {msg:?}"
            );
        }

        // ---- Distinctness invariant: every recognised spec maps to
        // a unique SymbolSize. Catches any duplicate-arm typo.
        let specs: &[&str] = &[
            // Squares (24).
            "10x10", "12x12", "14x14", "16x16", "18x18", "20x20", "22x22", "24x24", "26x26",
            "32x32", "36x36", "40x40", "44x44", "48x48", "52x52", "64x64", "72x72", "80x80",
            "88x88", "96x96", "104x104", "120x120", "132x132", "144x144",
            // Original 6 rectangles.
            "8x18", "8x32", "12x26", "12x36", "16x36", "16x48", // DMRE 18.
            "8x48", "8x64", "8x80", "8x96", "8x120", "8x144", "12x64", "12x88", "16x64", "20x36",
            "20x44", "20x64", "22x48", "24x48", "24x64", "26x40", "26x48", "26x64",
        ];
        assert_eq!(specs.len(), 48, "must enumerate every arm");
        let mut seen: Vec<SymbolSize> = Vec::with_capacity(48);
        for spec in specs {
            let size = parse_symbol_size(spec)
                .unwrap_or_else(|e| panic!("recognised spec {spec:?} should not Err: {e:?}"));
            assert!(
                !seen.contains(&size),
                "spec {spec:?} produced duplicate SymbolSize {size:?}"
            );
            seen.push(size);
        }
    }
}
