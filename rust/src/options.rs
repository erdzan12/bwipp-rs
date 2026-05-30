//! Rendering options shared across symbologies.
//!
//! Each symbology accepts additional symbology-specific options; those go in
//! [`Options::extras`] as `(key, value)` string pairs to mirror BWIPP's flag
//! convention (`includecheck=true`, `eclevel=M`, etc.).

/// Module-level rendering options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Pixel multiplier for raster output, or stroke unit for vector output.
    /// Default: 4.
    pub scale: u32,
    /// Bar height in modules (only meaningful for 1D codes). Default: 50.
    pub bar_height: u32,
    /// Quiet zone in modules around the barcode. Default: 4.
    pub quiet_zone: u32,
    /// Whether to render human-readable text under 1D symbologies.
    pub include_text: bool,
    /// Foreground color as RGB. Default: black.
    pub foreground: [u8; 3],
    /// Background color as RGB. Default: white.
    pub background: [u8; 3],
    /// Symbology-specific options (e.g. `("eclevel", "M")`, `("type", "29")`).
    pub extras: Vec<(String, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scale: 4,
            bar_height: 50,
            quiet_zone: 4,
            include_text: false,
            foreground: [0, 0, 0],
            background: [255, 255, 255],
            extras: Vec::new(),
        }
    }
}

impl Options {
    /// Look up an extra option by key (case-sensitive, like BWIPP).
    ///
    /// # Example
    ///
    /// ```
    /// use bwipp::Options;
    ///
    /// let opts = Options::default().with("eclevel", "H");
    /// assert_eq!(opts.get("eclevel"), Some("H"));
    /// assert_eq!(opts.get("missing"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<&str> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Add an option `key=value`. Designed to be chained as a builder.
    ///
    /// # Example
    ///
    /// ```
    /// use bwipp::{Options, Symbology, render_svg};
    ///
    /// let opts = Options::default()
    ///     .with("eclevel", "Q")
    ///     .with("version", "5");
    /// let svg = render_svg(Symbology::QrCode, "Hello", &opts).unwrap();
    /// assert!(svg.starts_with("<svg"));
    /// ```
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extras.push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8c — pin `Options::default()` field values. Kills
    /// any mutation that changes a default literal (e.g. scale=4 →
    /// scale=0, quiet_zone=4 → quiet_zone=0).
    #[test]
    fn default_field_values() {
        let opts = Options::default();
        assert_eq!(opts.scale, 4);
        assert_eq!(opts.bar_height, 50);
        assert_eq!(opts.quiet_zone, 4);
        assert!(!opts.include_text);
        assert_eq!(opts.foreground, [0, 0, 0]);
        assert_eq!(opts.background, [255, 255, 255]);
        assert!(opts.extras.is_empty());
    }

    /// Stage 11.A8c — pin `Options::get` lookup semantics: returns
    /// Some for known keys, None for unknown, case-sensitive (BWIPP
    /// behavior). Kills `find` predicate mutations.
    #[test]
    fn get_lookup_semantics() {
        let opts = Options::default().with("eclevel", "H").with("version", "5");
        assert_eq!(opts.get("eclevel"), Some("H"));
        assert_eq!(opts.get("version"), Some("5"));
        assert_eq!(opts.get("missing"), None);
        // Case-sensitive.
        assert_eq!(opts.get("ECLEVEL"), None);
        assert_eq!(opts.get("Eclevel"), None);
        // Empty key.
        assert_eq!(opts.get(""), None);
    }

    /// Stage 11.A8c — pin `Options::with` builder behavior. Kills
    /// `push` order or value-swap mutations.
    #[test]
    fn with_builder_appends_and_returns() {
        let opts = Options::default().with("a", "1").with("b", "2");
        assert_eq!(opts.extras.len(), 2);
        // Order preserved: first append at index 0, second at index 1.
        assert_eq!(opts.extras[0], ("a".to_string(), "1".to_string()));
        assert_eq!(opts.extras[1], ("b".to_string(), "2".to_string()));
        // Subsequent .with for the same key appends a duplicate (BWIPP
        // matches by key from front, so later wins for `get`? Let's
        // verify both behaviors are pinned).
        let opts = Options::default().with("k", "first").with("k", "second");
        assert_eq!(opts.extras.len(), 2);
        // get returns the FIRST match.
        assert_eq!(opts.get("k"), Some("first"));
    }
}
