# Compatibility exceptions

This document is the authoritative list of catalog rows where
bwipp-rs does **not** produce byte-for-byte output identical to BWIPP
(or bwip-js) yet remains spec-compliant for the symbology's scanner
contract.

Every row here is a deliberate, audited trade-off — not a "we'll get
to it later" placeholder. If a symbology is listed here, the
`rust/PORT_STATUS.md` table marks it accordingly and the test suite
pins both the spec-compliant behavior and the precise nature of the
divergence.

If you find a symbology that's listed as `verified` in PORT_STATUS but
produces output a scanner rejects, **that's a bug**, not a
compatibility exception. Open an issue.

---

## Current exceptions: **none**

As of Stage 17e, the bucket is empty. Every catalog row that was
previously a compatibility exception has been graduated to
`verified`:

* **QR Code family** (`qrcode`, `qrcode_iso`, `microqrcode`,
  `rectangularmicroqrcode`, `swissqrcode`, `gs1qrcode`, `gs1dlqrcode`,
  `hibc_lic_qrcode`, `hibc_pas_qrcode`) — was a compatibility
  exception when the `qrcode` crate was the default substrate
  (Stage 13 and earlier). The `qrcode` crate's mask scorer tie-breaks
  differently from BWIPP, so the rendered module pattern could
  diverge. Stage 16 flipped `prefer-native-qrcode` to a default Cargo
  feature, routing every QR-family catalog row through the native
  bwipp-faithful encoder in `src/symbology/qrcode_native/`. The
  native encoder is byte-for-byte verified against bwip-js on **48
  oracle-pinned corpus rows** (24 Full V1–V40 × L/M/Q/H samples + 8
  Micro M1–M4 × valid EC levels + 16 rMQR R7×_..R17×_ × M/H), so the
  family is no longer a divergence-by-design — it's actively
  enforced equivalence. The `qrcode` crate substrate is preserved as
  an opt-out via `cargo build --no-default-features` for callers who
  specifically want the upstream-crate behaviour.

* **`gs1qrcode`** — was a per-row compatibility exception during
  Stages 15f–17b because the GS1 mode indicator (FNC1-first-position,
  4-bit `0101`) was injected through `qrcode::Bits::push_optimal_data`
  and the native encoder hadn't yet exposed an FNC1-aware API.
  Stage 17c added `qrcode_native::encode_gs1_qrcode` and an
  fnc1-aware version-search that matches BWIPP's exact size choice
  for short GTIN payloads; the wrapper is now routed through the
  native path by default and byte-for-byte equivalent to BWIPP on
  the pinned corpus rows.

## What used to be an exception (kept for history)

The historical QR-family exception (status="compatibility exception"
in PORT_STATUS rows for `qrcode`/`microqrcode`/etc.) documented a
*substrate*-side divergence: the upstream `qrcode` crate implements
the same ISO/IEC 18004 spec but its mask-scorer loop iterates in a
slightly different order than BWIPP's PostScript encoder, so the
output module pattern occasionally landed on a tied mask whose
number differed from BWIPP's pick. The result decoded to the
correct payload, but a strict pixel-diff against bwip-js failed.

This is no longer relevant to the default build. It applies only
when a caller specifically opts out of the native encoder via
`--no-default-features`. The opt-out path is exercised by the
test suite (via the `substrate_baseline_pixs_for_hello` regression)
to detect any future drift in the upstream `qrcode` crate that
would break the substrate fallback.

## Future regressions

If a future port stage introduces a real divergence that can't be
closed within the same iteration, document it here and pin it with:

* a spec-compliance test (proves the symbol still decodes correctly), and
* a divergence pin (proves the precise nature of the divergence is
  intentional, so a refactor can't silently "fix" it back to BWIPP
  output without updating this doc).

Append a new section "Current exceptions" with the row(s) and
rationale, and bump the PORT_STATUS table accordingly so
`scripts/check-doc-counts.sh` stays green.
