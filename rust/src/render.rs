//! Renderers — turn an [`Encoded`] barcode into bytes (SVG or PNG).

use crate::encoding::{BitMatrix, Encoded, LinearPattern, Postal4Pattern, StackedPattern};
use crate::error::Error;
use crate::options::Options;
use crate::symbology::Symbology;

mod png;
mod svg;

/// Output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Scalable vector graphics (string).
    Svg,
    /// PNG raster image.
    Png,
}

/// Render directly to an SVG string.
///
/// # Errors
/// Returns [`Error::InvalidData`] if `data` doesn't match the symbology's
/// input format, or [`Error::InvalidOption`] if `opts.extras` carries a
/// symbology-specific switch the encoder doesn't recognise.
///
/// # Example
/// ```
/// use bwipp::{render_svg, Options, Symbology};
/// let svg = render_svg(Symbology::Code39, "HELLO", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn render_svg(symbology: Symbology, data: &str, opts: &Options) -> Result<String, Error> {
    let encoded = symbology.encode(data, opts)?;
    Ok(match encoded {
        Encoded::Linear(p) => svg::render_linear(&p, opts),
        Encoded::Matrix(m) => svg::render_matrix(&m, opts),
        Encoded::Postal4State(p) => svg::render_postal4(&p, opts),
        Encoded::Stacked(p) => svg::render_stacked(&p, opts),
        Encoded::Dots(d) => svg::render_dots(&d, opts),
        Encoded::Hex(s) => svg::render_hex(&s, opts),
        Encoded::ColorMatrix(m) => svg::render_color_matrix(&m, opts),
    })
}

/// Render to PNG bytes.
///
/// # Errors
/// Same as [`render_svg`]. Also returns [`Error::Backend`] if the PNG
/// encoder library reports a write failure (shouldn't happen with the
/// default `Vec<u8>` writer).
///
/// # Example
/// ```
/// use bwipp::{render_png, Options, Symbology};
/// let png = render_png(Symbology::Code39, "HELLO", &Options::default()).unwrap();
/// assert_eq!(&png[..4], b"\x89PNG");
/// ```
pub fn render_png(symbology: Symbology, data: &str, opts: &Options) -> Result<Vec<u8>, Error> {
    let encoded = symbology.encode(data, opts)?;
    match encoded {
        Encoded::Linear(p) => png::render_linear(&p, opts),
        Encoded::Matrix(m) => png::render_matrix(&m, opts),
        Encoded::Postal4State(p) => png::render_postal4(&p, opts),
        Encoded::Stacked(p) => png::render_stacked(&p, opts),
        Encoded::Dots(d) => png::render_dots(&d, opts),
        Encoded::Hex(s) => png::render_hex(&s, opts),
        Encoded::ColorMatrix(m) => png::render_color_matrix(&m, opts),
    }
}

#[allow(dead_code)]
pub(crate) fn iter_linear_runs(p: &LinearPattern) -> impl Iterator<Item = (u32, bool)> + '_ {
    p.bars
        .iter()
        .enumerate()
        .map(|(i, &width)| (u32::from(width), i % 2 == 0))
}

#[allow(dead_code)]
pub(crate) fn matrix_dimensions(m: &BitMatrix) -> (usize, usize) {
    (m.width(), m.height())
}

#[allow(dead_code)]
pub(crate) fn postal4_bar_count(p: &Postal4Pattern) -> usize {
    p.len()
}

#[allow(dead_code)]
pub(crate) fn stacked_row_count(p: &StackedPattern) -> usize {
    p.height_rows()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin the helper accessors at the bottom of
    /// `render.rs` (`iter_linear_runs`, `matrix_dimensions`,
    /// `postal4_bar_count`, `stacked_row_count`). They have no
    /// production callers (gated by `#[allow(dead_code)]`), but they
    /// remain in the API as utilities for downstream tooling /
    /// future renderers. Mutations on the body (e.g. swap width and
    /// height, drop the `i % 2 == 0` bar-polarity flip,
    /// `p.height_rows()` → `0`) would silently pass — there's nothing
    /// exercising them.
    #[test]
    fn helper_accessors_return_expected_values() {
        use crate::encoding::{Bar4State, LinearPattern, Postal4Pattern};

        // iter_linear_runs: yields (width, is_bar) for each entry,
        // alternating bar→space→bar starting with bar.
        let lp = LinearPattern {
            bars: vec![1, 2, 3, 4],
            text: None,
        };
        let runs: Vec<(u32, bool)> = iter_linear_runs(&lp).collect();
        assert_eq!(
            runs,
            vec![(1, true), (2, false), (3, true), (4, false)],
            "bar/space alternation must start with bar (i==0)"
        );

        // matrix_dimensions: (width, height) tuple straight off BitMatrix.
        let m = BitMatrix::new(7, 4);
        assert_eq!(matrix_dimensions(&m), (7, 4));

        // postal4_bar_count: returns the underlying len().
        let p = Postal4Pattern {
            bars: vec![Bar4State::Full; 5],
            text: None,
        };
        assert_eq!(postal4_bar_count(&p), 5);

        // stacked_row_count: returns underlying height_rows().
        let sp = StackedPattern::new(
            vec![
                LinearPattern {
                    bars: vec![1, 2, 3],
                    text: None,
                },
                LinearPattern {
                    bars: vec![3, 2, 1],
                    text: None,
                },
            ],
            None,
        )
        .unwrap();
        assert_eq!(stacked_row_count(&sp), 2);
    }
}
