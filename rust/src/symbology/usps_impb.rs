//! USPS Intelligent Mail Package Barcode (IMpb).
//!
//! IMpb is a GS1-128 derivative used for USPS package shipping. The
//! payload is a sequence of GS1 Application Identifiers — typically
//! AI (420) for ZIP, AI (92) for tracking, possibly AI (8008) for date.
//!
//! Wire-format-wise it's just a GS1-128 symbol. We delegate to
//! [`super::gs1_128::encode`] after a minimal sanity-check on the input.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

use super::gs1_128;

/// Encode a USPS IMpb payload. Accepts a parenthesised AI element string
/// (e.g. `(420)94401(92)0040145914745473030413`).
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidData(
            "USPS IMpb: payload must not be empty".into(),
        ));
    }
    if !trimmed.starts_with('(') {
        return Err(Error::InvalidData(
            "USPS IMpb: payload must use parenthesised AI format (e.g. (420)...)".into(),
        ));
    }
    gs1_128::encode(trimmed, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 21-23
        // (`USPS IMpb: payload must not be empty`). Cross-arm guard
        // against the non-parenthesised arm.
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS IMpb:"),
                    "missing `USPS IMpb:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("parenthesised"),
                    "wrong arm — non-AI-format diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty USPS IMpb payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_ai_payload() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 25-28
        // (`USPS IMpb: payload must use parenthesised AI format (e.g.
        // (420)...)`). Cross-arm guard against the empty arm.
        match encode("just digits", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS IMpb:"),
                    "missing `USPS IMpb:` prefix: {msg}"
                );
                assert!(
                    msg.contains("parenthesised AI format"),
                    "missing parenthesised-AI predicate: {msg}"
                );
                assert!(
                    !msg.contains("must not be empty"),
                    "wrong arm — empty-payload diagnostic leaked: {msg}"
                );
            }
            other => panic!("`just digits` should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn renders_canonical_payload() {
        // (420) is a numeric ZIP, variable up to 20 digits. (92) is variable
        // alphanumeric. Use a payload that the GS1 AI table understands.
        // Stage 11.A8c (cont) — descriptive label naming input + path.
        let p = encode("(420)94401", &Options::default()).unwrap();
        assert!(
            p.total_width() > 0,
            "encode(\"(420)94401\") (USPS IMpb with GS1 AI 420 ZIP=94401) must compose into non-empty 4-state bar symbol; got {}",
            p.total_width()
        );
    }

    /// Stage 11.A8c — pin trim + whitespace edge cases. The existing
    /// `rejects_empty` and `rejects_non_ai_payload` tests don't cover
    /// inputs that exercise the `trim()` step: whitespace-only,
    /// leading-whitespace + valid payload, or leading-whitespace +
    /// invalid payload. Mutations to catch:
    ///   - `data.trim()` → `data`: leading whitespace would skip the
    ///     `starts_with('(')` check and either succeed (with leading
    ///     whitespace propagated downstream) or fail with a different
    ///     error message.
    ///   - `trimmed.is_empty()` → `data.is_empty()`: would not reject
    ///     whitespace-only inputs as empty.
    ///   - `starts_with('(')` → `starts_with(' ')` or different
    ///     literal: invalid prefix detection breaks.
    #[test]
    fn trim_and_whitespace_edge_cases() {
        // Whitespace-only input must reject as empty (catches missing
        // trim before is_empty check).
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("empty")` upgraded to 3-anchor pin:
        //   1. `USPS IMpb:` symbology prefix
        //   2. `payload must not be empty` full predicate
        //   3. cross-arm contamination guard: must NOT contain
        //      `parenthesised` (sibling arm at line 27 of
        //      usps_impb.rs)
        match encode("   ", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS IMpb:"),
                    "missing USPS IMpb prefix: {msg:?}"
                );
                assert!(
                    msg.contains("payload must not be empty"),
                    "missing full predicate `payload must not be empty`: {msg:?}"
                );
                assert!(
                    !msg.contains("parenthesised"),
                    "cross-arm contamination: empty reject mentions `parenthesised`: {msg:?}"
                );
            }
            other => panic!("whitespace-only must reject as empty, got {other:?}"),
        }

        // Newline + tab only: same — reject as empty.
        // Stage 11.A8c — upgrade discriminant-only to 2-anchor pin
        // matching the same source diagnostic as the whitespace-only
        // arm above, with cross-arm guard.
        match encode("\n\t  ", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS IMpb:") && msg.contains("must not be empty"),
                    "`\\n\\t  ` (newline+tab) must trim to empty + report empty arm; got {msg}"
                );
                assert!(
                    !msg.contains("parenthesised"),
                    "newline+tab arm leaked non-AI-format diagnostic: {msg}"
                );
            }
            other => panic!("`\\n\\t  ` (newline+tab) should trim+reject as empty, got {other:?}"),
        }

        // Leading whitespace + valid (...) payload: trim should
        // normalise to the canonical form and succeed.
        let trimmed = encode("(420)94401", &Options::default()).unwrap();
        let leading = encode("  (420)94401", &Options::default()).unwrap();
        assert_eq!(
            leading.bars, trimmed.bars,
            "leading whitespace should be trimmed; bars must match canonical input"
        );

        // Trailing whitespace + valid (...) payload: also trimmed.
        let trailing = encode("(420)94401  ", &Options::default()).unwrap();
        assert_eq!(trailing.bars, trimmed.bars);

        // Leading whitespace + non-paren payload: trim then reject
        // with the "parenthesised AI format" error (NOT the empty
        // error — proves trim happens before non-empty + format check).
        // Diagnostic at line 27: "USPS IMpb: payload must use
        // parenthesised AI format (e.g. (420)...)" — 4-anchor `&&`
        // pin replaces the weak `||` (both substrings always present).
        match encode("  abc", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("USPS IMpb:"),
                    "diagnostic must carry the USPS IMpb prefix; got {msg:?}"
                );
                assert!(
                    msg.contains("parenthesised"),
                    "diagnostic must carry the 'parenthesised' format hint; got {msg:?}"
                );
                assert!(
                    msg.contains("AI format"),
                    "diagnostic must carry the 'AI format' descriptor; got {msg:?}"
                );
                assert!(
                    msg.contains("(420)"),
                    "diagnostic must carry the (420) example AI hint; got {msg:?}"
                );
            }
            other => panic!("leading-ws + non-paren must reject as wrong format, got {other:?}"),
        }
    }

    /// IMpb is just a thin wrapper that validates the parenthesised
    /// AI format and delegates to the verified `gs1_128::encode`.
    /// Anchor the byte-for-byte sbs equality so a future refactor
    /// can't smuggle in a different code path.
    #[test]
    fn matches_underlying_gs1_128() {
        let payload = "(420)94401";
        let impb = encode(payload, &Options::default()).unwrap();
        let gs1 = super::super::gs1_128::encode(payload, &Options::default()).unwrap();
        assert_eq!(impb.bars, gs1.bars);
    }
}
