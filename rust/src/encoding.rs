//! Core encoding representations.
//!
//! All symbologies produce an [`Encoded`] value that the renderer then turns
//! into SVG or PNG. The two shapes mirror BWIPP's split between `renlinear`
//! (1D) and `renmatrix` (2D).

/// Output of encoding a barcode payload.
#[derive(Debug, Clone)]
pub enum Encoded {
    /// One-dimensional barcode: a run-length stream of bar widths in modules.
    Linear(LinearPattern),
    /// Two-dimensional barcode: a black/white grid of modules.
    Matrix(BitMatrix),
    /// 4-state postal code: each character produces one bar of one of four
    /// vertical shapes (full / ascender / descender / tracker), separated by
    /// fixed-width gaps. Used by Royal Mail, KIX, Japan Post, DAFT, etc.
    Postal4State(Postal4Pattern),
    /// Stacked / multi-row 1D barcode: each row is its own [`LinearPattern`]
    /// drawn beneath the previous one, with a fixed-height row separator.
    /// Used by Codablock-F, PDF417, GS1 DataBar Stacked, and the
    /// composite-code 2D companion.
    Stacked(StackedPattern),
    /// 2D dot-matrix barcode: a sparse pattern of dots on a parity grid.
    /// Each "on" cell is rendered as a round dot rather than a square
    /// module — the defining visual feature of DotCode.
    Dots(DotMatrix),
    /// MaxiCode hexagonal symbol — a fixed 33×30 cell grid where
    /// odd-numbered rows are physically offset by half a module to
    /// the right, producing a hex-packed layout. Boxed because the
    /// inline `[bool; 990]` cells array dwarfs the other variants.
    Hex(Box<crate::symbology::maxicode::MaxiCodeSymbol>),
    /// Multi-colour 2D matrix barcode — each cell carries a palette
    /// index `0..=7` rather than a single bit. Currently used by
    /// Ultracode, which uses an 8-entry palette of CMYWK colours
    /// (white, cyan, magenta, yellow, green, blue, red, black per
    /// the AIM USS Ultracode spec). The renderer emits one coloured
    /// rect per cell in SVG / one RGB byte per pixel in PNG.
    ColorMatrix(ColorMatrix),
}

/// A 2D colour matrix: `rows × columns` cells where each cell
/// carries a palette index into an 8-entry `[Rgb8; 8]` colour table.
/// Cells with palette index 0 are treated as background (typically
/// white) by the renderer — no rect / pixel emitted.
///
/// The 8-colour palette is fixed at the symbology level (e.g. the
/// AIM Ultracode CMYWK palette is defined in
/// [`crate::symbology::ultracode::ULTRACODE_PALETTE`]) so the
/// `palette` field is included in the struct for the renderer's
/// convenience.
#[derive(Debug, Clone)]
pub struct ColorMatrix {
    width: usize,
    height: usize,
    /// Each entry is a palette index 0..=7. Index 0 = background.
    cells: Vec<u8>,
    /// 8-entry RGB palette (sRGB 8-bit per channel).
    palette: [Rgb8; 8],
}

/// 8-bit-per-channel sRGB colour. Used in [`ColorMatrix`] palettes
/// and the renderer dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb8 {
    /// Red channel (0..=255).
    pub r: u8,
    /// Green channel (0..=255).
    pub g: u8,
    /// Blue channel (0..=255).
    pub b: u8,
}

impl Rgb8 {
    /// Construct an RGB colour from 8-bit channel values.
    #[inline]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Format the colour as a CSS hex string `#rrggbb` (lowercase).
    pub fn to_css_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl ColorMatrix {
    /// Allocate an all-background (palette index 0) matrix of the
    /// given dimensions, with the given colour palette.
    pub fn new(width: usize, height: usize, palette: [Rgb8; 8]) -> Self {
        Self {
            width,
            height,
            cells: vec![0; width * height],
            palette,
        }
    }

    /// Width in modules.
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in modules.
    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Read a cell's palette index (`0..=7`).
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.cells[y * self.width + x]
    }

    /// Write a cell's palette index. Caller is responsible for
    /// passing a value in `0..=7`; out-of-range values are masked
    /// to 3 bits to keep the field invariant.
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, palette_idx: u8) {
        self.cells[y * self.width + x] = palette_idx & 0b111;
    }

    /// Borrow the 8-entry RGB palette.
    #[inline]
    pub fn palette(&self) -> &[Rgb8; 8] {
        &self.palette
    }

    /// Resolve a cell to its RGB colour.
    #[inline]
    pub fn cell_color(&self, x: usize, y: usize) -> Rgb8 {
        self.palette[self.get(x, y) as usize]
    }
}

/// A sparse 2D dot grid: `rows × columns` cells where `true` means a
/// dot is present at that position. Only odd-parity positions
/// (`(x + y) % 2 == 1`) ever carry true cells from a DotCode encoder;
/// the renderer treats all true cells uniformly.
#[derive(Debug, Clone)]
pub struct DotMatrix {
    width: usize,
    height: usize,
    data: Vec<bool>,
}

impl DotMatrix {
    /// Allocate a `width × height` grid with every cell `false`.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![false; width * height],
        }
    }

    /// Width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Read a cell. `true` means a dot is present at `(x, y)`.
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.data[y * self.width + x]
    }

    /// Write a cell.
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        self.data[y * self.width + x] = value;
    }
}

/// A single bar in a 4-state postal code.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bar4State {
    /// Short bar centered vertically (codeword digit `0`).
    Tracker,
    /// Bar that extends from the centerline down to the baseline (digit `1`).
    Descender,
    /// Bar that extends from the top down to the centerline (digit `2`).
    Ascender,
    /// Full-height bar (digit `3`).
    Full,
}

impl Bar4State {
    /// Construct from a 0..=3 codeword (returns `None` for other values).
    pub fn from_digit(d: u8) -> Option<Self> {
        Some(match d {
            0 => Self::Tracker,
            1 => Self::Descender,
            2 => Self::Ascender,
            3 => Self::Full,
            _ => return None,
        })
    }

    /// Whether the bar reaches the top of the symbol (`Ascender` or `Full`).
    pub fn has_ascender(self) -> bool {
        matches!(self, Self::Ascender | Self::Full)
    }

    /// Whether the bar reaches the bottom (`Descender` or `Full`).
    pub fn has_descender(self) -> bool {
        matches!(self, Self::Descender | Self::Full)
    }
}

/// A 4-state postal symbol: an ordered sequence of bars with optional
/// human-readable text.
#[derive(Debug, Clone)]
pub struct Postal4Pattern {
    /// Each element is one bar; bars are separated by single-module gaps in
    /// the rendered output.
    pub bars: Vec<Bar4State>,
    /// Human-readable text rendered beneath the symbol when
    /// [`crate::Options::include_text`] is `true`.
    pub text: Option<String>,
}

/// A stacked / multi-row 1D barcode. Each row carries its own
/// [`LinearPattern`]; the renderer draws them top-to-bottom with a one-
/// module spacer between rows.
#[derive(Debug, Clone)]
pub struct StackedPattern {
    /// Rows, top to bottom. All rows must share the same `total_width()`.
    pub rows: Vec<LinearPattern>,
    /// Optional human-readable text rendered below the entire symbol.
    pub text: Option<String>,
}

impl StackedPattern {
    /// Build a `StackedPattern` from a non-empty vec of rows. Verifies that
    /// every row has the same total width (returns the first mismatch as an
    /// `Err` of `(expected, got)` for the caller to report).
    pub fn new(rows: Vec<LinearPattern>, text: Option<String>) -> Result<Self, (u32, u32)> {
        if rows.is_empty() {
            return Ok(Self { rows, text });
        }
        let w = rows[0].total_width();
        for r in &rows[1..] {
            let rw = r.total_width();
            if rw != w {
                return Err((w, rw));
            }
        }
        Ok(Self { rows, text })
    }

    /// Width of one row in modules (all rows share this width).
    pub fn width(&self) -> u32 {
        self.rows.first().map(|r| r.total_width()).unwrap_or(0)
    }

    /// Number of rows.
    pub fn height_rows(&self) -> usize {
        self.rows.len()
    }
}

impl Postal4Pattern {
    /// Build a [`Postal4Pattern`] from a slice of 0..=3 codeword digits.
    /// Returns `None` if any digit is out of range.
    pub fn from_digits(digits: &[u8], text: Option<String>) -> Option<Self> {
        let mut bars = Vec::with_capacity(digits.len());
        for &d in digits {
            bars.push(Bar4State::from_digit(d)?);
        }
        Some(Self { bars, text })
    }

    /// Total number of bars in the symbol.
    pub fn len(&self) -> usize {
        self.bars.len()
    }

    /// Whether the symbol is empty.
    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }
}

/// A 1D bar pattern as a run-length encoding.
///
/// `bars[i]` is the width (in modules) of the i-th element. Even indices
/// (0, 2, 4, ...) are bars (foreground), odd indices are spaces (background).
/// This matches BWIPP's `sbs` (start-bar-space) convention.
#[derive(Debug, Clone)]
pub struct LinearPattern {
    /// Run-length widths, starting with a bar.
    pub bars: Vec<u8>,
    /// Human-readable text rendered beneath the bars (if the symbology
    /// supports it).
    pub text: Option<String>,
}

impl LinearPattern {
    /// Build a `LinearPattern` from a string of '1' and '0' characters where
    /// '1' is a bar module and '0' is a space module.
    pub fn from_modules(modules: &str, text: Option<String>) -> Self {
        let mut bars = Vec::new();
        let chars: Vec<char> = modules.chars().collect();
        if chars.is_empty() {
            return Self { bars, text };
        }
        // The pattern always starts with a bar. If the first module is '0',
        // we emit a zero-width bar so the alternation invariant holds.
        let mut expected = '1';
        let mut run: u8 = 0;
        for c in chars {
            if c == expected {
                run += 1;
            } else {
                bars.push(run);
                run = 1;
                expected = if expected == '1' { '0' } else { '1' };
            }
        }
        bars.push(run);
        Self { bars, text }
    }

    /// Total width in modules (sum of all bar/space widths).
    pub fn total_width(&self) -> u32 {
        self.bars.iter().map(|&b| u32::from(b)).sum()
    }
}

/// A 2D black/white module grid. `(0, 0)` is the top-left.
#[derive(Debug, Clone)]
pub struct BitMatrix {
    width: usize,
    height: usize,
    data: Vec<bool>,
}

impl BitMatrix {
    /// Allocate an all-white matrix.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![false; width * height],
        }
    }

    /// Width in modules.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in modules.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Read a module. `true` is black/foreground.
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.data[y * self.width + x]
    }

    /// Write a module.
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        self.data[y * self.width + x] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_modules_run_length_encodes() {
        let p = LinearPattern::from_modules("111001100011", None);
        assert_eq!(p.bars, vec![3, 2, 2, 3, 2]);
        assert_eq!(p.total_width(), 12);
    }

    /// Stage 11.A8c — pin `LinearPattern::from_modules` corner cases
    /// beyond the canonical mid-length input. The function maintains
    /// the invariant "pattern always starts with a bar": if the first
    /// module is '0', it emits a zero-width bar to keep
    /// even-indexed slots representing bars.
    ///
    /// Mutations to catch:
    ///   - `expected = '1'` → `'0'` (inverted initial polarity)
    ///   - `run += 1` → `run *= 2` or similar
    ///   - the alternation toggle `if expected == '1' { '0' } else { '1' }`
    ///   - skipping the final `bars.push(run)` after the loop
    #[test]
    fn from_modules_edge_cases() {
        // Empty → no bars.
        let p = LinearPattern::from_modules("", None);
        assert!(p.bars.is_empty(), "empty input → empty bars");
        assert_eq!(p.total_width(), 0);

        // Single bar.
        let p = LinearPattern::from_modules("1", None);
        assert_eq!(p.bars, vec![1]);

        // Single space → leading 0-width bar then 1-wide space.
        let p = LinearPattern::from_modules("0", None);
        assert_eq!(
            p.bars,
            vec![0, 1],
            "single '0' inserts a 0-width bar to preserve alternation"
        );

        // Two bars, one wide.
        let p = LinearPattern::from_modules("11", None);
        assert_eq!(p.bars, vec![2]);

        // Bar-space-bar.
        let p = LinearPattern::from_modules("101", None);
        assert_eq!(p.bars, vec![1, 1, 1]);

        // Leading two zeros: 0-width bar + 2-wide space + 2 bars.
        let p = LinearPattern::from_modules("0011", None);
        assert_eq!(p.bars, vec![0, 2, 2]);

        // Trailing space at the end.
        let p = LinearPattern::from_modules("100", None);
        assert_eq!(p.bars, vec![1, 2]);

        // Trailing bar at the end.
        let p = LinearPattern::from_modules("0010", None);
        assert_eq!(p.bars, vec![0, 2, 1, 1]);

        // Full alternation.
        let p = LinearPattern::from_modules("10101", None);
        assert_eq!(p.bars, vec![1, 1, 1, 1, 1]);

        // text passthrough.
        let p = LinearPattern::from_modules("1", Some("X".into()));
        assert_eq!(p.text.as_deref(), Some("X"));
    }

    #[test]
    fn matrix_default_white_and_set_works() {
        let mut m = BitMatrix::new(5, 3);
        assert!(!m.get(2, 1));
        m.set(2, 1, true);
        assert!(m.get(2, 1));
        assert!(!m.get(0, 0));
    }

    /// Stage 11.A8c — pin `DotMatrix::new`/`get`/`set` round-trip.
    /// DotMatrix carries the dot pattern for DotCode encoding;
    /// previously no direct test existed (it was only exercised
    /// end-to-end through the dotcode renderer). Mutations on
    /// width/height accessors or set→get round-trip would propagate
    /// to renderer pixel placement, so a direct test pins:
    ///   - Default-false invariant after new().
    ///   - set() write + get() read round-trip preserves the value.
    ///   - width() / height() report constructor values.
    ///   - Setting one cell doesn't affect other cells.
    #[test]
    fn dot_matrix_new_set_get() {
        let mut m = DotMatrix::new(7, 4);
        assert_eq!(m.width(), 7);
        assert_eq!(m.height(), 4);
        // All cells default false.
        for y in 0..4 {
            for x in 0..7 {
                assert!(!m.get(x, y), "({x},{y}) should default false");
            }
        }
        // Set one cell, verify only that cell is true.
        m.set(3, 2, true);
        assert!(m.get(3, 2));
        for y in 0..4 {
            for x in 0..7 {
                let expected = (x, y) == (3, 2);
                assert_eq!(m.get(x, y), expected, "({x},{y})");
            }
        }
        // Set another, verify both true (and others still false).
        m.set(0, 0, true);
        assert!(m.get(0, 0));
        assert!(m.get(3, 2));
        // Set a previously-true cell back to false.
        m.set(3, 2, false);
        assert!(!m.get(3, 2));
        assert!(m.get(0, 0)); // unaffected
    }

    #[test]
    fn bar4state_from_digit_round_trip() {
        assert_eq!(Bar4State::from_digit(0), Some(Bar4State::Tracker));
        assert_eq!(Bar4State::from_digit(1), Some(Bar4State::Descender));
        assert_eq!(Bar4State::from_digit(2), Some(Bar4State::Ascender));
        assert_eq!(Bar4State::from_digit(3), Some(Bar4State::Full));
        assert_eq!(Bar4State::from_digit(4), None);
    }

    #[test]
    fn bar4state_ascender_descender_flags() {
        assert!(Bar4State::Ascender.has_ascender());
        assert!(Bar4State::Full.has_ascender());
        assert!(!Bar4State::Descender.has_ascender());
        assert!(!Bar4State::Tracker.has_ascender());

        assert!(Bar4State::Descender.has_descender());
        assert!(Bar4State::Full.has_descender());
        assert!(!Bar4State::Ascender.has_descender());
        assert!(!Bar4State::Tracker.has_descender());
    }

    #[test]
    fn postal4_pattern_from_digits_rejects_out_of_range() {
        assert!(Postal4Pattern::from_digits(&[0, 1, 2, 4], None).is_none());
        let p = Postal4Pattern::from_digits(&[3, 2, 1, 0], None).unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(
            p.bars,
            vec![
                Bar4State::Full,
                Bar4State::Ascender,
                Bar4State::Descender,
                Bar4State::Tracker
            ]
        );
    }

    /// Stage 11.A8c — pin `Postal4Pattern::is_empty` and `len`
    /// behavior. `is_empty` is a thin wrapper around `bars.is_empty()`
    /// but had no direct test; mutations like `is_empty` → `!is_empty`
    /// or `true` would survive since callers typically check `len()`
    /// instead.
    #[test]
    fn postal4_pattern_len_and_is_empty() {
        // Empty input → empty pattern.
        let empty = Postal4Pattern::from_digits(&[], None).unwrap();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        // Single bar → not empty.
        let one = Postal4Pattern::from_digits(&[0], None).unwrap();
        assert_eq!(one.len(), 1);
        assert!(!one.is_empty(), "1-bar pattern must NOT be empty");

        // Multi-bar.
        let four = Postal4Pattern::from_digits(&[3, 2, 1, 0], None).unwrap();
        assert_eq!(four.len(), 4);
        assert!(!four.is_empty());

        // Direct construction (no `from_digits` filter).
        let direct = Postal4Pattern {
            bars: vec![],
            text: Some("ignored".into()),
        };
        assert!(
            direct.is_empty(),
            "empty bars → is_empty regardless of text"
        );
        assert_eq!(direct.len(), 0);
    }

    /// Stage 11.A8c — pin `StackedPattern::new`'s width-uniformity
    /// validation. The constructor returns `Err((expected, got))`
    /// when row widths don't all match, and `Ok` for empty or
    /// uniform input. No direct test existed; the function is only
    /// exercised end-to-end through stacked encoders (codablockf,
    /// code16k, code49) where a row-width mismatch would surface as
    /// a downstream encoding error.
    ///
    /// Pinning the helper directly catches:
    ///   - `delete the width check loop` → unequal rows silently accepted
    ///   - `if rw != w` → `==` swap → only equal-width rows would error
    ///   - The (expected, got) tuple ordering
    ///   - Empty/single-row passthrough behavior
    #[test]
    fn stacked_pattern_new_width_uniformity() {
        // Empty rows → Ok with empty rows.
        let empty: Vec<LinearPattern> = Vec::new();
        let sp = StackedPattern::new(empty, None).unwrap();
        assert_eq!(sp.height_rows(), 0);
        assert_eq!(sp.width(), 0);

        // Single row → Ok.
        let single = vec![LinearPattern {
            bars: vec![1, 2, 3, 4],
            text: None,
        }];
        let sp = StackedPattern::new(single, Some("hi".into())).unwrap();
        assert_eq!(sp.height_rows(), 1);
        assert_eq!(sp.width(), 10); // 1+2+3+4
        assert_eq!(sp.text.as_deref(), Some("hi"));

        // Three rows, same width (1+2+3 = 6) → Ok.
        let uniform = vec![
            LinearPattern {
                bars: vec![1, 2, 3],
                text: None,
            },
            LinearPattern {
                bars: vec![3, 2, 1],
                text: None,
            },
            LinearPattern {
                bars: vec![2, 2, 2],
                text: None,
            },
        ];
        let sp = StackedPattern::new(uniform, None).unwrap();
        assert_eq!(sp.height_rows(), 3);
        assert_eq!(sp.width(), 6);

        // Width mismatch at row 1: expected 6, got 7. Err((6, 7)).
        let mismatch = vec![
            LinearPattern {
                bars: vec![1, 2, 3],
                text: None,
            }, // total_width = 6
            LinearPattern {
                bars: vec![1, 2, 4],
                text: None,
            }, // total_width = 7
        ];
        match StackedPattern::new(mismatch, None) {
            Err((expected, got)) => {
                assert_eq!(expected, 6, "first row's width is the 'expected'");
                assert_eq!(got, 7, "diverging row's width is 'got'");
            }
            Ok(_) => panic!("width mismatch should error"),
        }

        // Mismatch in 3rd row (rows 0, 1 match; row 2 diverges) —
        // the loop reports the first mismatch.
        let late_mismatch = vec![
            LinearPattern {
                bars: vec![1, 2, 3],
                text: None,
            }, // 6
            LinearPattern {
                bars: vec![3, 2, 1],
                text: None,
            }, // 6
            LinearPattern {
                bars: vec![5, 5, 5],
                text: None,
            }, // 15
        ];
        match StackedPattern::new(late_mismatch, None) {
            Err((expected, got)) => {
                assert_eq!((expected, got), (6, 15));
            }
            Ok(_) => panic!("late mismatch must error"),
        }
    }

    /// Pinned `Rgb8` helpers: constructor preserves channels and the
    /// CSS hex formatter zero-pads each channel to 2 lowercase hex
    /// digits. SVG renderers depend on this exact format.
    #[test]
    fn rgb8_constructor_and_css_hex() {
        let c = Rgb8::new(0x12, 0xab, 0xff);
        assert_eq!((c.r, c.g, c.b), (0x12, 0xab, 0xff));
        assert_eq!(c.to_css_hex(), "#12abff");
        // Zero-padded.
        assert_eq!(Rgb8::new(0, 0, 0).to_css_hex(), "#000000");
        assert_eq!(Rgb8::new(0x0a, 0, 0x0f).to_css_hex(), "#0a000f");
    }

    /// `ColorMatrix::new` allocates an all-background matrix; `set`
    /// writes a palette index that `get` reads back, and `cell_color`
    /// resolves through the palette table.
    #[test]
    fn color_matrix_new_set_get_cell_color() {
        let palette: [Rgb8; 8] = [
            Rgb8::new(0xff, 0xff, 0xff),
            Rgb8::new(0x10, 0x20, 0x30),
            Rgb8::new(0x40, 0x50, 0x60),
            Rgb8::new(0x70, 0x80, 0x90),
            Rgb8::new(0xa0, 0xb0, 0xc0),
            Rgb8::new(0xd0, 0xe0, 0xf0),
            Rgb8::new(0x01, 0x02, 0x03),
            Rgb8::new(0x00, 0x00, 0x00),
        ];
        let mut m = ColorMatrix::new(4, 3, palette);
        assert_eq!(m.width(), 4);
        assert_eq!(m.height(), 3);
        // Fresh matrix is all-background (index 0).
        assert_eq!(m.get(0, 0), 0);
        assert_eq!(m.get(3, 2), 0);
        assert_eq!(m.cell_color(0, 0), palette[0]);

        // Set a few cells and read back.
        m.set(0, 0, 5);
        m.set(3, 2, 7);
        m.set(1, 1, 2);
        assert_eq!(m.get(0, 0), 5);
        assert_eq!(m.get(3, 2), 7);
        assert_eq!(m.get(1, 1), 2);
        assert_eq!(m.cell_color(0, 0), palette[5]);
        assert_eq!(m.cell_color(3, 2), palette[7]);
        assert_eq!(m.cell_color(1, 1), palette[2]);
    }

    /// Out-of-range palette indices passed to `set` are masked to
    /// 3 bits so the cell field stays in `0..=7`. This is the
    /// invariant the renderer relies on for safe palette indexing.
    #[test]
    fn color_matrix_set_masks_out_of_range_index() {
        let palette: [Rgb8; 8] = [Rgb8::new(0, 0, 0); 8];
        let mut m = ColorMatrix::new(1, 1, palette);
        m.set(0, 0, 9); // 0b1001 → masked to 0b001 = 1
        assert_eq!(m.get(0, 0), 1);
        m.set(0, 0, 0xff); // 0b11111111 → masked to 0b111 = 7
        assert_eq!(m.get(0, 0), 7);
        m.set(0, 0, 0x08); // 0b1000 → masked to 0b000 = 0
        assert_eq!(m.get(0, 0), 0);
    }

    /// `palette()` borrows the matrix's palette table — the renderer
    /// uses this to look up RGB for each cell index without copying.
    #[test]
    fn color_matrix_palette_borrow() {
        let palette: [Rgb8; 8] = [
            Rgb8::new(0x11, 0x22, 0x33),
            Rgb8::new(0x44, 0x55, 0x66),
            Rgb8::new(0x77, 0x88, 0x99),
            Rgb8::new(0xaa, 0xbb, 0xcc),
            Rgb8::new(0xdd, 0xee, 0xff),
            Rgb8::new(0x00, 0x11, 0x22),
            Rgb8::new(0x33, 0x44, 0x55),
            Rgb8::new(0x66, 0x77, 0x88),
        ];
        let m = ColorMatrix::new(2, 2, palette);
        let borrowed = m.palette();
        assert_eq!(borrowed[0], palette[0]);
        assert_eq!(borrowed[7], palette[7]);
        // Ensure it's the same array (length pinned at 8).
        assert_eq!(borrowed.len(), 8);
    }

    /// Stage 11.A8c — pin `Rgb8::to_css_hex` at boundary colors.
    /// Kills the `{:02x}` formatter mutations and channel-order
    /// swap mutations on line 82.
    #[test]
    fn rgb8_to_css_hex_known_colors() {
        // Black.
        assert_eq!(Rgb8::new(0, 0, 0).to_css_hex(), "#000000");
        // White.
        assert_eq!(Rgb8::new(255, 255, 255).to_css_hex(), "#ffffff");
        // Pure red — distinguishes channel order.
        assert_eq!(Rgb8::new(255, 0, 0).to_css_hex(), "#ff0000");
        // Pure green.
        assert_eq!(Rgb8::new(0, 255, 0).to_css_hex(), "#00ff00");
        // Pure blue.
        assert_eq!(Rgb8::new(0, 0, 255).to_css_hex(), "#0000ff");
        // Lowercase hex letters (the {:02x} formatter).
        assert_eq!(Rgb8::new(0xAB, 0xCD, 0xEF).to_css_hex(), "#abcdef");
        // Two-digit zero padding for small values.
        assert_eq!(Rgb8::new(1, 2, 3).to_css_hex(), "#010203");
    }

    /// Stage 11.A8c — pin `ColorMatrix::set` palette-index masking
    /// to 3 bits. Kills `& 0b111` mutation on line 121.
    #[test]
    fn color_matrix_set_masks_to_three_bits() {
        let palette = [Rgb8::new(0, 0, 0); 8];
        let mut cm = ColorMatrix::new(2, 2, palette);
        // In-range values stored as-is.
        cm.set(0, 0, 5);
        assert_eq!(cm.get(0, 0), 5);
        cm.set(1, 0, 7);
        assert_eq!(cm.get(1, 0), 7);
        // Out-of-range values masked to 3 bits.
        cm.set(0, 1, 8); // 0b1000 → masked to 0
        assert_eq!(cm.get(0, 1), 0);
        cm.set(1, 1, 0xFF); // 0b11111111 → masked to 0b111 = 7
        assert_eq!(cm.get(1, 1), 7);
    }

    /// Stage 11.A8c — pin `ColorMatrix::cell_color` palette resolution.
    /// Kills `get(x,y) as usize` index mutations on line 133.
    #[test]
    fn color_matrix_cell_color_resolves_palette() {
        let palette = [
            Rgb8::new(0, 0, 0),
            Rgb8::new(255, 0, 0),
            Rgb8::new(0, 255, 0),
            Rgb8::new(0, 0, 255),
            Rgb8::new(255, 255, 0),
            Rgb8::new(255, 0, 255),
            Rgb8::new(0, 255, 255),
            Rgb8::new(255, 255, 255),
        ];
        let mut cm = ColorMatrix::new(1, 1, palette);
        cm.set(0, 0, 3);
        assert_eq!(cm.cell_color(0, 0), Rgb8::new(0, 0, 255));
        cm.set(0, 0, 5);
        assert_eq!(cm.cell_color(0, 0), Rgb8::new(255, 0, 255));
    }
}
