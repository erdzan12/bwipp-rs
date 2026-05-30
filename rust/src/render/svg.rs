//! SVG renderer.

use std::fmt::Write;

use crate::encoding::{
    Bar4State, BitMatrix, DotMatrix, LinearPattern, Postal4Pattern, StackedPattern,
};
use crate::options::Options;

pub(crate) fn render_linear(pattern: &LinearPattern, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let bar_height = opts.bar_height.max(1);
    let text_height = if pattern.text.is_some() && opts.include_text {
        10
    } else {
        0
    };

    let width_modules = pattern.total_width() + 2 * quiet;
    let height_modules = bar_height + text_height;
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#,
    ).unwrap();

    // Background.
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background),
    )
    .unwrap();

    // Bars.
    let mut x: u32 = quiet * scale;
    for (i, &width) in pattern.bars.iter().enumerate() {
        let is_bar = i % 2 == 0;
        let w = u32::from(width) * scale;
        if is_bar && width > 0 {
            writeln!(
                svg,
                r#"<rect x="{x}" y="0" width="{w}" height="{}" fill="{}"/>"#,
                bar_height * scale,
                rgb(opts.foreground),
            )
            .unwrap();
        }
        x += w;
    }

    // Text.
    if let Some(text) = &pattern.text {
        if opts.include_text {
            let font_size = text_height * scale;
            writeln!(
                svg,
                r#"<text x="{}" y="{}" font-family="monospace" font-size="{}" text-anchor="middle" fill="{}">{}</text>"#,
                width_px / 2,
                height_px - scale,
                font_size,
                rgb(opts.foreground),
                escape_xml(text),
            ).unwrap();
        }
    }

    writeln!(svg, "</svg>").unwrap();
    svg
}

/// Render a [`ColorMatrix`] as a per-cell coloured rect grid.
///
/// Each cell carries a palette index 0..=7 into the matrix's
/// 8-entry RGB palette. Index 0 is the background (typically
/// white) — no rect is emitted for those cells so the SVG
/// background `<rect>` shows through. Coalescing runs of the same
/// non-background colour in a row keeps the output compact for
/// Ultracode's row-of-N-same-colour-cells layout.
pub(crate) fn render_color_matrix(matrix: &crate::encoding::ColorMatrix, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width_modules = matrix.width() as u32 + 2 * quiet;
    let height_modules = matrix.height() as u32 + 2 * quiet;
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;
    let palette = matrix.palette();

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#,
    ).unwrap();
    // Background: use the matrix's palette[0] rather than opts.background
    // because a ColorMatrix's palette is the source of truth.
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        palette[0].to_css_hex(),
    )
    .unwrap();

    // Per-row run-length coalesce: emit one rect for each run of
    // same-palette-index cells (skipping background runs).
    for y in 0..matrix.height() {
        let mut x = 0;
        while x < matrix.width() {
            let idx = matrix.get(x, y);
            if idx == 0 {
                x += 1;
                continue;
            }
            let mut run = 1;
            while x + run < matrix.width() && matrix.get(x + run, y) == idx {
                run += 1;
            }
            let px = (quiet + x as u32) * scale;
            let py = (quiet + y as u32) * scale;
            let pw = run as u32 * scale;
            writeln!(
                svg,
                r#"<rect x="{px}" y="{py}" width="{pw}" height="{scale}" fill="{}"/>"#,
                palette[idx as usize].to_css_hex(),
            )
            .unwrap();
            x += run;
        }
    }

    writeln!(svg, "</svg>").unwrap();
    svg
}

pub(crate) fn render_matrix(matrix: &BitMatrix, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width_modules = matrix.width() as u32 + 2 * quiet;
    let height_modules = matrix.height() as u32 + 2 * quiet;
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#,
    ).unwrap();
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background),
    )
    .unwrap();

    // Coalesce consecutive black modules in a row into a single rect.
    for y in 0..matrix.height() {
        let mut x = 0;
        while x < matrix.width() {
            if matrix.get(x, y) {
                let mut run = 1;
                while x + run < matrix.width() && matrix.get(x + run, y) {
                    run += 1;
                }
                let px = (quiet + x as u32) * scale;
                let py = (quiet + y as u32) * scale;
                let pw = run as u32 * scale;
                writeln!(
                    svg,
                    r#"<rect x="{px}" y="{py}" width="{pw}" height="{scale}" fill="{}"/>"#,
                    rgb(opts.foreground),
                )
                .unwrap();
                x += run;
            } else {
                x += 1;
            }
        }
    }

    writeln!(svg, "</svg>").unwrap();
    svg
}

/// Render a [`DotMatrix`] as round dots on a background rectangle.
/// Each `true` cell becomes a filled `<circle>` centred in its
/// module-sized cell; its diameter is `0.8 * scale` pixels so dots
/// don't quite touch their neighbours (the DotCode visual style).
pub(crate) fn render_dots(dots: &DotMatrix, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let width_modules = dots.width() as u32 + 2 * quiet;
    let height_modules = dots.height() as u32 + 2 * quiet;
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;
    // Render dots at 80% of cell size so adjacent dots have visible
    // gaps. Keep ≥ 1 px radius even at scale=1.
    let radius = ((scale as f32) * 0.4).max(0.5);

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#,
    )
    .unwrap();
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background),
    )
    .unwrap();
    for y in 0..dots.height() {
        for x in 0..dots.width() {
            if dots.get(x, y) {
                let cx = (quiet + x as u32) * scale + scale / 2;
                let cy = (quiet + y as u32) * scale + scale / 2;
                writeln!(
                    svg,
                    r#"<circle cx="{cx}" cy="{cy}" r="{radius}" fill="{}"/>"#,
                    rgb(opts.foreground),
                )
                .unwrap();
            }
        }
    }
    writeln!(svg, "</svg>").unwrap();
    svg
}

/// Render a [`MaxiCodeSymbol`] as hexagonal modules in a hex-packed
/// grid. Odd rows are physically offset by half a module to the
/// right (the visual hex-stagger), and the vertical step is `sqrt(3)/2`
/// times the horizontal step to keep hex packing tight.
///
/// Each "on" cell becomes a filled `<polygon>` hexagon. Pointy-top
/// orientation (vertices at 30° steps starting at 0°/north).
pub(crate) fn render_hex(
    sym: &crate::symbology::maxicode::MaxiCodeSymbol,
    opts: &Options,
) -> String {
    let scale = opts.scale.max(1) as f32;
    let quiet = opts.quiet_zone as f32;
    let cols = sym.cols() as f32;
    let rows = sym.rows() as f32;
    // Horizontal step = scale; vertical step ≈ 0.866 × scale (hex packing).
    let h_step = scale;
    let v_step = scale * 3f32.sqrt() / 2.0;
    let width_px = ((cols + 0.5) * h_step + 2.0 * quiet * scale).ceil() as u32;
    let height_px = ((rows - 1.0) * v_step + scale + 2.0 * quiet * scale).ceil() as u32;
    // Hex circumradius = scale/sqrt(3). Pointy-top vertices at angles
    // 30°, 90°, 150°, 210°, 270°, 330° (relative to centre).
    let r = scale / 3f32.sqrt() * 0.95; // 5% inset so adjacent hexes don't touch.

    let mut svg = String::new();
    use std::fmt::Write;
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#,
    )
    .unwrap();
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background),
    )
    .unwrap();
    let fg = rgb(opts.foreground);
    for row in 0..sym.rows() {
        for col in 0..sym.cols() {
            if !sym.is_on(row, col) {
                continue;
            }
            let x_offset = if row % 2 == 1 { h_step / 2.0 } else { 0.0 };
            let cx = quiet * scale + col as f32 * h_step + x_offset + h_step / 2.0;
            let cy = quiet * scale + row as f32 * v_step + scale / 2.0;
            // 6 pointy-top vertices.
            let mut points = String::new();
            for i in 0..6 {
                let angle = std::f32::consts::PI / 3.0 * (i as f32) + std::f32::consts::PI / 6.0;
                let x = cx + r * angle.cos();
                let y = cy - r * angle.sin();
                if i > 0 {
                    points.push(' ');
                }
                let _ = write!(points, "{x:.2},{y:.2}");
            }
            writeln!(svg, r#"<polygon points="{points}" fill="{fg}"/>"#).unwrap();
        }
    }
    writeln!(svg, "</svg>").unwrap();
    svg
}

pub(crate) fn render_postal4(pattern: &Postal4Pattern, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;

    // Geometry derived from the BWIPP `renpostal` block:
    //   * each bar is 1 module wide, separated by a 1-module gap;
    //   * full-height bar = full symbol height (8 modules by default);
    //   * ascender/descender = top or bottom 5/8 of the height;
    //   * tracker = central 2/8.
    let bar_width: u32 = 1;
    let gap: u32 = 1;
    let total_h_modules: u32 = 8;
    let top_pad_modules: u32 = 3; // ascender top section
    let bot_pad_modules: u32 = 3; // descender bottom section

    let n = pattern.bars.len() as u32;
    let width_modules = if n == 0 {
        2 * quiet
    } else {
        2 * quiet + (bar_width * n) + (gap * n.saturating_sub(1))
    };
    let text_height_modules = if pattern.text.is_some() && opts.include_text {
        2
    } else {
        0
    };
    let height_modules = total_h_modules + text_height_modules;
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#
    )
    .unwrap();
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background)
    )
    .unwrap();

    let fg = rgb(opts.foreground);
    for (i, bar) in pattern.bars.iter().enumerate() {
        let x = quiet * scale + (i as u32) * (bar_width + gap) * scale;
        let bar_px = bar_width * scale;
        let (y_start_mod, y_end_mod): (u32, u32) = match bar {
            Bar4State::Full => (0, total_h_modules),
            Bar4State::Ascender => (0, total_h_modules - bot_pad_modules),
            Bar4State::Descender => (top_pad_modules, total_h_modules),
            Bar4State::Tracker => (top_pad_modules, total_h_modules - bot_pad_modules),
        };
        let y = y_start_mod * scale;
        let h = (y_end_mod - y_start_mod) * scale;
        writeln!(
            svg,
            r#"<rect x="{x}" y="{y}" width="{bar_px}" height="{h}" fill="{fg}"/>"#
        )
        .unwrap();
    }

    if let Some(text) = &pattern.text {
        if opts.include_text {
            let font_size = text_height_modules.max(1) * scale;
            writeln!(
                svg,
                r#"<text x="{}" y="{}" font-family="monospace" font-size="{}" text-anchor="middle" fill="{fg}">{}</text>"#,
                width_px / 2,
                height_px - scale,
                font_size,
                escape_xml(text),
            )
            .unwrap();
        }
    }

    writeln!(svg, "</svg>").unwrap();
    svg
}

pub(crate) fn render_stacked(pattern: &StackedPattern, opts: &Options) -> String {
    let scale = opts.scale.max(1);
    let quiet = opts.quiet_zone;
    let row_height = opts.bar_height.max(1).max(8); // each row at least 8 modules tall
    let gap = 1u32; // 1-module spacer between rows
    let width_modules = pattern.width() + 2 * quiet;
    let n_rows = pattern.rows.len() as u32;
    let height_modules = if n_rows == 0 {
        2 * quiet
    } else {
        n_rows * row_height + (n_rows - 1) * gap + 2 * quiet
    };
    let width_px = width_modules * scale;
    let height_px = height_modules * scale;

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width_px} {height_px}" width="{width_px}" height="{height_px}">"#
    ).unwrap();
    writeln!(
        svg,
        r#"<rect width="{width_px}" height="{height_px}" fill="{}"/>"#,
        rgb(opts.background)
    )
    .unwrap();

    let fg = rgb(opts.foreground);
    for (row_idx, row) in pattern.rows.iter().enumerate() {
        let row_y = (quiet + (row_idx as u32) * (row_height + gap)) * scale;
        let row_h = row_height * scale;
        let mut x: u32 = quiet * scale;
        for (i, &width) in row.bars.iter().enumerate() {
            let is_bar = i % 2 == 0;
            let w = u32::from(width) * scale;
            if is_bar && width > 0 {
                writeln!(
                    svg,
                    r#"<rect x="{x}" y="{row_y}" width="{w}" height="{row_h}" fill="{fg}"/>"#,
                )
                .unwrap();
            }
            x += w;
        }
    }
    writeln!(svg, "</svg>").unwrap();
    svg
}

fn rgb(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{ColorMatrix, Rgb8};

    /// Smoke test: a fabricated `ColorMatrix` with one cyan cell and
    /// one black cell renders to an SVG that contains:
    ///   - a `<svg ...>` opener,
    ///   - a background `<rect>` filled with palette[0] (white),
    ///   - one `<rect>` per non-background cell with the matching
    ///     palette colour.
    ///
    /// This pins the colour pipeline's end-to-end SVG path (Stage 4a)
    /// independently of the ultracode encoder, so a future encoder
    /// regression doesn't accidentally mask a renderer regression.
    #[test]
    fn render_color_matrix_emits_palette_fills() {
        let palette: [Rgb8; 8] = [
            Rgb8::new(0xff, 0xff, 0xff), // 0 white (background)
            Rgb8::new(0x00, 0xff, 0xff), // 1 cyan
            Rgb8::new(0xff, 0x00, 0xff), // 2 magenta
            Rgb8::new(0xff, 0xff, 0x00), // 3 yellow
            Rgb8::new(0x00, 0xff, 0x00), // 4 green
            Rgb8::new(0x00, 0x00, 0xff), // 5 blue
            Rgb8::new(0xff, 0x00, 0x00), // 6 red
            Rgb8::new(0x00, 0x00, 0x00), // 7 black
        ];
        let mut m = ColorMatrix::new(4, 2, palette);
        m.set(0, 0, 1); // cyan
        m.set(2, 1, 7); // black

        let opts = Options {
            scale: 2,
            quiet_zone: 0,
            ..Options::default()
        };
        let svg = render_color_matrix(&m, &opts);

        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
        // Background rect uses palette[0] (white), not opts.background.
        assert!(svg.contains(r##"fill="#ffffff""##));
        // Cyan cell rect.
        assert!(
            svg.contains(r##"fill="#00ffff""##),
            "expected cyan fill, got:\n{svg}"
        );
        // Black cell rect.
        assert!(
            svg.contains(r##"fill="#000000""##),
            "expected black fill, got:\n{svg}"
        );
    }

    /// Same-colour cells in a row coalesce into one `<rect>` (per-row
    /// run-length compression) — the renderer emits two `<rect>`s for
    /// a row with [white, cyan, cyan, white], not three.
    #[test]
    fn render_color_matrix_coalesces_runs_in_a_row() {
        let palette: [Rgb8; 8] = [Rgb8::new(0xff, 0xff, 0xff); 8];
        let mut p = palette;
        p[1] = Rgb8::new(0x00, 0xff, 0xff);
        let mut m = ColorMatrix::new(4, 1, p);
        m.set(1, 0, 1);
        m.set(2, 0, 1);

        let opts = Options {
            scale: 1,
            quiet_zone: 0,
            ..Options::default()
        };
        let svg = render_color_matrix(&m, &opts);
        // 1 background <rect> + 1 coalesced cyan <rect> = 2 rects total.
        let n_rects = svg.matches("<rect").count();
        assert_eq!(
            n_rects, 2,
            "expected 2 rects (bg + coalesced cyan), got {n_rects}:\n{svg}"
        );
    }

    /// Stage 11.A8c — pin `rgb` byte-to-hex formatting. The helper is
    /// used by every render path that emits a color (`#rrggbb`). It's
    /// 1 line so the mutation surface is small but distinct:
    ///   - `{:02x}` could become `{:02X}` (uppercase) — visible
    ///   - `c[0], c[1], c[2]` could swap (e.g. BGR order)
    ///   - the leading `#` could be dropped
    #[test]
    fn rgb_formats_bytes_as_lowercase_hex() {
        // All-zero → "#000000".
        assert_eq!(rgb([0, 0, 0]), "#000000");
        // All-max → "#ffffff" (lowercase).
        assert_eq!(rgb([255, 255, 255]), "#ffffff");
        // Distinct R/G/B values to catch component-order swaps.
        // [0x12, 0x34, 0x56] → "#123456" (R/G/B in order).
        assert_eq!(
            rgb([0x12, 0x34, 0x56]),
            "#123456",
            "byte order must be R,G,B (catches BGR / GRB swaps)"
        );
        // Single-digit hex values get the `02` padding.
        assert_eq!(rgb([1, 2, 3]), "#010203", "needs `02` zero-padding");
        // High-bit boundary: 16 → "10", 17 → "11".
        assert_eq!(rgb([16, 17, 0]), "#101100");
    }

    /// Stage 11.A8c — pin `escape_xml`'s 5-entity replacement chain
    /// directly. Each special char maps to one entity:
    ///   `&` → `&amp;`,  `<` → `&lt;`,  `>` → `&gt;`,
    ///   `"` → `&quot;`, `'` → `&apos;`.
    ///
    /// The mutations to catch:
    ///   - Delete any single `.replace(...)` link → that character
    ///     passes through unescaped.
    ///   - Swap the entity name (e.g. `&amp;` → `&xxx;`) → invalid XML.
    ///   - Reorder the chain so `&amp;` runs after `&` → escapes the
    ///     `&` inside `&lt;` etc. (double-escape bug).
    #[test]
    fn escape_xml_replaces_all_five_xml_special_characters() {
        // Each character on its own.
        assert_eq!(escape_xml("&"), "&amp;");
        assert_eq!(escape_xml("<"), "&lt;");
        assert_eq!(escape_xml(">"), "&gt;");
        assert_eq!(escape_xml("\""), "&quot;");
        assert_eq!(escape_xml("'"), "&apos;");
        // Combined input → all 5 escapes.
        assert_eq!(escape_xml("&<>\"'"), "&amp;&lt;&gt;&quot;&apos;");
        // Plain text untouched.
        assert_eq!(escape_xml("HELLO 123"), "HELLO 123");
        assert_eq!(escape_xml(""), "");
        // CRITICAL: `&` MUST be the first replacement so the entity
        // text it inserts (`&amp;`) doesn't get re-escaped by the
        // `<` / `>` / etc. passes. Inputs like "a<b" should produce
        // "a&lt;b" (not "a&amp;lt;b").
        assert_eq!(
            escape_xml("a<b"),
            "a&lt;b",
            "reordering the chain so `<` runs before `&` would \
             double-escape the inserted `&amp;`"
        );
        // Mixed real-world payload-ish.
        assert_eq!(
            escape_xml("Q&A: <foo bar=\"x\">"),
            "Q&amp;A: &lt;foo bar=&quot;x&quot;&gt;"
        );
    }

    /// Stage 11.A8c — pin `render_postal4` geometry for each of the
    /// four bar states. Targets:
    ///
    ///   * The `match bar` arm assignment: each Bar4State maps to a
    ///     distinct (y_start, y_end) pair. Mutations that swap two
    ///     arms (e.g. Ascender ↔ Descender) change the y/h pair.
    ///   * `total_h_modules = 8`, `top_pad_modules = 3`,
    ///     `bot_pad_modules = 3` constants — different values change
    ///     every height.
    ///   * `bar_width = 1`, `gap = 1` and the x-position formula
    ///     `quiet * scale + i * (bar_width + gap) * scale` — mutations
    ///     would shift x by ±1 or ±2 modules.
    ///   * width_modules: `2*quiet + bar*n + gap*(n-1)` — for n=4,
    ///     quiet=0, scale=1: 0 + 4 + 3 = 7.
    ///
    /// With scale=1, quiet_zone=0, no text:
    ///   bar 0 Full      → x=0, y=0, h=8
    ///   bar 1 Ascender  → x=2, y=0, h=5  (8 - bot_pad=3)
    ///   bar 2 Descender → x=4, y=3, h=5  (8 - top_pad=3)
    ///   bar 3 Tracker   → x=6, y=3, h=2  (top_pad..8-bot_pad)
    ///
    /// Each is a distinct (x, y, h) triple — swapping any pair of arms
    /// or shifting a constant would change at least one bar's rect.
    #[test]
    fn render_postal4_each_bar_state_emits_distinct_rect_geometry() {
        use crate::encoding::Bar4State;
        use crate::encoding::Postal4Pattern;
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
            include_text: false,
            ..Options::default()
        };
        let svg = render_postal4(&pattern, &opts);
        // Width and height in the viewBox.
        assert!(
            svg.contains(r#"viewBox="0 0 7 8""#),
            "viewBox mismatch:\n{svg}"
        );
        // Background rect for full canvas (white default).
        assert!(
            svg.contains(r##"<rect width="7" height="8" fill="#ffffff"/>"##),
            "background rect missing:\n{svg}"
        );
        // Bar 0 Full: x=0, y=0, h=8.
        assert!(
            svg.contains(r##"<rect x="0" y="0" width="1" height="8" fill="#000000"/>"##),
            "Full bar (x=0 y=0 h=8) missing:\n{svg}"
        );
        // Bar 1 Ascender: x=2, y=0, h=5.
        assert!(
            svg.contains(r##"<rect x="2" y="0" width="1" height="5" fill="#000000"/>"##),
            "Ascender (x=2 y=0 h=5) missing:\n{svg}"
        );
        // Bar 2 Descender: x=4, y=3, h=5.
        assert!(
            svg.contains(r##"<rect x="4" y="3" width="1" height="5" fill="#000000"/>"##),
            "Descender (x=4 y=3 h=5) missing:\n{svg}"
        );
        // Bar 3 Tracker: x=6, y=3, h=2.
        assert!(
            svg.contains(r##"<rect x="6" y="3" width="1" height="2" fill="#000000"/>"##),
            "Tracker (x=6 y=3 h=2) missing:\n{svg}"
        );
        // Total rects: 1 background + 4 bars = 5 (no text).
        let n_rects = svg.matches("<rect").count();
        assert_eq!(n_rects, 5, "expected 5 <rect>s, got {n_rects}:\n{svg}");
    }

    /// Stage 11.A8c — pin `render_stacked` row geometry and bar/space
    /// alternation. Targets:
    ///
    ///   * `i % 2 == 0` bar/space discriminator: even = bar, odd = space.
    ///     Mutations flipping it would emit rects for the spaces instead.
    ///   * `gap = 1u32` row spacer constant.
    ///   * Per-row y-coord `(quiet + row_idx * (row_height + gap)) * scale`
    ///     — mutations to `+`/`*` or dropping `gap` shift row 1 by ±1.
    ///   * `row_height = opts.bar_height.max(1).max(8)` — floor of 8.
    ///     Passing bar_height=4 must still yield row_height=8, so row 1's
    ///     y-coord is 0 + 1 * (8 + 1) = 9, NOT 0 + 1 * (4 + 1) = 5.
    ///   * `width > 0` emit guard (no rect for zero-width bars).
    ///   * `n_rows * row_height + (n_rows - 1) * gap` height formula:
    ///     for n=2, row_height=8, gap=1 → 17 modules.
    ///
    /// Construction:
    ///   Row 0: bars [2, 1, 1] — bar=2, space=1, bar=1 (total_width=4).
    ///   Row 1: bars [1, 1, 2] — bar=1, space=1, bar=2 (total_width=4).
    /// Different bar widths between rows discriminate row geometry.
    #[test]
    fn render_stacked_pins_row_geometry_and_alternation() {
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
        // bar_height=4 is BELOW the floor of 8 → row_height should clamp
        // to 8. Pinning this case kills any mutation that drops or
        // weakens the `.max(8)` floor.
        let opts = Options {
            scale: 1,
            quiet_zone: 0,
            bar_height: 4,
            include_text: false,
            ..Options::default()
        };
        let svg = render_stacked(&pattern, &opts);
        // width=4 modules; height = 2*8 + 1 = 17.
        assert!(
            svg.contains(r#"viewBox="0 0 4 17""#),
            "viewBox should be 4x17 (height=2*8+1, bar_height=4 floor 8):\n{svg}"
        );
        // Background rect.
        assert!(
            svg.contains(r##"<rect width="4" height="17" fill="#ffffff"/>"##),
            "background:\n{svg}"
        );
        // Row 0 bars (y=0, h=8):
        //   i=0 bar w=2: x=0 → rect x=0 y=0 w=2 h=8
        //   i=1 space w=1: no rect, x advances to 3
        //   i=2 bar w=1: x=3 → rect x=3 y=0 w=1 h=8
        assert!(
            svg.contains(r##"<rect x="0" y="0" width="2" height="8" fill="#000000"/>"##),
            "row0 bar0 (x=0 y=0 w=2 h=8):\n{svg}"
        );
        assert!(
            svg.contains(r##"<rect x="3" y="0" width="1" height="8" fill="#000000"/>"##),
            "row0 bar2 (x=3 y=0 w=1 h=8):\n{svg}"
        );
        // Row 1 bars (y=9 = 0 + 1*(8+1), h=8):
        //   i=0 bar w=1: x=0 → rect x=0 y=9 w=1 h=8
        //   i=1 space w=1: no rect, x advances to 2
        //   i=2 bar w=2: x=2 → rect x=2 y=9 w=2 h=8
        assert!(
            svg.contains(r##"<rect x="0" y="9" width="1" height="8" fill="#000000"/>"##),
            "row1 bar0 (x=0 y=9 w=1 h=8) — pins row gap:\n{svg}"
        );
        assert!(
            svg.contains(r##"<rect x="2" y="9" width="2" height="8" fill="#000000"/>"##),
            "row1 bar2 (x=2 y=9 w=2 h=8):\n{svg}"
        );
        // Total rects: 1 background + 4 bar rects (spaces don't emit) = 5.
        let n_rects = svg.matches("<rect").count();
        assert_eq!(
            n_rects, 5,
            "expected 5 rects (1 bg + 4 bars; spaces silent):\n{svg}"
        );
        // Negative: no rect at the SPACE x-positions (which would
        // indicate the bar/space discriminator flipped). Row 0 has
        // its space at x=2 (after bar w=2); that x with y=0 must
        // not appear.
        assert!(
            !svg.contains(r#"<rect x="2" y="0""#),
            "no rect should sit at row0 space x=2 (would mean i%2 flipped):\n{svg}"
        );
    }

    /// Stage 11.A8c — pin `render_dots` cell-to-circle math + radius
    /// floor. Targets the per-dot center coordinates and the radius
    /// floor of 0.5 at small scales.
    ///
    /// Math (with quiet=1, scale=2):
    ///   * width_px  = (3 + 2*1) * 2 = 10
    ///   * height_px = (2 + 2*1) * 2 = 8
    ///   * radius    = (2.0 * 0.4).max(0.5) = 0.8
    ///   * cell (0,0) → cx = (1+0)*2 + 2/2 = 3, cy = 3
    ///   * cell (2,1) → cx = (1+2)*2 + 2/2 = 7, cy = (1+1)*2 + 2/2 = 5
    ///
    /// Mutations caught:
    ///   * `quiet + x` → `quiet - x` or `quiet * x` shifts cx
    ///   * `scale / 2` integer offset (here 1) drops or changes
    ///   * `(scale as f32) * 0.4` ratio changes radius
    ///   * `.max(0.5)` floor removal — the scale=1 sub-test pins this:
    ///       radius = (1*0.4).max(0.5) = 0.5 (NOT 0.4)
    ///   * `dots.get(x, y)` membership flip emits extras / drops
    #[test]
    fn render_dots_pins_circle_centers_and_radius_floor() {
        use crate::encoding::DotMatrix;
        // Part 1: 3x2 grid, quiet=1, scale=2 → pins center math.
        let mut dots = DotMatrix::new(3, 2);
        dots.set(0, 0, true);
        dots.set(2, 1, true);
        let opts = Options {
            scale: 2,
            quiet_zone: 1,
            ..Options::default()
        };
        let svg = render_dots(&dots, &opts);
        assert!(
            svg.contains(r#"viewBox="0 0 10 8""#),
            "viewBox (3+2)*2 x (2+2)*2:\n{svg}"
        );
        // Cell (0,0) — should appear as a circle at (3,3) r=0.8.
        assert!(
            svg.contains(r##"<circle cx="3" cy="3" r="0.8" fill="#000000"/>"##),
            "dot (0,0) → cx=3 cy=3 r=0.8:\n{svg}"
        );
        // Cell (2,1) — should appear at (7, 5).
        assert!(
            svg.contains(r##"<circle cx="7" cy="5" r="0.8" fill="#000000"/>"##),
            "dot (2,1) → cx=7 cy=5 r=0.8:\n{svg}"
        );
        // Exactly 2 circles (one per set dot).
        let n_circles = svg.matches("<circle").count();
        assert_eq!(n_circles, 2, "expected 2 circles:\n{svg}");

        // Part 2: pin the radius `.max(0.5)` floor at scale=1.
        // (1.0 * 0.4) = 0.4 which is BELOW 0.5, so the .max(0.5) must
        // raise it. If the floor were removed, radius would be 0.4.
        let mut single = DotMatrix::new(1, 1);
        single.set(0, 0, true);
        let opts = Options {
            scale: 1,
            quiet_zone: 0,
            ..Options::default()
        };
        let svg = render_dots(&single, &opts);
        assert!(
            svg.contains(r##"r="0.5""##),
            "scale=1 must clamp radius to 0.5 (floor):\n{svg}"
        );
        assert!(
            !svg.contains(r##"r="0.4""##),
            "must NOT use unfloored 0.4:\n{svg}"
        );
        // cx = (0+0)*1 + 1/2 = 0 (integer division); same for cy.
        assert!(
            svg.contains(r##"<circle cx="0" cy="0" r="0.5""##),
            "single dot (0,0) scale=1: cx=cy=0:\n{svg}"
        );
    }

    /// Stage 11.A8c — pin `render_linear` bar/space alternation, the
    /// text-height conditional, and the text-position formulae.
    ///
    /// Setup (scale=2, quiet=1, bar_height=5, include_text=true, text="HI"):
    ///   * bars=[3, 1, 2, 2] → bar=3, space=1, bar=2, space=2
    ///   * total_width=8 → width_modules=8+2*1=10, width_px=20
    ///   * text present + include_text → text_height=10
    ///   * height_modules=5+10=15 → height_px=30
    ///   * font_size = text_height * scale = 10 * 2 = 20
    ///
    /// Bar layout (x starts at quiet*scale=2; only even i are bars):
    ///   * i=0 bar w=3: rect x=2 y=0 w=6 h=10; x → 8
    ///   * i=1 space w=1: silent; x → 10
    ///   * i=2 bar w=2: rect x=10 y=0 w=4 h=10; x → 14
    ///   * i=3 space w=2: silent; x → 18
    ///
    /// Text position: x = width_px/2 = 10, y = height_px - scale = 28.
    ///
    /// Mutations caught:
    ///   * `i % 2 == 0` flip — would emit rects for the SPACE indices
    ///     (assert NO rect at x=8 for the i=1 space).
    ///   * `text_height = 10` constant — width/height/text x/y all shift.
    ///   * `pattern.text.is_some() && opts.include_text` && → || would
    ///     emit text element when text=None.
    ///   * `width_px / 2` text centering math.
    ///   * `height_px - scale` text baseline.
    ///   * `font_size = text_height * scale` font sizing.
    ///   * `bar_height.max(1)` floor — covered by passing bar_height=5.
    ///   * `x += w` advances even on spaces (catches space=0 mutation).
    #[test]
    fn render_linear_pins_alternation_and_text_geometry() {
        use crate::encoding::LinearPattern;
        let pattern = LinearPattern {
            bars: vec![3, 1, 2, 2],
            text: Some("HI".to_string()),
        };
        let opts = Options {
            scale: 2,
            quiet_zone: 1,
            bar_height: 5,
            include_text: true,
            ..Options::default()
        };
        let svg = render_linear(&pattern, &opts);
        // viewBox = 20 × 30.
        assert!(
            svg.contains(r#"viewBox="0 0 20 30""#),
            "viewBox 20x30 (width=8+2, height=5+10):\n{svg}"
        );
        // Background rect.
        assert!(
            svg.contains(r##"<rect width="20" height="30" fill="#ffffff"/>"##),
            "background:\n{svg}"
        );
        // i=0 bar (w=3): rect x=2 y=0 w=6 h=10.
        assert!(
            svg.contains(r##"<rect x="2" y="0" width="6" height="10" fill="#000000"/>"##),
            "i=0 bar (x=2 y=0 w=6 h=10):\n{svg}"
        );
        // i=2 bar (w=2): rect x=10 y=0 w=4 h=10. The x advanced past
        // the i=1 space (w=2) from x=8 to x=10, which pins the
        // `x += w` accumulator for spaces too.
        assert!(
            svg.contains(r##"<rect x="10" y="0" width="4" height="10" fill="#000000"/>"##),
            "i=2 bar (x=10 y=0 w=4 h=10) — x advanced past i=1 space:\n{svg}"
        );
        // Total <rect>: 1 background + 2 bars (NOT 4 — spaces silent).
        let n_rects = svg.matches("<rect").count();
        assert_eq!(n_rects, 3, "expected 3 rects (1 bg + 2 bars):\n{svg}");
        // Negative: no bar rect at the SPACE x-position (x=8). If
        // `i % 2 == 0` flipped, the i=1 space at x=8 w=2 would emit
        // `<rect x="8" y="0" width="2" ...>`.
        assert!(
            !svg.contains(r#"<rect x="8" y="0" width="2""#),
            "no rect should sit at space x=8 (i%2 flip):\n{svg}"
        );
        // Text element: x=10 (width_px/2), y=28 (height_px-scale),
        // font_size=20 (text_height*scale).
        assert!(
            svg.contains(
                r##"<text x="10" y="28" font-family="monospace" font-size="20" text-anchor="middle" fill="#000000">HI</text>"##
            ),
            "text element with x=10 y=28 font-size=20:\n{svg}"
        );

        // Negative sub-check: with include_text=false, no <text> element.
        let opts_no_text = Options {
            include_text: false,
            ..opts
        };
        let svg_no_text = render_linear(&pattern, &opts_no_text);
        assert!(
            !svg_no_text.contains("<text"),
            "include_text=false must suppress <text>:\n{svg_no_text}"
        );
        // Height also shrinks (no text_height): height_modules=5, px=10.
        assert!(
            svg_no_text.contains(r#"viewBox="0 0 20 10""#),
            "viewBox 20x10 with include_text=false:\n{svg_no_text}"
        );
    }

    /// Stage 11.A8c — pin `render_matrix` row-coalesce + cell math.
    ///
    /// 3×3 matrix (scale=2, quiet=1):
    ///   row 0: [T T F] → coalesced run=2 at x=0 → rect x=2 y=2 w=4 h=2
    ///   row 1: [F T T] → coalesced run=2 at x=1 → rect x=4 y=4 w=4 h=2
    ///   row 2: [T F T] → two unit rects: rect x=2 y=6 w=2 h=2
    ///                                    rect x=6 y=6 w=2 h=2
    /// Total: 1 background + 4 cell rects = 5 rects.
    ///
    /// Mutations caught:
    ///   * `run += 1` → `run = 1`: any coalesced run gives only w=2 px
    ///     (the row 0 and row 1 runs of 2 cells would shrink to w=2).
    ///   * `x + run < matrix.width()` → `<=`: would OOB-panic on row 0.
    ///   * `(quiet + x as u32) * scale` shifted by mutations: px wrong.
    ///   * `(quiet + y as u32) * scale`: py wrong.
    ///   * `pw = run as u32 * scale`: width wrong.
    ///   * `height="{scale}"`: row height wrong.
    ///   * `matrix.get(x, y)` → `get(y, x)` transposition flips T/F pattern.
    ///   * Else `x += 1` removed: hang / OOB.
    ///   * `x += run` after emit: would re-process same cells.
    ///   * Row-by-row order: y outer, x inner.
    #[test]
    fn render_matrix_coalesces_runs_and_pins_cell_math() {
        use crate::encoding::BitMatrix;
        let mut bm = BitMatrix::new(3, 3);
        // Row 0: T T F
        bm.set(0, 0, true);
        bm.set(1, 0, true);
        // Row 1: F T T
        bm.set(1, 1, true);
        bm.set(2, 1, true);
        // Row 2: T F T
        bm.set(0, 2, true);
        bm.set(2, 2, true);

        let opts = Options {
            scale: 2,
            quiet_zone: 1,
            ..Options::default()
        };
        let svg = render_matrix(&bm, &opts);
        // viewBox = (3+2)*2 × (3+2)*2 = 10×10.
        assert!(
            svg.contains(r#"viewBox="0 0 10 10""#),
            "viewBox 10x10:\n{svg}"
        );
        // Row 0 coalesced run=2 → rect x=2 y=2 w=4 h=2.
        assert!(
            svg.contains(r##"<rect x="2" y="2" width="4" height="2" fill="#000000"/>"##),
            "row0 coalesced (x=2 y=2 w=4 h=2):\n{svg}"
        );
        // Row 1 coalesced run=2 starting at x=1 → rect x=4 y=4 w=4 h=2.
        assert!(
            svg.contains(r##"<rect x="4" y="4" width="4" height="2" fill="#000000"/>"##),
            "row1 coalesced (x=4 y=4 w=4 h=2):\n{svg}"
        );
        // Row 2 two unit rects at x=0 and x=2 → x=2/x=6.
        assert!(
            svg.contains(r##"<rect x="2" y="6" width="2" height="2" fill="#000000"/>"##),
            "row2 cell (0,2) (x=2 y=6 w=2 h=2):\n{svg}"
        );
        assert!(
            svg.contains(r##"<rect x="6" y="6" width="2" height="2" fill="#000000"/>"##),
            "row2 cell (2,2) (x=6 y=6 w=2 h=2):\n{svg}"
        );
        // Exactly 5 rects = 1 background + 4 cells (row 0 + row 1 each
        // coalesce to 1; row 2 emits 2 separately).
        let n_rects = svg.matches("<rect").count();
        assert_eq!(
            n_rects, 5,
            "expected 5 rects (1 bg + 2 coalesced + 2 unit):\n{svg}"
        );
        // Negative: no rect at the F cells. Row 0 F at (2,0) → would
        // sit at x=6 y=2. Row 1 F at (0,1) → x=2 y=4. Row 2 F at
        // (1,2) → x=4 y=6. None should appear.
        assert!(
            !svg.contains(r#"<rect x="6" y="2""#),
            "no rect at row0 F cell (2,0):\n{svg}"
        );
        assert!(
            !svg.contains(r#"<rect x="2" y="4""#),
            "no rect at row1 F cell (0,1):\n{svg}"
        );
        assert!(
            !svg.contains(r#"<rect x="4" y="6""#),
            "no rect at row2 F cell (1,2):\n{svg}"
        );
    }
}
