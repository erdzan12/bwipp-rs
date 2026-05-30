#![allow(
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items,
    clippy::almost_complete_range,
    clippy::needless_range_loop,
    clippy::single_match,
    clippy::type_complexity,
    unused_parens
)]
//! # bwipp-rs
//!
//! Pure-Rust port of [BWIPP](https://github.com/bwipp/postscriptbarcode)
//! (Barcode Writer in Pure PostScript).
//!
//! Current scope: **every user-facing BWIPP catalog encoder** —
//! monochrome 1D, monochrome 2D (matrix, stacked, dot, hex), and the
//! single colour 2D entry (`ultracode`, 6-colour palette). Output is
//! SVG (vector) or PNG (raster); the colour path is a new
//! `Encoded::ColorMatrix` carrier added in Stage 4. The [`Symbology`]
//! enum tells you exactly what's implemented; see
//! [`PORT_STATUS.md`](https://github.com/erdzan12/bwipp-rs/blob/main/rust/PORT_STATUS.md)
//! for the per-row verification status (169 verified + 0 partial out
//! of 169 catalog rows as of this revision).
//!
//! ## Quick start
//!
//! ```
//! use bwipp::{Symbology, Options, render_svg};
//!
//! let svg = render_svg(Symbology::Code128, "Hello, world!", &Options::default()).unwrap();
//! assert!(svg.starts_with("<svg"));
//! ```
//!
//! ## PNG output
//!
//! ```
//! use bwipp::{Symbology, Options, render_png};
//!
//! let png = render_png(Symbology::Ean13, "0123456789012", &Options::default()).unwrap();
//! // PNG files always start with the 8-byte signature \x89 P N G \r \n \x1A \n.
//! assert_eq!(&png[..4], b"\x89PNG");
//! ```
//!
//! ## Looking up a symbology by ID
//!
//! ```
//! use bwipp::Symbology;
//!
//! assert_eq!(Symbology::from_id("code39"), Some(Symbology::Code39));
//! assert_eq!(Symbology::from_id("code128a"), Some(Symbology::Code128)); // alias
//! assert!(Symbology::from_id("not_a_real_symbology").is_none());
//! ```
//!
//! ## Customising rendering
//!
//! The renderer-level fields (`scale`, `bar_height`, `quiet_zone`,
//! `include_text`, foreground/background colours) are typed fields on
//! `Options`; encoder-specific switches go in the `extras` list via
//! [`Options::with`].
//!
//! ```
//! use bwipp::{Symbology, Options, render_svg};
//!
//! let mut opts = Options::default();
//! opts.scale = 3;            // 3 pixels per module (default 4)
//! opts.include_text = true;  // draw the human-readable text below the bars
//! let opts = opts.with("includecheck", "true"); // encoder-specific
//!
//! let svg = render_svg(Symbology::Code39, "HELLO", &opts).unwrap();
//! assert!(svg.contains("<svg"));
//! ```
//!
//! ## Error handling
//!
//! Every entry point returns `Result<_, Error>`. Match on the variant
//! to distinguish a bad payload from a bad option from a backend
//! failure:
//!
//! ```
//! use bwipp::{render_svg, Options, Symbology, Error};
//!
//! match render_svg(Symbology::Ean13, "not digits", &Options::default()) {
//!     Ok(_)                   => panic!("EAN-13 should reject letters"),
//!     Err(Error::InvalidData(msg))   => assert!(msg.contains("digit") || msg.contains("EAN-13")),
//!     Err(Error::InvalidOption(_))   => panic!("not an option problem"),
//!     Err(Error::Unimplemented(_))   => panic!("EAN-13 is implemented"),
//!     Err(Error::Backend(_))         => panic!("not a backend problem"),
//! }
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod encoding;
pub mod error;
pub mod options;
pub mod render;
pub mod symbology;
pub(crate) mod util;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use encoding::{
    Bar4State, BitMatrix, ColorMatrix, DotMatrix, Encoded, LinearPattern, Postal4Pattern, Rgb8,
    StackedPattern,
};
pub use error::Error;
pub use options::Options;
pub use render::{render_png, render_svg, Format};
pub use symbology::Symbology;

// MaxiCode hex-grid carrier for `Encoded::Hex`. Re-exported here so
// consumers pattern-matching on `Encoded::Hex(sym)` can name the
// inner type without spelling the full
// `crate::symbology::maxicode::MaxiCodeSymbol` path. The other
// `Encoded::*` carriers (`BitMatrix`, `ColorMatrix`, `DotMatrix`,
// `LinearPattern`, `Postal4Pattern`, `StackedPattern`) are already
// re-exported above; this keeps the public surface symmetric.
pub use symbology::maxicode::MaxiCodeSymbol;
