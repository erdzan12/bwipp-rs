//! Swiss QR Code.
//!
//! A QR Code carrying a Swiss QR-bill payment payload (SPC format). The
//! barcode geometry is plain QR; the only special thing is the **payload
//! structure** which is the user's responsibility — Swiss QR-bills require
//! a specific 32-line SPC text format ending with `EPD`.
//!
//! We accept any QR-encodable input and validate the SPC header so users
//! get a clear error if they pass non-Swiss data.

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

#[cfg(not(feature = "prefer-native-qrcode"))]
use super::qrcode_;

/// Encode a Swiss QR Code. The payload must start with the SPC header
/// (`SPC\n0200\n` — QR Type "SPC", version 02.00) or be a single-line
/// short payload for testing.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Minimal Swiss QR-bill payload. The full structure has ~30 lines
/// // separated by `\n`; this is the bare minimum that passes the SPC
/// // header check.
/// let payload = "SPC\n0200\n1\nCH4431999123000889012\n\
///     S\nMax Muster\nMustergasse\n22\n8000\nZuerich\nCH\n\
///     \n\n\n\n\n\n\n100.00\nCHF\nS\nSimone Muster\n\
///     Musterstrasse\n1\n8000\nZuerich\nCH\nNON\n\nThank you\nEPD";
/// let svg = render_svg(Symbology::SwissQrCode, payload, &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    if data.trim().is_empty() {
        return Err(Error::InvalidData("Swiss QR Code: payload is empty".into()));
    }
    if !data.starts_with("SPC") && !data.starts_with("SPC\r") && !data.starts_with("SPC\n") {
        return Err(Error::InvalidData(
            "Swiss QR Code: payload must start with the SPC header line".into(),
        ));
    }
    // Swiss QR-bills mandate error correction level M.
    let mut o = opts.clone();
    o.extras.retain(|(k, _)| k != "eclevel");
    o.extras.push(("eclevel".to_string(), "M".to_string()));
    #[cfg(feature = "prefer-native-qrcode")]
    {
        crate::symbology::qrcode_native::encode_with_options(data.as_bytes(), &o)
    }
    #[cfg(not(feature = "prefer-native-qrcode"))]
    {
        qrcode_::encode(data, &o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_SPC: &str = "SPC\n0200\n1\nCH4431999123000889012\n\
        S\nMax Muster\nMustergasse\n22\n8000\nZuerich\nCH\n\
        \n\n\n\n\n\n\n100.00\nCHF\nS\nSimone Muster\n\
        Musterstrasse\n1\n8000\nZuerich\nCH\nNON\n\nThank you\nEPD";

    #[test]
    fn rejects_empty() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 2-anchor
        // pin matching the source diagnostic at line 39 of swiss_qr.rs
        // (`Swiss QR Code: payload is empty`):
        //   1. `Swiss QR Code:` prefix
        //   2. `payload is empty` predicate
        match encode("", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Swiss QR Code:"),
                    "missing `Swiss QR Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "missing `payload is empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("SPC header"),
                    "wrong arm — SPC header diagnostic leaked into empty reject: {msg}"
                );
            }
            other => panic!("empty payload should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_spc_payload() {
        // Stage 11.A8c (cont) — upgrade discriminant-only to 2-anchor
        // pin matching the source diagnostic at line 42-44 of
        // swiss_qr.rs (`Swiss QR Code: payload must start with the
        // SPC header line`):
        //   1. `Swiss QR Code:` prefix
        //   2. `must start with the SPC header line` predicate
        match encode("hello", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Swiss QR Code:"),
                    "missing `Swiss QR Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with the SPC header line"),
                    "missing `must start with the SPC header line` predicate: {msg}"
                );
                assert!(
                    !msg.contains("payload is empty"),
                    "wrong arm — empty-payload diagnostic leaked into non-SPC reject: {msg}"
                );
            }
            other => panic!("\"hello\" should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn renders_minimal_spc() {
        let m = encode(MINIMAL_SPC, &Options::default()).unwrap();
        // The minimal Swiss QR-bill payload won't fit in version-1 QR;
        // accept any size >= 25 (QR version 2).
        // Stage 11.A8c (cont) — descriptive label naming the QR version
        // floor (V2=25 modules — the MINIMAL_SPC payload's size bracket
        // — kills a mutation that picks V1=21 and produces an undersized
        // symbol).
        assert!(
            m.width() >= 25,
            "Swiss QR Code minimal SPC payload must produce QR symbol ≥25 modules wide (QR V2+; V1=21 would not fit MINIMAL_SPC); got width={}, height={}",
            m.width(),
            m.height()
        );
    }

    /// Kills the `&& with ||` mutant at line ~41 col 33 (the first
    /// AND in the SPC-prefix guard:
    /// `!data.starts_with("SPC") && !data.starts_with("SPC\r") && !data.starts_with("SPC\n")`).
    ///
    /// Truth table for an input that starts with bare `"SPC"` (no
    /// trailing `\r` or `\n`):
    ///   a = !SPC   = false   (input does start with SPC)
    ///   b = !SPC\r = true    (input does NOT start with SPC\r)
    ///   c = !SPC\n = true    (input does NOT start with SPC\n)
    /// Original `a && b && c` = false → don't error (accept input).
    /// Mutant `a || b && c`   = false || (true && true) = true → error.
    ///
    /// So a "bare SPC" payload is accepted by the original and
    /// rejected by the mutant. We pin that direction.
    ///
    /// The companion `&& with ||` at line 41 col 63 (the SECOND AND)
    /// is equivalent: prefix relationships in the input force
    /// `a → false ⇒ at least one of b,c → false`, so under both the
    /// original `a && b && c` and the mutant `a && (b || c)` the
    /// only inputs that error are those starting with NEITHER SPC,
    /// SPC\r, nor SPC\n — i.e. exactly the same set. Documented as
    /// equivalent.
    #[test]
    fn accepts_bare_spc_without_newline() {
        // "SPC" followed by a payload-body. Note: this isn't a
        // valid Swiss QR-bill (the spec mandates a "\n" after the
        // identifier), but the *header check* should accept it,
        // and we don't require the downstream qrcode encoder to
        // emit a particular pixmap — only that the header guard
        // doesn't reject it. We use a payload that the downstream
        // encoder can absorb without panicking.
        let payload = "SPC payload body";
        // The encoder may error downstream on the substrate (the
        // qrcode crate doesn't reject odd payloads), but the
        // header guard SHOULD pass it through. Either success or a
        // non-InvalidData error (e.g. Backend) is acceptable — the
        // mutant would surface InvalidData with "SPC header".
        match encode(payload, &Options::default()) {
            Ok(_) => {}                  // Header passed, substrate succeeded.
            Err(Error::Backend(_)) => {} // Header passed, substrate failed.
            Err(Error::InvalidData(msg)) => {
                if msg.contains("SPC") {
                    panic!(
                        "bare 'SPC' payload was rejected by the header guard: {msg:?}; \
                         the `&& -> ||` mutant at line 41 may have shifted the guard"
                    );
                }
            }
            Err(other) => {
                // Any other error variant is also evidence the
                // header guard didn't reject the input.
                let _ = other;
            }
        }
    }

    /// Kills the `!= with ==` mutant at line ~48 col 32 (the
    /// eclevel-filter guard `k != "eclevel"`). Under the original
    /// the filter strips a user-supplied eclevel and re-injects
    /// `"M"`; under the mutant it keeps only `eclevel` entries.
    /// We pass `eclevel = "L"`, which under the mutant would survive
    /// into `o.extras` (alongside the canonical "M") and — since
    /// `Options::get` returns the first match — make the downstream
    /// encoder honor "L" instead of "M".
    ///
    /// Swiss QR-bill mandates EC level M; we anchor that the output
    /// matches the M-forced version regardless of what eclevel the
    /// user passes.
    #[test]
    fn forces_eclevel_m_over_user_supplied_l() {
        let with_user_l = encode(MINIMAL_SPC, &Options::default().with("eclevel", "L")).unwrap();
        let canonical_m = encode(MINIMAL_SPC, &Options::default()).unwrap();
        assert_eq!(
            with_user_l.width(),
            canonical_m.width(),
            "user-supplied eclevel=L leaked through the filter; \
             output dimensions differ from canonical eclevel=M"
        );
        assert_eq!(with_user_l.height(), canonical_m.height());
        for y in 0..canonical_m.height() {
            for x in 0..canonical_m.width() {
                assert_eq!(
                    with_user_l.get(x, y),
                    canonical_m.get(x, y),
                    "user-supplied eclevel=L leaked at ({x},{y})"
                );
            }
        }
    }

    /// Swiss QR Code is a thin wrapper that validates the SPC header,
    /// forces `eclevel = M` (mandated by the Swiss QR-bill spec), and
    /// delegates to the QR substrate. Pin the byte equality so a
    /// refactor can't re-implement Swiss QR through a different path.
    /// When `prefer-native-qrcode` is on (default), the comparison
    /// target is `qrcode_native::encode_with_options`; otherwise it's
    /// the upstream `qrcode_::encode`.
    #[test]
    fn composes_eclevel_m_and_qrcode() {
        let got = encode(MINIMAL_SPC, &Options::default()).unwrap();
        let opts = Options::default().with("eclevel", "M");
        #[cfg(feature = "prefer-native-qrcode")]
        let want =
            crate::symbology::qrcode_native::encode_with_options(MINIMAL_SPC.as_bytes(), &opts)
                .unwrap();
        #[cfg(not(feature = "prefer-native-qrcode"))]
        let want = crate::symbology::qrcode_::encode(MINIMAL_SPC, &opts).unwrap();
        assert_eq!(got.width(), want.width());
        assert_eq!(got.height(), want.height());
        for y in 0..got.height() {
            for x in 0..got.width() {
                assert_eq!(got.get(x, y), want.get(x, y), "{x},{y}");
            }
        }
    }

    /// Stage 11.A8c — pin SPC-prefix variants. The `starts_with` chain
    /// accepts "SPC", "SPC\r", "SPC\n" (and longer inputs starting
    /// with any of those). Kills `&& with ||` mutations on line 41.
    #[test]
    fn spc_prefix_variants_accepted() {
        // Pure "SPC" with newline content is valid (matches MINIMAL_SPC
        // which starts with "SPC\n").
        let with_lf = encode("SPC\n0200", &Options::default());
        // It may error downstream for incomplete payload but should
        // pass the SPC-prefix check.
        match with_lf {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    !msg.contains("must start with the SPC header"),
                    "SPC\\n prefix unexpectedly rejected: {msg}"
                );
            }
            _ => {} // OK
        }

        // CR variant.
        let with_cr = encode("SPC\r0200", &Options::default());
        if let Err(Error::InvalidData(msg)) = with_cr {
            assert!(
                !msg.contains("must start with the SPC header"),
                "SPC\\r prefix unexpectedly rejected: {msg}"
            );
        }

        // No SPC prefix → reject with the header message.
        //
        // Stage 11.A8c (cont) — upgrade single-substring header check
        // to 3-anchor pin:
        //   1. `Swiss QR Code:` symbology prefix
        //   2. `must start with the SPC header` predicate
        //   3. cross-arm contamination guard: must NOT contain
        //      `payload is empty` (the sibling empty-input arm at
        //      line 39 of swiss_qr.rs).
        let bad = encode("XPC\n", &Options::default()).unwrap_err();
        match bad {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Swiss QR Code:"),
                    "missing Swiss QR Code prefix: {msg}"
                );
                assert!(
                    msg.contains("must start with the SPC header"),
                    "non-SPC prefix should report header rejection, got {msg}"
                );
                assert!(
                    !msg.contains("payload is empty"),
                    "cross-arm contamination: header reject mentions `payload is empty`: {msg}"
                );
            }
            // Stage 11.A8c — input + variant echo so a regression
            // pinpoints whether the wrong variant fired or the
            // payload coerced through.
            other => panic!("`XPC\\n` should reject as InvalidData, got {other:?}"),
        }

        // Whitespace-only → empty rejection.
        //
        // Stage 11.A8c (cont) — upgrade single-substring "empty"
        // check (would match any message containing "empty") to
        // 3-anchor pin:
        //   1. `Swiss QR Code:` symbology prefix
        //   2. `payload is empty` predicate (exact phrasing)
        //   3. cross-arm contamination guard: must NOT contain `must
        //      start with the SPC header` (sibling arm at line 43).
        let empty_ish = encode("   ", &Options::default()).unwrap_err();
        match empty_ish {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Swiss QR Code:"),
                    "missing Swiss QR Code prefix: {msg}"
                );
                assert!(
                    msg.contains("payload is empty"),
                    "whitespace-only payload should report `payload is empty`, got {msg}"
                );
                assert!(
                    !msg.contains("must start with the SPC header"),
                    "cross-arm contamination: empty reject mentions `must start with the SPC header`: {msg}"
                );
            }
            // Stage 11.A8c — input + variant echo for whitespace-only
            // payload.
            other => panic!("whitespace-only payload should reject as InvalidData, got {other:?}"),
        }
    }
}
