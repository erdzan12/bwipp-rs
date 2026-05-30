#![no_main]
//! Stage 11.A8d — dedicated cargo-fuzz target for the `Code128`
//! encoder family. Concentrates fuzzing budget on one complex encoder
//! implementation (vs. `fuzz_target_1` which dispatches across all 154
//! symbology variants and so gives each only a thin slice of budget).
//!
//! Drives the public `render_svg` entry point with arbitrary input.
//! The contract being fuzzed: the encoder must return
//! `Result<_, Error>` for ANY input — never panic, never UB. libfuzzer
//! reports any abort/panic/sanitizer finding as a crash.
//!
//!   cargo +nightly fuzz run fuzz_code128 -- -max_total_time=30

use bwipp::{render_svg, Options, Symbology};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let payload = String::from_utf8_lossy(data);
    let _ = render_svg(Symbology::Code128, &payload, &Options::default());
});
