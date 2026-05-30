//! Stage 11.A5 — CLI binary integration tests.
//!
//! Exercises the `bwipp` binary (defined in `src/bin/bwipp.rs`) for
//! the full set of CLI surface contracts:
//!
//! 1. **No-args prints usage + non-zero exit code** (matches the
//!    `if args.len() < 3` guard at `bin/bwipp.rs:22-30`).
//! 2. **`render every Symbology variant via the CLI for a canonical
//!    payload`** — every `Symbology::all()` member gets one CLI run
//!    against a generic payload; the test passes as long as the
//!    process exits cleanly (status 0 with a valid output, or status
//!    1 with a clean `render error:` message — never a panic / abort).
//! 3. **SVG and PNG output bytes are deterministic and non-empty** —
//!    running the CLI twice with identical args produces byte-equal
//!    outputs.
//! 4. **Exit codes match the documented protocol**:
//!    - `2` for usage / unknown-symbology / unknown-format.
//!    - `1` for render errors (data doesn't fit the symbology).
//!    - `0` for success.
//!
//! The tests use `CARGO_BIN_EXE_bwipp` (set by Cargo when running
//! `cargo test --bin bwipp` or implicit integration tests), so they
//! pick up the just-built binary without `cargo build` from inside
//! the test.

use std::path::PathBuf;
use std::process::Command;

use bwipp::Symbology;

/// Resolve the just-built `bwipp` binary. Cargo sets
/// `CARGO_BIN_EXE_bwipp` to the binary path for integration tests of
/// crates that ship binaries.
fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwipp"))
}

/// Run the CLI with the given args and a unique tmpfile output path
/// (so concurrent test runs don't stomp on each other). Returns
/// `(stdout, stderr, exit_code, output_bytes_on_success)`.
fn run_cli(args: &[&str]) -> (String, String, i32, Vec<u8>) {
    // Append a tmp output path so the binary writes somewhere
    // predictable, then we read the file back. If the caller already
    // supplied an output path (4th positional arg), use that.
    let tmp = tempfile_path();
    let mut full: Vec<&str> = args.to_vec();
    if args.len() == 3 {
        full.push(tmp.to_str().expect("utf8 tmp path"));
    }
    let out = Command::new(binary_path())
        .args(&full)
        .output()
        .expect("failed to spawn bwipp binary");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let code = out.status.code().unwrap_or(-1);
    let bytes = std::fs::read(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    (stdout, stderr, code, bytes)
}

/// Generate a unique temporary file path keyed off process id + a
/// monotonic counter. Avoids `tempfile` crate dep.
fn tempfile_path() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut path = std::env::temp_dir();
    path.push(format!("bwipp-cli-test-{pid}-{n}.bin"));
    path
}

/// A5.1 — no-args invocation prints usage and exits with code 2.
#[test]
fn cli_no_args_prints_usage_with_exit_2() {
    let out = Command::new(binary_path())
        .output()
        .expect("failed to spawn bwipp binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("usage: bwipp"),
        "stderr should contain usage line; got {stderr:?}",
    );
    assert!(
        stderr.contains("known symbologies:"),
        "stderr should list known symbologies; got {stderr:?}",
    );
    assert_eq!(out.status.code(), Some(2));
}

/// A5.2 — unknown symbology returns exit code 2 with a clear error
/// message.
#[test]
fn cli_unknown_symbology_exits_with_code_2() {
    let (_, stderr, code, _) = run_cli(&["not-a-real-symbology", "data", "svg"]);
    assert_eq!(code, 2, "unknown symbology should exit with code 2");
    assert!(
        stderr.contains("unknown symbology"),
        "stderr should contain `unknown symbology`; got {stderr:?}",
    );
}

/// A5.3 — unknown output format returns exit code 2.
#[test]
fn cli_unknown_format_exits_with_code_2() {
    let (_, stderr, code, _) = run_cli(&["code128", "Hello", "tiff"]);
    assert_eq!(code, 2, "unknown format should exit with code 2");
    assert!(
        stderr.contains("unknown format"),
        "stderr should contain `unknown format`; got {stderr:?}",
    );
}

/// A5.4 — invalid payload returns exit code 1 with a `render error:`
/// stderr line (the renderer rejects but the binary doesn't panic).
#[test]
fn cli_invalid_payload_exits_with_code_1() {
    // EAN-13 needs exactly 13 digits; "abc" is wholly invalid.
    let (_, stderr, code, _) = run_cli(&["ean13", "abc", "svg"]);
    assert_eq!(code, 1, "invalid payload should exit with code 1");
    assert!(
        stderr.contains("render error:"),
        "stderr should start with `render error:`; got {stderr:?}",
    );
}

/// A5.5 — success path writes a non-empty SVG and exits with code 0.
#[test]
fn cli_writes_non_empty_svg_for_canonical_payload() {
    let (stdout, stderr, code, bytes) = run_cli(&["code128", "Hello", "svg"]);
    assert_eq!(code, 0, "expected exit 0; stderr={stderr:?}");
    assert!(stdout.contains("wrote"), "stdout was {stdout:?}");
    assert!(!bytes.is_empty(), "output SVG was empty");
    let svg = String::from_utf8_lossy(&bytes);
    assert!(
        svg.starts_with("<svg"),
        "output doesn't look like SVG: {}",
        &svg[..svg.len().min(64)],
    );
}

/// A5.6 — success path writes a non-empty PNG with the correct magic
/// bytes.
#[test]
fn cli_writes_non_empty_png_for_canonical_payload() {
    let (_, stderr, code, bytes) = run_cli(&["code128", "Hello", "png"]);
    assert_eq!(code, 0, "expected exit 0; stderr={stderr:?}");
    assert!(bytes.len() >= 8, "PNG too small: {} bytes", bytes.len());
    // PNG magic: 89 50 4E 47 0D 0A 1A 0A
    let magic = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    assert_eq!(&bytes[..8], &magic, "output is not a PNG");
}

/// A5.7 — SVG output is deterministic: same args twice → identical bytes.
#[test]
fn cli_svg_output_is_deterministic() {
    let (_, _, c1, b1) = run_cli(&["code128", "Hello, world!", "svg"]);
    let (_, _, c2, b2) = run_cli(&["code128", "Hello, world!", "svg"]);
    assert_eq!(c1, 0);
    assert_eq!(c2, 0);
    assert_eq!(b1, b2, "SVG output drifts between runs");
}

/// A5.8 — PNG output is deterministic.
#[test]
fn cli_png_output_is_deterministic() {
    let (_, _, c1, b1) = run_cli(&["code128", "Hello, world!", "png"]);
    let (_, _, c2, b2) = run_cli(&["code128", "Hello, world!", "png"]);
    assert_eq!(c1, 0);
    assert_eq!(c2, 0);
    assert_eq!(b1, b2, "PNG output drifts between runs");
}

/// A5.9 — every `Symbology::all()` variant either renders cleanly
/// (exit 0) or rejects cleanly (exit 1 with a `render error:`
/// message). The forbidden outcome is a process abort / panic that
/// bubbles up as a non-{0,1,2} exit code.
#[test]
fn cli_every_symbology_returns_a_clean_exit_code() {
    // A small pool of generic payloads. We try each one and accept
    // the first that produces a successful render (or fall through to
    // verifying that the binary at least exits cleanly).
    let payloads = &[
        "Hello",
        "12345",
        "0123456789012",                         // EAN-13-shape
        "012345678905",                          // UPC-A-shape
        "(01)12345678901231",                    // GS1 AI
        "(01)24012345678905",                    // DataBar GTIN-14
        "(01)15012345678907",                    // DataBar Limited GTIN-14
        "A123BJC5D6E71",                         // HIBC LIC
        "SN34RD1A",                              // RoyalMail
        "https://example.com/01/09521234543213", // GS1 DigitalLink
        "Hello, world!",
    ];
    let mut covered = 0;
    for &sym in Symbology::all() {
        let id = sym.id();
        let mut any_ok = false;
        for payload in payloads {
            let (_, _, code, _) = run_cli(&[id, payload, "svg"]);
            assert!(
                code == 0 || code == 1,
                "{id} returned exit code {code} (expected 0 or 1) for payload {payload:?}",
            );
            if code == 0 {
                any_ok = true;
                break;
            }
        }
        if any_ok {
            covered += 1;
        }
    }
    // Sanity: most symbologies should accept at least one of our
    // generic payloads. The exact count varies as new variants land;
    // 25 is a conservative lower bound.
    assert!(
        covered >= 25,
        "expected at least 25 symbologies to accept one of our generic \
         payloads via the CLI; got {covered}",
    );
}
