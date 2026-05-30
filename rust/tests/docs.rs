//! Stage 11.A7 — docs.rs preview verification.
//!
//! The existing strict-CI gate in `scripts/ci-rust.sh` runs
//!
//!   RUSTDOCFLAGS="--cfg docsrs -D warnings" \
//!     cargo doc --no-deps --all-features --quiet
//!
//! on every CI run — that's the docs.rs build profile (matches
//! `[package.metadata.docs.rs]` in `rust/Cargo.toml`). Any rustdoc
//! warning — broken intra-doc link, private-item leak, missing
//! example syntax, etc. — already fails CI.
//!
//! This integration test adds a complementary content-level check:
//! it shells out to the same `cargo doc` command, then verifies the
//! generated HTML actually contains:
//!
//! 1. an entry page (`bwipp/index.html`),
//! 2. an enum page for `Symbology`, and
//! 3. a struct page for `Options`.
//!
//! If any of those go missing the test fails loudly — that would
//! mean docs.rs is publishing an empty (or wrongly-scoped) HTML
//! tree.
//!
//! The test is gated by `#[ignore]` by default because it runs
//! `cargo doc` from inside `cargo test`, which is slow (~5-10 s) and
//! redundant with the strict-CI doc gate. Run it explicitly with
//! `cargo test --test docs -- --ignored docs_build_produces_*` when
//! you want a content-level smoke check.

use std::path::PathBuf;
use std::process::Command;

/// Resolve the `cargo` binary. Cargo sets `CARGO` for cargo-spawned
/// processes; outside Cargo we fall back to `cargo` on `$PATH`.
fn cargo_path() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Find the workspace target/doc directory. We can't rely on
/// `CARGO_TARGET_DIR` always being set, so probe relative to
/// `CARGO_MANIFEST_DIR`.
fn doc_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // `rust/target/doc/bwipp/...` after `cargo doc` from `rust/`.
    manifest.join("target").join("doc")
}

/// A7.1 — `cargo doc` runs cleanly with the docs.rs flags.
#[test]
#[ignore = "slow: runs cargo doc; covered by strict CI's doc gate"]
fn docs_build_succeeds_with_docs_rs_flags() {
    let out = Command::new(cargo_path())
        .args([
            "doc",
            "--no-deps",
            "--all-features",
            "--manifest-path",
            "Cargo.toml",
        ])
        .env("RUSTDOCFLAGS", "--cfg docsrs -D warnings")
        .output()
        .expect("failed to spawn cargo doc");
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        panic!("cargo doc failed:\n{stderr}");
    }
}

/// A7.2 — the generated HTML tree contains the expected entry pages.
#[test]
#[ignore = "slow: requires running cargo doc first"]
fn docs_build_produces_expected_pages() {
    // Run cargo doc to populate target/doc.
    let out = Command::new(cargo_path())
        .args([
            "doc",
            "--no-deps",
            "--all-features",
            "--manifest-path",
            "Cargo.toml",
            "--quiet",
        ])
        .env("RUSTDOCFLAGS", "--cfg docsrs -D warnings")
        .output()
        .expect("failed to spawn cargo doc");
    assert!(out.status.success(), "cargo doc failed");

    let doc = doc_dir();
    let crate_dir = doc.join("bwipp");
    assert!(
        crate_dir.is_dir(),
        "missing bwipp/ in {}: docs.rs build wouldn't ship anything",
        doc.display(),
    );
    // Symbology and Options live in the symbology and options
    // submodules respectively. They're re-exported at the crate root
    // (so the bwipp::Symbology path works), but rustdoc emits the
    // canonical page under the defining module's HTML tree.
    let must_exist = [
        crate_dir.join("index.html"),
        crate_dir.join("symbology").join("enum.Symbology.html"),
        crate_dir.join("options").join("struct.Options.html"),
    ];
    for path in must_exist {
        assert!(path.is_file(), "missing docs page: {}", path.display(),);
    }
}

/// A7.3 — the `Symbology` enum page mentions the major variants.
/// Cheap content-level smoke that catches accidental rust-doc scope
/// regressions (e.g. if a feature flag silently hides the QR Code
/// variant from the public API).
#[test]
#[ignore = "slow: requires running cargo doc first"]
fn docs_symbology_page_lists_core_variants() {
    let out = Command::new(cargo_path())
        .args([
            "doc",
            "--no-deps",
            "--all-features",
            "--manifest-path",
            "Cargo.toml",
            "--quiet",
        ])
        .env("RUSTDOCFLAGS", "--cfg docsrs -D warnings")
        .output()
        .expect("failed to spawn cargo doc");
    assert!(out.status.success(), "cargo doc failed");

    let page = doc_dir()
        .join("bwipp")
        .join("symbology")
        .join("enum.Symbology.html");
    let html = std::fs::read_to_string(&page)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", page.display()));
    // Each name appears as a rustdoc enum variant anchor like
    // `<a href="enum.Symbology.html#variant.Code128">`. We don't pin
    // the exact link form (it changes between rustdoc versions); we
    // just confirm the variant name appears.
    for variant in [
        "Code128",
        "QrCode",
        "DataMatrix",
        "Ean13",
        "Pdf417",
        "Ultracode",
    ] {
        assert!(
            html.contains(variant),
            "Symbology page is missing the {variant:?} variant in its rendered HTML",
        );
    }
}
