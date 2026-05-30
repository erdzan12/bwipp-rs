#![no_main]
//! Stage 11.A4 — cargo-fuzz harness for every encoder reachable from
//! `Symbology::from_id`.
//!
//! The fuzz input is a tagged byte stream:
//!   byte 0  : symbology selector (mod number-of-variants)
//!   byte 1+ : utf-8-lossy payload
//!
//! For each fuzz iteration the harness encodes via `render_svg` and
//! asserts only that the call returns `Result<_, _>` — never a panic.
//! libfuzzer reports any abort/panic as a crash with the corpus
//! reproducer saved to fuzz/artifacts/.
//!
//! Run a smoke pass with:
//!   cargo +nightly fuzz run fuzz_target_1 -- -max_total_time=30

use bwipp::{render_svg, Options, Symbology};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let all = Symbology::all();
    let sym = all[data[0] as usize % all.len()];
    let payload = String::from_utf8_lossy(&data[1..]);
    let _ = render_svg(sym, &payload, &Options::default());
});
