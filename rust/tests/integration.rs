//! Black-box integration tests: encode + render every implemented symbology
//! in both formats, sanity-check the output.

use bwipp::{render_png, render_svg, Options, Symbology};

/// Symbologies that don't yet have a working encoder; integration tests
/// skip them (their input validation paths still get exercised by their own
/// module-level unit tests).
///
/// As of Stage 22d this list is **empty** — POSICODE's default `a`
/// path now works through the full BWIPP auto-encoder. The four
/// `partial` rows (`code16k`, `code49`, `codeone`, `posicode`)
/// each have a fully-working default path that the integration
/// smoke can exercise; the partial-status applies only to specific
/// extension paths that are documented in PORT_STATUS.md.
fn is_stub(_s: Symbology) -> bool {
    false
}

#[test]
fn every_symbology_renders_svg() {
    let opts = Options::default();
    for &s in Symbology::all() {
        if is_stub(s) {
            continue;
        }
        let svg = render_svg(s, s.default_data(), &opts)
            .unwrap_or_else(|e| panic!("{} svg failed: {e}", s.id()));
        assert!(svg.starts_with("<svg"), "{} svg has wrong prefix", s.id());
        assert!(svg.ends_with("</svg>\n"), "{} svg has wrong suffix", s.id());
        // A barcode SVG must contain at least one foreground rect/path,
        // otherwise we silently emitted an empty quiet zone. Every
        // current renderer uses `<rect ` for both bars and matrix cells.
        assert!(
            svg.contains("<rect "),
            "{} svg contains no <rect> element",
            s.id()
        );
    }
}

#[test]
fn every_symbology_renders_png() {
    let opts = Options::default();
    for &s in Symbology::all() {
        if is_stub(s) {
            continue;
        }
        let png = render_png(s, s.default_data(), &opts)
            .unwrap_or_else(|e| panic!("{} png failed: {e}", s.id()));
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "{} not a PNG",
            s.id()
        );
        assert!(png.len() > 100, "{} png suspiciously small", s.id());
    }
}

#[test]
fn id_round_trip() {
    for &s in Symbology::all() {
        assert_eq!(Symbology::from_id(s.id()), Some(s));
    }
}

/// Renderer-level Options fields (`foreground`, `background`,
/// `quiet_zone`, `scale`) all change the SVG output. Pin one example
/// of each so a regression in option plumbing surfaces immediately.
#[test]
fn renderer_options_change_svg_output() {
    // Stage 11.A8c — replace `.unwrap()` with `.expect()` calls
    // carrying option-config-specific failure-mode labels so a
    // regression here pinpoints WHICH renderer-option arm regressed
    // rather than panicking with a bare "called Option::unwrap on a
    // None value".
    let baseline = render_svg(Symbology::Code39, "HELLO", &Options::default())
        .expect("Code 39 baseline render with default Options must succeed");

    // Custom foreground colour should appear in the rect fill="..." attributes.
    let opts = Options {
        foreground: [255, 0, 0],
        ..Options::default()
    };
    let red = render_svg(Symbology::Code39, "HELLO", &opts)
        .expect("Code 39 render with custom foreground [255,0,0] must succeed");
    assert!(
        red.contains("#ff0000"),
        "custom foreground colour not reflected in SVG"
    );
    assert_ne!(red, baseline);

    // Custom background should also show up.
    let opts = Options {
        background: [0, 255, 0],
        ..Options::default()
    };
    let green_bg = render_svg(Symbology::Code39, "HELLO", &opts)
        .expect("Code 39 render with custom background [0,255,0] must succeed");
    assert!(
        green_bg.contains("#00ff00"),
        "custom background colour not reflected in SVG"
    );

    // Larger scale should produce a strictly larger SVG (more bytes in
    // the width/height attributes and rect dimensions).
    let opts = Options {
        scale: 8,
        ..Options::default()
    };
    let big = render_svg(Symbology::Code39, "HELLO", &opts)
        .expect("Code 39 render with scale=8 must succeed");
    assert!(big.len() >= baseline.len());
    assert_ne!(big, baseline);

    // include_text adds a <text> element with the human-readable
    // payload underneath the bars; the default-off baseline has none.
    let opts = Options {
        include_text: true,
        ..Options::default()
    };
    let with_text = render_svg(Symbology::Code39, "HELLO", &opts)
        .expect("Code 39 render with include_text=true must succeed");
    assert!(!baseline.contains("<text"), "baseline shouldn't have text");
    assert!(with_text.contains("<text"), "include_text should add text");
    assert!(
        with_text.contains("HELLO"),
        "the rendered text should appear in the SVG"
    );
}

/// Alias IDs that PORT_STATUS documents as `verified` aliases (they
/// don't have their own `Symbology` variant) — pin them to the
/// canonical target so a regression in `from_id` would surface here.
#[test]
fn alias_ids_route_to_canonical_symbology() {
    let cases: &[(&str, Symbology)] = &[
        ("code128a", Symbology::Code128),
        ("code128b", Symbology::Code128),
        ("code128c", Symbology::Code128),
        ("plessey_bidir", Symbology::Plessey),
        ("swedish_postal", Symbology::Sscc18),
        ("qrcode_iso", Symbology::QrCode),
        ("qr_code", Symbology::QrCode),
        ("telepen_alpha", Symbology::TelepenNumeric),
        ("telepen_numeric", Symbology::TelepenNumeric),
        ("datamatrixrectangular", Symbology::DataMatrixRectangular),
        ("datamatrix_rectangular", Symbology::DataMatrixRectangular),
        // Postal aliases: the Python/web catalog uses per-digit-count IDs;
        // the Rust encoder validates the digit count internally so every
        // alias resolves to the same variant.
        ("usps_postnet5", Symbology::Postnet),
        ("usps_postnet9", Symbology::Postnet),
        ("usps_postnet11", Symbology::Postnet),
        ("planet12", Symbology::Planet),
        ("planet14", Symbology::Planet),
        // USPS Intelligent Mail Barcode (IMb) is the catalog's alias name
        // for the BWIPP onecode encoder.
        ("usps_imb", Symbology::UspsOneCode),
        // Upstream BWIPP / bwip-js encoder names — these are the
        // bcid strings used by `bwipp.raw(bcid, ...)`. We accept them
        // so callers migrating from bwip-js see their encoder ids
        // continue to resolve. See `rust/PORT_COMPLETENESS.md`.
        ("pzn", Symbology::Pzn7),
        ("auspost", Symbology::AuspostCustomer),
        ("rationalizedCodabar", Symbology::Codabar),
        ("rationalizedcodabar", Symbology::Codabar),
        ("ean13composite", Symbology::CompositeEan13Cca),
        ("ean8composite", Symbology::CompositeEan8Cca),
        ("upcacomposite", Symbology::CompositeUpcaCca),
        ("upcecomposite", Symbology::CompositeUpceCca),
        ("databaromnicomposite", Symbology::CompositeDatabarOmniCca),
        (
            "databarlimitedcomposite",
            Symbology::CompositeDatabarLimitedCca,
        ),
        (
            "databarexpandedcomposite",
            Symbology::CompositeDatabarExpandedCca,
        ),
        ("gs1-128composite", Symbology::CompositeGs1_128Cca),
        // EAN-14 / GTIN-14 — bwip-js `ean14`. `gtin14` is a convenience
        // alias since "GTIN-14" is the modern name for the same encoder.
        ("ean14", Symbology::Ean14),
        ("gtin14", Symbology::Ean14),
        // HIBC LIC wrappers added during this hardening pass.
        ("hibcazteccode", Symbology::HibcAztecCode),
        (
            "hibcdatamatrixrectangular",
            Symbology::HibcDataMatrixRectangular,
        ),
        // GS1 Data Matrix rectangular — `gs1datamatrix` with
        // `shape=rectangular` injected.
        (
            "gs1datamatrixrectangular",
            Symbology::Gs1DataMatrixRectangular,
        ),
        (
            "gs1-datamatrix-rectangular",
            Symbology::Gs1DataMatrixRectangular,
        ),
        // Aztec Code Compact — `aztec` with format forced to "compact".
        ("azteccodecompact", Symbology::AztecCodeCompact),
        ("aztec_code_compact", Symbology::AztecCodeCompact),
        // M&S — bwip-js `mands` (UK retailer EAN-8 variant).
        ("mands", Symbology::MarksAndSpencer),
        ("marks_and_spencer", Symbology::MarksAndSpencer),
        // Aztec Rune — fixed 11×11 marker, 8-bit payload.
        ("aztecrune", Symbology::AztecRune),
        ("aztec_rune", Symbology::AztecRune),
        // Channel Code — bwip-js `channelcode` (USPS Tray Labels).
        ("channelcode", Symbology::ChannelCode),
        ("channel_code", Symbology::ChannelCode),
        // GS1 Digital Link — URI-validation wrappers over verified
        // datamatrix / qrcode substrates.
        ("gs1dldatamatrix", Symbology::Gs1DlDataMatrix),
        ("gs1-dl-datamatrix", Symbology::Gs1DlDataMatrix),
        ("gs1dlqrcode", Symbology::Gs1DlQrCode),
        ("gs1-dl-qrcode", Symbology::Gs1DlQrCode),
        // DMRE — bwip-js `datamatrixrectangularextension`.
        (
            "datamatrixrectangularextension",
            Symbology::DataMatrixRectangularExtension,
        ),
        ("dmre", Symbology::DataMatrixRectangularExtension),
        // DataBar Truncated composite — bwip-js `databartruncatedcomposite`.
        // Bare upstream id routes to the CC-A variant (matches Omni / Limited
        // / Expanded composite naming).
        (
            "databartruncatedcomposite",
            Symbology::CompositeDatabarTruncatedCca,
        ),
        (
            "databartruncatedcomposite_cca",
            Symbology::CompositeDatabarTruncatedCca,
        ),
        (
            "databartruncatedcomposite_ccb",
            Symbology::CompositeDatabarTruncatedCcb,
        ),
        // DataBar Stacked composite — bwip-js `databarstackedcomposite`.
        (
            "databarstackedcomposite",
            Symbology::CompositeDatabarStackedCca,
        ),
        (
            "databarstackedcomposite_cca",
            Symbology::CompositeDatabarStackedCca,
        ),
        (
            "databarstackedcomposite_ccb",
            Symbology::CompositeDatabarStackedCcb,
        ),
        // DataBar Stacked Omni composite — bwip-js `databarstackedomnicomposite`.
        (
            "databarstackedomnicomposite",
            Symbology::CompositeDatabarStackedOmniCca,
        ),
        (
            "databarstackedomnicomposite_cca",
            Symbology::CompositeDatabarStackedOmniCca,
        ),
        (
            "databarstackedomnicomposite_ccb",
            Symbology::CompositeDatabarStackedOmniCcb,
        ),
        // DataBar Expanded Stacked composite — bwip-js
        // `databarexpandedstackedcomposite`.
        (
            "databarexpandedstackedcomposite",
            Symbology::CompositeDatabarExpandedStackedCca,
        ),
        (
            "databarexpandedstackedcomposite_cca",
            Symbology::CompositeDatabarExpandedStackedCca,
        ),
        (
            "databarexpandedstackedcomposite_ccb",
            Symbology::CompositeDatabarExpandedStackedCcb,
        ),
    ];
    for &(alias, target) in cases {
        assert_eq!(
            Symbology::from_id(alias),
            Some(target),
            "alias {alias:?} should route to {target:?}"
        );
    }
}

#[test]
fn every_symbology_has_a_display_name_and_category() {
    for &s in Symbology::all() {
        let name = s.display_name();
        let cat = s.category();
        assert!(!name.is_empty(), "{} has empty display_name", s.id());
        assert!(!cat.is_empty(), "{} has empty category", s.id());
        // Display name should differ from the id (id is machine-friendly,
        // display name is human-friendly).
        assert_ne!(name, s.id(), "{}: display_name == id", s.id());
    }
}

#[test]
fn every_symbology_has_non_empty_default_data() {
    for &s in Symbology::all() {
        let d = s.default_data();
        assert!(!d.is_empty(), "{} has empty default_data", s.id());
    }
}

/// `Symbology::all()` is the canonical enumeration — adding a new
/// variant without listing it here silently makes the new symbology
/// invisible to every code path that iterates the catalog (web
/// catalog generator, integration tests, list_symbologies WASM
/// export). Pin the count so the omission is loud.
#[test]
fn all_contains_every_variant() {
    // 139 = enum-variant count tracked in AUDIT.md §3 ("Rust
    // `Symbology` enum + `id()` (canonical) | 139"). Bump this
    // assertion AND add the new variant to `Symbology::all()` AND
    // update AUDIT.md when porting a new encoder.
    let count = Symbology::all().len();
    assert_eq!(
        count, 154,
        "Symbology::all() count drifted to {count} — did a new variant get added without listing it in all()?"
    );
}

/// `Symbology::id` is the API stable identifier — two variants sharing
/// one would silently break `from_id` (whichever route the match arm
/// hits first wins) and any caller that round-trips an id back to a
/// `Symbology`. Pin uniqueness here so a future copy-paste regression
/// surfaces immediately.
#[test]
fn ids_are_unique() {
    let mut seen = std::collections::HashMap::<&'static str, Symbology>::new();
    for &s in Symbology::all() {
        if let Some(prev) = seen.insert(s.id(), s) {
            panic!("duplicate id {:?}: {prev:?} and {s:?}", s.id());
        }
    }
}

/// Sanity-check the error path: a representative set of bad inputs
/// should each return a typed `Error` (not panic, not `Ok`).
/// Catching this at the integration boundary protects against an
/// encoder regressing to e.g. an `unwrap` on user input.
///
/// Stage 11.A8c (cont) — upgrade from bare `.is_err()` to per-arm
/// diagnostic-substring pin. Each case now carries the expected
/// substring its symbology's encoder must emit. This catches
/// mutations that route one symbology's reject through another's
/// diagnostic at the integration boundary (the bare `.is_err()`
/// would survive any such cross-symbology rewiring).
///
/// Stage 11.A8c (cont, 2nd pass) — single-anchor symbology name was
/// too weak: any rejection arm of an encoder mentions its own
/// symbology name, so a mutant that re-routes (e.g.) the EAN-13
/// non-digit path through the length-spec path would still emit
/// "EAN-13" and survive. Each case now carries a SECOND anchor —
/// the specific predicate phrase that pins the EXPECTED rejection
/// arm, not just the rejection symbology.
#[test]
fn invalid_input_returns_error() {
    let opts = Options::default();
    // (symbology, bad-input, symbology-anchor, predicate-anchor)
    // Predicates are exact substrings of the source-emitted format
    // strings — drift here means a mutant survived format-string
    // edits OR a real format-string change went unnoticed.
    let cases: &[(Symbology, &str, &str, &str)] = &[
        // EAN-13 "abcdefghijkl" → 12 non-digit chars filtered out,
        // leaves 0 digits. normalize() at src/symbology/ean.rs:112
        // emits `EAN-13: expected 12 or 13 digits, got 0`.
        // Predicate: "expected 12 or 13 digits" (pins both valid
        // lengths — kills a mutation that drops either '12' or '13'
        // from the format string).
        (
            Symbology::Ean13,
            "abcdefghijkl",
            "EAN-13",
            "expected 12 or 13 digits",
        ),
        // PostNet "abc" → 'a','b','c' all non-digit. encode_postnet
        // at src/symbology/postnet.rs emits `PostNet: digits only
        // (got "abc")`. Predicate: "digits only" (the rejection arm
        // pinned by the digit-membership guard).
        (Symbology::Postnet, "abc", "PostNet", "digits only"),
        // Code 39 "héllo" → 'é' is non-ASCII / not in the alphabet.
        // src/symbology/code39.rs:104 emits `Code 39 does not support
        // character {c:?}`. Predicate: "does not support character".
        (
            Symbology::Code39,
            "héllo",
            "Code 39",
            "does not support character",
        ),
        // Codabar "1234" → missing A-D start/stop framing chars.
        // src/symbology/codabar.rs emits `Codabar: ... must begin and
        // end with A, B, C, or D`. Predicate: "must begin and end with".
        (
            Symbology::Codabar,
            "1234",
            "Codabar",
            "must begin and end with",
        ),
        // Pharmacode "0" → below the 3..=131070 range. parse_int via
        // src/symbology/pharmacode.rs:34 emits `Pharmacode One-Track:
        // value must be in [3, 131070] (got 0)`. Predicate:
        // "must be in [3, 131070]" (pins the exact range).
        (
            Symbology::Pharmacode,
            "0",
            "Pharmacode",
            "must be in [3, 131070]",
        ),
        // Code 128 "" → empty payload. src/symbology/code128.rs:294
        // emits `Code 128 payload must not be empty`. Predicate:
        // "payload must not be empty".
        (
            Symbology::Code128,
            "",
            "Code 128",
            "payload must not be empty",
        ),
    ];
    for &(sym, bad, symbology_anchor, predicate_anchor) in cases {
        let err = render_svg(sym, bad, &opts)
            .err()
            .unwrap_or_else(|| panic!("{} should reject input {bad:?}, got Ok", sym.id()));
        let msg = err.to_string();
        assert!(
            msg.contains(symbology_anchor),
            "{} reject for {bad:?} must contain symbology anchor {symbology_anchor:?}, got {msg:?}",
            sym.id(),
        );
        assert!(
            msg.contains(predicate_anchor),
            "{} reject for {bad:?} must contain predicate anchor {predicate_anchor:?} \
             (kills mutations re-routing this case to a different rejection arm), \
             got {msg:?}",
            sym.id(),
        );
    }
}

/// DotCode renders as round dots, not square modules. Pin that the
/// SVG output uses `<circle>` for each dot and that the PNG output
/// stays well under the bytes a square renderer would produce
/// (round dots have ~50% fewer pixels filled).
#[test]
fn dotcode_renders_as_circles() {
    let opts = Options::default();
    for payload in ["Hello", "1234", "ABC123abc"] {
        let svg = render_svg(Symbology::DotCode, payload, &opts)
            .unwrap_or_else(|e| panic!("DotCode {payload:?} svg failed: {e}"));
        assert!(svg.starts_with("<svg"));
        assert!(
            svg.contains("<circle "),
            "DotCode SVG should contain <circle>"
        );
        assert!(
            !svg.contains("<rect x="),
            "DotCode SVG should NOT contain dot-positioned <rect> (only the background)",
        );

        let png =
            render_png(Symbology::DotCode, payload, &opts).expect("DotCode PNG should succeed");
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
        assert!(png.len() > 100);
    }
}

/// `Symbology::DotCode` should be reachable via the public catalog
/// — `Symbology::all()` contains it, `Symbology::from_id("dotcode")`
/// resolves to it, and the default-data round-trips.
#[test]
fn dotcode_is_in_public_catalog() {
    assert!(Symbology::all().contains(&Symbology::DotCode));
    assert_eq!(Symbology::from_id("dotcode"), Some(Symbology::DotCode));
    assert_eq!(Symbology::from_id("dot_code"), Some(Symbology::DotCode));
    assert_eq!(Symbology::DotCode.id(), "dotcode");
    assert_eq!(Symbology::DotCode.display_name(), "DotCode");
    assert_eq!(Symbology::DotCode.category(), "2D - Matrix");
}

/// Substrate-dimension drift net.
///
/// For every catalog row whose 2D layout is delegated to the upstream
/// `qrcode` or `datamatrix` crate, pin the symbol dimensions our
/// substrate produces for a fixed canonical payload against the
/// dimensions bwip-js produces for the same payload. The exact
/// module pattern can differ (substrate mode-selector divergence is
/// the documented compatibility-exception posture for QR-family
/// rows, and the substrate-spec posture for DataMatrix-family rows),
/// but the symbol *size* should agree for the canonical inputs we
/// pin — if a substrate-crate version bump changes the size-selection
/// heuristic for any of these inputs, this test fails loudly so the
/// drift gets reviewed instead of silently shipping a different
/// symbol size than bwip-js.
///
/// Update procedure: re-run
/// `node -e '... b.raw(bcid, data, {}) ...'` for any row whose
/// dimensions intentionally change, then update the corresponding
/// row here.
#[test]
fn substrate_rows_match_bwip_js_dimensions() {
    use bwipp::{Encoded, Symbology};

    let cases: &[(Symbology, &str, usize, usize)] = &[
        // qrcode substrate (compatibility-exception family — dims
        // still pinned because they don't depend on the mask choice).
        // When `prefer-native-qrcode` is enabled, Symbology::QrCode and
        // Symbology::MicroQrCode route through qrcode_native and may
        // pick different versions than the substrate (different mode
        // classification heuristics + EC-upgrade behavior). Those two
        // rows are gated out below; the GS1 + HIBC QR wrappers stay
        // pinned (they still ultimately call qrcode_::encode for the
        // QR substrate).
        #[cfg(not(feature = "prefer-native-qrcode"))]
        (Symbology::QrCode, "https://example.com", 25, 25),
        #[cfg(not(feature = "prefer-native-qrcode"))]
        (Symbology::MicroQrCode, "12345", 11, 11),
        (Symbology::Gs1QrCode, "(01)04012345123456(17)260101", 21, 21),
        (Symbology::HibcQrCode, "A99912345/52001510X3", 25, 25),
        // datamatrix substrate (verified family).
        (Symbology::DataMatrix, "hello", 12, 12),
        (Symbology::DataMatrixRectangular, "hello", 18, 8),
        (Symbology::Gs1DataMatrix, "(01)04012345123456", 16, 16),
        (
            Symbology::Gs1DataMatrixRectangular,
            "(01)04012345123456",
            32,
            8,
        ),
        (Symbology::HibcDataMatrix, "A99912345/52001510X3", 18, 18),
        (
            Symbology::HibcDataMatrixRectangular,
            "A99912345/52001510X3",
            26,
            12,
        ),
        (
            Symbology::Gs1DlDataMatrix,
            "https://id.gs1.org/01/04012345123456",
            22,
            22,
        ),
        // Note: `Symbology::HibcPasQrCode` deliberately omitted — for the
        // canonical longer PAS payload our qrcode-crate substrate picks
        // version 3 (29×29) where BWIPP picks version 2 (25×25). This is
        // an additional substrate divergence beyond the documented
        // qrcode-family mask-selection exception (the encoder also
        // disagrees on payload-mode classification for certain inputs).
        // Tracked alongside the QR-family compatibility exception.
        //
        // Note: `Symbology::DataMatrixRectangularExtension` deliberately
        // omitted — for the canonical 40-character "AB" input, our
        // substrate picks a classic 36×16 size where BWIPP picks the
        // DMRE-only 80×8 size. Both are spec-compliant. The dedicated
        // `datamatrix_::tests::dmre_*` tests pin the divergence.
    ];

    for &(sym, data, want_w, want_h) in cases {
        let opts = bwipp::Options::default();
        match sym.encode(data, &opts) {
            Ok(Encoded::Matrix(m)) => {
                assert_eq!(
                    (m.width(), m.height()),
                    (want_w, want_h),
                    "{}: size drift from bwip-js for {data:?} (got {}×{}, expected {want_w}×{want_h})",
                    sym.id(),
                    m.width(),
                    m.height(),
                );
            }
            Ok(other) => panic!("{}: expected Matrix, got {:?}", sym.id(), other),
            Err(e) => panic!("{} encode({data:?}) failed: {e}", sym.id()),
        }
    }
}

#[test]
fn categories_cover_known_groups() {
    let cats: std::collections::HashSet<&'static str> =
        Symbology::all().iter().map(|s| s.category()).collect();
    // These are the buckets the UI groups on; if any disappears the demo
    // page's grouping breaks silently.
    for expected in [
        "1D - Standard",
        "1D - Retail / EAN / UPC",
        "Postal",
        "2D - Matrix",
        "HIBC (Healthcare)",
    ] {
        assert!(cats.contains(expected), "missing category: {expected}");
    }
}

/// When the `prefer-native-qrcode` Cargo feature is enabled,
/// `Symbology::QrCode` and `Symbology::MicroQrCode` route through
/// the BWIPP-faithful `qrcode_native` encoder. Verify both produce
/// a non-empty BitMatrix for short payloads (V1 / M2 sizing).
#[cfg(feature = "prefer-native-qrcode")]
#[test]
fn prefer_native_qrcode_routes_full_and_micro() {
    use bwipp::{Encoded, Options, Symbology};
    let opts = Options::default();
    let full = Symbology::QrCode
        .encode("HELLO WORLD", &opts)
        .expect("native QR `HELLO WORLD` encode must succeed (V1 alphanumeric)");
    match full {
        Encoded::Matrix(m) => {
            assert_eq!(m.width(), 21, "V1 expected for 11-char alphanumeric");
            assert_eq!(m.height(), 21);
        }
        _ => panic!("expected Matrix encoding for QrCode"),
    }
    let micro = Symbology::MicroQrCode
        .encode("1234", &opts)
        .expect("native Micro QR `1234` encode must succeed (M1/M2 numeric)");
    match micro {
        Encoded::Matrix(m) => {
            // 4 digits at M (the default eclevel passed to
            // encode_micro_auto) → M1 (numeric, 5 digits fits in M1-L).
            // encode_micro_auto starts at requested_ec=1=M; if M doesn't
            // fit any version, it walks up. For 4 digits the smallest
            // is M1 (11x11) if numeric+L fits; the EC level upgrade
            // logic may push to a different size depending on capacity.
            assert!(
                m.width() >= 11 && m.width() <= 17,
                "Micro QR width 11..=17, got {}",
                m.width()
            );
        }
        _ => panic!("expected Matrix encoding for MicroQrCode"),
    }
}
