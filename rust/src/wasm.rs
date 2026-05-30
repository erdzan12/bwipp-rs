//! WebAssembly / JS bindings.
//!
//! Compile with:
//!
//! ```sh
//! cargo build --target wasm32-unknown-unknown --no-default-features --features wasm
//! ```
//!
//! The exported surface is intentionally tiny: three functions that let a
//! JavaScript host pick a symbology, encode some data, and receive an SVG
//! string or PNG bytes. Options are passed as a flat JS object
//! `{ scale: 4, eclevel: "M", ... }`; recognised numeric fields override
//! the equivalent fields on [`crate::Options`], and any remaining
//! key/value pairs become string-valued `extras`.

#![allow(missing_docs)]

use wasm_bindgen::prelude::*;

use crate::{render_png as r_png, render_svg as r_svg, Options, Symbology};

/// Wasm-bindgen start hook: installs a panic handler that forwards to
/// `console.error` so JS users see readable stack traces.
#[wasm_bindgen(start)]
pub fn __bwipp_rs_init() {
    // Forward panics to console.error so JS users get readable stack traces.
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Record<string, string | number | boolean> | undefined")]
    pub type JsOpts;
}

/// Render a barcode to an SVG string.
#[wasm_bindgen(js_name = renderSvg)]
pub fn render_svg(symbology_id: &str, data: &str, opts: Option<JsOpts>) -> Result<String, JsError> {
    let sym = resolve_symbology(symbology_id)?;
    let options = options_from_js(opts)?;
    r_svg(sym, data, &options).map_err(|e| JsError::new(&e.to_string()))
}

/// Render a barcode to PNG bytes (returned as a `Uint8Array`).
#[wasm_bindgen(js_name = renderPng)]
pub fn render_png(
    symbology_id: &str,
    data: &str,
    opts: Option<JsOpts>,
) -> Result<Vec<u8>, JsError> {
    let sym = resolve_symbology(symbology_id)?;
    let options = options_from_js(opts)?;
    r_png(sym, data, &options).map_err(|e| JsError::new(&e.to_string()))
}

/// Return every supported symbology id (matches [`Symbology::all`]).
#[wasm_bindgen(js_name = listSymbologies)]
pub fn list_symbologies() -> Vec<js_sys::JsString> {
    Symbology::all()
        .iter()
        .map(|s| js_sys::JsString::from(s.id()))
        .collect()
}

/// Return `[id, displayName, category, defaultData]` quadruples for every
/// supported symbology. Useful for populating a grouped UI dropdown and
/// prefilling the input field with realistic sample data.
#[wasm_bindgen(js_name = listSymbologyDetails)]
pub fn list_symbology_details() -> Vec<js_sys::Array> {
    Symbology::all()
        .iter()
        .map(|s| {
            let arr = js_sys::Array::new();
            arr.push(&js_sys::JsString::from(s.id()).into());
            arr.push(&js_sys::JsString::from(s.display_name()).into());
            arr.push(&js_sys::JsString::from(s.category()).into());
            arr.push(&js_sys::JsString::from(s.default_data()).into());
            arr
        })
        .collect()
}

/// Return the bundled sample payload for a symbology id, or an empty
/// string for unknown ids. Lets the demo prefill the input field without
/// duplicating the table on the JS side.
#[wasm_bindgen(js_name = defaultData)]
pub fn default_data(symbology_id: &str) -> String {
    match Symbology::from_id(symbology_id) {
        Some(s) => s.default_data().to_string(),
        None => String::new(),
    }
}

/// Return the symbology-specific default option overrides as an object
/// (e.g. `{ columns: "8" }` for `codablockf`). Empty object for symbologies
/// without any defaults. The demo merges this into the options it passes
/// back to [`render_svg`] so stacked symbologies render at a sensible size.
#[wasm_bindgen(js_name = defaultExtras)]
pub fn default_extras(symbology_id: &str) -> js_sys::Object {
    let obj = js_sys::Object::new();
    if let Some(s) = Symbology::from_id(symbology_id) {
        for (k, v) in s.default_extras() {
            let _ = js_sys::Reflect::set(
                &obj,
                &js_sys::JsString::from(*k).into(),
                &js_sys::JsString::from(*v).into(),
            );
        }
    }
    obj
}

fn resolve_symbology(id: &str) -> Result<Symbology, JsError> {
    Symbology::from_id(id).ok_or_else(|| JsError::new(&format!("unknown symbology id: {id}")))
}

fn options_from_js(opts: Option<JsOpts>) -> Result<Options, JsError> {
    let Some(opts) = opts else {
        return Ok(Options::default());
    };
    let js_val: JsValue = opts.into();
    if js_val.is_null() || js_val.is_undefined() {
        return Ok(Options::default());
    }
    let obj: js_sys::Object = js_val
        .dyn_into()
        .map_err(|_| JsError::new("options must be a plain object"))?;

    let mut out = Options::default();
    let entries = js_sys::Object::entries(&obj);
    for i in 0..entries.length() {
        let pair = js_sys::Array::from(&entries.get(i));
        let key = pair
            .get(0)
            .as_string()
            .ok_or_else(|| JsError::new("options key must be a string"))?;
        let val = pair.get(1);
        apply_option(&mut out, &key, val);
    }
    Ok(out)
}

fn apply_option(out: &mut Options, key: &str, val: JsValue) {
    match key {
        "scale" => {
            if let Some(n) = val.as_f64() {
                out.scale = n as u32;
            }
        }
        "bar_height" | "barHeight" => {
            if let Some(n) = val.as_f64() {
                out.bar_height = n as u32;
            }
        }
        "quiet_zone" | "quietZone" => {
            if let Some(n) = val.as_f64() {
                out.quiet_zone = n as u32;
            }
        }
        "include_text" | "includeText" | "includetext" => {
            if let Some(b) = val.as_bool() {
                out.include_text = b;
            } else if let Some(s) = val.as_string() {
                out.include_text = s == "true";
            }
        }
        _ => {
            let value = if let Some(s) = val.as_string() {
                s
            } else if let Some(b) = val.as_bool() {
                if b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            } else if let Some(n) = val.as_f64() {
                n.to_string()
            } else {
                return;
            };
            out.extras.push((key.to_string(), value));
        }
    }
}
