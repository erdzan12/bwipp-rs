//! Minimal browser-facing WASM ABI for the `bwipp-rs` encoder.
//!
//! The public exports intentionally use plain pointers and strings so the web
//! app can load the `.wasm` file directly without a generated JavaScript glue
//! package. Responses are length-prefixed UTF-8 buffers.
//!
//! # Unsafe boundary
//!
//! This crate is the *only* place in the project where `unsafe` is allowed:
//! the upstream `bwipp` library is built under `#![forbid(unsafe_code)]`.
//! The unsafe surface here is intentionally tiny — three functions —
//! and each `unsafe fn` carries a `# Safety` doc comment explaining the
//! invariants the JavaScript caller must uphold. The host (the canonical
//! `web/` Next.js app under `src/lib/rust-engine.ts`) is the only consumer
//! we currently ship and it satisfies these invariants by construction:
//!
//! 1. It allocates via `bwipp_wasm_alloc(len)` and writes exactly `len`
//!    bytes before calling into `bwipp_wasm_encode_svg`.
//! 2. It reads the response length prefix from the returned pointer and
//!    immediately calls `bwipp_wasm_dealloc(ptr, total_len)`.
//! 3. It never double-frees or reuses a freed pointer.
//!
//! The wasm-pack-generated bindings under `rust/src/wasm.rs` go through
//! `wasm-bindgen` and avoid this raw ABI entirely.
#![deny(missing_docs)]

use std::mem;
use std::slice;

use bwipp::{render_svg, Options, Symbology};

/// Allocate `len` bytes in the WASM linear memory and return a raw pointer.
///
/// The caller must subsequently free the returned pointer with
/// [`bwipp_wasm_dealloc`]`(ptr, len)` using the *same* `len`. Returning a raw
/// pointer is the price of avoiding a JS-glue layer; the canonical web app
/// uses this purely to stage a UTF-8 string before calling into
/// [`bwipp_wasm_encode_svg`].
#[no_mangle]
pub extern "C" fn bwipp_wasm_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let mut buffer = vec![0_u8; len].into_boxed_slice();
    let ptr = buffer.as_mut_ptr();
    mem::forget(buffer);
    ptr
}

/// Free a buffer previously returned by [`bwipp_wasm_alloc`] or by the
/// internal response writer.
///
/// # Safety
///
/// * `ptr` must have been returned by [`bwipp_wasm_alloc`] *or* be a
///   response pointer returned by one of the `encode` exports.
/// * `len` must equal the length the buffer was originally allocated with
///   (for response buffers that is `4 + length_prefix_value`).
/// * The caller must not double-free, must not access `ptr` after this
///   call, and must not pass a pointer that was not produced by this
///   module.
#[no_mangle]
pub unsafe extern "C" fn bwipp_wasm_dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: per the function-level contract, `ptr` was allocated by us
    // (Box<[u8]>::into_boxed_slice -> mem::forget) with exactly `len`
    // bytes, and the caller has not freed or aliased it.
    let slice = std::ptr::slice_from_raw_parts_mut(ptr, len);
    drop(Box::from_raw(slice));
}

/// Return every catalog id supported by the WASM bridge as a JSON string
/// array (e.g. `["qrcode","ean13",...]`).
///
/// The returned pointer follows the same length-prefixed convention as
/// [`bwipp_wasm_encode_svg`] — free it with
/// `bwipp_wasm_dealloc(ptr, 4 + length_prefix)`.
#[no_mangle]
pub extern "C" fn bwipp_wasm_supported_ids() -> *mut u8 {
    let ids = Symbology::all()
        .iter()
        .map(|sym| format!(r#""{}""#, escape_json(sym.id())))
        .collect::<Vec<_>>()
        .join(",");
    response_buffer(format!("[{ids}]"))
}

/// Encode a barcode to SVG and return a length-prefixed JSON response.
///
/// # Safety
///
/// Each `(ptr, len)` pair must satisfy:
/// * `ptr` is either null (when `len == 0`) or a valid pointer into this
///   module's linear memory returned by [`bwipp_wasm_alloc`].
/// * `len` is exactly the number of bytes the caller wrote at `ptr`.
/// * The bytes at `ptr..ptr+len` are valid UTF-8.
/// * Each input buffer is freed by the caller exactly once via
///   [`bwipp_wasm_dealloc`] after this function returns.
///
/// The returned pointer points to a fresh allocation owned by the caller;
/// it has a 4-byte little-endian length prefix followed by `len` bytes of
/// UTF-8 JSON. Free it with `bwipp_wasm_dealloc(ptr, 4 + length)`.
#[no_mangle]
pub unsafe extern "C" fn bwipp_wasm_encode_svg(
    symbology_ptr: *const u8,
    symbology_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    options_ptr: *const u8,
    options_len: usize,
) -> *mut u8 {
    let result = (|| {
        // SAFETY: per the function-level contract, each (ptr, len) is
        // either (null, 0) or points to `len` valid UTF-8 bytes.
        let symbology_id = read_string(symbology_ptr, symbology_len)?;
        let data = read_string(data_ptr, data_len)?;
        let options_text = read_string(options_ptr, options_len)?;
        let symbology = Symbology::from_id(symbology_id.trim())
            .ok_or_else(|| format!("Rust WASM does not support '{}'.", symbology_id.trim()))?;
        let options = parse_options(&options_text);
        render_svg(symbology, &data, &options).map_err(|err| err.to_string())
    })();

    match result {
        Ok(svg) => response_buffer(format!(r#"{{"ok":true,"svg":"{}"}}"#, escape_json(&svg))),
        Err(error) => response_buffer(format!(
            r#"{{"ok":false,"error":"{}"}}"#,
            escape_json(&error)
        )),
    }
}

/// Materialize a UTF-8 [`String`] from a raw `(ptr, len)` pair.
///
/// # Safety
///
/// * `ptr` must be either null (when `len == 0`) or point to a buffer of
///   at least `len` initialized bytes that the caller has not freed.
/// * The pointed-to bytes must be a valid UTF-8 sequence.
unsafe fn read_string(ptr: *const u8, len: usize) -> Result<String, String> {
    if len == 0 {
        return Ok(String::new());
    }
    if ptr.is_null() {
        return Err("Received a null pointer for a non-empty string.".to_owned());
    }
    // SAFETY: the caller has guaranteed `ptr..ptr+len` is a live UTF-8
    // buffer (see fn-level contract). `from_utf8` then either rejects
    // invalid sequences or returns an owned copy, so the borrow does not
    // outlive this call.
    let bytes = slice::from_raw_parts(ptr, len);
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|err| format!("Input was not valid UTF-8: {err}"))
}

fn parse_options(input: &str) -> Options {
    let mut options = Options::default();

    for line in input.lines() {
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let value = raw_value.trim();
        match key {
            "scale" => {
                options.scale = parse_u32(value, options.scale).clamp(1, 32);
            }
            "bar_height" => {
                options.bar_height = parse_u32(value, options.bar_height).clamp(1, 512);
            }
            "quiet_zone" => {
                options.quiet_zone = parse_u32(value, options.quiet_zone).clamp(0, 128);
            }
            "include_text" => {
                options.include_text = matches!(value, "1" | "true" | "yes" | "on");
            }
            "foreground" => {
                options.foreground = parse_hex_color(value).unwrap_or(options.foreground);
            }
            "background" => {
                options.background = parse_hex_color(value).unwrap_or(options.background);
            }
            _ if key.starts_with("extra:") => {
                options.extras.push((key[6..].to_owned(), value.to_owned()));
            }
            _ => {}
        }
    }

    options
}

fn parse_u32(value: &str, fallback: u32) -> u32 {
    value.parse::<u32>().unwrap_or(fallback)
}

fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r, g, b])
}

fn response_buffer(payload: String) -> *mut u8 {
    let payload = payload.into_bytes();
    let len = payload.len() as u32;
    let mut buffer = vec![0_u8; payload.len() + 4].into_boxed_slice();
    buffer[0..4].copy_from_slice(&len.to_le_bytes());
    buffer[4..].copy_from_slice(&payload);
    let ptr = buffer.as_mut_ptr();
    mem::forget(buffer);
    ptr
}

fn escape_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if ch <= '\u{1f}' => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip through the unsafe ABI on a native target — exercises
    /// `alloc → fill → encode_svg → take string → dealloc`, mirroring what
    /// the JS host does in production. Native execution keeps the tests
    /// fast and ensures the `unsafe` is actually covered (not just behind
    /// a wasm-pack run).
    #[test]
    fn round_trip_encodes_and_frees_without_leaks() {
        let symbology = b"qrcode";
        let data = b"Hello, WASM!";
        let options = b"scale=2\nbar_height=10\nquiet_zone=4\ninclude_text=0";

        unsafe {
            let sym_ptr = bwipp_wasm_alloc(symbology.len());
            std::ptr::copy_nonoverlapping(symbology.as_ptr(), sym_ptr, symbology.len());

            let data_ptr = bwipp_wasm_alloc(data.len());
            std::ptr::copy_nonoverlapping(data.as_ptr(), data_ptr, data.len());

            let opts_ptr = bwipp_wasm_alloc(options.len());
            std::ptr::copy_nonoverlapping(options.as_ptr(), opts_ptr, options.len());

            let resp_ptr = bwipp_wasm_encode_svg(
                sym_ptr,
                symbology.len(),
                data_ptr,
                data.len(),
                opts_ptr,
                options.len(),
            );
            assert!(!resp_ptr.is_null());

            let len = u32::from_le_bytes([
                *resp_ptr,
                *resp_ptr.add(1),
                *resp_ptr.add(2),
                *resp_ptr.add(3),
            ]) as usize;
            let body = slice::from_raw_parts(resp_ptr.add(4), len);
            let json = std::str::from_utf8(body).unwrap();
            assert!(
                json.starts_with(r#"{"ok":true,"svg":""#),
                "unexpected response: {json}"
            );

            bwipp_wasm_dealloc(resp_ptr, len + 4);
            bwipp_wasm_dealloc(sym_ptr, symbology.len());
            bwipp_wasm_dealloc(data_ptr, data.len());
            bwipp_wasm_dealloc(opts_ptr, options.len());
        }
    }

    #[test]
    fn unknown_symbology_returns_error_json() {
        let symbology = b"not_a_real_symbology";
        let data = b"";
        let options = b"";
        unsafe {
            let sym_ptr = bwipp_wasm_alloc(symbology.len());
            std::ptr::copy_nonoverlapping(symbology.as_ptr(), sym_ptr, symbology.len());
            let resp_ptr = bwipp_wasm_encode_svg(
                sym_ptr,
                symbology.len(),
                data.as_ptr(),
                0,
                options.as_ptr(),
                0,
            );
            let len = u32::from_le_bytes([
                *resp_ptr,
                *resp_ptr.add(1),
                *resp_ptr.add(2),
                *resp_ptr.add(3),
            ]) as usize;
            let body = slice::from_raw_parts(resp_ptr.add(4), len);
            let json = std::str::from_utf8(body).unwrap();
            assert!(
                json.contains(r#""ok":false"#) && json.contains("not_a_real_symbology"),
                "expected error JSON, got {json}"
            );
            bwipp_wasm_dealloc(resp_ptr, len + 4);
            bwipp_wasm_dealloc(sym_ptr, symbology.len());
        }
    }
}
