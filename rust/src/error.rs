//! Error type.

use std::fmt;

/// Errors that can be returned by the encoder or renderer.
///
/// `Error` implements `std::error::Error` and `std::fmt::Display`, so it
/// integrates with `?`, `anyhow`, and `thiserror` like any other library
/// error.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology, Error};
///
/// let err = render_svg(Symbology::Ean13, "abc", &Options::default()).unwrap_err();
/// // The Display impl prepends a kind-specific prefix:
/// assert!(err.to_string().starts_with("invalid data:"));
/// // Pattern-match for programmatic recovery:
/// assert!(matches!(err, Error::InvalidData(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input data is not valid for the requested symbology
    /// (e.g. wrong length, illegal character, bad check digit).
    InvalidData(String),
    /// An option value is out of range or refers to an unknown setting.
    InvalidOption(String),
    /// The symbology requested is recognized but the requested
    /// option / variant lies outside this Rust port's scope (e.g.
    /// BWIPP feature that the port has consciously deferred).
    ///
    /// **Status note.** As of the current release, no code path in the
    /// crate actually returns this variant — the catalog is
    /// 169 verified / 0 partial (see
    /// [`PORT_STATUS.md`](https://github.com/erdzan12/bwipp-rs/blob/main/rust/PORT_STATUS.md)),
    /// and the BWIPP opt-in knobs that are deferred (e.g. POSICODE
    /// `parsefnc`, Code 16K `sam`, Code One `eci`/`version`,
    /// Ultracode `rev=1`/`raw=true`/`link1`) are surfaced via
    /// [`Error::InvalidData`] / [`Error::InvalidOption`] with a tagged
    /// message rather than via this variant. The variant is kept in
    /// the public API as a reserved slot for future semantic
    /// migration (callers who want to distinguish "input invalid" from
    /// "input valid but encoder lacks support") and removing it would
    /// be a breaking change. Match on it defensively if you care, but
    /// don't expect to see it fire today.
    Unimplemented(&'static str),
    /// A backing crate (e.g. `qrcode`, `datamatrix`) reported an error.
    Backend(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidData(s) => write!(f, "invalid data: {s}"),
            Error::InvalidOption(s) => write!(f, "invalid option: {s}"),
            Error::Unimplemented(s) => write!(f, "{s} is outside this port's scope"),
            Error::Backend(s) => write!(f, "backend error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin every Display format for the Error enum.
    /// Kills `delete match arm` mutations on each arm and any
    /// format-string swap (e.g. "invalid data" → "invalid option").
    #[test]
    fn display_format_per_variant() {
        let e = Error::InvalidData("bad input".into());
        assert_eq!(e.to_string(), "invalid data: bad input");

        let e = Error::InvalidOption("eclevel=Z".into());
        assert_eq!(e.to_string(), "invalid option: eclevel=Z");

        let e = Error::Unimplemented("foo");
        assert_eq!(e.to_string(), "foo is outside this port's scope");

        let e = Error::Backend("qrcode crate failed".into());
        assert_eq!(e.to_string(), "backend error: qrcode crate failed");
    }

    /// Stage 11.A8c — pin PartialEq behavior across variants.
    /// Kills any `derive(PartialEq)` accidental removal mutation.
    #[test]
    fn equality_across_variants() {
        assert_eq!(
            Error::InvalidData("x".into()),
            Error::InvalidData("x".into())
        );
        assert_ne!(
            Error::InvalidData("x".into()),
            Error::InvalidData("y".into())
        );
        // Different variants are never equal.
        assert_ne!(
            Error::InvalidData("x".into()),
            Error::InvalidOption("x".into())
        );
        assert_ne!(Error::Unimplemented("foo"), Error::Backend("foo".into()));
    }
}
