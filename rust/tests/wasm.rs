//! Integration tests for the wasm-bindgen API.
//!
//! These run under `wasm32-unknown-unknown` with the `wasm` feature
//! enabled. They are skipped on native targets because the wasm-bindgen
//! API is `cfg(feature = "wasm")` gated and only resolvable when the
//! crate is compiled for a wasm target.
//!
//! Run via:
//!
//! ```sh
//! # Install once:
//! cargo install wasm-bindgen-cli
//! cargo install --locked wasm-pack
//!
//! # Then:
//! wasm-pack test --node -- --no-default-features --features wasm
//! ```

#![cfg(all(target_arch = "wasm32", feature = "wasm"))]

use wasm_bindgen_test::*;

// Default test runner is Node-in-headless mode; opt into the browser runner
// by setting wasm-pack test --headless --chrome.

#[wasm_bindgen_test]
fn list_symbologies_is_non_empty() {
    let ids = bwipp::wasm::list_symbologies();
    assert!(
        ids.len() >= 80,
        "expected at least 80 symbologies, got {}",
        ids.len()
    );
}

#[wasm_bindgen_test]
fn render_qrcode_returns_svg() {
    let svg =
        bwipp::wasm::render_svg("qrcode", "hello", None).expect("qrcode renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_qrcode_returns_png_bytes() {
    let bytes =
        bwipp::wasm::render_png("qrcode", "hello", None).expect("qrcode renderPng should succeed");
    assert!(bytes.len() > 100);
    assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
}

#[wasm_bindgen_test]
fn render_ean13_renders() {
    let svg = bwipp::wasm::render_svg("ean13", "012345678905", None).expect("ean13 should render");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn unknown_symbology_returns_error() {
    // Stage 11.A8c (cont) — upgrade bare `.is_err()` to anchor parity
    // with the A6.4 sibling test `unknown_symbology_error_message_
    // includes_offending_id` at line 418-427. Pin both the symbology
    // prefix from the format string at line 114 of src/wasm.rs
    // (`unknown symbology id: {id}`) and the offending id echo.
    let result = bwipp::wasm::render_svg("not-a-real-symbology", "x", None);
    let err = result.expect_err("unknown id should fail");
    let msg = js_error_message(err);
    assert!(
        msg.contains("unknown symbology id"),
        "missing `unknown symbology id` predicate: {msg:?}"
    );
    assert!(
        msg.contains("not-a-real-symbology"),
        "missing offending-id echo `not-a-real-symbology`: {msg:?}"
    );
}

#[wasm_bindgen_test]
fn render_dotcode_returns_circle_svg() {
    let svg = bwipp::wasm::render_svg("dotcode", "Hello", None)
        .expect("dotcode renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(
        svg.contains("<circle "),
        "DotCode SVG should emit <circle> per dot"
    );
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_dotcode_returns_png_bytes() {
    let bytes = bwipp::wasm::render_png("dotcode", "Hello", None)
        .expect("dotcode renderPng should succeed");
    assert!(bytes.len() > 100);
    assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
}

#[wasm_bindgen_test]
fn render_databar_expanded_returns_svg() {
    // (01)+GTIN-14 — method 1 fast path.
    let svg = bwipp::wasm::render_svg("databar_expanded", "(01)90012345678908", None)
        .expect("DataBar Expanded renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_databar_expanded_stacked_returns_svg() {
    let svg = bwipp::wasm::render_svg("databar_expanded_stacked", "(01)90012345678908", None)
        .expect("DataBar Expanded Stacked renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_hanxin_returns_svg() {
    let svg = bwipp::wasm::render_svg("hanxin", "Hello World", None)
        .expect("Han Xin Code renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_aztec_returns_svg() {
    // UTF-8 multibyte input exercises Byte mode.
    let svg = bwipp::wasm::render_svg("azteccode", "café", None)
        .expect("Aztec Code renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_maxicode_returns_svg() {
    // Mode 4 default for general data — exercises the hex-grid render
    // path (Encoded::Hex).
    let svg = bwipp::wasm::render_svg("maxicode", "Hello", None)
        .expect("MaxiCode renderSvg should succeed");
    assert!(svg.starts_with("<svg"), "expected SVG, got: {svg:.50}");
    assert!(svg.contains("</svg>"));
}

#[wasm_bindgen_test]
fn render_mailmark_with_type_option_returns_svg() {
    // The `type` option flow — passes options as a JS object cast
    // through JsValue to match the wasm-bindgen JsOpts type.
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(
        &opts,
        &wasm_bindgen::JsValue::from_str("type"),
        &wasm_bindgen::JsValue::from_str("29"),
    )
    .expect("set type option");
    let js_opts: wasm_bindgen::JsValue = opts.into();
    let svg = bwipp::wasm::render_svg(
        "mailmark",
        "JGB 012100123412345678AB19XY1A 0             www.xyz.com",
        Some(js_opts.into()),
    )
    .expect("Mailmark type=29 renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_gs1_qrcode_returns_svg() {
    // GS1 QR Code — exercises the FNC1-first-position mode path.
    let svg = bwipp::wasm::render_svg("gs1qrcode", "(01)04012345123456", None)
        .expect("GS1 QR Code renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_composite_gs1_128_ccc_returns_svg() {
    // GS1-128 CC-C composite — the most complex composite path
    // (PDF417 2D companion stacked on a GS1-128 linear).
    let svg = bwipp::wasm::render_svg(
        "composite_gs1_128_ccc",
        "(01)04012345123456|(99)1234567",
        None,
    )
    .expect("GS1-128 CC-C renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_code128_returns_svg() {
    let svg = bwipp::wasm::render_svg("code128", "HELLO", None)
        .expect("Code 128 renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_code39_returns_svg() {
    let svg = bwipp::wasm::render_svg("code39", "BWIPP-RS", None)
        .expect("Code 39 renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_pdf417_returns_svg() {
    let svg = bwipp::wasm::render_svg("pdf417", "Hello World", None)
        .expect("PDF417 renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_micropdf417_returns_svg() {
    let svg = bwipp::wasm::render_svg("micropdf417", "Hello", None)
        .expect("MicroPDF417 renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_datamatrix_returns_svg() {
    let svg = bwipp::wasm::render_svg("datamatrix", "Hello", None)
        .expect("Data Matrix renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_microqrcode_returns_svg() {
    let svg = bwipp::wasm::render_svg("microqrcode", "12345", None)
        .expect("Micro QR Code renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_aztecrune_returns_svg() {
    let svg = bwipp::wasm::render_svg("aztecrune", "42", None)
        .expect("Aztec Rune renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_azteccodecompact_returns_svg() {
    let svg = bwipp::wasm::render_svg("azteccodecompact", "Hi", None)
        .expect("Aztec Compact renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_channelcode_returns_svg() {
    let svg = bwipp::wasm::render_svg("channelcode", "12", None)
        .expect("Channel Code renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_upca_returns_svg() {
    let svg = bwipp::wasm::render_svg("upca", "012345678905", None)
        .expect("UPC-A renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_codablockf_returns_svg() {
    let svg = bwipp::wasm::render_svg("codablockf", "Hello", None)
        .expect("Codablock-F renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_gs1_datamatrix_returns_svg() {
    let svg = bwipp::wasm::render_svg("gs1datamatrix", "(01)04012345123456", None)
        .expect("GS1 Data Matrix renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_gs1dldatamatrix_returns_svg() {
    let svg = bwipp::wasm::render_svg(
        "gs1dldatamatrix",
        "https://id.gs1.org/01/04012345123456",
        None,
    )
    .expect("GS1 DL Data Matrix renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_usps_imb_returns_svg() {
    let svg = bwipp::wasm::render_svg("usps_imb", "01234567094987654321", None)
        .expect("USPS IMb renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

#[wasm_bindgen_test]
fn render_datamatrixrectangularextension_returns_svg() {
    let svg = bwipp::wasm::render_svg("datamatrixrectangularextension", "12345", None)
        .expect("DMRE renderSvg should succeed");
    assert!(svg.starts_with("<svg"));
}

// ---------------------------------------------------------------------------
// Stage 11.A6 — exhaustive per-Symbology WASM-surface coverage.
//
// The hand-written tests above pin a curated set of "interesting" symbology
// renders (composite paths, ECI-flavoured GS1, JS-options object). They do
// not, by themselves, prove that every Symbology variant the catalog
// exposes is actually reachable through the wasm-bindgen API.
//
// The two big tests below close that gap: they iterate `Symbology::all()`
// (the same list the JS `listSymbologies` / `listSymbologyDetails` exports
// vend) and render each variant via the wasm API, using the same
// curated default payload + default-options object the demo's "select a
// symbology" dropdown would use. Any future Symbology variant added to
// `Symbology::all()` is automatically covered without touching this file.
//
// The error-surface tests below assert that error variants survive the
// wasm boundary intact (the JS-side `Error.message` is reachable from
// Rust by reading the `message` property off the `JsValue` form of the
// `JsError`), which is the load-bearing property for JS callers that
// need to render structured error UI.
// ---------------------------------------------------------------------------

/// Pull the JS-side `Error.message` from a `JsError` returned by the
/// wasm-bindgen-exported API. The wasm boundary boxes errors as JS
/// `Error` objects whose `message` is the `&str` we passed in Rust —
/// this helper round-trips back to a Rust `String` so we can assert on
/// its content (e.g. "unknown symbology id: …").
fn js_error_message(err: wasm_bindgen::JsError) -> String {
    let js_val: wasm_bindgen::JsValue = err.into();
    js_sys::Reflect::get(&js_val, &wasm_bindgen::JsValue::from_str("message"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// A6.1 — every Symbology returned by `Symbology::all()` renders to a
/// non-empty SVG via the wasm `render_svg` export, using its bundled
/// default payload and default-options object (which mirrors what the
/// JS demo's dropdown serves). This proves: every catalog variant is
/// reachable from JS, the JS-options coercion accepts whatever
/// `default_extras()` advertises, and no variant accidentally got
/// disconnected from the wasm surface.
#[wasm_bindgen_test]
fn every_symbology_renders_via_wasm_svg() {
    let mut failures: Vec<String> = Vec::new();
    for s in bwipp::Symbology::all() {
        let id = s.id();
        let data = s.default_data();
        // Build the JS options object from `default_extras()` exactly the
        // way the JS demo does. Empty `extras` ⇒ empty object ⇒ default
        // Options on the Rust side.
        let extras_obj = bwipp::wasm::default_extras(id);
        let opts: wasm_bindgen::JsValue = extras_obj.into();
        let result = bwipp::wasm::render_svg(id, data, Some(opts.into()));
        match result {
            Ok(svg) => {
                if !svg.starts_with("<svg") || !svg.contains("</svg>") {
                    failures.push(format!(
                        "{id}: rendered string is not a complete SVG ({len} bytes)",
                        len = svg.len()
                    ));
                }
            }
            Err(e) => {
                failures.push(format!("{id}: render_svg errored: {}", js_error_message(e)));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{n} symbology variants failed to render via the wasm SVG API:\n  - {}",
        failures.join("\n  - "),
        n = failures.len(),
    );
}

/// A6.2 — every Symbology renders to PNG bytes (with the PNG magic
/// header) via the wasm `render_png` export. Same iteration discipline
/// as A6.1, plus a byte-level check that we didn't accidentally route a
/// variant through a code path that returns the wrong container format.
#[wasm_bindgen_test]
fn every_symbology_renders_via_wasm_png() {
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut failures: Vec<String> = Vec::new();
    for s in bwipp::Symbology::all() {
        let id = s.id();
        let data = s.default_data();
        let extras_obj = bwipp::wasm::default_extras(id);
        let opts: wasm_bindgen::JsValue = extras_obj.into();
        let result = bwipp::wasm::render_png(id, data, Some(opts.into()));
        match result {
            Ok(bytes) => {
                if bytes.len() < 8 || bytes[..8] != PNG_MAGIC {
                    failures.push(format!(
                        "{id}: render_png output is not a PNG ({len} bytes, leading {head:02x?})",
                        len = bytes.len(),
                        head = &bytes[..bytes.len().min(8)],
                    ));
                }
            }
            Err(e) => {
                failures.push(format!("{id}: render_png errored: {}", js_error_message(e)));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{n} symbology variants failed to render via the wasm PNG API:\n  - {}",
        failures.join("\n  - "),
        n = failures.len(),
    );
}

/// A6.3 — the wasm `listSymbologies` / `listSymbologyDetails` exports
/// stay in lockstep with `Symbology::all()`. If a future change adds a
/// Symbology to `all()` but forgets to plumb it through the JS-facing
/// helper, this test catches it before it ships.
#[wasm_bindgen_test]
fn wasm_list_symbologies_matches_symbology_all() {
    let ids = bwipp::wasm::list_symbologies();
    let details = bwipp::wasm::list_symbology_details();
    let expected = bwipp::Symbology::all().len();
    assert_eq!(
        ids.len(),
        expected,
        "listSymbologies length {got} != Symbology::all() length {expected}",
        got = ids.len(),
    );
    assert_eq!(
        details.len(),
        expected,
        "listSymbologyDetails length {got} != Symbology::all() length {expected}",
        got = details.len(),
    );
}

/// A6.4 — an unknown symbology id surfaces a JS Error whose `message`
/// names the offending id. This is the contract the JS-side
/// "no such symbology" error UI depends on.
#[wasm_bindgen_test]
fn unknown_symbology_error_message_includes_offending_id() {
    let result = bwipp::wasm::render_svg("definitely-not-a-symbology-zzz", "x", None);
    let err = result.expect_err("unknown id should fail");
    let msg = js_error_message(err);
    assert!(
        msg.contains("definitely-not-a-symbology-zzz"),
        "expected JS Error.message to mention the bad id, got: {msg:?}",
    );
}

/// A6.5 — Rust-side validation errors (here: ean13 with a non-digit
/// payload) survive the wasm boundary as a JS Error with a
/// human-readable message. The message ends up in the JS user's
/// `catch` block; assert it isn't empty / opaque.
#[wasm_bindgen_test]
fn invalid_data_error_surfaces_across_wasm_boundary() {
    let result = bwipp::wasm::render_svg("ean13", "not-digits-here", None);
    let err = result.expect_err("ean13 with alphabetic payload should fail");
    let msg = js_error_message(err);
    assert!(
        !msg.is_empty(),
        "expected JS Error.message to be populated, got an empty string",
    );
}

/// A6.6 — option-level rejections also propagate. The wasm options
/// blob is a `Record<string, string | number | boolean>` on the JS
/// side; here we pass a string `eclevel` that the qrcode encoder
/// rejects. The resulting `JsError` should carry a non-empty message.
#[wasm_bindgen_test]
fn invalid_option_error_surfaces_across_wasm_boundary() {
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(
        &opts,
        &wasm_bindgen::JsValue::from_str("eclevel"),
        &wasm_bindgen::JsValue::from_str("Z"), // invalid: only L/M/Q/H
    )
    .expect("set eclevel option");
    let js_opts: wasm_bindgen::JsValue = opts.into();
    let result = bwipp::wasm::render_svg("qrcode", "hello", Some(js_opts.into()));
    let err = result.expect_err("eclevel=Z should be rejected");
    let msg = js_error_message(err);
    assert!(
        !msg.is_empty(),
        "expected option-error message to be populated, got an empty string",
    );
}

/// A6.7 — passing `null`/`undefined`/no options goes through the
/// JS-options coercion path on the Rust side. We've already covered
/// the `None` case in the earlier tests; this one passes an explicit
/// `JsValue::null()` to lock in the documented "null ⇒ defaults"
/// behaviour of `options_from_js`.
#[wasm_bindgen_test]
fn null_options_value_uses_defaults() {
    let null_opts: wasm_bindgen::JsValue = wasm_bindgen::JsValue::null();
    let svg = bwipp::wasm::render_svg("code128", "HELLO", Some(null_opts.into()))
        .expect("null options should be treated as defaults");
    assert!(svg.starts_with("<svg"));
}
