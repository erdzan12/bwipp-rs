//! Stage 11.A3 — property tests across every encoder.
//!
//! For each `Symbology` variant returned by `Symbology::all()`, this
//! suite asserts three invariants over a deterministic-LCG corpus:
//!
//! 1. **No panic on generic inputs.** Every symbology is exercised
//!    with a short pool of generic payloads — digits, ASCII, GS1-AI
//!    shape, URI shape. Result is allowed to be `Err(_)` (most
//!    symbologies reject inputs that don't match their narrow
//!    payload spec); the only forbidden outcome is a process abort.
//!
//! 2. **No panic on random byte fuzz.** A second pool of purely-
//!    random byte payloads (length 0..=64, including embedded zero
//!    bytes and high-bit characters) is fed to every symbology. Same
//!    rule: `Err(_)` is fine, `panic!` is not.
//!
//! 3. **Deterministic output for repeated encodes.** For the
//!    representative encoders that reliably accept an ASCII canonical
//!    payload, repeating the same `(symbology, payload, opts)`
//!    triple must produce identical bytes across runs.
//!
//! The LCG (glibc-style parameters) keeps the test reproducible and
//! avoids pulling `proptest` into the dependency tree.

use bwipp::{render_svg, Options, Symbology};

/// Deterministic LCG (glibc parameters).
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Self {
        Self(seed)
    }
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.0
    }
    fn next_byte(&mut self) -> u8 {
        (self.next_u32() & 0xff) as u8
    }
    fn next_in_range(&mut self, lo: usize, hi: usize) -> usize {
        let span = hi - lo + 1;
        lo + (self.next_u32() as usize % span)
    }
}

/// A small pool of generic payloads. Each is plausible for a
/// non-trivial subset of the catalog; symbologies that reject any
/// given shape simply return `Err(_)`, which is fine — we only
/// assert that the call doesn't panic.
const GENERIC_PAYLOADS: &[&str] = &[
    // Plain digits.
    "1234567",
    "0123456789012",
    "12345678901231",
    // GS1 AI shape.
    "(01)12345678901231",
    "(00)012345678000000005",
    "(01)24012345678905",
    "(01)15012345678907",
    // ASCII text.
    "ABC",
    "Hello",
    "Hello, world!",
    "A123BJC5D6E71",
    // GS1 DigitalLink URI.
    "https://example.com/01/09521234543213",
    // HIBC-shaped.
    "A99912345/52001510X3",
    // Postal-shaped.
    "SN34RD1A",
];

/// Property 1 — every Symbology variant + every generic payload
/// combination must return cleanly (Ok or Err — never panic).
#[test]
fn no_panic_on_generic_payloads_for_every_symbology() {
    let opts = Options::default();
    for &sym in Symbology::all() {
        for &payload in GENERIC_PAYLOADS {
            let payload_owned = payload.to_string();
            let result = std::panic::catch_unwind(|| {
                let _ = render_svg(sym, &payload_owned, &opts);
            });
            assert!(
                result.is_ok(),
                "encoder for {sym:?} panicked on payload {payload_owned:?}",
            );
        }
    }
}

/// Property 2 — random byte fuzz. 50 random payloads per symbology
/// × 88 symbologies = 4400 fuzz calls.
#[test]
fn no_panic_on_random_byte_fuzz_for_every_symbology() {
    let opts = Options::default();
    let mut lcg = Lcg::new(0xC0DE_BABE);
    for &sym in Symbology::all() {
        for iter in 0..50 {
            let len = lcg.next_in_range(0, 64);
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                bytes.push(lcg.next_byte());
            }
            let payload: String = String::from_utf8_lossy(&bytes).into_owned();
            let payload_for_msg = payload.clone();
            let result = std::panic::catch_unwind(|| {
                let _ = render_svg(sym, &payload, &opts);
            });
            assert!(
                result.is_ok(),
                "encoder for {sym:?} panicked on fuzz iter {iter} \
                 (payload bytes: {payload_for_msg:?})",
            );
        }
    }
}

/// Property 3 — encoders are deterministic across repeated invocations.
/// Sample a representative set whose canonical payloads succeed.
#[test]
fn deterministic_output_for_repeated_encodes() {
    let opts = Options::default();
    let cases: &[(Symbology, &str)] = &[
        (Symbology::Code39, "HELLO"),
        (Symbology::Code128, "Hello, world!"),
        (Symbology::Ean13, "0123456789012"),
        (Symbology::UpcA, "012345678905"),
        (Symbology::QrCode, "Hello, world!"),
        (Symbology::DataMatrix, "Hello, world!"),
        (Symbology::AztecCode, "Hello, world!"),
        (Symbology::Pdf417, "Hello, world!"),
        (Symbology::MicroPdf417, "Hello"),
        (Symbology::Ultracode, "Hello"),
        (Symbology::DotCode, "Hello"),
        (Symbology::Maxicode, "Hello, world!"),
    ];
    let mut at_least_one_ok = false;
    for &(sym, payload) in cases {
        let Ok(first) = render_svg(sym, payload, &opts) else {
            // The canonical payload was rejected — skip this row, but
            // ensure at least one of the representatives is exercised.
            continue;
        };
        at_least_one_ok = true;
        let Ok(second) = render_svg(sym, payload, &opts) else {
            panic!("{sym:?} succeeded once but failed on re-encode")
        };
        assert_eq!(first, second, "{sym:?} is non-deterministic across calls",);
        let Ok(third) = render_svg(sym, payload, &opts) else {
            panic!("{sym:?} succeeded twice but failed on third encode")
        };
        assert_eq!(first, third, "{sym:?} drifted after 3 encodes");
    }
    assert!(
        at_least_one_ok,
        "no representative symbology accepted its canonical payload — \
         A3 determinism check would have silently no-op'd",
    );
}

/// Property 4 — fixed-payload dimension stability per `Symbology`.
/// Encoders are pure functions of `(payload, opts)`, so two calls
/// with the same arguments must produce SVGs whose `viewBox`
/// dimensions (extracted via simple substring match) are identical.
#[test]
fn dimension_stable_for_fixed_symbology_payload() {
    let opts = Options::default();
    let mut hits = 0;
    for &sym in Symbology::all() {
        // Pick a payload that the symbology might accept.
        for &payload in &["Hello", "12345", "ABC", "(01)12345678901231"] {
            let Ok(svg1) = render_svg(sym, payload, &opts) else {
                continue;
            };
            let Ok(svg2) = render_svg(sym, payload, &opts) else {
                continue;
            };
            // Whole SVG must be identical (encoders are pure).
            assert_eq!(svg1, svg2, "{sym:?} not stable for {payload:?}");
            hits += 1;
            break; // one accepted payload per symbology is enough.
        }
    }
    assert!(
        hits >= 20,
        "expected at least 20 symbologies to accept one of our test \
         payloads; got {hits}",
    );
}
