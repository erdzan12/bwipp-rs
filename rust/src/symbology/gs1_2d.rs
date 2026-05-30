//! GS1 DataMatrix, NTIN, PPN, and GS1 QR Code.
//!
//! Three families of 2D GS1-style codes:
//!
//!   * **GS1 DataMatrix** — Data Matrix that carries GS1 AI data. We parse
//!     `(AI)data` with [`crate::util::gs1`] and delegate to the
//!     `datamatrix` crate's `encode_gs1` entry point, which inserts the
//!     symbol-level FNC1 codeword (232) and the per-element separators.
//!   * **NTIN** — a thin wrapper that prepends `(8003)` to the user's data
//!     and renders as GS1 DataMatrix.
//!   * **PPN** — Pharmacy Product Number rendered as plain Data Matrix
//!     wrapped in the ANSI MH10.8.2 envelope (`[)>RS 06 GS 9N<ppn> RS EOT`).
//!   * **GS1 QR Code** — emits the formal "FNC1 in first position" mode
//!     indicator (`0101` per ISO/IEC 18004 Annex L) prepended to the
//!     auto-segmented payload, then delegates 2D layout to the `qrcode`
//!     crate. Pinned by `gs1_qrcode_fnc1_first_position_mode_indicator_is_0101`.
//!     Listed as a compatibility exception in PORT_STATUS because the
//!     `qrcode` crate's mask scorer can tie-break differently from BWIPP;
//!     see `COMPATIBILITY_EXCEPTIONS.md` §1 / §1a.
//!
//! Reference: BWIPP `gs1datamatrix.ps.src`, `ntin.ps.src`, `ppn.ps.src`.

use datamatrix::{DataMatrix, SymbolList};

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

use crate::util::gs1;

// ---------------------------------------------------------------------------
// GS1 DataMatrix
// ---------------------------------------------------------------------------

/// Encode a GS1 Digital Link Data Matrix. BWIPP `gs1dldatamatrix`.
///
/// Input is a GS1 Digital Link URI, e.g.
/// `https://id.gs1.org/01/04012345123456`. We validate the URI shape
/// (must have an `http(s)://` scheme and at least one `/AI/value` path
/// segment with a valid 2-4 digit AI) via [`gs1::parse_dl_uri`], then
/// encode the **original URI string** as a plain Data Matrix —
/// matching BWIPP's `bwipp_gs1dldatamatrix` which uses
/// `bwipp_gs1process('dl')` for syntax validation only and then calls
/// `bwipp_datamatrix` on the raw URI.
///
/// # Errors
/// - `InvalidData` if the URI fails the light DL shape validation
///   ([`gs1::parse_dl_uri`] errors).
pub fn encode_gs1_dl_datamatrix(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    gs1::parse_dl_uri(data).map_err(|e| Error::InvalidData(e.to_string()))?;
    super::datamatrix_::encode(data, opts)
}

/// Encode a GS1 Digital Link QR Code. BWIPP `gs1dlqrcode`.
///
/// Same URI-validation + literal-encoding contract as
/// [`encode_gs1_dl_datamatrix`], but with `qrcode::encode` as the
/// substrate. Inherits the qrcode-substrate compatibility exception
/// since the underlying QR encoder is the same.
pub fn encode_gs1_dl_qrcode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    gs1::parse_dl_uri(data).map_err(|e| Error::InvalidData(e.to_string()))?;
    #[cfg(feature = "prefer-native-qrcode")]
    {
        super::qrcode_native::encode_with_options(data.as_bytes(), opts)
    }
    #[cfg(not(feature = "prefer-native-qrcode"))]
    {
        super::qrcode_::encode(data, opts)
    }
}

/// Encode a GS1 DataMatrix forced into rectangular shape. BWIPP
/// `gs1datamatrixrectangular` — same encoder as [`encode_gs1_datamatrix`]
/// but with `shape=rectangular` injected so callers don't have to set
/// it manually.
pub fn encode_gs1_datamatrix_rectangular(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "shape");
    o.extras
        .push(("shape".to_string(), "rectangular".to_string()));
    encode_gs1_datamatrix(data, &o)
}

/// Encode a GS1 DataMatrix from a parenthesised AI element string.
pub fn encode_gs1_datamatrix(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let elements = gs1::parse(data).map_err(|e| Error::InvalidData(e.to_string()))?;
    // `encode_with_fnc1` returns the canonical leading FNC1 + AI/data + inter-
    // element FNC1 byte stream. `datamatrix::encode_gs1` takes the GS1
    // payload *without* the leading FNC1 (it inserts the symbol-level
    // codeword internally), so skip the leading byte.
    let bytes = gs1::encode_with_fnc1(&elements);
    let payload = &bytes[1..]; // strip leading FNC1
    let symbol_list = symbol_list_from_opts(opts)?;
    let dm = DataMatrix::encode_gs1(payload, symbol_list)
        .map_err(|e| Error::Backend(format!("GS1 DataMatrix: {e:?}")))?;
    let bitmap = dm.bitmap();
    let (w, h) = (bitmap.width(), bitmap.height());
    let mut m = BitMatrix::new(w, h);
    for (x, y) in bitmap.pixels() {
        m.set(x, y, true);
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// NTIN
// ---------------------------------------------------------------------------

/// Encode an NTIN: prefix `(8003)` and render as GS1 DataMatrix.
pub fn encode_ntin(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let trimmed = data.trim();
    // Allow the user to pass either bare digits (we wrap) or already-AI
    // form ("(8003)..." we pass through).
    let payload = if trimmed.starts_with('(') {
        trimmed.to_string()
    } else {
        format!("(8003){trimmed}")
    };
    encode_gs1_datamatrix(&payload, opts)
}

// ---------------------------------------------------------------------------
// PPN
// ---------------------------------------------------------------------------

const PPN_RS: u8 = 0x1E; // Record Separator (ASCII)
const PPN_GS: u8 = 0x1D; // Group Separator (ASCII)
const PPN_EOT: u8 = 0x04; // End-of-Transmission (ASCII)

/// Encode a Pharmacy Product Number as a Data Matrix carrying the
/// ANSI MH10.8.2 envelope `[)>RS 06 GS 9N<ppn> RS EOT`.
pub fn encode_ppn(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let payload = data.trim();
    if payload.is_empty() {
        return Err(Error::InvalidData("PPN payload must not be empty".into()));
    }
    // Build the envelope byte stream.
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"[)>");
    bytes.push(PPN_RS);
    bytes.extend_from_slice(b"06");
    bytes.push(PPN_GS);
    bytes.extend_from_slice(b"9N");
    bytes.extend_from_slice(payload.as_bytes());
    bytes.push(PPN_RS);
    bytes.push(PPN_EOT);

    let symbol_list = symbol_list_from_opts(opts)?;
    let dm = DataMatrix::encode(&bytes, symbol_list)
        .map_err(|e| Error::Backend(format!("PPN Data Matrix: {e:?}")))?;
    let bitmap = dm.bitmap();
    let (w, h) = (bitmap.width(), bitmap.height());
    let mut m = BitMatrix::new(w, h);
    for (x, y) in bitmap.pixels() {
        m.set(x, y, true);
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// GS1 QR Code (compatibility exception — see COMPATIBILITY_EXCEPTIONS.md §1a)
// ---------------------------------------------------------------------------

/// Encode a GS1 QR Code with the formal "FNC1 in first position" mode
/// indicator (mode bits `0101`), per ISO/IEC 18004 annex L.
///
/// Uses `qrcode::Bits::push_fnc1_first_position()` to emit the GS1
/// header BEFORE the data segment — scanners that recognise the GS1
/// QR profile (e.g. GS1 General Specifications §5.7) decode this as
/// a GS1 element string rather than a generic byte payload.
///
/// The data segment is emitted in byte mode with the FNC1 (0x1D)
/// inter-element separators preserved. The leading FNC1 byte from
/// `gs1::encode_with_fnc1` is stripped, since the mode indicator
/// IS the leading FNC1.
///
/// Recognized options:
///   * `eclevel` = `L` | `M` | `Q` | `H` (default `M`)
///   * `version` = numeric 1..=40 (default auto)
pub fn encode_gs1_qrcode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let elements = gs1::parse(data).map_err(|e| Error::InvalidData(e.to_string()))?;
    let bytes = gs1::encode_with_fnc1(&elements);
    // Strip the leading FNC1 — the FNC1-first-position mode indicator
    // serves that role at the bit-stream level.
    debug_assert!(matches!(bytes.first(), Some(0x1d)));
    let payload: &[u8] = bytes.get(1..).unwrap_or(&[]);

    // Default path (Stage 17c): native qrcode_native encoder with
    // fnc1first=true. The fnc1-aware auto-select disables EC-upgrade
    // (BWIPP honours the user-requested EC verbatim for GS1 QR), so
    // the chosen size matches BWIPP. The `qrcode` crate fallback
    // stays for `--no-default-features` callers.
    #[cfg(feature = "prefer-native-qrcode")]
    {
        super::qrcode_native::encode_gs1_qrcode(payload, opts)
    }
    #[cfg(not(feature = "prefer-native-qrcode"))]
    {
        encode_gs1_qrcode_substrate(payload, opts)
    }
}

#[cfg(not(feature = "prefer-native-qrcode"))]
fn encode_gs1_qrcode_substrate(payload: &[u8], opts: &Options) -> Result<BitMatrix, Error> {
    use qrcode::bits::Bits;
    use qrcode::types::{EcLevel, Version};
    use qrcode::QrCode;

    let ec = match opts.get("eclevel").unwrap_or("M") {
        "L" => EcLevel::L,
        "M" => EcLevel::M,
        "Q" => EcLevel::Q,
        "H" => EcLevel::H,
        other => return Err(Error::InvalidOption(format!("eclevel={other}"))),
    };

    // Build the bit stream once for a candidate version and try
    // QrCode::with_bits. If the data doesn't fit, bump the version.
    // Explicit version override short-circuits the search.
    let version_override = opts
        .get("version")
        .map(|v| {
            v.parse::<i16>()
                .ok()
                .filter(|n| (1..=40).contains(n))
                .map(Version::Normal)
                .ok_or_else(|| Error::InvalidOption(format!("version={v}")))
        })
        .transpose()?;

    let candidates: Vec<Version> = match version_override {
        Some(v) => vec![v],
        None => (1..=40).map(Version::Normal).collect(),
    };

    let mut last_err: Option<String> = None;
    for version in candidates {
        let mut bits = Bits::new(version);
        if let Err(e) = bits.push_fnc1_first_position() {
            last_err = Some(format!("push_fnc1_first_position: {e:?}"));
            continue;
        }
        // Auto-segment the payload (numeric / alphanumeric / byte) for
        // BWIPP-compatible compactness. GS1 data is mostly digits with
        // 0x1D separators — the numeric mode (10 bits / 3 digits) is
        // dramatically more efficient than byte mode (24 bits / 3
        // digits). The qrcode crate's Parser+Optimizer picks the
        // optimal segment boundaries.
        if let Err(e) = bits.push_optimal_data(payload) {
            last_err = Some(format!("push_optimal_data: {e:?}"));
            continue;
        }
        if let Err(e) = bits.push_terminator(ec) {
            last_err = Some(format!("push_terminator: {e:?}"));
            continue;
        }
        match QrCode::with_bits(bits, ec) {
            Ok(code) => {
                let width = code.width();
                let mut matrix = BitMatrix::new(width, width);
                let modules = code.to_colors();
                for y in 0..width {
                    for x in 0..width {
                        if modules[y * width + x] == qrcode::Color::Dark {
                            matrix.set(x, y, true);
                        }
                    }
                }
                return Ok(matrix);
            }
            Err(e) => {
                last_err = Some(format!("with_bits: {e:?}"));
                continue;
            }
        }
    }
    Err(Error::Backend(format!(
        "GS1 QR Code: no version fits payload ({} bytes after FNC1 strip). \
         Last error: {}",
        payload.len(),
        last_err.unwrap_or_else(|| "<unknown>".into()),
    )))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn symbol_list_from_opts(opts: &Options) -> Result<SymbolList, Error> {
    if let Some(spec) = opts.get("version") {
        Ok(SymbolList::with_whitelist([parse_symbol_size(spec)?]))
    } else if let Some(shape) = opts.get("shape") {
        match shape {
            "square" => Ok(SymbolList::default().enforce_square()),
            "rectangular" => Ok(SymbolList::default().enforce_rectangular()),
            other => Err(Error::InvalidOption(format!(
                "shape={other} (use square|rectangular)"
            ))),
        }
    } else {
        // Default to square — matches BWIPP's `bwipp_datamatrix` size-table
        // preference (the encoder iterates all 24 square sizes before any
        // rectangular size). Without this default the `datamatrix` crate
        // picks the smallest fitting size regardless of shape, which can
        // produce a rectangular DM where BWIPP picks square (e.g. 32×8
        // vs 16×16 for `(01)04012345123456`). Callers wanting rectangular
        // can opt in via `shape=rectangular` or
        // `Symbology::Gs1DataMatrixRectangular`.
        Ok(SymbolList::default().enforce_square())
    }
}

fn parse_symbol_size(spec: &str) -> Result<datamatrix::SymbolSize, Error> {
    use datamatrix::SymbolSize::*;
    Ok(match spec {
        "10x10" => Square10,
        "12x12" => Square12,
        "14x14" => Square14,
        "16x16" => Square16,
        "18x18" => Square18,
        "20x20" => Square20,
        "22x22" => Square22,
        "24x24" => Square24,
        "26x26" => Square26,
        "32x32" => Square32,
        "36x36" => Square36,
        "40x40" => Square40,
        "44x44" => Square44,
        "48x48" => Square48,
        "52x52" => Square52,
        "64x64" => Square64,
        "72x72" => Square72,
        "80x80" => Square80,
        "88x88" => Square88,
        "96x96" => Square96,
        "104x104" => Square104,
        "120x120" => Square120,
        "132x132" => Square132,
        "144x144" => Square144,
        "8x18" => Rect8x18,
        "8x32" => Rect8x32,
        "12x26" => Rect12x26,
        "12x36" => Rect12x36,
        "16x36" => Rect16x36,
        "16x48" => Rect16x48,
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

    /// Stage 11.A8c — pin `parse_symbol_size` arms across the
    /// square/rectangular size families plus the catch-all error.
    ///
    /// Mutations caught:
    ///   * Any single literal swap (e.g. `"10x10"` → `"11x11"`) flips
    ///     a known size to the InvalidOption error path.
    ///   * Returning the same variant for different specs (cross-arm
    ///     mutation) would collapse multiple sizes onto one — caught
    ///     by `assert_ne!` between adjacent sizes.
    ///   * Catch-all error swapped for `Square10` would silently
    ///     accept invalid sizes.
    #[test]
    fn parse_symbol_size_per_family_anchors() {
        use datamatrix::SymbolSize::*;
        // Smallest square.
        assert_eq!(parse_symbol_size("10x10").unwrap(), Square10);
        // Mid-range square (one of the SC4 sizes).
        assert_eq!(parse_symbol_size("32x32").unwrap(), Square32);
        // Largest square.
        assert_eq!(parse_symbol_size("144x144").unwrap(), Square144);
        // Smallest rectangle (DMRE).
        assert_eq!(parse_symbol_size("8x18").unwrap(), Rect8x18);
        // Largest rectangle in the table.
        assert_eq!(parse_symbol_size("16x48").unwrap(), Rect16x48);
        // Adjacent sizes must NOT collapse to the same variant.
        assert_ne!(
            parse_symbol_size("10x10").unwrap(),
            parse_symbol_size("12x12").unwrap()
        );
        assert_ne!(
            parse_symbol_size("8x18").unwrap(),
            parse_symbol_size("8x32").unwrap()
        );
        // Unknown sizes → InvalidOption with the spec echoed back.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("11x11")` upgraded to 2-anchor pin:
        //   1. `no Data Matrix symbol of size` full predicate (kills
        //      mutations that drop or rename the symbology name in
        //      the format string at line 348 of gs1_2d.rs)
        //   2. `11x11` spec-echo (preserved)
        let err = parse_symbol_size("11x11").unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("no Data Matrix symbol of size"),
                    "missing full predicate `no Data Matrix symbol of size`: {msg}"
                );
                assert!(
                    msg.contains("11x11"),
                    "unknown size message should echo spec, got {msg}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
        // Defense-in-depth: empty + garbage inputs route through the SAME
        // InvalidOption arm (not a separate "empty" arm), and both echo
        // their raw spec back. This pins:
        //   * The catch-all branch must NOT be specialized away to a
        //     bare-string error (mutation: drop the `{other}` echo).
        //   * Empty must NOT divert to a different arm — there is only
        //     one rejection arm.
        let err_empty = parse_symbol_size("").unwrap_err();
        match err_empty {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("no Data Matrix symbol of size "),
                    "empty-spec message should keep the predicate, got {msg}"
                );
                // The trailing `{other}` substitution renders to "" — so the
                // message ends right after the trailing space.
                assert!(
                    msg.ends_with("size "),
                    "empty spec should leave the `{{other}}` echo empty, got {msg}"
                );
            }
            other => panic!("empty spec: expected InvalidOption, got {other:?}"),
        }
        let err_garbage = parse_symbol_size("garbage").unwrap_err();
        match err_garbage {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("no Data Matrix symbol of size "),
                    "garbage-spec message should keep the predicate, got {msg}"
                );
                assert!(
                    msg.contains("garbage"),
                    "garbage-spec message must echo the raw spec, got {msg}"
                );
            }
            other => panic!("garbage spec: expected InvalidOption, got {other:?}"),
        }
    }

    #[test]
    fn gs1_datamatrix_encodes_gtin() {
        let m = encode_gs1_datamatrix("(01)04012345123456", &Options::default()).unwrap();
        // GS1 DataMatrix for a single GTIN is small but non-zero. Accept any
        // square symbol of at least 8x8 (DMRE 8x18 is the smallest GS1
        // DataMatrix the spec allows).
        // Stage 11.A8c (cont) — descriptive label naming size floor +
        // rationale (DMRE 8x18 minimum, kills mutation producing 0x0).
        assert!(
            m.width() >= 8 && m.height() >= 8,
            "encode_gs1_datamatrix(\"(01)04012345123456\") (single GTIN-14) must produce DataMatrix ≥8×8 (DMRE 8x18 minimum); got {}×{}",
            m.width(),
            m.height()
        );
    }

    /// With `shape: "square"`, GS1 DataMatrix produces the same
    /// 16×16 square symbol BWIPP defaults to for a single-GTIN
    /// payload. Without the constraint the substrate picks a
    /// rectangular layout that's just as valid (8×32 here, also
    /// scannable) but doesn't match BWIPP's preferred size.
    /// (Even at the same 16×16 size the bit pattern differs — the
    /// `datamatrix` crate's encoder takes a different mode-selection
    /// path than BWIPP for GS1 payloads. The symbol is still a valid
    /// GS1 DataMatrix and decodes back to the same AI element string.)
    #[test]
    fn gs1_datamatrix_square_shape_matches_bwip_js_size() {
        let opts = Options::default().with("shape", "square");
        let m = encode_gs1_datamatrix("(01)04012345123456", &opts).unwrap();
        assert_eq!(m.width(), 16, "GS1 DM square shape should be 16x16");
        assert_eq!(m.height(), 16);
    }

    #[test]
    fn gs1_datamatrix_rejects_bad_ai() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 3-anchor pin via gs1::parse's ParseError::InvalidAi
        // display impl at line 68 of util/gs1.rs (`GS1 parse:
        // invalid AI {ai:?} (must be 2-4 digits)`).
        //   1. `GS1 parse:` prefix (the gs1::parse → encode_gs1_datamatrix
        //      delegation surfaces ParseError via Error::InvalidData)
        //   2. `invalid AI "99999"` AI-token echo (5 chars, fails the
        //      2-4 length check)
        //   3. `2-4 digits` range hint
        match encode_gs1_datamatrix("(99999)X", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 parse:"),
                    "missing `GS1 parse:` prefix: {msg}"
                );
                assert!(
                    msg.contains("invalid AI \"99999\""),
                    "missing `invalid AI \"99999\"` token echo: {msg}"
                );
                assert!(
                    msg.contains("2-4 digits"),
                    "missing `2-4 digits` range hint: {msg}"
                );
            }
            other => panic!("(99999)X should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `symbol_list_from_opts`'s `shape=<unknown>`
    /// rejection branch. Existing tests cover `shape=square` and
    /// `shape=rectangular` (implicit via Gs1DataMatrixRectangular)
    /// but never exercise the catch-all `Err(InvalidOption)` arm at
    /// line ~296. A mutant that swaps the catch-all to `Ok(default)`
    /// would silently accept invalid shape values.
    #[test]
    fn gs1_datamatrix_rejects_unknown_shape_option() {
        let opts = Options::default().with("shape", "circular");
        match encode_gs1_datamatrix("(01)04012345123456", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("shape="),
                    "expected 'shape=' in InvalidOption message, got: {msg:?}"
                );
                assert!(
                    msg.contains("circular"),
                    "error msg should mention the offending value 'circular', got: {msg:?}"
                );
                // Stage 11.A8c (cont) — add valid-shape enumeration
                // anchor `square|rectangular` (kills mutations that
                // drop or rename either of the two valid shape
                // identifiers in the format string at line 297 of
                // gs1_2d.rs). The original 2-anchor pin would survive
                // a swap to e.g. "use circle|oval".
                assert!(
                    msg.contains("use square|rectangular"),
                    "missing valid-shape enumeration `use square|rectangular`: {msg:?}"
                );
            }
            other => panic!(
                "expected InvalidOption(shape=...), got {other:?}; \
                 the catch-all shape arm may have been replaced"
            ),
        }
        // Also: shape="" (empty string) hits the same catch-all.
        //
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // matches! to 2-anchor pin matching the source diagnostic at
        // line 296-298 of gs1_2d.rs (`shape={other} (use square|
        // rectangular)`).
        let opts = Options::default().with("shape", "");
        match encode_gs1_datamatrix("(01)04012345123456", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(msg.contains("shape="), "missing `shape=` prefix: {msg}");
                assert!(
                    msg.contains("square|rectangular"),
                    "missing `square|rectangular` valid-values list: {msg}"
                );
            }
            other => panic!("shape=\"\" should reject as InvalidOption, got {other:?}"),
        }
    }

    /// GS1 Digital Link Data Matrix: BWIPP's `bwipp_gs1dldatamatrix`
    /// validates the URI then encodes the **raw URI string** as plain
    /// Data Matrix.
    ///
    /// Our datamatrix substrate (the `datamatrix` crate) produces a
    /// 22×22 symbol of the same size BWIPP picks for this URI, but
    /// the exact module pattern differs because the substrate's
    /// mode-selector (Base256 / Text / C40) tie-breaks differently
    /// from BWIPP for mixed-content URIs. The produced symbol is
    /// spec-compliant and decodes to the same URI; the divergence is
    /// the same substrate-spec posture documented for plain
    /// `datamatrix` on arbitrary input (GOLDEN_COVERAGE.md §12).
    /// Pin the size and the corner finder/timing patterns —
    /// substrate-spec guarantees those.
    #[test]
    fn gs1_dl_datamatrix_matches_bwip_js_size_and_structure() {
        let uri = "https://id.gs1.org/01/04012345123456";
        let m = encode_gs1_dl_datamatrix(uri, &Options::default()).unwrap();
        assert_eq!(m.width(), 22, "expected 22-wide DM matching bwip-js");
        assert_eq!(m.height(), 22);

        // L-finder: left column and bottom row are solid bars.
        for y in 0..22 {
            assert!(m.get(0, y), "left finder bar at (0,{y}) should be set");
        }
        for x in 0..22 {
            assert!(m.get(x, 21), "bottom finder bar at ({x},21) should be set");
        }
        // Timing pattern: top row + right column alternate.
        let mut alternating = 0;
        for x in 1..22 {
            if m.get(x, 0) != m.get(x - 1, 0) {
                alternating += 1;
            }
        }
        assert!(
            alternating >= 10,
            "top row should alternate (timing pattern); got {alternating} transitions"
        );
    }

    #[test]
    fn gs1_dl_datamatrix_rejects_invalid_uri() {
        // Stage 11.A8c (cont) — upgrade 3 discriminant-only matches!
        // arms to multi-anchor pins via DlUriError Display impls at
        // lines 273-279 of util/gs1.rs.

        // Not http(s) → BadScheme: `GS1 DL URI: must start with
        // http:// or https://`.
        match encode_gs1_dl_datamatrix("ftp://example.com/01/04012345123456", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DL URI:"),
                    "BadScheme: missing `GS1 DL URI:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with http:// or https://"),
                    "BadScheme: missing scheme predicate: {msg}"
                );
            }
            other => panic!("ftp:// should reject as InvalidData, got {other:?}"),
        }

        // No AI path segment → NoAiInPath: `GS1 DL URI: no AI segments
        // found in path`.
        match encode_gs1_dl_datamatrix("http://example.com/foo", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DL URI:"),
                    "NoAi: missing `GS1 DL URI:` prefix: {msg}"
                );
                assert!(
                    msg.contains("no AI segments found in path"),
                    "NoAi: missing `no AI segments found in path` predicate: {msg}"
                );
                assert!(
                    !msg.contains("must start with"),
                    "NoAi: BadScheme diagnostic leaked: {msg}"
                );
            }
            other => panic!("/foo should reject as InvalidData, got {other:?}"),
        }

        // Bare text (no scheme at all) → BadScheme arm again.
        match encode_gs1_dl_datamatrix("just text", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DL URI:"),
                    "bare-text: missing `GS1 DL URI:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with http:// or https://"),
                    "bare-text: missing scheme predicate: {msg}"
                );
            }
            other => panic!("\"just text\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// GS1 Digital Link QR Code: same URI-validation contract, but the
    /// substrate is the (compatibility-exception) qrcode crate so we
    /// only pin the shape and the round-trip semantics, not exact
    /// module pixs.
    /// Dimension-corpus check across three GS1 DL URIs of varying
    /// lengths. The datamatrix substrate's preferred Data Matrix size
    /// for the raw URI string should match bwip-js's:
    ///
    ///   * 36-char URI  → 22×22
    ///   * 47-char URI  → 24×24
    ///   * 49-char URI  → 26×26
    ///
    /// Catches substrate-version bumps that change the encoder's
    /// size-selection policy for medium-length ASCII payloads.
    #[test]
    fn gs1_dl_datamatrix_size_matches_bwip_js_across_uri_lengths() {
        let cases: &[(&str, usize, usize)] = &[
            ("https://id.gs1.org/01/04012345123456", 22, 22),
            ("https://example.com/01/04012345123456?17=251231", 24, 24),
            ("https://id.gs1.org/01/04012345123456/21/SERIAL123", 26, 26),
        ];
        for &(uri, want_w, want_h) in cases {
            let m = encode_gs1_dl_datamatrix(uri, &Options::default())
                .unwrap_or_else(|e| panic!("DL DM({uri:?}) failed: {e}"));
            assert_eq!(
                (m.width(), m.height()),
                (want_w, want_h),
                "DL DM size drift for URI len={} ({uri:?})",
                uri.len(),
            );
        }
    }

    #[test]
    fn gs1_dl_qrcode_renders_and_rejects_invalid_uri() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
        // input echo for the DL QR happy path.
        let uri = "https://id.gs1.org/01/04012345123456";
        let m = encode_gs1_dl_qrcode(uri, &Options::default()).unwrap_or_else(|e| {
            panic!("encode_gs1_dl_qrcode({uri:?}) (GS1 Digital Link → QR Code, canonical id.gs1.org GTIN-14) must succeed; got Err: {e}")
        });
        assert!(
            m.width() >= 21,
            "QR symbol size should be at least 21 (Version 1)"
        );
        assert_eq!(m.width(), m.height(), "QR is square");

        // Same rejection paths as the DM variant.
        //
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 2-anchor pin via DlUriError::BadScheme display impl at
        // line 273 of util/gs1.rs (`GS1 DL URI: must start with
        // http:// or https://`).
        match encode_gs1_dl_qrcode("not a uri", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DL URI:"),
                    "missing `GS1 DL URI:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with http:// or https://"),
                    "missing scheme predicate: {msg}"
                );
            }
            other => panic!("\"not a uri\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// `encode_gs1_datamatrix_rectangular` should produce a rectangular
    /// (width > height) symbol for the canonical AI payload and reject
    /// the same bad-AI input as the square variant.
    #[test]
    fn gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai() {
        let m =
            encode_gs1_datamatrix_rectangular("(01)04012345123456", &Options::default()).unwrap();
        assert!(
            m.width() > m.height(),
            "GS1 DM rectangular should be wider than tall; got {}×{}",
            m.width(),
            m.height()
        );

        // Stage 11.A8c (cont) — mirror the square-variant pin at
        // line 481 (gs1_datamatrix_rejects_bad_ai). Both surface the
        // same ParseError::InvalidAi diagnostic.
        match encode_gs1_datamatrix_rectangular("(99999)X", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("GS1 parse:"), "missing prefix: {msg}");
                assert!(
                    msg.contains("invalid AI \"99999\""),
                    "missing token echo: {msg}"
                );
                assert!(msg.contains("2-4 digits"), "missing range hint: {msg}");
            }
            other => panic!("(99999)X (rectangular) should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn ntin_wraps_with_8003() {
        // Stage 11.A8c (cont) — descriptive label naming NTIN auto-wrap.
        let m = encode_ntin("00012345678905", &Options::default()).unwrap();
        assert!(
            m.width() >= 10,
            "encode_ntin(\"00012345678905\") (bare 14-digit NTIN → DataMatrix via AI=8003 auto-wrap) must produce symbol ≥10 modules wide; got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn ntin_accepts_explicit_ai() {
        // Stage 11.A8c (cont) — descriptive label naming NTIN explicit
        // AI path (pass-through, no auto-wrap).
        let m = encode_ntin("(8003)00012345678905", &Options::default()).unwrap();
        assert!(
            m.width() >= 10,
            "encode_ntin(\"(8003)00012345678905\") (explicit AI=8003, pass-through) must produce symbol ≥10 modules wide; got {}×{}",
            m.width(),
            m.height()
        );
    }

    /// NTIN is a thin wrapper that prepends `(8003)` to bare digits
    /// (or passes through an explicit `(8003)...` form) and delegates
    /// to `encode_gs1_datamatrix`. Anchor the byte-equality so the
    /// composition can't drift through a refactor.
    #[test]
    fn ntin_composes_8003_and_gs1_datamatrix() {
        let bare = "00012345678905";
        let got = encode_ntin(bare, &Options::default()).unwrap();
        let want = encode_gs1_datamatrix(&format!("(8003){bare}"), &Options::default()).unwrap();
        assert_eq!(got.width(), want.width());
        assert_eq!(got.height(), want.height());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    #[test]
    fn ppn_renders_with_envelope() {
        // Stage 11.A8c (cont) — descriptive label naming PPN envelope.
        let m = encode_ppn("110375286414", &Options::default()).unwrap();
        assert!(
            m.width() >= 10,
            "encode_ppn(\"110375286414\") (12-digit PPN → DataMatrix via ANSI MH10.8.2 envelope [)>RS 06 GS 9N<ppn> RS EOT) must produce symbol ≥10 modules wide; got {}×{}",
            m.width(),
            m.height()
        );
    }

    /// PPN composes the ANSI MH10.8.2 envelope `[)>RS 06 GS 9N<ppn> RS
    /// EOT` and feeds the raw bytes through the `datamatrix` crate.
    /// PPN isn't in BWIPP (no upstream encoder to byte-match
    /// against), but the envelope construction itself is what
    /// distinguishes PPN from a plain Data Matrix — pin the produced
    /// byte stream so a regression in the envelope layout fails the
    /// test.
    #[test]
    fn ppn_envelope_bytes_are_correct() {
        let payload = "110375286414";
        // Reconstruct the expected envelope manually to lock down
        // the exact byte sequence the encoder feeds into DataMatrix.
        let mut want: Vec<u8> = Vec::new();
        want.extend_from_slice(b"[)>");
        want.push(0x1E); // RS
        want.extend_from_slice(b"06");
        want.push(0x1D); // GS
        want.extend_from_slice(b"9N");
        want.extend_from_slice(payload.as_bytes());
        want.push(0x1E); // RS
        want.push(0x04); // EOT
                         // Compare against what our DataMatrix substrate produces
                         // when given the same bytes — composition pin.
                         // encode_ppn now defaults to square shape (matches BWIPP). Use
                         // the same constraint here so the byte-for-byte comparison is
                         // apples-to-apples.
        let dm = DataMatrix::encode(&want, SymbolList::default().enforce_square())
            .expect("PPN substrate encode");
        let bitmap = dm.bitmap();
        let m = encode_ppn(payload, &Options::default()).unwrap();
        assert_eq!(m.width(), bitmap.width());
        assert_eq!(m.height(), bitmap.height());
        // Build a hash-set of substrate "on" pixels for a symmetric diff —
        // the one-way `for pixel in bitmap.pixels()` check would miss the
        // wrapper drawing modules the substrate doesn't.
        let substrate_on: std::collections::HashSet<(usize, usize)> = bitmap.pixels().collect();
        for y in 0..m.height() {
            for x in 0..m.width() {
                let wrapper_has = m.get(x, y);
                let substrate_has = substrate_on.contains(&(x, y));
                assert_eq!(
                    wrapper_has, substrate_has,
                    "PPN pixel mismatch at ({x},{y}): wrapper={wrapper_has} substrate={substrate_has}"
                );
            }
        }
    }

    #[test]
    fn ppn_rejects_empty() {
        // Stage 11.A8c (cont) — upgrade discriminant-only matches!
        // to 2-anchor pin matching the source diagnostic at line
        // 135 of gs1_2d.rs (`PPN payload must not be empty`).
        // Note: the sibling test `ppn_rejects_empty_with_diagnostic_
        // and_trim` (below this test) already pins the diagnostic
        // for whitespace-only inputs via msg.contains("PPN") &&
        // msg.contains("must not be empty"). This test independently
        // pins the literal-empty path with the same anchor pair.
        match encode_ppn("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("PPN"), "missing `PPN` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
            }
            other => panic!("empty PPN should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — strengthen `ppn_rejects_empty` with diagnostic
    /// substring + whitespace-trim coverage. The empty-payload arm at
    /// `encode_ppn` line 134-136 has the diagnostic "PPN payload must
    /// not be empty" — without pinning it, a mutant message could
    /// drift unnoticed. Also `payload = data.trim()` (line 133) was
    /// uncovered: a whitespace-only input must reject as empty.
    ///
    /// Anchors:
    ///   * "" → InvalidData with "PPN payload must not be empty".
    ///   * "   " (whitespace-only) → same diagnostic (catches missing
    ///     trim().
    ///   * "\t\n " (mixed whitespace) → same diagnostic.
    ///   * "  abc  " (trimmed to "abc") → succeeds (catches a mutant
    ///     that aborts on any whitespace, not just whitespace-only).
    #[test]
    fn ppn_rejects_empty_with_diagnostic_and_trim() {
        // Plain empty.
        match encode_ppn("", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("PPN") && msg.contains("must not be empty"),
                "empty PPN should pin diagnostic, got: {msg}"
            ),
            other => panic!("empty PPN should reject, got {other:?}"),
        }

        // Whitespace-only — must trim first, then reject as empty.
        match encode_ppn("   ", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("PPN") && msg.contains("must not be empty"),
                "whitespace-only PPN should reject as empty after trim, got: {msg}"
            ),
            other => panic!("whitespace-only PPN should reject, got {other:?}"),
        }

        // Mixed whitespace (tab + newline + space).
        //
        // Stage 11.A8c (cont) — anchor parity with the empty and
        // whitespace-only siblings (lines 770, 779) by adding the
        // `PPN` symbology-prefix anchor. The original 1-anchor pin
        // would survive a mutation that drops or substitutes the
        // `PPN` prefix in the format string.
        match encode_ppn("\t\n ", &Options::default()) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("PPN") && msg.contains("must not be empty"),
                "mixed whitespace PPN should reject, got: {msg}"
            ),
            other => panic!("mixed whitespace PPN should reject, got {other:?}"),
        }

        // Surrounding whitespace stripped — non-empty payload succeeds.
        // Compare against the canonical (no-whitespace) form to make
        // sure trim() not only doesn't reject but actually trims.
        let trimmed = encode_ppn("110375286414", &Options::default()).unwrap();
        let padded = encode_ppn("  110375286414  ", &Options::default()).unwrap();
        assert_eq!(
            (padded.width(), padded.height()),
            (trimmed.width(), trimmed.height()),
            "surrounding whitespace should be trimmed (same symbol size as canonical)"
        );
    }

    #[test]
    fn gs1_qrcode_encodes_gtin() {
        // Stage 11.A8c (cont) — descriptive label naming QR version floor.
        let m = encode_gs1_qrcode("(01)04012345123456", &Options::default()).unwrap();
        assert!(
            m.width() >= 21,
            "encode_gs1_qrcode(\"(01)04012345123456\") (single GTIN-14) must produce QR symbol ≥21 modules wide (V1 minimum); got {}×{}",
            m.width(),
            m.height()
        );
    }

    #[test]
    fn gs1_qrcode_optimal_segmentation_matches_bwipp_size() {
        // The 16-digit GTIN payload `(01)04012345123456` fits in
        // QR Version 1 (21x21) only when the encoder uses numeric
        // mode for the digit run. Byte mode produces 128 data bits
        // and forces Version 2 (25x25). BWIPP picks V1; this test
        // pins our behavior to match.
        let m = encode_gs1_qrcode("(01)04012345123456", &Options::default()).unwrap();
        assert_eq!(m.width(), 21, "must use Version 1 (numeric mode)");
        assert_eq!(m.height(), 21);
    }

    #[test]
    fn gs1_qrcode_with_explicit_version_override() {
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "5".into()));
        let m = encode_gs1_qrcode("(01)04012345123456", &opts).unwrap();
        // Version 5 → 37x37.
        assert_eq!(m.width(), 37);
    }

    #[test]
    fn gs1_qrcode_rejects_bad_eclevel() {
        // Stage 11.A8c (cont) — upgrade single-anchor pin to a
        // 4-anchor pin matching the source diagnostic at line 214 of
        // gs1_2d.rs (`eclevel={other}`):
        //   1. `eclevel=` prefix (proves the format template, not a
        //      generic `bad eclevel: Z` rewrite, was hit).
        //   2. `=Z` value echo (proves `{other}` interpolates the
        //      offending value).
        //   3. NOT `version=` — the sibling InvalidOption arm at
        //      line 227 uses `version={v}`; this guards against
        //      arm-swap mutations.
        //   4. NOT `eclevel=M` — proves the diagnostic doesn't fall
        //      through to the default ("M") arm's value echo.
        let mut opts = Options::default();
        opts.extras.push(("eclevel".into(), "Z".into()));
        match encode_gs1_qrcode("(01)04012345123456", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(msg.contains("eclevel="), "missing `eclevel=` prefix: {msg}");
                assert!(msg.contains("=Z"), "missing `=Z` value echo: {msg}");
                assert!(
                    !msg.contains("version="),
                    "wrong arm — version-override diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("eclevel=M"),
                    "wrong arm — default-value `eclevel=M` leaked (the `Z` should be the echoed value, not the fallback default): {msg}"
                );
            }
            other => panic!("eclevel=Z should reject as InvalidOption, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the version-override range check in
    /// `encode_gs1_qrcode_substrate`. The existing `_with_explicit_version_override`
    /// test only exercises a valid version (5). The error arms for
    /// out-of-range and unparseable versions were untested.
    ///
    /// The version filter is `(1..=40).contains(&n)`. Anchors:
    ///   - "0" → below 1, out of range.
    ///   - "41" → above 40, out of range.
    ///   - "abc" → fails i16::parse.
    ///   - "" → fails i16::parse.
    ///   - "1" and "40" → valid boundaries (both succeed).
    ///
    /// Mutations to catch:
    ///   - `(1..=40)` → `(1..40)`: rejects version=40.
    ///   - `(1..=40)` → `(0..=40)`: accepts version=0 (out of spec).
    ///   - `(1..=40)` → `(1..=39)`: rejects 40.
    ///   - `i16::parse` → `u8::parse`: would accept up to 255, then
    ///     filter to 40, so similar to current behaviour — but a
    ///     negative-looking input is currently rejected too.
    #[test]
    fn gs1_qrcode_rejects_out_of_range_or_unparseable_version() {
        // Under `prefer-native-qrcode` (default), encode_gs1_qrcode
        // dispatches to qrcode_native which uses TWO distinct rejection
        // arms (qrcode_native/mod.rs:4146 + 4148-4150):
        //   * Parse-fail arm: format!("version={ver_str}")   — for
        //     non-numeric inputs like "abc" or "" (parse fails first).
        //   * Range arm: format!("QR version must be 1..=40, got {N}")
        //     — for parseable integers outside [1, 40].
        //
        // Per-iteration pinning picks the correct anchor based on
        // whether the value parses as an i32:
        //   "0", "41", "100", "-1"  → range arm with value echo.
        //   "abc", ""                → parse-fail arm with raw echo.
        let bad_versions = ["0", "41", "100", "abc", "", "-1"];
        for v in bad_versions {
            let mut opts = Options::default();
            opts.extras.push(("version".into(), v.to_string()));
            let res = encode_gs1_qrcode("(01)04012345123456", &opts);
            match res {
                Err(Error::InvalidOption(msg)) => match v.parse::<u8>() {
                    // The native parser uses `u8` (qrcode_native/mod.rs:4144),
                    // so negative inputs fail at parse — not at the range check.
                    Ok(n) => {
                        // Range arm — echoes the parsed u8.
                        let want = format!("got {n}");
                        assert!(
                            msg.contains("QR version must be 1..=40"),
                            "version={v:?} range arm must carry 1..=40 hint; got {msg}"
                        );
                        assert!(
                            msg.contains(&want),
                            "version={v:?} range arm must echo parsed value as {want:?}; got {msg}"
                        );
                    }
                    Err(_) => {
                        // Parse-fail arm — echoes the raw string verbatim.
                        let want = format!("version={v}");
                        assert!(
                            msg.contains(&want),
                            "version={v:?} parse-fail arm must echo raw string as {want:?}; got {msg}"
                        );
                    }
                },
                other => panic!("version={v:?} should reject as InvalidOption, got {other:?}"),
            }
        }
        // Boundary: 1 and 40 must succeed.
        for v in ["1", "40"] {
            let mut opts = Options::default();
            opts.extras.push(("version".into(), v.to_string()));
            let res = encode_gs1_qrcode("(01)04012345123456", &opts);
            assert!(
                res.is_ok(),
                "version={v:?} should be accepted (boundary), got {res:?}"
            );
        }
    }

    /// Pin the FNC1-first-position emission directly through the qrcode
    /// crate's `Bits` API: we build the same bit stream
    /// `encode_gs1_qrcode` builds and assert the leading 4 bits are
    /// `0101` (ISO/IEC 18004 Annex L). If a future refactor of
    /// `encode_gs1_qrcode` accidentally drops `push_fnc1_first_position`,
    /// this test still fires only at the encoder level — but the
    /// `differs_from_plain_qr_of_same_payload` test below cross-checks
    /// the encoder output itself.
    #[test]
    fn gs1_qrcode_fnc1_first_position_mode_indicator_is_0101() {
        use qrcode::bits::Bits;
        use qrcode::types::{EcLevel, Version};

        let mut bits = Bits::new(Version::Normal(1));
        bits.push_fnc1_first_position().unwrap();
        bits.push_optimal_data(b"010401234512345617").unwrap();
        bits.push_terminator(EcLevel::M).unwrap();
        let bytes = bits.into_bytes();
        // FNC1-first-position mode indicator = `0101` in the top 4 bits
        // of the first byte; the remaining low nibble starts the data
        // segment header.
        assert_eq!(
            bytes[0] >> 4,
            0b0101,
            "expected FNC1-first-position mode indicator (0101) in top nibble"
        );
    }

    /// `encode_gs1_qrcode` must produce a *different* QR symbol from
    /// `encode_qrcode` over the same FNC1-separated byte stream. The
    /// difference is exactly the leading mode indicator (`0101`
    /// FNC1-first-position vs `0100` byte mode), which propagates
    /// through the entire codeword stream, ECC, and final module
    /// pattern. If a regression silently drops the FNC1 indicator
    /// (e.g. someone refactors out `push_fnc1_first_position`), both
    /// encoders would emit identical symbols and the GS1 semantics
    /// would be invisible to spec-compliant scanners.
    #[test]
    fn gs1_qrcode_differs_from_plain_qr_of_same_payload() {
        use crate::symbology::qrcode_;
        use crate::util::gs1;
        let input = "(01)04012345123456";
        let elements = gs1::parse(input).unwrap();
        let payload = gs1::encode_with_fnc1(&elements);
        // Pretty-print the FNC1-separated payload the way encode_qrcode
        // would receive it: bytes including the leading 0x1d.
        let payload_str = String::from_utf8_lossy(&payload).into_owned();

        let gs1_matrix = encode_gs1_qrcode(input, &Options::default()).unwrap();
        let plain_matrix = qrcode_::encode(&payload_str, &Options::default()).unwrap();

        // Same payload, same Version 1 size — the symbols only differ
        // at the bit level via the mode indicator + downstream
        // codewords + module mapping.
        assert_eq!(gs1_matrix.width(), plain_matrix.width());
        assert_eq!(gs1_matrix.height(), plain_matrix.height());

        let mut different = false;
        for y in 0..gs1_matrix.height() {
            for x in 0..gs1_matrix.width() {
                if gs1_matrix.get(x, y) != plain_matrix.get(x, y) {
                    different = true;
                    break;
                }
            }
            if different {
                break;
            }
        }
        assert!(
            different,
            "encode_gs1_qrcode produced identical output to plain QR — \
             FNC1-first-position mode indicator may have regressed"
        );
    }

    /// AI element string round-trip: parse, encode (which strips the
    /// leading FNC1 since the mode indicator replaces it), and verify
    /// the byte stream the encoder feeds into the QR substrate ends
    /// with the AI data we asked for. This pins the encoder's payload
    /// transformation independently of the final mask choice.
    #[test]
    fn gs1_qrcode_payload_round_trips_through_ai_parser() {
        use crate::util::gs1;
        let input = "(01)04012345123456";
        let elements = gs1::parse(input).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].ai, "01");
        assert_eq!(elements[0].data, "04012345123456");

        let payload = gs1::encode_with_fnc1(&elements);
        // Leading byte is the FNC1 separator (0x1d) — the encoder
        // strips it and lets the FNC1-first-position mode indicator
        // serve that role.
        assert_eq!(payload[0], 0x1d);
        // Trailing bytes are the AI digits + GTIN payload.
        assert_eq!(&payload[1..], b"0104012345123456");
    }

    /// Kills `encode_gs1_datamatrix_rectangular: replace != with ==`
    /// at line ~78 (the `retain(|(k, _)| k != "shape")` filter).
    /// Same pattern as the mailmark / hibc extras-filter survivors:
    /// pass an unrelated `version` extra that the encoder should let
    /// through, and assert the symbol size honours it (under the
    /// mutant `k == "shape"` filter, only "shape" extras are kept
    /// and the user's version is dropped, so the encoder picks an
    /// auto-default size instead of the requested one).
    #[test]
    fn encode_gs1_datamatrix_rectangular_preserves_unrelated_extras() {
        // Use a small payload so default auto-sizing picks the smallest
        // rectangle (8x18). Then force version=8x48 — under the
        // original filter the user's version survives and we get 8x48.
        // Under the mutant the version is dropped → auto-sized.
        let default =
            encode_gs1_datamatrix_rectangular("(01)04012345123456", &Options::default()).unwrap();
        let with_version = encode_gs1_datamatrix_rectangular(
            "(01)04012345123456",
            &Options::default().with("version", "16x36"),
        )
        .unwrap();
        assert_ne!(
            (default.width(), default.height()),
            (with_version.width(), with_version.height()),
            "encode_gs1_datamatrix_rectangular dropped the user-supplied `version` extra; \
             the `retain(k != \"shape\")` filter may have flipped"
        );
    }
}
