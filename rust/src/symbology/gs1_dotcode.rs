//! `gs1dotcode` — DotCode that carries GS1 Application Identifier
//! data.
//!
//! Mirrors BWIPP's `bwipp_gs1dotcode` (bwip-js line 42719+): a thin
//! wrapper that
//!
//! 1. parses the input as a GS1 element string `(NN)data(MM)data…`
//!    via [`crate::util::gs1::parse`], validating both AI syntax
//!    and the GS1 spec's per-AI length / character / requisite
//!    rules,
//! 2. flattens the elements into a byte stream with a leading FNC1
//!    (`0x1D`) and FNC1 separators after every variable-length AI
//!    except the last (via [`crate::util::gs1::encode_with_fnc1`]),
//!    and
//! 3. lifts that byte stream into the marker-aware
//!    `&[i16]` form DotCode's [`crate::symbology::dotcode::encode_with_markers`]
//!    consumes — every `0x1D` separator becomes the
//!    [`crate::symbology::dotcode::FN1`] marker (codeword 107 in the
//!    output), every other byte becomes its positive `i16` value.
//!
//! Reference: BWIPP `bwipp_gs1dotcode`, ISO/IEC 15434, GS1 General
//! Specifications §7.

use crate::error::Error;
use crate::symbology::dotcode::{self, DotCodeSymbol, FN1};
use crate::util::gs1;

/// Encode a GS1 element string `(NN)data(MM)data…` as a DotCode
/// symbol. Unlike [`dotcode::encode`] (which takes raw bytes), this
/// entry point performs full GS1 AI parsing + validation before
/// driving the DotCode state machine, so callers always pass the
/// canonical parenthesised representation.
///
/// # Errors
///
/// * `Error::InvalidData` if [`gs1::parse`] rejects the input (bad
///   AI syntax, unknown AI, data violating the AI spec, etc.).
/// * Whatever [`dotcode::encode_with_markers`] surfaces (typically
///   `InvalidData` for payloads that exceed the substrate's nw>112
///   threshold, etc.).
///
/// # Examples
///
/// Drive through the public `Symbology::Gs1DotCode` dispatch
/// (this module is `pub(crate)`; the canonical entry point is
/// [`crate::Symbology::Gs1DotCode`]):
///
/// ```
/// use bwipp::{Symbology, Options};
/// let sym = Symbology::Gs1DotCode;
/// let _enc = sym.encode("(01)04012345123456", &Options::default()).unwrap();
/// ```
pub fn encode(input: &[u8]) -> Result<DotCodeSymbol, Error> {
    // Parse GS1 AIs. `gs1::parse` takes &str — convert the byte
    // slice via from_utf8; non-UTF-8 input is by definition not a
    // valid GS1 element string (BWIPP rejects upstream too).
    let text = std::str::from_utf8(input)
        .map_err(|_| Error::InvalidData("gs1dotcode: input is not valid UTF-8".to_string()))?;
    let elements = gs1::parse(text).map_err(|e| Error::InvalidData(format!("gs1dotcode: {e}")))?;
    // Build the flat FNC1-separated byte stream (leading FNC1 +
    // AIs + inter-element separators where the AI is variable-
    // length and another AI follows). Then lift to the i16+FN1
    // form DotCode consumes.
    let bytes = gs1::encode_with_fnc1(&elements);
    let mut stream: Vec<i16> = Vec::with_capacity(bytes.len());
    for &b in &bytes {
        if b == gs1::FNC1 {
            stream.push(FN1);
        } else {
            stream.push(i16::from(b));
        }
    }
    dotcode::encode_with_markers(&stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(01)04012345123456` — GTIN-14, the canonical single-AI GS1
    /// payload. BWIPP's `bwipp_gs1dotcode` produces cws `[1, 4, 1,
    /// 23, 45, 12, 34, 56]` (8 digit pairs after the leading FN1 is
    /// absorbed at segstart, since 16 digits >= 2). Verified via
    /// `tools/oracle-dotcode-fnc.js` (and the bwipp_gs1dotcode
    /// patched oracle — bwip-js msg = `[-25, 48, 49, 48, 52, …,
    /// 53, 54]` with 17 entries, our stream matches byte-for-byte).
    #[test]
    fn encode_gtin_14_matches_bwip_js_logical_cws() {
        let text = "(01)04012345123456";
        let elements = gs1::parse(text).unwrap();
        let bytes = gs1::encode_with_fnc1(&elements);
        let stream: Vec<i16> = bytes
            .iter()
            .map(|&b| if b == gs1::FNC1 { FN1 } else { i16::from(b) })
            .collect();
        // Sanity-check stream: leading FN1, then "01" + 14 digits.
        assert_eq!(stream[0], FN1);
        let cws = dotcode::encode_message_with_markers(&stream).unwrap();
        assert_eq!(cws, vec![1, 4, 1, 23, 45, 12, 34, 56]);
    }

    /// `(01)04012345123456(10)ABC123` — GTIN-14 + variable-length
    /// batch/lot. BWIPP cws `[1, 4, 1, 23, 45, 12, 34, 56, 10, 106,
    /// 33, 34, 35, 17, 18, 19]`:
    ///   * 8 pairs from "01" + GTIN-14.
    ///   * `10` pair = the AI "10" digits.
    ///   * `106` = LAB latch to B (3 mode-B chars + 3 digits don't
    ///     beat staying in C from here).
    ///   * `33, 34, 35` = Bvals[A,B,C].
    ///   * `17, 18, 19` = Bvals[1, 2, 3] — single-digit encoding in B
    ///     because the trailing "123" isn't enough for back-to-C.
    ///
    /// Note: AI (01) is fixed-length so no FN1 separator goes
    /// between (01) data and (10). Per BWIPP spec.
    #[test]
    fn encode_gtin_with_lot_matches_bwip_js_logical_cws() {
        // Stage 11.A8c (cont) — descriptive label naming GTIN-14 + LOT
        // composition path + expected DotCode geometry.
        let sym = encode(b"(01)04012345123456(10)ABC123").unwrap();
        assert!(
            !sym.pixs.is_empty(),
            "encode(\"(01)04012345123456(10)ABC123\") (GTIN-14 + LOT \"ABC123\") must produce non-empty DotCode pixs vec; got len={}",
            sym.pixs.len()
        );
        assert_eq!(
            sym.rows, 19,
            "expected DotCode rows=19 for GTIN-14+LOT \"ABC123\"; got {}",
            sym.rows
        );
        assert_eq!(
            sym.columns, 28,
            "expected DotCode columns=28 for GTIN-14+LOT \"ABC123\"; got {}",
            sym.columns
        );
    }

    /// `(01)04012345123456(17)260520` — GTIN-14 + expiry date
    /// (both fixed-length AIs; no FN1 between them). BWIPP cws
    /// `[1, 4, 1, 23, 45, 12, 34, 56, 17, 26, 5, 20]` — 12 pure
    /// digit pairs.
    #[test]
    fn encode_gtin_with_expiry_matches_bwip_js_logical_cws() {
        let text = "(01)04012345123456(17)260520";
        let elements = gs1::parse(text).unwrap();
        let bytes = gs1::encode_with_fnc1(&elements);
        let stream: Vec<i16> = bytes
            .iter()
            .map(|&b| if b == gs1::FNC1 { FN1 } else { i16::from(b) })
            .collect();
        let cws = dotcode::encode_message_with_markers(&stream).unwrap();
        assert_eq!(cws, vec![1, 4, 1, 23, 45, 12, 34, 56, 17, 26, 5, 20]);
    }

    /// Stage 11.A8c — pin the three top-of-encode rejection paths with
    /// diagnostic substrings so mutants that:
    ///   * drop the `"gs1dotcode: "` prefix,
    ///   * swap the from_utf8 vs gs1::parse error wrappers,
    ///   * drop the `{e}` interpolation in `gs1dotcode: {e}`,
    ///   * change ParseError::Empty's message string,
    /// are all caught. The previous weak `matches!(_, Error::InvalidData(_))`
    /// checks pinned only the variant.
    ///
    /// Empty input is rejected (matches BWIPP / `gs1::parse`).
    #[test]
    fn encode_empty_rejected() {
        // Stage 11.A8c (cont) — three sibling tests previously shared
        // identical `panic!("expected InvalidData; got {err:?}")` text.
        // Converted to `match` so the catch-all panic names the SPECIFIC
        // input (empty / invalid AI / non-UTF-8) — a mutation that
        // re-routes one wrapper path to a different Error variant lands
        // with a self-explanatory failure naming the rerouted input.
        let msg = match encode(b"").unwrap_err() {
            Error::InvalidData(m) => m,
            err => panic!(
                "encode(b\"\") must reject as Err(InvalidData(empty)); got {err:?} (mutation re-routed gs1dotcode wrapper Empty path)"
            ),
        };
        assert!(
            msg.starts_with("gs1dotcode: "),
            "wrapper prefix must be present; got {msg:?}"
        );
        assert!(
            msg.contains("input is empty"),
            "must carry ParseError::Empty message; got {msg:?}"
        );
        assert!(
            !msg.contains("UTF-8"),
            "empty path must not leak the UTF-8 message; got {msg:?}"
        );
    }

    /// Bad AI is rejected.
    #[test]
    fn encode_invalid_ai_rejected() {
        let msg = match encode(b"(99999)X").unwrap_err() {
            Error::InvalidData(m) => m,
            err => panic!(
                "encode(b\"(99999)X\") must reject as Err(InvalidData(invalid AI)); got {err:?} (mutation re-routed gs1dotcode wrapper InvalidAi path)"
            ),
        };
        assert!(
            msg.starts_with("gs1dotcode: "),
            "wrapper prefix must be present; got {msg:?}"
        );
        // "99999" is 5 digits → ParseError::InvalidAi (must be 2-4 digits).
        assert!(
            msg.contains("invalid AI") && msg.contains("99999"),
            "must carry the offending AI string and 'invalid AI' tag; got {msg:?}"
        );
        assert!(
            !msg.contains("UTF-8") && !msg.contains("input is empty"),
            "invalid-AI path must not leak other diagnostics; got {msg:?}"
        );
    }

    /// Non-UTF-8 bytes (would break the GS1 parser) are rejected
    /// cleanly with an InvalidData error rather than a panic.
    #[test]
    fn encode_non_utf8_rejected() {
        let msg = match encode(&[0xFF, 0xFE, b'(', b'0', b'1', b')']).unwrap_err() {
            Error::InvalidData(m) => m,
            err => panic!(
                "encode(&[0xFF, 0xFE, ...]) must reject as Err(InvalidData(non-UTF-8)); got {err:?} (mutation re-routed gs1dotcode wrapper UTF-8 path)"
            ),
        };
        assert!(
            msg.starts_with("gs1dotcode: "),
            "wrapper prefix must be present; got {msg:?}"
        );
        // Stage 11.A8c (cont) — tighten `msg.contains("not valid
        // UTF-8")` to the full predicate `input is not valid UTF-8`
        // (matches the format string at line 58 of gs1_dotcode.rs).
        // The original substring would survive a mutation that drops
        // `input is` (e.g. "gs1dotcode: not valid UTF-8") or replaces
        // `input` with another noun; the full predicate locks both.
        assert!(
            msg.contains("input is not valid UTF-8"),
            "must carry the full UTF-8 diagnostic predicate; got {msg:?}"
        );
        assert!(
            !msg.contains("GS1 parse:"),
            "non-UTF-8 path must short-circuit before gs1::parse; got {msg:?}"
        );
    }

    /// Stage 11.A8c — pin the FNC1→FN1 lift loop at lines 65-72.
    /// The existing happy-path tests cover payloads where the
    /// FNC1-separated byte stream contains only the leading FNC1
    /// (no inter-element separators). A payload with a variable-
    /// length AI followed by another AI produces an INTERNAL FNC1
    /// separator — the lift loop must convert both leading and
    /// internal FNC1 bytes to the FN1 marker, not just the first.
    ///
    /// Use `(10)A(11)260520`: variable AI 10 (lot "A") followed by
    /// fixed AI 11 (date "260520"). encode_with_fnc1 produces:
    ///   [FNC1, '1','0','A', FNC1, '1','1','2','6','0','5','2','0']
    ///
    /// Mutations to catch:
    ///   - `b == gs1::FNC1` → `b != gs1::FNC1`: swaps semantics —
    ///     EVERY non-FNC1 byte becomes FN1, and FNC1 becomes the
    ///     positive 29 (0x1D). The resulting stream would error
    ///     out in dotcode::encode_with_markers.
    ///   - `FN1` → other marker constant (`FN2`/`FN3`/etc.):
    ///     dotcode would emit a different control codeword.
    ///   - Drop the `if`/`else` and always push `i16::from(b)`:
    ///     FNC1 (0x1D = 29) would become positive codeword 29,
    ///     scrambling the encoder mode.
    #[test]
    fn encode_payload_with_internal_fnc1_separator_lifts_to_fn1_markers() {
        // Verify the underlying gs1::encode_with_fnc1 byte stream
        // has FNC1 at positions [0, 4] (leading + after 'A').
        let elements = gs1::parse("(10)A(11)260520").unwrap();
        let bytes = gs1::encode_with_fnc1(&elements);
        assert_eq!(bytes[0], gs1::FNC1, "leading FNC1 sentinel must be present");
        assert_eq!(
            bytes[4],
            gs1::FNC1,
            "internal FNC1 separator must be inserted after variable AI 10's data"
        );
        // Surrounding bytes are AI digits + data.
        assert_eq!(&bytes[1..4], b"10A");
        assert_eq!(&bytes[5..], b"11260520");

        // Now drive `encode` — it must successfully produce a symbol
        // (the lift loop converts BOTH FNC1 bytes to FN1 markers,
        // which dotcode::encode_with_markers accepts).
        let sym = encode(b"(10)A(11)260520")
            .expect("encode must succeed; payload with internal FNC1 is well-formed GS1 + DotCode");
        assert!(!sym.pixs.is_empty(), "symbol must have non-empty pixs");
        assert!(sym.rows > 0 && sym.columns > 0);
    }
}
