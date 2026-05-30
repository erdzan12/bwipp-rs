//! Compile + run guard for the code snippets shown in the repository
//! `README.md` and `rust/README.md`. Keeps the public-API examples from
//! silently rotting (e.g. a wrong crate name in a `use` line).

use bwipp::{render_svg, Options, Symbology};

/// Mirrors the root `README.md` quick-start snippet verbatim.
#[test]
fn root_readme_quickstart_compiles_and_runs() {
    let svg = render_svg(Symbology::QrCode, "Hello", &Options::default())
        .expect("QrCode render must succeed");
    assert!(svg.starts_with("<svg"));
}
