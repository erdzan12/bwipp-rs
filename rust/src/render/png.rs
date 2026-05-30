//! PNG renderer (raster).

use image::{ImageBuffer, ImageFormat, Rgb};

use crate::encoding::{
    Bar4State, BitMatrix, DotMatrix, LinearPattern, Postal4Pattern, StackedPattern,
};
use crate::error::Error;
use crate::options::Options;

pub(crate) fn render_linear(pattern: &LinearPattern, opts: &Options) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let bar_height = opts.bar_height.max(1);

    let width = (pattern.total_width() + 2 * quiet) * scale;
    let height = bar_height * scale;

    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));

    let mut x = quiet * scale;
    for (i, &w) in pattern.bars.iter().enumerate() {
        let is_bar = i % 2 == 0;
        let span = u32::from(w) * scale;
        if is_bar && w > 0 {
            for px in x..x + span {
                for py in 0..height {
                    img.put_pixel(px, py, Rgb(opts.foreground));
                }
            }
        }
        x += span;
    }

    encode_png(img)
}

pub(crate) fn render_matrix(matrix: &BitMatrix, opts: &Options) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width = (matrix.width() as u32 + 2 * quiet) * scale;
    let height = (matrix.height() as u32 + 2 * quiet) * scale;

    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));

    for y in 0..matrix.height() {
        for x in 0..matrix.width() {
            if matrix.get(x, y) {
                let px = (quiet + x as u32) * scale;
                let py = (quiet + y as u32) * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        img.put_pixel(px + dx, py + dy, Rgb(opts.foreground));
                    }
                }
            }
        }
    }

    encode_png(img)
}

/// Render a [`ColorMatrix`] as a per-cell coloured pixel grid.
///
/// Each cell carries a palette index 0..=7 into the matrix's
/// 8-entry RGB palette. Index 0 is the background (the canvas is
/// pre-filled with `palette[0]`). For non-zero indices, the cell's
/// `scale × scale` pixel block is filled with the palette colour.
pub(crate) fn render_color_matrix(
    matrix: &crate::encoding::ColorMatrix,
    opts: &Options,
) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width = (matrix.width() as u32 + 2 * quiet) * scale;
    let height = (matrix.height() as u32 + 2 * quiet) * scale;
    let palette = matrix.palette();

    let bg = palette[0];
    let mut img = ImageBuffer::from_pixel(width, height, Rgb([bg.r, bg.g, bg.b]));

    for y in 0..matrix.height() {
        for x in 0..matrix.width() {
            let idx = matrix.get(x, y);
            if idx == 0 {
                continue;
            }
            let c = palette[idx as usize];
            let rgb = Rgb([c.r, c.g, c.b]);
            let px = (quiet + x as u32) * scale;
            let py = (quiet + y as u32) * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel(px + dx, py + dy, rgb);
                }
            }
        }
    }

    encode_png(img)
}

/// Render a [`DotMatrix`] as filled circles on a background field.
/// Each `true` cell becomes a disc of diameter `0.8 * scale`,
/// rasterised by sampling at pixel centres. Slow but simple — fine
/// for the symbol sizes DotCode produces.
pub(crate) fn render_dots(dots: &DotMatrix, opts: &Options) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width = (dots.width() as u32 + 2 * quiet) * scale;
    let height = (dots.height() as u32 + 2 * quiet) * scale;
    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));
    let radius = ((scale as f32) * 0.4).max(0.5);
    let r2 = radius * radius;
    for y in 0..dots.height() {
        for x in 0..dots.width() {
            if !dots.get(x, y) {
                continue;
            }
            let cx = (quiet + x as u32) as f32 * scale as f32 + scale as f32 * 0.5;
            let cy = (quiet + y as u32) as f32 * scale as f32 + scale as f32 * 0.5;
            // Bounding box around the circle.
            let x_lo = (cx - radius).floor().max(0.0) as u32;
            let x_hi = ((cx + radius).ceil() as u32).min(width.saturating_sub(1));
            let y_lo = (cy - radius).floor().max(0.0) as u32;
            let y_hi = ((cy + radius).ceil() as u32).min(height.saturating_sub(1));
            for py in y_lo..=y_hi {
                for px in x_lo..=x_hi {
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    if dx * dx + dy * dy <= r2 {
                        img.put_pixel(px, py, Rgb(opts.foreground));
                    }
                }
            }
        }
    }
    encode_png(img)
}

/// Render a MaxiCode hex symbol as filled hexagons. Rasterised by
/// point-in-hex tests around each cell's bounding box.
pub(crate) fn render_hex(
    sym: &crate::symbology::maxicode::MaxiCodeSymbol,
    opts: &Options,
) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let h_step = scale as f32;
    let v_step = scale as f32 * 3f32.sqrt() / 2.0;
    let cols = sym.cols() as f32;
    let rows = sym.rows() as f32;
    let width = ((cols + 0.5) * h_step + 2.0 * (quiet as f32) * scale as f32).ceil() as u32;
    let height =
        ((rows - 1.0) * v_step + scale as f32 + 2.0 * (quiet as f32) * scale as f32).ceil() as u32;
    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));
    // Hex circumradius (vertex distance) — slight inset to keep gaps.
    let r = scale as f32 / 3f32.sqrt() * 0.95;
    // Apothem (centre-to-edge of a flat side) = r * sqrt(3) / 2.
    let apothem = r * 3f32.sqrt() / 2.0;
    for row in 0..sym.rows() {
        for col in 0..sym.cols() {
            if !sym.is_on(row, col) {
                continue;
            }
            let x_offset = if row % 2 == 1 { h_step / 2.0 } else { 0.0 };
            let cx = (quiet as f32) * scale as f32 + col as f32 * h_step + x_offset + h_step / 2.0;
            let cy = (quiet as f32) * scale as f32 + row as f32 * v_step + scale as f32 / 2.0;
            // Bounding box: pointy-top hex is `r` tall above centre,
            // `r` tall below, `apothem` wide each side.
            let x_lo = ((cx - apothem).floor() as i32).max(0) as u32;
            let x_hi = ((cx + apothem).ceil() as u32).min(width.saturating_sub(1));
            let y_lo = ((cy - r).floor() as i32).max(0) as u32;
            let y_hi = ((cy + r).ceil() as u32).min(height.saturating_sub(1));
            for py in y_lo..=y_hi {
                for px in x_lo..=x_hi {
                    let dx = (px as f32 + 0.5 - cx).abs();
                    let dy = (py as f32 + 0.5 - cy).abs();
                    // Pointy-top hex: |dy| ≤ r AND |dx| ≤ apothem AND
                    // dy ≤ r - (dx / apothem) * (r - r/2)
                    // Simpler check: the hex is the intersection of
                    // three slabs. For pointy-top with circumradius r:
                    //   |dy| ≤ r
                    //   √3/2 * |dx| + |dy|/2 ≤ √3/2 * r  (i.e. apothem-side)
                    //   √3/2 * |dx| - |dy|/2 ≤ √3/2 * r
                    let slab1 = dy <= r;
                    let s = 3f32.sqrt() / 2.0;
                    let slab2 = s * dx + dy / 2.0 <= s * r;
                    let slab3 = s * dx - dy / 2.0 <= s * r;
                    if slab1 && slab2 && slab3 {
                        img.put_pixel(px, py, Rgb(opts.foreground));
                    }
                }
            }
        }
    }
    encode_png(img)
}

pub(crate) fn render_postal4(pattern: &Postal4Pattern, opts: &Options) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let bar_width: u32 = 1;
    let gap: u32 = 1;
    let total_h_modules: u32 = 8;
    let top_pad_modules: u32 = 3;
    let bot_pad_modules: u32 = 3;

    let n = pattern.bars.len() as u32;
    let width_modules = if n == 0 {
        2 * quiet
    } else {
        2 * quiet + (bar_width * n) + (gap * n.saturating_sub(1))
    };
    let width = width_modules * scale;
    let height = total_h_modules * scale;

    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));

    for (i, bar) in pattern.bars.iter().enumerate() {
        let x = quiet * scale + (i as u32) * (bar_width + gap) * scale;
        let bar_px = bar_width * scale;
        let (y_start_mod, y_end_mod): (u32, u32) = match bar {
            Bar4State::Full => (0, total_h_modules),
            Bar4State::Ascender => (0, total_h_modules - bot_pad_modules),
            Bar4State::Descender => (top_pad_modules, total_h_modules),
            Bar4State::Tracker => (top_pad_modules, total_h_modules - bot_pad_modules),
        };
        let y0 = y_start_mod * scale;
        let y1 = y_end_mod * scale;
        for px in x..x + bar_px {
            for py in y0..y1 {
                img.put_pixel(px, py, Rgb(opts.foreground));
            }
        }
    }

    encode_png(img)
}

pub(crate) fn render_stacked(pattern: &StackedPattern, opts: &Options) -> Result<Vec<u8>, Error> {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let row_height = opts.bar_height.max(1).max(8);
    let gap = 1u32;
    let n_rows = pattern.rows.len() as u32;
    let width = (pattern.width() + 2 * quiet) * scale;
    let height = if n_rows == 0 {
        2 * quiet * scale
    } else {
        (n_rows * row_height + (n_rows - 1) * gap + 2 * quiet) * scale
    };

    let mut img = ImageBuffer::from_pixel(width, height, Rgb(opts.background));
    for (row_idx, row) in pattern.rows.iter().enumerate() {
        let row_y0 = (quiet + (row_idx as u32) * (row_height + gap)) * scale;
        let row_y1 = row_y0 + row_height * scale;
        let mut x = quiet * scale;
        for (i, &w) in row.bars.iter().enumerate() {
            let is_bar = i % 2 == 0;
            let span = u32::from(w) * scale;
            if is_bar && w > 0 {
                for px in x..x + span {
                    for py in row_y0..row_y1 {
                        img.put_pixel(px, py, Rgb(opts.foreground));
                    }
                }
            }
            x += span;
        }
    }
    encode_png(img)
}

fn encode_png(img: ImageBuffer<Rgb<u8>, Vec<u8>>) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| Error::Backend(format!("PNG encode failed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{ColorMatrix, Rgb8};
    use image::ImageReader;
    use std::io::Cursor;

    /// Smoke test: a fabricated 2-cell `ColorMatrix` renders to PNG
    /// bytes that (a) start with the PNG magic header and (b) decode
    /// back to a raster where the cyan cell and the black cell have
    /// exactly the palette colours we put in.
    ///
    /// This pins the colour PNG pipeline (Stage 4a) independently of
    /// the ultracode encoder — a future renderer regression can't
    /// hide behind the missing encoder.
    #[test]
    fn render_color_matrix_round_trips_through_png() {
        let palette: [Rgb8; 8] = [
            Rgb8::new(0xff, 0xff, 0xff), // 0 white
            Rgb8::new(0x00, 0xff, 0xff), // 1 cyan
            Rgb8::new(0xff, 0x00, 0xff), // 2 magenta
            Rgb8::new(0xff, 0xff, 0x00), // 3 yellow
            Rgb8::new(0x00, 0xff, 0x00), // 4 green
            Rgb8::new(0x00, 0x00, 0xff), // 5 blue
            Rgb8::new(0xff, 0x00, 0x00), // 6 red
            Rgb8::new(0x00, 0x00, 0x00), // 7 black
        ];
        let mut m = ColorMatrix::new(2, 1, palette);
        m.set(0, 0, 1); // cyan
        m.set(1, 0, 7); // black

        let opts = Options {
            scale: 3,
            quiet_zone: 0,
            ..Options::default()
        };
        let png_bytes = render_color_matrix(&m, &opts).expect("PNG should encode");

        // PNG magic header.
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        // Decode back and probe pixel centres.
        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 6);
        assert_eq!(img.height(), 3);
        // Cell (0,0) → cyan. Pixel (1,1) is inside that 3×3 block.
        assert_eq!(img.get_pixel(1, 1).0, [0x00, 0xff, 0xff]);
        // Cell (1,0) → black. Pixel (4,1) is inside that 3×3 block.
        assert_eq!(img.get_pixel(4, 1).0, [0x00, 0x00, 0x00]);
    }

    /// Stage 11.A8c — pin `render_color_matrix` (PNG) cell-block
    /// expansion, palette indexing, and skip-on-idx-0 behaviour at
    /// the pixel level. The existing round-trip test only probes
    /// 2 sample pixels; this one fills a 2×2 matrix with 3 distinct
    /// non-zero indices + 1 background cell and validates the
    /// per-cell 2×2 block at quiet_zone=1, scale=2.
    ///
    /// Setup:
    ///   Palette: 0=white, 1=cyan, 2=magenta, 3=yellow.
    ///   Cells: (0,0)=1 cyan, (1,0)=2 magenta, (0,1)=3 yellow, (1,1)=0.
    /// Geometry (scale=2, quiet=1): width=8, height=8.
    ///   Cell (0,0) → block (2,2)..(3,3) cyan #00ffff.
    ///   Cell (1,0) → block (4,2)..(5,3) magenta #ff00ff.
    ///   Cell (0,1) → block (2,4)..(3,5) yellow #ffff00.
    ///   Cell (1,1) idx=0 → stays palette[0] = white via skip continue.
    ///
    /// Mutations caught:
    ///   * `palette[idx as usize]` → `palette[(idx + 1) as usize]`
    ///     shifts every cell's colour.
    ///   * `if idx == 0 { continue }` removal: would write palette[0]
    ///     for idx=0 cells — same colour as bg so doesn't show, BUT
    ///     mutations to the `palette` indexing combined with this
    ///     would surface differently.
    ///   * `(quiet + x as u32) * scale` shift formula.
    ///   * `(quiet + y as u32) * scale`.
    ///   * `for dy in 0..scale` / `dx in 0..scale` boundary — corner
    ///     pixels of each block.
    ///   * Quiet-zone fill from `palette[0]` (not opts.background).
    #[test]
    fn render_color_matrix_png_pins_per_cell_palette_blocks() {
        let palette: [Rgb8; 8] = [
            Rgb8::new(0xff, 0xff, 0xff), // 0 white
            Rgb8::new(0x00, 0xff, 0xff), // 1 cyan
            Rgb8::new(0xff, 0x00, 0xff), // 2 magenta
            Rgb8::new(0xff, 0xff, 0x00), // 3 yellow
            Rgb8::new(0x00, 0xff, 0x00), // 4 green
            Rgb8::new(0x00, 0x00, 0xff), // 5 blue
            Rgb8::new(0xff, 0x00, 0x00), // 6 red
            Rgb8::new(0x00, 0x00, 0x00), // 7 black
        ];
        let mut m = ColorMatrix::new(2, 2, palette);
        m.set(0, 0, 1); // cyan
        m.set(1, 0, 2); // magenta
        m.set(0, 1, 3); // yellow
                        // (1,1) stays 0 (background)

        let opts = Options {
            scale: 2,
            quiet_zone: 1,
            ..Options::default()
        };
        let png = render_color_matrix(&m, &opts).expect("PNG encode");
        let img = ImageReader::new(Cursor::new(&png))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);

        // Cell (0,0) cyan block (2,2)..(3,3): probe all 4 corners.
        for &(x, y) in &[(2u32, 2u32), (3, 2), (2, 3), (3, 3)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0xff, 0xff],
                "cell (0,0) cyan pixel ({x},{y})"
            );
        }
        // Cell (1,0) magenta block (4,2)..(5,3).
        for &(x, y) in &[(4u32, 2u32), (5, 2), (4, 3), (5, 3)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0x00, 0xff],
                "cell (1,0) magenta pixel ({x},{y})"
            );
        }
        // Cell (0,1) yellow block (2,4)..(3,5).
        for &(x, y) in &[(2u32, 4u32), (3, 4), (2, 5), (3, 5)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0xff, 0x00],
                "cell (0,1) yellow pixel ({x},{y})"
            );
        }
        // Cell (1,1) idx=0: block (4,4)..(5,5) should be background white.
        for &(x, y) in &[(4u32, 4u32), (5, 4), (4, 5), (5, 5)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0xff, 0xff],
                "cell (1,1) idx=0 pixel ({x},{y}) → bg white"
            );
        }
        // Quiet zone outer corners stay palette[0] = white.
        for &(x, y) in &[(0u32, 0u32), (7, 0), (0, 7), (7, 7), (0, 4), (4, 0)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0xff, 0xff],
                "quiet zone pixel ({x},{y}) → palette[0] white"
            );
        }
    }

    /// Stage 11.A8c — pin `render_matrix` (PNG path) cell-to-pixel-block
    /// expansion. Mirrors the SVG render_matrix test but verifies the
    /// raster path: each set cell becomes a `scale × scale` block of
    /// foreground pixels.
    ///
    /// Setup: BitMatrix 2×1 with cells [T, F], scale=3, quiet=1.
    ///   width  = (2 + 2)*3 = 12
    ///   height = (1 + 2)*3 = 9
    ///   cell (0,0) T → 3×3 black block at (3,3)..(5,5)
    ///   cell (1,0) F → background-coloured (white)
    ///
    /// Mutations caught:
    ///   * `matrix.get(x, y)` → `get(y, x)` transposes — would write
    ///     the block at (3,3) only if cell (0,0) is T regardless;
    ///     pinned by the cell-(1,0)=F leaving (6,4) white.
    ///   * `(quiet + x as u32) * scale` shift formula — px wrong.
    ///   * `(quiet + y as u32) * scale` — py wrong.
    ///   * `for dy in 0..scale` / `dx in 0..scale` boundary — would
    ///     leave a corner pixel un-filled or fill one beyond.
    ///   * `width / height` viewport formula.
    ///   * `opts.background` / `opts.foreground` choice — checked at
    ///     the quiet-zone corner (must be background = white).
    #[test]
    fn render_matrix_png_pins_cell_block_expansion() {
        use crate::encoding::BitMatrix;
        let mut bm = BitMatrix::new(2, 1);
        bm.set(0, 0, true);
        // (1, 0) stays false (background).

        let opts = Options {
            scale: 3,
            quiet_zone: 1,
            ..Options::default()
        };
        let png_bytes = render_matrix(&bm, &opts).expect("PNG should encode");
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 12, "width = (2+2*1)*3 = 12");
        assert_eq!(img.height(), 9, "height = (1+2*1)*3 = 9");

        // Cell (0,0) is set: black 3×3 block from (3,3) to (5,5)
        // inclusive. Probe all 4 corners of the block.
        for &(x, y) in &[(3u32, 3u32), (5, 3), (3, 5), (5, 5), (4, 4)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0x00, 0x00],
                "cell (0,0) pixel ({x},{y}) should be foreground black"
            );
        }
        // Just OUTSIDE the (0,0) block (e.g. (2,4) in quiet zone or
        // (6,4) in the F-cell territory) must be background white.
        assert_eq!(
            img.get_pixel(2, 4).0,
            [0xff, 0xff, 0xff],
            "quiet-zone pixel (2,4) should be background white"
        );
        assert_eq!(
            img.get_pixel(6, 4).0,
            [0xff, 0xff, 0xff],
            "F-cell (1,0) territory pixel (6,4) should be background"
        );
        // Quiet zone corners — all background.
        assert_eq!(img.get_pixel(0, 0).0, [0xff, 0xff, 0xff], "(0,0) corner");
        assert_eq!(img.get_pixel(11, 8).0, [0xff, 0xff, 0xff], "(11,8) corner");
    }

    /// Stage 11.A8c — pin `render_linear` (PNG path) bar/space pixel
    /// expansion. Mirrors the SVG render_linear test but verifies the
    /// raster path produces bar-width × bar_height-px black columns at
    /// the correct x positions.
    ///
    /// Setup: bars=[3,1,2], scale=2, quiet=1, bar_height=4.
    ///   width  = (6 + 2)*2 = 16
    ///   height = 4*2 = 8
    ///   x starts at quiet*scale = 2.
    ///   i=0 bar w=3, span=6 → pixels x=2..8, y=0..8 all black.
    ///   i=1 space w=1, span=2 → silent, x → 10.
    ///   i=2 bar w=2, span=4 → pixels x=10..14, y=0..8 all black.
    ///
    /// Mutations caught:
    ///   * `i % 2 == 0` flip — would fill the space at x=8..10 black.
    ///   * `w > 0` guard removal.
    ///   * `x += span` accumulator — wrong x for bar 2.
    ///   * `for py in 0..height` boundary — top/bottom row would skip.
    ///   * `for px in x..x+span` boundary — leftmost/rightmost column
    ///     of each bar would skip.
    ///   * `bar_height.max(1)` floor.
    ///   * Foreground/background swap.
    #[test]
    fn render_linear_png_pins_bar_pixel_blocks() {
        use crate::encoding::LinearPattern;
        let pattern = LinearPattern {
            bars: vec![3, 1, 2],
            text: None,
        };
        let opts = Options {
            scale: 2,
            quiet_zone: 1,
            bar_height: 4,
            include_text: false,
            ..Options::default()
        };
        let png_bytes = render_linear(&pattern, &opts).expect("PNG should encode");
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 16, "(6+2)*2");
        assert_eq!(img.height(), 8, "4*2");

        // Bar 0 corners + middle: x=2..8, y=0..8.
        for &(x, y) in &[(2u32, 0u32), (7, 0), (2, 7), (7, 7), (4, 4)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0x00, 0x00],
                "bar 0 pixel ({x},{y}) should be black"
            );
        }
        // Bar 2 corners + middle: x=10..14, y=0..8.
        for &(x, y) in &[(10u32, 0u32), (13, 0), (10, 7), (13, 7), (11, 4)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0x00, 0x00],
                "bar 2 pixel ({x},{y}) should be black"
            );
        }
        // Space region (x=8..10) must be background white.
        for &(x, y) in &[(8u32, 0u32), (8, 7), (9, 0), (9, 7), (9, 4)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0xff, 0xff],
                "space pixel ({x},{y}) should be white (i%2=1 silent)"
            );
        }
        // Quiet zone left (x=0..2) and right (x=14..16) must be white.
        for &(x, y) in &[(0u32, 0u32), (1, 4), (14, 0), (15, 7)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0xff, 0xff, 0xff],
                "quiet zone pixel ({x},{y}) should be white"
            );
        }
    }

    /// Stage 11.A8c — pin `render_postal4` (PNG path) per-bar pixel
    /// fill. Mirrors the SVG render_postal4 test (each of the 4
    /// Bar4State variants emits a distinct (y_start, y_end) column).
    ///
    /// Setup: bars=[Full, Ascender, Descender, Tracker], scale=1,
    /// quiet=0. width=7, height=8.
    ///   bar 0 Full      → x=0, y=0..8  (column 0, 8 black pixels)
    ///   bar 1 Ascender  → x=2, y=0..5  (column 2, 5 black pixels)
    ///   bar 2 Descender → x=4, y=3..8  (column 4, 5 black pixels)
    ///   bar 3 Tracker   → x=6, y=3..5  (column 6, 2 black pixels)
    /// Gaps at x=1, 3, 5 stay white.
    ///
    /// Mutations caught:
    ///   * `match bar` arm swaps (Ascender ↔ Descender etc.) change
    ///     which y-band fills.
    ///   * `total_h_modules = 8`, `top_pad = 3`, `bot_pad = 3`
    ///     constants — every pixel position shifts.
    ///   * `bar_width = 1`, `gap = 1` and the
    ///     `i * (bar_width + gap)` x-formula — wrong x for each bar.
    ///   * `width_modules = 2*quiet + bar*n + gap*(n-1)` — viewport.
    ///   * `for py in y0..y1` boundary — top/bottom row would skip.
    #[test]
    fn render_postal4_png_pins_per_bar_pixel_columns() {
        use crate::encoding::{Bar4State, Postal4Pattern};
        let pattern = Postal4Pattern {
            bars: vec![
                Bar4State::Full,
                Bar4State::Ascender,
                Bar4State::Descender,
                Bar4State::Tracker,
            ],
            text: None,
        };
        let opts = Options {
            scale: 1,
            quiet_zone: 0,
            ..Options::default()
        };
        let png_bytes = render_postal4(&pattern, &opts).expect("PNG should encode");
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 7, "width = 0+1*4+1*3 = 7");
        assert_eq!(img.height(), 8, "total_h_modules = 8");

        // Bar 0 Full at x=0: every y∈0..8 black.
        for y in 0..8 {
            assert_eq!(
                img.get_pixel(0, y).0,
                [0x00, 0x00, 0x00],
                "Full bar (0,{y}) should be black"
            );
        }
        // Bar 1 Ascender at x=2: y∈0..5 black, y∈5..8 white.
        for y in 0..5 {
            assert_eq!(
                img.get_pixel(2, y).0,
                [0x00, 0x00, 0x00],
                "Ascender (2,{y}) should be black (y<5)"
            );
        }
        for y in 5..8 {
            assert_eq!(
                img.get_pixel(2, y).0,
                [0xff, 0xff, 0xff],
                "Ascender (2,{y}) should be white (y≥5, bot_pad)"
            );
        }
        // Bar 2 Descender at x=4: y∈0..3 white, y∈3..8 black.
        for y in 0..3 {
            assert_eq!(
                img.get_pixel(4, y).0,
                [0xff, 0xff, 0xff],
                "Descender (4,{y}) should be white (y<3, top_pad)"
            );
        }
        for y in 3..8 {
            assert_eq!(
                img.get_pixel(4, y).0,
                [0x00, 0x00, 0x00],
                "Descender (4,{y}) should be black"
            );
        }
        // Bar 3 Tracker at x=6: y∈3..5 black, rest white.
        for y in 0..3 {
            assert_eq!(
                img.get_pixel(6, y).0,
                [0xff, 0xff, 0xff],
                "Tracker (6,{y}) should be white (y<3)"
            );
        }
        for y in 3..5 {
            assert_eq!(
                img.get_pixel(6, y).0,
                [0x00, 0x00, 0x00],
                "Tracker (6,{y}) should be black"
            );
        }
        for y in 5..8 {
            assert_eq!(
                img.get_pixel(6, y).0,
                [0xff, 0xff, 0xff],
                "Tracker (6,{y}) should be white (y≥5)"
            );
        }
        // Gap columns x=1, 3, 5 always white.
        for &x in &[1u32, 3, 5] {
            for y in 0..8 {
                assert_eq!(
                    img.get_pixel(x, y).0,
                    [0xff, 0xff, 0xff],
                    "gap column ({x},{y}) should be white"
                );
            }
        }
    }

    /// Stage 11.A8c — pin `render_stacked` (PNG path) row stacking +
    /// gap row. Mirrors the SVG render_stacked test for the raster
    /// pipeline.
    ///
    /// Setup: 2 rows, scale=1, quiet=0, bar_height=8.
    ///   Row 0: bars [2, 1, 1] (bar=2 at x=0..2, space, bar=1 at x=3..4)
    ///   Row 1: bars [1, 1, 2] (bar=1 at x=0..1, space, bar=2 at x=2..4)
    /// width = 4, height = 2*8 + 1 = 17.
    ///   Row 0 band: y=0..8.
    ///   Gap row:   y=8 (1 pixel high, always background).
    ///   Row 1 band: y=9..17.
    ///
    /// Mutations caught:
    ///   * `gap = 1u32` → `gap = 0`: row 1 starts at y=8 (no gap row).
    ///     The (0, 8) gap-row assertion catches this.
    ///   * `(row_idx as u32) * (row_height + gap)` formula — any shift
    ///     to row 1's y0 detected by the y=9 assertion.
    ///   * `i % 2 == 0` bar/space flip — would fill spaces; the
    ///     space at (2, 4) being white pins this.
    ///   * `row_y1 = row_y0 + row_height * scale` boundary — y=7
    ///     bottom row of row 0 would be missing or gap row would
    ///     bleed in.
    ///   * `for py in row_y0..row_y1` boundary — top/bottom row of
    ///     each band would skip.
    ///   * `for (i, &w)` ordering: dropping a bar entry would change
    ///     pixel counts (caught implicitly by the per-bar probes).
    #[test]
    fn render_stacked_png_pins_rows_and_gap() {
        use crate::encoding::{LinearPattern, StackedPattern};
        let row0 = LinearPattern {
            bars: vec![2, 1, 1],
            text: None,
        };
        let row1 = LinearPattern {
            bars: vec![1, 1, 2],
            text: None,
        };
        let pattern = StackedPattern::new(vec![row0, row1], None).expect("equal widths");
        let opts = Options {
            scale: 1,
            quiet_zone: 0,
            bar_height: 8,
            ..Options::default()
        };
        let png_bytes = render_stacked(&pattern, &opts).expect("PNG should encode");
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 4, "row width = 4 modules");
        assert_eq!(img.height(), 17, "2*8 + 1 = 17");

        // Row 0 bar 0 (x=0..2, y=0..8): probe corners + middle.
        for &(x, y) in &[(0u32, 0u32), (1, 0), (0, 7), (1, 7), (0, 4)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0x00, 0x00],
                "row 0 bar 0 pixel ({x},{y}) black"
            );
        }
        // Row 0 bar 2 (x=3, y=0..8).
        for y in 0..8 {
            assert_eq!(
                img.get_pixel(3, y).0,
                [0x00, 0x00, 0x00],
                "row 0 bar 2 pixel (3,{y}) black"
            );
        }
        // Row 0 space at x=2 (between bars): white.
        for y in 0..8 {
            assert_eq!(
                img.get_pixel(2, y).0,
                [0xff, 0xff, 0xff],
                "row 0 space (2,{y}) white (i%2 flip would fill)"
            );
        }
        // Gap row y=8: ALL x must be white (no bars span the gap).
        for x in 0..4 {
            assert_eq!(
                img.get_pixel(x, 8).0,
                [0xff, 0xff, 0xff],
                "gap row pixel ({x},8) white (gap=1 module high)"
            );
        }
        // Row 1 bar 0 (x=0, y=9..17): all black.
        for y in 9..17 {
            assert_eq!(
                img.get_pixel(0, y).0,
                [0x00, 0x00, 0x00],
                "row 1 bar 0 pixel (0,{y}) black"
            );
        }
        // Row 1 bar 2 (x=2..4, y=9..17): corners + middle.
        for &(x, y) in &[(2u32, 9u32), (3, 9), (2, 16), (3, 16), (2, 12)] {
            assert_eq!(
                img.get_pixel(x, y).0,
                [0x00, 0x00, 0x00],
                "row 1 bar 2 pixel ({x},{y}) black"
            );
        }
        // Row 1 space at x=1: white.
        for y in 9..17 {
            assert_eq!(
                img.get_pixel(1, y).0,
                [0xff, 0xff, 0xff],
                "row 1 space (1,{y}) white"
            );
        }
    }

    /// Stage 11.A8c — pin `render_dots` (PNG path) circle rasteriser.
    ///
    /// Setup: 2×1 DotMatrix with dots at (0,0) only; scale=5, quiet=0.
    ///   width  = 2*5 = 10, height = 1*5 = 5.
    ///   radius = (5.0 * 0.4).max(0.5) = 2.0 → r² = 4.
    ///   Dot (0,0) center cx=2.5, cy=2.5.
    ///   Pixel (px,py) is filled iff (px+0.5-2.5)² + (py+0.5-2.5)² ≤ 4
    ///   i.e. (px-2)² + (py-2)² ≤ 4.
    ///
    /// Resulting pattern (B = black filled, . = white background) in
    /// the 5×5 sub-region covering dot (0,0):
    ///   . . B . .       (px=2,py=0 only black)
    ///   . B B B .       (px=1..3)
    ///   B B B B B       (px=0..4, full row)
    ///   . B B B .
    ///   . . B . .
    /// 13 black pixels in a plus shape; 12 white.
    ///
    /// Mutations caught:
    ///   * `radius = (scale * 0.4).max(0.5)` — different ratio shrinks
    ///     or grows the diamond visibly.
    ///   * `r2 = radius * radius` — squared comparison breaks if the
    ///     `* radius` is dropped (linear instead of squared).
    ///   * `cx = (quiet + x) * scale + scale * 0.5` center math.
    ///   * `dx * dx + dy * dy <= r2` — `<=` (not `<`) matters at
    ///     boundary pixels (0,2), (2,0), (2,4), (4,2) where distance²=4.
    ///   * `dx = px + 0.5 - cx` sub-pixel sampling — without the +0.5
    ///     the diamond shifts by half a pixel.
    ///   * `dots.get(x, y)` membership check — F-cell at (1,0) must
    ///     leave the right half (x=5..10) entirely background.
    #[test]
    fn render_dots_png_pins_circle_rasteriser() {
        use crate::encoding::DotMatrix;
        let mut dots = DotMatrix::new(2, 1);
        dots.set(0, 0, true);
        // (1, 0) stays false → right half must be all background.

        let opts = Options {
            scale: 5,
            quiet_zone: 0,
            ..Options::default()
        };
        let png_bytes = render_dots(&dots, &opts).expect("PNG should encode");
        assert_eq!(&png_bytes[..4], b"\x89PNG");

        let img = ImageReader::new(Cursor::new(&png_bytes))
            .with_guessed_format()
            .expect("PNG reader")
            .decode()
            .expect("PNG decode")
            .to_rgb8();
        assert_eq!(img.width(), 10);
        assert_eq!(img.height(), 5);

        // Expected diamond pattern in the 5×5 left half.
        // Computed: (px-2)² + (py-2)² ≤ 4.
        let want: [[u8; 5]; 5] = [
            [0, 0, 1, 0, 0],
            [0, 1, 1, 1, 0],
            [1, 1, 1, 1, 1],
            [0, 1, 1, 1, 0],
            [0, 0, 1, 0, 0],
        ];
        for py in 0..5u32 {
            for px in 0..5u32 {
                let want_black = want[py as usize][px as usize] == 1;
                let pixel = img.get_pixel(px, py).0;
                if want_black {
                    assert_eq!(
                        pixel,
                        [0x00, 0x00, 0x00],
                        "diamond pixel ({px},{py}) should be black"
                    );
                } else {
                    assert_eq!(
                        pixel,
                        [0xff, 0xff, 0xff],
                        "diamond pixel ({px},{py}) should be white"
                    );
                }
            }
        }
        // Right half (x=5..10) covers F-cell (1,0). MUST be all white.
        for py in 0..5u32 {
            for px in 5..10u32 {
                assert_eq!(
                    img.get_pixel(px, py).0,
                    [0xff, 0xff, 0xff],
                    "F-cell area pixel ({px},{py}) must be white"
                );
            }
        }
    }
}
