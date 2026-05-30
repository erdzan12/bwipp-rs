//! Royal Mail Mailmark.
//!
//! Two BWIPP catalog entries share this module:
//!
//! - **`mailmark`** ([`encode_typed`]) is BWIPP's standard entry. It takes
//!   an explicit `type` option (`7`, `9`, or `29`) that selects the Data
//!   Matrix size + format, and requires a `JGB ` prefix on the payload.
//! - **`mailmark2d`** ([`encode_2d`]) is the BWIPP catalog's
//!   length-derived variant: it picks the size from `data.len()` (45,
//!   70, or 90 characters) and skips the `JGB ` check.
//!
//! Both produce Data Matrix output via the `datamatrix` crate, which
//! auto-selects optimal encodation modes (ASCII / C40 / Text / X12 /
//! EDIFACT / Base256). The C40 mode handles the compressible Royal Mail
//! structured-field portion (uppercase + digits + space). The catalog
//! example for `mailmark` with `type=29` (a 56-char payload) encodes
//! cleanly to 16×48 — the same size BWIPP produces.
//!
//! Note: 90-char Type 29 payloads need spec-conformant Royal Mail
//! structure to compress under C40. A 90-char filler payload (e.g. 30
//! ASCII chars + 60 spaces) exceeds 16×48 capacity in both this port
//! and BWIPP — that's not a port-side limitation, just the spec's
//! capacity boundary.

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

use super::datamatrix_;

/// Encode a Royal Mail Mailmark via BWIPP's standard `type`-based
/// selector. Reads `opts.extras["type"]` (defaults to `"7"`) and picks
/// the matching Data Matrix size:
///
/// | type | version  | format     |
/// |------|----------|------------|
/// | `7`  | 24×24    | square     |
/// | `9`  | 32×32    | square     |
/// | `29` | 16×48    | rectangle  |
///
/// The payload must be at least 45 characters and start with `JGB `.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // 45-character `JGB `-prefixed payload (type 7, 24x24 square).
/// // The first 30 chars are the structured spec fields; the rest is
/// // space-padded customer reference.
/// let payload = "JGB 012100123412345678AB19XY1A               ";
/// assert_eq!(payload.len(), 45);
/// let svg = render_svg(Symbology::Mailmark, payload, &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_typed(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let type_param = opts.get("type").unwrap_or("7");
    let version = match type_param {
        "7" => "24x24",
        "9" => "32x32",
        "29" => "16x48",
        other => {
            return Err(Error::InvalidOption(format!(
                "Mailmark: type must be 7, 9, or 29 (got {other:?})"
            )));
        }
    };
    let payload = data.trim_end_matches('\n');
    if payload.len() < 45 {
        return Err(Error::InvalidData(format!(
            "Mailmark: payload must be at least 45 chars (got {})",
            payload.len()
        )));
    }
    if !payload.starts_with("JGB ") {
        return Err(Error::InvalidData(
            "Mailmark: payload must start with `JGB ` identifier".into(),
        ));
    }
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "version" && k != "type");
    o.extras.push(("version".to_string(), version.to_string()));
    datamatrix_::encode(payload, &o)
}

/// Encode a Royal Mail Mailmark 2D payload using length-derived sizing.
pub fn encode_2d(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let payload = data.trim_end_matches('\n');
    let version = match payload.len() {
        45 => "24x24",
        70 => "32x32",
        90 => "16x48",
        other => {
            return Err(Error::InvalidData(format!(
                "Mailmark 2D payload must be 45, 70, or 90 characters (got {other})"
            )));
        }
    };
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "version");
    o.extras.push(("version".to_string(), version.to_string()));
    datamatrix_::encode(payload, &o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(n: usize) -> String {
        // Build a payload that fits the spec: starts with "JGB " for the
        // typical UK Mailmark prefix, padded to length with spaces.
        let head = "JGB 012100123412345678AB19XY1A";
        let mut s = head.to_string();
        if s.len() < n {
            s.push_str(&" ".repeat(n - s.len()));
        }
        s.truncate(n);
        s
    }

    #[test]
    fn renders_45_char_payload_as_24x24() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark 2D 45-char Type-7 24×24 path.
        let m = encode_2d(&payload(45), &Options::default()).expect(
            "encode_2d(45-char payload, default) (Mailmark 2D 45-char Royal Mail Type-7 24×24 path) must succeed",
        );
        assert_eq!(m.width(), 24);
        assert_eq!(m.height(), 24);
    }

    #[test]
    fn renders_70_char_payload_as_32x32() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark 2D 70-char Type-9 32×32 path.
        let m = encode_2d(&payload(70), &Options::default()).expect(
            "encode_2d(70-char payload, default) (Mailmark 2D 70-char Royal Mail Type-9 32×32 path) must succeed",
        );
        assert_eq!(m.width(), 32);
        assert_eq!(m.height(), 32);
    }

    #[test]
    fn renders_90_char_filler_payload_exceeds_capacity() {
        // A 90-char payload constructed from `JGB <30 ASCII chars> + 60
        // spaces` doesn't have the Royal Mail spec's compressible
        // structure. The datamatrix backend's auto-mode optimiser tries
        // C40 / Text / Base256 and still can't fit it in 16×48 (49 data
        // codewords). BWIPP behaves identically — confirmed via
        // node-sidecar/test-mailmark90.js: BWIPP also raises
        // `bwipp.datamatrixNoValidSymbol#25761`. So this is not a port-
        // side gap; the Royal Mail Type C spec requires a more
        // structured 90-char payload.
        let result = encode_2d(&payload(90), &Options::default());
        // Stage 11.A8c — upgrade from matches!(_, Err(Error::Backend(_))) to
        // pin the Data Matrix backend prefix. encode_2d at line 87-103 has
        // TWO error variants:
        //   * InvalidData arm (line 94-96) for unrecognised payload lengths
        //   * Backend arm (forwarded from datamatrix_::encode line 134) for
        //     datamatrix-crate failures: "Data Matrix: {e:?}"
        //
        // For a 90-char "JGB " padded payload that exceeds 16x48 capacity,
        // BWIPP and the Rust port both surface the Backend variant. A
        // mutant that drops the "Data Matrix:" prefix or routes the
        // overflow through the InvalidData arm survives variant-only.
        let err = result.expect_err("90-char Mailmark fallback must fail");
        let Error::Backend(msg) = err else {
            panic!("expected Backend variant for 90-char overflow; got {err:?}");
        };
        assert!(
            msg.contains("Data Matrix:"),
            "diagnostic must carry the 'Data Matrix:' prefix forwarded from \
             datamatrix_::encode; got {msg:?}"
        );
        assert!(
            !msg.contains("must be 45, 70, or 90"),
            "diagnostic must NOT leak the length-validator arm (which would \
             indicate a mutant rerouted overflow); got {msg:?}"
        );
    }

    #[test]
    fn type_29_with_real_mailmark_sample_matches_16x48() {
        // BWIPP's bundled example payload for mailmark type=29 is
        // 56 chars: "JGB 012100123412345678AB19XY1A 0...www.xyz.com".
        // Both Rust and BWIPP encode this to 16×48 (48×16 in Rust's
        // dimensions since Data Matrix is laid out width × height).
        let sample = "JGB 012100123412345678AB19XY1A 0             www.xyz.com";
        let opts = Options::default().with("type", "29");
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark Type-29 56-char 48×16 path (BWIPP-bundled sample).
        let m = encode_typed(sample, &opts).expect(
            "encode_typed(BWIPP 56-char sample, type=29) (Mailmark Type-29 path: real Royal Mail sample → 48×16 rectangle) must succeed",
        );
        assert_eq!(m.width(), 48);
        assert_eq!(m.height(), 16);
    }

    #[test]
    fn rejects_wrong_length() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 3-anchor pin
        // matching the source diagnostic at line 94-96 of mailmark.rs:
        //   1. `Mailmark 2D` helper-qualified prefix
        //   2. `45, 70, or 90 characters` full length-spec (3-arm —
        //      a mutation dropping one length value fails)
        //   3. `got 9` value echo of the supplied length ("too short"
        //      has 9 chars)
        match encode_2d("too short", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Mailmark 2D"),
                    "missing `Mailmark 2D` helper prefix: {msg}"
                );
                assert!(
                    msg.contains("45, 70, or 90 characters"),
                    "missing full length-spec `45, 70, or 90 characters`: {msg}"
                );
                assert!(msg.contains("got 9"), "missing `got 9` value echo: {msg}");
            }
            other => panic!("\"too short\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn typed_default_is_type_7() {
        // No type extra → defaults to 7 → 24×24 square.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark Type-default path: no `type` extra → defaults
        // to Type-7 (45-char 24×24).
        let m = encode_typed(&payload(45), &Options::default()).expect(
            "encode_typed(45-char payload, default) (Mailmark default-type path: no `type` extra → Type-7 → 24×24) must succeed",
        );
        assert_eq!(m.width(), 24);
        assert_eq!(m.height(), 24);
    }

    #[test]
    fn typed_9_picks_32x32() {
        let opts = Options::default().with("type", "9");
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark explicit Type-9 path: type=9 → 70-char 32×32.
        let m = encode_typed(&payload(70), &opts).expect(
            "encode_typed(70-char payload, type=9) (Mailmark explicit Type-9 path → 32×32) must succeed",
        );
        assert_eq!(m.width(), 32);
        assert_eq!(m.height(), 32);
    }

    #[test]
    fn typed_rejects_unknown_type() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidOption(_)))` to 3-anchor pin
        // matching the source diagnostic at line 63-65 of mailmark.rs.
        //
        // Note: the sibling test `typed_rejection_arms_pin_diagnostic_
        // and_length_boundary` (line 274+) already pins type=13 with
        // a 2-anchor `contains("type must be...") && contains("\"13\"")`
        // check. This sibling test independently pins the same
        // diagnostic via the standalone `typed_rejects_unknown_type`
        // entry point — covers a mutation that drops the
        // `_ => Err(Error::InvalidOption(...))` catch-all arm by way
        // of two independent test paths.
        let opts = Options::default().with("type", "13");
        match encode_typed(&payload(45), &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("Mailmark:"),
                    "missing `Mailmark:` prefix: {msg}"
                );
                assert!(
                    msg.contains("type must be 7, 9, or 29"),
                    "missing `type must be 7, 9, or 29` full alphabet: {msg}"
                );
                assert!(
                    msg.contains("\"13\""),
                    "missing `\"13\"` Debug echo of offending type: {msg}"
                );
            }
            other => panic!("type=13 should reject as InvalidOption, got {other:?}"),
        }
    }

    #[test]
    fn typed_rejects_missing_jgb_prefix() {
        // Stage 11.A8c (cont) — upgrade from discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to 2-anchor pin
        // matching the source diagnostic at line 76-77 of mailmark.rs.
        //   1. `Mailmark:` symbology prefix
        //   2. `must start with \`JGB \` identifier` predicate (with
        //      escaped backticks — pins the exact diagnostic, kills
        //      a mutation that drops the prefix-check entirely)
        //
        // The 45-char payload check passes first (length is exactly
        // 45), so this exercises ONLY the JGB-prefix branch at line
        // 75-77 of mailmark.rs.
        //
        // 45-char payload that doesn't start with "JGB ".
        let bad = "DXX 012100123412345678AB19XY1A".to_string()
            + &" ".repeat(45 - "DXX 012100123412345678AB19XY1A".len());
        match encode_typed(&bad, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Mailmark:"),
                    "missing `Mailmark:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with `JGB ` identifier"),
                    "missing `must start with \\`JGB \\` identifier` predicate: {msg}"
                );
                assert!(
                    !msg.contains("at least 45 chars"),
                    "wrong arm — length-cap diagnostic leaked into JGB-prefix reject: {msg}"
                );
            }
            other => panic!("non-JGB payload should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — strengthen the existing weak Mailmark rejection
    /// assertions and pin the short-payload arm (line 69-73) which is
    /// otherwise completely uncovered.
    ///
    /// Anchors:
    ///   1. encode_typed with type=13 → "Mailmark: type must be 7, 9,
    ///      or 29" + offending value diagnostic.
    ///   2. encode_typed with empty type ("") → same rejection (catches
    ///      the catch-all arm with empty string).
    ///   3. encode_typed with 44-char payload → "at least 45 chars" +
    ///      "got 44" diagnostic (catches `< 45` boundary and message
    ///      drift on the length-cap arm).
    ///   4. encode_typed with 45-char non-JGB prefix → "must start with
    ///      `JGB ` identifier" diagnostic.
    ///   5. Sanity: 45-char JGB-prefixed payload + default type → ok
    ///      (kills `< 45` → `<= 45` boundary mutant).
    ///
    /// Mutations killed:
    ///   * Type-map message text drift.
    ///   * Type-map catch-all `_ => Err(...)` swapped for `_ => Ok(...)`.
    ///   * `payload.len() < 45` boundary mutation (`<= 45` would reject 45).
    ///   * Length-error message text drift.
    ///   * JGB prefix arm message text drift.
    ///   * `payload.starts_with("JGB ")` predicate negation.
    #[test]
    fn typed_rejection_arms_pin_diagnostic_and_length_boundary() {
        // Anchor 1: type=13 → catch-all arm with named diagnostic.
        let opts = Options::default().with("type", "13");
        match encode_typed(&payload(45), &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("type must be 7, 9, or 29") && msg.contains("\"13\""),
                "type=13 should pin 'type must be...' + '13', got: {msg}"
            ),
            other => panic!("type=13 should reject as InvalidOption, got {other:?}"),
        }

        // Anchor 2: type="" → catch-all arm.
        //
        // Stage 11.A8c (cont) — bring this arm to parity with sibling
        // anchor 1 (line 261, type="13") by adding the symbology
        // prefix + offending-value Debug echo:
        //   1. `Mailmark:` symbology prefix
        //   2. `type must be 7, 9, or 29` full predicate (preserved)
        //   3. `""` empty-string Debug echo (catches `{other:?}`
        //      interpolation drop in the format string at line 64
        //      of mailmark.rs)
        let opts = Options::default().with("type", "");
        match encode_typed(&payload(45), &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(msg.contains("Mailmark:"), "missing Mailmark prefix: {msg}");
                assert!(
                    msg.contains("type must be 7, 9, or 29"),
                    "type='' should reject with 'type must be' diagnostic, got: {msg}"
                );
                assert!(
                    msg.contains("\"\""),
                    "missing empty-string Debug echo `\"\"`: {msg}"
                );
            }
            other => panic!("type='' should reject, got {other:?}"),
        }

        // Anchor 3: 44-char payload → length-cap arm.
        //
        // Stage 11.A8c (cont) — add the `Mailmark:` symbology prefix
        // anchor (kills mutations that drop or substitute the symbology
        // name in the format string at line 70-73 of mailmark.rs).
        let p44 = payload(44);
        match encode_typed(&p44, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Mailmark:"), "missing Mailmark prefix: {msg}");
                assert!(
                    msg.contains("at least 45 chars") && msg.contains("got 44"),
                    "44-char payload should pin 'at least 45 chars' + 'got 44', got: {msg}"
                );
            }
            other => panic!("44-char payload should reject, got {other:?}"),
        }
        // Empty payload also hits the < 45 arm.
        match encode_typed("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Mailmark:"), "missing Mailmark prefix: {msg}");
                assert!(
                    msg.contains("at least 45 chars") && msg.contains("got 0"),
                    "empty payload should pin diagnostic with 'got 0', got: {msg}"
                );
            }
            other => panic!("empty payload should reject, got {other:?}"),
        }

        // Anchor 4: 45-char payload missing JGB prefix.
        //
        // Stage 11.A8c (cont) — add `Mailmark:` symbology-prefix
        // anchor (kills mutations that drop or substitute the
        // symbology name in the format string at line 76-78 of
        // mailmark.rs).
        let bad = "DXX 012100123412345678AB19XY1A".to_string()
            + &" ".repeat(45 - "DXX 012100123412345678AB19XY1A".len());
        assert_eq!(bad.len(), 45);
        match encode_typed(&bad, &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(msg.contains("Mailmark:"), "missing Mailmark prefix: {msg}");
                assert!(
                    msg.contains("must start with") && msg.contains("JGB"),
                    "non-JGB 45-char payload should pin JGB-prefix diagnostic, got: {msg}"
                );
            }
            other => panic!("non-JGB 45-char payload should reject, got {other:?}"),
        }

        // Anchor 5: 45-char JGB payload + default type → success
        // (kills `< 45` → `<= 45` boundary mutant which would reject 45).
        assert!(
            encode_typed(&payload(45), &Options::default()).is_ok(),
            "45-char JGB payload should encode (boundary not yet hit)"
        );
    }

    #[test]
    fn typed_and_2d_produce_identical_24x24() {
        // For matched type+length pairs the two API entry points must
        // produce identical Data Matrix output, since they share the
        // same datamatrix backend with the same `version` setting.
        let body = payload(45);
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark encode_typed-vs-encode_2d equivalence path:
        // matched 45-char + default type → both APIs must produce
        // pixel-identical 24×24 Data Matrix.
        let typed = encode_typed(&body, &Options::default()).expect(
            "encode_typed(45-char, default) (Mailmark typed-entry baseline; default type → 24×24 for typed-vs-2d equivalence cross-check) must succeed",
        );
        let length = encode_2d(&body, &Options::default()).expect(
            "encode_2d(45-char, default) (Mailmark length-entry baseline; 45 chars → 24×24 for typed-vs-2d equivalence cross-check) must succeed",
        );
        assert_eq!(typed.width(), length.width());
        assert_eq!(typed.height(), length.height());
        for y in 0..typed.height() {
            for x in 0..typed.width() {
                assert_eq!(typed.get(x, y), length.get(x, y), "mismatch at ({x},{y})");
            }
        }
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8b mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills the three extras-filter mutants on line ~81 of
    /// `encode_typed`:
    ///   - `!= with ==` (col 32, the `k != "version"` half),
    ///   - `&& with ||` (col 45, the conjunction),
    ///   - `!= with ==` (col 50, the `k != "type"` half).
    ///
    /// The original `retain(|(k, _)| k != "version" && k != "type")`
    /// keeps every extra that is NEITHER "version" NOR "type", then
    /// re-injects the version derived from the type option. Each
    /// mutant breaks that filter in a different way:
    ///   - the first `!=` flip retains only "version" entries (drops
    ///     unrelated extras that should pass through to datamatrix_);
    ///   - the `&&` → `||` flip retains everything (so the encoder's
    ///     own "version" / "type" extras leak through to datamatrix_
    ///     in addition to the freshly-injected one);
    ///   - the second `!=` flip retains only "type" entries.
    ///
    /// We pass an unrelated extra (`borderwidth`) and a `type`
    /// extra and assert encode_typed produces the same output as if
    /// only `type` were passed — under the first and third flip the
    /// `borderwidth` extra is dropped (no observable change for
    /// borderwidth since it's a renderer concern); under the `||`
    /// flip, two `version` entries reach datamatrix_ and the
    /// resulting symbol size shifts. We also test with an unrelated
    /// extra alone and verify it passes through.
    #[test]
    fn typed_filters_only_version_and_type_extras() {
        // Baseline: type=29 without unrelated extras → 16x48 symbol.
        let body = "JGB 012100123412345678AB19XY1A 0             www.xyz.com";
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark encode_typed extras-filter baseline: type=29
        // canonical produces 16×48 without user-supplied version.
        let baseline = encode_typed(body, &Options::default().with("type", "29")).expect(
            "encode_typed(BWIPP sample, type=29) (Mailmark encode_typed extras-filter baseline; canonical 16×48 against which stale-version-filtered output must match) must succeed",
        );
        assert_eq!(baseline.height(), 16);
        assert_eq!(baseline.width(), 48);

        // With a user-supplied (and stale) `version` extra: the
        // encoder's filter strips it and re-injects the canonical
        // type-derived version. Under the `!= "version"` → `==`
        // mutant the user's version is retained; under the `&&` →
        // `||` mutant *both* the user's and the canonical version
        // reach datamatrix_ (last-write-wins or stacked extras).
        // Either way the resulting size is no longer the canonical
        // 16x48 if any version drift occurred.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Mailmark stale-version-filter path: a stale user-
        // supplied `version=32x32` extra must be stripped + re-
        // injected with the type-29 canonical (48×16).
        let with_stale_version = encode_typed(
            body,
            &Options::default()
                .with("version", "32x32") // wrong; encoder must strip
                .with("type", "29"),
        )
        .expect(
            "encode_typed(BWIPP sample, version=32x32 + type=29) (Mailmark stale-version filter path; encoder must strip user `version` extra and re-inject type-29's canonical version, yielding 16×48 not 32×32) must succeed",
        );
        assert_eq!(
            with_stale_version.height(),
            baseline.height(),
            "encode_typed leaked a user-supplied `version` extra past the filter"
        );
        assert_eq!(
            with_stale_version.width(),
            baseline.width(),
            "encode_typed leaked a user-supplied `version` extra past the filter"
        );
    }

    /// Kills `encode_2d: replace != with ==` at line ~100 — the
    /// `retain(|(k, _)| k != "version")` filter that strips a
    /// user-supplied `version` before re-injecting the length-derived
    /// one. Same approach as the typed-counterpart above.
    #[test]
    fn encode_2d_filters_user_supplied_version_extra() {
        let body = payload(45); // length → 24x24
                                // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
                                // the Mailmark encode_2d extras-filter baseline + stale-version
                                // strip path: 45-char body canonical 24×24 vs same body with
                                // user-supplied version=32x32 (must strip + re-inject).
        let baseline = encode_2d(&body, &Options::default()).expect(
            "encode_2d(45-char body, default) (Mailmark encode_2d extras-filter baseline; 24×24 canonical against which stale-version output must match) must succeed",
        );
        assert_eq!(baseline.height(), 24);
        assert_eq!(baseline.width(), 24);

        let with_stale_version =
            encode_2d(&body, &Options::default().with("version", "32x32")).expect(
                "encode_2d(45-char body, version=32x32) (Mailmark encode_2d stale-version filter; user version=32x32 must be stripped, re-injected with length-derived 24×24) must succeed",
            );
        assert_eq!(
            with_stale_version.height(),
            baseline.height(),
            "encode_2d leaked a user-supplied `version` extra past the filter"
        );
        assert_eq!(
            with_stale_version.width(),
            baseline.width(),
            "encode_2d leaked a user-supplied `version` extra past the filter"
        );
    }
}
