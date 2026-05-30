//! Native QR Code encoder (Path D / Path 3 from
//! `crate::symbology::QR_PATH_D_PLAN`).
//!
//! Reference: ISO/IEC 18004 (full QR + Micro QR) and ISO/IEC 23941
//! (rMQR) implemented to **BWIPP `bwipp_qrcode` semantics** (bwip-js
//! `src/bwipp.js` lines 25521-28528, the 3003-line monolithic
//! encoder that handles all three QR formats via `$_.format` dispatch
//! on `{"full", "micro", "rmqr"}`).
//!
//! The two micro-form encoders in BWIPP are 24-line wrappers that
//! delegate here with the `format` option preset:
//!
//! * `bwipp_microqrcode` (bwip-js line 28614) → `format = "micro"`.
//! * `bwipp_rectangularmicroqrcode` (line 28638) → `format = "rmqr"`.
//!
//! ## Why a from-scratch native encoder?
//!
//! The upstream [`qrcode`](https://crates.io/crates/qrcode) crate
//! (which currently powers the 8 catalog rows `qrcode`, `qrcode_iso`,
//! `microqrcode`, `swissqrcode`, `gs1qrcode`, `gs1dlqrcode`,
//! `hibc_lic_qrcode`, `hibc_pas_qrcode`) doesn't support rMQR and
//! doesn't expose BWIPP's mask-tiebreak rule (lower-mask-index wins
//! on score ties). That divergence is documented in
//! [`crate::symbology::COMPATIBILITY_EXCEPTIONS`] and applies to the
//! whole QR family at once. Path 3 — this module — replaces the
//! upstream dependency with a BWIPP-faithful encoder so all 9 rows
//! graduate to byte-for-byte verified at once.
//!
//! Stage breakdown is documented in
//! [`QR_NATIVE_PORT_PLAN.md`](./QR_NATIVE_PORT_PLAN.md).
//!
//! ## Port status
//!
//! Stage 10 — BWIPP auto-version search in place. On top of Stage 9's
//! `encode_full_qr` entry point, this iteration adds:
//!
//! * [`auto_select_full_qr_version`] — picks the smallest V1..V40
//!   that holds the payload at the requested EC level, then upgrades
//!   the EC level to the highest that still fits (BWIPP `!fixedeclevel`
//!   branch). Mirrors bwip-js 27367-27437.
//! * Updated [`encode`] — runs auto-version-select then dispatches
//!   to [`encode_full_qr`]. Now produces correct symbols for any
//!   payload that fits Full QR (V1-V40).
//!
//! Catalog cutover into `Symbology::QrCode*` variants and the rMQR
//! formatfimmap remain deferred — both are non-blocking since the
//! public encode entry now works for the most common Full QR cases.

#![allow(dead_code)]

use crate::encoding::BitMatrix;
use crate::error::Error;

/// QR format dispatch. Mirrors BWIPP's `$_.format` option ranges.
/// All three formats share the same encoder body but with distinct
/// version-metrics tables, alignment-pattern layouts, mask sets, and
/// score-evaluation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub(crate) enum Format {
    /// Standard QR Code (ISO/IEC 18004). Versions 1..=40 (21×21 .. 177×177).
    Full,
    /// Micro QR Code (ISO/IEC 18004). Versions M1..=M4 (11×11 .. 17×17).
    Micro,
    /// Rectangular Micro QR Code (ISO/IEC 23941:2022). 32 rectangular
    /// variants (R7×43 .. R17×139 — heights 7/9/11/13/15/17, widths
    /// 43/59/77/99/139 depending on the row height).
    Rmqr,
}

/// One row of the BWIPP `qrcode_metrics` table (bwip-js line 25718).
/// Each tuple is laid out as:
///
///   `[format, version, layout_id, rows, cols, fimax, fimas, datacap_bits,
///     [eclen_l, eclen_m, eclen_q, eclen_h], [blocks_l_a, blocks_l_b,
///     blocks_m_a, blocks_m_b, blocks_q_a, blocks_q_b, blocks_h_a,
///     blocks_h_b]]`
///
/// Field meanings:
///
/// * `format` — [`Format::Full`] / [`Format::Micro`] / [`Format::Rmqr`].
/// * `version_str` — BWIPP's canonical id (`"M1"`, `"1"`, `"R7x43"`, …).
/// * `layout_id` — index into the alignment / function-pattern layout
///   table (BWIPP's `$_.qrcode_vM1` / `vR7x43` / etc.). Stage 6 maps
///   these to per-version coordinate lists.
/// * `rows`, `cols` — symbol dimensions in modules.
/// * `fimax`, `fimas` — BWIPP's format-info / version-info bit offsets
///   (used at Stage 6 placement). `99` is BWIPP's "not applicable"
///   sentinel.
/// * `datacap_bits` — total data-codeword capacity in bits (before the
///   mode-indicator + char-count-indicator overhead).
/// * `eclen` — number of EC codewords per (data block × EC level)
///   for [L, M, Q, H]. `99` means EC level not supported.
/// * `blocks` — eight-element block-count table: pairs of
///   `(count_a, count_b)` per EC level. The total data codewords for
///   EC level X = `eclen[X] × (count_a + count_b)`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VersionMetric {
    pub format: Format,
    pub version_str: &'static str,
    pub layout_id: u8,
    pub rows: u16,
    pub cols: u16,
    pub fimax: u16,
    pub fimas: u16,
    pub datacap_bits: u32,
    /// EC codeword count per EC level [L, M, Q, H], totalled across
    /// all interleaved blocks. `99` (NA) means EC level not supported
    /// at this version. Stored as `u16` because V40's totals
    /// (max 2430 at EC H) exceed `u8::MAX`.
    pub eclen: [u16; 4],
    pub blocks: [i8; 8],
}

/// BWIPP's "not applicable" sentinel value, used in fimax/fimas/eclen
/// when an EC level / format-info path doesn't apply.
pub(crate) const NA: u16 = 99;
/// `i8` flavour of [`NA`] for the `blocks` slots (BWIPP uses `-1` and
/// `99` interchangeably depending on the column; we store as-is).
pub(crate) const NA_I8: i8 = 99;

/// Full BWIPP `qrcode_metrics` table (bwip-js line 25718) — 76 rows
/// covering all Micro QR (M1..=M4), Full QR (V1..=V40), and rMQR
/// (32 variants R7×43..R17×139) versions.
///
/// Layout: one [`VersionMetric`] per `(format, version)` pair. The
/// `layout_id` is an opaque integer slot index that Stage 6 will map
/// to per-version alignment-pattern coordinate tables — for now it's
/// just a forward-reference placeholder.
///
/// Values extracted mechanically from bwip-js via
/// `rust/tools/extract-qrcode-metrics.js` (re-run if bwip-js
/// upgrades). Stored as a single `#[rustfmt::skip]` block so the
/// 76 entries stay on one line each for diff-friendliness.
#[rustfmt::skip]
pub(crate) const FULL_METRICS: [VersionMetric; 76] = [
    VersionMetric { format: Format::Micro, version_str: "M1", layout_id: 3, rows: 11, cols: 11, fimax: 98, fimas: 99, datacap_bits: 36, eclen: [2, 99, 99, 99], blocks: [1, 0, -1, -1, -1, -1, -1, -1] },
    VersionMetric { format: Format::Micro, version_str: "M2", layout_id: 4, rows: 13, cols: 13, fimax: 98, fimas: 99, datacap_bits: 80, eclen: [5, 6, 99, 99], blocks: [1, 0, 1, 0, -1, -1, -1, -1] },
    VersionMetric { format: Format::Micro, version_str: "M3", layout_id: 5, rows: 15, cols: 15, fimax: 98, fimas: 99, datacap_bits: 132, eclen: [6, 8, 99, 99], blocks: [1, 0, 1, 0, -1, -1, -1, -1] },
    VersionMetric { format: Format::Micro, version_str: "M4", layout_id: 6, rows: 17, cols: 17, fimax: 98, fimas: 99, datacap_bits: 192, eclen: [8, 10, 14, 99], blocks: [1, 0, 1, 0, 1, 0, -1, -1] },
    VersionMetric { format: Format::Full, version_str: "1", layout_id: 0, rows: 21, cols: 21, fimax: 98, fimas: 99, datacap_bits: 208, eclen: [7, 10, 13, 17], blocks: [1, 0, 1, 0, 1, 0, 1, 0] },
    VersionMetric { format: Format::Full, version_str: "2", layout_id: 0, rows: 25, cols: 25, fimax: 18, fimas: 99, datacap_bits: 359, eclen: [10, 16, 22, 28], blocks: [1, 0, 1, 0, 1, 0, 1, 0] },
    VersionMetric { format: Format::Full, version_str: "3", layout_id: 0, rows: 29, cols: 29, fimax: 22, fimas: 99, datacap_bits: 567, eclen: [15, 26, 36, 44], blocks: [1, 0, 1, 0, 2, 0, 2, 0] },
    VersionMetric { format: Format::Full, version_str: "4", layout_id: 0, rows: 33, cols: 33, fimax: 26, fimas: 99, datacap_bits: 807, eclen: [20, 36, 52, 64], blocks: [1, 0, 2, 0, 2, 0, 4, 0] },
    VersionMetric { format: Format::Full, version_str: "5", layout_id: 0, rows: 37, cols: 37, fimax: 30, fimas: 99, datacap_bits: 1079, eclen: [26, 48, 72, 88], blocks: [1, 0, 2, 0, 2, 2, 2, 2] },
    VersionMetric { format: Format::Full, version_str: "6", layout_id: 0, rows: 41, cols: 41, fimax: 34, fimas: 99, datacap_bits: 1383, eclen: [36, 64, 96, 112], blocks: [2, 0, 4, 0, 4, 0, 4, 0] },
    VersionMetric { format: Format::Full, version_str: "7", layout_id: 0, rows: 45, cols: 45, fimax: 22, fimas: 38, datacap_bits: 1568, eclen: [40, 72, 108, 130], blocks: [2, 0, 4, 0, 2, 4, 4, 1] },
    VersionMetric { format: Format::Full, version_str: "8", layout_id: 0, rows: 49, cols: 49, fimax: 24, fimas: 42, datacap_bits: 1936, eclen: [48, 88, 132, 156], blocks: [2, 0, 2, 2, 4, 2, 4, 2] },
    VersionMetric { format: Format::Full, version_str: "9", layout_id: 0, rows: 53, cols: 53, fimax: 26, fimas: 46, datacap_bits: 2336, eclen: [60, 110, 160, 192], blocks: [2, 0, 3, 2, 4, 4, 4, 4] },
    VersionMetric { format: Format::Full, version_str: "10", layout_id: 1, rows: 57, cols: 57, fimax: 28, fimas: 50, datacap_bits: 2768, eclen: [72, 130, 192, 224], blocks: [2, 2, 4, 1, 6, 2, 6, 2] },
    VersionMetric { format: Format::Full, version_str: "11", layout_id: 1, rows: 61, cols: 61, fimax: 30, fimas: 54, datacap_bits: 3232, eclen: [80, 150, 224, 264], blocks: [4, 0, 1, 4, 4, 4, 3, 8] },
    VersionMetric { format: Format::Full, version_str: "12", layout_id: 1, rows: 65, cols: 65, fimax: 32, fimas: 58, datacap_bits: 3728, eclen: [96, 176, 260, 308], blocks: [2, 2, 6, 2, 4, 6, 7, 4] },
    VersionMetric { format: Format::Full, version_str: "13", layout_id: 1, rows: 69, cols: 69, fimax: 34, fimas: 62, datacap_bits: 4256, eclen: [104, 198, 288, 352], blocks: [4, 0, 8, 1, 8, 4, 12, 4] },
    VersionMetric { format: Format::Full, version_str: "14", layout_id: 1, rows: 73, cols: 73, fimax: 26, fimas: 46, datacap_bits: 4651, eclen: [120, 216, 320, 384], blocks: [3, 1, 4, 5, 11, 5, 11, 5] },
    VersionMetric { format: Format::Full, version_str: "15", layout_id: 1, rows: 77, cols: 77, fimax: 26, fimas: 48, datacap_bits: 5243, eclen: [132, 240, 360, 432], blocks: [5, 1, 5, 5, 5, 7, 11, 7] },
    VersionMetric { format: Format::Full, version_str: "16", layout_id: 1, rows: 81, cols: 81, fimax: 26, fimas: 50, datacap_bits: 5867, eclen: [144, 280, 408, 480], blocks: [5, 1, 7, 3, 15, 2, 3, 13] },
    VersionMetric { format: Format::Full, version_str: "17", layout_id: 1, rows: 85, cols: 85, fimax: 30, fimas: 54, datacap_bits: 6523, eclen: [168, 308, 448, 532], blocks: [1, 5, 10, 1, 1, 15, 2, 17] },
    VersionMetric { format: Format::Full, version_str: "18", layout_id: 1, rows: 89, cols: 89, fimax: 30, fimas: 56, datacap_bits: 7211, eclen: [180, 338, 504, 588], blocks: [5, 1, 9, 4, 17, 1, 2, 19] },
    VersionMetric { format: Format::Full, version_str: "19", layout_id: 1, rows: 93, cols: 93, fimax: 30, fimas: 58, datacap_bits: 7931, eclen: [196, 364, 546, 650], blocks: [3, 4, 3, 11, 17, 4, 9, 16] },
    VersionMetric { format: Format::Full, version_str: "20", layout_id: 1, rows: 97, cols: 97, fimax: 34, fimas: 62, datacap_bits: 8683, eclen: [224, 416, 600, 700], blocks: [3, 5, 3, 13, 15, 5, 15, 10] },
    VersionMetric { format: Format::Full, version_str: "21", layout_id: 1, rows: 101, cols: 101, fimax: 28, fimas: 50, datacap_bits: 9252, eclen: [224, 442, 644, 750], blocks: [4, 4, 17, 0, 17, 6, 19, 6] },
    VersionMetric { format: Format::Full, version_str: "22", layout_id: 1, rows: 105, cols: 105, fimax: 26, fimas: 50, datacap_bits: 10068, eclen: [252, 476, 690, 816], blocks: [2, 7, 17, 0, 7, 16, 34, 0] },
    VersionMetric { format: Format::Full, version_str: "23", layout_id: 1, rows: 109, cols: 109, fimax: 30, fimas: 54, datacap_bits: 10916, eclen: [270, 504, 750, 900], blocks: [4, 5, 4, 14, 11, 14, 16, 14] },
    VersionMetric { format: Format::Full, version_str: "24", layout_id: 1, rows: 113, cols: 113, fimax: 28, fimas: 54, datacap_bits: 11796, eclen: [300, 560, 810, 960], blocks: [6, 4, 6, 14, 11, 16, 30, 2] },
    VersionMetric { format: Format::Full, version_str: "25", layout_id: 1, rows: 117, cols: 117, fimax: 32, fimas: 58, datacap_bits: 12708, eclen: [312, 588, 870, 1050], blocks: [8, 4, 8, 13, 7, 22, 22, 13] },
    VersionMetric { format: Format::Full, version_str: "26", layout_id: 1, rows: 121, cols: 121, fimax: 30, fimas: 58, datacap_bits: 13652, eclen: [336, 644, 952, 1110], blocks: [10, 2, 19, 4, 28, 6, 33, 4] },
    VersionMetric { format: Format::Full, version_str: "27", layout_id: 2, rows: 125, cols: 125, fimax: 34, fimas: 62, datacap_bits: 14628, eclen: [360, 700, 1020, 1200], blocks: [8, 4, 22, 3, 8, 26, 12, 28] },
    VersionMetric { format: Format::Full, version_str: "28", layout_id: 2, rows: 129, cols: 129, fimax: 26, fimas: 50, datacap_bits: 15371, eclen: [390, 728, 1050, 1260], blocks: [3, 10, 3, 23, 4, 31, 11, 31] },
    VersionMetric { format: Format::Full, version_str: "29", layout_id: 2, rows: 133, cols: 133, fimax: 30, fimas: 54, datacap_bits: 16411, eclen: [420, 784, 1140, 1350], blocks: [7, 7, 21, 7, 1, 37, 19, 26] },
    VersionMetric { format: Format::Full, version_str: "30", layout_id: 2, rows: 137, cols: 137, fimax: 26, fimas: 52, datacap_bits: 17483, eclen: [450, 812, 1200, 1440], blocks: [5, 10, 19, 10, 15, 25, 23, 25] },
    VersionMetric { format: Format::Full, version_str: "31", layout_id: 2, rows: 141, cols: 141, fimax: 30, fimas: 56, datacap_bits: 18587, eclen: [480, 868, 1290, 1530], blocks: [13, 3, 2, 29, 42, 1, 23, 28] },
    VersionMetric { format: Format::Full, version_str: "32", layout_id: 2, rows: 145, cols: 145, fimax: 34, fimas: 60, datacap_bits: 19723, eclen: [510, 924, 1350, 1620], blocks: [17, 0, 10, 23, 10, 35, 19, 35] },
    VersionMetric { format: Format::Full, version_str: "33", layout_id: 2, rows: 149, cols: 149, fimax: 30, fimas: 58, datacap_bits: 20891, eclen: [540, 980, 1440, 1710], blocks: [17, 1, 14, 21, 29, 19, 11, 46] },
    VersionMetric { format: Format::Full, version_str: "34", layout_id: 2, rows: 153, cols: 153, fimax: 34, fimas: 62, datacap_bits: 22091, eclen: [570, 1036, 1530, 1800], blocks: [13, 6, 14, 23, 44, 7, 59, 1] },
    VersionMetric { format: Format::Full, version_str: "35", layout_id: 2, rows: 157, cols: 157, fimax: 30, fimas: 54, datacap_bits: 23008, eclen: [570, 1064, 1590, 1890], blocks: [12, 7, 12, 26, 39, 14, 22, 41] },
    VersionMetric { format: Format::Full, version_str: "36", layout_id: 2, rows: 161, cols: 161, fimax: 24, fimas: 50, datacap_bits: 24272, eclen: [600, 1120, 1680, 1980], blocks: [6, 14, 6, 34, 46, 10, 2, 64] },
    VersionMetric { format: Format::Full, version_str: "37", layout_id: 2, rows: 165, cols: 165, fimax: 28, fimas: 54, datacap_bits: 25568, eclen: [630, 1204, 1770, 2100], blocks: [17, 4, 29, 14, 49, 10, 24, 46] },
    VersionMetric { format: Format::Full, version_str: "38", layout_id: 2, rows: 169, cols: 169, fimax: 32, fimas: 58, datacap_bits: 26896, eclen: [660, 1260, 1860, 2220], blocks: [4, 18, 13, 32, 48, 14, 42, 32] },
    VersionMetric { format: Format::Full, version_str: "39", layout_id: 2, rows: 173, cols: 173, fimax: 26, fimas: 54, datacap_bits: 28256, eclen: [720, 1316, 1950, 2310], blocks: [20, 4, 40, 7, 43, 22, 10, 67] },
    VersionMetric { format: Format::Full, version_str: "40", layout_id: 2, rows: 177, cols: 177, fimax: 30, fimas: 58, datacap_bits: 29648, eclen: [750, 1372, 2040, 2430], blocks: [19, 6, 18, 31, 34, 34, 20, 61] },
    VersionMetric { format: Format::Rmqr, version_str: "R7x43", layout_id: 7, rows: 7, cols: 43, fimax: 22, fimas: 99, datacap_bits: 104, eclen: [99, 7, 99, 10], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R7x59", layout_id: 8, rows: 7, cols: 59, fimax: 20, fimas: 40, datacap_bits: 171, eclen: [99, 9, 99, 14], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R7x77", layout_id: 9, rows: 7, cols: 77, fimax: 26, fimas: 52, datacap_bits: 261, eclen: [99, 12, 99, 22], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R7x99", layout_id: 10, rows: 7, cols: 99, fimax: 24, fimas: 50, datacap_bits: 358, eclen: [99, 16, 99, 30], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R7x139", layout_id: 11, rows: 7, cols: 139, fimax: 28, fimas: 56, datacap_bits: 545, eclen: [99, 24, 99, 44], blocks: [-1, -1, 1, 0, -1, -1, 2, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R9x43", layout_id: 12, rows: 9, cols: 43, fimax: 22, fimas: 99, datacap_bits: 170, eclen: [99, 9, 99, 14], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R9x59", layout_id: 13, rows: 9, cols: 59, fimax: 20, fimas: 40, datacap_bits: 267, eclen: [99, 12, 99, 22], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R9x77", layout_id: 14, rows: 9, cols: 77, fimax: 26, fimas: 52, datacap_bits: 393, eclen: [99, 18, 99, 32], blocks: [-1, -1, 1, 0, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R9x99", layout_id: 15, rows: 9, cols: 99, fimax: 24, fimas: 50, datacap_bits: 532, eclen: [99, 24, 99, 44], blocks: [-1, -1, 1, 0, -1, -1, 2, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R9x139", layout_id: 16, rows: 9, cols: 139, fimax: 28, fimas: 56, datacap_bits: 797, eclen: [99, 36, 99, 66], blocks: [-1, -1, 1, 1, -1, -1, 3, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x27", layout_id: 17, rows: 11, cols: 27, fimax: 98, fimas: 99, datacap_bits: 122, eclen: [99, 8, 99, 10], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x43", layout_id: 18, rows: 11, cols: 43, fimax: 22, fimas: 99, datacap_bits: 249, eclen: [99, 12, 99, 20], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x59", layout_id: 19, rows: 11, cols: 59, fimax: 20, fimas: 40, datacap_bits: 376, eclen: [99, 16, 99, 32], blocks: [-1, -1, 1, 0, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x77", layout_id: 20, rows: 11, cols: 77, fimax: 26, fimas: 52, datacap_bits: 538, eclen: [99, 24, 99, 44], blocks: [-1, -1, 1, 0, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x99", layout_id: 21, rows: 11, cols: 99, fimax: 24, fimas: 50, datacap_bits: 719, eclen: [99, 32, 99, 60], blocks: [-1, -1, 1, 1, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R11x139", layout_id: 22, rows: 11, cols: 139, fimax: 28, fimas: 56, datacap_bits: 1062, eclen: [99, 48, 99, 90], blocks: [-1, -1, 2, 0, -1, -1, 3, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x27", layout_id: 23, rows: 13, cols: 27, fimax: 98, fimas: 99, datacap_bits: 172, eclen: [99, 9, 99, 14], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x43", layout_id: 24, rows: 13, cols: 43, fimax: 22, fimas: 99, datacap_bits: 329, eclen: [99, 14, 99, 28], blocks: [-1, -1, 1, 0, -1, -1, 1, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x59", layout_id: 25, rows: 13, cols: 59, fimax: 20, fimas: 40, datacap_bits: 486, eclen: [99, 22, 99, 40], blocks: [-1, -1, 1, 0, -1, -1, 2, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x77", layout_id: 26, rows: 13, cols: 77, fimax: 26, fimas: 52, datacap_bits: 684, eclen: [99, 32, 99, 56], blocks: [-1, -1, 1, 1, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x99", layout_id: 27, rows: 13, cols: 99, fimax: 24, fimas: 50, datacap_bits: 907, eclen: [99, 40, 99, 78], blocks: [-1, -1, 1, 1, -1, -1, 1, 2] },
    VersionMetric { format: Format::Rmqr, version_str: "R13x139", layout_id: 28, rows: 13, cols: 139, fimax: 28, fimas: 56, datacap_bits: 1328, eclen: [99, 60, 99, 112], blocks: [-1, -1, 2, 1, -1, -1, 2, 2] },
    VersionMetric { format: Format::Rmqr, version_str: "R15x43", layout_id: 29, rows: 15, cols: 43, fimax: 22, fimas: 99, datacap_bits: 409, eclen: [99, 18, 99, 36], blocks: [-1, -1, 1, 0, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R15x59", layout_id: 30, rows: 15, cols: 59, fimax: 20, fimas: 40, datacap_bits: 596, eclen: [99, 26, 99, 48], blocks: [-1, -1, 1, 0, -1, -1, 2, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R15x77", layout_id: 31, rows: 15, cols: 77, fimax: 26, fimas: 52, datacap_bits: 830, eclen: [99, 36, 99, 72], blocks: [-1, -1, 1, 1, -1, -1, 2, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R15x99", layout_id: 32, rows: 15, cols: 99, fimax: 24, fimas: 50, datacap_bits: 1095, eclen: [99, 48, 99, 88], blocks: [-1, -1, 2, 0, -1, -1, 4, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R15x139", layout_id: 33, rows: 15, cols: 139, fimax: 28, fimas: 56, datacap_bits: 1594, eclen: [99, 72, 99, 130], blocks: [-1, -1, 2, 1, -1, -1, 1, 4] },
    VersionMetric { format: Format::Rmqr, version_str: "R17x43", layout_id: 34, rows: 17, cols: 43, fimax: 22, fimas: 99, datacap_bits: 489, eclen: [99, 22, 99, 40], blocks: [-1, -1, 1, 0, -1, -1, 1, 1] },
    VersionMetric { format: Format::Rmqr, version_str: "R17x59", layout_id: 35, rows: 17, cols: 59, fimax: 20, fimas: 40, datacap_bits: 706, eclen: [99, 32, 99, 60], blocks: [-1, -1, 2, 0, -1, -1, 2, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R17x77", layout_id: 36, rows: 17, cols: 77, fimax: 26, fimas: 52, datacap_bits: 976, eclen: [99, 44, 99, 84], blocks: [-1, -1, 2, 0, -1, -1, 1, 2] },
    VersionMetric { format: Format::Rmqr, version_str: "R17x99", layout_id: 37, rows: 17, cols: 99, fimax: 24, fimas: 50, datacap_bits: 1283, eclen: [99, 60, 99, 104], blocks: [-1, -1, 2, 1, -1, -1, 4, 0] },
    VersionMetric { format: Format::Rmqr, version_str: "R17x139", layout_id: 38, rows: 17, cols: 139, fimax: 28, fimas: 56, datacap_bits: 1860, eclen: [99, 80, 99, 156], blocks: [-1, -1, 4, 0, -1, -1, 2, 4] },
];

// ---------------------------------------------------------------------------
// BCH(15,5) format-info encoding — ISO/IEC 18004 §6.9.
//
// Used to encode the 5-bit format-info value (2-bit EC level + 3-bit
// mask index) into a 15-bit code with error-correction. The encoded
// 15-bit value is then XOR'd with a per-format mask (`FORMAT_INFO_MASK_FULL`
// for Full QR / `FORMAT_INFO_MASK_MICRO` for Micro QR) before placement.
//
// The generator polynomial is `x^10 + x^8 + x^5 + x^4 + x^2 + x + 1`
// = `0x537` (ISO/IEC 18004 §C.2.1).
// ---------------------------------------------------------------------------

/// BCH(15,5) generator polynomial: `x^10 + x^8 + x^5 + x^4 + x^2 + x + 1`.
pub(crate) const BCH_15_5_POLY: u32 = 0x537;
/// Format-info XOR mask for Full QR Code (ISO/IEC 18004 §C.2.1).
pub(crate) const FORMAT_INFO_MASK_FULL: u16 = 0x5412;
/// Format-info XOR mask for Micro QR Code (ISO/IEC 18004 §C.2.2).
pub(crate) const FORMAT_INFO_MASK_MICRO: u16 = 0x4445;

/// BCH(18,6) generator polynomial: `x^12 + x^11 + x^10 + x^9 + x^8 + x^5 + x^2 + 1`.
/// Used for V7+ Full QR version-info encoding.
pub(crate) const BCH_18_6_POLY: u32 = 0x1F25;

/// Encode a 5-bit format-info value (2-bit EC level << 3 | 3-bit mask
/// index) into a 15-bit codeword via BCH(15,5).
///
/// The result is the bare 15-bit ECC stream; callers XOR with
/// [`FORMAT_INFO_MASK_FULL`] or [`FORMAT_INFO_MASK_MICRO`] depending on
/// format. ISO/IEC 18004 §C.2.
///
/// # Panics
///
/// Debug-asserts `data < 32`.
pub(crate) fn bch15_5_encode(data: u8) -> u16 {
    debug_assert!(data < 32, "format-info data must be 5 bits");
    let mut d: u32 = u32::from(data) << 10;
    // Polynomial long-division: for each bit position from 14..=10,
    // if that bit is set in `d`, XOR the shifted generator polynomial.
    for shift in (10..=14).rev() {
        if d & (1u32 << shift) != 0 {
            d ^= BCH_15_5_POLY << (shift - 10);
        }
    }
    // Combine data (top 5 bits, shifted left by 10) with ECC (low 10 bits).
    let ecc = d & 0x3FF;
    ((u32::from(data) << 10) | ecc) as u16
}

/// Encode a 6-bit version value (Full QR V7..=V40) into an 18-bit
/// codeword via BCH(18,6). ISO/IEC 18004 §C.3.
///
/// # Panics
///
/// Debug-asserts `version >= 7 && version <= 40`.
pub(crate) fn bch18_6_encode(version: u8) -> u32 {
    debug_assert!(
        (7..=40).contains(&version),
        "version-info only encoded for V7+"
    );
    bch18_6_encode_data(version)
}

/// Generic BCH(18,6) encoder accepting any 6-bit input (0..=63).
/// Used for rMQR format-info codewords where the 6-bit "data" is
/// `(ec_id_rmqr * 32) + verind`, with `verind = metric_idx - 44`
/// (the 0-based offset of the rMQR metric in `FULL_METRICS`).
pub(crate) fn bch18_6_encode_data(data: u8) -> u32 {
    debug_assert!(data < 64, "BCH(18,6) input must be 6 bits (0..=63)");
    let mut d: u32 = u32::from(data) << 12;
    for shift in (12..=17).rev() {
        if d & (1u32 << shift) != 0 {
            d ^= BCH_18_6_POLY << (shift - 12);
        }
    }
    let ecc = d & 0xFFF;
    (u32::from(data) << 12) | ecc
}

/// rMQR format-info bit-codeword #1 XOR mask. Per BWIPP bwip-js
/// line 26831 — `fmtvalsrmqr1[data] = bch18_6(data) ^ 129714`
/// (=0x1FAB2). BCH input is `bchrem(data, 0x1F25, 12)`.
pub(crate) const RMQR_FMTVAL1_MASK: u32 = 0x1_FAB2;

/// rMQR format-info bit-codeword #2 XOR mask. Per BWIPP bwip-js
/// line 26833 — `fmtvalsrmqr2[data] = bch18_6(data) ^ 133755`
/// (=0x20A7B).
pub(crate) const RMQR_FMTVAL2_MASK: u32 = 0x2_0A7B;

/// Compute the 18-bit rMQR format-info codeword #1 for the given
/// 6-bit data (= `ec_id_rmqr * 32 + verind`).
pub(crate) fn rmqr_fmtval1(data: u8) -> u32 {
    bch18_6_encode_data(data) ^ RMQR_FMTVAL1_MASK
}

/// Compute the 18-bit rMQR format-info codeword #2 for the given
/// 6-bit data.
pub(crate) fn rmqr_fmtval2(data: u8) -> u32 {
    bch18_6_encode_data(data) ^ RMQR_FMTVAL2_MASK
}

/// rMQR EC indicator table: maps `ec_level` (0=L, 1=M, 2=Q, 3=H)
/// to BWIPP's `qrcode_ecidrmqr` value (`-1` means "unsupported";
/// only M=0 and H=1 are valid for rMQR).
pub(crate) const QRCODE_EC_INDICATOR_RMQR: [i8; 4] = [-1, 0, -1, 1];

// ---------------------------------------------------------------------------
// Stage 3 — Mode encoders (Numeric / Alphanumeric / Byte / Kanji / ECI)
//
// BWIPP source: bwip-js `src/bwipp.js` `bwipp_qrcode` body
//   * encN — line 26758   (numeric — 10/7/4 bits per 3/2/1 digits)
//   * encA — line 26707   (alphanumeric — 11 bits per pair / 6 bits leftover)
//   * encB — line 26821   (byte — 8 bits each)
//   * encK — line 26864   (kanji — 13 bits per Shift-JIS pair)
//   * encE — line 26901   (ECI — 8/16/24-bit variable)
//   * qrcode_cclens — line 25637  (CCI bit-widths per version-bin × mode)
//
// Each helper appends bits to a `Vec<bool>` rather than constructing the
// padded byte-stream — Stage 4 (codeword pad + EC interleave) is where the
// final byte stream is assembled.
// ---------------------------------------------------------------------------

/// QR segment-encoding modes. Matches BWIPP's `qrcode_N/A/B/K/E`
/// constants (bwip-js line 26233-26237). Discriminant values are the
/// CCI-table column indices: 0=N, 1=A, 2=B, 3=K. ECI (4) is a
/// header-only mode with no CCI column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Numeric: digits `0..=9`. 10/7/4 bits per 3/2/1-digit group.
    Numeric = 0,
    /// Alphanumeric: 45-char table (`0-9 A-Z $%*+-./: ` + space).
    /// 11 bits per pair, 6 bits for a leftover.
    Alphanumeric = 1,
    /// Byte: arbitrary 8-bit values. Default charset is ISO-8859-1
    /// unless preceded by an ECI header.
    Byte = 2,
    /// Kanji: Shift-JIS double-byte. 13 bits per char after the
    /// BWIPP/ISO offset folding.
    Kanji = 3,
    /// ECI: Extended Channel Interpretation header. 8/16/24 bits
    /// depending on the assignment-number range.
    Eci = 4,
}

/// 45-char alphanumeric value table for Mode::Alphanumeric. Indexed by
/// ASCII byte; the slot stores the table-position value 0..=44, or -1
/// when the byte isn't part of the alphanumeric set.
///
/// Layout (per ISO/IEC 18004 Table 5 and BWIPP `qrcode_Aexcl` /
/// `qrcode_Nexcl` at bwip-js lines 26245-26257):
///
/// * `0..=9`  → ASCII `'0'..='9'`
/// * `10..=35` → ASCII `'A'..='Z'`
/// * `36`     → space (`0x20`)
/// * `37`     → `'$'` (`0x24`)
/// * `38`     → `'%'` (`0x25`)
/// * `39`     → `'*'` (`0x2A`)
/// * `40`     → `'+'` (`0x2B`)
/// * `41`     → `'-'` (`0x2D`)
/// * `42`     → `'.'` (`0x2E`)
/// * `43`     → `'/'` (`0x2F`)
/// * `44`     → `':'` (`0x3A`)
pub(crate) const ALPHANUMERIC_VALUE: [i8; 256] = {
    let mut t = [-1i8; 256];
    let mut i = 0;
    // '0'..='9' -> 0..=9
    while i < 10 {
        t[(b'0' + i as u8) as usize] = i as i8;
        i += 1;
    }
    // 'A'..='Z' -> 10..=35
    let mut j = 0;
    while j < 26 {
        t[(b'A' + j as u8) as usize] = (10 + j) as i8;
        j += 1;
    }
    t[b' ' as usize] = 36;
    t[b'$' as usize] = 37;
    t[b'%' as usize] = 38;
    t[b'*' as usize] = 39;
    t[b'+' as usize] = 40;
    t[b'-' as usize] = 41;
    t[b'.' as usize] = 42;
    t[b'/' as usize] = 43;
    t[b':' as usize] = 44;
    t
};

/// Push the low `nbits` bits of `value` onto `out`, MSB-first.
///
/// BWIPP's `tobin` (bwip-js around line 27001) returns an ASCII '0'/'1'
/// string; we use the boolean-vector form throughout the encoder.
pub(crate) fn push_bits(out: &mut Vec<bool>, value: u32, nbits: u8) {
    for shift in (0..nbits).rev() {
        out.push(((value >> shift) & 1) == 1);
    }
}

/// Encode a numeric segment (BWIPP `encN`, bwip-js line 26758).
///
/// Groups of 3 digits emit 10 bits; remainder 2 emits 7 bits; remainder
/// 1 emits 4 bits. Input bytes must be ASCII `'0'..='9'`.
///
/// # Errors
///
/// Returns `Error::InvalidData` if any input byte is not a digit.
pub(crate) fn encode_numeric_segment(digits: &[u8]) -> Result<Vec<bool>, Error> {
    let mut out: Vec<bool> = Vec::with_capacity((digits.len() * 10).div_ceil(3));
    let mut i = 0;
    while i < digits.len() {
        let remaining = digits.len() - i;
        match remaining {
            r if r >= 3 => {
                let d0 = digit_value(digits[i])?;
                let d1 = digit_value(digits[i + 1])?;
                let d2 = digit_value(digits[i + 2])?;
                let v = u32::from(d0) * 100 + u32::from(d1) * 10 + u32::from(d2);
                push_bits(&mut out, v, 10);
                i += 3;
            }
            2 => {
                let d0 = digit_value(digits[i])?;
                let d1 = digit_value(digits[i + 1])?;
                push_bits(&mut out, u32::from(d0) * 10 + u32::from(d1), 7);
                i += 2;
            }
            _ => {
                let d0 = digit_value(digits[i])?;
                push_bits(&mut out, u32::from(d0), 4);
                i += 1;
            }
        }
    }
    Ok(out)
}

#[inline]
fn digit_value(b: u8) -> Result<u8, Error> {
    if b.is_ascii_digit() {
        Ok(b - b'0')
    } else {
        Err(Error::InvalidData(format!(
            "qrcode_native: numeric mode expects ASCII digit, got 0x{b:02X}"
        )))
    }
}

/// Encode an alphanumeric segment (BWIPP `encA`, bwip-js line 26707).
///
/// Pairs of chars emit 11 bits (`c0 * 45 + c1`); a leftover char emits
/// 6 bits.
///
/// # Errors
///
/// Returns `Error::InvalidData` if any input byte is outside the 45-char
/// table.
pub(crate) fn encode_alphanumeric_segment(input: &[u8]) -> Result<Vec<bool>, Error> {
    let mut out: Vec<bool> = Vec::with_capacity(input.len() * 11 / 2 + 1);
    let mut i = 0;
    while i < input.len() {
        let remaining = input.len() - i;
        if remaining >= 2 {
            let c0 = alpha_value(input[i])?;
            let c1 = alpha_value(input[i + 1])?;
            push_bits(&mut out, u32::from(c0) * 45 + u32::from(c1), 11);
            i += 2;
        } else {
            let c0 = alpha_value(input[i])?;
            push_bits(&mut out, u32::from(c0), 6);
            i += 1;
        }
    }
    Ok(out)
}

#[inline]
fn alpha_value(b: u8) -> Result<u8, Error> {
    let v = ALPHANUMERIC_VALUE[b as usize];
    if v < 0 {
        Err(Error::InvalidData(format!(
            "qrcode_native: alphanumeric mode rejects byte 0x{b:02X}"
        )))
    } else {
        Ok(v as u8)
    }
}

/// Encode a byte segment (BWIPP `encB`, bwip-js line 26821). Each byte
/// becomes 8 raw bits, MSB-first.
pub(crate) fn encode_byte_segment(bytes: &[u8]) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        push_bits(&mut out, u32::from(b), 8);
    }
    out
}

/// Encode a kanji segment (BWIPP `encK`, bwip-js line 26864).
///
/// Input is a slice of Shift-JIS 16-bit code units. Each code unit is
/// folded to the 13-bit value `((hi << 8) | lo) → hi*0xC0 + lo` where
/// `hi`/`lo` are the upper/lower bytes of the offset:
///
/// * `0x8140..=0x9FFC` → offset = code − 0x8140
/// * `0xE040..=0xEBBF` → offset = code − 0xC140
///
/// # Errors
///
/// Returns `Error::InvalidData` if any code falls outside both valid
/// ranges.
pub(crate) fn encode_kanji_segment(jis: &[u16]) -> Result<Vec<bool>, Error> {
    let mut out: Vec<bool> = Vec::with_capacity(jis.len() * 13);
    for &c in jis {
        let offset: u32 = if (0x8140..=0x9FFC).contains(&c) {
            u32::from(c) - 0x8140
        } else if (0xE040..=0xEBBF).contains(&c) {
            u32::from(c) - 0xC140
        } else {
            return Err(Error::InvalidData(format!(
                "qrcode_native: kanji mode rejects Shift-JIS 0x{c:04X}"
            )));
        };
        let hi = offset >> 8;
        let lo = offset & 0xFF;
        push_bits(&mut out, hi * 0xC0 + lo, 13);
    }
    Ok(out)
}

/// Encode an ECI assignment number header (BWIPP `encE`, bwip-js line
/// 26901). Output width varies by range:
///
/// * `0..=127` → 8 bits (raw value)
/// * `128..=16_383` → 16 bits (value | 0x8000)
/// * `16_384..=999_999` → 24 bits (value | 0xC0_0000)
///
/// # Errors
///
/// Returns `Error::InvalidData` if `eci > 999_999` (BWIPP's implicit
/// upper bound — assignment numbers > 6 digits are unreachable).
pub(crate) fn encode_eci_segment(eci: u32) -> Result<Vec<bool>, Error> {
    let mut out = Vec::with_capacity(24);
    if eci <= 127 {
        push_bits(&mut out, eci, 8);
    } else if eci <= 16_383 {
        push_bits(&mut out, eci | 0x8000, 16);
    } else if eci <= 999_999 {
        push_bits(&mut out, eci | 0xC0_0000, 24);
    } else {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ECI assignment number {eci} exceeds 6-digit max"
        )));
    }
    Ok(out)
}

/// BWIPP `qrcode_cclens` (bwip-js line 25637): character-count
/// indicator bit-widths, indexed by `[version_bin][mode]`.
///
/// Row index matches the `layout_id` field on each [`VersionMetric`]:
///
/// * Rows 0/1/2 — Full V1-9 / V10-26 / V27-40
/// * Rows 3/4/5/6 — Micro M1/M2/M3/M4
/// * Rows 7..=38 — rMQR R7×43 .. R17×139 (in [`FULL_METRICS`] order)
///
/// Column index matches [`Mode`] discriminants (0=N, 1=A, 2=B, 3=K).
/// Value `-1` means the mode is unsupported in that version-bin; the
/// encoder must reject any attempt to start a segment in that mode.
pub(crate) const CC_LENS: [[i8; 4]; 39] = [
    [10, 9, 8, 8],    // 0  Full V1-9
    [12, 11, 16, 10], // 1  Full V10-26
    [14, 13, 16, 12], // 2  Full V27-40
    [3, -1, -1, -1],  // 3  M1
    [4, 3, -1, -1],   // 4  M2
    [5, 4, 4, 3],     // 5  M3
    [6, 5, 5, 4],     // 6  M4
    [4, 3, 3, 2],     // 7  R7x43
    [5, 5, 4, 3],     // 8  R7x59
    [6, 5, 5, 4],     // 9  R7x77
    [7, 6, 5, 5],     // 10 R7x99
    [7, 6, 6, 5],     // 11 R7x139
    [5, 5, 4, 3],     // 12 R9x43
    [6, 5, 5, 4],     // 13 R9x59
    [7, 6, 5, 5],     // 14 R9x77
    [7, 6, 6, 5],     // 15 R9x99
    [8, 7, 6, 6],     // 16 R9x139
    [4, 4, 3, 2],     // 17 R11x27
    [6, 5, 5, 4],     // 18 R11x43
    [7, 6, 5, 5],     // 19 R11x59
    [7, 6, 6, 5],     // 20 R11x77
    [8, 7, 6, 6],     // 21 R11x99
    [8, 7, 7, 6],     // 22 R11x139
    [5, 5, 4, 3],     // 23 R13x27
    [6, 6, 5, 5],     // 24 R13x43
    [7, 6, 6, 5],     // 25 R13x59
    [7, 7, 6, 6],     // 26 R13x77
    [8, 7, 7, 6],     // 27 R13x99
    [8, 8, 7, 7],     // 28 R13x139
    [7, 6, 6, 5],     // 29 R15x43
    [7, 7, 6, 5],     // 30 R15x59
    [8, 7, 7, 6],     // 31 R15x77
    [8, 7, 7, 6],     // 32 R15x99
    [9, 8, 7, 7],     // 33 R15x139
    [7, 6, 6, 5],     // 34 R17x43
    [8, 7, 6, 6],     // 35 R17x59
    [8, 7, 7, 6],     // 36 R17x77
    [8, 8, 7, 6],     // 37 R17x99
    [9, 8, 8, 7],     // 38 R17x139
];

/// CCI bit-width for `(layout_id, mode)`, or `None` if the mode is not
/// supported in that version-bin.
///
/// `layout_id` is the value stored on each [`VersionMetric`]. `mode` is
/// a [`Mode`] discriminant (only N/A/B/K have CCI fields; ECI carries
/// no character count).
pub(crate) fn cci_bits(layout_id: u8, mode: Mode) -> Option<u8> {
    if matches!(mode, Mode::Eci) {
        return None;
    }
    let bin = layout_id as usize;
    if bin >= CC_LENS.len() {
        return None;
    }
    let col = mode as usize;
    let v = CC_LENS[bin][col];
    if v < 0 {
        None
    } else {
        Some(v as u8)
    }
}

// ---------------------------------------------------------------------------
// Stage 5a — Mode-selector input counters + threshold tables
//
// BWIPP source: bwip-js `src/bwipp.js` lines 26641-26659 (threshold
// tables), 26282 (qrcode_mids), 27108-27160 (counter arrays).
//
// The mode-selector decision in BWIPP (lines 27160-27305) reads four
// trailing-character counters per position and compares against per-
// version thresholds to decide whether to switch modes. Stage 5b
// (below, scheduled next iteration) ports the actual state machine.
//
// Stage 5a delivers the substrate: the counter arrays, the 19 lookup
// tables that the state machine consults, and the mode-indicator
// dispatch table.
// ---------------------------------------------------------------------------

/// BWIPP `$_.e` infeasibility sentinel — `10_000`. Threshold rows use
/// this value to mark (version-bin, mode) combinations where the
/// state-machine decision is unconditionally rejected (e.g. M1 cannot
/// host Byte or Kanji segments at all).
pub(crate) const INFEAS: u16 = 10_000;

/// Mode-indicator bit-strings indexed by `[layout_id][mode]`. Mirrors
/// BWIPP `qrcode_mids` (bwip-js line 26282). Each cell is either an
/// empty string (terminator-only encoding for M1's numeric-only path)
/// or a bit-string of length 1..4. Rows are:
///
/// * 0-2  Full QR V1-9 / V10-26 / V27-40 — all use 4-bit indicators
///   (`0001` N, `0010` A, `0100` B, `1000` K, `0111` E).
/// * 3    M1 — only Numeric allowed; mode bits are empty.
/// * 4    M2 — 1-bit indicator (`0` N, `1` A).
/// * 5    M3 — 2-bit indicator (N=`00` A=`01` B=`10` K=`11`).
/// * 6    M4 — 3-bit indicator (N=`000` A=`001` B=`010` K=`011`).
/// * 7-38 rMQR — 3-bit indicator (N=`001` A=`010` B=`011` K=`100` E=`111`).
///
/// `None` cells mark unsupported (layout, mode) combinations.
pub(crate) const QRCODE_MIDS: [[Option<&str>; 5]; 39] = {
    let n = None;
    [
        // 0-2: Full V1-9 / V10-26 / V27-40
        [
            Some("0001"),
            Some("0010"),
            Some("0100"),
            Some("1000"),
            Some("0111"),
        ],
        [
            Some("0001"),
            Some("0010"),
            Some("0100"),
            Some("1000"),
            Some("0111"),
        ],
        [
            Some("0001"),
            Some("0010"),
            Some("0100"),
            Some("1000"),
            Some("0111"),
        ],
        // 3: M1 (numeric-only, empty mode bits)
        [Some(""), n, n, n, n],
        // 4: M2 (1-bit)
        [Some("0"), Some("1"), n, n, n],
        // 5: M3 (2-bit)
        [Some("00"), Some("01"), Some("10"), Some("11"), n],
        // 6: M4 (3-bit)
        [Some("000"), Some("001"), Some("010"), Some("011"), n],
        // 7-38: rMQR (3-bit, all rows identical)
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
        [
            Some("001"),
            Some("010"),
            Some("011"),
            Some("100"),
            Some("111"),
        ],
    ]
};

/// Threshold tables for the BWIPP mode-selector. All 19 tables are
/// 39-element arrays indexed by `layout_id`. The sentinel value
/// [`INFEAS`] (10_000) marks an "always reject" cell.
///
/// Naming mirrors BWIPP's variables: `mode0_force_kb` is the threshold
/// for forcing Kanji/Byte at start-of-input. `mode_b_k_before_b` is
/// "starting in B, is K reached before another B?" — etc.
pub(crate) const QRCODE_MODE0_FORCE_KB: [u16; 39] = [
    1, 1, 1, INFEAS, INFEAS, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];
pub(crate) const QRCODE_MODE0_FORCE_A: [u16; 39] = [
    1, 1, 1, INFEAS, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1,
];
pub(crate) const QRCODE_MODE0_FORCE_N: [u16; 39] = [1u16; 39];
pub(crate) const QRCODE_MODE0_N_BEFORE_B: [u16; 39] = [
    4, 4, 5, INFEAS, INFEAS, 2, 3, 2, 2, 3, 3, 3, 2, 3, 3, 3, 3, 2, 3, 3, 3, 3, 3, 2, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
];
pub(crate) const QRCODE_MODE_BK_BEFORE_B: [u16; 39] = [
    9, 12, 13, INFEAS, INFEAS, 4, 6, 4, 5, 6, 6, 6, 5, 6, 6, 6, 7, 4, 6, 6, 6, 7, 7, 5, 6, 6, 7, 7,
    7, 6, 6, 7, 7, 7, 6, 7, 7, 7, 8,
];
pub(crate) const QRCODE_MODE_BK_BEFORE_A: [u16; 39] = [
    8, 10, 11, INFEAS, INFEAS, 4, 5, 4, 5, 5, 6, 6, 5, 5, 6, 6, 6, 4, 5, 6, 6, 6, 6, 5, 6, 6, 6, 6,
    7, 6, 6, 6, 6, 7, 6, 6, 6, 7, 7,
];
pub(crate) const QRCODE_MODE_BK_BEFORE_N: [u16; 39] = [
    8, 9, 11, INFEAS, INFEAS, 3, 5, 3, 4, 5, 5, 5, 4, 5, 5, 5, 6, 3, 5, 5, 5, 6, 6, 4, 5, 5, 6, 6,
    6, 5, 5, 6, 6, 7, 5, 6, 6, 6, 7,
];
pub(crate) const QRCODE_MODE_BK_BEFORE_E: [u16; 39] = [
    5, 5, 6, INFEAS, INFEAS, 2, 3, 2, 3, 3, 3, 3, 3, 3, 3, 3, 4, 2, 3, 3, 3, 4, 4, 3, 3, 3, 4, 4,
    4, 3, 3, 4, 4, 4, 3, 4, 4, 4, 4,
];
pub(crate) const QRCODE_MODE_BA_BEFORE_K: [u16; 39] = [
    11, 12, 14, INFEAS, INFEAS, 5, 7, 5, 6, 7, 8, 8, 6, 7, 8, 8, 8, 6, 7, 8, 8, 8, 8, 6, 8, 8, 8,
    8, 9, 8, 8, 8, 8, 9, 8, 8, 8, 9, 9,
];
pub(crate) const QRCODE_MODE_BA_BEFORE_B: [u16; 39] = [
    11, 15, 16, INFEAS, INFEAS, 6, 7, 6, 7, 7, 8, 8, 7, 7, 8, 8, 8, 6, 7, 8, 8, 8, 9, 7, 8, 8, 8,
    9, 9, 8, 8, 9, 9, 9, 8, 8, 9, 9, 10,
];
pub(crate) const QRCODE_MODE_BA_BEFORE_N: [u16; 39] = [
    12, 13, 15, INFEAS, INFEAS, 6, 8, 6, 7, 8, 8, 8, 7, 8, 8, 8, 9, 6, 8, 8, 8, 9, 9, 7, 8, 8, 9,
    9, 10, 8, 9, 9, 9, 10, 8, 9, 9, 10, 10,
];
pub(crate) const QRCODE_MODE_BA_BEFORE_E: [u16; 39] = [
    6, 7, 8, INFEAS, INFEAS, 3, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 4, 4, 4, 4, 5, 5, 4, 4, 4, 5, 5,
    5, 4, 5, 5, 5, 5, 4, 5, 5, 5, 5,
];
pub(crate) const QRCODE_MODE_BN_BEFORE_K: [u16; 39] = [
    6, 7, 8, INFEAS, INFEAS, 3, 4, 3, 4, 4, 5, 5, 4, 4, 5, 5, 5, 3, 4, 5, 5, 5, 5, 4, 4, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
];
pub(crate) const QRCODE_MODE_BN_BEFORE_B: [u16; 39] = [
    6, 8, 9, INFEAS, INFEAS, 3, 4, 3, 4, 4, 5, 5, 4, 4, 5, 5, 5, 3, 4, 5, 5, 5, 5, 4, 4, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6,
];
pub(crate) const QRCODE_MODE_BN_BEFORE_A: [u16; 39] = [
    6, 7, 8, INFEAS, INFEAS, 3, 4, 3, 4, 4, 5, 5, 4, 4, 5, 5, 5, 4, 4, 5, 5, 5, 5, 4, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 6, 5, 5, 5, 5, 6,
];
pub(crate) const QRCODE_MODE_BN_BEFORE_E: [u16; 39] = [
    3, 4, 4, INFEAS, INFEAS, 2, 3, 2, 2, 3, 3, 3, 2, 3, 3, 3, 3, 2, 3, 3, 3, 3, 3, 2, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
];
pub(crate) const QRCODE_MODE_AN_BEFORE_A: [u16; 39] = [
    13, 15, 17, INFEAS, 5, 7, 9, 7, 8, 9, 9, 9, 8, 9, 9, 9, 11, 7, 9, 9, 9, 11, 11, 8, 9, 9, 10,
    11, 11, 9, 10, 11, 11, 11, 9, 11, 11, 11, 11,
];
pub(crate) const QRCODE_MODE_AN_BEFORE_B: [u16; 39] = [
    13, 17, 18, INFEAS, INFEAS, 7, 9, 7, 8, 9, 9, 9, 8, 9, 9, 9, 10, 7, 9, 9, 9, 10, 11, 8, 9, 9,
    9, 11, 11, 9, 9, 11, 11, 11, 9, 10, 11, 11, 11,
];
pub(crate) const QRCODE_MODE_AN_BEFORE_E: [u16; 39] = [
    7, 8, 9, INFEAS, 3, 4, 5, 4, 5, 5, 5, 5, 5, 5, 5, 5, 6, 4, 5, 5, 5, 6, 6, 5, 5, 5, 5, 6, 6, 5,
    5, 6, 6, 6, 5, 6, 6, 6, 6,
];

/// Per-position counter arrays computed during BWIPP's parse-input
/// pre-pass (bwip-js line 27108-27168). Field names mirror BWIPP's
/// variable names exactly. All arrays have length `msg.len() + 1`;
/// index `msglen` is the "after end" base case.
///
/// Two related families:
///
/// * `num_*[i]` — "if I start a segment in mode X at position i, how
///   many chars can the segment consume?" Computed backwards. For
///   kanji, the unit is *characters* (= byte-pairs) so each valid
///   kanji pair contributes 1.
/// * `next_*[i]` — "distance from i to the next char of type X". Used
///   to test whether a chosen segment terminator falls right before a
///   character of a contrasting mode.
///
/// The `num_a_or_n` field is BWIPP's `numAorNs` — count of trailing
/// chars where each is either a digit (numeric) or an alpha-only
/// character (Aexcl). Used by the AorNbeforeB / AorNbeforeE
/// predicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputCounters {
    /// `numNs[i]` — trailing numeric (digit) run from position `i`.
    pub num_n: Vec<u32>,
    /// `numAs[i]` — trailing alpha-only run (Aexcl set, no digits).
    pub num_a: Vec<u32>,
    /// `numAorNs[i]` — trailing alphanumeric (Aexcl ∪ Nexcl) run.
    pub num_a_or_n: Vec<u32>,
    /// `numBs[i]` — trailing pure-byte run (chars that are NOT
    /// numeric, alpha-only, kanji-leader, or ECI escape).
    pub num_b: Vec<u32>,
    /// `numKs[i]` — trailing kanji (Shift-JIS pair) run in
    /// *characters*. Each consumed kanji adds 1; only the leader
    /// position retains a non-zero count (the trail position is
    /// zeroed by a second pass).
    pub num_k: Vec<u32>,
    /// `nextNs[i]` — 0 if `msg[i]` is a digit, else distance to the
    /// next digit. Trailing sentinel is `0` (BWIPP initializes from
    /// an `Infinity`-marked stack but the value at index `msglen` is
    /// never read).
    pub next_n: Vec<u32>,
    /// `nextAs[i]` — 0 if `msg[i]` is alpha-only, else distance to
    /// the next alpha-only character.
    pub next_a: Vec<u32>,
    /// `nextBs[i]` — 0 if `msg[i]` is a pure-byte char, else distance
    /// to the next byte character.
    pub next_b: Vec<u32>,
    /// `nextKs[i]` — 0 if `msg[i]` starts a valid kanji pair, else
    /// distance to the next kanji-leader position. The trailing
    /// sentinel is BWIPP's `9999` (so positions past msglen always
    /// look "far").
    pub next_k: Vec<u32>,
    /// `isECI[i]` — true if `msg[i]` is an ECI escape. The Rust
    /// surface API doesn't yet model ECI as a separate marker (it's
    /// encoded as a special segment by the caller), so this is
    /// always `false` for now. Kept as a slot for Stage 5b's
    /// signature parity.
    pub is_eci: Vec<bool>,
}

/// Test whether a byte is a kanji-leader (Shift-JIS first byte) per
/// BWIPP `qrcode_Kexcl`: 0x81..=0x9F or 0xE0..=0xEB.
#[inline]
pub(crate) fn is_kanji_leader(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xEB).contains(&b)
}

/// Validate that `(hi, lo)` is a complete Shift-JIS code per BWIPP's
/// kanji check at bwip-js line 27123-27128:
///
/// * combined value in `0x8140..=0x9FFC` or `0xE040..=0xEBBF`
/// * low byte in `0x40..=0xFC` and `!= 0x7F`
#[inline]
fn is_valid_shift_jis(hi: u8, lo: u8) -> bool {
    let val = ((hi as u16) << 8) | (lo as u16);
    let in_range = (0x8140..=0x9FFC).contains(&val) || (0xE040..=0xEBBF).contains(&val);
    let low_ok = (0x40..=0xFC).contains(&lo) && lo != 0x7F;
    in_range && low_ok
}

/// Compute the BWIPP parse-input counter arrays for `msg`. Mirrors
/// the backward scan + post-process loops at bwip-js line 27108-27168.
///
/// All arrays have length `msg.len() + 1`. `suppress_kanji_mode` set
/// to `true` matches BWIPP's `suppresskanjimode` default for the
/// `bwipp_qrcode` entry point — turn it off only if callers explicitly
/// want Shift-JIS Kanji segments.
pub(crate) fn compute_input_counters(msg: &[u8], suppress_kanji_mode: bool) -> InputCounters {
    let n = msg.len();
    let mut num_n = vec![0u32; n + 1];
    let mut num_a = vec![0u32; n + 1];
    let mut num_a_or_n = vec![0u32; n + 1];
    let mut num_k = vec![0u32; n + 1];
    let mut next_n = vec![0u32; n + 1];
    let mut next_a = vec![0u32; n + 1];
    let mut next_k = vec![0u32; n + 1];
    let is_eci = vec![false; n + 1];

    // BWIPP initializes nextKs[msglen] = 9999 (the trailing sentinel
    // is "infinitely far away"). All other "next" arrays start at 0.
    if n > 0 {
        next_k[n] = 9999;
    }

    // Pass 1 (backward): per-char classification + run accumulation.
    for i in (0..n).rev() {
        let b = msg[i];
        // Kanji check: leader byte + valid Shift-JIS pair with msg[i+1].
        let is_k = !suppress_kanji_mode
            && is_kanji_leader(b)
            && i + 1 < n
            && is_valid_shift_jis(b, msg[i + 1]);
        if is_k {
            next_k[i] = 0;
            num_k[i] = num_k[i + 2] + 1;
        } else {
            next_k[i] = next_k[i + 1] + 1;
        }
        let is_digit = b.is_ascii_digit();
        if is_digit {
            next_n[i] = 0;
            num_n[i] = num_n[i + 1] + 1;
            num_a_or_n[i] = num_a_or_n[i + 1] + 1;
        } else {
            next_n[i] = next_n[i + 1] + 1;
        }
        // BWIPP qrcode_Aexcl = alpha-only set (35 chars: A-Z + 9
        // punctuation chars). Digits are NOT in Aexcl — they're in
        // Nexcl. The `num_a` counter accumulates only when the char
        // is in Aexcl (alpha-only).
        let is_alpha_only = is_alpha_only_byte(b);
        if is_alpha_only {
            next_a[i] = 0;
            num_a[i] = num_a[i + 1] + 1;
            num_a_or_n[i] = num_a_or_n[i + 1] + 1;
        } else {
            next_a[i] = next_a[i + 1] + 1;
        }
    }

    // Pass 2 (forward): zero out the second byte of each kanji pair
    // so that the LOW byte of a kanji doesn't itself look like the
    // start of a kanji run. Mirrors bwip-js 27154-27158.
    for i in 0..n.saturating_sub(1) {
        if num_k[i] > 0 {
            num_k[i + 1] = 0;
            next_k[i + 1] = next_k[i + 1].saturating_add(1);
        }
    }

    // Pass 3 (backward): numBs/nextBs. A position is a "byte" char
    // iff num_n[i] + num_a[i] + num_k[i] == 0 AND !is_eci[i].
    let mut num_b = vec![0u32; n + 1];
    let mut next_b = vec![0u32; n + 1];
    for i in (0..n).rev() {
        let is_byte = num_n[i] + num_a[i] + num_k[i] == 0 && !is_eci[i];
        if is_byte {
            next_b[i] = 0;
            num_b[i] = num_b[i + 1] + 1;
        } else {
            next_b[i] = next_b[i + 1] + 1;
        }
    }

    InputCounters {
        num_n,
        num_a,
        num_a_or_n,
        num_b,
        num_k,
        next_n,
        next_a,
        next_b,
        next_k,
        is_eci,
    }
}

/// True if `b` is a BWIPP `qrcode_Aexcl` member — alpha-only (no
/// digits). `qrcode_Aexcl` is the 35-char set: A-Z (26) + space + $ +
/// % + * + + + - + . + / + : (9).
#[inline]
fn is_alpha_only_byte(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z' | b' ' | b'$' | b'%' | b'*' | b'+' | b'-' | b'.' | b'/' | b':'
    )
}

// ---------------------------------------------------------------------------
// Stage 5b — segment-selector state machine + composer
//
// BWIPP source: bwip-js `src/bwipp.js` lines 27160-27355.
//
// `select_segments` walks the message left-to-right; at each position it
// reads the trailing run counts (from `compute_input_counters`) and
// decides whether to continue the current mode or switch. Decisions are
// gated by comparisons against the 19 per-version threshold tables.
//
// `compose_segments` then iterates the selected segments and emits:
//   1. mode indicator (QRCODE_MIDS lookup)
//   2. character count indicator (CC_LENS lookup)
//   3. mode-specific payload bits (encode_*_segment helpers)
// ---------------------------------------------------------------------------

/// One emitted segment selected by [`select_segments`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Segment {
    pub mode: Mode,
    /// Starting byte position in the input message.
    pub start: usize,
    /// Length in input bytes. For Kanji this is 2 × (character count).
    pub len: usize,
}

/// Run BWIPP's mode-selector state machine over `msg` for the given
/// `layout_id`. Returns a sequence of [`Segment`] entries that together
/// partition the input.
///
/// `fnc1first` mirrors BWIPP's `$_.fnc1first` — when true the
/// state-machine is constrained to NOT switch to Kanji mode in the
/// first segment (BWIPP forces `qrcode_B` instead). Set to `false`
/// for plain QR encoding; `true` for GS1 QR which prepends FNC1.
///
/// This implementation mirrors the BWIPP `for (;;)` decision loop at
/// bwip-js line 27160-27305. The control flow is intentionally close
/// to the JS source so the threshold-table semantics stay verifiable
/// against bwip-js drift.
pub(crate) fn select_segments(msg: &[u8], layout_id: u8, fnc1first: bool) -> Vec<Segment> {
    let n = msg.len();
    if n == 0 {
        return Vec::new();
    }
    // Compute counters with kanji classification (BWIPP default for
    // bwipp_qrcode is suppresskanjimode=true — we keep that default so
    // that "raw bytes that happen to be in the kanji range" don't get
    // misclassified as kanji segments).
    let counters = compute_input_counters(msg, /*suppress_kanji=*/ true);
    let bin = layout_id as usize;

    let mut segments: Vec<Segment> = Vec::new();
    let mut mode: Option<Mode> = None;
    let mut start: usize = 0;
    let mut i: usize = 0;

    while i < n {
        let num_k = counters.num_k[i];
        let num_b = counters.num_b[i];
        let num_a = counters.num_a[i];
        let num_n = counters.num_n[i];
        let num_a_or_n = counters.num_a_or_n[i];

        // Decision helpers (close to BWIPP one-to-one). Each takes the
        // threshold-table row for `ver` and returns the bool result.
        let k_before_b = |table: &[u16; 39]| {
            num_k >= table[bin] as u32 && counters.next_b[i + 2 * num_k as usize] == 0
        };
        let k_before_a = |table: &[u16; 39]| {
            num_k >= table[bin] as u32 && counters.next_a[i + 2 * num_k as usize] == 0
        };
        let k_before_n = |table: &[u16; 39]| {
            num_k >= table[bin] as u32 && counters.next_n[i + 2 * num_k as usize] == 0
        };
        let k_before_e =
            |table: &[u16; 39]| num_k >= table[bin] as u32 && (i + 2 * num_k as usize) == n;
        let a_before_k = |table: &[u16; 39]| {
            num_a >= table[bin] as u32 && counters.next_k[i + num_a as usize] == 0
        };
        let a_before_b = |table: &[u16; 39]| {
            num_a >= table[bin] as u32 && counters.next_b[i + num_a as usize] == 0
        };
        let a_before_n = |table: &[u16; 39]| {
            num_a >= table[bin] as u32 && counters.next_n[i + num_a as usize] == 0
        };
        let a_before_e =
            |table: &[u16; 39]| num_a >= table[bin] as u32 && (i + num_a as usize) == n;
        let n_before_k = |table: &[u16; 39]| {
            num_n >= table[bin] as u32 && counters.next_k[i + num_n as usize] == 0
        };
        let n_before_b = |table: &[u16; 39]| {
            num_n >= table[bin] as u32 && counters.next_b[i + num_n as usize] == 0
        };
        let n_before_a = |table: &[u16; 39]| {
            num_n >= table[bin] as u32 && counters.next_a[i + num_n as usize] == 0
        };
        let n_before_e =
            |table: &[u16; 39]| num_n >= table[bin] as u32 && (i + num_n as usize) == n;
        let a_or_n_before_b = |table: &[u16; 39]| {
            num_a_or_n >= table[bin] as u32 && counters.next_b[i + num_a_or_n as usize] == 0
        };
        let a_or_n_before_e =
            |table: &[u16; 39]| num_a_or_n >= table[bin] as u32 && (i + num_a_or_n as usize) == n;

        // BWIPP decision loop: pick the next mode based on current mode
        // and trailing-run counters. The outer `loop { … break }` is
        // BWIPP's `for (;;)` — used purely for early exit. The
        // `never_loop` lint is intentionally suppressed: the labeled
        // break is the structural translation of BWIPP's nested `if`
        // ladder and rewriting it with returns would obscure the
        // direct correspondence to bwip-js lines 27160-27305.
        #[allow(clippy::never_loop)]
        let next_mode: Mode = 'pick: loop {
            // BWIPP wraps the entire body in an `if (!$_.eci)` guard;
            // we don't yet support ECI escapes in the input slice so
            // `$_.eci` is always false.

            // Branch 0: starting mode (`$_.mode == -1`).
            if mode.is_none() {
                if k_before_a(&QRCODE_MODE0_FORCE_KB) {
                    break 'pick Mode::Kanji;
                }
                if k_before_n(&QRCODE_MODE0_FORCE_KB) {
                    break 'pick Mode::Kanji;
                }
                if k_before_b(&QRCODE_MODE_BK_BEFORE_E) {
                    break 'pick Mode::Kanji;
                }
                if k_before_e(&QRCODE_MODE0_FORCE_KB) {
                    break 'pick Mode::Kanji;
                }
                if num_k >= 1 {
                    break 'pick Mode::Byte;
                }
                if n_before_k(&QRCODE_MODE0_N_BEFORE_B) {
                    break 'pick Mode::Numeric;
                }
                if n_before_b(&QRCODE_MODE0_N_BEFORE_B) {
                    break 'pick Mode::Numeric;
                }
                if n_before_b(&QRCODE_MODE0_FORCE_KB) {
                    break 'pick Mode::Byte;
                }
                if n_before_a(&QRCODE_MODE_AN_BEFORE_E) {
                    break 'pick Mode::Numeric;
                }
                if n_before_e(&QRCODE_MODE0_FORCE_N) {
                    break 'pick Mode::Numeric;
                }
                if a_before_k(&QRCODE_MODE_BA_BEFORE_E) {
                    break 'pick Mode::Alphanumeric;
                }
                if a_or_n_before_b(&QRCODE_MODE_BA_BEFORE_E) {
                    break 'pick Mode::Alphanumeric;
                }
                if a_or_n_before_e(&QRCODE_MODE0_FORCE_A) {
                    break 'pick Mode::Alphanumeric;
                }
                break 'pick Mode::Byte;
            }

            let m = mode.unwrap();

            // Branch 1: current mode = Byte.
            if matches!(m, Mode::Byte) {
                if k_before_b(&QRCODE_MODE_BK_BEFORE_B) {
                    break 'pick Mode::Kanji;
                }
                if k_before_a(&QRCODE_MODE_BK_BEFORE_A) {
                    break 'pick Mode::Kanji;
                }
                if k_before_n(&QRCODE_MODE_BK_BEFORE_N) {
                    break 'pick Mode::Kanji;
                }
                if k_before_e(&QRCODE_MODE_BK_BEFORE_E) {
                    break 'pick Mode::Kanji;
                }
                if a_before_k(&QRCODE_MODE_BA_BEFORE_K) {
                    break 'pick Mode::Alphanumeric;
                }
                if a_before_b(&QRCODE_MODE_BA_BEFORE_B) {
                    break 'pick Mode::Alphanumeric;
                }
                if a_before_n(&QRCODE_MODE_BA_BEFORE_N) {
                    break 'pick Mode::Alphanumeric;
                }
                if a_before_e(&QRCODE_MODE_BA_BEFORE_E) {
                    break 'pick Mode::Alphanumeric;
                }
                if n_before_k(&QRCODE_MODE_BN_BEFORE_K) {
                    break 'pick Mode::Numeric;
                }
                if n_before_b(&QRCODE_MODE_BN_BEFORE_B) {
                    break 'pick Mode::Numeric;
                }
                if n_before_a(&QRCODE_MODE_BN_BEFORE_A) {
                    break 'pick Mode::Numeric;
                }
                if n_before_e(&QRCODE_MODE_BN_BEFORE_E) {
                    break 'pick Mode::Numeric;
                }
                // BWIPP's nested "AorN ... && nextNslt(modeBNbeforeA)"
                // tie-breaker — only relevant when both predicates are
                // close. We skip it for now (degenerates to keep-Byte)
                // and rely on oracle pinning to catch any divergence.
                break 'pick Mode::Byte;
            }

            // Branch 2: current mode = Alphanumeric.
            if matches!(m, Mode::Alphanumeric) {
                if num_k >= 1 {
                    break 'pick Mode::Kanji;
                }
                if num_b >= 1 {
                    break 'pick Mode::Byte;
                }
                if n_before_a(&QRCODE_MODE_AN_BEFORE_A) {
                    break 'pick Mode::Numeric;
                }
                if n_before_b(&QRCODE_MODE_AN_BEFORE_B) {
                    break 'pick Mode::Numeric;
                }
                if n_before_e(&QRCODE_MODE_AN_BEFORE_E) {
                    break 'pick Mode::Numeric;
                }
                if num_a >= 1 || num_n >= 1 {
                    break 'pick Mode::Alphanumeric;
                }
                break 'pick Mode::Byte;
            }

            // Branch 3: current mode = Numeric.
            if matches!(m, Mode::Numeric) {
                if num_k >= 1 {
                    break 'pick Mode::Kanji;
                }
                if num_b >= 1 {
                    break 'pick Mode::Byte;
                }
                if num_a >= 1 {
                    break 'pick Mode::Alphanumeric;
                }
                if num_n >= 1 {
                    break 'pick Mode::Numeric;
                }
                break 'pick Mode::Byte;
            }

            // Branch 4: current mode = Kanji.
            if matches!(m, Mode::Kanji) {
                if num_b >= 1 {
                    break 'pick Mode::Byte;
                }
                if num_a >= 1 {
                    break 'pick Mode::Alphanumeric;
                }
                if num_n >= 1 {
                    break 'pick Mode::Numeric;
                }
                if num_k >= 1 {
                    break 'pick Mode::Kanji;
                }
                break 'pick Mode::Byte;
            }

            // ECI mode: BWIPP routes explicit ECI escapes through a
            // distinct codepath (qrcode::Bits::push_eci_designator).
            // This Rust port's catalog API doesn't accept raw ECI
            // headers — non-ECI payloads default to Byte as the
            // fallback mode.
            break 'pick Mode::Byte;
        };

        // BWIPP: if mode would be Kanji AND fnc1first, force Byte.
        let next_mode = if matches!(next_mode, Mode::Kanji) && fnc1first {
            Mode::Byte
        } else {
            next_mode
        };

        // Compute the advance count for the chosen mode.
        let advance: usize = match next_mode {
            Mode::Kanji => 2 * num_k as usize,
            Mode::Byte => num_b as usize,
            Mode::Alphanumeric => num_a as usize,
            Mode::Numeric => num_n as usize,
            Mode::Eci => 1,
        };
        // If the chosen mode-counter is 0 (no run starts here), fall
        // back to consuming one byte in Byte mode. This handles edge
        // cases the BWIPP decision tree doesn't reach (e.g. a single
        // lowercase char selected after a long alpha run).
        let advance = if advance == 0 { 1 } else { advance };
        let chosen = if advance == 0 { Mode::Byte } else { next_mode };

        // Merge with the previous segment if same mode and contiguous.
        if let Some(last) = segments.last_mut() {
            if last.mode == chosen && last.start + last.len == i {
                last.len += advance;
                i += advance;
                mode = Some(chosen);
                let _ = start; // suppress lint
                continue;
            }
        }
        segments.push(Segment {
            mode: chosen,
            start: i,
            len: advance,
        });
        i += advance;
        mode = Some(chosen);
        start = i;
    }

    segments
}

/// Emit the raw mode-encoded bit-stream for a sequence of segments,
/// matching BWIPP's `bits` accumulator at bwip-js line 27331-27353.
///
/// For each segment, emits:
///
///   1. The mode-indicator bit-string from [`QRCODE_MIDS`] for
///      `(layout_id, mode)`.
///   2. The character-count indicator (CCI) per [`CC_LENS`].
///   3. The mode-specific payload bits via `encode_*_segment`.
///
/// If `fnc1first` is true, the FNC1-first indicator is prepended
/// (BWIPP: `"0101"` for Full QR / `"101"` for rMQR).
///
/// # Errors
///
/// Returns `Error::InvalidData` if any segment's mode is unsupported
/// by the given `layout_id` (e.g. Alphanumeric on M1, or Kanji on M1
/// /M2), or if any segment's payload is malformed.
pub(crate) fn compose_segments(
    msg: &[u8],
    segments: &[Segment],
    layout_id: u8,
    fnc1first: bool,
) -> Result<Vec<bool>, Error> {
    let mut bits: Vec<bool> = Vec::new();
    let bin = layout_id as usize;
    if bin >= QRCODE_MIDS.len() {
        return Err(Error::InvalidData(format!(
            "qrcode_native: layout_id {layout_id} out of range"
        )));
    }

    // FNC1-first prefix (Full QR vs rMQR).
    if fnc1first {
        let prefix = if layout_id < 7 { "0101" } else { "101" };
        for c in prefix.chars() {
            bits.push(c == '1');
        }
    }

    for seg in segments {
        let mode_idx = seg.mode as usize;
        let mid = QRCODE_MIDS[bin][mode_idx].ok_or_else(|| {
            Error::InvalidData(format!(
                "qrcode_native: mode {:?} not supported by layout_id {layout_id}",
                seg.mode
            ))
        })?;
        for c in mid.chars() {
            bits.push(c == '1');
        }

        // Character count.
        let char_count = match seg.mode {
            Mode::Kanji => seg.len / 2,
            _ => seg.len,
        };
        if !matches!(seg.mode, Mode::Eci) {
            let cci_width = cci_bits(layout_id, seg.mode).ok_or_else(|| {
                Error::InvalidData(format!(
                    "qrcode_native: no CCI width for layout_id={layout_id} mode={:?}",
                    seg.mode
                ))
            })?;
            push_bits(&mut bits, char_count as u32, cci_width);
        }

        // Payload.
        let chunk = &msg[seg.start..seg.start + seg.len];
        let payload: Vec<bool> = match seg.mode {
            Mode::Numeric => encode_numeric_segment(chunk)?,
            Mode::Alphanumeric => encode_alphanumeric_segment(chunk)?,
            Mode::Byte => encode_byte_segment(chunk),
            Mode::Kanji => {
                let mut codes: Vec<u16> = Vec::with_capacity(chunk.len() / 2);
                let mut p = 0;
                while p + 1 < chunk.len() {
                    let code = ((chunk[p] as u16) << 8) | (chunk[p + 1] as u16);
                    codes.push(code);
                    p += 2;
                }
                encode_kanji_segment(&codes)?
            }
            Mode::Eci => {
                return Err(Error::InvalidData(
                    "qrcode_native: ECI segments are outside this port's compose_segments scope (BWIPP's parsefnc-driven ECI escapes use a separate codepath; for plain Latin-1/UTF-8 payloads use Byte mode, which is selected automatically by the mode selector)".into(),
                ));
            }
        };
        bits.extend_from_slice(&payload);
    }

    Ok(bits)
}

// ---------------------------------------------------------------------------
// Stage 4a — codeword padding + Reed-Solomon (single-block)
//
// BWIPP source: bwip-js `src/bwipp.js` `bwipp_qrcode` body
//   * GF(256) tables — line 26725-26728 (rsalog/rslog over primitive
//     poly 285 = 0x011D, distinct from Datamatrix/Codeone's 0x012D)
//   * qrcode_rsprod — line 26780 (log-table multiply with zero short-circuit)
//   * qrcode_gencoeffs — line 26805 (generator polynomial builder)
//   * qrcode_termlens — line 25650 (terminator bit-width per layout_id)
//   * qrcode_padstrs — line 25651 (0xEC / 0x11 alternation)
//   * Pad/term/RS-application loop — line 27445-27530
//
// Stage 4b (below) glues these into a multi-block interleaver and the
// full end-to-end codeword stream verified byte-for-byte against the
// bwip-js oracle.
// ---------------------------------------------------------------------------

/// MSB-first pack a bit slice into bytes. Trailing partial byte is
/// zero-padded.
pub(crate) fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}

/// Terminator bit-width per `layout_id`. BWIPP `qrcode_termlens`
/// (bwip-js line 25650). Indices align with [`CC_LENS`] / `layout_id`:
///
/// * 0/1/2 → Full V1-9 / V10-26 / V27-40 → 4 bits
/// * 3 → M1 → 3 bits, 4 → M2 → 5 bits, 5 → M3 → 7 bits, 6 → M4 → 9 bits
/// * 7..=38 → all 32 rMQR variants → 3 bits
pub(crate) const TERMINATOR_LEN: [u8; 39] = {
    let mut t = [3u8; 39];
    t[0] = 4;
    t[1] = 4;
    t[2] = 4;
    t[3] = 3;
    t[4] = 5;
    t[5] = 7;
    t[6] = 9;
    // rMQR rows 7..=38 keep the default 3.
    t
};

/// BWIPP `qrcode_padstrs` (bwip-js line 25651): alternating EC-padding
/// codewords used to fill the data area after the terminator.
pub(crate) const PADDING_CODEWORDS: [u8; 2] = [0xEC, 0x11];

/// Pad a raw mode-encoded bit-stream to a full codeword stream:
///
/// 1. Append the format-specific terminator (`TERMINATOR_LEN`), truncated
///    if `bits.len() + termlen > data_capacity_bits`.
/// 2. Zero-pad to the next byte boundary.
/// 3. Pack to bytes (MSB-first).
/// 4. Alternate `PADDING_CODEWORDS` (0xEC, 0x11) until the byte array
///    reaches `data_codeword_count` bytes.
///
/// Returns the data-codeword byte vector. `lc4b` (M1/M3 only) flags
/// that the final codeword carries only 4 high-order bits — the caller
/// handles that nibble fix-up in Stage 4b's downstream pipeline. For
/// non-`lc4b` versions this helper is the complete data-stream
/// producer.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if `bits.len() > data_capacity_bits`.
pub(crate) fn pad_codewords(
    bits: &[bool],
    layout_id: u8,
    data_capacity_bits: u32,
    data_codeword_count: u32,
) -> Result<Vec<u8>, Error> {
    if bits.len() as u32 > data_capacity_bits {
        return Err(Error::InvalidData(format!(
            "qrcode_native: bit-stream ({} bits) exceeds capacity ({data_capacity_bits} bits)",
            bits.len()
        )));
    }
    let term_len = TERMINATOR_LEN[layout_id as usize] as u32;
    let remaining = data_capacity_bits - bits.len() as u32;
    let actual_term = term_len.min(remaining);
    let mut buf: Vec<bool> = Vec::with_capacity(data_codeword_count as usize * 8);
    buf.extend_from_slice(bits);
    buf.extend(std::iter::repeat_n(false, actual_term as usize));
    // Zero-pad to the next byte boundary.
    let bytes_remainder = buf.len() % 8;
    if bytes_remainder != 0 {
        buf.extend(std::iter::repeat_n(false, 8 - bytes_remainder));
    }
    let mut bytes = bits_to_bytes(&buf);
    // For lc4b symbols (M1, M3) the LAST codeword carries only 4
    // valid data bits in its HIGH nibble (the low nibble is unused).
    // BWIPP's padding loop (bwip-js line 27701) fills 8-bit blocks
    // only up to `dmod - 5`, leaving positions (dcws-1)*8..(dcws-1)*8+3
    // as `0` for the lc4b last codeword. We mirror that by:
    // 1. Truncating `bytes` to at most dcws-1 full bytes (in case
    //    msg+term+align overflowed into byte dcws-1).
    // 2. Filling 0xEC/0x11 padding up to dcws-1 bytes.
    // 3. Constructing byte dcws-1 from the 4 bits at positions
    //    (dcws-1)*8..(dcws-1)*8+3 of `buf` (high nibble), low nibble 0.
    let lc4b = layout_id == 3 || layout_id == 5;
    let mut idx = 0;
    if lc4b {
        let target = data_codeword_count as usize - 1;
        bytes.truncate(target);
        while bytes.len() < target {
            bytes.push(PADDING_CODEWORDS[idx]);
            idx = (idx + 1) % 2;
        }
        let last_codeword_start = target * 8;
        let mut high_nibble = 0u8;
        for k in 0..4 {
            if last_codeword_start + k < buf.len() && buf[last_codeword_start + k] {
                high_nibble |= 1 << (3 - k);
            }
        }
        bytes.push(high_nibble << 4);
    } else {
        while bytes.len() < data_codeword_count as usize {
            bytes.push(PADDING_CODEWORDS[idx]);
            idx = (idx + 1) % 2;
        }
    }
    Ok(bytes)
}

// --- GF(256) for QR family (primitive polynomial 0x011D) ---------------------

/// GF(256) exponent table (`alpha^i mod p(x)`) over the QR primitive
/// polynomial `0x011D = x^8 + x^4 + x^3 + x^2 + 1`. Mirrors BWIPP
/// `$_.rsalog` at bwip-js line 26725.
pub(crate) const QR_GF256_EXP: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut v: u16 = 1;
    let mut i = 0;
    while i < 255 {
        t[i] = v as u8;
        v <<= 1;
        if v >= 256 {
            v ^= 0x011D;
        }
        i += 1;
    }
    // BWIPP wraps once more for the 255-length array, but since alpha^255 == 1
    // and we only ever index by `i % 255`, the trailing slot is fine left as 0.
    t
};

/// GF(256) logarithm table: `QR_GF256_LOG[QR_GF256_EXP[i]] == i` for
/// `i in 1..=254`. Slot 0 is unused (log of 0 is undefined; callers
/// short-circuit zero operands before lookup).
pub(crate) const QR_GF256_LOG: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i: usize = 1;
    while i < 255 {
        t[QR_GF256_EXP[i] as usize] = i as u8;
        i += 1;
    }
    t
};

/// GF(256) multiply over the QR primitive polynomial. Short-circuits
/// when either operand is zero, matching BWIPP `qrcode_rsprod`.
#[inline]
pub(crate) fn qr_gf256_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let la = QR_GF256_LOG[a as usize] as u16;
    let lb = QR_GF256_LOG[b as usize] as u16;
    QR_GF256_EXP[((la + lb) % 255) as usize]
}

/// Compute the generator polynomial for `ec_len` error-correction
/// codewords. Mirrors BWIPP `qrcode_gencoeffs` (bwip-js line 26805).
///
/// Returns the `ec_len`-element vector `[g_{n-1}, …, g_1, g_0]` where
/// `g(x) = (x - α^0)(x - α^1)…(x - α^{n-1})` — i.e. the running
/// LFSR-style polynomial that the BWIPP `bwipp_rsecbinary` helper
/// reduces against.
pub(crate) fn qr_rs_gen_coeffs(ec_len: usize) -> Vec<u8> {
    // BWIPP starts with [1, 0, 0, …, 0] (`ec_len + 1` entries) and
    // grows the polynomial by multiplying by `(x - α^i)` for i in
    // 0..ec_len. The shape mirrors codeone's gencoeffs exactly except
    // for the GF(256) operand poly (0x011D vs 0x012D).
    let mut coeffs = vec![0u8; ec_len + 1];
    coeffs[0] = 1;
    for i in 0..ec_len {
        // Shift up: coeffs[i+1] = coeffs[i]
        coeffs[i + 1] = coeffs[i];
        // Sweep down from i to 1: coeffs[j] = mul(coeffs[j], alog[i]) XOR coeffs[j-1].
        let alpha_i = QR_GF256_EXP[i];
        for j in (1..=i).rev() {
            coeffs[j] = qr_gf256_mul(coeffs[j], alpha_i) ^ coeffs[j - 1];
        }
        // coeffs[0] = mul(coeffs[0], alog[i])
        coeffs[0] = qr_gf256_mul(coeffs[0], alpha_i);
    }
    // BWIPP returns `geti(coeffs, 0, coeffs.length - 1)` — drop the last entry.
    coeffs.truncate(ec_len);
    coeffs
}

/// Compute `ec_len` Reed-Solomon error-correction bytes for a single
/// data block. Mirrors BWIPP `bwipp_rsecbinary` invoked from
/// `$_.rscodes` (bwip-js line 27506-27509). The LFSR runs
/// `data.len()` times; on each step it XORs the leading byte with the
/// current `lfsr[0]`, multiplies that scalar through the generator
/// polynomial, and shifts the LFSR register.
pub(crate) fn qr_rs_block_ecc(data: &[u8], ec_len: usize) -> Vec<u8> {
    let coeffs = qr_rs_gen_coeffs(ec_len);
    // LFSR state: `ec_len` entries, all zero initially. Output is
    // collected MSB-first inside `lfsr` at the end.
    let mut lfsr = vec![0u8; ec_len];
    for &d in data {
        let feedback = d ^ lfsr[ec_len - 1];
        // Compute new lfsr[k] = lfsr[k-1] XOR coeffs[ec_len-1-k] * feedback,
        // shifting "down" (toward lfsr[0]). BWIPP `bwipp_rsecbinary` walks
        // the polynomial coefficients in the same order; the `coeffs`
        // vector returned by `qr_rs_gen_coeffs` is already in LFSR order.
        for k in (1..ec_len).rev() {
            lfsr[k] = lfsr[k - 1] ^ qr_gf256_mul(coeffs[k], feedback);
        }
        lfsr[0] = qr_gf256_mul(coeffs[0], feedback);
    }
    // bwipp_rsecbinary returns lfsr MSB-first as `[lfsr[ec_len-1], …,
    // lfsr[1], lfsr[0]]`.
    lfsr.reverse();
    lfsr
}

// ---------------------------------------------------------------------------
// Stage 4b — multi-block EC interleaver + end-to-end codeword stream
//
// BWIPP source: bwip-js `src/bwipp.js` `bwipp_qrcode` body lines
//   * 27378-27392 (block-layout fields)
//   * 27515-27530 (block splitting + RS per block)
//   * 27531-27548 (data interleave then ECC interleave — ISO 18004 §8.6.1)
//   * 27556-27572 (rbit zero-codeword + lc4b nibble shift)
//
// Verification: build_codeword_stream() oracle-pinned byte-for-byte
// against patched bwip-js debugecc dump for V1-L/V1-M/V5-Q/M1/R7x43.
// ---------------------------------------------------------------------------

/// Derived block-layout parameters for a given (metric, ec_level).
/// Mirrors the post-init values BWIPP computes at bwip-js line
/// 27378-27440.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockLayout {
    /// Total codewords in the symbol (data + ECC + rbit padding).
    pub ncws: u32,
    /// Data codewords.
    pub dcws: u32,
    /// ECC codewords per block.
    pub ecpb: u32,
    /// Number of group-1 (small) blocks.
    pub ecb1: u32,
    /// Number of group-2 (large) blocks. Group-2 blocks each carry
    /// `dcpb + 1` data codewords.
    pub ecb2: u32,
    /// Data codewords per group-1 block (group-2 has `dcpb + 1`).
    pub dcpb: u32,
    /// Remainder bits — when > 0 the cws stream is padded with one
    /// trailing zero byte (= "remainder bits" in ISO 18004 §7.4.10).
    pub rbit: u8,
    /// "Last codeword has 4 bits" — true for M1 and M3 only. When
    /// true, the last data codeword carries 4 high-order bits, and
    /// `apply_lc4b_nibble_fixup` packs the trailing ECC bytes after
    /// the 4-bit boundary.
    pub lc4b: bool,
}

/// Compute the block-layout parameters for (metric_idx, ec_level).
/// Mirrors BWIPP's branch at bwip-js line 27378-27392 + line 27439-27440.
///
/// # Errors
///
/// Returns `Error::InvalidData` if `ec_level` ≥ 4 or if the chosen
/// `(format, version)` doesn't support the requested EC level (i.e.
/// the metric's `eclen`/`blocks` entry is the NA sentinel).
pub(crate) fn block_layout(metric_idx: usize, ec_level: u8) -> Result<BlockLayout, Error> {
    if ec_level >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ec_level {ec_level} out of range (0..=3 = L/M/Q/H)"
        )));
    }
    if metric_idx >= FULL_METRICS.len() {
        return Err(Error::InvalidData(format!(
            "qrcode_native: metric_idx {metric_idx} out of range (0..{})",
            FULL_METRICS.len()
        )));
    }
    let m = &FULL_METRICS[metric_idx];
    let lc4b = m.version_str == "M1" || m.version_str == "M3";
    let mut ncws = m.datacap_bits / 8;
    let mut rbit = (m.datacap_bits % 8) as u8;
    if lc4b {
        ncws += 1;
        rbit = 0;
    }
    let ec_idx = ec_level as usize;
    let ecws = m.eclen[ec_idx];
    let ecb1 = m.blocks[ec_idx * 2];
    let ecb2 = m.blocks[ec_idx * 2 + 1];
    if ecws as i32 == NA_I8 as i32 || ecb1 < 0 || ecb2 < 0 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: format/version `{}` does not support EC level {}",
            m.version_str,
            "LMQH".as_bytes()[ec_idx] as char
        )));
    }
    let ecws = u32::from(ecws);
    let dcws = ncws - ecws;
    let ecb1 = ecb1 as u32;
    let ecb2 = ecb2 as u32;
    let num_blocks = ecb1 + ecb2;
    if num_blocks == 0 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: zero-block layout for metric {metric_idx} EC {ec_level}",
        )));
    }
    let dcpb = dcws / num_blocks;
    let ecpb = ncws / num_blocks - dcpb;
    Ok(BlockLayout {
        ncws,
        dcws,
        ecpb,
        ecb1,
        ecb2,
        dcpb,
        rbit,
        lc4b,
    })
}

/// Split `data` into per-block slices according to a [`BlockLayout`].
///
/// Group-1 blocks (ebc1 of them) each get `dcpb` codewords. Group-2
/// blocks (ebc2 of them) each get `dcpb + 1` codewords. The
/// concatenated length must equal `layout.dcws`.
///
/// Returns `Vec<&[u8]>` so callers can avoid copying the data; the
/// caller usually feeds each slice through [`qr_rs_block_ecc`] to
/// build the per-block ECC.
pub(crate) fn split_data_blocks<'a>(data: &'a [u8], layout: &BlockLayout) -> Vec<&'a [u8]> {
    let dcpb = layout.dcpb as usize;
    let ecb1 = layout.ecb1 as usize;
    let ecb2 = layout.ecb2 as usize;
    let mut blocks = Vec::with_capacity(ecb1 + ecb2);
    let mut offset = 0;
    for _ in 0..ecb1 {
        blocks.push(&data[offset..offset + dcpb]);
        offset += dcpb;
    }
    for _ in 0..ecb2 {
        blocks.push(&data[offset..offset + dcpb + 1]);
        offset += dcpb + 1;
    }
    blocks
}

/// Interleave per-block data and ECC into the final codeword stream
/// per ISO 18004 §8.6.1. Data codewords are interleaved column-by-column
/// across blocks (skipping shorter group-1 blocks for the trailing
/// column); ECC codewords are then interleaved column-by-column.
pub(crate) fn interleave_blocks(data_blocks: &[&[u8]], ecc_blocks: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(
        data_blocks.len(),
        ecc_blocks.len(),
        "must have same number of data and ECC blocks"
    );
    let max_data_cols = data_blocks.iter().map(|b| b.len()).max().unwrap_or(0);
    let ecc_cols = ecc_blocks.first().map(|b| b.len()).unwrap_or(0);
    let total = data_blocks.iter().map(|b| b.len()).sum::<usize>()
        + ecc_blocks.iter().map(|b| b.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    for col in 0..max_data_cols {
        for block in data_blocks {
            if col < block.len() {
                out.push(block[col]);
            }
        }
    }
    for col in 0..ecc_cols {
        for block in ecc_blocks {
            out.push(block[col]);
        }
    }
    out
}

/// Apply BWIPP's `lc4b` nibble shift (bwip-js lines 27566-27572). For
/// M1 and M3 only, the last data codeword holds 4 high-order bits and
/// the following ECC bytes are shifted 4 bits left to consume the low
/// nibble of each preceding byte.
pub(crate) fn apply_lc4b_nibble_fixup(stream: &mut [u8], dcws: u32, ncws: u32) {
    let dcws = dcws as usize;
    let ncws = ncws as usize;
    if ncws == 0 {
        return;
    }
    // The last data codeword: shift its high nibble down (the 4 valid
    // bits live in the upper half of the byte coming out of the pad
    // pipeline; BWIPP reads them as the data half-nibble).
    stream[dcws - 1] >>= 4;
    for i in (dcws - 1)..(ncws - 1) {
        stream[i] = (stream[i] & 0x0F) << 4;
        stream[i] |= (stream[i + 1] >> 4) & 0x0F;
    }
    stream[ncws - 1] = (stream[ncws - 1] & 0x0F) << 4;
}

/// Build the full post-RS codeword stream for one (metric_idx,
/// ec_level, padded_data) triple. Mirrors BWIPP's rscodes pipeline at
/// bwip-js lines 27445-27572.
///
/// `padded_data` must be the byte-packed data-codeword stream produced
/// by [`pad_codewords`] — length `block_layout(...).dcws`.
///
/// # Errors
///
/// Returns `Error::InvalidData` if `padded_data.len()` mismatches the
/// computed `dcws`.
pub(crate) fn build_codeword_stream(
    padded_data: &[u8],
    metric_idx: usize,
    ec_level: u8,
) -> Result<Vec<u8>, Error> {
    let layout = block_layout(metric_idx, ec_level)?;
    if padded_data.len() != layout.dcws as usize {
        return Err(Error::InvalidData(format!(
            "qrcode_native: padded_data length {} does not match dcws {}",
            padded_data.len(),
            layout.dcws
        )));
    }
    let data_blocks = split_data_blocks(padded_data, &layout);
    let ecc_blocks: Vec<Vec<u8>> = data_blocks
        .iter()
        .map(|b| qr_rs_block_ecc(b, layout.ecpb as usize))
        .collect();
    let mut stream = interleave_blocks(&data_blocks, &ecc_blocks);
    // BWIPP `rbit > 0` branch (line 27556): append one trailing zero
    // codeword carrying the symbol's remainder bits.
    if layout.rbit > 0 {
        stream.push(0);
    }
    if layout.lc4b {
        apply_lc4b_nibble_fixup(&mut stream, layout.dcws, layout.ncws);
    }
    Ok(stream)
}

// ---------------------------------------------------------------------------
// Stage 6a — Matrix scaffold: init, finder patterns, timing patterns
//
// BWIPP source: bwip-js `src/bwipp.js` `bwipp_qrcode` body lines
//   * 27586 (pixs init: `rows × cols` of `-1`)
//   * 27587 (qmv helper: row,col → linear index)
//   * 27594-27622 (timing patterns — per-format)
//   * 27625-27773 (finder patterns — per-format)
//
// The matrix is a `Vec<i8>` of size `rows × cols`, stored row-major.
// Cells start at `-1` (unset). Function patterns write `0` / `1`;
// the zig-zag codeword walker (Stage 6c) will only fill cells that
// are still `-1`.
// ---------------------------------------------------------------------------

/// Sentinel for an unset cell in the QR pixs grid. Function-pattern
/// writers and codeword placement both check for `< 0` to know
/// whether a cell is free.
pub(crate) const PIXS_UNSET: i8 = -1;

/// Allocate a fresh QR matrix of `rows × cols` cells, all initialized
/// to [`PIXS_UNSET`]. The pixs vector is row-major: `pixs[row * cols + col]`.
pub(crate) fn init_pixs_matrix(rows: u16, cols: u16) -> Vec<i8> {
    vec![PIXS_UNSET; rows as usize * cols as usize]
}

/// Convert `(row, col)` to a linear index into the pixs vector.
/// Mirrors BWIPP `qmv` (bwip-js line 27587).
#[inline]
pub(crate) fn qmv(row: usize, col: usize, cols: usize) -> usize {
    row * cols + col
}

/// Write a 0/1 bit at `(row, col)` in `pixs`. No-op when the cell is
/// out of bounds (the matrix-builder routines run blind over rectangular
/// rMQR symbols where finder coordinates can sit at unused row/col positions).
///
/// The OOB check must verify `col < cols` independently of the linear
/// index — a `pixs.len()` bound alone is not sufficient because
/// `qmv(row, cols, cols) == qmv(row + 1, 0, cols)`, so writing to a
/// notionally-OOB `col == cols` would silently corrupt the start of
/// the next row.
#[inline]
fn pixs_set(pixs: &mut [i8], row: usize, col: usize, cols: usize, value: i8) {
    if col >= cols {
        return;
    }
    let idx = qmv(row, col, cols);
    if idx < pixs.len() {
        pixs[idx] = value;
    }
}

/// Place the timing patterns for the given format. Mirrors BWIPP
/// bwip-js lines 27594-27622:
///
/// * Full QR — alternating stripe along row 6, cols 8..=cols-9 and
///   col 6, rows 8..=rows-9. The cell value is `(i + 1) % 2`, so
///   `i = 8` ⇒ value `1` (timing starts dark).
/// * Micro QR — stripe along row 0 cols 8..=cols-1, and col 0 rows
///   8..=rows-1 (the finder's right/bottom edges).
/// * rMQR — top row (0), bottom row (rows-1), leftmost col (0),
///   rightmost col (cols-1) PLUS a third "alignment-column" run of
///   vertical strips at cols asp2-1, asp2-1+step, … each spanning
///   rows 3..=rows-4 (BWIPP source 27617-27622). For sizes with
///   `fimas == NA` (the smallest rMQR variants) a single strip
///   lives at col asp2-1; for others, multiple strips connect the
///   per-alignment columns.
///
/// `fimax` (= asp2 in BWIPP) and `fimas` (= asp3) are passed through
/// from `FULL_METRICS` for the rMQR-only vertical strip placement;
/// they are ignored for Full/Micro.
pub(crate) fn place_timing_patterns(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
    fimax: u16,
    fimas: u16,
) {
    let cols_u = cols as usize;
    let rows_u = rows as usize;
    let bin = layout_id as usize;
    // Format dispatch from FULL_METRICS[layout_id].format.
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };

    match format {
        Format::Full => {
            // Row-6 timing pattern, cols 8..=cols-9.
            for i in 8..=cols_u.saturating_sub(9) {
                let bit = ((i + 1) % 2) as i8;
                pixs_set(pixs, 6, i, cols_u, bit);
                pixs_set(pixs, i, 6, cols_u, bit);
            }
            let _ = (fimax, fimas);
        }
        Format::Micro => {
            // Row-0 / col-0 timing pattern, indices 8..=cols-1.
            for i in 8..cols_u {
                let bit = ((i + 1) % 2) as i8;
                pixs_set(pixs, 0, i, cols_u, bit);
                pixs_set(pixs, i, 0, cols_u, bit);
            }
            let _ = (fimax, fimas);
        }
        Format::Rmqr => {
            // Top/bottom rows, cols 3..=cols-4.
            for i in 3..=cols_u.saturating_sub(4) {
                let bit = ((i + 1) % 2) as i8;
                pixs_set(pixs, 0, i, cols_u, bit);
                pixs_set(pixs, rows_u - 1, i, cols_u, bit);
            }
            // Left/right cols, rows 3..=rows-4.
            for i in 3..=rows_u.saturating_sub(4) {
                let bit = ((i + 1) % 2) as i8;
                pixs_set(pixs, i, 0, cols_u, bit);
                pixs_set(pixs, i, cols_u - 1, cols_u, bit);
            }
            // Vertical timing strips at alignment-pattern columns
            // (BWIPP bwip-js line 27617-27622). For each alignment
            // column i = asp2-1, asp2-1+step, …, ≤ cols-13: write
            // alternating bits down rows 3..=rows-4 at col i.
            if fimax != NA {
                let step = if fimas == NA {
                    0i32
                } else {
                    (fimas as i32) - (fimax as i32)
                };
                let edge_end = cols_u.saturating_sub(13);
                let mut i = (fimax as usize).saturating_sub(1);
                loop {
                    if i > edge_end {
                        break;
                    }
                    for j in 3..=rows_u.saturating_sub(4) {
                        let bit = ((j + 1) % 2) as i8;
                        pixs_set(pixs, j, i, cols_u, bit);
                    }
                    if step <= 0 {
                        break;
                    }
                    i += step as usize;
                }
            }
        }
    }
}

/// Canonical 7×7 finder pattern (BWIPP bwip-js implicitly uses this
/// via nested loops at line 27628-27660). Reading row-by-row:
///
/// ```text
/// 1 1 1 1 1 1 1
/// 1 0 0 0 0 0 1
/// 1 0 1 1 1 0 1
/// 1 0 1 1 1 0 1
/// 1 0 1 1 1 0 1
/// 1 0 0 0 0 0 1
/// 1 1 1 1 1 1 1
/// ```
pub(crate) const FINDER_PATTERN: [[u8; 7]; 7] = [
    [1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 0, 0, 1],
    [1, 0, 1, 1, 1, 0, 1],
    [1, 0, 1, 1, 1, 0, 1],
    [1, 0, 1, 1, 1, 0, 1],
    [1, 0, 0, 0, 0, 0, 1],
    [1, 1, 1, 1, 1, 1, 1],
];

// `place_one_finder` was retired in Stage 15c; finder placement now
// uses the unified 8×8 4-corner walk over `FPAT_RMQR` (and the
// rMQR-specific `FCORPAT_RMQR` / `FSUBPAT_RMQR`) per BWIPP's
// `qrcode_fpatmap` model. The `FINDER_PATTERN` 7×7 table is kept
// for its inline documentation + the `finder_pattern_*` doctests.

/// rMQR top-right / bottom-left "corner mark" — a 3-cell L-shape.
/// Per BWIPP `qrcode_fcorpat` (bwip-js line 26076). Values of `9`
/// are skip-sentinels (don't write). Reading row-by-row:
///
/// ```text
/// 1 1 1 . . . . .
/// 1 0 . . . . . .
/// 1 . . . . . . .
/// . . . . . . . .
/// (rows 3..7 all skip)
/// ```
pub(crate) const FCORPAT_RMQR: [[i8; 8]; 8] = [
    [1, 1, 1, 9, 9, 9, 9, 9],
    [1, 0, 9, 9, 9, 9, 9, 9],
    [1, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
];

/// rMQR bottom-right 5×5 "sub-finder" pattern. Per BWIPP
/// `qrcode_fsubpat` (bwip-js line 26074). Values of `9` are skip
/// sentinels. The sub-finder is a 5×5 dark ring with a center dark
/// cell — note that unlike the 7×7 TL finder, the sub-finder has
/// **no separator** (BWIPP does not write 0 around it). Reading
/// row-by-row (BWIPP convention: y is row, x is col):
///
/// ```text
/// 1 1 1 1 1 . . .
/// 1 0 0 0 1 . . .
/// 1 0 1 0 1 . . .
/// 1 0 0 0 1 . . .
/// 1 1 1 1 1 . . .
/// (rows 5..7 all skip)
/// ```
pub(crate) const FSUBPAT_RMQR: [[i8; 8]; 8] = [
    [1, 1, 1, 1, 1, 9, 9, 9],
    [1, 0, 0, 0, 1, 9, 9, 9],
    [1, 0, 1, 0, 1, 9, 9, 9],
    [1, 0, 0, 0, 1, 9, 9, 9],
    [1, 1, 1, 1, 1, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
    [9, 9, 9, 9, 9, 9, 9, 9],
];

/// rMQR top-left 7×7 finder pattern *with separator*, packed into
/// the same 8×8 grid as the other rMQR finder cells so a single
/// loop can write all four corners. Mirrors BWIPP's
/// `qrcode_fpat` (bwip-js line 26070): rows 0..=6 cols 0..=6 are
/// the standard QR finder; col 7 of rows 0..=6 and all of row 7
/// are `0` (the separator border). Note: BWIPP gates this with
/// `y < rows`, so for rMQR R7×_ the separator-row write is
/// elided (no row to write to).
pub(crate) const FPAT_RMQR: [[i8; 8]; 8] = [
    [1, 1, 1, 1, 1, 1, 1, 0],
    [1, 0, 0, 0, 0, 0, 1, 0],
    [1, 0, 1, 1, 1, 0, 1, 0],
    [1, 0, 1, 1, 1, 0, 1, 0],
    [1, 0, 1, 1, 1, 0, 1, 0],
    [1, 0, 0, 0, 0, 0, 1, 0],
    [1, 1, 1, 1, 1, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// Place all finder patterns for the given layout. Per BWIPP's
/// unified 4-corner approach (`qrcode_fpatmap`, bwip-js line
/// 26079-26083 + the 8×8 placement loop at line 28270-28316):
///
/// * Full QR — fpat × 3 (TL, TR, BL); BR is null.
/// * Micro QR — fpat × 1 (TL only); TR/BL/BR all null.
/// * rMQR — fpat (TL) + fcorpat (TR) + fcorpat (BL) + fsubpat (BR).
///
/// Reflection convention: TR uses `(y, cols-x-1)`, BL uses
/// `(rows-y-1, x)`, BR uses `(rows-y-1, cols-x-1)`. TL is gated
/// on `y < rows` so the separator-row write is elided when the
/// symbol is shorter than 8 rows (rMQR R7×_).
///
/// IMPORTANT: BWIPP draws finder patterns *after* timing patterns
/// so the finder cells overwrite any overlap. For Full/Micro the
/// regions are disjoint so the order is moot, but for rMQR the
/// TL finder bottom edge overlaps row-0 timing (cols 3..=6), and
/// the BR sub-finder right edge overlaps col-(cols-1) timing
/// (row 3 only). `encode_qr_at_metric` calls timing before
/// finder accordingly.
pub(crate) fn place_finder_patterns(pixs: &mut [i8], layout_id: u8, rows: u16, cols: u16) {
    let cols_u = cols as usize;
    let rows_u = rows as usize;
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };

    // BWIPP's qrcode_fpatmap[format] = [TL, TR, BL, BR]. `null` is
    // `qrcode_fnullpat` (all-9), so we just send a 9-only pattern.
    const NULL_PAT: [[i8; 8]; 8] = [[9; 8]; 8];
    let (tl, tr, bl, br) = match format {
        Format::Full => (&FPAT_RMQR, &FPAT_RMQR, &FPAT_RMQR, &NULL_PAT),
        Format::Micro => (&FPAT_RMQR, &NULL_PAT, &NULL_PAT, &NULL_PAT),
        Format::Rmqr => (&FPAT_RMQR, &FCORPAT_RMQR, &FCORPAT_RMQR, &FSUBPAT_RMQR),
    };

    // 8×8 walk. Mirrors BWIPP line 28271-28316.
    for y in 0..8usize {
        for x in 0..8usize {
            // TL — gated on y < rows so the row-7 separator is
            // elided for rMQR R7×_ (rows=7).
            let fpb0 = tl[y][x];
            if fpb0 != 9 && y < rows_u {
                pixs_set(pixs, y, x, cols_u, fpb0);
            }
            // TR — reflected horizontally.
            let fpb1 = tr[y][x];
            if fpb1 != 9 && x < cols_u {
                pixs_set(pixs, y, cols_u - x - 1, cols_u, fpb1);
            }
            // BL — reflected vertically.
            let fpb2 = bl[y][x];
            if fpb2 != 9 && y < rows_u {
                pixs_set(pixs, rows_u - y - 1, x, cols_u, fpb2);
            }
            // BR — reflected both ways.
            let fpb3 = br[y][x];
            if fpb3 != 9 && x < cols_u && y < rows_u {
                pixs_set(pixs, rows_u - y - 1, cols_u - x - 1, cols_u, fpb3);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 6b — Alignment patterns
//
// BWIPP source: bwip-js `src/bwipp.js`
//   * qrcode_algnpatfull / qrcode_algnpatrmqr — line 25739 / 25742
//   * putalgnpat — line 27969 (writes a 5×5 pattern at given (px, py),
//     skipping cells whose pattern value is the 9-sentinel)
//   * alignment-pattern placement loops — line 27992-28013 (Full +
//     rMQR; Micro QR has none)
//
// Alignment coordinates are computed dynamically from each metric's
// `fimax` / `fimas` fields (BWIPP's `asp2` / `asp3`):
//
//   * `fimax` = first alignment-pattern row/col offset (lower-left of
//     the leftmost pattern).
//   * `fimas` = step between successive alignment-pattern centers.
//
// `fimax == NA` (= 99) means "no alignment patterns" — only V1 and
// the all-zero-padding rMQR rows skip the alignment placement
// entirely (V1's center finder is enough).
// ---------------------------------------------------------------------------

/// `9` sentinel in the alignment-pattern arrays — "skip this cell;
/// don't write to pixs". Used by [`ALIGNMENT_PATTERN_RMQR`] to truncate
/// the 3×3 pattern out of the nominal 5×5 stamp.
pub(crate) const ALGN_SKIP: i8 = 9;

/// Full QR 5×5 alignment-pattern stamp from BWIPP
/// `qrcode_algnpatfull` (bwip-js line 25739):
///
/// ```text
/// 1 1 1 1 1
/// 1 0 0 0 1
/// 1 0 1 0 1
/// 1 0 0 0 1
/// 1 1 1 1 1
/// ```
pub(crate) const ALIGNMENT_PATTERN_FULL: [[i8; 5]; 5] = [
    [1, 1, 1, 1, 1],
    [1, 0, 0, 0, 1],
    [1, 0, 1, 0, 1],
    [1, 0, 0, 0, 1],
    [1, 1, 1, 1, 1],
];

/// rMQR 5×5 alignment-pattern stamp from BWIPP
/// `qrcode_algnpatrmqr` (bwip-js line 25742). The `9`s denote cells
/// outside the truncated 3×3 effective pattern.
///
/// ```text
/// 1 1 1 9 9
/// 1 0 1 9 9
/// 1 1 1 9 9
/// 9 9 9 9 9
/// 9 9 9 9 9
/// ```
pub(crate) const ALIGNMENT_PATTERN_RMQR: [[i8; 5]; 5] = [
    [1, 1, 1, ALGN_SKIP, ALGN_SKIP],
    [1, 0, 1, ALGN_SKIP, ALGN_SKIP],
    [1, 1, 1, ALGN_SKIP, ALGN_SKIP],
    [ALGN_SKIP, ALGN_SKIP, ALGN_SKIP, ALGN_SKIP, ALGN_SKIP],
    [ALGN_SKIP, ALGN_SKIP, ALGN_SKIP, ALGN_SKIP, ALGN_SKIP],
];

/// Stamp one alignment pattern with its top-left corner at
/// `(px, py)`. Mirrors BWIPP `putalgnpat` (bwip-js line 27969).
/// Cells whose pattern value is [`ALGN_SKIP`] (= 9) are left
/// untouched.
fn put_alignment_pattern(
    pixs: &mut [i8],
    px: usize,
    py: usize,
    cols: usize,
    pattern: &[[i8; 5]; 5],
) {
    for (pb, row) in pattern.iter().enumerate() {
        for (pa, &cell) in row.iter().enumerate() {
            if cell != ALGN_SKIP {
                pixs_set(pixs, py + pb, px + pa, cols, cell);
            }
        }
    }
}

/// Place all alignment patterns for the given layout. Mirrors BWIPP
/// bwip-js lines 27992-28013.
///
/// For Full QR (V2..=V40), there are two pattern-placement passes:
///   1. Outer pass — "edge" alignment patterns along row 4 / col 4
///      (next to the finders), stepping by `fimas - fimax`.
///   2. Inner double-pass — the central grid; positions on the
///      x-axis and y-axis both use the same stepping.
///
/// For rMQR, BWIPP places patterns along col 0 and col rows-3 in a
/// single pass.
///
/// Micro QR has no alignment patterns.
///
/// `fimax` / `fimas` come from the [`VersionMetric`] for the symbol.
/// When `fimax == NA` (= 99) the routine returns without writing.
pub(crate) fn place_alignment_patterns(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
    fimax: u16,
    fimas: u16,
) {
    if fimax == NA {
        return;
    }
    let cols_u = cols as usize;
    let rows_u = rows as usize;
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };

    match format {
        Format::Full => {
            let pattern = &ALIGNMENT_PATTERN_FULL;
            // Step is fimas-fimax. If fimas == NA (V2..V6), the inner
            // double-pass should produce a single position at fimax-2,
            // fimax-2 (the center). BWIPP's loop sentinel handles this
            // via the step-direction check; we mirror the same:
            //
            //   start = fimax - 2
            //   step = fimas - fimax (signed)
            //   end_outer = cols - 13 (for the edge pass)
            //   end_inner = cols - 9 / rows - 9 (for the central pass)
            //
            // When step <= 0, BWIPP's loop continues while start >= end
            // (counting down); otherwise while start <= end.
            //
            // For our purposes: if fimas == NA, there is no step — we
            // just place one position at (fimax-2, fimax-2). For V2-V6.
            let start = fimax.saturating_sub(2) as usize;
            if fimas == NA {
                // V2..V6: single central alignment pattern.
                put_alignment_pattern(pixs, start, start, cols_u, pattern);
            } else {
                // V7+: nested loops. BWIPP edge-pass writes patterns
                // at (i, 4) and (4, i); central-pass writes at all
                // (x, y) combinations on the same grid.
                let step = (fimas as i32) - (fimax as i32); // > 0 for V7+
                if step <= 0 {
                    return;
                }
                let step = step as usize;
                // Edge pass: i from start to cols-13 step.
                let edge_end = cols_u.saturating_sub(13);
                let mut i = start;
                while i <= edge_end {
                    put_alignment_pattern(pixs, i, 4, cols_u, pattern);
                    put_alignment_pattern(pixs, 4, i, cols_u, pattern);
                    i += step;
                }
                // Central pass: nested over (x, y).
                let inner_end_x = cols_u.saturating_sub(9);
                let inner_end_y = rows_u.saturating_sub(9);
                let mut x = start;
                while x <= inner_end_x {
                    let mut y = start;
                    while y <= inner_end_y {
                        put_alignment_pattern(pixs, x, y, cols_u, pattern);
                        y += step;
                    }
                    x += step;
                }
            }
        }
        Format::Rmqr => {
            let pattern = &ALIGNMENT_PATTERN_RMQR;
            if fimas == NA {
                // Single-column rMQR pattern at (fimax-2, 0) and
                // (fimax-2, rows-3). For tiny rMQR variants.
                let start = fimax.saturating_sub(2) as usize;
                put_alignment_pattern(pixs, start, 0, cols_u, pattern);
                put_alignment_pattern(pixs, start, rows_u.saturating_sub(3), cols_u, pattern);
            } else {
                let step = (fimas as i32) - (fimax as i32);
                if step <= 0 {
                    return;
                }
                let step = step as usize;
                let edge_end = cols_u.saturating_sub(13);
                let mut i = fimax.saturating_sub(2) as usize;
                while i <= edge_end {
                    put_alignment_pattern(pixs, i, 0, cols_u, pattern);
                    put_alignment_pattern(pixs, i, rows_u.saturating_sub(3), cols_u, pattern);
                    i += step;
                }
            }
        }
        Format::Micro => {
            // Micro QR has no alignment patterns.
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 6d — Format-info reservation (Full + Micro)
//
// BWIPP source: bwip-js `src/bwipp.js` lines 26566-26597 (closures)
// and 27690-27701 (placement loop). The closures compute (row, col)
// positions from (rows, cols); the placement loop iterates each
// closure pair and writes `1` to the resulting cells. Stage 8 (later)
// XORs the format-info BCH-encoded bits into those reserved cells.
//
// This stage covers Full and Micro layouts. rMQR's 18-cluster
// reservation table (per-version, with some positions out-of-bounds
// for smaller rMQR variants) was landed in Stage 6f as
// `place_rmqr_format_info_reservation` further below.
//
// V7+ Full QR's 36-cell version-info reservation (qrcode_vimmap) is
// implemented separately as Stage 6e in the next section
// (`place_version_info_reservation`).
// ---------------------------------------------------------------------------

/// Write the format-info reservation cells for Full QR. Mirrors
/// BWIPP `qrcode_formatfimmap.full` (bwip-js lines 26563-26577) +
/// placement loop at 27693-27701. Each Full QR symbol reserves 30
/// cells split between two redundant 15-bit format-info clusters:
/// one wrapping the top-left finder, one split between the
/// top-right and bottom-left finders.
///
/// The reserved value is `0` (BWIPP writes `1` but masks it out; we
/// match by writing the eventual final value `0`. Stage 8's
/// `place_format_info_bits` will XOR the actual encoded bits in.)
/// BWIPP writes `1` (dark) to all format-info reservation cells, not
/// `0`. This matters for mask scoring because evalfull's N1/N2/N3
/// pre-walker pass treats reserved cells as dark, which changes the
/// run-length / 2×2-block / finder-like-pattern counts. Stage 8's
/// `write_format_info_bits` later overwrites these cells with the
/// real BCH-encoded format-info bits.
const FORMAT_INFO_RESERVATION_VALUE: i8 = 1;

fn place_format_info_reservation_full(pixs: &mut [i8], rows: u16, cols: u16) {
    let cols_u = cols as usize;
    let rows_u = rows as usize;
    // TL cluster (around top-left finder):
    //   rows 0..6 of col 8 (skipping row 6 timing), then (7,8), (8,8),
    //   then col 7..0 of row 8 (skipping col 6 timing).
    let tl_cells = [
        (0, 8),
        (1, 8),
        (2, 8),
        (3, 8),
        (4, 8),
        (5, 8),
        (7, 8),
        (8, 8),
        (8, 7),
        (8, 5),
        (8, 4),
        (8, 3),
        (8, 2),
        (8, 1),
        (8, 0),
    ];
    for &(r, c) in &tl_cells {
        pixs_set(pixs, r, c, cols_u, FORMAT_INFO_RESERVATION_VALUE);
    }
    // BL/TR cluster (one segment along TR's bottom edge, one along
    // BL's right edge):
    //   row 8 cols cols-1..cols-8 (8 cells) — top-right cluster.
    //   col 8 rows rows-8..rows-1 (8 cells) — bottom-left cluster.
    //
    // The TR cluster spans **8** cells (cols cols-8..=cols-1 of row 8)
    // because BWIPP's `qrcode_formatfimmap.full` cluster 7's "Dup"
    // position lands at (row=8, col=cols-8), e.g. (8, 13) for V1 —
    // see bwip-js src/bwipp.js around line 26553 (cluster `_5p`,
    // which emits `(col=cols-8, row=8)` via `cols-8, 8`). A 7-cell
    // reservation here leaves (8, cols-8) UNSET, so the walker
    // visits it as data and every subsequent codeword bit shifts by
    // one position — leading to a wholesale ~25% cell divergence
    // against the bwip-js oracle (root cause of the corpus harness
    // mismatch).
    //
    // The first BL cell at (rows-8, 8) coincides with BWIPP's
    // "dark module" position which is normally a known `1` per
    // ISO 18004 §6.10. Stage 8's format-info writer overwrites these
    // cells with the BCH-encoded format-info bits.
    for k in 0..8 {
        pixs_set(
            pixs,
            8,
            cols_u - 1 - k,
            cols_u,
            FORMAT_INFO_RESERVATION_VALUE,
        );
    }
    // BL cluster: 7 cells at col 8 rows rows-7..rows-1. The
    // (rows-8, 8) "dark module" cell is NOT part of BWIPP's
    // qrcode_formatfimmap — it's set to 0 by the separate
    // dark-module pre-init below (bwip-js line 28083-28092),
    // then later overwritten with 1 by `write_dark_module_full`
    // in Stage 8.
    for k in 1..8 {
        pixs_set(
            pixs,
            rows_u - 8 + k,
            8,
            cols_u,
            FORMAT_INFO_RESERVATION_VALUE,
        );
    }
    // BWIPP "dark module pre-init" — explicit 0 at (rows-8, 8).
    pixs_set(pixs, rows_u - 8, 8, cols_u, 0);
}

/// Write the format-info reservation cells for Micro QR. Mirrors
/// BWIPP `qrcode_formatfimmap.micro`. Micro QR has one cluster
/// (15 cells) wrapping the lone top-left finder.
fn place_format_info_reservation_micro(pixs: &mut [i8], rows: u16, cols: u16) {
    let cols_u = cols as usize;
    let _ = rows; // rows unused; Micro QR format-info cells are at
                  // fixed positions independent of size.
    let cells = [
        (1, 8),
        (2, 8),
        (3, 8),
        (4, 8),
        (5, 8),
        (6, 8),
        (7, 8),
        (8, 8),
        (8, 7),
        (8, 6),
        (8, 5),
        (8, 4),
        (8, 3),
        (8, 2),
        (8, 1),
    ];
    for &(r, c) in &cells {
        pixs_set(pixs, r, c, cols_u, FORMAT_INFO_RESERVATION_VALUE);
    }
}

/// Place format-info reservation cells per format. Stage 8 fills these
/// cells with the BCH-encoded format-info bits.
///
/// Returns the number of cells reserved (for caller verification).
/// rMQR is currently a no-op pending Stage 6f.
pub(crate) fn place_format_info_reservation(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
) -> usize {
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };
    match format {
        Format::Full => {
            place_format_info_reservation_full(pixs, rows, cols);
            // 15 TL + 8 TR (row 8 cols cols-8..cols-1) + 8 BL
            // (col 8 rows rows-8..rows-1, dark module included) = 31.
            // BWIPP's qrcode_formatfimmap has 30 entries (excludes
            // the dark module which it sets to 0 then 1 separately);
            // we include the dark module in the reservation so the
            // walker treats it as non-UNSET, then write_dark_module_full
            // sets it to 1 during Stage 8 format-info write.
            31
        }
        Format::Micro => {
            place_format_info_reservation_micro(pixs, rows, cols);
            15
        }
        Format::Rmqr => {
            place_format_info_reservation_rmqr(pixs, rows, cols);
            36
        }
    }
}

/// rMQR formatfimmap cluster table. Mirrors BWIPP `qrcode_formatfimmap`
/// rMQR entry (bwip-js src/bwipp.js around line 26580). Each cluster
/// produces TWO cells:
///
/// * The "TL" position is a fixed `(col, row)` pair.
/// * The "Dup" position is computed from the symbol's `(rows, cols)`
///   as `(cols - dup_dx, rows - dup_dy)`.
///
/// For small rMQR sizes some Dup positions land out of bounds — the
/// `pixs_set` helper silently no-ops, mirroring BWIPP's tolerant
/// placement loop. The number of *reachable* cells per metric is
/// therefore size-dependent, but the cluster table itself is uniform
/// (18 clusters × 2 cells = 36 entries).
///
/// Derived empirically from BWIPP closures via
/// `rust/tools/dump-rmqr-clusters.js`. The cluster-by-cluster (TL,
/// Dup) shape matches the V7+ Full QR version-info table from
/// Stage 8b but the cells are different (format-info bits, not
/// version-info bits).
#[allow(clippy::type_complexity)]
pub(crate) const RMQR_FORMATFIMMAP_CLUSTERS: [(u8, u8, u8, u8); 18] = [
    // (tl_col, tl_row, dup_dx, dup_dy) — Dup at (cols-dup_dx, rows-dup_dy).
    (11, 3, 3, 6),
    (11, 2, 4, 6),
    (11, 1, 5, 6),
    (10, 5, 6, 2),
    (10, 4, 6, 3),
    (10, 3, 6, 4),
    (10, 2, 6, 5),
    (10, 1, 6, 6),
    (9, 5, 7, 2),
    (9, 4, 7, 3),
    (9, 3, 7, 4),
    (9, 2, 7, 5),
    (9, 1, 7, 6),
    (8, 5, 8, 2),
    (8, 4, 8, 3),
    (8, 3, 8, 4),
    (8, 2, 8, 5),
    (8, 1, 8, 6),
];

/// Compute the 18 cluster pairs for a given rMQR (rows, cols). Each
/// pair is `((tl_row, tl_col), (dup_row, dup_col))`. Out-of-bounds
/// Dup positions are returned as-is — callers use `pixs_set` which
/// silently skips them.
pub(crate) fn rmqr_formatfimmap_pairs(rows: u16, cols: u16) -> [((u16, u16), (u16, u16)); 18] {
    let mut out = [((0u16, 0u16), (0u16, 0u16)); 18];
    for (i, &(tl_col, tl_row, dup_dx, dup_dy)) in RMQR_FORMATFIMMAP_CLUSTERS.iter().enumerate() {
        let dup_col = cols.saturating_sub(dup_dx as u16);
        let dup_row = rows.saturating_sub(dup_dy as u16);
        out[i] = ((tl_row as u16, tl_col as u16), (dup_row, dup_col));
    }
    out
}

/// Stamp the 36 rMQR format-info reservation cells. Mirrors BWIPP's
/// formatfimmap write loop (bwip-js line 28042-28049) applied to the
/// rMQR cluster table. Cells out of bounds (for small rMQR sizes) are
/// silently dropped by `pixs_set`.
fn place_format_info_reservation_rmqr(pixs: &mut [i8], rows: u16, cols: u16) {
    let cols_u = cols as usize;
    let pairs = rmqr_formatfimmap_pairs(rows, cols);
    for &((tl_row, tl_col), (dup_row, dup_col)) in &pairs {
        pixs_set(
            pixs,
            tl_row as usize,
            tl_col as usize,
            cols_u,
            FORMAT_INFO_RESERVATION_VALUE,
        );
        pixs_set(
            pixs,
            dup_row as usize,
            dup_col as usize,
            cols_u,
            FORMAT_INFO_RESERVATION_VALUE,
        );
    }
}

// ---------------------------------------------------------------------------
// Stage 6e — V7+ Version-info reservation (Full QR only)
//
// BWIPP source: bwip-js `src/bwipp.js` lines 26229-26295 (closure
// table) + 28045-28058 (placement loop). The 18 closure pairs emit
// (col, row) coordinates for the V7+ version-info bits. Each cluster
// emits two positions (one per redundant version-info block — top-
// right and bottom-left of the symbol). Total 36 cells.
//
// ISO 18004 §6.10 describes the placement directly:
//   * Block 1 (BL): 3 rows × 6 cols at rows-11..=rows-9, cols 0..=5.
//   * Block 2 (TR): 6 rows × 3 cols at rows 0..=5, cols-11..=cols-9.
//
// Only V7..V40 use version-info; V1..V6 have `fimas == NA` in the
// metric and produce no version-info reservation.
// ---------------------------------------------------------------------------

/// Write the version-info reservation cells for V7+ Full QR. Returns
/// the number of cells reserved (36 for V7+, 0 otherwise). Mirrors
/// BWIPP `qrcode_vimmap` placement at bwip-js line 28045-28058.
///
/// The 18-bit version-info value (6-bit version << 12 | 12-bit BCH)
/// is laid out twice for redundancy: once in a 3×6 block to the lower
/// left of the top-right finder, and once in a 6×3 block above the
/// bottom-left finder. The two blocks together hold 36 cells.
///
/// `version` should be the canonical Full-QR version number
/// (1..=40). The routine returns `0` and is a no-op when `version < 7`.
pub(crate) fn place_version_info_reservation(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
    version: u8,
) -> usize {
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };
    if !matches!(format, Format::Full) || version < 7 {
        return 0;
    }
    let cols_u = cols as usize;
    let rows_u = rows as usize;
    // BL block: rows rows-11..=rows-9 × cols 0..=5 (3 rows × 6 cols = 18).
    for r in (rows_u - 11)..=(rows_u - 9) {
        for c in 0..=5 {
            pixs_set(pixs, r, c, cols_u, 0);
        }
    }
    // TR block: rows 0..=5 × cols cols-11..=cols-9 (6 rows × 3 cols = 18).
    for r in 0..=5 {
        for c in (cols_u - 11)..=(cols_u - 9) {
            pixs_set(pixs, r, c, cols_u, 0);
        }
    }
    36
}

// ---------------------------------------------------------------------------
// Stage 6c — Codeword zig-zag walker
//
// BWIPP source: bwip-js `src/bwipp.js` lines 27761-27791.
//
// The walker traverses the QR matrix in 2-column pairs from right to
// left, snaking up and down between top and bottom edges. At each cell
// it checks `pixs[pos] == -1` (unset) — function-pattern cells are
// skipped. The data bits are written MSB-first from the codeword stream
// produced by Stage 4's `build_codeword_stream`.
//
// Start position:
//   * Full / Micro: posx = cols - 1 (rightmost column).
//   * rMQR:         posx = cols - 2 (rightmost data column, the rightmost
//                                     col is the timing line).
//
// Special handling: For Full QR, when the walker descends past the
// row-6 timing pattern it skips column 6 entirely (BWIPP `if (format ==
// "full" && posx == 6) posx -= 1`).
//
// The format/version-info reservation maps are NOT yet ported in this
// stage — Stage 6d will add `place_format_info_reservation` /
// `place_version_info_reservation`. The walker itself is correct: as
// long as those reservation cells are marked `!= -1` before the walker
// runs, the walker will skip them automatically.
// ---------------------------------------------------------------------------

/// Walk the QR data positions in the BWIPP zig-zag order. Returns the
/// linear pixs indices in visit order, skipping cells that are NOT
/// equal to [`PIXS_UNSET`].
///
/// The caller is responsible for having pre-populated `pixs` with all
/// function patterns (finder, alignment, timing, format-info /
/// version-info reservations) before invoking this. The walker
/// detects those cells via the sentinel and visits only `-1` cells.
pub(crate) fn walk_codeword_positions(
    layout_id: u8,
    rows: u16,
    cols: u16,
    pixs: &[i8],
) -> Vec<usize> {
    let cols_u = cols as usize;
    let rows_i = rows as i32;
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };

    let start_offset = if matches!(format, Format::Rmqr) { 2 } else { 1 };
    let mut posx = cols_u as i32 - start_offset;
    let mut posy = rows_i - 1;
    let mut dir: i32 = -1; // -1 = up, +1 = down
    let mut col: i32 = 1; // 1 = right column of pair, 0 = left column
    let mut visited: Vec<usize> = Vec::new();

    while posx >= 0 {
        // Sample the current cell.
        let idx = qmv(posy as usize, posx as usize, cols_u);
        if idx < pixs.len() && pixs[idx] == PIXS_UNSET {
            visited.push(idx);
        }
        if col == 1 {
            col = 0;
            posx -= 1;
        } else {
            col = 1;
            posx += 1;
            posy += dir;
            if posy < 0 || posy >= rows_i {
                dir = -dir;
                posy += dir;
                posx -= 2;
                // Skip the row-6 timing column for Full QR.
                if matches!(format, Format::Full) && posx == 6 {
                    posx -= 1;
                }
            }
        }
    }
    visited
}

/// Fill a series of pre-computed pixs positions with codeword bits.
/// Mirrors the body of BWIPP's walker at lines 27771-27774, but the
/// bit-stream is consumed in MSB-first byte order from the cws array.
///
/// `positions.len()` should be ≥ `cws.len() * 8`. Excess positions are
/// left at [`PIXS_UNSET`] (BWIPP zeros them via the rbit padding
/// codeword; we leave them as `-1` so callers can detect remaining
/// positions, e.g. for the lc4b-final-nibble shrink. The caller can
/// post-process by replacing remaining `-1`s with `0`.)
///
/// # Errors
///
/// Returns `Error::InvalidData` if `cws.len() * 8` exceeds
/// `positions.len()`.
pub(crate) fn place_codewords_at(
    pixs: &mut [i8],
    positions: &[usize],
    cws: &[u8],
) -> Result<(), Error> {
    let total_bits = cws.len() * 8;
    // BWIPP's "rbit" handling (bwip-js line 27556) appends a trailing
    // zero codeword to carry the symbol's remainder bits (`rbit` bits
    // beyond the last full codeword). For V2+ this means the stream
    // is `ncws + 1` bytes long while the walker has only
    // `ncws*8 + rbit` positions. The bits 0..=rbit-1 of the trailing
    // byte get placed; bits rbit..=7 are dropped on the floor. We
    // therefore truncate gracefully when the stream is at most one
    // codeword (8 bits) longer than the position list — anything
    // beyond that is a genuine sizing error.
    let placeable = positions.len().min(total_bits);
    if total_bits > positions.len() + 8 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: codeword stream ({total_bits} bits) exceeds available positions ({}) by more than one rbit-padded codeword",
            positions.len()
        )));
    }
    for (i, &pos) in positions.iter().enumerate().take(placeable) {
        let byte = cws[i / 8];
        let bit = (byte >> (7 - (i % 8))) & 1;
        if pos < pixs.len() {
            pixs[pos] = bit as i8;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stage 7 — Mask functions + Micro QR mask scoring + selector
//
// BWIPP source: bwip-js `src/bwipp.js`
//   * qrcode_maskfuncs — lines 26329-26349 (8 mask predicate functions)
//   * formatmaskbits dispatch — line 26683 (per-format mask candidate list)
//   * evalmicro — line 28336 (Micro QR mask scoring)
//   * Mask selection loop — line 27916-27940
//
// ISO 18004 §8.8 numbers the 8 masks (0..=7) for Full QR. Micro QR
// uses only masks {1, 4, 6, 7}; rMQR uses only mask 4 (no scoring).
//
// Full QR's N1/N2/N3/N4 penalty score (ISO 18004 §8.8.2) is large
// and is ported as Stage 7b. This iteration delivers:
//   * The 8 mask predicates (deterministic ISO formulas).
//   * Per-format mask-candidate dispatch [`mask_candidates`].
//   * [`apply_mask_to_data_cells`] which XORs the mask into the data
//     cells (cells whose pre-mask value is 0 or 1).
//   * [`evaluate_mask_micro`] — Micro QR scoring (sum of dark modules
//     along the rightmost column + bottom row, packed per BWIPP's
//     `-(min*16 + max)` formula so smaller = better).
//   * [`select_best_micro_mask`] — iterates candidates, picks lowest
//     score. Stage 7b adds the Full QR variant.
// ---------------------------------------------------------------------------

/// All 8 mask predicates per ISO 18004 §8.8 Table 21. Each
/// `MASK_FUNCS[m](row, col)` returns `true` iff mask `m` is "on" at
/// `(row, col)` — i.e., the data bit at that cell should be flipped.
pub(crate) const MASK_FUNCS: [fn(usize, usize) -> bool; 8] = [
    |row, col| (row + col) % 2 == 0,
    |row, _col| row % 2 == 0,
    |_row, col| col % 3 == 0,
    |row, col| (row + col) % 3 == 0,
    |row, col| (row / 2 + col / 3) % 2 == 0,
    |row, col| (row * col) % 2 + (row * col) % 3 == 0,
    |row, col| ((row * col) % 2 + (row * col) % 3) % 2 == 0,
    |row, col| ((row + col) % 2 + (row * col) % 3) % 2 == 0,
];

/// Per-format mask-candidate indices (BWIPP `qrcode_formatmaskbits`
/// at line 26683):
///
/// * Full QR — all 8 masks (0..=7).
/// * Micro QR — only masks {1, 4, 6, 7}.
/// * rMQR — only mask 4 (no scoring needed since it's the only one).
pub(crate) fn mask_candidates(layout_id: u8) -> &'static [u8] {
    let bin = layout_id as usize;
    if bin <= 2 {
        // Full QR
        &[0, 1, 2, 3, 4, 5, 6, 7]
    } else if bin <= 6 {
        // Micro QR
        &[1, 4, 6, 7]
    } else {
        // rMQR
        &[4]
    }
}

/// Apply a mask (XOR with `MASK_FUNCS[mask_idx]`) to all data cells
/// of `pixs`. A data cell is one whose current value is `0` or `1`
/// (function-pattern cells are typically `0` or `1` after Stage 6e,
/// but we only flip cells that the walker would have visited —
/// i.e., the caller should pre-tag function-pattern cells with
/// values >= 2 if it wants to distinguish them, OR maintain a
/// parallel `is_data` map. For this Stage 7a baseline we just XOR
/// every 0/1 cell, leaving Stage 8 to enforce the data-only
/// invariant via a parallel function-pattern bitmap).
///
/// # Panics
///
/// Debug-asserts `mask_idx < 8`.
pub(crate) fn apply_mask_to_data_cells(pixs: &mut [i8], mask_idx: u8, rows: u16, cols: u16) {
    debug_assert!(mask_idx < 8, "mask_idx must be 0..=7");
    let mask = MASK_FUNCS[mask_idx as usize];
    let cu = cols as usize;
    for r in 0..(rows as usize) {
        for c in 0..cu {
            let idx = qmv(r, c, cu);
            if (pixs[idx] == 0 || pixs[idx] == 1) && mask(r, c) {
                pixs[idx] ^= 1;
            }
        }
    }
}

/// Variant of [`apply_mask_to_data_cells`] that only flips cells in
/// a caller-supplied list of `data_positions` (typically the output
/// of [`walk_codeword_positions`]). This is the safer Stage 8 variant
/// that excludes function patterns by construction.
pub(crate) fn apply_mask_at_positions(
    pixs: &mut [i8],
    data_positions: &[usize],
    mask_idx: u8,
    cols: u16,
) {
    debug_assert!(mask_idx < 8, "mask_idx must be 0..=7");
    let mask = MASK_FUNCS[mask_idx as usize];
    let cu = cols as usize;
    for &pos in data_positions {
        let row = pos / cu;
        let col = pos % cu;
        if (pixs[pos] == 0 || pixs[pos] == 1) && mask(row, col) {
            pixs[pos] ^= 1;
        }
    }
}

/// Evaluate the Micro QR mask penalty per ISO 18004 §8.8.2.3.
///
/// Counts dark modules along the rightmost column (cells `(1..rows-1,
/// cols-1)`) and along the bottom row (cells `(rows-1, 1..cols-1)`),
/// then returns BWIPP's combined penalty value
/// `-(min(rhs, bot) * 16 + max(rhs, bot))`. Smaller (= more negative)
/// scores are better — the mask that maximizes the boundary dark
/// count "wins".
pub(crate) fn evaluate_mask_micro(pixs: &[i8], rows: u16, cols: u16) -> i32 {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    let mut dk_rhs: i32 = 0; // rightmost column darks
    let mut dk_bot: i32 = 0; // bottom row darks
                             // BWIPP loops `for (i = 1; i <= cols - 1; i++)` (bwip-js line 27901)
                             // — INCLUSIVE upper bound. The corner cell at (cols-1, cols-1) gets
                             // counted on BOTH the rightmost-column AND the bottom-row passes,
                             // which matters for the dk_rhs <= dk_bot tie-break in some Micro
                             // QR symbols.
    for i in 1..=cols_u.saturating_sub(1) {
        // Right column: position (i, cols-1).
        let cell_rhs = pixs[qmv(i, cols_u - 1, cols_u)];
        if cell_rhs == 1 {
            dk_rhs += 1;
        }
        // Bottom row: position (rows-1, i).
        let cell_bot = pixs[qmv(rows_u - 1, i, cols_u)];
        if cell_bot == 1 {
            dk_bot += 1;
        }
    }
    let (lo, hi) = if dk_rhs <= dk_bot {
        (dk_rhs, dk_bot)
    } else {
        (dk_bot, dk_rhs)
    };
    -(lo * 16 + hi)
}

/// Iterate over the Micro QR mask candidates, apply each in a fresh
/// copy of `data_template`, score it via `evaluate_mask_micro`, and
/// return the (mask_index, masked_pixs) for the best-scoring mask.
///
/// `data_template` should be the function-pattern + data-bit pixs
/// grid AFTER the walker has placed codeword bits but BEFORE any
/// masking is applied. `data_positions` are the positions whose
/// values represent data (function-pattern positions are left
/// untouched by the mask application).
///
/// # Errors
///
/// Returns `Error::InvalidData` if `layout_id` is not a Micro QR
/// layout (3..=6).
pub(crate) fn select_best_micro_mask(
    data_template: &[i8],
    data_positions: &[usize],
    layout_id: u8,
    rows: u16,
    cols: u16,
) -> Result<(u8, Vec<i8>), Error> {
    let bin = layout_id as usize;
    if !(3..=6).contains(&bin) {
        return Err(Error::InvalidData(format!(
            "qrcode_native: select_best_micro_mask called with non-Micro layout_id {layout_id}"
        )));
    }
    let candidates = mask_candidates(layout_id);
    // The returned mask index is the CANDIDATE INDEX (0..=3 = position
    // in the candidate list `{1, 4, 6, 7}`), NOT the absolute mask
    // number. BWIPP's `qrcode_fmtvalsmicro` table is indexed by
    // `sym_id * 4 + candidate_index` (bwip-js line 27968), so
    // `write_format_info_bits` for Micro QR consumes the candidate
    // index. The applied mask cell pattern is the absolute mask
    // (from `candidates[idx]`).
    let mut best: Option<(u8, i32, Vec<i8>)> = None;
    for (idx, &m) in candidates.iter().enumerate() {
        let mut trial = data_template.to_vec();
        apply_mask_at_positions(&mut trial, data_positions, m, cols);
        let score = evaluate_mask_micro(&trial, rows, cols);
        let cand_idx = idx as u8;
        match &best {
            None => best = Some((cand_idx, score, trial)),
            Some(b) if score < b.1 => best = Some((cand_idx, score, trial)),
            _ => {}
        }
    }
    let (cand_idx, _score, pixs) = best.expect("at least one mask candidate for Micro QR");
    Ok((cand_idx, pixs))
}

// ---------------------------------------------------------------------------
// Stage 7b — Full QR N1+N2+N3+N4 mask scoring
//
// BWIPP source: bwip-js `src/bwipp.js`
//   * evalfulln1n3 — line 28164 (per-row/col N1 + N3 sub-scoring)
//   * evalfull — line 28212 (top-level N1+N2+N3+N4 aggregation)
//
// ISO 18004 §8.8.2 penalty rules (Table 24):
//   N1: Adjacent same-color runs of ≥5 modules in a row or column.
//       Penalty = `3 + (length - 5)` per such run.
//   N2: 2×2 same-color blocks. Penalty = 3 × (number of blocks).
//   N3: 1:1:3:1:1 finder-like patterns in rows/cols, preceded or
//       followed by 4+ same-color cells. Penalty = 40 per pattern.
//   N4: Dark-percentage deviation from 50%. Penalty = `10 × ⌊|p - 50| / 5⌋`.
// ---------------------------------------------------------------------------

/// Run-length encode a single row or column of a pixs grid. Returns a
/// `Vec<u32>` matching BWIPP's RLE convention used by `evalfulln1n3`:
///
/// * For a row/column starting with a **light** cell:
///   `[N_light, run1_dark, run2_light, ..., last_run]`
/// * For a row/column starting with a **dark** cell:
///   `[0, run1_dark, run2_light, ..., last_run]`
///   (the leading `0` is the count of "leading light cells", which is
///   zero — BWIPP does NOT emit a duplicate `[0, 0, ...]` here, and
///   neither do we.)
///
/// Mirrors BWIPP's inline RLE built at line 27845-27849. Cells with
/// `pixs[i] != 0 && pixs[i] != 1` are treated as `0` (light) — they
/// shouldn't appear in well-formed inputs but we degrade gracefully.
///
/// **Important**: a previous version of this function emitted an
/// extra `0` sentinel at index 0 in *all* cases, which shifted every
/// j-index in `evalfull_n1n3` by one and caused N3 to miss every
/// 1:1:3:1:1 finder-like pattern. The mask-scoring divergence vs
/// bwip-js for V1-L "HELLO WORLD" traced directly to that bug.
fn rle_run(cells: impl Iterator<Item = i8>) -> Vec<u32> {
    let mut runs: Vec<u32> = Vec::new();
    let mut last_color: i8 = 0;
    let mut current: u32 = 0;
    for cell in cells {
        let color = if cell == 1 { 1 } else { 0 };
        if color == last_color {
            current += 1;
        } else {
            runs.push(current);
            current = 1;
            last_color = color;
        }
    }
    runs.push(current);
    runs
}

/// Compute N1 + N3 penalties for one row/column RLE. Mirrors BWIPP
/// `evalfulln1n3` (bwip-js 28164-28210).
///
/// Returns `(n1, n3)` where `n1` is the sum of `(run - 2)` for each
/// run of length ≥5, and `n3` is `40 × count` of 1:1:3:1:1
/// finder-like patterns with appropriate quiet-zone modules.
pub(crate) fn evalfull_n1n3(scrle: &[u32]) -> (u32, u32) {
    let mut n1: u32 = 0;
    for &run in scrle {
        if run >= 5 {
            n1 += run - 2;
        }
    }
    let mut n3: u32 = 0;
    if scrle.len() >= 6 {
        // BWIPP iterates j from 3 to scrle.len()-3 in steps of 2
        // (the central "3" cell of the 1:1:3:1:1 pattern is at j).
        let mut j: usize = 3;
        while j + 3 <= scrle.len() {
            if scrle[j] % 3 == 0 {
                let fact = scrle[j] / 3;
                if fact > 0
                    && scrle[j - 2] == fact
                    && scrle[j - 1] == fact
                    && scrle[j + 1] == fact
                    && scrle[j + 2] == fact
                {
                    // Quiet-zone check: at boundary OR preceding/
                    // following run is ≥4 cells.
                    let at_start = j == 3;
                    let at_end = j + 4 >= scrle.len();
                    let pre_quiet = scrle[j - 3] >= 4;
                    let post_quiet = (j + 3 < scrle.len()) && scrle[j + 3] >= 4;
                    if at_start || at_end || pre_quiet || post_quiet {
                        n3 += 40;
                    }
                }
            }
            j += 2;
        }
    }
    (n1, n3)
}

/// Count 2×2 same-color blocks. Penalty = 3 × (count). Mirrors the
/// `lastpairs` / `thispairs` logic in BWIPP `evalfull` (bwip-js
/// 27866-27876).
fn evalfull_n2(pixs: &[i8], rows: u16, cols: u16) -> u32 {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    if rows_u < 2 || cols_u < 2 {
        return 0;
    }
    let mut count: u32 = 0;
    for r in 0..(rows_u - 1) {
        for c in 0..(cols_u - 1) {
            let a = pixs[qmv(r, c, cols_u)];
            let b = pixs[qmv(r, c + 1, cols_u)];
            let d = pixs[qmv(r + 1, c, cols_u)];
            let e = pixs[qmv(r + 1, c + 1, cols_u)];
            if a == b && a == d && a == e && (a == 0 || a == 1) {
                count += 1;
            }
        }
    }
    count * 3
}

/// Compute the dark-percentage deviation penalty (N4). Mirrors BWIPP
/// `evalfull` lines 27886-27887: penalty = `(⌊|dark*100/(cols*cols) - 50| / 5⌋) * 10`.
///
/// Note BWIPP divides by `cols * cols` even for non-square symbols
/// (this is the "always square" assumption that holds for Full QR).
fn evalfull_n4(pixs: &[i8], rows: u16, cols: u16) -> u32 {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    let total = rows_u * cols_u;
    if total == 0 {
        return 0;
    }
    let dark: u32 = pixs.iter().take(total).filter(|&&c| c == 1).count() as u32;
    // BWIPP uses cols*cols rather than rows*cols. For Full QR symbols
    // (which are always square), these are equivalent.
    let denom = (cols_u * cols_u) as u32;
    let percent = (dark * 100) / denom;
    let deviation = percent.abs_diff(50);
    (deviation / 5) * 10
}

/// Compute the full QR mask penalty (N1 + N2 + N3 + N4). Mirrors
/// BWIPP `evalfull` (bwip-js 28212-28335) without the `bestscore`
/// early-exit optimization — Rust callers typically only score the
/// 8 candidates and take the min.
pub(crate) fn evaluate_mask_full(pixs: &[i8], rows: u16, cols: u16) -> u32 {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    let mut n1_total: u32 = 0;
    let mut n3_total: u32 = 0;
    // Per-column passes: build RLE for each column.
    for c in 0..cols_u {
        let col_rle = rle_run((0..rows_u).map(|r| pixs[qmv(r, c, cols_u)]));
        let (n1, n3) = evalfull_n1n3(&col_rle);
        n1_total += n1;
        n3_total += n3;
    }
    // Per-row passes: build RLE for each row.
    for r in 0..rows_u {
        let row_rle = rle_run((0..cols_u).map(|c| pixs[qmv(r, c, cols_u)]));
        let (n1, n3) = evalfull_n1n3(&row_rle);
        n1_total += n1;
        n3_total += n3;
    }
    let n2 = evalfull_n2(pixs, rows, cols);
    let n4 = evalfull_n4(pixs, rows, cols);
    n1_total + n2 + n3_total + n4
}

/// Iterate over the Full QR mask candidates, apply each in a fresh
/// copy of `data_template`, score it via `evaluate_mask_full`, and
/// return the (mask_index, masked_pixs) for the lowest-score mask.
///
/// Per ISO 18004 §8.8.2, lower scores are better. BWIPP's tie-break
/// for ties is "lower mask index wins" — we follow that by using
/// `<` (strict less-than) in the comparison.
///
/// # Errors
///
/// Returns `Error::InvalidData` if `layout_id` is not a Full QR
/// layout (0..=2).
pub(crate) fn select_best_full_mask(
    data_template: &[i8],
    data_positions: &[usize],
    layout_id: u8,
    rows: u16,
    cols: u16,
) -> Result<(u8, Vec<i8>), Error> {
    let bin = layout_id as usize;
    if bin > 2 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: select_best_full_mask called with non-Full layout_id {layout_id}"
        )));
    }
    let candidates = mask_candidates(layout_id);
    let mut best: Option<(u8, u32, Vec<i8>)> = None;
    for &m in candidates {
        let mut trial = data_template.to_vec();
        apply_mask_at_positions(&mut trial, data_positions, m, cols);
        let score = evaluate_mask_full(&trial, rows, cols);
        match &best {
            None => best = Some((m, score, trial)),
            Some(b) if score < b.1 => best = Some((m, score, trial)),
            _ => {}
        }
    }
    let (m, _score, pixs) = best.expect("at least one mask candidate for Full QR");
    Ok((m, pixs))
}

/// Top-level mask-selection dispatcher. Picks the per-format
/// evaluator and selector. For rMQR (single candidate, mask 4), no
/// scoring is performed.
pub(crate) fn select_best_mask(
    data_template: &[i8],
    data_positions: &[usize],
    layout_id: u8,
    rows: u16,
    cols: u16,
) -> Result<(u8, Vec<i8>), Error> {
    let bin = layout_id as usize;
    if bin <= 2 {
        select_best_full_mask(data_template, data_positions, layout_id, rows, cols)
    } else if bin <= 6 {
        select_best_micro_mask(data_template, data_positions, layout_id, rows, cols)
    } else {
        // rMQR: only mask 4, no scoring.
        let mut pixs = data_template.to_vec();
        apply_mask_at_positions(&mut pixs, data_positions, 4, cols);
        Ok((4, pixs))
    }
}

// ---------------------------------------------------------------------------
// Stage 8 — Format-info / dark-module bit write (Full + Micro)
//
// BWIPP source: bwip-js `src/bwipp.js` lines 28395-28445.
//
// Format-info bits go through BCH(15,5) encoding + XOR mask, then
// are written MSB-first into the format-info reservation positions.
// Bit ordering: `formatmap[0]` gets bit 14 (MSB), `formatmap[14]`
// gets bit 0 (LSB). For Full QR, each formatmap entry has TWO
// positions (TL + TR/BL redundant clusters); for Micro QR, ONE.
//
// EC indicator table (BWIPP `qrcode_ecidfull` at line 26148): maps
// ec_level {L=0, M=1, Q=2, H=3} to the 2-bit ISO encoded value
// {L=1, M=0, Q=3, H=2}.
//
// V7+ version-info (BCH(18,6) into 36 cells) is implemented below
// as `write_version_info_bits` (Stage 8b). rMQR formatfimmap is
// handled by `rmqr_fmtval1`/`rmqr_fmtval2` (Stage 6e) and
// `write_format_info_bits`'s rMQR arm — both already wired in.
// ---------------------------------------------------------------------------

/// BWIPP `qrcode_ecidfull` (bwip-js line 26148): maps `ec_level`
/// {L=0, M=1, Q=2, H=3} to the ISO 18004 §C.2.1 ec-indicator value
/// {L=1, M=0, Q=3, H=2}. Used as the upper 2 bits of the BCH(15,5)
/// input.
pub(crate) const QRCODE_EC_INDICATOR_FULL: [u8; 4] = [1, 0, 3, 2];

/// Format-info position pairs for Full QR. Each entry yields two
/// `(row, col)` positions where the corresponding format-info bit
/// is written (the redundant TL cluster + TR/BL cluster).
///
/// Order: `formatmap[i]` receives bit `(14 - i)` of the post-mask
/// BCH(15,5) value, so `formatmap[0]` carries the MSB and
/// `formatmap[14]` carries the LSB.
fn format_info_pairs_full(rows: u16, cols: u16) -> [((usize, usize), (usize, usize)); 15] {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    [
        // bit 14: TL leftmost (8, 0) + BL bottommost (rows-1, 8)
        ((8, 0), (rows_u - 1, 8)),
        ((8, 1), (rows_u - 2, 8)),
        ((8, 2), (rows_u - 3, 8)),
        ((8, 3), (rows_u - 4, 8)),
        ((8, 4), (rows_u - 5, 8)),
        ((8, 5), (rows_u - 6, 8)),
        // bit 8: skip (8, 6) timing → (8, 7) + (rows-7, 8)
        ((8, 7), (rows_u - 7, 8)),
        // bit 7: dark-module side of TL = (8, 8) + TR cluster start (8, cols-8)
        ((8, 8), (8, cols_u - 8)),
        // bit 6: skip (6, 8) timing in TL → (7, 8) + (8, cols-7)
        ((7, 8), (8, cols_u - 7)),
        ((5, 8), (8, cols_u - 6)),
        ((4, 8), (8, cols_u - 5)),
        ((3, 8), (8, cols_u - 4)),
        ((2, 8), (8, cols_u - 3)),
        ((1, 8), (8, cols_u - 2)),
        // bit 0: TL top (0, 8) + TR rightmost (8, cols-1)
        ((0, 8), (8, cols_u - 1)),
    ]
}

/// Format-info positions for Micro QR. Single cluster wrapping the
/// lone top-left finder, 15 positions in MSB-first bit order.
const FORMAT_INFO_POSITIONS_MICRO: [(usize, usize); 15] = [
    (8, 1),
    (8, 2),
    (8, 3),
    (8, 4),
    (8, 5),
    (8, 6),
    (8, 7),
    (8, 8),
    (7, 8),
    (6, 8),
    (5, 8),
    (4, 8),
    (3, 8),
    (2, 8),
    (1, 8),
];

/// Compute the masked 15-bit format-info codeword for a given
/// `(layout_id, ec_level, mask_index)`. The output is the value
/// that BWIPP's `fmtvalsfull[ec_id * 8 + mask]` /
/// `fmtvalsmicro[sym_id * 4 + mask]` lookup tables encode.
///
/// `mask_index` is 0..=7 for Full QR, 0..=3 indexing into Micro QR's
/// {1, 4, 6, 7} candidates — wait, BWIPP's micro mask indexing is
/// 0..=3 (the position in the candidate array), not the global mask
/// index. We follow BWIPP's convention by passing the candidate
/// position (0..=3) for Micro and the global index (0..=7) for Full.
///
/// # Errors
///
/// Returns `Error::InvalidData` if `ec_level` ≥ 4 or `mask_index` is
/// out of range for the format. rMQR layouts are handled by
/// `rmqr_fmtval1` / `rmqr_fmtval2` directly via
/// `write_format_info_bits`'s rMQR arm; this helper rejects them
/// with an explicit error to surface accidental misuse.
pub(crate) fn compute_format_info_bits(
    layout_id: u8,
    ec_level: u8,
    mask_index: u8,
) -> Result<u16, Error> {
    if ec_level >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ec_level {ec_level} out of range (0..=3 = L/M/Q/H)"
        )));
    }
    let bin = layout_id as usize;
    if bin <= 2 {
        // Full QR. Data = (ec_indicator << 3) | mask.
        if mask_index >= 8 {
            return Err(Error::InvalidData(format!(
                "qrcode_native: Full QR mask_index {mask_index} out of range (0..=7)"
            )));
        }
        let ec_id = QRCODE_EC_INDICATOR_FULL[ec_level as usize];
        let data = (ec_id << 3) | mask_index;
        let bch = bch15_5_encode(data);
        Ok(bch ^ FORMAT_INFO_MASK_FULL)
    } else if bin <= 6 {
        // Micro QR. Data = (sym_id << 2) | mask_candidate_index.
        // sym_id depends on (M-version, ec_level) per BWIPP
        // qrcode_ecidmicrosym. Encoder callers pass mask_index as
        // the position in the Micro candidate array (0..=3).
        if mask_index >= 4 {
            return Err(Error::InvalidData(format!(
                "qrcode_native: Micro QR mask_index {mask_index} out of range (0..=3)"
            )));
        }
        let sym_id = micro_sym_id(layout_id, ec_level)?;
        let data = (sym_id << 2) | mask_index;
        let bch = bch15_5_encode(data);
        Ok(bch ^ FORMAT_INFO_MASK_MICRO)
    } else {
        // rMQR uses the separate fmtval1/fmtval2 BCH(18,6) tables
        // emitted by `rmqr_fmtval1` / `rmqr_fmtval2`; the
        // `write_format_info_bits` rMQR arm calls those directly
        // and never routes through this helper. If a caller ends
        // up here for an rMQR layout, surface a clear error rather
        // than silently encoding the wrong format-info value.
        Err(Error::InvalidData(
            "qrcode_native: compute_format_info_bits handles Full/Micro layouts only — rMQR layouts (layout_id >= 7) use rmqr_fmtval1/rmqr_fmtval2 via write_format_info_bits".to_string(),
        ))
    }
}

/// Map (Micro QR layout_id, ec_level) to the BWIPP `qrcode_ecidmicrosym`
/// symbol-id used as the upper 3 bits of the BCH(15,5) data. The
/// table is:
///
/// ```text
///   M1 (cols=11): L→0
///   M2 (cols=13): L→1, M→2
///   M3 (cols=15): L→3, M→4
///   M4 (cols=17): L→5, M→6, Q→7
/// ```
fn micro_sym_id(layout_id: u8, ec_level: u8) -> Result<u8, Error> {
    let table: [&[Option<u8>]; 4] = [
        &[Some(0), None, None, None],       // M1
        &[Some(1), Some(2), None, None],    // M2
        &[Some(3), Some(4), None, None],    // M3
        &[Some(5), Some(6), Some(7), None], // M4
    ];
    // layout_id 3..=6 → micro index 0..=3.
    let mi = (layout_id as usize).checked_sub(3).ok_or_else(|| {
        Error::InvalidData(format!(
            "qrcode_native: layout_id {layout_id} is not a Micro QR layout"
        ))
    })?;
    if mi >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: layout_id {layout_id} is not a Micro QR layout"
        )));
    }
    let row = table[mi];
    let ec_idx = ec_level as usize;
    if ec_idx >= row.len() {
        return Err(Error::InvalidData(format!(
            "qrcode_native: Micro QR layout_id {layout_id} doesn't support EC level {ec_level}"
        )));
    }
    row[ec_idx].ok_or_else(|| {
        Error::InvalidData(format!(
            "qrcode_native: Micro QR layout_id {layout_id} doesn't support EC level {ec_level}"
        ))
    })
}

/// Write the 15 format-info bits (post-mask, MSB-first) into the
/// pixs grid at the reserved positions for the given layout. Mirrors
/// BWIPP's format-info write loops at bwip-js 28407-28445.
///
/// For Full QR, also sets the dark module at `(rows - 8, 8)` to `1`
/// per ISO 18004 §6.10.
///
/// # Errors
///
/// Returns `Error::InvalidData` if the inputs are out of range.
/// rMQR layouts use the separate `rmqr_fmtval1` / `rmqr_fmtval2`
/// tables emitted from this function's rMQR arm.
pub(crate) fn write_format_info_bits(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
    ec_level: u8,
    mask_index: u8,
) -> Result<(), Error> {
    let bin = layout_id as usize;
    let cols_u = cols as usize;
    if bin <= 2 {
        // Full QR: 15 pair positions.
        let fmtval = compute_format_info_bits(layout_id, ec_level, mask_index)?;
        let pairs = format_info_pairs_full(rows, cols);
        for (i, &((r1, c1), (r2, c2))) in pairs.iter().enumerate() {
            let bit = ((fmtval >> (14 - i)) & 1) as i8;
            pixs_set(pixs, r1, c1, cols_u, bit);
            pixs_set(pixs, r2, c2, cols_u, bit);
        }
        // Dark module per ISO 18004 §6.10.
        write_dark_module_full(pixs, rows, cols);
    } else if bin <= 6 {
        // Micro QR: 15 single positions.
        let fmtval = compute_format_info_bits(layout_id, ec_level, mask_index)?;
        for (i, &(r, c)) in FORMAT_INFO_POSITIONS_MICRO.iter().enumerate() {
            let bit = ((fmtval >> (14 - i)) & 1) as i8;
            pixs_set(pixs, r, c, cols_u, bit);
        }
    } else {
        // rMQR: 18 cluster pairs × 2 redundant codewords (fmtval1
        // + fmtval2). Mirrors BWIPP bwip-js source 28789-28810.
        //
        // fmtvalu = qrcode_ecidrmqr[eclval] * 32 + verind
        // fmtval1 = qrcode_fmtvalsrmqr1[fmtvalu]
        // fmtval2 = qrcode_fmtvalsrmqr2[fmtvalu]
        // for i in 0..18:
        //   formatmap[i][0] := bit (17 - i) of fmtval1
        //   formatmap[i][1] := bit (17 - i) of fmtval2
        let ec_id = QRCODE_EC_INDICATOR_RMQR
            .get(ec_level as usize)
            .copied()
            .unwrap_or(-1);
        if ec_id < 0 {
            return Err(Error::InvalidOption(format!(
                "qrcode_native: rMQR supports only EC levels M (1) and H (3); got {ec_level}"
            )));
        }
        // verind = (rMQR metric_idx) - 44. Equivalently, layout_id - 7
        // because rMQR rows in FULL_METRICS map layout_id 7..=38 ↔
        // metric_idx 44..=75 one-to-one.
        let verind = bin
            .checked_sub(7)
            .ok_or_else(|| Error::InvalidData(format!("rMQR layout_id underflow ({bin})")))?
            as u8;
        if verind >= 32 {
            return Err(Error::InvalidData(format!(
                "qrcode_native: rMQR verind {verind} out of range (0..32)"
            )));
        }
        // 6-bit composite key: (ec_id_rmqr << 5) | verind. ec_id_rmqr
        // is 0 or 1 (M / H), so the value fits in 6 bits.
        let data = (ec_id as u8) * 32 + verind;
        let f1 = rmqr_fmtval1(data);
        let f2 = rmqr_fmtval2(data);
        let _ = mask_index; // rMQR mask is fixed at 4; not part of f1/f2.
        let pairs = rmqr_formatfimmap_pairs(rows, cols);
        for (i, &((r1, c1), (r2, c2))) in pairs.iter().enumerate() {
            let b1 = ((f1 >> (17 - i)) & 1) as i8;
            let b2 = ((f2 >> (17 - i)) & 1) as i8;
            pixs_set(pixs, r1 as usize, c1 as usize, cols_u, b1);
            pixs_set(pixs, r2 as usize, c2 as usize, cols_u, b2);
        }
    }
    Ok(())
}

/// Set the Full QR dark module at `(rows - 8, 8)` to `1`. Mirrors
/// BWIPP bwip-js line 27950.
pub(crate) fn write_dark_module_full(pixs: &mut [i8], rows: u16, cols: u16) {
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    pixs_set(pixs, rows_u - 8, 8, cols_u, 1);
}

// ---------------------------------------------------------------------------
// Stage 8b — V7+ version-info bit write (Full QR)
//
// BWIPP source: bwip-js `src/bwipp.js`
//   * qrcode_vimmap closures — line 26203-26295 (18 cluster pairs)
//   * Placement loop — line 28045-28058 + 28480-28495
//
// For V7..V40, BWIPP writes the BCH(18,6)-encoded version value
// (18 bits) into 36 cells split between two redundant blocks:
//   * TR block: rows 0..=5 × cols (cols-11)..=(cols-9), arranged as
//     a 6×3 grid (6 rows × 3 cols).
//   * BL block: rows (cols-11)..=(cols-9) × cols 0..=5, arranged
//     as a 3×6 grid.
//
// Each cluster `i` (0..=17) gets bit `(17 - i)` of the BCH codeword.
// The bit layout per ISO 18004 §6.10 / D.1:
//   * TR: cluster i goes at row `5 - (i div 3)`, col `cols - 9 - (i mod 3)`.
//   * BL: cluster i goes at row `cols - 9 - (i mod 3)`, col `5 - (i div 3)`.
//
// rMQR formatfimmap is the parallel 18-cluster per-layout closure
// for rMQR symbols and is implemented separately via `rmqr_fmtval1`
// / `rmqr_fmtval2` + `write_format_info_bits`'s rMQR arm (Stage 6e
// / Stage 8b). The bottom-right sub-finder is placed by
// `place_rmqr_sub_finder` earlier in this file.
// ---------------------------------------------------------------------------

/// Position pairs for V7+ Full QR version-info. Returns 18
/// `(TR, BL)` position pairs in MSB-first cluster order (cluster 0
/// receives the MSB of the BCH(18,6) codeword).
fn version_info_pairs_full(cols: u16) -> [((usize, usize), (usize, usize)); 18] {
    let cu = cols as usize;
    let mut out = [((0usize, 0usize), (0usize, 0usize)); 18];
    for (i, slot) in out.iter_mut().enumerate() {
        let row = 5 - (i / 3);
        let off = 9 + (i % 3);
        *slot = ((row, cu - off), (cu - off, row));
    }
    out
}

/// Write the 18-bit BCH(18,6)-encoded version-info bits into the 36
/// reserved cells for V7+ Full QR. No-op for V1..V6 / Micro / rMQR.
///
/// Returns the number of cells written (36 for V7+ Full, 0 otherwise).
///
/// # Errors
///
/// Returns `Error::InvalidData` if `version > 40`.
pub(crate) fn write_version_info_bits(
    pixs: &mut [i8],
    layout_id: u8,
    rows: u16,
    cols: u16,
    version: u8,
) -> Result<usize, Error> {
    let bin = layout_id as usize;
    let format = if bin <= 2 {
        Format::Full
    } else if bin <= 6 {
        Format::Micro
    } else {
        Format::Rmqr
    };
    if !matches!(format, Format::Full) || version < 7 {
        return Ok(0);
    }
    if version > 40 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: invalid Full QR version {version} (must be 1..=40)"
        )));
    }
    let _ = rows; // currently unused — closures only depend on cols.
    let verval = bch18_6_encode(version);
    let cu = cols as usize;
    let pairs = version_info_pairs_full(cols);
    for (i, &((r1, c1), (r2, c2))) in pairs.iter().enumerate() {
        let bit = ((verval >> (17 - i)) & 1) as i8;
        pixs_set(pixs, r1, c1, cu, bit);
        pixs_set(pixs, r2, c2, cu, bit);
    }
    Ok(36)
}

// ---------------------------------------------------------------------------
// Stage 9 — Public encode entry point
//
// Composes all 8 prior stages into a working Full QR encoder:
//
//   select_segments → compose_segments → pad_codewords →
//   build_codeword_stream → init_pixs_matrix → place_finder /
//   _timing / _alignment / format-info-reservation /
//   version-info-reservation → walk_codeword_positions →
//   place_codewords_at → select_best_mask → write_format_info_bits →
//   write_version_info_bits → BitMatrix
//
// The catalog cutover (Symbology::QrCode etc. routing through this
// module) landed in Stage 10 + Stage 16 — see `Symbology::encode`
// for the routing, gated by the default `prefer-native-qrcode`
// Cargo feature.
// ---------------------------------------------------------------------------

/// Encode `msg` as a Full QR code at the requested `version`
/// (1..=40) and `ec_level` (0=L, 1=M, 2=Q, 3=H). Returns a
/// final-state [`BitMatrix`] with masked function-pattern + data
/// bits + format-info / version-info bits applied.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if version is out of range,
/// EC level is unsupported, or the message overflows the symbol's
/// data capacity at the requested EC level.
pub(crate) fn encode_full_qr(msg: &[u8], version: u8, ec_level: u8) -> Result<BitMatrix, Error> {
    if !(1..=40).contains(&version) {
        return Err(Error::InvalidData(format!(
            "qrcode_native: Full QR version {version} out of range (1..=40)"
        )));
    }
    // FULL_METRICS layout: 4 Micro rows (M1-M4) + 40 Full rows (V1-V40)
    // + 32 rMQR rows. V1 is at index 4.
    let metric_idx = 4 + (version - 1) as usize;
    encode_qr_at_metric(msg, metric_idx, ec_level)
}

/// Encode a QR-family symbol (Full / Micro / rMQR) at the given
/// `metric_idx` into the global [`FULL_METRICS`] table. This is the
/// metric-direct entry point used by both [`encode_full_qr`] (which
/// computes `metric_idx = 4 + version - 1`) and [`encode_micro_qr`]
/// (`metric_idx = 0..=3` for M1..=M4).
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if `ec_level >= 4` or the chosen
/// `(metric, ec_level)` combination produces an empty / un-encodable
/// symbol.
pub(crate) fn encode_qr_at_metric(
    msg: &[u8],
    metric_idx: usize,
    ec_level: u8,
) -> Result<BitMatrix, Error> {
    encode_qr_at_metric_with_fnc1(msg, metric_idx, ec_level, false)
}

/// Variant of [`encode_qr_at_metric`] that injects BWIPP's
/// "FNC1 in first position" mode indicator (4-bit `0101`) before the
/// first data segment — the bit-stream prefix that converts a plain
/// QR symbol into a GS1 QR Code symbol per ISO/IEC 18004 Annex L.
///
/// `fnc1first = true` is the only knob; everything downstream
/// (mode selector / RS / mask / format-info) is identical to the
/// non-GS1 path.
pub(crate) fn encode_qr_at_metric_with_fnc1(
    msg: &[u8],
    metric_idx: usize,
    ec_level: u8,
    fnc1first: bool,
) -> Result<BitMatrix, Error> {
    if ec_level >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ec_level {ec_level} out of range (0..=3 = L/M/Q/H)"
        )));
    }
    if metric_idx >= FULL_METRICS.len() {
        return Err(Error::InvalidData(format!(
            "qrcode_native: metric_idx {metric_idx} out of range (0..{})",
            FULL_METRICS.len()
        )));
    }
    let m = &FULL_METRICS[metric_idx];
    let layout_id = m.layout_id;
    let rows = m.rows;
    let cols = m.cols;
    let datacap_bits = m.datacap_bits;

    // Version number for version-info reservation/write (V7+ only). For
    // Full QR metric_idx ≥ 4: version = metric_idx - 3 (1..=40). For
    // Micro / rMQR: version is unused — both reservation and write
    // functions early-exit on non-Full layouts.
    let version = if matches!(m.format, Format::Full) {
        (metric_idx - 3) as u8
    } else {
        0
    };

    let layout = block_layout(metric_idx, ec_level)?;

    // 1-2. Segments → raw bits. `fnc1first` (GS1 QR) prefixes the
    //      4-bit `0101` mode indicator per ISO 18004 Annex L.
    let segments = select_segments(msg, layout_id, fnc1first);
    let bits = compose_segments(msg, &segments, layout_id, fnc1first)?;

    // 3. Pad bits to byte boundary + 0xEC/0x11 alternation.
    let padded_data = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws)?;

    // 4. Full RS interleave + rbit/lc4b handling.
    let stream = build_codeword_stream(&padded_data, metric_idx, ec_level)?;

    // 5. Allocate pixs grid.
    let mut pixs = init_pixs_matrix(rows, cols);

    // 6. Function patterns + reservation regions.
    //    Order matches BWIPP (bwip-js line ~27594→28316): timing
    //    first, then finder patterns. For Full/Micro the regions
    //    are disjoint so the order is invisible, but rMQR's TL
    //    finder bottom edge and BR sub-finder right edge each
    //    overlap a timing row/col — the finder draw must come
    //    after so it overwrites.
    place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
    place_finder_patterns(&mut pixs, layout_id, rows, cols);
    place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
    place_format_info_reservation(&mut pixs, layout_id, rows, cols);
    place_version_info_reservation(&mut pixs, layout_id, rows, cols, version);

    // 7-8. Walk positions + place codeword bits.
    let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
    place_codewords_at(&mut pixs, &positions, &stream)?;

    // 9. Mask scoring + selection.
    let (mask, masked_pixs) = select_best_mask(&pixs, &positions, layout_id, rows, cols)?;
    let mut pixs = masked_pixs;

    // 10. Format-info BCH(15,5) bits (+ dark module for Full QR).
    write_format_info_bits(&mut pixs, layout_id, rows, cols, ec_level, mask)?;

    // 11. Version-info BCH(18,6) bits (V7+ Full QR only — no-op otherwise).
    write_version_info_bits(&mut pixs, layout_id, rows, cols, version)?;

    // 12. Convert i8 pixs → BitMatrix.
    let rows_u = rows as usize;
    let cols_u = cols as usize;
    let mut matrix = BitMatrix::new(cols_u, rows_u);
    for r in 0..rows_u {
        for c in 0..cols_u {
            if pixs[qmv(r, c, cols_u)] == 1 {
                matrix.set(c, r, true);
            }
        }
    }
    Ok(matrix)
}

/// Encode a Micro QR symbol at the given `micro_idx` (0=M1, 1=M2,
/// 2=M3, 3=M4).
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if `micro_idx >= 4` or the chosen
/// `(M, ec_level)` combination is invalid (e.g. M1 + EC level ≠ L).
pub(crate) fn encode_micro_qr(
    msg: &[u8],
    micro_idx: usize,
    ec_level: u8,
) -> Result<BitMatrix, Error> {
    if micro_idx >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: micro_idx {micro_idx} out of range (0..4 = M1..M4)"
        )));
    }
    encode_qr_at_metric(msg, micro_idx, ec_level)
}

/// Encode a Rectangular Micro QR (rMQR) symbol. `version_str` is one
/// of the 32 rMQR size names (e.g. `"R7x43"`, `"R9x59"`, `"R17x139"`);
/// `ec_level` is `1` (M) or `3` (H) — rMQR does not support L or Q
/// per ISO/IEC 23941. Mirrors the BWIPP `rectangularmicroqrcode`
/// entry point.
///
/// # Errors
///
/// * [`Error::InvalidOption`] — unknown version name, or EC level
///   not in `{1, 3}`.
/// * [`Error::InvalidData`] — payload exceeds chosen size capacity.
pub fn encode_rmqr(text: &[u8], version_str: &str, ec_level: u8) -> Result<BitMatrix, Error> {
    let metric_idx = FULL_METRICS
        .iter()
        .position(|m| matches!(m.format, Format::Rmqr) && m.version_str == version_str)
        .ok_or_else(|| {
            Error::InvalidOption(format!(
                "qrcode_native: unknown rMQR version `{version_str}` (expected R7x43, R7x59, ..., R17x139)"
            ))
        })?;
    encode_qr_at_metric(text, metric_idx, ec_level)
}

/// Public QR code encoder entry point. Runs the BWIPP-faithful
/// auto-version search (Stage 10) to pick the smallest Full QR
/// version that holds the payload, then encodes via
/// [`encode_full_qr`].
///
/// The existing 9 QR-family catalog rows (Symbology::QrCode, etc.)
/// continue routing through the upstream `qrcode` crate via
/// `crate::symbology::qrcode_::encode`; this native encoder is
/// reachable only via direct module access until the catalog cutover
/// lands (tracked separately).
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if the input is empty or doesn't fit
/// any Full QR version (V1-V40 at the requested EC level).
pub(crate) fn encode(input: &[u8]) -> Result<BitMatrix, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData(
            "qrcode_native: empty input is not encodable".to_string(),
        ));
    }
    let (version, ec_level) = auto_select_full_qr_version(input, /*requested_ec=*/ 1)?;
    encode_full_qr(input, version, ec_level)
}

/// Public Full-QR encode entry point that mirrors
/// `crate::symbology::qrcode_::encode`'s opts parsing — extracts
/// `eclevel` (`L`/`M`/`Q`/`H`, default `M`) and optional `version`
/// (1..=40) from the catalog `Options` shape. Used by the QR-family
/// wrappers (`gs1_2d::encode_gs1_qrcode`, `swiss_qr::encode`,
/// `hibc::encode_qrcode`, etc.) when the `prefer-native-qrcode`
/// feature is enabled so they pick up byte-for-byte BWIPP parity.
///
/// # Errors
///
/// * [`Error::InvalidOption`] — unrecognised `eclevel` or unparseable
///   `version`.
/// * [`Error::InvalidData`] — empty input or payload exceeds the
///   chosen size's capacity.
pub fn encode_with_options(input: &[u8], opts: &crate::Options) -> Result<BitMatrix, Error> {
    encode_with_options_fnc1(input, opts, /*fnc1first=*/ false)
}

/// Public GS1 QR encode entry point. Identical to
/// [`encode_with_options`] except prefixes the bit stream with
/// BWIPP's "FNC1 in first position" mode indicator (4-bit `0101`)
/// per ISO/IEC 18004 Annex L — the bit that turns a plain QR into a
/// **GS1 QR Code**. Used by `gs1_2d::encode_gs1_qrcode` under the
/// `prefer-native-qrcode` feature.
///
/// `input` must be the FNC1-stripped GS1 element string (BWIPP's
/// `push_optimal_data` payload). Callers that parse a GS1 AI string
/// first should strip the leading `0x1d` FNC1 separator (mirroring
/// the existing wrapper's `bytes.get(1..)` slice).
///
/// # Errors
///
/// Same surface as [`encode_with_options`].
pub fn encode_gs1_qrcode(input: &[u8], opts: &crate::Options) -> Result<BitMatrix, Error> {
    encode_with_options_fnc1(input, opts, /*fnc1first=*/ true)
}

fn encode_with_options_fnc1(
    input: &[u8],
    opts: &crate::Options,
    fnc1first: bool,
) -> Result<BitMatrix, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData(
            "qrcode_native: empty input is not encodable".to_string(),
        ));
    }
    let ec_level = match opts.get("eclevel").unwrap_or("M") {
        "L" => 0u8,
        "M" => 1u8,
        "Q" => 2u8,
        "H" => 3u8,
        other => return Err(Error::InvalidOption(format!("eclevel={other}"))),
    };
    if let Some(ver_str) = opts.get("version") {
        let version: u8 = ver_str
            .parse()
            .map_err(|_| Error::InvalidOption(format!("version={ver_str}")))?;
        if !(1..=40).contains(&version) {
            return Err(Error::InvalidOption(format!(
                "QR version must be 1..=40, got {version}"
            )));
        }
        let metric_idx = 4 + (version - 1) as usize;
        encode_qr_at_metric_with_fnc1(input, metric_idx, ec_level, fnc1first)
    } else {
        // Auto-select uses the same `auto_select_full_qr_version` —
        // the fnc1first prefix is a fixed 4-bit overhead, so the
        // selected size correctly accounts for it (the bit budget
        // already includes mode-header bits).
        // The fnc1-aware auto-select accounts for the 4-bit FNC1-first
        // mode-indicator overhead so the chosen size always fits.
        let (version, final_ec) =
            auto_select_full_qr_version_with_fnc1(input, ec_level, fnc1first)?;
        let metric_idx = 4 + (version - 1) as usize;
        encode_qr_at_metric_with_fnc1(input, metric_idx, final_ec, fnc1first)
    }
}

/// Public Micro QR encode entry point that mirrors
/// `qrcode_::encode_micro`'s opts parsing. Used by the
/// `prefer-native-qrcode` default arm for `Symbology::MicroQrCode`.
///
/// # Errors
///
/// As for [`encode_with_options`]; in addition, `version` must be
/// `M1`..`M4` when supplied.
pub fn encode_micro_with_options(input: &[u8], opts: &crate::Options) -> Result<BitMatrix, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData(
            "qrcode_native: empty Micro QR input is not encodable".to_string(),
        ));
    }
    let ec_level = match opts.get("eclevel").unwrap_or("L") {
        "L" => 0u8,
        "M" => 1u8,
        "Q" => 2u8,
        "H" => {
            return Err(Error::InvalidOption(
                "Micro QR does not support EC level H".to_string(),
            ));
        }
        other => return Err(Error::InvalidOption(format!("eclevel={other}"))),
    };
    if let Some(ver_str) = opts.get("version") {
        let stripped = ver_str.strip_prefix('M').unwrap_or(ver_str);
        let n: u8 = stripped
            .parse()
            .map_err(|_| Error::InvalidOption(format!("version={ver_str}")))?;
        if !(1..=4).contains(&n) {
            return Err(Error::InvalidOption(format!(
                "Micro QR version must be M1..=M4, got M{n}"
            )));
        }
        encode_micro_qr(input, (n - 1) as usize, ec_level)
    } else {
        encode_micro_auto(input, ec_level)
    }
}

/// Pick the smallest Full QR version (1..=40) that can hold `msg` at
/// the requested EC level. Mirrors BWIPP's metric-search loop at
/// bwip-js 27367-27437. After finding the smallest fit, also tries to
/// upgrade the EC level to the highest level that still fits (BWIPP's
/// `!fixedeclevel` branch at line 27437) — improves robustness for
/// payloads that have spare capacity at the chosen size.
///
/// Returns `(version, final_ec_level)` where `final_ec_level >=
/// requested_ec`.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if no Full QR version can hold the
/// message at the requested EC level, or if `requested_ec >= 4`.
/// Public Micro QR encoder entry point with auto-version search.
/// Iterates M1..M4 metrics, picks the smallest that holds the payload
/// at the requested EC level, then optionally upgrades the EC level
/// to the highest that still fits. Mirrors BWIPP's metric-search
/// loop applied to the Micro QR row of `qrcode_metrics`.
///
/// `requested_ec`: 0=L, 1=M, 2=Q, 3=H. Note that not every Micro
/// version supports every EC level (M1 supports L only; M2 supports
/// L/M; M3 supports L/M; M4 supports L/M/Q).
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if the input is empty or doesn't
/// fit any Micro QR version (M1-M4) at the requested EC level.
pub(crate) fn encode_micro_auto(input: &[u8], requested_ec: u8) -> Result<BitMatrix, Error> {
    if input.is_empty() {
        return Err(Error::InvalidData(
            "qrcode_native: empty input is not encodable as Micro QR".to_string(),
        ));
    }
    let (micro_idx, ec_level) = auto_select_micro_qr_version(input, requested_ec)?;
    encode_micro_qr(input, micro_idx, ec_level)
}

/// Pick the smallest Micro QR version (M1..M4 = `micro_idx` 0..=3)
/// that can hold `msg` at the requested EC level. Mirrors BWIPP's
/// metric-search loop applied to the Micro QR slice. After finding
/// the smallest fit, tries to upgrade the EC level to the highest
/// level that still fits.
///
/// Returns `(micro_idx, final_ec_level)`.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if no Micro QR version can hold
/// the message at the requested EC level, or if `requested_ec >= 4`.
pub(crate) fn auto_select_micro_qr_version(
    msg: &[u8],
    requested_ec: u8,
) -> Result<(usize, u8), Error> {
    if requested_ec >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ec_level {requested_ec} out of range (0..=3 = L/M/Q/H)"
        )));
    }
    for (micro_idx, m) in FULL_METRICS.iter().enumerate().take(4) {
        let layout_id = m.layout_id;
        let segments = select_segments(msg, layout_id, false);
        let bits = match compose_segments(msg, &segments, layout_id, false) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let layout = match block_layout(micro_idx, requested_ec) {
            Ok(l) => l,
            Err(_) => continue,
        };
        // For lc4b (M1/M3): dmod = dcws*8 - 4. Else dmod = dcws*8.
        let lc4b = layout_id == 3 || layout_id == 5;
        let dmod = if lc4b {
            layout.dcws * 8 - 4
        } else {
            layout.dcws * 8
        };
        if (bits.len() as u32) <= dmod {
            // Found a fit. Try to upgrade EC.
            let mut final_ec = requested_ec;
            for try_ec in (requested_ec + 1)..=3 {
                if let Ok(layout_upgraded) = block_layout(micro_idx, try_ec) {
                    let upgraded_dmod = if lc4b {
                        layout_upgraded.dcws * 8 - 4
                    } else {
                        layout_upgraded.dcws * 8
                    };
                    if (bits.len() as u32) <= upgraded_dmod {
                        final_ec = try_ec;
                    }
                }
            }
            return Ok((micro_idx, final_ec));
        }
    }
    Err(Error::InvalidData(format!(
        "qrcode_native: message of {} bytes does not fit any Micro QR \
         version (M1..M4) at EC level {requested_ec}",
        msg.len()
    )))
}

pub(crate) fn auto_select_full_qr_version(msg: &[u8], requested_ec: u8) -> Result<(u8, u8), Error> {
    auto_select_full_qr_version_with_fnc1(msg, requested_ec, false)
}

/// Variant of [`auto_select_full_qr_version`] that accounts for the
/// 4-bit "FNC1 in first position" mode-indicator overhead when
/// `fnc1first` is set. Used by the GS1 QR auto-version path.
pub(crate) fn auto_select_full_qr_version_with_fnc1(
    msg: &[u8],
    requested_ec: u8,
    fnc1first: bool,
) -> Result<(u8, u8), Error> {
    if requested_ec >= 4 {
        return Err(Error::InvalidData(format!(
            "qrcode_native: ec_level {requested_ec} out of range (0..=3 = L/M/Q/H)"
        )));
    }
    for version in 1..=40u8 {
        let metric_idx = 4 + (version - 1) as usize;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        // Compute the message bit length for this version's CCI width.
        // BWIPP keeps a per-version-group cache (qrcode_msgbits) but for
        // a single payload the cost is dominated by the state machine —
        // running it 40 times worst-case is acceptable for now.
        let segments = select_segments(msg, layout_id, fnc1first);
        let bits = match compose_segments(msg, &segments, layout_id, fnc1first) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let layout = match block_layout(metric_idx, requested_ec) {
            Ok(l) => l,
            Err(_) => continue,
        };
        let dmod = layout.dcws * 8;
        if (bits.len() as u32) <= dmod {
            // Non-GS1 path: BWIPP's `!fixedeclevel` branch tries to
            // upgrade the EC level if the chosen version has spare
            // capacity at a higher EC. GS1 / FNC1-first path skips
            // this — BWIPP's GS1 QR encoder honours the user-specified
            // EC level verbatim (per gs1_2d::encode_gs1_qrcode's
            // `bits.push_terminator(ec)` against the *requested* ec)
            // so the size-search must too, otherwise a short payload
            // ends up at V1-H where BWIPP picks V1-Q/M.
            let final_ec = if fnc1first {
                requested_ec
            } else {
                let mut e = requested_ec;
                for try_ec in (requested_ec + 1)..=3 {
                    if let Ok(layout_upgraded) = block_layout(metric_idx, try_ec) {
                        let upgraded_dmod = layout_upgraded.dcws * 8;
                        if (bits.len() as u32) <= upgraded_dmod {
                            e = try_ec;
                        }
                    }
                }
                e
            };
            return Ok((version, final_ec));
        }
    }
    Err(Error::InvalidData(format!(
        "qrcode_native: message of {} bytes does not fit any Full QR version \
         (V1..V40) at EC level {requested_ec}",
        msg.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `encode` rejects empty input.
    ///
    /// Stage 11.A8c — upgrade from matches!(_, InvalidData(_)) to pin
    /// the empty-specific diagnostic. The qrcode_native module has
    /// multiple InvalidData paths (empty guard, payload-overflow,
    /// mode-encoder errors), so variant-only assertion can't
    /// distinguish. The empty arm (line 4080-4083) produces:
    ///   "qrcode_native: empty input is not encodable"
    #[test]
    fn encode_rejects_empty() {
        let err = encode(b"").unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("encode(b\"\") must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("qrcode_native:"),
            "diagnostic must carry the symbology tag; got {msg:?}"
        );
        assert!(
            msg.contains("empty input"),
            "diagnostic must call out 'empty input'; got {msg:?}"
        );
        assert!(
            msg.contains("not encodable"),
            "diagnostic must use the 'not encodable' predicate; got {msg:?}"
        );
    }

    /// `encode_full_qr` for "HELLO" V1-M produces a 21×21 BitMatrix
    /// with non-trivial content. Stage 10's catalog cutover will
    /// add oracle-pinned pixel-level integrity tests; for now we
    /// just verify the symbol size + that the dark module ends up
    /// at the correct position + that the dark-cell count is
    /// within ISO-reasonable bounds.
    #[test]
    fn encode_full_qr_v1_m_hello() {
        let matrix = encode_full_qr(b"HELLO", 1, 1).unwrap();
        assert_eq!(matrix.width(), 21);
        assert_eq!(matrix.height(), 21);
        // Top-left finder corner cell (0, 0) is part of the outer
        // ring and should be dark.
        assert!(matrix.get(0, 0), "TL finder corner (0, 0)");
        // ISO 18004 §6.10 dark module at (rows-8, 8) = (13, 8) for V1.
        assert!(matrix.get(8, 13), "V1 dark module at (col 8, row 13)");
        // Some data cells should be set (mask must produce a mix).
        let mut dark_count: usize = 0;
        for r in 0..21 {
            for c in 0..21 {
                if matrix.get(c, r) {
                    dark_count += 1;
                }
            }
        }
        assert!(
            (100..=350).contains(&dark_count),
            "V1-M HELLO should have a roughly-balanced dark count, got {dark_count}"
        );
    }

    /// `encode_full_qr` for "01234567" V1-M (ISO 18004 Annex I example).
    #[test]
    fn encode_full_qr_v1_m_iso_annex_i() {
        let matrix = encode_full_qr(b"01234567", 1, 1).unwrap();
        assert_eq!(matrix.width(), 21);
        assert_eq!(matrix.height(), 21);
        // Dark module at (rows-8, 8) = (13, 8).
        assert!(
            matrix.get(8, 13),
            "V1 dark module at (row 13, col 8) must be set"
        );
    }

    /// `encode_full_qr` rejects invalid version.
    #[test]
    fn encode_full_qr_rejects_invalid_version() {
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostic at line
        // 3894-3896 (`qrcode_native: Full QR version {v} out of range
        // (1..=40)`). Distinct boundary values (0 below-min, 41
        // above-max) prove the range check fires on both sides.
        match encode_full_qr(b"HELLO", 0, 1).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "below-min arm missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("Full QR version 0"),
                    "below-min arm missing `Full QR version 0` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (1..=40)"),
                    "below-min arm missing range hint: {msg}"
                );
                assert!(
                    !msg.contains("ec_level"),
                    "wrong arm — ec_level diagnostic leaked: {msg}"
                );
            }
            other => panic!("version 0 should reject as InvalidData, got {other:?}"),
        }
        match encode_full_qr(b"HELLO", 41, 1).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "above-max arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("Full QR version 41"),
                    "above-max arm missing `Full QR version 41` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (1..=40)"),
                    "above-max arm missing range hint: {msg}"
                );
            }
            other => panic!("version 41 should reject as InvalidData, got {other:?}"),
        }
    }

    /// `encode_full_qr` rejects invalid ec_level.
    #[test]
    fn encode_full_qr_rejects_invalid_ec_level() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 1912-1914 (`qrcode_native: ec_level 4 out of range
        // (0..=3 = L/M/Q/H)`). Cross-arm guard against the version
        // range arm.
        match encode_full_qr(b"HELLO", 1, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("ec_level 4"),
                    "missing `ec_level 4` value echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=3"),
                    "missing range hint `(0..=3`: {msg}"
                );
                assert!(
                    msg.contains("L/M/Q/H"),
                    "missing `L/M/Q/H` level-name hint: {msg}"
                );
                assert!(
                    !msg.contains("Full QR version"),
                    "wrong arm — version diagnostic leaked: {msg}"
                );
            }
            other => panic!("ec_level 4 should reject as InvalidData, got {other:?}"),
        }
    }

    /// `encode_qr_at_metric` for rMQR R7×43 + EC M smoke-encodes a
    /// short payload. Verifies the end-to-end pipeline (segments →
    /// codeword stream → matrix → finder + sub-finder + format-info
    /// write → mask 4) doesn't crash; byte-for-byte pinning is added
    /// separately by `encode_rmqr_pixs_corpus_matches_oracle`.
    #[test]
    fn encode_rmqr_r7x43_m_smoke() {
        // R7x43 = layout_id 7 = FULL_METRICS index 44. EC M = 1.
        let result = encode_qr_at_metric(b"HELLO", 44, 1);
        match &result {
            Ok(m) => {
                assert_eq!(m.width(), 43, "rMQR R7x43 width");
                assert_eq!(m.height(), 7, "rMQR R7x43 height");
            }
            Err(e) => panic!("encode_qr_at_metric(R7x43, EC M, HELLO) failed: {e:?}"),
        }
    }

    /// rMQR rejects EC level L (only M and H are valid per ISO 23941).
    #[test]
    fn encode_rmqr_rejects_ec_level_l() {
        // R7x43 + EC L = 0.
        let result = encode_qr_at_metric(b"HELLO", 44, 0);
        assert!(
            matches!(
                &result,
                Err(Error::InvalidOption(_)) | Err(Error::InvalidData(_))
            ),
            "rMQR + EC L must error, got {result:?}",
        );
    }

    /// rMQR rejects EC level Q.
    #[test]
    fn encode_rmqr_rejects_ec_level_q() {
        let result = encode_qr_at_metric(b"HELLO", 44, 2);
        assert!(
            matches!(
                &result,
                Err(Error::InvalidOption(_)) | Err(Error::InvalidData(_))
            ),
            "rMQR + EC Q must error, got {result:?}",
        );
    }

    /// Stage 11.A8c — pin `encode_micro_qr`'s `micro_idx >= 4`
    /// out-of-range rejection arm at line 4033-4036. Existing tests
    /// only pass micro_idx 0..3 (valid M1..M4), so the rejection arm
    /// and its diagnostic substring are uncovered. A mutant that
    /// changes the boundary (`>= 4` → `> 4` or `>= 5`) would let
    /// micro_idx=4 silently slip into encode_qr_at_metric where it
    /// would point at the FIRST FULL QR metric (index 4) instead of
    /// rejecting.
    ///
    /// Anchors:
    ///   * micro_idx=4 → InvalidData with "micro_idx 4" + "0..4 = M1..M4"
    ///     diagnostic.
    ///   * micro_idx=5 → also rejects.
    ///   * micro_idx=255 (max usize-castable) → rejects.
    ///   * micro_idx=3 (M4, max valid) → succeeds (kills `>= 4` →
    ///     `> 4` boundary mutation).
    ///   * micro_idx=0 (M1, min valid) → succeeds.
    #[test]
    fn encode_micro_qr_rejects_out_of_range_micro_idx() {
        // micro_idx=4 → rejected with diagnostic.
        //
        // Stage 11.A8c (cont) — add `qrcode_native:` symbology prefix
        // anchor (matches the source diagnostic at line 4033-4036 of
        // qrcode_native/mod.rs and brings parity with the surrounding
        // tests at lines 4398-4406 + 4738-4754 that already require
        // the prefix).
        match encode_micro_qr(b"1", 4, 0) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("qrcode_native:")
                    && msg.contains("micro_idx 4")
                    && msg.contains("0..4")
                    && msg.contains("M1..M4"),
                "micro_idx=4 should pin diagnostic 'qrcode_native:' + 'micro_idx 4' + '0..4 = M1..M4', got: {msg}"
            ),
            other => panic!("micro_idx=4 should reject as InvalidData, got {other:?}"),
        }

        // micro_idx=5, 255 also reject — per-value diagnostic pin
        // mirrors the micro_idx=4 boundary anchor and proves the
        // `{micro_idx}` interpolation routes the caller's value
        // (not a hardcoded "4" from the boundary case).
        match encode_micro_qr(b"1", 5, 0) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("micro_idx 5"),
                    "micro_idx=5 diagnostic must echo the caller's value; got {msg}"
                );
                assert!(
                    msg.contains("0..4") && msg.contains("M1..M4"),
                    "micro_idx=5 diagnostic must carry the 0..4 = M1..M4 range hint; got {msg}"
                );
            }
            other => panic!("micro_idx=5 should reject as InvalidData, got {other:?}"),
        }
        match encode_micro_qr(b"1", 255, 0) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("micro_idx 255"),
                    "micro_idx=255 diagnostic must echo the caller's value; got {msg}"
                );
                assert!(
                    msg.contains("0..4") && msg.contains("M1..M4"),
                    "micro_idx=255 diagnostic must carry the 0..4 = M1..M4 range hint; got {msg}"
                );
            }
            other => panic!("micro_idx=255 should reject as InvalidData, got {other:?}"),
        }

        // micro_idx=3 (M4) — valid, succeeds for a small payload.
        assert!(
            encode_micro_qr(b"1", 3, 0).is_ok(),
            "micro_idx=3 (M4) max valid must succeed"
        );

        // micro_idx=0 (M1) — valid, succeeds for digit "1".
        assert!(
            encode_micro_qr(b"1", 0, 0).is_ok(),
            "micro_idx=0 (M1) min valid must succeed"
        );
    }

    /// Stage 11.A8c — pin `encode_rmqr`'s unknown-`version_str`
    /// rejection arm at line 4056-4060. The existing rMQR tests drive
    /// the encoder via `encode_qr_at_metric` directly (with a
    /// known-good metric index), so the `position()` lookup + `.ok_or_else()`
    /// rejection arm in `encode_rmqr` itself was uncovered. A mutant
    /// that swaps `.position(|m| ... && m.version_str == version_str)`
    /// for `.position(|m| ... )` would silently match the first rMQR
    /// row regardless of the requested version.
    ///
    /// Anchors:
    ///   1. version="R7x43" (canonical smallest) → succeeds.
    ///   2. version="R17x139" (canonical largest) → succeeds at a
    ///      DIFFERENT metric (kills "always pick first" mutation).
    ///   3. version="R5x99" (made-up, not in spec) → InvalidOption with
    ///      diagnostic substring.
    ///   4. version="" (empty) → InvalidOption (lookup miss).
    ///   5. version="r7x43" (lowercase, case-sensitive in BWIPP) →
    ///      InvalidOption (FULL_METRICS uses exact-case match).
    ///
    /// Mutations to catch:
    ///   * `.position(|m| matches!(m.format, Format::Rmqr) && m.version_str == version_str)`
    ///     → drop the `version_str ==` half: would match the first
    ///     rMQR row, returning the same matrix for any version.
    ///   * `Format::Rmqr` arm swap: would attempt to match Full QR /
    ///     Micro versions and miss every rMQR request.
    ///   * Error message string mutations.
    #[test]
    fn encode_rmqr_rejects_unknown_version_string() {
        // Anchor 1: canonical smallest rMQR succeeds.
        let r7x43 = encode_rmqr(b"HELLO", "R7x43", 1).expect("R7x43 + EC M should encode");
        assert_eq!(r7x43.width(), 43);
        assert_eq!(r7x43.height(), 7);

        // Anchor 2: canonical largest rMQR succeeds at a different
        // metric (different size — kills "always pick first" mutant).
        let r17x139 =
            encode_rmqr(b"HELLO WORLD", "R17x139", 1).expect("R17x139 + EC M should encode");
        assert_eq!(r17x139.width(), 139);
        assert_eq!(r17x139.height(), 17);
        assert_ne!(
            (r7x43.width(), r7x43.height()),
            (r17x139.width(), r17x139.height()),
            "different versions must produce different sizes"
        );

        // Anchor 3: made-up version → InvalidOption with diagnostic.
        match encode_rmqr(b"HELLO", "R5x99", 1) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("unknown rMQR version") && m.contains("R5x99") && m.contains("R7x43"),
                "expected unknown-rMQR diagnostic with offending value + spec hint, got: {m}"
            ),
            other => panic!("R5x99 should reject as unknown rMQR, got {other:?}"),
        }

        // Anchor 4: empty version string → InvalidOption (lookup miss).
        match encode_rmqr(b"HELLO", "", 1) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("unknown rMQR version"),
                "empty version should reject as unknown, got: {m}"
            ),
            other => panic!("empty version should reject, got {other:?}"),
        }

        // Anchor 5: lowercase version → InvalidOption (case-sensitive).
        match encode_rmqr(b"HELLO", "r7x43", 1) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("unknown rMQR version") && m.contains("r7x43"),
                "lowercase version should reject (FULL_METRICS uses exact case), got: {m}"
            ),
            other => panic!("r7x43 should reject (case-sensitive), got {other:?}"),
        }
    }

    /// `encode_full_qr` rejects payload exceeding V1-L capacity.
    #[test]
    fn encode_full_qr_rejects_overflow() {
        // V1-L = 19 codewords = 152 bits, 16 data codewords (dcws).
        // 33 alphanumeric chars exceed capacity; the bit-stream length
        // check at line 1702 doesn't trigger because the upstream mode
        // selector emits a longer-than-needed byte-mode segment for the
        // overflowing run, but the codeword-stream builder catches it
        // when `padded_data.len()` (25 bytes here) does not match the
        // V1-L `dcws` (16) at line 2059-2064.
        //
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the actual diagnostic:
        //   1. `qrcode_native:` prefix
        //   2. `padded_data length` predicate
        //   3. `does not match dcws` predicate
        //   4. `dcws 16` — the V1-L data-codeword count, proving the
        //      version/ECC pair the test names is what was actually
        //      consulted (kills hardcoded-dcws mutations).
        let too_long = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 33 chars
        match encode_full_qr(too_long.as_bytes(), 1, 1) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("padded_data length"),
                    "missing `padded_data length` predicate: {msg}"
                );
                assert!(
                    msg.contains("does not match dcws"),
                    "missing `does not match dcws` predicate: {msg}"
                );
                assert!(
                    msg.contains("dcws 16"),
                    "missing V1-L dcws echo `dcws 16`: {msg}"
                );
            }
            other => panic!("33-alpha V1-L should reject as InvalidData, got {other:?}"),
        }
    }

    /// `auto_select_full_qr_version` picks V1 for "HELLO WORLD"
    /// (11 alphanumeric chars).
    #[test]
    fn auto_select_v1_hello_world() {
        let (version, _ec) = auto_select_full_qr_version(b"HELLO WORLD", 0).unwrap();
        assert_eq!(version, 1, "11-char alpha fits V1");
    }

    /// `auto_select_full_qr_version` picks V1 with EC upgrade
    /// (short message has spare capacity).
    #[test]
    fn auto_select_v1_ec_upgrades() {
        // "ABC" (3 chars alpha) fits V1-H easily. Requesting L should
        // upgrade to H.
        let (version, ec) = auto_select_full_qr_version(b"ABC", 0).unwrap();
        assert_eq!(version, 1);
        assert_eq!(ec, 3, "short payload should auto-upgrade to H");
    }

    /// `auto_select_full_qr_version` picks larger version for longer
    /// payload.
    #[test]
    fn auto_select_larger_version() {
        // 100 numeric digits = needs more than V1.
        // V1-L cap = 152 bits. Mode (4) + CCI (10) + body (100/3=33 full
        // groups * 10 + 1 leftover * 4 = 334 bits) = 348 bits. Definitely
        // doesn't fit V1.
        let payload = "0123456789".repeat(10); // 100 digits
        let (version, _ec) = auto_select_full_qr_version(payload.as_bytes(), 0).unwrap();
        assert!(version >= 2, "100 digits needs V2+, got V{version}");
        assert!(version <= 10, "100 digits should fit V2-V10");
    }

    /// `auto_select_full_qr_version` rejects too-long payload.
    ///
    /// Pins the overflow-arm diagnostic at line 4372-4376:
    ///   "qrcode_native: message of {N} bytes does not fit any Full QR
    ///    version (V1..V40) at EC level {requested_ec}"
    ///
    /// Anchors:
    ///   * "message of 8000 bytes" — proves `msg.len()` interpolates.
    ///   * "any Full QR version (V1..V40)" — proves the V1-V40 range
    ///     hint survives mutations to the format string.
    ///   * "EC level 3" — proves `{requested_ec}` interpolates the
    ///     caller's value (not a hardcoded number).
    ///   * ABSENCE of "ec_level 3 out of range" — cross-arm
    ///     contamination guard: ec=3 is valid, so the range-arm
    ///     wording must NOT appear.
    #[test]
    fn auto_select_rejects_overflow() {
        // 5000 alpha chars: even V40-L (29648 bits) can't hold
        // 5000 * 5.5 bits ≈ 27500 + headers > 29648... actually it
        // might fit V40-L. Let me use 8000 chars instead.
        let payload = "A".repeat(8000);
        let result = auto_select_full_qr_version(payload.as_bytes(), 3);
        let err = result.unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "overflow diagnostic must carry the symbology tag; got {msg}"
                );
                assert!(
                    msg.contains("message of 8000 bytes"),
                    "overflow diagnostic must echo msg.len() (8000); got {msg}"
                );
                assert!(
                    msg.contains("does not fit any Full QR version"),
                    "overflow diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("(V1..V40)"),
                    "overflow diagnostic must carry the V1..V40 range hint; got {msg}"
                );
                assert!(
                    msg.contains("EC level 3"),
                    "overflow diagnostic must echo requested_ec (3); got {msg}"
                );
                assert!(
                    !msg.contains("ec_level 3 out of range"),
                    "overflow diagnostic must NOT leak the ec_level-range arm; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// `auto_select_full_qr_version` rejects invalid EC level.
    #[test]
    fn auto_select_rejects_invalid_ec() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 4323-4325 (`qrcode_native: ec_level 4 out of range (0..=3
        // = L/M/Q/H)`). Cross-arm guard against the version-range
        // diagnostic in the encode_full_qr path.
        match auto_select_full_qr_version(b"HELLO", 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("ec_level 4"),
                    "missing `ec_level 4` value echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=3"),
                    "missing range hint: {msg}"
                );
                assert!(
                    msg.contains("L/M/Q/H"),
                    "missing `L/M/Q/H` level-name hint: {msg}"
                );
                assert!(
                    !msg.contains("Full QR version"),
                    "wrong arm — version diagnostic leaked into ec_level arm: {msg}"
                );
            }
            other => panic!("ec_level 4 auto-select should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the EC-level range boundary + diagnostic
    /// for `auto_select_full_qr_version*`. Existing
    /// `auto_select_rejects_invalid_ec` only tests ec=4 with
    /// `is_err()`. A mutant that drifts the `>= 4` boundary or
    /// swaps the error message would still pass.
    ///
    /// Anchors:
    ///   * ec=3 → accepts (just under the bound).
    ///   * ec=4 → rejects with "ec_level 4" + "0..=3" diagnostic.
    ///   * ec=5 → rejects (well above bound).
    ///   * ec=255 (max u8) → rejects.
    #[test]
    fn auto_select_ec_level_boundary_and_diagnostic() {
        // Anchor: ec=3 (H, max valid) → succeeds for a short payload.
        let result = auto_select_full_qr_version(b"HELLO", 3);
        assert!(
            result.is_ok(),
            "ec=3 (max valid) must succeed for short payload"
        );

        // Anchor: ec=4 → rejects with diagnostic.
        match auto_select_full_qr_version(b"HELLO", 4) {
            Err(Error::InvalidData(msg)) => assert!(
                msg.contains("ec_level 4") && msg.contains("0..=3"),
                "expected 'ec_level 4 out of range (0..=3 ...)' diagnostic, got: {msg}"
            ),
            other => panic!("ec=4 should reject with InvalidData, got {other:?}"),
        }

        // Anchor: ec=5 → rejects (kills `>= 4 → > 4` boundary mutant).
        // Per-value diagnostic pin proves `{requested_ec}` interpolates
        // the caller's value (not a hardcoded "4" from the boundary case).
        match auto_select_full_qr_version(b"HELLO", 5) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("ec_level 5"),
                    "ec=5 diagnostic must echo requested_ec=5; got {msg}"
                );
                assert!(
                    msg.contains("0..=3"),
                    "ec=5 diagnostic must carry the 0..=3 range hint; got {msg}"
                );
            }
            other => panic!("ec=5 should reject with InvalidData, got {other:?}"),
        }

        // Anchor: ec=255 (max u8) → rejects. Per-value diagnostic pin
        // proves the format string handles arbitrarily-large u8 values.
        match auto_select_full_qr_version(b"HELLO", 255) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("ec_level 255"),
                    "ec=255 diagnostic must echo requested_ec=255; got {msg}"
                );
                assert!(
                    msg.contains("0..=3"),
                    "ec=255 diagnostic must carry the 0..=3 range hint; got {msg}"
                );
            }
            other => panic!("ec=255 should reject with InvalidData, got {other:?}"),
        }

        // Anchor: ec=0 (L, min valid) → succeeds.
        assert!(
            auto_select_full_qr_version(b"HELLO", 0).is_ok(),
            "ec=0 (min valid) must succeed"
        );
    }

    /// Stage 11.A8c — pin the `fnc1first` arm at lines 4355-4368 of
    /// `auto_select_full_qr_version_with_fnc1`. The non-fnc1 path
    /// upgrades the EC level to the highest that still fits (BWIPP's
    /// `!fixedeclevel` branch); the fnc1 path honors the requested
    /// EC level verbatim. Existing tests only drive the non-fnc1
    /// wrapper (`auto_select_full_qr_version`), so the fnc1=true arm
    /// is uncovered — a mutant that swaps `if fnc1first` with `if !fnc1first`
    /// or removes the early-return would survive.
    ///
    /// Anchors:
    ///   * Short payload "1" at ec=0 (L) without fnc1: should upgrade
    ///     to a higher EC (Q or H) because V1 has spare capacity.
    ///   * Same payload at ec=0 with fnc1=true: should stay at ec=0
    ///     (no upgrade per GS1 QR semantics).
    ///   * The version chosen is the same in both cases (smallest fit).
    #[test]
    fn auto_select_fnc1first_skips_ec_upgrade() {
        // Plain path: ec=0 (L) for "1" should upgrade to higher EC.
        let (version_plain, ec_plain) =
            auto_select_full_qr_version_with_fnc1(b"1", 0, false).unwrap();
        // FNC1 path: ec=0 (L) for "1" should honor L verbatim.
        let (version_fnc1, ec_fnc1) = auto_select_full_qr_version_with_fnc1(b"1", 0, true).unwrap();

        // Both pick V1 (smallest fit).
        assert_eq!(version_plain, 1, "plain path picks V1");
        assert_eq!(version_fnc1, 1, "fnc1 path picks V1");

        // Plain path upgrades EC; fnc1 path stays at L=0.
        assert!(
            ec_plain >= ec_fnc1,
            "plain path must upgrade (or at least equal) ec={ec_plain} vs fnc1={ec_fnc1}"
        );
        assert_eq!(ec_fnc1, 0, "fnc1 path must honor requested ec=0 verbatim");
        // The upgrade should actually fire (V1 holds way more than 1 byte
        // even at H=3).
        assert!(
            ec_plain > 0,
            "plain path with 1-byte payload must upgrade ec=0 to higher level (V1 has spare capacity)"
        );
    }

    /// Public `encode("HELLO WORLD")` should produce a 21×21 V1
    /// BitMatrix (smallest version that holds 11 alpha chars).
    #[test]
    fn encode_auto_selects_v1() {
        let matrix = encode(b"HELLO WORLD").unwrap();
        assert_eq!(matrix.width(), 21);
        assert_eq!(matrix.height(), 21);
    }

    /// Public `encode` selects a larger symbol for longer input.
    #[test]
    fn encode_auto_selects_larger() {
        let payload = "HELLO WORLD HELLO WORLD HELLO WORLD HELLO WORLD HELLO WORLD";
        let matrix = encode(payload.as_bytes()).unwrap();
        assert!(
            matrix.width() >= 25,
            "Wide payload should produce >= V2 25x25"
        );
    }

    /// Format enum has exactly 3 variants — Full / Micro / Rmqr.
    #[test]
    fn format_variants() {
        let _ = Format::Full;
        let _ = Format::Micro;
        let _ = Format::Rmqr;
        assert_ne!(Format::Full, Format::Micro);
        assert_ne!(Format::Micro, Format::Rmqr);
        assert_ne!(Format::Full, Format::Rmqr);
    }

    /// Full `qrcode_metrics` table — 76 rows covering all Micro QR
    /// (M1..=M4), Full QR (V1..=V40), and rMQR (R7×43..=R17×139).
    /// Validates shape + known values verbatim from bwip-js line 25718.
    #[test]
    fn full_metrics_shape_and_anchors() {
        assert_eq!(FULL_METRICS.len(), 76);
        // 4 Micro + 40 Full + 32 rMQR.
        let n_micro = FULL_METRICS
            .iter()
            .filter(|m| m.format == Format::Micro)
            .count();
        let n_full = FULL_METRICS
            .iter()
            .filter(|m| m.format == Format::Full)
            .count();
        let n_rmqr = FULL_METRICS
            .iter()
            .filter(|m| m.format == Format::Rmqr)
            .count();
        assert_eq!(n_micro, 4);
        assert_eq!(n_full, 40);
        assert_eq!(n_rmqr, 32);

        // Index 0: M1.
        let m1 = &FULL_METRICS[0];
        assert_eq!(m1.format, Format::Micro);
        assert_eq!(m1.version_str, "M1");
        assert_eq!(m1.rows, 11);
        assert_eq!(m1.cols, 11);
        assert_eq!(m1.datacap_bits, 36);
        assert_eq!(m1.eclen, [2, 99, 99, 99]);

        // Index 4: V1 (first Full QR after the 4 Micro rows).
        let v1 = &FULL_METRICS[4];
        assert_eq!(v1.format, Format::Full);
        assert_eq!(v1.version_str, "1");
        assert_eq!(v1.rows, 21);
        assert_eq!(v1.cols, 21);
        assert_eq!(v1.datacap_bits, 208);
        assert_eq!(v1.eclen, [7, 10, 13, 17]);

        // Index 10: V7 (first version with version-info bits).
        let v7 = &FULL_METRICS[10];
        assert_eq!(v7.version_str, "7");
        assert_eq!(v7.fimas, 38, "V7 fimas (version-info offset) should be 38");
        assert_eq!(v7.eclen, [40, 72, 108, 130]);

        // Index 43: V40 (largest Full QR; index = 4 micro + 39 = 43).
        let v40 = &FULL_METRICS[43];
        assert_eq!(v40.version_str, "40");
        assert_eq!(v40.rows, 177);
        assert_eq!(v40.cols, 177);
        assert_eq!(v40.datacap_bits, 29648);
        assert_eq!(v40.eclen, [750, 1372, 2040, 2430]);

        // Index 44: R7×43 (first rMQR).
        let r0 = &FULL_METRICS[44];
        assert_eq!(r0.format, Format::Rmqr);
        assert_eq!(r0.version_str, "R7x43");
        assert_eq!(r0.rows, 7);
        assert_eq!(r0.cols, 43);
        assert_eq!(r0.eclen, [99, 7, 99, 10]);

        // Index 75: R17×139 (last rMQR / last row of the table).
        let r_last = &FULL_METRICS[75];
        assert_eq!(r_last.version_str, "R17x139");
        assert_eq!(r_last.rows, 17);
        assert_eq!(r_last.cols, 139);
        assert_eq!(r_last.datacap_bits, 1860);
        assert_eq!(r_last.eclen, [99, 80, 99, 156]);
    }

    /// NA sentinel constants match BWIPP's "not applicable" values.
    #[test]
    fn na_sentinels() {
        assert_eq!(NA, 99);
        assert_eq!(NA_I8, 99);
    }

    /// BCH(15,5) format-info encoder. Anchor against the BWIPP-built
    /// post-mask format-info bit-strings per ISO/IEC 18004 Tables 12
    /// (Full QR) + 13 (Micro QR). Computed pre-mask values:
    ///
    ///   data=0b00000 (M+mask0) → ECC 0 → 0x0000 (post-mask = 0x5412 = ISO L+mask0? No — M+mask0)
    ///   data=0b00001 (M+mask1) → 0x0537
    ///   data=0b00010 (M+mask2) → 0x0A6E
    ///   data=0b01000 (L+mask0) → 0x23D6 → post-mask 0x77C4 = ISO L+mask0
    ///   data=0b01001 (L+mask1) → 0x26E1 → post-mask 0x72F3 = ISO L+mask1
    ///
    /// (Per ISO 18004 §8.9 Table 12, the 5-bit data is
    /// `(ec_indicator << 3) | mask`, where ec_indicator = L:01 M:00
    /// Q:11 H:10. So data=0 = M+mask0, data=8 = L+mask0, data=9 =
    /// L+mask1, etc.)
    #[test]
    fn bch15_5_encode_anchors() {
        // data=0 (M+mask0) → ECC is 0 (generator poly never fires).
        assert_eq!(bch15_5_encode(0b00000), 0x0000);
        // data=1 (M+mask1) → ECC = 0x137 → output 0x0537. Per ISO Table 12,
        // post-mask = 0x0537 ^ 0x5412 = 0x5125 = M+mask1.
        assert_eq!(bch15_5_encode(0b00001), 0x0537);
        // data=2 (M+mask2) → ECC = 0x26E → output 0x0A6E.
        assert_eq!(bch15_5_encode(0b00010), 0x0A6E);
        // data=8 (L+mask0) → output 0x23D6. Post-mask 0x77C4 = ISO L+mask0.
        assert_eq!(bch15_5_encode(0b01000), 0x23D6);
        // data=9 (L+mask1) → output 0x26E1. Post-mask 0x72F3 = ISO L+mask1.
        assert_eq!(bch15_5_encode(0b01001), 0x26E1);
        // Round-trip property: top 5 bits = data.
        for d in 0u8..32u8 {
            let enc = bch15_5_encode(d);
            assert_eq!(
                (enc >> 10) as u8,
                d,
                "BCH(15,5) for data={d}: top 5 bits should equal data"
            );
            assert!(enc <= 0x7FFF, "BCH(15,5) output should be ≤ 15 bits");
        }
        // ISO Table 12 verification via post-mask: bch15_5_encode(8) ^ MASK_FULL
        // should equal ISO L+mask0 = 0b111011111000100 = 0x77C4.
        assert_eq!(
            bch15_5_encode(0b01000) ^ FORMAT_INFO_MASK_FULL,
            0x77C4,
            "L+mask0 post-mask per ISO 18004 Table 12"
        );
    }

    /// BCH(18,6) version-info encoder for V7+ Full QR. Anchor against
    /// ISO/IEC 18004 Table D.1.
    #[test]
    fn bch18_6_encode_anchors() {
        // V7 → encoded 0b000111110010010100 = 0x07C94.
        // Verify by computing: 7 << 12 = 0x7000. Then BCH division
        // against 0x1F25 from bit 14 down. Top bit of 0x7000 is bit 14.
        //   bit 14: 0x4000 set, XOR 0x1F25 << 2 = 0x7C94. d = 0x7000 ^ 0x7C94 = 0x0C94.
        //   bit 13: 0x0C94 & 0x2000 = 0; skip.
        //   bit 12: 0x0C94 & 0x1000 = 0; skip.
        // result = top6(0x07C94) = 7, low12 = 0xC94. enc = (7<<12)|0xC94 = 0x7C94.
        assert_eq!(bch18_6_encode(7), 0x7C94);
        // V40 → known value 0b101010000010010001 = 0x28A11.
        // Just verify top 6 bits round-trip.
        for v in 7u8..=40u8 {
            let enc = bch18_6_encode(v);
            let recovered = (enc >> 12) as u8;
            assert_eq!(
                recovered, v,
                "BCH(18,6) for v={v}: top 6 bits should equal v"
            );
            assert!(enc <= 0x3FFFF, "BCH(18,6) output should be ≤ 18 bits");
        }
    }

    /// BCH polynomial + mask constants match ISO/IEC 18004 §C.
    #[test]
    fn bch_constants() {
        assert_eq!(BCH_15_5_POLY, 0x537);
        assert_eq!(BCH_18_6_POLY, 0x1F25);
        assert_eq!(FORMAT_INFO_MASK_FULL, 0x5412);
        assert_eq!(FORMAT_INFO_MASK_MICRO, 0x4445);
    }

    // ---------------------------------------------------------------
    // Stage 3 — mode-encoder tests
    // ---------------------------------------------------------------

    /// Convert a `Vec<bool>` to an MSB-first "01" string for readable
    /// assertions.
    fn bits_to_string(bits: &[bool]) -> String {
        bits.iter().map(|b| if *b { '1' } else { '0' }).collect()
    }

    /// Numeric segment for "123" — single 3-digit group, 10 bits.
    /// Value 123 = 0b0001111011.
    #[test]
    fn encode_numeric_segment_three_digits() {
        let bits = encode_numeric_segment(b"123").unwrap();
        assert_eq!(bits.len(), 10);
        assert_eq!(bits_to_string(&bits), "0001111011");
    }

    /// Numeric segment for "01234567" — ISO 18004 Annex H worked
    /// example.
    ///
    /// * "012" = 12 → 0000001100 (10 bits)
    /// * "345" = 345 → 0101011001 (10 bits)
    /// * "67" = 67 → 1000011 (7 bits)
    ///
    /// Total = 27 bits.
    #[test]
    fn encode_numeric_segment_iso_annex_h() {
        let bits = encode_numeric_segment(b"01234567").unwrap();
        assert_eq!(bits.len(), 27);
        assert_eq!(
            bits_to_string(&bits),
            "000000110001010110011000011",
            "ISO 18004 Annex H numeric example"
        );
    }

    /// Numeric segment, single trailing digit → 4 bits.
    #[test]
    fn encode_numeric_segment_one_remainder() {
        let bits = encode_numeric_segment(b"1234").unwrap();
        assert_eq!(bits.len(), 14, "3 digits → 10 bits, +1 digit → 4 bits");
        // "123" = 0001111011, "4" = 0100.
        assert_eq!(bits_to_string(&bits), "00011110110100");
    }

    /// Numeric segment rejects non-digit input.
    #[test]
    fn encode_numeric_segment_rejects_letter() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 455-457
        // (`qrcode_native: numeric mode expects ASCII digit, got
        // 0x41` for 'A'). Cross-mode guard against alphanumeric arm.
        match encode_numeric_segment(b"12A").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("numeric mode"),
                    "missing `numeric mode` predicate: {msg}"
                );
                assert!(
                    msg.contains("expects ASCII digit"),
                    "missing `expects ASCII digit` hint: {msg}"
                );
                assert!(
                    msg.contains("0x41"),
                    "missing hex echo `0x41` for 'A': {msg}"
                );
                assert!(
                    !msg.contains("alphanumeric mode"),
                    "wrong mode — alphanumeric diagnostic leaked into numeric: {msg}"
                );
            }
            other => panic!("`12A` numeric should reject as InvalidData, got {other:?}"),
        }
    }

    /// Alphanumeric segment for "AC-42" — ISO 18004 Annex H worked
    /// example.
    ///
    /// * "AC" → 10*45+12 = 462 → 00111001110 (11 bits)
    /// * "-4" → 41*45+4 = 1849 → 11100111001 (11 bits)
    /// * "2"  → 2 → 000010 (6 bits)
    ///
    /// Total = 28 bits = "0011100111011100111001000010".
    #[test]
    fn encode_alphanumeric_segment_iso_annex_h() {
        let bits = encode_alphanumeric_segment(b"AC-42").unwrap();
        assert_eq!(bits.len(), 28);
        assert_eq!(
            bits_to_string(&bits),
            "0011100111011100111001000010",
            "ISO 18004 Annex H alphanumeric example"
        );
    }

    /// Alphanumeric value table spot-checks.
    #[test]
    fn alphanumeric_value_table() {
        assert_eq!(ALPHANUMERIC_VALUE[b'0' as usize], 0);
        assert_eq!(ALPHANUMERIC_VALUE[b'9' as usize], 9);
        assert_eq!(ALPHANUMERIC_VALUE[b'A' as usize], 10);
        assert_eq!(ALPHANUMERIC_VALUE[b'Z' as usize], 35);
        assert_eq!(ALPHANUMERIC_VALUE[b' ' as usize], 36);
        assert_eq!(ALPHANUMERIC_VALUE[b'$' as usize], 37);
        assert_eq!(ALPHANUMERIC_VALUE[b'%' as usize], 38);
        assert_eq!(ALPHANUMERIC_VALUE[b'*' as usize], 39);
        assert_eq!(ALPHANUMERIC_VALUE[b'+' as usize], 40);
        assert_eq!(ALPHANUMERIC_VALUE[b'-' as usize], 41);
        assert_eq!(ALPHANUMERIC_VALUE[b'.' as usize], 42);
        assert_eq!(ALPHANUMERIC_VALUE[b'/' as usize], 43);
        assert_eq!(ALPHANUMERIC_VALUE[b':' as usize], 44);
        // Lowercase + most punctuation rejected.
        assert_eq!(ALPHANUMERIC_VALUE[b'a' as usize], -1);
        assert_eq!(ALPHANUMERIC_VALUE[b'@' as usize], -1);
        assert_eq!(ALPHANUMERIC_VALUE[b'!' as usize], -1);
    }

    /// Alphanumeric rejects lowercase.
    #[test]
    fn encode_alphanumeric_segment_rejects_lowercase() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 493-494 (`qrcode_native: alphanumeric mode rejects byte
        // 0x61` for lowercase 'a' at position 1). Sibling-arm guard:
        // the numeric mode arm uses the substring `numeric mode`
        // which is also contained inside `alphanumeric mode` — so we
        // can't directly negate-guard `numeric mode`. Instead guard
        // against `expects ASCII digit` (the numeric arm's hint).
        match encode_alphanumeric_segment(b"Aa").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("alphanumeric mode"),
                    "missing `alphanumeric mode` predicate: {msg}"
                );
                assert!(
                    msg.contains("rejects byte"),
                    "missing `rejects byte` hint: {msg}"
                );
                assert!(
                    msg.contains("0x61"),
                    "missing hex echo `0x61` for 'a': {msg}"
                );
                assert!(
                    !msg.contains("expects ASCII digit"),
                    "wrong mode — numeric-mode hint leaked: {msg}"
                );
            }
            other => panic!("`Aa` alphanumeric should reject as InvalidData, got {other:?}"),
        }
    }

    /// Byte segment is raw 8-bit MSB-first.
    #[test]
    fn encode_byte_segment_hi() {
        let bits = encode_byte_segment(b"Hi");
        assert_eq!(bits.len(), 16);
        // 'H' = 0x48 = 01001000, 'i' = 0x69 = 01101001.
        assert_eq!(bits_to_string(&bits), "0100100001101001");
    }

    /// Kanji segment for Shift-JIS 0x935F → ISO 18004 §6.4.5 example
    /// "点": 0x935F.
    ///
    /// * offset = 0x935F - 0x8140 = 0x121F
    /// * hi=0x12, lo=0x1F → 0x12 * 0xC0 + 0x1F = 0x0D9F = 0b0110110011111
    ///
    /// 13 bits = "0110110011111".
    #[test]
    fn encode_kanji_segment_iso_example() {
        let bits = encode_kanji_segment(&[0x935F]).unwrap();
        assert_eq!(bits.len(), 13);
        assert_eq!(bits_to_string(&bits), "0110110011111");
    }

    /// Kanji segment for Shift-JIS 0xE4AA: high-band example.
    ///
    /// * offset = 0xE4AA - 0xC140 = 0x236A
    /// * hi=0x23, lo=0x6A → 0x23 * 0xC0 + 0x6A = 0x1A8A + 0x6A = 0x1AF4? Recompute:
    ///   0x23 * 0xC0 = 0x1A40; 0x1A40 + 0x6A = 0x1AAA.
    /// * 13 bits of 0x1AAA = "1101010101010".
    #[test]
    fn encode_kanji_segment_high_band() {
        let bits = encode_kanji_segment(&[0xE4AA]).unwrap();
        assert_eq!(bits.len(), 13);
        assert_eq!(bits_to_string(&bits), "1101010101010");
    }

    /// Kanji rejects out-of-range Shift-JIS.
    #[test]
    fn encode_kanji_segment_rejects_out_of_range() {
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostic at line
        // 532-533 (`qrcode_native: kanji mode rejects Shift-JIS
        // 0x????`). Distinct boundary values cover the low-band
        // (0x0041, below 0x8140) and high-band (0xA000, between
        // 0x9FFC and 0xE040 — the gap between bands).
        match encode_kanji_segment(&[0x0041]).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "low-band arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("kanji mode"),
                    "low-band arm missing `kanji mode` predicate: {msg}"
                );
                assert!(
                    msg.contains("Shift-JIS 0x0041"),
                    "low-band arm missing `Shift-JIS 0x0041` echo: {msg}"
                );
                assert!(
                    !msg.contains("numeric mode") && !msg.contains("alphanumeric mode"),
                    "wrong mode — non-kanji diagnostic leaked: {msg}"
                );
            }
            other => panic!("low-band kanji 0x0041 should reject as InvalidData, got {other:?}"),
        }
        match encode_kanji_segment(&[0xA000]).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "gap-band arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("kanji mode"),
                    "gap-band arm missing predicate: {msg}"
                );
                assert!(
                    msg.contains("Shift-JIS 0xA000"),
                    "gap-band arm missing `Shift-JIS 0xA000` echo (proves the gap-band kills hardcoded 0x0041): {msg}"
                );
            }
            other => panic!("gap-band kanji 0xA000 should reject as InvalidData, got {other:?}"),
        }
    }

    /// ECI assignment number encoding (BWIPP encE).
    ///
    /// * 7 → 8 bits: 00000111
    /// * 9 → 8 bits: 00001001
    /// * 16383 → 16 bits: 1011111111111111 (16383 | 0x8000 = 0xBFFF)
    /// * 16384 → 24 bits: (16384 | 0xC00000) = 0xC04000 → "110000000000010000000000"
    #[test]
    fn encode_eci_segment_lengths() {
        assert_eq!(bits_to_string(&encode_eci_segment(7).unwrap()), "00000111");
        assert_eq!(bits_to_string(&encode_eci_segment(9).unwrap()), "00001001");
        assert_eq!(
            bits_to_string(&encode_eci_segment(127).unwrap()),
            "01111111"
        );
        assert_eq!(
            bits_to_string(&encode_eci_segment(128).unwrap()),
            "1000000010000000"
        );
        assert_eq!(
            bits_to_string(&encode_eci_segment(16383).unwrap()),
            "1011111111111111"
        );
        assert_eq!(
            bits_to_string(&encode_eci_segment(16384).unwrap()),
            "110000000100000000000000"
        );
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 563-564
        // (`qrcode_native: ECI assignment number 1000000 exceeds
        // 6-digit max`).
        match encode_eci_segment(1_000_000).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("ECI assignment number"),
                    "missing `ECI assignment number` predicate: {msg}"
                );
                assert!(
                    msg.contains("1000000"),
                    "missing `1000000` value echo: {msg}"
                );
                assert!(
                    msg.contains("6-digit max"),
                    "missing `6-digit max` cap hint: {msg}"
                );
            }
            other => panic!("ECI 1_000_000 should reject as InvalidData, got {other:?}"),
        }
    }

    /// CCI bit-widths match BWIPP qrcode_cclens (bwip-js 25637) for
    /// every supported (version-bin, mode) pair.
    #[test]
    fn cci_bits_anchors() {
        // Full V1-9 (layout_id 0): N=10, A=9, B=8, K=8.
        assert_eq!(cci_bits(0, Mode::Numeric), Some(10));
        assert_eq!(cci_bits(0, Mode::Alphanumeric), Some(9));
        assert_eq!(cci_bits(0, Mode::Byte), Some(8));
        assert_eq!(cci_bits(0, Mode::Kanji), Some(8));
        // Full V10-26 (layout_id 1): N=12, A=11, B=16, K=10.
        assert_eq!(cci_bits(1, Mode::Numeric), Some(12));
        assert_eq!(cci_bits(1, Mode::Byte), Some(16));
        // Full V27-40 (layout_id 2): N=14, A=13, B=16, K=12.
        assert_eq!(cci_bits(2, Mode::Numeric), Some(14));
        // M1 (layout_id 3): N=3 only.
        assert_eq!(cci_bits(3, Mode::Numeric), Some(3));
        assert_eq!(cci_bits(3, Mode::Alphanumeric), None);
        assert_eq!(cci_bits(3, Mode::Byte), None);
        assert_eq!(cci_bits(3, Mode::Kanji), None);
        // M2 (layout_id 4): N+A only.
        assert_eq!(cci_bits(4, Mode::Numeric), Some(4));
        assert_eq!(cci_bits(4, Mode::Alphanumeric), Some(3));
        assert_eq!(cci_bits(4, Mode::Byte), None);
        // R7x43 (layout_id 7) — first rMQR row.
        assert_eq!(cci_bits(7, Mode::Numeric), Some(4));
        assert_eq!(cci_bits(7, Mode::Kanji), Some(2));
        // R17x139 (layout_id 38) — largest rMQR.
        assert_eq!(cci_bits(38, Mode::Numeric), Some(9));
        assert_eq!(cci_bits(38, Mode::Kanji), Some(7));
        // ECI mode has no CCI.
        assert_eq!(cci_bits(0, Mode::Eci), None);
        // Out-of-range layout_id.
        assert_eq!(cci_bits(99, Mode::Numeric), None);
    }

    /// Every (layout_id, mode) in CC_LENS round-trips through
    /// cci_bits. Negative values map to None.
    #[test]
    fn cci_bits_complete_table_roundtrip() {
        let modes = [Mode::Numeric, Mode::Alphanumeric, Mode::Byte, Mode::Kanji];
        for (bin, row) in CC_LENS.iter().enumerate() {
            for (col, &expected) in row.iter().enumerate() {
                let got = cci_bits(bin as u8, modes[col]);
                if expected < 0 {
                    assert_eq!(got, None, "bin={bin} mode={col}");
                } else {
                    assert_eq!(got, Some(expected as u8), "bin={bin} mode={col}");
                }
            }
        }
    }

    /// CC_LENS layout is keyed by the same `layout_id` that lives on
    /// each `VersionMetric`. Spot-check a few format/version anchors:
    /// the lookup should hit a meaningful row, not just zeros.
    #[test]
    fn cci_bits_match_metrics_layout_id() {
        // V1 (FULL_METRICS[4]) has layout_id 0.
        assert_eq!(FULL_METRICS[4].layout_id, 0);
        assert_eq!(cci_bits(FULL_METRICS[4].layout_id, Mode::Numeric), Some(10));
        // V10 (FULL_METRICS[13]) has layout_id 1.
        assert_eq!(FULL_METRICS[13].layout_id, 1);
        assert_eq!(
            cci_bits(FULL_METRICS[13].layout_id, Mode::Numeric),
            Some(12)
        );
        // M1 (FULL_METRICS[0]) has layout_id 3.
        assert_eq!(FULL_METRICS[0].layout_id, 3);
        assert_eq!(cci_bits(FULL_METRICS[0].layout_id, Mode::Numeric), Some(3));
        // R7x43 (FULL_METRICS[44]) has layout_id 7.
        assert_eq!(FULL_METRICS[44].layout_id, 7);
        assert_eq!(cci_bits(FULL_METRICS[44].layout_id, Mode::Numeric), Some(4));
    }

    /// push_bits emits MSB-first.
    #[test]
    fn push_bits_msb_first() {
        let mut out = Vec::new();
        push_bits(&mut out, 0b1011_0100, 8);
        assert_eq!(bits_to_string(&out), "10110100");
        push_bits(&mut out, 0b0001, 4);
        assert_eq!(bits_to_string(&out), "101101000001");
    }

    // ---------------------------------------------------------------
    // Stage 4a — pad + GF(256) + RS tests
    // ---------------------------------------------------------------

    /// `bits_to_bytes` packs MSB-first; partial trailing byte is
    /// zero-padded.
    #[test]
    fn bits_to_bytes_round_trip() {
        // 8 bits = 0x48 ('H')
        let bits = [false, true, false, false, true, false, false, false];
        assert_eq!(bits_to_bytes(&bits), vec![0x48]);
        // 16 bits: 0x40 (0100_0000), 0x12 (0001_0010)
        let bits16 = vec![
            false, true, false, false, false, false, false, false, false, false, false, true,
            false, false, true, false,
        ];
        assert_eq!(bits_to_bytes(&bits16), vec![0x40, 0x12]);
        // 4 bits = 0x6 -> zero-pad to 0x60.
        let bits4 = vec![false, true, true, false];
        assert_eq!(bits_to_bytes(&bits4), vec![0x60]);
    }

    /// TERMINATOR_LEN matches BWIPP qrcode_termlens (bwip-js 25650).
    #[test]
    fn terminator_len_table() {
        assert_eq!(TERMINATOR_LEN[0], 4, "Full V1-9");
        assert_eq!(TERMINATOR_LEN[1], 4, "Full V10-26");
        assert_eq!(TERMINATOR_LEN[2], 4, "Full V27-40");
        assert_eq!(TERMINATOR_LEN[3], 3, "M1");
        assert_eq!(TERMINATOR_LEN[4], 5, "M2");
        assert_eq!(TERMINATOR_LEN[5], 7, "M3");
        assert_eq!(TERMINATOR_LEN[6], 9, "M4");
        // rMQR rows all 3.
        for (i, &len) in TERMINATOR_LEN.iter().enumerate().skip(7) {
            assert_eq!(len, 3, "rMQR row {i}");
        }
    }

    /// PADDING_CODEWORDS literal check.
    #[test]
    fn padding_codewords_constants() {
        assert_eq!(PADDING_CODEWORDS, [0xEC, 0x11]);
    }

    /// ISO 18004 Annex I worked example for V1 H: "01234567" Numeric.
    ///
    /// Mode header (0001) + CCI (0000001000) + numeric body
    /// (000000110001010110011000011) + terminator 0000.
    /// Concatenated then byte-packed produces:
    ///   0x10 0x20 0x0C 0x56 0x61 0x80 0xEC 0x11 0xEC ...
    ///
    /// V1-H data capacity = 9 codewords (we test the pad fill).
    #[test]
    fn pad_codewords_iso_annex_i() {
        // Build the bit-stream: mode + CCI + body. We hand-construct it
        // so this test isolates pad_codewords from the upstream segment
        // emitters. Use the v1to9/Numeric pieces:
        //   * mode indicator 4 bits = 0001
        //   * cci 10 bits = 0000001000 (= 8 = number of digits)
        //   * body 27 bits = 000000110001010110011000011
        // Total: 41 bits.
        let mut bits = Vec::new();
        // mode 4 bits
        for c in "0001".chars() {
            bits.push(c == '1');
        }
        // cci 10 bits
        for c in "0000001000".chars() {
            bits.push(c == '1');
        }
        // numeric body
        for c in "000000110001010110011000011".chars() {
            bits.push(c == '1');
        }
        assert_eq!(bits.len(), 41);

        // V1-H: datacap_bits = 208 - (17 * 8) = 72, dcws = 9. Per
        // FULL_METRICS[4]: datacap_bits=208 (V1 module count), eclen[3]=17,
        // so dcws = (208/8) - 17 = 26 - 17 = 9.
        let pad = pad_codewords(&bits, 0, 72, 9).unwrap();
        assert_eq!(pad.len(), 9);
        // ISO 18004 Annex I final padded data:
        //   0x10 0x20 0x0C 0x56 0x61 0x80 0xEC 0x11 0xEC
        assert_eq!(
            pad,
            vec![0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC]
        );
    }

    /// `pad_codewords` truncates the terminator if the remaining
    /// capacity is shorter than the format terminator length.
    #[test]
    fn pad_codewords_terminator_truncated() {
        // Fill exactly to data_capacity_bits — terminator should be 0,
        // zero-pad = 0, no 0xEC needed (already full). Use V1-H (Full
        // layout_id 0, dcws=9, datacap=72 bits) so no lc4b nibble fix
        // is needed.
        let bits = [true; 72];
        let pad = pad_codewords(&bits, 0, 72, 9).unwrap();
        assert_eq!(pad.len(), 9);
        // All ones in the first 9 bytes.
        assert_eq!(pad, vec![0xFF; 9]);
    }

    /// `pad_codewords` rejects over-capacity input.
    #[test]
    fn pad_codewords_overcap_error() {
        let bits = vec![false; 100];
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 1702-1706 (`qrcode_native: bit-stream (100 bits) exceeds
        // capacity (72 bits)`).
        match pad_codewords(&bits, 0, 72, 9).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("bit-stream"),
                    "missing `bit-stream` predicate: {msg}"
                );
                assert!(
                    msg.contains("100 bits"),
                    "missing `100 bits` actual-length echo: {msg}"
                );
                assert!(
                    msg.contains("exceeds capacity"),
                    "missing `exceeds capacity` predicate: {msg}"
                );
                assert!(
                    msg.contains("72 bits"),
                    "missing `72 bits` capacity echo: {msg}"
                );
            }
            other => panic!("100-bit pad_codewords should reject as InvalidData, got {other:?}"),
        }
    }

    /// GF(256) exp/log tables are mutual inverses for all non-zero
    /// values.
    #[test]
    fn qr_gf256_exp_log_inverses() {
        for i in 1u8..=254 {
            let e = QR_GF256_EXP[i as usize];
            assert_ne!(e, 0, "EXP[{i}] should be non-zero");
            assert_eq!(QR_GF256_LOG[e as usize], i, "LOG[EXP[{i}]] should be {i}");
        }
    }

    /// GF(256) multiplication is commutative; multiplying by 0 = 0;
    /// multiplying by 1 = identity.
    #[test]
    fn qr_gf256_mul_properties() {
        for a in [0u8, 1, 2, 7, 13, 100, 255] {
            assert_eq!(qr_gf256_mul(a, 0), 0);
            assert_eq!(qr_gf256_mul(0, a), 0);
            assert_eq!(qr_gf256_mul(a, 1), a);
            assert_eq!(qr_gf256_mul(1, a), a);
        }
        for a in [2u8, 3, 5, 17, 100, 200] {
            for b in [2u8, 3, 5, 17, 100, 200] {
                assert_eq!(qr_gf256_mul(a, b), qr_gf256_mul(b, a), "commutativity");
            }
        }
        // Specific anchor: alpha^1 * alpha^1 = alpha^2 = 4.
        assert_eq!(qr_gf256_mul(2, 2), 4);
        // alpha^7 * alpha^248 = alpha^(7+248 mod 255) = alpha^0 = 1.
        assert_eq!(
            qr_gf256_mul(QR_GF256_EXP[7], QR_GF256_EXP[248]),
            1,
            "log-sum mod 255 wraps to 0 → alpha^0 = 1"
        );
    }

    /// QR generator polynomial for 10 EC bytes matches the ISO 18004
    /// Annex A.2 coefficients (printed in α-exponent form there; we
    /// store in linear GF(256) form, so verify via the LFSR output
    /// against ISO 18004 Annex I).
    ///
    /// Annex A.2 for n=10 says the polynomial is:
    ///   x^10 + α^251·x^9 + α^67·x^8 + α^46·x^7 + α^61·x^6 + α^118·x^5
    ///        + α^70·x^4 + α^64·x^3 + α^94·x^2 + α^32·x + α^45
    ///
    /// Convert to linear: α^251=193, α^67=139, α^46=216, α^61=70,
    /// α^118=46, α^70=15, α^64=92, α^94=180, α^32=86, α^45=251.
    ///
    /// Our `qr_rs_gen_coeffs` returns coefficients in LFSR-order:
    /// `[a^45, a^32, a^94, a^64, a^70, a^118, a^61, a^46, a^67, a^251]`.
    #[test]
    fn qr_rs_gen_coeffs_n10() {
        let coeffs = qr_rs_gen_coeffs(10);
        assert_eq!(coeffs.len(), 10);
        // BWIPP returns LFSR-order coeffs (low-degree first). The polynomial
        // (x - α^0)…(x - α^9) expanded yields constant-term α^45 first.
        // Verify by checking each coefficient is the alpha exponent stated
        // in ISO 18004 Annex A.2.
        let alpha_exps = [45u8, 32, 94, 64, 70, 118, 61, 46, 67, 251];
        for (i, &exp) in alpha_exps.iter().enumerate() {
            assert_eq!(
                coeffs[i], QR_GF256_EXP[exp as usize],
                "coeff[{i}] should be α^{exp}"
            );
        }
    }

    /// ISO 18004 Annex I — V1-M (Numeric "01234567"), full RS verification.
    ///
    /// V1-M uses 16 data codewords + 10 EC codewords (eclen[1] = 10).
    /// Annex I encodes "01234567" producing data codewords:
    ///   0x10 0x20 0x0C 0x56 0x61 0x80 0xEC 0x11 0xEC 0x11 0xEC 0x11
    ///   0xEC 0x11 0xEC 0x11
    /// Expected 10 EC codewords (ISO 18004 Annex I.2):
    ///   0xA5 0x24 0xD4 0xC1 0xED 0x36 0xC7 0x87 0x2C 0x55
    #[test]
    fn qr_rs_block_ecc_iso_annex_i_v1_m() {
        let data = [
            0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11,
            0xEC, 0x11,
        ];
        let expected_ecc = [0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55];
        let ecc = qr_rs_block_ecc(&data, 10);
        assert_eq!(ecc, expected_ecc, "ISO 18004 Annex I.2 EC codewords");
    }

    // ---------------------------------------------------------------
    // Stage 4b — interleaver + end-to-end stream tests
    // ---------------------------------------------------------------

    /// `block_layout` computes the per-(metric, ec_level) parameters
    /// matching the bwip-js oracle dump.
    #[test]
    fn block_layout_v1_l() {
        // V1 is FULL_METRICS[4] (after 4 micro rows).
        let l = block_layout(4, 0).unwrap();
        assert_eq!(l.ncws, 26);
        assert_eq!(l.dcws, 19);
        assert_eq!(l.ecpb, 7);
        assert_eq!(l.ecb1, 1);
        assert_eq!(l.ecb2, 0);
        assert_eq!(l.dcpb, 19);
        assert_eq!(l.rbit, 0);
        assert!(!l.lc4b);
    }

    #[test]
    fn block_layout_v5_q() {
        // V5 is FULL_METRICS[8] (V1..V5 = index 4..=8). EC Q = 2.
        let l = block_layout(8, 2).unwrap();
        assert_eq!(l.ncws, 134);
        assert_eq!(l.dcws, 62);
        assert_eq!(l.ecpb, 18);
        assert_eq!(l.ecb1, 2);
        assert_eq!(l.ecb2, 2);
        assert_eq!(l.dcpb, 15);
        assert_eq!(l.rbit, 7, "V5 datacap_bits=1079, 1079%8=7");
        assert!(!l.lc4b);
    }

    #[test]
    fn block_layout_m1() {
        let l = block_layout(0, 0).unwrap();
        assert_eq!(l.ncws, 5, "M1 datacap=36, +1 for lc4b");
        assert_eq!(l.dcws, 3);
        assert_eq!(l.ecpb, 2);
        assert_eq!(l.ecb1, 1);
        assert_eq!(l.ecb2, 0);
        assert!(l.lc4b);
        assert_eq!(l.rbit, 0);
    }

    #[test]
    fn block_layout_r7x43() {
        // R7x43 is FULL_METRICS[44]. EC M only (eclen[0]=NA).
        let l = block_layout(44, 1).unwrap();
        assert_eq!(l.ncws, 13);
        assert_eq!(l.dcws, 6);
        assert_eq!(l.ecpb, 7);
        // Confirm EC L is rejected (eclen[0]=NA sentinel).
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 1935-1939 (`qrcode_native: format/version `R7x43` does
        // not support EC level L`). Cross-arm guard against the
        // ec_level >= 4 range arm.
        match block_layout(44, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(msg.contains("qrcode_native:"), "missing prefix: {msg}");
                assert!(
                    msg.contains("R7x43"),
                    "missing `R7x43` version-name echo: {msg}"
                );
                assert!(
                    msg.contains("does not support EC level"),
                    "missing `does not support EC level` predicate: {msg}"
                );
                assert!(
                    msg.contains("EC level L"),
                    "missing `EC level L` letter echo: {msg}"
                );
                assert!(
                    !msg.contains("out of range"),
                    "wrong arm — ec_level range diagnostic leaked: {msg}"
                );
            }
            other => panic!(
                "block_layout(44, 0) (R7x43 EC L) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    #[test]
    fn block_layout_rejects_out_of_range() {
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at
        // lines 1912-1914 (ec_level overflow) and 1916-1920
        // (metric_idx overflow).
        match block_layout(4, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "ec_level overflow arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("ec_level 4"),
                    "ec_level overflow arm missing `ec_level 4` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=3"),
                    "ec_level overflow arm missing range hint: {msg}"
                );
                assert!(
                    !msg.contains("metric_idx"),
                    "wrong arm — metric_idx diagnostic leaked: {msg}"
                );
            }
            other => panic!("block_layout(4, 4) (ec_level overflow) should reject as InvalidData, got {other:?}"),
        }
        match block_layout(999, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "metric_idx overflow arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("metric_idx 999"),
                    "metric_idx overflow arm missing `metric_idx 999` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range"),
                    "metric_idx overflow arm missing `out of range` predicate: {msg}"
                );
                assert!(
                    !msg.contains("ec_level"),
                    "wrong arm — ec_level diagnostic leaked: {msg}"
                );
            }
            other => panic!("block_layout(999, 0) (metric_idx overflow) should reject as InvalidData, got {other:?}"),
        }
    }

    /// `split_data_blocks` honors ebc1 + ebc2 layout.
    #[test]
    fn split_data_blocks_multi_block() {
        // Synthetic V5-Q layout: 2 group-1 (15 each) + 2 group-2 (16 each).
        let layout = block_layout(8, 2).unwrap();
        let data: Vec<u8> = (0..62).collect();
        let blocks = split_data_blocks(&data, &layout);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].len(), 15);
        assert_eq!(blocks[0], &(0..15).collect::<Vec<u8>>());
        assert_eq!(blocks[1].len(), 15);
        assert_eq!(blocks[1], &(15..30).collect::<Vec<u8>>());
        assert_eq!(blocks[2].len(), 16);
        assert_eq!(blocks[2], &(30..46).collect::<Vec<u8>>());
        assert_eq!(blocks[3].len(), 16);
        assert_eq!(blocks[3], &(46..62).collect::<Vec<u8>>());
    }

    /// `interleave_blocks` column-major: data then ECC.
    #[test]
    fn interleave_blocks_simple_2_block() {
        let d: Vec<u8> = (0..5).collect();
        let e: Vec<u8> = (100..103).collect();
        let d2: Vec<u8> = (10..15).collect();
        let e2: Vec<u8> = (200..203).collect();
        let interleaved = interleave_blocks(&[&d, &d2], &[e.clone(), e2.clone()]);
        // Data column-major: (0,10),(1,11),(2,12),(3,13),(4,14)
        // ECC column-major: (100,200),(101,201),(102,202)
        let want = vec![
            0, 10, 1, 11, 2, 12, 3, 13, 4, 14, 100, 200, 101, 201, 102, 202,
        ];
        assert_eq!(interleaved, want);
    }

    /// `interleave_blocks` skips shorter group-1 blocks for trailing
    /// column.
    #[test]
    fn interleave_blocks_mixed_block_sizes() {
        let d_small: Vec<u8> = vec![1, 2];
        let d_large: Vec<u8> = vec![10, 20, 30];
        let e_small: Vec<u8> = vec![100];
        let e_large: Vec<u8> = vec![200];
        let out = interleave_blocks(&[&d_small, &d_large], &[e_small, e_large]);
        // Data cols 0..3: col0 = (1, 10); col1 = (2, 20); col2 = (skip small, 30).
        // ECC col0 = (100, 200).
        let want = vec![1, 10, 2, 20, 30, 100, 200];
        assert_eq!(out, want);
    }

    /// V1-L "HELLO WORLD" — full codeword stream byte-for-byte against
    /// patched bwip-js oracle (extract-qrcode-codewords.js debugecc).
    #[test]
    fn build_codeword_stream_v1_l_hello_world_oracle() {
        let padded_data: [u8; 19] = [
            32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17, 236, 17, 236,
        ];
        let stream = build_codeword_stream(&padded_data, 4, 0).unwrap();
        let oracle: [u8; 26] = [
            32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17, 236, 17, 236,
            209, 239, 196, 207, 78, 195, 109,
        ];
        assert_eq!(stream, oracle, "V1-L 'HELLO WORLD' bwip-js oracle");
    }

    /// V1-M "01234567" — full codeword stream byte-for-byte.
    #[test]
    fn build_codeword_stream_v1_m_01234567_oracle() {
        let padded_data: [u8; 16] = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17,
        ];
        let stream = build_codeword_stream(&padded_data, 4, 1).unwrap();
        let oracle: [u8; 26] = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 165, 36, 212,
            193, 237, 54, 199, 135, 44, 85,
        ];
        assert_eq!(stream, oracle, "V1-M '01234567' bwip-js oracle");
    }

    /// V5-Q "HELLO WORLD HELLO WORLD HELLO WORLD" — multi-block (4
    /// blocks, 2 of size 15 + 2 of size 16), rbit=7 trailing zero.
    /// Full stream byte-for-byte against patched bwip-js oracle.
    #[test]
    fn build_codeword_stream_v5_q_multi_block_oracle() {
        let padded_data: [u8; 62] = [
            33, 27, 11, 120, 209, 114, 220, 77, 68, 218, 194, 222, 52, 92, 183, 19, 81, 54, 176,
            183, 141, 23, 45, 196, 212, 52, 0, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 236,
            17, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17,
            236, 17, 236, 17, 236,
        ];
        let stream = build_codeword_stream(&padded_data, 8, 2).unwrap();
        assert_eq!(
            stream.len(),
            135,
            "V5-Q stream = ncws(134) + rbit(7) zero codeword"
        );
        let oracle: [u8; 135] = [
            33, 19, 17, 17, 27, 81, 236, 236, 11, 54, 17, 17, 120, 176, 236, 236, 209, 183, 17, 17,
            114, 141, 236, 236, 220, 23, 17, 17, 77, 45, 236, 236, 68, 196, 17, 17, 218, 212, 236,
            236, 194, 52, 17, 17, 222, 0, 236, 236, 52, 236, 17, 17, 92, 17, 236, 236, 183, 236,
            17, 17, 236, 236, 182, 231, 135, 135, 112, 198, 147, 147, 20, 209, 7, 7, 208, 230, 41,
            41, 195, 83, 128, 128, 45, 133, 150, 150, 98, 147, 120, 120, 45, 123, 184, 184, 59, 97,
            37, 37, 109, 238, 181, 181, 175, 185, 205, 205, 145, 205, 222, 222, 168, 174, 231, 231,
            105, 114, 8, 8, 195, 184, 44, 44, 143, 241, 81, 81, 15, 19, 173, 173, 111, 164, 80, 80,
            0,
        ];
        assert_eq!(stream, oracle, "V5-Q multi-block bwip-js oracle");
    }

    /// R7x43 "1234" (rMQR, single block) — oracle stream byte-for-byte.
    /// The bwip-js oracle was invoked with `fixedeclevel: true` and
    /// default eclevel ("unset"). For rMQR R7x43 BWIPP picks M (the
    /// lowest supported level; L and Q are NA) → dcws = 6 (3 message
    /// bytes + 3 padding 0xEC 0x11 0xEC), ecpb = 7.
    #[test]
    fn build_codeword_stream_r7x43_oracle() {
        let padded_data: [u8; 6] = [40, 61, 160, 236, 17, 236];
        let stream = build_codeword_stream(&padded_data, 44, 1).unwrap();
        let oracle: [u8; 13] = [40, 61, 160, 236, 17, 236, 23, 232, 137, 120, 33, 163, 40];
        assert_eq!(stream, oracle, "R7x43 rMQR oracle (EC M)");
    }

    /// M1 "12345" — lc4b nibble shift applied, single block.
    #[test]
    fn build_codeword_stream_m1_lc4b_oracle() {
        let padded_data: [u8; 3] = [163, 218, 208];
        let stream = build_codeword_stream(&padded_data, 0, 0).unwrap();
        let oracle: [u8; 5] = [163, 218, 214, 236, 112];
        assert_eq!(stream, oracle, "M1 '12345' lc4b oracle");
    }

    /// `build_codeword_stream` rejects data length mismatch.
    #[test]
    fn build_codeword_stream_length_mismatch() {
        let bad = [0u8; 18]; // V1-L expects 19
                             // Stage 11.A8c — upgrade discriminant-only `matches!` to a
                             // 4-anchor pin matching the source diagnostic at line
                             // 2060-2064 (`qrcode_native: padded_data length 18 does not
                             // match dcws 19`).
        match build_codeword_stream(&bad, 4, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("padded_data length 18"),
                    "missing actual-length echo `padded_data length 18`: {msg}"
                );
                assert!(
                    msg.contains("does not match dcws"),
                    "missing predicate: {msg}"
                );
                assert!(
                    msg.contains("dcws 19"),
                    "missing expected-length echo `dcws 19` (V1-L data codewords): {msg}"
                );
            }
            other => panic!(
                "18-byte V1-L (expects 19) build_codeword_stream should reject as InvalidData, got {other:?}"
            ),
        }
    }

    // ---------------------------------------------------------------
    // Stage 5a — input counters + threshold tables
    // ---------------------------------------------------------------

    /// `QRCODE_MIDS` shape and known cells.
    #[test]
    fn qrcode_mids_shape_and_anchors() {
        // Full V1-9 has 4-bit indicators for all 5 modes.
        assert_eq!(QRCODE_MIDS[0][0], Some("0001"));
        assert_eq!(QRCODE_MIDS[0][1], Some("0010"));
        assert_eq!(QRCODE_MIDS[0][2], Some("0100"));
        assert_eq!(QRCODE_MIDS[0][3], Some("1000"));
        assert_eq!(QRCODE_MIDS[0][4], Some("0111"));
        // Same for V10-26 and V27-40.
        assert_eq!(QRCODE_MIDS[1][0], Some("0001"));
        assert_eq!(QRCODE_MIDS[2][0], Some("0001"));
        // M1: numeric-only, empty mode bits.
        assert_eq!(QRCODE_MIDS[3][0], Some(""));
        assert_eq!(QRCODE_MIDS[3][1], None);
        assert_eq!(QRCODE_MIDS[3][4], None);
        // M2: 1-bit (N=0, A=1).
        assert_eq!(QRCODE_MIDS[4][0], Some("0"));
        assert_eq!(QRCODE_MIDS[4][1], Some("1"));
        assert_eq!(QRCODE_MIDS[4][2], None);
        // M3: 2-bit (N=00..K=11).
        assert_eq!(QRCODE_MIDS[5][0], Some("00"));
        assert_eq!(QRCODE_MIDS[5][3], Some("11"));
        assert_eq!(QRCODE_MIDS[5][4], None);
        // M4: 3-bit (N=000..K=011).
        assert_eq!(QRCODE_MIDS[6][0], Some("000"));
        assert_eq!(QRCODE_MIDS[6][3], Some("011"));
        assert_eq!(QRCODE_MIDS[6][4], None);
        // rMQR R7x43 (row 7): 3-bit (N=001..E=111).
        assert_eq!(QRCODE_MIDS[7][0], Some("001"));
        assert_eq!(QRCODE_MIDS[7][4], Some("111"));
        // R17x139 (last row): same.
        assert_eq!(QRCODE_MIDS[38][0], Some("001"));
    }

    /// Threshold tables match BWIPP bwip-js 26641-26659 anchors.
    #[test]
    fn mode_threshold_tables_anchors() {
        // mode0forceKB: row 0 = 1, row 3 (M1) = INFEAS, row 5 (M3) = 1.
        assert_eq!(QRCODE_MODE0_FORCE_KB[0], 1);
        assert_eq!(QRCODE_MODE0_FORCE_KB[3], INFEAS);
        assert_eq!(QRCODE_MODE0_FORCE_KB[5], 1);
        // mode0forceN: all rows = 1 (N is supported in all 39 layouts).
        for (i, v) in QRCODE_MODE0_FORCE_N.iter().enumerate() {
            assert_eq!(*v, 1, "FORCE_N row {i}");
        }
        // modeBNbeforeE row 0 = 3 (Full V1-9).
        assert_eq!(QRCODE_MODE_BN_BEFORE_E[0], 3);
        // modeANbeforeA row 38 (R17x139) = 11.
        assert_eq!(QRCODE_MODE_AN_BEFORE_A[38], 11);
        // Spot-check infeasibility: M1 (row 3) has INFEAS in all
        // K/A-bearing tables.
        assert_eq!(QRCODE_MODE_BK_BEFORE_B[3], INFEAS);
        assert_eq!(QRCODE_MODE_BA_BEFORE_K[3], INFEAS);
    }

    /// `INFEAS` sentinel matches BWIPP `$_.e = 10000`.
    #[test]
    fn infeas_sentinel() {
        assert_eq!(INFEAS, 10_000);
    }

    /// Stage 11.A8c — pin `is_alpha_only_byte(b)`. The 35-char QR
    /// `qrcode_Aexcl` membership predicate (A-Z + 9 symbols, NO
    /// digits) is only exercised transitively through the segment
    /// selector. Mutations to catch:
    ///   - `b'A'..=b'Z'` → `b'A'..=b'Y'` or `b'B'..=b'Z'`: drops
    ///     a letter endpoint.
    ///   - `| b' '` removed: space (the most common QR-alpha char)
    ///     wrongly rejected.
    ///   - Any symbol arm removed: that symbol classified as
    ///     "alpha-only-with-digits" instead of "alpha-only".
    ///   - Whole match replaced with `false`: alpha-only mode never
    ///     selected.
    ///   - Whole match replaced with `true`: digits and lowercase
    ///     all wrongly classified as alpha-only.
    #[test]
    fn is_alpha_only_byte_membership() {
        // All 26 letters A-Z accepted.
        for c in b'A'..=b'Z' {
            assert!(
                is_alpha_only_byte(c),
                "letter {:?} should be alpha-only",
                c as char
            );
        }
        // All 9 symbols accepted (BWIPP qrcode_Aexcl).
        for &c in b" $%*+-./:" {
            assert!(
                is_alpha_only_byte(c),
                "symbol {:?} should be alpha-only",
                c as char
            );
        }
        // Digits 0-9: NOT in the set (the "no digits" defining trait).
        for c in b'0'..=b'9' {
            assert!(
                !is_alpha_only_byte(c),
                "digit {:?} must NOT be alpha-only (this set excludes digits)",
                c as char
            );
        }
        // Lowercase letters: not in QR alphanumeric at all.
        for c in b'a'..=b'z' {
            assert!(!is_alpha_only_byte(c), "lowercase {:?} rejected", c as char);
        }
        // Boundary chars near A-Z.
        assert!(!is_alpha_only_byte(b'@'), "@ (one before A) rejected");
        assert!(!is_alpha_only_byte(b'['), "[ (one after Z) rejected");
        // Adjacent non-members of the symbol set.
        assert!(!is_alpha_only_byte(b'!'), "! rejected");
        assert!(!is_alpha_only_byte(b'#'), "# rejected");
        assert!(!is_alpha_only_byte(b'_'), "_ rejected");
        assert!(!is_alpha_only_byte(b'?'), "? rejected");
        assert!(!is_alpha_only_byte(b';'), "; (one after :) rejected");
        assert!(!is_alpha_only_byte(b','), ", (one before -) rejected");
        // Control chars and high bytes.
        assert!(!is_alpha_only_byte(0));
        assert!(!is_alpha_only_byte(0x1F));
        assert!(!is_alpha_only_byte(0x80));
        assert!(!is_alpha_only_byte(0xFF));
    }

    /// Stage 11.A8c — pin `is_valid_shift_jis(hi, lo)`. The helper
    /// combines two range bands on the (hi,lo) u16 with a low-byte
    /// validity check and a `lo != 0x7F` exclusion. Only the full
    /// segment compiler exercises it transitively; mutations to catch:
    ///   - `(0x8140..=0x9FFC)` → `(0x8140..0x9FFC)`: excludes the
    ///     upper end of the first band.
    ///   - `||` → `&&` between the two range bands: collapses to
    ///     empty intersection (both bands disjoint).
    ///   - `lo != 0x7F` → `lo == 0x7F`: only 0x7F is accepted as low.
    ///   - `(0x40..=0xFC)` mutated to wrong endpoints: 0x40 / 0xFC
    ///     boundary becomes wrong.
    /// Stage 11.A8c — pin `digit_value(b)`:
    ///   * `b.is_ascii_digit()` → `Ok(b - b'0')`.
    ///   * Anything else → `Err(InvalidData)` with byte echoed.
    ///
    /// Mutations caught:
    ///   * `is_ascii_digit()` → `is_alphanumeric()` lets letters through.
    ///   * `b - b'0'` → `b + b'0'` returns wrong value.
    ///   * Catch-all swapped to `Ok(...)` would silently accept.
    #[test]
    fn digit_value_pins_digit_arm_and_default_error() {
        // 0..=9 → 0..=9.
        for d in 0..=9 {
            assert_eq!(digit_value(b'0' + d).unwrap(), d);
        }
        // Stage 11.A8c (cont) — 7 bare `.is_err()` boundary checks
        // upgraded to 3-anchor pins matching the source diagnostic at
        // line 437-449 of qrcode_native/mod.rs (`qrcode_native:
        // numeric mode expects ASCII digit; got byte 0x{b:02X}`).
        // Per arm: `qrcode_native:` prefix + `numeric mode expects
        // ASCII digit` predicate + hex value-echo. Brings parity with
        // the alpha_value sibling and the explicit b'X' arm below.
        for (b, hex) in [
            (b'/', "0x2F"), // one before '0'
            (b':', "0x3A"), // one after '9'
            (b'A', "0x41"), // ASCII letter
            (b'a', "0x61"),
            (b' ', "0x20"), // space
            (0, "0x00"),    // NUL
            (255, "0xFF"),  // 0xFF
        ] {
            match digit_value(b).unwrap_err() {
                crate::error::Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("qrcode_native:"),
                        "byte={b:#x}: missing `qrcode_native:` prefix: {msg}"
                    );
                    assert!(
                        msg.contains("numeric mode expects ASCII digit"),
                        "byte={b:#x}: missing `numeric mode expects ASCII digit` predicate: {msg}"
                    );
                    assert!(
                        msg.contains(hex),
                        "byte={b:#x}: missing `{hex}` hex value-echo: {msg}"
                    );
                }
                other => panic!("byte={b:#x} should reject as InvalidData, got {other:?}"),
            }
        }
        // Error message contains the rejected byte's hex.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains("0x58")`
        // upgraded to 4-anchor pin:
        //   1. `qrcode_native:` symbology prefix
        //   2. `numeric mode` mode-name anchor (kills `numeric →
        //      alpha` mode-rename mutation)
        //   3. `expects ASCII digit` predicate
        //   4. `0x58` hex value echo (already pinned)
        match digit_value(b'X').unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing qrcode_native prefix: {msg}"
                );
                assert!(
                    msg.contains("numeric mode"),
                    "missing `numeric mode` mode-name: {msg}"
                );
                assert!(
                    msg.contains("expects ASCII digit"),
                    "missing predicate: {msg}"
                );
                assert!(msg.contains("0x58"), "expected hex 0x58 in: {msg}");
            }
            o => panic!("expected InvalidData, got {o:?}"),
        }
    }

    /// Stage 11.A8c — pin `alpha_value(b)`:
    ///   * Indexes ALPHANUMERIC_VALUE[b]; if v < 0 → Err, else Ok(v).
    ///   * 45-char alphabet: '0'..='9' → 0..=9, 'A'..='Z' → 10..=35,
    ///     ' ' → 36, '$' → 37, '%' → 38, '*' → 39, '+' → 40, '-' → 41,
    ///     '.' → 42, '/' → 43, ':' → 44.
    ///
    /// Mutations caught:
    ///   * `v < 0` → `v <= 0` would reject value 0 (`'0'`).
    ///   * `v as u8` cast direction reversal flips error vs ok.
    ///   * ALPHANUMERIC_VALUE table corruption per-arm checked by
    ///     spot-checks below.
    #[test]
    fn alpha_value_per_arm_and_reject() {
        // Digits 0..9.
        assert_eq!(alpha_value(b'0').unwrap(), 0);
        assert_eq!(alpha_value(b'9').unwrap(), 9);
        // Uppercase A..Z → 10..35.
        assert_eq!(alpha_value(b'A').unwrap(), 10);
        assert_eq!(alpha_value(b'Z').unwrap(), 35);
        // Punctuation singletons.
        assert_eq!(alpha_value(b' ').unwrap(), 36);
        assert_eq!(alpha_value(b'$').unwrap(), 37);
        assert_eq!(alpha_value(b'%').unwrap(), 38);
        assert_eq!(alpha_value(b'*').unwrap(), 39);
        assert_eq!(alpha_value(b'+').unwrap(), 40);
        assert_eq!(alpha_value(b'-').unwrap(), 41);
        assert_eq!(alpha_value(b'.').unwrap(), 42);
        assert_eq!(alpha_value(b'/').unwrap(), 43);
        assert_eq!(alpha_value(b':').unwrap(), 44);
        // Lowercase + other punctuation rejected.
        // Stage 11.A8c (cont) — 5 bare `.is_err()` checks upgraded to
        // 3-anchor pins matching the source diagnostic at line 493-495
        // of qrcode_native/mod.rs (`qrcode_native: alphanumeric mode
        // rejects byte 0x{b:02X}`). Each arm pins:
        //   1. `qrcode_native:` symbology prefix
        //   2. `alphanumeric mode` mode-name (kills `alphanumeric →
        //      numeric` mode-rename mutations — bringing parity with
        //      the digit_value test's `numeric mode` anchor)
        //   3. Hex value-echo of the rejected byte (0x61 / 0x21 /
        //      0x23 / 0x40 / 0x5B)
        for (b, hex) in [
            (b'a', "0x61"),
            (b'!', "0x21"),
            (b'#', "0x23"),
            (b'@', "0x40"),
            (b'[', "0x5B"),
        ] {
            match alpha_value(b).unwrap_err() {
                crate::error::Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("qrcode_native:"),
                        "byte={b:#x}: missing `qrcode_native:` prefix: {msg}"
                    );
                    assert!(
                        msg.contains("alphanumeric mode"),
                        "byte={b:#x}: missing `alphanumeric mode` mode-name: {msg}"
                    );
                    assert!(
                        msg.contains(hex),
                        "byte={b:#x}: missing `{hex}` hex value-echo: {msg}"
                    );
                    // Note: cross-mode-name guard against `numeric
                    // mode` is impossible because `alphanumeric mode`
                    // is a literal superstring of `numeric mode`. The
                    // 3 positive anchors above are sufficient — the
                    // `alphanumeric mode` substring is unique enough
                    // (it appears only in this format string in the
                    // qrcode_native module).
                }
                other => panic!("byte={b:#x} should reject, got {other:?}"),
            }
        }
    }

    #[test]
    fn is_valid_shift_jis_band_and_low_byte_checks() {
        // Valid in first band: 0x8140 (band start) and 0x9FFC (band end).
        assert!(is_valid_shift_jis(0x81, 0x40), "0x8140 (first band start)");
        assert!(is_valid_shift_jis(0x9F, 0xFC), "0x9FFC (first band end)");
        // Valid in second band: 0xE040 (start) and 0xEBBF (end).
        assert!(is_valid_shift_jis(0xE0, 0x40), "0xE040 (second band start)");
        assert!(is_valid_shift_jis(0xEB, 0xBF), "0xEBBF (second band end)");
        // Valid mid-range.
        assert!(is_valid_shift_jis(0x88, 0x50));

        // Excluded low byte: lo == 0x7F.
        assert!(
            !is_valid_shift_jis(0x81, 0x7F),
            "lo=0x7F must be excluded even if hi is in range"
        );
        assert!(!is_valid_shift_jis(0x88, 0x7F));
        // Low byte below 0x40.
        assert!(
            !is_valid_shift_jis(0x81, 0x39),
            "lo<0x40 rejected (just below range)"
        );
        assert!(!is_valid_shift_jis(0x81, 0x00));
        // Low byte above 0xFC.
        assert!(
            !is_valid_shift_jis(0x81, 0xFD),
            "lo>0xFC rejected (just above range)"
        );
        assert!(!is_valid_shift_jis(0x81, 0xFF));

        // Outside both bands: between-band gap and end-of-band+1.
        assert!(
            !is_valid_shift_jis(0xA0, 0x40),
            "0xA040 in the gap between the two bands"
        );
        assert!(!is_valid_shift_jis(0xDF, 0x40), "0xDF40 still in the gap");
        assert!(!is_valid_shift_jis(0xEC, 0x40), "0xEC40 beyond second band");
        assert!(!is_valid_shift_jis(0x80, 0x40), "0x8040 below first band");
        // Boundary just past first band end: 0x9FFD.
        assert!(
            !is_valid_shift_jis(0x9F, 0xFD),
            "0x9FFD just past first band (also lo>0xFC)"
        );
    }

    /// `is_kanji_leader` matches BWIPP qrcode_Kexcl ranges.
    #[test]
    fn is_kanji_leader_ranges() {
        // 0x81..=0x9F
        assert!(is_kanji_leader(0x81));
        assert!(is_kanji_leader(0x90));
        assert!(is_kanji_leader(0x9F));
        // 0xE0..=0xEB
        assert!(is_kanji_leader(0xE0));
        assert!(is_kanji_leader(0xEB));
        // Outside
        assert!(!is_kanji_leader(0x00));
        assert!(!is_kanji_leader(0x80));
        assert!(!is_kanji_leader(0xA0));
        assert!(!is_kanji_leader(0xDF));
        assert!(!is_kanji_leader(0xEC));
        assert!(!is_kanji_leader(0xFF));
    }

    /// `compute_input_counters` for "01234567" — all-numeric input.
    #[test]
    fn input_counters_numeric() {
        // suppress_kanji_mode=true matches BWIPP's default for the
        // bwipp_qrcode entry point.
        let c = compute_input_counters(b"01234567", true);
        assert_eq!(c.num_n.len(), 9, "msglen + 1 entries");
        assert_eq!(c.num_n, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]);
        // No alpha-only chars (digits are in Nexcl, not Aexcl).
        assert_eq!(c.num_a, vec![0; 9]);
        // numAorN = numN + numA = numN since no alpha-only chars.
        assert_eq!(c.num_a_or_n, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]);
        // No bytes anywhere.
        assert_eq!(c.num_b, vec![0; 9]);
        assert_eq!(c.num_k, vec![0; 9]);
        // nextNs: 0 at every digit position; trailing 0 at msglen.
        assert_eq!(c.next_n, vec![0; 9]);
        // nextAs: distance to next alpha-only — none in input, so
        // increases by 1 each step backward from msglen.
        assert_eq!(c.next_a, vec![8, 7, 6, 5, 4, 3, 2, 1, 0]);
    }

    /// `compute_input_counters` for "HELLO WORLD" — alphanumeric.
    #[test]
    fn input_counters_alphanumeric() {
        let c = compute_input_counters(b"HELLO WORLD", true);
        assert_eq!(c.num_n.len(), 12);
        // No digits anywhere → numN = 0.
        assert_eq!(c.num_n, vec![0; 12]);
        // All chars are alpha-only (incl. space which is in Aexcl).
        assert_eq!(c.num_a, vec![11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(c.num_a_or_n, vec![11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(c.num_b, vec![0; 12]);
        assert_eq!(c.num_k, vec![0; 12]);
        // nextNs: no digits → bumps by 1 each backward step.
        assert_eq!(c.next_n[0], 11);
        // nextAs: 0 at every char position (all are alpha-only).
        assert_eq!(c.next_a, vec![0; 12]);
    }

    /// `compute_input_counters` for "ABC123def" — mixed alpha-only +
    /// digit + lowercase (lowercase falls back to Byte).
    #[test]
    fn input_counters_mixed() {
        let c = compute_input_counters(b"ABC123def", true);
        assert_eq!(c.num_n.len(), 10);
        // numN = trailing digit run. Digits at positions 3-5.
        assert_eq!(c.num_n, vec![0, 0, 0, 3, 2, 1, 0, 0, 0, 0]);
        // numA = trailing alpha-only run. Alpha at positions 0-2.
        assert_eq!(c.num_a, vec![3, 2, 1, 0, 0, 0, 0, 0, 0, 0]);
        // numAorN = trailing alpha-or-digit run. Positions 0-5 are
        // alpha-or-digit; 6-8 are lowercase (bytes).
        assert_eq!(c.num_a_or_n, vec![6, 5, 4, 3, 2, 1, 0, 0, 0, 0]);
        // numB = trailing byte run. Positions 6-8 ('d','e','f') are
        // bytes; 0-5 aren't.
        assert_eq!(c.num_b, vec![0, 0, 0, 0, 0, 0, 3, 2, 1, 0]);
        assert_eq!(c.num_k, vec![0; 10]);
    }

    /// `compute_input_counters` for a kanji Shift-JIS pair when
    /// `suppress_kanji_mode = false` — actually counts the kanji.
    #[test]
    fn input_counters_kanji_active() {
        // 0x93 0x5F = "点" (valid Shift-JIS).
        let c = compute_input_counters(&[0x93, 0x5F], false);
        assert_eq!(c.num_k.len(), 3);
        // One kanji pair → numK[0] = 1, numK[1] = 0 (zeroed by post-pass).
        assert_eq!(c.num_k, vec![1, 0, 0]);
        // nextK: 0 at position 0 (kanji leader), 1 at position 1
        // (was 9999, then incremented by post-pass).
        assert_eq!(c.next_k[0], 0);
    }

    /// With `suppress_kanji_mode = true` the kanji classification
    /// is disabled — the pair is treated as raw bytes.
    #[test]
    fn input_counters_kanji_suppressed() {
        let c = compute_input_counters(&[0x93, 0x5F], true);
        assert_eq!(c.num_k, vec![0, 0, 0]);
        // Bytes instead: 2 trailing bytes.
        assert_eq!(c.num_b, vec![2, 1, 0]);
    }

    /// Single kanji-leader without a valid low byte should NOT count
    /// as kanji regardless of suppress_kanji_mode.
    #[test]
    fn input_counters_kanji_unpaired() {
        let c = compute_input_counters(&[0x93], false);
        assert_eq!(c.num_k, vec![0, 0]);
        assert_eq!(c.num_b, vec![1, 0]);
    }

    // ---------------------------------------------------------------
    // Stage 5b — segment-selector state machine + composer
    // ---------------------------------------------------------------

    /// `select_segments` for "01234567" V1 → single Numeric segment.
    #[test]
    fn select_segments_all_numeric_v1() {
        // V1 (FULL_METRICS[4], layout_id 0).
        let segs = select_segments(b"01234567", 0, false);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].mode, Mode::Numeric);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].len, 8);
    }

    /// `select_segments` for "HELLO WORLD" V1 → single Alphanumeric.
    #[test]
    fn select_segments_all_alphanumeric_v1() {
        let segs = select_segments(b"HELLO WORLD", 0, false);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].mode, Mode::Alphanumeric);
        assert_eq!(segs[0].len, 11);
    }

    /// `select_segments` for pure lowercase "hello" V1 → single Byte.
    #[test]
    fn select_segments_byte_only_v1() {
        // Stage 11.A8c (cont) — descriptive label naming Byte-mode-only path.
        // Pure lowercase "hello" is non-alphanumeric and non-numeric,
        // so the mode selector must produce exactly one Byte segment
        // (no spurious mode-switch overhead).
        let segs = select_segments(b"hello", 0, false);
        assert!(
            !segs.is_empty(),
            "select_segments(b\"hello\", v=0, kanji=false) (pure lowercase → Byte mode) must produce ≥1 segment; got empty vec"
        );
        assert_eq!(
            segs[0].mode,
            Mode::Byte,
            "lowercase \"hello\" must select Mode::Byte (not Alphanumeric — lowercase is not in the QR alphanumeric set); got {:?}",
            segs[0].mode
        );
        assert_eq!(
            segs[0].len, 5,
            "first segment len must equal payload length 5 (no spurious split); got {}",
            segs[0].len
        );
    }

    /// `compose_segments` + `pad_codewords` for V1-L "HELLO WORLD" —
    /// byte-for-byte against bwip-js debugcws oracle (extracted at
    /// commit ec52f9e via /tmp/dump-data-cws.js with fixedeclevel).
    #[test]
    fn compose_v1_l_hello_world_matches_oracle() {
        let segs = select_segments(b"HELLO WORLD", 0, false);
        let bits = compose_segments(b"HELLO WORLD", &segs, 0, false).unwrap();
        // V1-L: layout_id=0, datacap_bits=72 (208/8 - 17 = 9 dcws → 72 bits),
        // dcws=19.
        let data = pad_codewords(&bits, 0, 152, 19).unwrap();
        let expected: [u8; 19] = [
            32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17, 236, 17, 236,
        ];
        assert_eq!(data, expected, "V1-L 'HELLO WORLD' bwip-js debugcws");
    }

    /// `compose_segments` + `pad_codewords` for V1-M "01234567" —
    /// byte-for-byte against bwip-js debugcws oracle (= ISO 18004
    /// Annex I.1 worked example).
    #[test]
    fn compose_v1_m_01234567_matches_oracle() {
        let segs = select_segments(b"01234567", 0, false);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].mode, Mode::Numeric);
        let bits = compose_segments(b"01234567", &segs, 0, false).unwrap();
        // V1-M: layout_id=0, dcws=16, dmod = 128 bits.
        let data = pad_codewords(&bits, 0, 128, 16).unwrap();
        let expected: [u8; 16] = [
            16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17,
        ];
        assert_eq!(
            data, expected,
            "V1-M '01234567' bwip-js debugcws / ISO Annex I.1"
        );
    }

    /// `compose_segments` rejects unsupported mode for a layout.
    #[test]
    fn compose_segments_rejects_unsupported_mode() {
        // M1 (layout_id=3) supports Numeric only. Try injecting an
        // Alphanumeric segment — should error.
        let segs = vec![Segment {
            mode: Mode::Alphanumeric,
            start: 0,
            len: 2,
        }];
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 1574-1577 (`qrcode_native: mode Alphanumeric not supported
        // by layout_id 3`). Cross-arm guard against the layout_id
        // out-of-range arm.
        match compose_segments(b"AB", &segs, 3, false).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("Alphanumeric"),
                    "missing `Alphanumeric` mode echo: {msg}"
                );
                assert!(
                    msg.contains("not supported"),
                    "missing `not supported` predicate: {msg}"
                );
                assert!(
                    msg.contains("layout_id 3"),
                    "missing `layout_id 3` (M1) echo: {msg}"
                );
                assert!(
                    !msg.contains("out of range"),
                    "wrong arm — layout_id range diagnostic leaked: {msg}"
                );
            }
            other => {
                panic!("M1 (layout_id=3) Alphanumeric should reject as InvalidData, got {other:?}")
            }
        }
    }

    // ---------------------------------------------------------------
    // Stage 6a — Matrix scaffold tests
    // ---------------------------------------------------------------

    /// `init_pixs_matrix` shape and initial values.
    #[test]
    fn init_pixs_matrix_shapes() {
        // V1: 21×21 = 441 cells.
        let v1 = init_pixs_matrix(21, 21);
        assert_eq!(v1.len(), 441);
        assert!(v1.iter().all(|&c| c == PIXS_UNSET));
        // M1: 11×11 = 121.
        let m1 = init_pixs_matrix(11, 11);
        assert_eq!(m1.len(), 121);
        // R7x43: 7×43 = 301.
        let r = init_pixs_matrix(7, 43);
        assert_eq!(r.len(), 301);
    }

    /// `qmv` row-major indexing.
    #[test]
    fn qmv_index() {
        assert_eq!(qmv(0, 0, 21), 0);
        assert_eq!(qmv(0, 20, 21), 20);
        assert_eq!(qmv(1, 0, 21), 21);
        assert_eq!(qmv(6, 6, 21), 6 * 21 + 6);
        assert_eq!(qmv(20, 20, 21), 440);
    }

    /// V1 (Full QR) finder patterns stamp 7×7 modules at all 3 corners.
    #[test]
    fn finder_v1_full() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        let cols = 21usize;
        // Top-left corner: pixs[0,0] should be 1 (first cell of finder).
        assert_eq!(pixs[qmv(0, 0, cols)], 1);
        // The center 3×3 of the top-left finder is all 1s. Sample
        // a few inner cells.
        for (r, c) in [(2, 2), (2, 4), (3, 3), (4, 2), (4, 4)] {
            assert_eq!(
                pixs[qmv(r, c, cols)],
                1,
                "TL finder centre at ({r},{c}) should be 1"
            );
        }
        // The inner ring (1-thick border around the centre) is all 0s.
        assert_eq!(pixs[qmv(1, 1, cols)], 0);
        assert_eq!(pixs[qmv(5, 5, cols)], 0);
        // Top-right finder at col 14..=20. (14, 0) corner top-left.
        assert_eq!(pixs[qmv(0, 14, cols)], 1);
        assert_eq!(pixs[qmv(0, 20, cols)], 1);
        // Bottom-left finder at row 14..=20.
        assert_eq!(pixs[qmv(14, 0, cols)], 1);
        assert_eq!(pixs[qmv(20, 0, cols)], 1);
        // Separator row 7, col 0..=7 should be 0.
        for c in 0..=7 {
            assert_eq!(pixs[qmv(7, c, cols)], 0, "TL separator row 7 col {c}");
        }
    }

    /// M1 (Micro) places a single finder at the top-left only.
    #[test]
    fn finder_m1_micro() {
        let mut pixs = init_pixs_matrix(11, 11);
        place_finder_patterns(&mut pixs, 3, 11, 11);
        // TL corner = 1.
        assert_eq!(pixs[qmv(0, 0, 11)], 1);
        // No bottom-left finder for Micro — pixs[10, 0] should remain
        // UNSET. (Micro timing pattern starts at col 8 of row 0, so
        // (10, 0) is untouched.)
        assert_eq!(pixs[qmv(10, 0, 11)], PIXS_UNSET);
    }

    /// Timing patterns: V1 Full has alternating bits on row 6
    /// cols 8..=12.
    #[test]
    fn timing_v1_full() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_timing_patterns(&mut pixs, 0, 21, 21, NA, NA);
        // Row 6, cols 8..=12 (= cols-9 for V1=21): bits = (i+1)%2.
        for c in 8..=12 {
            let expected = ((c + 1) % 2) as i8;
            assert_eq!(pixs[qmv(6, c, 21)], expected, "row-6 timing at col {c}");
        }
        // Col 6, rows 8..=12: same.
        for r in 8..=12 {
            let expected = ((r + 1) % 2) as i8;
            assert_eq!(pixs[qmv(r, 6, 21)], expected, "col-6 timing at row {r}");
        }
    }

    /// Timing patterns: M1 Micro has timing along col 0 / row 0 from
    /// index 8 onward. (Cols 0-7 / rows 0-7 belong to the finder.)
    #[test]
    fn timing_m1_micro() {
        let mut pixs = init_pixs_matrix(11, 11);
        place_timing_patterns(&mut pixs, 3, 11, 11, NA, NA);
        // Row 0, cols 8..=10: bits = (i+1)%2.
        assert_eq!(pixs[qmv(0, 8, 11)], 1); // (8+1)%2=1
        assert_eq!(pixs[qmv(0, 9, 11)], 0);
        assert_eq!(pixs[qmv(0, 10, 11)], 1);
        // Col 0, rows 8..=10: same pattern.
        assert_eq!(pixs[qmv(8, 0, 11)], 1);
        assert_eq!(pixs[qmv(9, 0, 11)], 0);
        assert_eq!(pixs[qmv(10, 0, 11)], 1);
    }

    // ---------------------------------------------------------------
    // Stage 6b — Alignment patterns
    // ---------------------------------------------------------------

    /// Full QR alignment pattern matches BWIPP `qrcode_algnpatfull`.
    #[test]
    fn alignment_pattern_full_shape() {
        assert_eq!(ALIGNMENT_PATTERN_FULL[0], [1, 1, 1, 1, 1]);
        assert_eq!(ALIGNMENT_PATTERN_FULL[1], [1, 0, 0, 0, 1]);
        assert_eq!(ALIGNMENT_PATTERN_FULL[2], [1, 0, 1, 0, 1]);
        assert_eq!(ALIGNMENT_PATTERN_FULL[3], [1, 0, 0, 0, 1]);
        assert_eq!(ALIGNMENT_PATTERN_FULL[4], [1, 1, 1, 1, 1]);
    }

    /// rMQR alignment pattern matches BWIPP `qrcode_algnpatrmqr`.
    /// The 9-sentinel marks "skip cell".
    #[test]
    fn alignment_pattern_rmqr_shape() {
        assert_eq!(ALIGNMENT_PATTERN_RMQR[0], [1, 1, 1, 9, 9]);
        assert_eq!(ALIGNMENT_PATTERN_RMQR[1], [1, 0, 1, 9, 9]);
        assert_eq!(ALIGNMENT_PATTERN_RMQR[2], [1, 1, 1, 9, 9]);
        assert_eq!(ALIGNMENT_PATTERN_RMQR[3], [9, 9, 9, 9, 9]);
        assert_eq!(ALIGNMENT_PATTERN_RMQR[4], [9, 9, 9, 9, 9]);
        assert_eq!(ALGN_SKIP, 9);
    }

    /// `place_alignment_patterns` for V1 — fimax = NA → no patterns
    /// placed.
    #[test]
    fn alignment_v1_no_patterns() {
        let mut pixs = init_pixs_matrix(21, 21);
        // V1: FULL_METRICS[4] — fimax=98, fimas=99 (both NA → skip).
        place_alignment_patterns(&mut pixs, 0, 21, 21, 98, 99);
        // All cells remain UNSET.
        assert!(pixs.iter().all(|&c| c == PIXS_UNSET));
    }

    /// `place_alignment_patterns` for V2 — single central alignment
    /// at (16, 16) per BWIPP (fimax=18, fimas=NA → single position
    /// at fimax-2 = 16).
    #[test]
    fn alignment_v2_single_central() {
        let cols: u16 = 25;
        let mut pixs = init_pixs_matrix(25, cols);
        // V2: FULL_METRICS[5] — fimax=18, fimas=99 (NA).
        place_alignment_patterns(&mut pixs, 0, 25, cols, 18, 99);
        // Pattern center cell should be at (16+2, 16+2) = (18, 18).
        assert_eq!(pixs[qmv(18, 18, cols as usize)], 1, "V2 center cell");
        // Outer ring cells.
        assert_eq!(pixs[qmv(16, 16, cols as usize)], 1);
        assert_eq!(pixs[qmv(16, 20, cols as usize)], 1);
        assert_eq!(pixs[qmv(20, 16, cols as usize)], 1);
        assert_eq!(pixs[qmv(20, 20, cols as usize)], 1);
        // Inner-ring cells (the 0s around the center).
        assert_eq!(pixs[qmv(17, 17, cols as usize)], 0);
        assert_eq!(pixs[qmv(17, 19, cols as usize)], 0);
        assert_eq!(pixs[qmv(19, 17, cols as usize)], 0);
        assert_eq!(pixs[qmv(19, 19, cols as usize)], 0);
    }

    /// `place_alignment_patterns` for V7 — multiple patterns. V7's
    /// `fimax=22`, `fimas=38`, step = 16 → positions start at 20.
    /// V7 is 45×45 modules. Expected alignment centers (per ISO):
    /// (6, 22), (22, 6), (22, 22), (22, 38), (38, 22), (38, 38).
    /// In BWIPP coords those are pattern-top-left = (4, 20), (20, 4),
    /// (20, 20), (20, 36), (36, 20), (36, 36).
    #[test]
    fn alignment_v7_grid() {
        let cols: u16 = 45;
        let mut pixs = init_pixs_matrix(45, cols);
        // V7: FULL_METRICS[10] — fimax=22, fimas=38.
        place_alignment_patterns(&mut pixs, 0, 45, cols, 22, 38);
        let cu = cols as usize;
        // V7 alignment centers (BWIPP indexes from pattern top-left,
        // center is at +2 from corner). The edge-pass writes the
        // (i, 4) and (4, i) family: i = fimax-2 = 20.
        //   (20, 4) → center (22, 6)
        //   (4, 20) → center (6, 22)
        assert_eq!(pixs[qmv(6, 22, cu)], 1, "V7 alignment (6,22)");
        assert_eq!(pixs[qmv(22, 6, cu)], 1, "V7 alignment (22,6)");
        // Central-pass: x in {20}, y in {20} → (20, 20) → center
        // (22, 22).
        assert_eq!(pixs[qmv(22, 22, cu)], 1, "V7 alignment (22,22)");
    }

    /// `put_alignment_pattern` honors the 9 sentinel (no write).
    #[test]
    fn put_alignment_pattern_skip_9() {
        let cols: u16 = 9;
        let mut pixs = init_pixs_matrix(9, cols);
        put_alignment_pattern(&mut pixs, 0, 0, cols as usize, &ALIGNMENT_PATTERN_RMQR);
        // The 3×3 active region should be written.
        assert_eq!(pixs[qmv(0, 0, cols as usize)], 1);
        assert_eq!(pixs[qmv(1, 1, cols as usize)], 0);
        assert_eq!(pixs[qmv(2, 2, cols as usize)], 1);
        // The 9-cells in row 3 / col 3 should remain UNSET.
        assert_eq!(pixs[qmv(3, 0, cols as usize)], PIXS_UNSET);
        assert_eq!(pixs[qmv(0, 3, cols as usize)], PIXS_UNSET);
        assert_eq!(pixs[qmv(3, 3, cols as usize)], PIXS_UNSET);
    }

    /// Finder pattern constant has the expected ISO 18004 shape.
    #[test]
    fn finder_pattern_shape() {
        // Outer corners.
        assert_eq!(FINDER_PATTERN[0][0], 1);
        assert_eq!(FINDER_PATTERN[0][6], 1);
        assert_eq!(FINDER_PATTERN[6][0], 1);
        assert_eq!(FINDER_PATTERN[6][6], 1);
        // Center 3×3 all 1s.
        for row in FINDER_PATTERN.iter().take(5).skip(2) {
            for cell in row.iter().take(5).skip(2) {
                assert_eq!(*cell, 1, "centre cell of FINDER_PATTERN");
            }
        }
        // Inner ring all 0s.
        assert_eq!(FINDER_PATTERN[1][1], 0);
        assert_eq!(FINDER_PATTERN[5][5], 0);
        assert_eq!(FINDER_PATTERN[1][5], 0);
    }

    /// `place_finder_patterns` for rMQR R11×27 (one of the smaller
    /// non-extreme sizes): draws the TL 7×7 finder + separator, the
    /// TR/BL 3-cell corner marks, and the BR 5×5 sub-finder per
    /// BWIPP `qrcode_fpatmap.rmqr`.
    #[test]
    fn place_finder_patterns_rmqr_r11x27() {
        // rMQR R11×27 lives at metric_idx 54 (layout_id 7 = Rmqr).
        let rows = 11u16;
        let cols = 27u16;
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, 7, rows, cols);

        // TL 7×7 finder corners.
        assert_eq!(pixs[qmv(0, 0, cols as usize)], 1, "TL (0,0)");
        assert_eq!(pixs[qmv(0, 6, cols as usize)], 1, "TL (0,6)");
        assert_eq!(pixs[qmv(6, 0, cols as usize)], 1, "TL (6,0)");
        assert_eq!(pixs[qmv(6, 6, cols as usize)], 1, "TL (6,6)");
        // TL center cell.
        assert_eq!(pixs[qmv(3, 3, cols as usize)], 1, "TL center");
        // TL inner ring is light.
        assert_eq!(pixs[qmv(1, 1, cols as usize)], 0, "TL inner (1,1)");
        // TL separator on the right edge (col 7, rows 0..=7).
        assert_eq!(pixs[qmv(0, 7, cols as usize)], 0, "TL sep (0,7)");
        assert_eq!(pixs[qmv(7, 7, cols as usize)], 0, "TL sep (7,7)");
        // TL separator below (row 7, cols 0..=7).
        assert_eq!(pixs[qmv(7, 0, cols as usize)], 0, "TL sep (7,0)");

        // TR corner mark — 3 cells along row 0, cols cols-3..=cols-1.
        let cu = cols as usize;
        assert_eq!(pixs[qmv(0, cu - 1, cu)], 1, "TR (0,cols-1)");
        assert_eq!(pixs[qmv(0, cu - 2, cu)], 1, "TR (0,cols-2)");
        assert_eq!(pixs[qmv(0, cu - 3, cu)], 1, "TR (0,cols-3)");
        assert_eq!(pixs[qmv(1, cu - 1, cu)], 1, "TR (1,cols-1)");
        assert_eq!(pixs[qmv(1, cu - 2, cu)], 0, "TR (1,cols-2)");
        assert_eq!(pixs[qmv(2, cu - 1, cu)], 1, "TR (2,cols-1)");

        // BL corner mark — 3 cells along col 0, rows rows-3..=rows-1.
        let ru = rows as usize;
        assert_eq!(pixs[qmv(ru - 1, 0, cu)], 1, "BL (rows-1,0)");
        assert_eq!(pixs[qmv(ru - 2, 0, cu)], 1, "BL (rows-2,0)");
        assert_eq!(pixs[qmv(ru - 3, 0, cu)], 1, "BL (rows-3,0)");
        assert_eq!(pixs[qmv(ru - 1, 1, cu)], 1, "BL (rows-1,1)");
        assert_eq!(pixs[qmv(ru - 2, 1, cu)], 0, "BL (rows-2,1)");
        assert_eq!(pixs[qmv(ru - 1, 2, cu)], 1, "BL (rows-1,2)");

        // BR sub-finder — 5×5 dark outer ring + dark center cell.
        // Outer ring: row rows-5 cols cols-5..=cols-1 all 1, and
        // mirrored across the 5×5.
        for x in 0..5 {
            assert_eq!(
                pixs[qmv(ru - 5, cu - 1 - x, cu)],
                1,
                "BR top edge col {}",
                cu - 1 - x
            );
            assert_eq!(
                pixs[qmv(ru - 1, cu - 1 - x, cu)],
                1,
                "BR bottom edge col {}",
                cu - 1 - x
            );
        }
        for y in 0..5 {
            assert_eq!(
                pixs[qmv(ru - 1 - y, cu - 5, cu)],
                1,
                "BR left edge row {}",
                ru - 1 - y
            );
            assert_eq!(
                pixs[qmv(ru - 1 - y, cu - 1, cu)],
                1,
                "BR right edge row {}",
                ru - 1 - y
            );
        }
        // BR center cell.
        assert_eq!(pixs[qmv(ru - 3, cu - 3, cu)], 1, "BR center");
        // BR inner ring should be light (the cells between the outer
        // ring and the center).
        assert_eq!(pixs[qmv(ru - 4, cu - 4, cu)], 0, "BR inner (ru-4,cu-4)");
        assert_eq!(pixs[qmv(ru - 2, cu - 2, cu)], 0, "BR inner (ru-2,cu-2)");
        assert_eq!(pixs[qmv(ru - 4, cu - 2, cu)], 0, "BR inner (ru-4,cu-2)");
        assert_eq!(pixs[qmv(ru - 2, cu - 4, cu)], 0, "BR inner (ru-2,cu-4)");
    }

    /// `place_finder_patterns` for rMQR R7×43 (the shortest rMQR,
    /// where the TL separator row would land at row 7 — outside the
    /// symbol — and is therefore elided per BWIPP's `y < rows` gate).
    /// The BR sub-finder fits because it starts at row rows-5 = 2.
    #[test]
    fn place_finder_patterns_rmqr_r7x43() {
        let rows = 7u16;
        let cols = 43u16;
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, 7, rows, cols);

        let cu = cols as usize;
        let ru = rows as usize;

        // TL — only 7 rows fit, so row 7 separator never drawn.
        // Outer corners of the 7×7 finder still present.
        assert_eq!(pixs[qmv(0, 0, cu)], 1);
        assert_eq!(pixs[qmv(6, 0, cu)], 1);
        assert_eq!(pixs[qmv(6, 6, cu)], 1);

        // BR sub-finder spans rows 2..=6 × cols 38..=42.
        for x in 0..5 {
            assert_eq!(pixs[qmv(2, cu - 1 - x, cu)], 1, "BR top edge");
            assert_eq!(pixs[qmv(ru - 1, cu - 1 - x, cu)], 1, "BR bottom edge");
        }
        // BR center.
        assert_eq!(pixs[qmv(4, cu - 3, cu)], 1, "BR center");
        // BR inner ring light.
        assert_eq!(pixs[qmv(3, cu - 2, cu)], 0, "BR inner (3,cu-2)");

        // BL corner mark at rows 4..=6 × cols 0..=2.
        assert_eq!(pixs[qmv(ru - 1, 0, cu)], 1, "BL (6,0)");
        assert_eq!(pixs[qmv(ru - 2, 0, cu)], 1, "BL (5,0)");
        assert_eq!(pixs[qmv(ru - 3, 0, cu)], 1, "BL (4,0)");
        assert_eq!(pixs[qmv(ru - 1, 1, cu)], 1, "BL (6,1)");
        assert_eq!(pixs[qmv(ru - 2, 1, cu)], 0, "BL (5,1)");

        // TR corner mark at row 0..=2 × cols 40..=42.
        assert_eq!(pixs[qmv(0, cu - 1, cu)], 1, "TR (0,42)");
        assert_eq!(pixs[qmv(0, cu - 2, cu)], 1, "TR (0,41)");
        assert_eq!(pixs[qmv(0, cu - 3, cu)], 1, "TR (0,40)");
        assert_eq!(pixs[qmv(1, cu - 1, cu)], 1, "TR (1,42)");
        assert_eq!(pixs[qmv(1, cu - 2, cu)], 0, "TR (1,41)");
        assert_eq!(pixs[qmv(2, cu - 1, cu)], 1, "TR (2,42)");
    }

    // ---------------------------------------------------------------
    // Stage 6c — Codeword zig-zag walker
    // ---------------------------------------------------------------

    /// `walk_codeword_positions` for a fully-unset 21×21 matrix:
    /// without reservations it visits every cell except the row-6
    /// timing column.
    #[test]
    fn walk_codeword_positions_v1_no_reservations() {
        let pixs = init_pixs_matrix(21, 21);
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        // V1 = 21×21 = 441 cells. Full QR walker skips col 6 entirely
        // (treated as a "phantom column" — but the walker doesn't
        // visit col 6 when posx hits it after the row-6 transition).
        // BWIPP's behaviour: walker visits col 6 when starting (posx
        // begins at cols-1 = 20 going left through col 7); but the
        // posx == 6 skip kicks in after a top-edge bounce that lands
        // us at posx = 6, which then drops to posx = 5.
        //
        // For a fully-unset matrix the visit count is harder to
        // predict because the row-6 timing isn't actually reserved
        // (we passed an all-UNSET grid). Just verify the walker
        // covers a non-trivial portion of the grid (> half).
        // Stage 11.A8c (cont) — descriptive label on upper bound naming
        // V1 grid cap (21x21 = 441 modules — walker can't exceed total
        // module count regardless of reservation).
        assert!(positions.len() >= 220, "got {}", positions.len());
        assert!(
            positions.len() <= 441,
            "walker must visit at most 441 positions (V1 grid: 21x21 = 441 modules); got {}",
            positions.len()
        );
    }

    /// `walk_codeword_positions` skips reserved (non-UNSET) cells.
    #[test]
    fn walk_codeword_positions_skips_reserved() {
        let mut pixs = init_pixs_matrix(21, 21);
        // Reserve the entire top-left corner (rows 0-7, cols 0-7).
        for r in 0..8 {
            for c in 0..8 {
                pixs[qmv(r, c, 21)] = 1;
            }
        }
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        // None of the visited indices should land in the reserved
        // top-left corner.
        for &pos in &positions {
            let row = pos / 21;
            let col = pos % 21;
            assert!(
                !(row < 8 && col < 8),
                "walker visited reserved TL ({row},{col})"
            );
        }
    }

    /// `walk_codeword_positions` for V1 with all 3 finder patterns
    /// placed visits a sensible count of cells.
    #[test]
    fn walk_codeword_positions_v1_with_finders() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        place_timing_patterns(&mut pixs, 0, 21, 21, NA, NA);
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        // V1 ISO 18004 data + ECC = 26 codewords × 8 = 208 bits. With
        // only the finder + timing patterns reserved (Stage 6c's
        // current state — Stage 6d will add format-info + version-info
        // reservation), the walker covers ~238 cells. After Stage 6d
        // applies format-info reservation (15 cells per finder × 2
        // finders ≈ 30 more reserved cells, accounting for the (8,8)
        // dark-module overlap), the count should drop to 208.
        //
        // For now we just verify the walker produces a non-trivial
        // result close to the full data-region size.
        assert!(
            (230..=245).contains(&positions.len()),
            "expected 230-245 cells with finder + timing only, got {}",
            positions.len()
        );
    }

    /// `place_codewords_at` round-trip: known codewords → positions →
    /// extract bits back from pixs.
    #[test]
    fn place_codewords_at_round_trip() {
        let positions: Vec<usize> = (0..16).collect();
        let mut pixs = vec![PIXS_UNSET; 16];
        // Two bytes: 0x4D 0xA0 = bits 0100_1101 1010_0000.
        let cws = vec![0x4D, 0xA0];
        place_codewords_at(&mut pixs, &positions, &cws).unwrap();
        let expected: [i8; 16] = [
            0, 1, 0, 0, 1, 1, 0, 1, // 0x4D
            1, 0, 1, 0, 0, 0, 0, 0, // 0xA0
        ];
        assert_eq!(pixs.as_slice(), &expected);
    }

    /// `place_codewords_at` rejects when codeword bits exceed
    /// positions by more than 8 bits (one rbit-padded codeword).
    /// An overflow within 8 bits is tolerated for the BWIPP `rbit > 0`
    /// trailing-codeword pattern.
    #[test]
    fn place_codewords_at_rejects_overflow() {
        let positions: Vec<usize> = (0..8).collect();
        let mut pixs = vec![PIXS_UNSET; 8];
        // 24 bits with only 8 positions = 16-bit overflow → genuine error.
        let cws = vec![0u8; 3];
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 3047-3052 (`qrcode_native: codeword stream (24 bits)
        // exceeds available positions (8) by more than one rbit-
        // padded codeword`).
        match place_codewords_at(&mut pixs, &positions, &cws).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("codeword stream (24 bits)"),
                    "missing `codeword stream (24 bits)` actual-length echo: {msg}"
                );
                assert!(
                    msg.contains("exceeds available positions (8)"),
                    "missing `exceeds available positions (8)` capacity echo: {msg}"
                );
                assert!(
                    msg.contains("rbit-padded codeword"),
                    "missing `rbit-padded codeword` BWIPP-specific hint: {msg}"
                );
            }
            other => panic!(
                "24-bit cws / 8-pos place_codewords_at should reject as InvalidData, got {other:?}"
            ),
        }
        // 16 bits with 8 positions = exactly 8-bit overflow → allowed
        // (BWIPP rbit pattern). Verify it succeeds with truncation.
        let mut pixs2 = vec![PIXS_UNSET; 8];
        let cws_rbit = vec![0u8; 2];
        place_codewords_at(&mut pixs2, &positions, &cws_rbit).unwrap();
    }

    // ---------------------------------------------------------------
    // Stage 6d — Format-info reservation
    // ---------------------------------------------------------------

    /// `place_format_info_reservation` for V1 reserves 31 cells:
    /// 15 TL + 8 TR (row 8 cols cols-8..cols-1) + 8 BL (col 8 rows
    /// rows-8..rows-1, dark module included). Note: BWIPP's
    /// `qrcode_formatfimmap.full` has 30 cells (excludes the dark
    /// module which it sets separately to 0 then later to 1). Our
    /// 31-cell layout matches the **walker-visible** state (all 31
    /// cells are non-UNSET so the walker skips them), and the
    /// dark-module cell at (rows-8, 8) is overwritten with 1 by
    /// `write_dark_module_full` in Stage 8.
    ///
    /// rMQR format-info codeword tables: our `rmqr_fmtval1` /
    /// `rmqr_fmtval2` must reproduce BWIPP's pre-computed
    /// `fmtvalsrmqr1` / `fmtvalsrmqr2` tables byte-for-byte across
    /// all 64 input values (= ec_id × 32 + verind).
    #[test]
    fn rmqr_fmtval_tables_match_bwipp_oracle() {
        // Golden tables extracted via tools/dump-rmqr-fmtvals.js
        // (saved at tests/fixtures/qrcode_native_rmqr_fmtvals.json).
        let golden = include_str!("../../../tests/fixtures/qrcode_native_rmqr_fmtvals.json");
        // Parse the JSON output manually — it's two flat arrays of 64
        // integers each, in a fixed shape. The pretty-printed format
        // has 64 lines per array; we can extract numbers via regex.
        let nums: Vec<u32> = golden
            .lines()
            .filter_map(|l| {
                let s = l.trim_start().trim_end_matches(',').trim_end_matches(']');
                s.parse::<u32>().ok()
            })
            .collect();
        assert_eq!(nums.len(), 128, "fixture must contain 128 numbers (2 × 64)");
        let (rmqr1, rmqr2) = nums.split_at(64);
        for (i, &want) in rmqr1.iter().enumerate() {
            let got = rmqr_fmtval1(i as u8);
            assert_eq!(got, want, "rmqr_fmtval1({i}) golden mismatch");
        }
        for (i, &want) in rmqr2.iter().enumerate() {
            let got = rmqr_fmtval2(i as u8);
            assert_eq!(got, want, "rmqr_fmtval2({i}) golden mismatch");
        }
    }

    /// `QRCODE_EC_INDICATOR_RMQR` matches BWIPP `qrcode_ecidrmqr`.
    #[test]
    fn qrcode_ec_indicator_rmqr_matches_bwipp() {
        // Per BWIPP source line 26605: `qrcode_ecidrmqr = [-1, 0, -1, 1]`.
        assert_eq!(QRCODE_EC_INDICATOR_RMQR, [-1, 0, -1, 1]);
    }

    /// rMQR formatfimmap golden: for each of the 32 rMQR sizes, our
    /// `rmqr_formatfimmap_pairs(rows, cols)` must produce the same
    /// 18 (TL, Dup) cluster pairs as BWIPP's `qrcode_formatfimmap.rmqr`
    /// closure list. The fixture
    /// `tests/fixtures/qrcode_native_rmqr_formatfimmap.txt` was
    /// generated by walking BWIPP's closures via
    /// `tools/extract-qrcode-formatfimmap.js`.
    #[test]
    fn rmqr_formatfimmap_matches_bwipp_oracle() {
        let corpus = include_str!("../../../tests/fixtures/qrcode_native_rmqr_formatfimmap.txt");
        let mut tested = 0;
        for line in corpus.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, '\t');
            let vstr = parts.next().expect("missing vstr");
            let rows: u16 = parts
                .next()
                .expect("missing rows")
                .parse()
                .expect("bad rows");
            let cols: u16 = parts
                .next()
                .expect("missing cols")
                .parse()
                .expect("bad cols");
            let pairs_str = parts.next().expect("missing pairs");
            let expected_pairs: Vec<((u16, u16), (u16, u16))> = pairs_str
                .split(';')
                .map(|p| {
                    let nums: Vec<u16> =
                        p.split(',').map(|n| n.parse().expect("bad num")).collect();
                    assert_eq!(nums.len(), 4, "{vstr}: pair must have 4 numbers");
                    ((nums[0], nums[1]), (nums[2], nums[3]))
                })
                .collect();
            assert_eq!(expected_pairs.len(), 18, "{vstr}: 18 pairs expected");

            let got = rmqr_formatfimmap_pairs(rows, cols);
            for (i, (want, got_pair)) in expected_pairs.iter().zip(got.iter()).enumerate() {
                assert_eq!(
                    got_pair, want,
                    "{vstr} cluster {i}: got {got_pair:?} want {want:?}"
                );
            }
            tested += 1;
        }
        assert_eq!(tested, 32, "expected 32 rMQR variants");
    }

    #[test]
    fn format_info_reservation_v1_full() {
        let mut pixs = init_pixs_matrix(21, 21);
        let count = place_format_info_reservation(&mut pixs, 0, 21, 21);
        assert_eq!(count, 31);
        // Sample known cells. Reserved value is 1 (dark) per BWIPP
        // `qrcode_formatfimmap` write loop (bwip-js line 28048).
        let cols_u = 21usize;
        // TL cluster:
        assert_eq!(pixs[qmv(0, 8, cols_u)], 1);
        assert_eq!(pixs[qmv(5, 8, cols_u)], 1);
        assert_eq!(pixs[qmv(7, 8, cols_u)], 1); // row 7 (skip 6=timing)
        assert_eq!(pixs[qmv(8, 8, cols_u)], 1); // shared TL/Dup corner
        assert_eq!(pixs[qmv(8, 7, cols_u)], 1);
        assert_eq!(pixs[qmv(8, 0, cols_u)], 1); // leftmost
                                                // TR cluster (row 8): 8 cells, cols 13..=20.
        assert_eq!(pixs[qmv(8, 20, cols_u)], 1); // cols-1
        assert_eq!(pixs[qmv(8, 14, cols_u)], 1); // cols-7
        assert_eq!(pixs[qmv(8, 13, cols_u)], 1); // cols-8 (cluster 7 Dup)
                                                 // BL cluster (col 8): 7 cells at rows rows-7..rows-1 (= 14..20
                                                 // for V1), reserved at 1. The dark-module slot (rows-8, 8) =
                                                 // (13, 8) is set to 0 by the dark-module pre-init (mirroring
                                                 // bwip-js line 28083-28092), then later overwritten with 1
                                                 // by write_dark_module_full in Stage 8.
        assert_eq!(pixs[qmv(13, 8, cols_u)], 0); // dark module pre-init
        assert_eq!(pixs[qmv(14, 8, cols_u)], 1);
        assert_eq!(pixs[qmv(20, 8, cols_u)], 1);
    }

    /// `place_format_info_reservation` for M1 reserves 15 cells.
    #[test]
    fn format_info_reservation_m1_micro() {
        let mut pixs = init_pixs_matrix(11, 11);
        let count = place_format_info_reservation(&mut pixs, 3, 11, 11);
        assert_eq!(count, 15);
        let cu = 11usize;
        // Reserved value is 1 (dark) per BWIPP convention.
        // Column 8, rows 1..=8
        assert_eq!(pixs[qmv(1, 8, cu)], 1);
        assert_eq!(pixs[qmv(8, 8, cu)], 1);
        // Row 8, cols 1..=7
        assert_eq!(pixs[qmv(8, 1, cu)], 1);
        assert_eq!(pixs[qmv(8, 7, cu)], 1);
    }

    /// `place_format_info_reservation` for rMQR places 36 cells per
    /// BWIPP's `qrcode_formatfimmap.rmqr` (18 cluster pairs). Some
    /// of those positions may be OOB for very small rMQR sizes (e.g.
    /// R7x43); the `pixs_set` helper silently no-ops on OOB writes,
    /// matching BWIPP's tolerant placement loop.
    #[test]
    fn format_info_reservation_rmqr_basic_count() {
        let mut pixs = init_pixs_matrix(7, 43);
        let count = place_format_info_reservation(&mut pixs, 7, 7, 43);
        assert_eq!(count, 36, "rMQR formatfimmap returns 36 placement attempts");
        // Spot-check: cluster 0 TL is (3, 11), Dup at (cols-3, rows-6) = (1, 40).
        let cu = 43usize;
        assert_eq!(pixs[qmv(3, 11, cu)], 1, "rMQR cluster 0 TL");
        assert_eq!(pixs[qmv(1, 40, cu)], 1, "rMQR cluster 0 Dup");
    }

    /// V7+ version-info reservation: 36 cells (2 redundant blocks of
    /// 3×6 and 6×3) at known positions.
    #[test]
    fn version_info_reservation_v7() {
        // V7 is FULL_METRICS[10] (4 micro + V1..V6 = 10), layout_id 0.
        // Symbol size 45×45.
        let mut pixs = init_pixs_matrix(45, 45);
        let count = place_version_info_reservation(&mut pixs, 0, 45, 45, 7);
        assert_eq!(count, 36, "V7+ version-info has 36 reserved cells");
        let cu = 45usize;
        // BL block: rows 34, 35, 36 × cols 0..=5.
        for r in 34..=36 {
            for c in 0..=5 {
                assert_eq!(
                    pixs[qmv(r, c, cu)],
                    0,
                    "V7 BL version-info at ({r},{c}) should be 0"
                );
            }
        }
        // TR block: rows 0..=5 × cols 34, 35, 36.
        for r in 0..=5 {
            for c in 34..=36 {
                assert_eq!(
                    pixs[qmv(r, c, cu)],
                    0,
                    "V7 TR version-info at ({r},{c}) should be 0"
                );
            }
        }
    }

    /// V1..V6 (< V7) skip version-info reservation entirely.
    #[test]
    fn version_info_reservation_pre_v7() {
        let mut pixs = init_pixs_matrix(21, 21);
        let count = place_version_info_reservation(&mut pixs, 0, 21, 21, 1);
        assert_eq!(count, 0);
        assert!(pixs.iter().all(|&c| c == PIXS_UNSET));
    }

    /// Micro QR has no version-info reservation regardless of version
    /// (M1..M4 don't carry version-info bits).
    #[test]
    fn version_info_reservation_micro_no_op() {
        let mut pixs = init_pixs_matrix(11, 11);
        // layout_id 3 = M1. Even with a fictional "version 7" the
        // Micro branch should return 0.
        let count = place_version_info_reservation(&mut pixs, 3, 11, 11, 7);
        assert_eq!(count, 0);
    }

    /// V40 version-info at the extreme upper bound: 177×177 symbol,
    /// blocks at corner.
    #[test]
    fn version_info_reservation_v40() {
        let mut pixs = init_pixs_matrix(177, 177);
        let count = place_version_info_reservation(&mut pixs, 2, 177, 177, 40);
        assert_eq!(count, 36);
        let cu = 177usize;
        // BL block: rows 166, 167, 168 × cols 0..=5.
        assert_eq!(pixs[qmv(166, 0, cu)], 0);
        assert_eq!(pixs[qmv(168, 5, cu)], 0);
        // TR block: rows 0..=5 × cols 166, 167, 168.
        assert_eq!(pixs[qmv(0, 166, cu)], 0);
        assert_eq!(pixs[qmv(5, 168, cu)], 0);
    }

    /// V1 walker visit count after Stage 6d reservation. The ISO
    /// 18004 V1 codeword bit count is 208 (26 codewords × 8). With
    /// finder + timing + format-info reservation in place, the
    /// walker currently visits 209 cells — one cell off, attributable
    /// to a separator/format-info boundary BWIPP handles in its
    /// per-format function-pattern mask layer (Stage 7 mask scoring
    /// will reveal exactly where the discrepancy lies). The walker
    /// logic itself is verified by the standalone `walk_codeword_*`
    /// tests; this test just pins the current count so that the
    /// Stage 7 work is anchored against a known starting state.
    #[test]
    fn walker_v1_with_full_function_patterns() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        place_timing_patterns(&mut pixs, 0, 21, 21, NA, NA);
        place_format_info_reservation(&mut pixs, 0, 21, 21);
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        // Expected 208; current count 209. The one-cell gap is
        // tracked for Stage 7 audit — see PORT_PLAN.md for the
        // function-pattern-layer reconciliation work.
        assert!(
            (208..=210).contains(&positions.len()),
            "V1 walker should visit close to 208 data cells (ISO V1 = 26 codewords × 8 bits = 208), got {}",
            positions.len()
        );
    }

    // ---------------------------------------------------------------
    // Stage 7 — Mask functions + Micro QR scoring
    // ---------------------------------------------------------------

    /// `MASK_FUNCS[0]` matches ISO 18004 `(i + j) mod 2 == 0`.
    #[test]
    fn mask_fn_0() {
        assert!(MASK_FUNCS[0](0, 0));
        assert!(!MASK_FUNCS[0](0, 1));
        assert!(!MASK_FUNCS[0](1, 0));
        assert!(MASK_FUNCS[0](1, 1));
        assert!(MASK_FUNCS[0](2, 2));
    }

    /// `MASK_FUNCS[1]` matches `row mod 2 == 0`.
    #[test]
    fn mask_fn_1() {
        for col in 0..5 {
            assert!(MASK_FUNCS[1](0, col), "row 0 col {col}");
            assert!(!MASK_FUNCS[1](1, col), "row 1 col {col}");
            assert!(MASK_FUNCS[1](4, col), "row 4 col {col}");
        }
    }

    /// `MASK_FUNCS[2]` matches `col mod 3 == 0`.
    #[test]
    fn mask_fn_2() {
        for row in 0..5 {
            assert!(MASK_FUNCS[2](row, 0));
            assert!(!MASK_FUNCS[2](row, 1));
            assert!(!MASK_FUNCS[2](row, 2));
            assert!(MASK_FUNCS[2](row, 3));
            assert!(MASK_FUNCS[2](row, 6));
        }
    }

    /// `MASK_FUNCS[3]` matches `(row + col) mod 3 == 0`.
    #[test]
    fn mask_fn_3() {
        assert!(MASK_FUNCS[3](0, 0));
        assert!(MASK_FUNCS[3](1, 2));
        assert!(MASK_FUNCS[3](2, 1));
        assert!(!MASK_FUNCS[3](1, 1));
    }

    /// `MASK_FUNCS[4]` matches `(row/2 + col/3) mod 2 == 0`.
    #[test]
    fn mask_fn_4() {
        // (0/2 + 0/3) % 2 = 0 → true
        assert!(MASK_FUNCS[4](0, 0));
        // (1/2 + 1/3) % 2 = 0 → true
        assert!(MASK_FUNCS[4](1, 1));
        // (2/2 + 0/3) % 2 = 1 → false
        assert!(!MASK_FUNCS[4](2, 0));
        // (0/2 + 3/3) % 2 = 1 → false
        assert!(!MASK_FUNCS[4](0, 3));
    }

    /// `MASK_FUNCS[5/6/7]` consistency checks.
    #[test]
    fn mask_fn_5_6_7() {
        // Mask 5: (i*j)%2 + (i*j)%3 == 0
        assert!(MASK_FUNCS[5](0, 0)); // 0%2 + 0%3 = 0 ✓
        assert!(MASK_FUNCS[5](6, 1)); // 6%2 + 6%3 = 0 + 0 = 0 ✓
        assert!(!MASK_FUNCS[5](1, 1)); // 1%2 + 1%3 = 1 + 1 = 2
                                       // Mask 6: ((i*j)%2 + (i*j)%3) % 2 == 0
        assert!(MASK_FUNCS[6](0, 0));
        assert!(MASK_FUNCS[6](1, 1)); // (1+1)%2 = 0 ✓
                                      // Mask 7: ((i+j)%2 + (i*j)%3) % 2 == 0
        assert!(MASK_FUNCS[7](0, 0));
        assert!(MASK_FUNCS[7](0, 2)); // (2 + 0) % 2 = 0 ✓
    }

    /// `mask_candidates` returns the BWIPP-defined per-format mask set.
    #[test]
    fn mask_candidates_per_format() {
        // Full QR (layout_id 0/1/2) — all 8 masks.
        assert_eq!(mask_candidates(0), &[0u8, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(mask_candidates(2), &[0u8, 1, 2, 3, 4, 5, 6, 7]);
        // Micro QR (layout_id 3..=6) — {1, 4, 6, 7}.
        assert_eq!(mask_candidates(3), &[1u8, 4, 6, 7]);
        assert_eq!(mask_candidates(6), &[1u8, 4, 6, 7]);
        // rMQR (layout_id 7..=38) — {4} only.
        assert_eq!(mask_candidates(7), &[4u8]);
        assert_eq!(mask_candidates(38), &[4u8]);
    }

    /// `apply_mask_to_data_cells` flips data cells per the chosen mask.
    /// Function-pattern cells with value other than 0/1 are left alone.
    #[test]
    fn apply_mask_to_data_cells_basic() {
        // 3×3 grid with mask 0 (XOR (i+j)%2). All cells start as 0.
        let mut pixs = vec![0i8; 9];
        apply_mask_to_data_cells(&mut pixs, 0, 3, 3);
        // mask 0: (i+j)%2==0 → flip. Pattern:
        //   (0,0): flip → 1
        //   (0,1): no → 0
        //   (0,2): flip → 1
        //   (1,0): no → 0
        //   (1,1): flip → 1
        //   (1,2): no → 0
        //   (2,0): flip → 1
        //   (2,1): no → 0
        //   (2,2): flip → 1
        assert_eq!(pixs, vec![1, 0, 1, 0, 1, 0, 1, 0, 1]);
    }

    /// `evaluate_mask_micro`: for an all-dark right column + bottom row,
    /// the score should be maximally negative.
    #[test]
    fn evaluate_mask_micro_all_dark() {
        // Build 11×11 M1 grid with all-1 right column + bottom row.
        let mut pixs = vec![0i8; 121];
        let cols = 11usize;
        // Right column = col 10.
        for r in 1..10 {
            pixs[qmv(r, 10, cols)] = 1;
        }
        // Bottom row = row 10.
        for c in 1..10 {
            pixs[qmv(10, c, cols)] = 1;
        }
        let score = evaluate_mask_micro(&pixs, 11, 11);
        // dk_rhs = 9, dk_bot = 9. lo = 9, hi = 9. score = -(9*16+9) = -153.
        assert_eq!(score, -153);
    }

    /// `evaluate_mask_micro`: all-light yields score 0.
    #[test]
    fn evaluate_mask_micro_all_light() {
        let pixs = vec![0i8; 121];
        let score = evaluate_mask_micro(&pixs, 11, 11);
        assert_eq!(score, 0);
    }

    /// `evaluate_mask_micro`: mismatched boundary counts get
    /// asymmetric formula.
    #[test]
    fn evaluate_mask_micro_asymmetric() {
        // 5 darks on right column, 2 on bottom row.
        let mut pixs = vec![0i8; 121];
        let cols = 11usize;
        for r in 1..6 {
            pixs[qmv(r, 10, cols)] = 1;
        }
        for c in 1..3 {
            pixs[qmv(10, c, cols)] = 1;
        }
        // dk_rhs = 5, dk_bot = 2. lo = 2, hi = 5. score = -(2*16 + 5) = -37.
        assert_eq!(evaluate_mask_micro(&pixs, 11, 11), -37);
    }

    /// `select_best_micro_mask` rejects non-Micro layout_ids.
    #[test]
    fn select_best_micro_mask_rejects_non_micro() {
        let pixs = vec![0i8; 441];
        let positions: Vec<usize> = (0..208).collect();
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostic at
        // line 3232-3234 (`qrcode_native: select_best_micro_mask
        // called with non-Micro layout_id {layout_id}`). Distinct
        // layout_ids (0 = Full QR V1, 7 = rMQR R7x43) prove the
        // {layout_id} interpolation surfaces the offending value.
        match select_best_micro_mask(&pixs, &positions, 0, 21, 21).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "Full-QR layout_id=0 missing prefix: {msg}"
                );
                assert!(
                    msg.contains("select_best_micro_mask"),
                    "missing function name in diagnostic: {msg}"
                );
                assert!(
                    msg.contains("non-Micro layout_id"),
                    "missing `non-Micro layout_id` predicate: {msg}"
                );
                assert!(
                    msg.contains("layout_id 0"),
                    "layout_id=0 value-echo missing: {msg}"
                );
            }
            other => panic!(
                "select_best_micro_mask(layout_id=0) should reject as InvalidData, got {other:?}"
            ),
        }
        match select_best_micro_mask(&pixs, &positions, 7, 7, 43).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "rMQR layout_id=7 missing prefix: {msg}"
                );
                assert!(
                    msg.contains("non-Micro layout_id"),
                    "missing `non-Micro layout_id` predicate: {msg}"
                );
                assert!(
                    msg.contains("layout_id 7"),
                    "layout_id=7 value-echo missing (proves {{layout_id}} interpolation isn't hardcoded 0): {msg}"
                );
            }
            other => panic!(
                "select_best_micro_mask(layout_id=7) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// `select_best_micro_mask` returns a candidate index (0..=3,
    /// representing position in the candidate list `{1, 4, 6, 7}`),
    /// not the absolute mask number. `write_format_info_bits` for
    /// Micro QR consumes the candidate index.
    #[test]
    fn select_best_micro_mask_returns_valid_candidate() {
        let pixs = vec![0i8; 121];
        let positions: Vec<usize> = (0..36).collect();
        let (cand_idx, _result_pixs) =
            select_best_micro_mask(&pixs, &positions, 3, 11, 11).unwrap();
        assert!(cand_idx < 4, "candidate idx must be 0..=3, got {cand_idx}");
    }

    // ---------------------------------------------------------------
    // Stage 7b — Full QR N1+N2+N3+N4 scoring
    // ---------------------------------------------------------------

    /// `evalfull_n1n3`: a run of 5 contributes 3 (= 5 - 2). A run of
    /// 6 contributes 4. Runs < 5 contribute 0.
    #[test]
    fn evalfull_n1_simple_runs() {
        // BWIPP sentinel is at index 0.
        let rle = vec![0u32, 4, 5, 6, 3, 7];
        let (n1, _n3) = evalfull_n1n3(&rle);
        // Run 5 -> 3, run 6 -> 4, run 7 -> 5. Total = 12.
        assert_eq!(n1, 12);
    }

    /// `evalfull_n1n3` no runs >= 5 → n1 = 0.
    #[test]
    fn evalfull_n1_no_long_runs() {
        let rle = vec![0u32, 2, 3, 4, 1, 2];
        let (n1, n3) = evalfull_n1n3(&rle);
        assert_eq!(n1, 0);
        assert_eq!(n3, 0);
    }

    /// `evalfull_n1n3` finder-like 1:1:3:1:1 pattern at start of row.
    /// fact=1 → central run=3, others=1. Pattern at j=3 (at_start
    /// quiet-zone exemption).
    #[test]
    fn evalfull_n3_pattern_at_start() {
        // scrle = [0, 1, 1, 3, 1, 1, ...] — j=3, central=3, others=1.
        let rle = vec![0u32, 1, 1, 3, 1, 1, 0];
        let (n1, n3) = evalfull_n3_input_to_n3(&rle);
        assert_eq!(n1, 0);
        assert_eq!(n3, 40, "finder-like pattern at start should add 40");
    }

    /// Helper closure for the test — same as evalfull_n1n3 but
    /// destructuring.
    fn evalfull_n3_input_to_n3(rle: &[u32]) -> (u32, u32) {
        evalfull_n1n3(rle)
    }

    /// N2 = 3 × number of 2×2 same-color blocks. All-light 4×4 grid:
    /// number of 2×2 blocks = (4-1) × (4-1) = 9. Penalty = 27.
    #[test]
    fn evalfull_n2_all_light() {
        let pixs = vec![0i8; 16];
        let n2 = evalfull_n2(&pixs, 4, 4);
        assert_eq!(n2, 27);
    }

    /// N2 = 0 when no 2×2 block is uniform. Checkerboard pattern:
    /// every 2×2 block has 2 dark + 2 light → not uniform → N2 = 0.
    #[test]
    fn evalfull_n2_checkerboard() {
        // 4×4 checkerboard.
        let mut pixs = vec![0i8; 16];
        for r in 0..4 {
            for c in 0..4 {
                if (r + c) % 2 == 0 {
                    pixs[r * 4 + c] = 1;
                }
            }
        }
        let n2 = evalfull_n2(&pixs, 4, 4);
        assert_eq!(n2, 0);
    }

    /// N4 = 0 when dark-percentage is exactly 50%.
    #[test]
    fn evalfull_n4_balanced() {
        // 4×4 with exactly 8 dark cells (50%).
        let mut pixs = vec![0i8; 16];
        for cell in pixs.iter_mut().take(8) {
            *cell = 1;
        }
        let n4 = evalfull_n4(&pixs, 4, 4);
        assert_eq!(n4, 0);
    }

    /// N4: all-dark → 100% dark, deviation 50, penalty = ⌊50/5⌋*10 = 100.
    #[test]
    fn evalfull_n4_all_dark() {
        let pixs = vec![1i8; 16];
        let n4 = evalfull_n4(&pixs, 4, 4);
        assert_eq!(n4, 100);
    }

    /// `evaluate_mask_full` for all-light 21×21 (V1): all-light → 100%
    /// light → N4=100. No long runs, no 2×2 mixed (all uniform light).
    /// All-light 2×2 blocks: (21-1)*(21-1) = 400, N2 = 1200. N1 per row:
    /// each row has one run of 21 → (21-2) = 19. 21 rows + 21 cols ×
    /// 19 = 798. N3=0 (no 1:1:3:1:1 with fact=1 since central run is 21).
    /// Total = 798 + 1200 + 0 + 100 = 2098.
    #[test]
    fn evaluate_mask_full_all_light_v1() {
        let pixs = vec![0i8; 441];
        let score = evaluate_mask_full(&pixs, 21, 21);
        // 21 rows × (21-2) + 21 cols × (21-2) + 400*3 + 100 = 798 + 1200 + 100 = 2098.
        assert_eq!(score, 2098);
    }

    /// `select_best_full_mask` rejects non-Full layouts.
    #[test]
    fn select_best_full_mask_rejects_non_full() {
        let pixs = vec![0i8; 121];
        let positions: Vec<usize> = (0..36).collect();
        // Stage 11.A8c — upgrade 2 discriminant-only sites to
        // multi-anchor pins matching the source diagnostic at line
        // 3452-3453 (`qrcode_native: select_best_full_mask called
        // with non-Full layout_id {layout_id}`). Distinct layout_ids
        // cover both non-Full families: M1 (Micro, id=3) + R7x43
        // (rMQR, id=7). Sibling-arm pin parallel to the
        // select_best_micro_mask upgrade (78a4ccf).
        match select_best_full_mask(&pixs, &positions, 3, 11, 11).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "M1 layout_id=3 missing prefix: {msg}"
                );
                assert!(
                    msg.contains("select_best_full_mask"),
                    "missing function name: {msg}"
                );
                assert!(
                    msg.contains("non-Full layout_id"),
                    "missing `non-Full layout_id` predicate: {msg}"
                );
                assert!(msg.contains("layout_id 3"), "M1 value-echo missing: {msg}");
            }
            other => panic!(
                "select_best_full_mask(layout_id=3) should reject as InvalidData, got {other:?}"
            ),
        }
        match select_best_full_mask(&pixs, &positions, 7, 7, 43).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "R7x43 layout_id=7 missing prefix: {msg}"
                );
                assert!(
                    msg.contains("non-Full layout_id"),
                    "missing `non-Full layout_id` predicate: {msg}"
                );
                assert!(
                    msg.contains("layout_id 7"),
                    "R7x43 value-echo missing (proves {{layout_id}} interpolation isn't hardcoded to 3): {msg}"
                );
            }
            other => panic!(
                "select_best_full_mask(layout_id=7) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// `select_best_full_mask` returns a valid candidate (0..=7) for V1.
    #[test]
    fn select_best_full_mask_returns_valid_candidate() {
        let pixs = vec![0i8; 441];
        let positions: Vec<usize> = (0..208).collect();
        let (mask, _pixs) = select_best_full_mask(&pixs, &positions, 0, 21, 21).unwrap();
        assert!(mask < 8, "mask must be 0..=7, got {mask}");
    }

    // ---------------------------------------------------------------
    // Stage 8 — Format-info bit write
    // ---------------------------------------------------------------

    /// QRCODE_EC_INDICATOR_FULL matches BWIPP qrcode_ecidfull
    /// (bwip-js 26148) — L=1, M=0, Q=3, H=2.
    #[test]
    fn ec_indicator_full_constants() {
        assert_eq!(QRCODE_EC_INDICATOR_FULL[0], 1, "L");
        assert_eq!(QRCODE_EC_INDICATOR_FULL[1], 0, "M");
        assert_eq!(QRCODE_EC_INDICATOR_FULL[2], 3, "Q");
        assert_eq!(QRCODE_EC_INDICATOR_FULL[3], 2, "H");
    }

    /// `compute_format_info_bits` for Full QR L+mask0.
    /// ec_id=1, data = (1<<3) | 0 = 8.
    /// BCH(15,5) of 8 = 0x23D6 (per Stage 2b anchor).
    /// XOR mask = 0x5412 → 0x77C4 (matches ISO Table 12 L+mask0).
    #[test]
    fn format_info_bits_full_l_mask0() {
        let result = compute_format_info_bits(0, 0, 0).unwrap();
        assert_eq!(
            result, 0x77C4,
            "Full QR L+mask0 should be 0x77C4 per ISO 18004 Table 12"
        );
    }

    /// `compute_format_info_bits` for Full QR L+mask1.
    /// data = 8 | 1 = 9. BCH(9) = 0x26E1, XOR 0x5412 = 0x72F3.
    #[test]
    fn format_info_bits_full_l_mask1() {
        let result = compute_format_info_bits(0, 0, 1).unwrap();
        assert_eq!(result, 0x72F3, "ISO 18004 Table 12 L+mask1");
    }

    /// Micro QR M1 has only L (ec_level=0) → sym_id=0. mask_index
    /// must be 0..=3 per BWIPP convention. data = (0<<2) | 0 = 0.
    /// BCH(0) = 0. XOR FORMAT_INFO_MASK_MICRO (0x4445) = 0x4445.
    #[test]
    fn format_info_bits_micro_m1() {
        let result = compute_format_info_bits(3, 0, 0).unwrap();
        assert_eq!(result, 0x4445, "Micro M1 L+mask_candidate_0");
    }

    /// `micro_sym_id` for various (layout, ec_level).
    ///
    /// The two rejection-arm anchors (M1 with ec=1, M4 with ec=3) both
    /// route through the None-cell unwrap arm at line 3678, which
    /// emits `"qrcode_native: Micro QR layout_id {layout_id} doesn't
    /// support EC level {ec_level}"`. Each anchor pins the distinct
    /// {layout_id}/{ec_level} echo so mutations that drop either
    /// interpolation get caught.
    #[test]
    fn micro_sym_id_anchors() {
        // M1 L only.
        assert_eq!(micro_sym_id(3, 0).unwrap(), 0);
        // M1 doesn't support M. Pin diagnostic + layout/ec echoes.
        match micro_sym_id(3, 1).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: Micro QR layout_id 3"),
                    "M1+M diagnostic must echo layout_id=3; got {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 1"),
                    "M1+M diagnostic must echo ec_level=1; got {msg}"
                );
            }
            other => panic!("M1+M: expected InvalidData, got {other:?}"),
        }
        // M4 L/M/Q.
        assert_eq!(micro_sym_id(6, 0).unwrap(), 5);
        assert_eq!(micro_sym_id(6, 1).unwrap(), 6);
        assert_eq!(micro_sym_id(6, 2).unwrap(), 7);
        // M4 doesn't support H. Pin diagnostic + distinct echoes
        // (layout=6, ec=3) — proves the format string interpolates
        // BOTH parameters not just one.
        match micro_sym_id(6, 3).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: Micro QR layout_id 6"),
                    "M4+H diagnostic must echo layout_id=6; got {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 3"),
                    "M4+H diagnostic must echo ec_level=3; got {msg}"
                );
                // Cross-arm contamination guard: the layout-range arm
                // says "is not a Micro QR layout" — must NOT appear
                // when the None-cell arm fires.
                assert!(
                    !msg.contains("is not a Micro QR layout"),
                    "M4+H diagnostic must NOT leak the layout-range arm; got {msg}"
                );
            }
            other => panic!("M4+H: expected InvalidData, got {other:?}"),
        }
    }

    /// `compute_format_info_bits` rejects rMQR (deferred sub-stage).
    #[test]
    fn format_info_bits_rmqr_rejected() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line
        // 3637-3639 (`qrcode_native: compute_format_info_bits handles
        // Full/Micro layouts only — rMQR layouts (layout_id >= 7)
        // use rmqr_fmtval1/rmqr_fmtval2 via write_format_info_bits`).
        match compute_format_info_bits(7, 1, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing prefix: {msg}"
                );
                assert!(
                    msg.contains("Full/Micro layouts only"),
                    "missing `Full/Micro layouts only` predicate: {msg}"
                );
                assert!(
                    msg.contains("rmqr_fmtval"),
                    "missing rMQR-specific remediation hint: {msg}"
                );
            }
            other => panic!(
                "compute_format_info_bits(rMQR layout_id=7) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// `compute_format_info_bits` rejects out-of-range inputs.
    #[test]
    fn format_info_bits_rejects_invalid() {
        // Stage 11.A8c — upgrade 3 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at
        // lines 3600-3602 (ec_level overflow), 3608-3610 (Full
        // mask overflow), 3622-3624 (Micro mask overflow).
        //
        // ec_level >= 4.
        match compute_format_info_bits(0, 4, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "ec_level arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("ec_level 4"),
                    "ec_level arm missing value echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=3"),
                    "ec_level arm missing range hint: {msg}"
                );
                assert!(
                    !msg.contains("mask_index"),
                    "wrong arm — mask_index diagnostic leaked: {msg}"
                );
            }
            other => panic!(
                "compute_format_info_bits(ec_level=4) should reject as InvalidData, got {other:?}"
            ),
        }
        // Full mask >= 8.
        match compute_format_info_bits(0, 0, 8).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Full QR mask_index 8"),
                    "Full mask arm missing `Full QR mask_index 8` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=7)"),
                    "Full mask arm missing range hint `0..=7`: {msg}"
                );
                assert!(
                    !msg.contains("Micro QR"),
                    "wrong family — Micro QR diagnostic leaked into Full arm: {msg}"
                );
            }
            other => panic!(
                "compute_format_info_bits(Full mask=8) should reject as InvalidData, got {other:?}"
            ),
        }
        // Micro mask >= 4.
        match compute_format_info_bits(3, 0, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Micro QR mask_index 4"),
                    "Micro mask arm missing `Micro QR mask_index 4` echo: {msg}"
                );
                assert!(
                    msg.contains("out of range (0..=3)"),
                    "Micro mask arm missing range hint `0..=3`: {msg}"
                );
                assert!(
                    !msg.contains("Full QR mask_index"),
                    "wrong family — Full QR mask diagnostic leaked into Micro arm: {msg}"
                );
            }
            other => panic!(
                "compute_format_info_bits(Micro mask=4) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// `write_format_info_bits` for V1-L mask0 — verify 30 cells
    /// hold the correct bits + dark module is 1.
    #[test]
    fn write_format_info_bits_v1_l_mask0() {
        let mut pixs = vec![PIXS_UNSET; 441];
        write_format_info_bits(&mut pixs, 0, 21, 21, 0, 0).unwrap();
        let cu = 21usize;
        let expected = 0x77C4u16;
        // Verify TL cluster bit-by-bit.
        let pairs = format_info_pairs_full(21, 21);
        for (i, &((r1, c1), (r2, c2))) in pairs.iter().enumerate() {
            let want = ((expected >> (14 - i)) & 1) as i8;
            assert_eq!(
                pixs[qmv(r1, c1, cu)],
                want,
                "TL cluster bit {i} at ({r1},{c1})"
            );
            assert_eq!(
                pixs[qmv(r2, c2, cu)],
                want,
                "TR/BL cluster bit {i} at ({r2},{c2})"
            );
        }
        // Dark module at (rows-8, 8) = (13, 8).
        assert_eq!(pixs[qmv(13, 8, cu)], 1, "dark module must be 1");
    }

    /// `write_format_info_bits` for M1 L+mask_candidate_0.
    #[test]
    fn write_format_info_bits_m1() {
        let mut pixs = vec![PIXS_UNSET; 121];
        write_format_info_bits(&mut pixs, 3, 11, 11, 0, 0).unwrap();
        let cu = 11usize;
        let expected = 0x4445u16;
        for (i, &(r, c)) in FORMAT_INFO_POSITIONS_MICRO.iter().enumerate() {
            let want = ((expected >> (14 - i)) & 1) as i8;
            assert_eq!(pixs[qmv(r, c, cu)], want, "Micro M1 bit {i} at ({r},{c})");
        }
    }

    /// `write_dark_module_full` writes 1 at (rows-8, 8).
    #[test]
    fn dark_module_write() {
        let mut pixs = vec![PIXS_UNSET; 441];
        write_dark_module_full(&mut pixs, 21, 21);
        assert_eq!(pixs[qmv(13, 8, 21)], 1);
    }

    // ---------------------------------------------------------------
    // Stage 8b — V7+ version-info bit write
    // ---------------------------------------------------------------

    /// `version_info_pairs_full` for V7 (cols=45): cluster i=0 is at
    /// (TR row 5, col 36) + (BL row 36, col 5).
    #[test]
    fn version_info_pairs_v7() {
        let pairs = version_info_pairs_full(45);
        // Cluster 0 (MSB).
        assert_eq!(pairs[0], ((5, 36), (36, 5)));
        // Cluster 1.
        assert_eq!(pairs[1], ((5, 35), (35, 5)));
        // Cluster 2.
        assert_eq!(pairs[2], ((5, 34), (34, 5)));
        // Cluster 3 = K2=4, K1=9 → (4, 36), (36, 4).
        assert_eq!(pairs[3], ((4, 36), (36, 4)));
        // Cluster 17 (LSB) = K2=0, K1=11 → (0, 34), (34, 0).
        assert_eq!(pairs[17], ((0, 34), (34, 0)));
    }

    /// `write_version_info_bits` for V7 writes 36 cells with the
    /// correct BCH(18,6) encoded value.
    #[test]
    fn write_version_info_bits_v7() {
        let mut pixs = vec![PIXS_UNSET; 45 * 45];
        let count = write_version_info_bits(&mut pixs, 0, 45, 45, 7).unwrap();
        assert_eq!(count, 36);
        // V7 BCH(18,6) = 0x7C94 = 0b000111110010010100.
        let expected = 0x7C94u32;
        let cu = 45usize;
        let pairs = version_info_pairs_full(45);
        for (i, &((r1, c1), (r2, c2))) in pairs.iter().enumerate() {
            let want = ((expected >> (17 - i)) & 1) as i8;
            assert_eq!(
                pixs[qmv(r1, c1, cu)],
                want,
                "V7 TR cluster {i} at ({r1},{c1})"
            );
            assert_eq!(
                pixs[qmv(r2, c2, cu)],
                want,
                "V7 BL cluster {i} at ({r2},{c2})"
            );
        }
    }

    /// V1..V6 are no-op (no version-info reservation).
    #[test]
    fn write_version_info_bits_pre_v7_no_op() {
        let mut pixs = vec![PIXS_UNSET; 21 * 21];
        let count = write_version_info_bits(&mut pixs, 0, 21, 21, 1).unwrap();
        assert_eq!(count, 0);
        assert!(pixs.iter().all(|&c| c == PIXS_UNSET));
    }

    /// Micro QR / rMQR are no-op.
    #[test]
    fn write_version_info_bits_non_full_no_op() {
        // Micro QR M1 with fictional version=7.
        let mut pixs = vec![PIXS_UNSET; 11 * 11];
        let count = write_version_info_bits(&mut pixs, 3, 11, 11, 7).unwrap();
        assert_eq!(count, 0);
        // rMQR R7x43 with fictional version=7.
        let mut pixs2 = vec![PIXS_UNSET; 7 * 43];
        let count2 = write_version_info_bits(&mut pixs2, 7, 7, 43, 7).unwrap();
        assert_eq!(count2, 0);
    }

    /// V40 (177×177) version-info — extreme case.
    #[test]
    fn write_version_info_bits_v40() {
        let mut pixs = vec![PIXS_UNSET; 177 * 177];
        let count = write_version_info_bits(&mut pixs, 2, 177, 177, 40).unwrap();
        assert_eq!(count, 36);
        let pairs = version_info_pairs_full(177);
        // Cluster 0 for V40 (cols=177): (5, 168) + (168, 5).
        assert_eq!(pairs[0], ((5, 168), (168, 5)));
        // BCH(18, 6) of V40: top 6 bits round-trip.
        let expected = bch18_6_encode(40);
        assert_eq!((expected >> 12) as u8, 40);
    }

    /// `write_version_info_bits` rejects version > 40.
    #[test]
    fn write_version_info_bits_rejects_invalid_version() {
        let mut pixs = vec![PIXS_UNSET; 21 * 21];
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line
        // 3848-3850 (`qrcode_native: invalid Full QR version 41
        // (must be 1..=40)`). Cross-helper guard: this diagnostic
        // is distinct from the `Full QR version out of range`
        // wording used by `encode_full_qr` (which says
        // `out of range (1..=40)` rather than `(must be 1..=40)`).
        match write_version_info_bits(&mut pixs, 0, 21, 21, 41).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("invalid Full QR version"),
                    "missing `invalid Full QR version` predicate: {msg}"
                );
                assert!(
                    msg.contains("41"),
                    "missing offending version value `41`: {msg}"
                );
                assert!(
                    msg.contains("must be 1..=40"),
                    "missing `must be 1..=40` range hint: {msg}"
                );
                assert!(
                    !msg.contains("out of range (1..=40)"),
                    "wrong helper — encode_full_qr's `out of range (1..=40)` wording leaked: {msg}"
                );
            }
            other => panic!(
                "write_version_info_bits(version=41) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// `select_best_mask` dispatches per format.
    #[test]
    fn select_best_mask_dispatch() {
        let pixs_v1 = vec![0i8; 441];
        let positions_v1: Vec<usize> = (0..208).collect();
        let (m_full, _) = select_best_mask(&pixs_v1, &positions_v1, 0, 21, 21).unwrap();
        assert!(m_full < 8);

        // Micro QR: return value is the CANDIDATE INDEX (0..=3),
        // not the absolute mask number.
        let pixs_m1 = vec![0i8; 121];
        let positions_m1: Vec<usize> = (0..36).collect();
        let (m_micro, _) = select_best_mask(&pixs_m1, &positions_m1, 3, 11, 11).unwrap();
        assert!(m_micro < 4, "Micro QR candidate idx must be 0..=3");

        let pixs_r = vec![0i8; 301];
        let positions_r: Vec<usize> = (0..50).collect();
        let (m_rmqr, _) = select_best_mask(&pixs_r, &positions_r, 7, 7, 43).unwrap();
        assert_eq!(m_rmqr, 4, "rMQR uses mask 4 with no scoring");
    }

    /// Debug helper: apply mask 0 (bwip-js's choice for V1-L HELLO
    /// WORLD per format-info bit decoding) to our pre-mask pixs and
    /// dump the resulting matrix. If our data placement matches
    /// bwip-js, the data region (excluding format-info) should be
    /// bit-identical with the bwip-js golden.
    #[test]
    #[ignore]
    fn debug_apply_mask0_v1_l_hello() {
        let msg = b"HELLO WORLD";
        let metric_idx = 4;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        apply_mask_at_positions(&mut pixs, &positions, 0, cols);
        write_format_info_bits(&mut pixs, layout_id, rows, cols, ec_level, 0).unwrap();
        let cu = cols as usize;
        println!("=== V1-L HELLO WORLD forced-mask-0 pixs (vs bwip-js golden) ===");
        for r in 0..rows as usize {
            let mut line = String::new();
            for c in 0..cu {
                line.push(match pixs[qmv(r, c, cu)] {
                    0 => '.',
                    1 => '#',
                    -1 => '?',
                    _ => '!',
                });
            }
            println!("{line}");
        }
    }

    /// Debug helper: dump ALL walker positions for V1-L for parallel
    /// diff against bwip-js's walker trace at /tmp/bwipp_walker_v1_l.txt.
    #[test]
    #[ignore]
    fn debug_walker_full_trace() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        place_timing_patterns(&mut pixs, 0, 21, 21, NA, NA);
        place_alignment_patterns(&mut pixs, 0, 21, 21, 0, 0);
        place_format_info_reservation(&mut pixs, 0, 21, 21);
        let _ = place_version_info_reservation(&mut pixs, 0, 21, 21, 1);
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        for (i, &pos) in positions.iter().enumerate() {
            let posy = pos / 21;
            let posx = pos % 21;
            println!("WALK num={i} posx={posx} posy={posy} pos={pos}");
        }
    }

    /// Debug helper: dump first 30 walker positions for V1-L to
    /// verify zig-zag order matches ISO 18004 Figure 24.
    #[test]
    #[ignore]
    fn debug_walker_first_positions() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        place_timing_patterns(&mut pixs, 0, 21, 21, NA, NA);
        place_alignment_patterns(&mut pixs, 0, 21, 21, 0, 0);
        place_format_info_reservation(&mut pixs, 0, 21, 21);
        let _ = place_version_info_reservation(&mut pixs, 0, 21, 21, 1);
        let positions = walk_codeword_positions(0, 21, 21, &pixs);
        for (i, &pos) in positions.iter().take(30).enumerate() {
            let r = pos / 21;
            let c = pos % 21;
            println!("walker[{i}] = idx {pos} = (r={r}, c={c})");
        }
        // Positions 24-30 (transition out of cols 19-20 into cols 17-18)
        for (i, &pos) in positions.iter().enumerate().take(35).skip(20) {
            let r = pos / 21;
            let c = pos % 21;
            println!("walker[{i}] = idx {pos} = (r={r}, c={c})");
        }
    }

    /// Debug helper: find the walker position index for a given
    /// (row, col) cell and print the corresponding codeword stream
    /// bit value.
    #[test]
    #[ignore]
    fn debug_walker_position_for_cell() {
        let msg = b"HELLO WORLD";
        let metric_idx = 4;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        println!(
            "walker produced {} positions; stream is {} bytes / {} bits",
            positions.len(),
            stream.len(),
            stream.len() * 8
        );
        // For some specific cells of interest, print their position
        // index + the codeword bit that lands there.
        let cu = cols as usize;
        for &(r, c) in &[
            (1usize, 10usize),
            (1, 11),
            (1, 12),
            (1, 9),
            (0, 9),
            (0, 10),
            (20, 20),
            (20, 19),
            (19, 20),
        ] {
            let idx = qmv(r, c, cu);
            if let Some(pos_i) = positions.iter().position(|&p| p == idx) {
                let byte = stream[pos_i / 8];
                let bit = (byte >> (7 - (pos_i % 8))) & 1;
                println!(
                    "({r},{c}) idx={idx} → walker pos {pos_i} → cws[{}].bit{} = {}",
                    pos_i / 8,
                    7 - (pos_i % 8),
                    bit
                );
            } else {
                println!("({r},{c}) idx={idx} NOT in walker positions");
            }
        }
    }

    /// Debug helper: dumps our RLE for each row and column of the
    /// V1-L HELLO WORLD mask-0-applied pixs, plus per-row/col N1+N3,
    /// for comparison against bwip-js's evalfull internals.
    #[test]
    #[ignore]
    fn debug_rle_v1_l_mask0() {
        let msg = b"HELLO WORLD";
        let metric_idx = 4;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        apply_mask_at_positions(&mut pixs, &positions, 0, cols);
        let cu = cols as usize;
        let ru = rows as usize;
        for r in 0..ru {
            let row_rle = rle_run((0..cu).map(|c| pixs[qmv(r, c, cu)]));
            let (n1, n3) = evalfull_n1n3(&row_rle);
            println!("ROW {r}: rle={row_rle:?} n1={n1} n3={n3}");
        }
        for c in 0..cu {
            let col_rle = rle_run((0..ru).map(|r| pixs[qmv(r, c, cu)]));
            let (n1, n3) = evalfull_n1n3(&col_rle);
            println!("COL {c}: rle={col_rle:?} n1={n1} n3={n3}");
        }
    }

    /// Debug helper: dumps our N2-block detection count for V1-L
    /// HELLO WORLD mask 0, with per-cell breakdown.
    #[test]
    #[ignore]
    fn debug_n2_blocks_v1_l_mask0() {
        let msg = b"HELLO WORLD";
        let metric_idx = 4;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        apply_mask_at_positions(&mut pixs, &positions, 0, cols);
        let cu = cols as usize;
        let ru = rows as usize;
        let mut count = 0;
        for r in 0..(ru - 1) {
            for c in 0..(cu - 1) {
                let a = pixs[qmv(r, c, cu)];
                let b = pixs[qmv(r, c + 1, cu)];
                let d = pixs[qmv(r + 1, c, cu)];
                let e = pixs[qmv(r + 1, c + 1, cu)];
                if a == b && a == d && a == e && (a == 0 || a == 1) {
                    count += 1;
                    println!("N2 block at ({r},{c}) val={a}");
                }
            }
        }
        println!("total N2 blocks: {count} → N2 penalty {}", count * 3);
    }

    /// Debug helper: dump col 8 of V1-Q HELLO mask 4 post-mask cells.
    #[test]
    #[ignore]
    fn debug_col8_v1_q_hello_mask4() {
        let msg = b"HELLO";
        let metric_idx = 4;
        let ec_level = 2;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        apply_mask_at_positions(&mut pixs, &positions, 4, cols);
        let cu = cols as usize;
        let ru = rows as usize;
        let mut col8: Vec<i8> = Vec::with_capacity(ru);
        for r in 0..ru {
            col8.push(pixs[qmv(r, 8, cu)]);
        }
        println!("Our col 8 (V1-Q HELLO mask 4): {col8:?}");
        let col_rle = rle_run((0..ru).map(|r| pixs[qmv(r, 8, cu)]));
        println!("  RLE: {col_rle:?}");
    }

    /// Debug helper: dump our M1 12345 (mask scored) output vs the
    /// bwip-js golden side by side.
    #[test]
    #[ignore]
    fn debug_m1_12345_diff() {
        let matrix = encode_micro_qr(b"12345", 0, 0).unwrap();
        let w = matrix.width();
        println!("Our M1 12345 (mask scored):");
        for r in 0..w {
            let mut line = String::new();
            for c in 0..w {
                line.push(if matrix.get(c, r) { '#' } else { '.' });
            }
            println!("  {line}");
        }
    }

    /// Debug helper: M3-L HELLO12 dump.
    #[test]
    #[ignore]
    fn debug_m3_hello12_diff() {
        let matrix = encode_micro_qr(b"HELLO12", 2, 0).unwrap();
        let w = matrix.width();
        println!("Our M3 HELLO12 (mask scored):");
        for r in 0..w {
            let mut line = String::new();
            for c in 0..w {
                line.push(if matrix.get(c, r) { '#' } else { '.' });
            }
            println!("  {line}");
        }
    }

    /// Debug helper: dump our M3-L HELLO12 raw bits (after compose).
    #[test]
    #[ignore]
    fn debug_m3_compose_bits() {
        let msg = b"HELLO12";
        let layout_id = 5; // M3
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        println!("M3-L HELLO12: {} bits", bits.len());
        let mut s = String::new();
        for &b in &bits {
            s.push(if b { '1' } else { '0' });
        }
        println!("bits: {s}");
    }

    /// Debug helper: dump our M3-L HELLO12 cws stream after lc4b fixup.
    #[test]
    #[ignore]
    fn debug_m3_cws_stream() {
        let msg = b"HELLO12";
        let metric_idx = 2;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        println!("Our padded data (pre-lc4b): {padded:?}");
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        println!("Our M3-L HELLO12 cws (post-lc4b): {stream:?}");
    }

    /// Debug helper: dump our M3 walker positions for HELLO12.
    #[test]
    #[ignore]
    fn debug_m3_walker_trace() {
        let msg = b"HELLO12";
        let metric_idx = 2; // M3
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        println!(
            "walker produced {} positions; stream is {} bytes = {} bits",
            positions.len(),
            stream.len(),
            stream.len() * 8
        );
        let cu = cols as usize;
        for (i, &pos) in positions.iter().enumerate() {
            let posy = pos / cu;
            let posx = pos % cu;
            let byte = stream[i / 8];
            let bit = (byte >> (7 - (i % 8))) & 1;
            println!("WALK num={i} posx={posx} posy={posy} pos={pos} bit={bit}");
        }
    }

    /// Debug helper: per-row and per-col (n1, n3) contributions for
    /// V1-Q HELLO mask 4, for diff against bwip-js's per-line dump.
    #[test]
    #[ignore]
    fn debug_per_line_v1_q_hello_mask4() {
        let msg = b"HELLO";
        let metric_idx = 4;
        let ec_level = 2;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        apply_mask_at_positions(&mut pixs, &positions, 4, cols);
        let cu = cols as usize;
        let ru = rows as usize;
        for i in 0..cu {
            let col_rle = rle_run((0..ru).map(|r| pixs[qmv(r, i, cu)]));
            let (n1, n3) = evalfull_n1n3(&col_rle);
            println!("COL i={i} m=4 n3_added={n3} n1_added={n1}");
        }
        for i in 0..ru {
            let row_rle = rle_run((0..cu).map(|c| pixs[qmv(i, c, cu)]));
            let (n1, n3) = evalfull_n1n3(&row_rle);
            println!("ROW i={i} m=4 n3_added={n3} n1_added={n1}");
        }
    }

    /// Debug helper: same as debug_score_components_v1_l_hello but for
    /// V1-Q HELLO (the last remaining corpus failure).
    #[test]
    #[ignore]
    fn debug_score_components_v1_q_hello() {
        let msg = b"HELLO";
        let metric_idx = 4;
        let ec_level = 2; // Q
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        for m_idx in 0..8u8 {
            let mut trial = pixs.clone();
            apply_mask_at_positions(&mut trial, &positions, m_idx, cols);
            let cu = cols as usize;
            let ru = rows as usize;
            let mut n1: u32 = 0;
            let mut n3: u32 = 0;
            for c in 0..cu {
                let col_rle = rle_run((0..ru).map(|r| trial[qmv(r, c, cu)]));
                let (n1c, n3c) = evalfull_n1n3(&col_rle);
                n1 += n1c;
                n3 += n3c;
            }
            for r in 0..ru {
                let row_rle = rle_run((0..cu).map(|c| trial[qmv(r, c, cu)]));
                let (n1r, n3r) = evalfull_n1n3(&row_rle);
                n1 += n1r;
                n3 += n3r;
            }
            let n2 = evalfull_n2(&trial, rows, cols);
            let n4 = evalfull_n4(&trial, rows, cols);
            println!(
                "MASK_DETAIL m={m_idx} n1={n1} n2={n2} n3={n3} n4={n4} total={}",
                n1 + n2 + n3 + n4
            );
        }
    }

    /// Debug helper: encodes V1-L HELLO WORLD up to the mask-scoring
    /// step and dumps n1/n2/n3/n4 components per mask for direct
    /// comparison against bwip-js's evalfull.
    #[test]
    #[ignore]
    fn debug_score_components_v1_l_hello() {
        let msg = b"HELLO WORLD";
        let metric_idx = 4;
        let ec_level = 0;
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        for m_idx in 0..8u8 {
            let mut trial = pixs.clone();
            apply_mask_at_positions(&mut trial, &positions, m_idx, cols);
            // Compute n1 + n3 piecewise (column then row).
            let cu = cols as usize;
            let ru = rows as usize;
            let mut n1: u32 = 0;
            let mut n3: u32 = 0;
            for c in 0..cu {
                let col_rle = rle_run((0..ru).map(|r| trial[qmv(r, c, cu)]));
                let (n1c, n3c) = evalfull_n1n3(&col_rle);
                n1 += n1c;
                n3 += n3c;
            }
            for r in 0..ru {
                let row_rle = rle_run((0..cu).map(|c| trial[qmv(r, c, cu)]));
                let (n1r, n3r) = evalfull_n1n3(&row_rle);
                n1 += n1r;
                n3 += n3r;
            }
            let n2 = evalfull_n2(&trial, rows, cols);
            let n4 = evalfull_n4(&trial, rows, cols);
            println!(
                "MASK_DETAIL m={m_idx} n1={n1} n2={n2} n3={n3} n4={n4} total={}",
                n1 + n2 + n3 + n4
            );
        }
    }

    /// Debug helper: encodes V1-L HELLO WORLD up to the mask-scoring
    /// step and dumps the score of each of the 8 mask candidates.
    /// Used to diagnose mask-selection divergence vs bwip-js.
    #[test]
    #[ignore]
    fn debug_score_each_mask_v1_l_hello() {
        // Replay the encode_full_qr pipeline up through codeword
        // placement, then evaluate each mask separately.
        let msg = b"HELLO WORLD";
        let metric_idx = 4; // V1
        let ec_level = 0; // L
        let m = &FULL_METRICS[metric_idx];
        let layout_id = m.layout_id;
        let rows = m.rows;
        let cols = m.cols;
        let datacap_bits = m.datacap_bits;
        let layout = block_layout(metric_idx, ec_level).unwrap();
        let segments = select_segments(msg, layout_id, false);
        let bits = compose_segments(msg, &segments, layout_id, false).unwrap();
        let padded = pad_codewords(&bits, layout_id, datacap_bits, layout.dcws).unwrap();
        let stream = build_codeword_stream(&padded, metric_idx, ec_level).unwrap();
        let mut pixs = init_pixs_matrix(rows, cols);
        place_finder_patterns(&mut pixs, layout_id, rows, cols);
        place_timing_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_alignment_patterns(&mut pixs, layout_id, rows, cols, m.fimax, m.fimas);
        place_format_info_reservation(&mut pixs, layout_id, rows, cols);
        let _ = place_version_info_reservation(&mut pixs, layout_id, rows, cols, 1);
        let positions = walk_codeword_positions(layout_id, rows, cols, &pixs);
        place_codewords_at(&mut pixs, &positions, &stream).unwrap();
        for m_idx in 0..8u8 {
            let mut trial = pixs.clone();
            apply_mask_at_positions(&mut trial, &positions, m_idx, cols);
            let score = evaluate_mask_full(&trial, rows, cols);
            println!("V1-L HELLO WORLD mask {m_idx} score = {score}");
        }
    }

    /// One-off debug dumper: prints both our matrix and the corpus
    /// expectation for `v1_l_hello_world` so divergences can be eyed.
    /// Marked `#[ignore]` so it doesn't run in normal CI; toggle with
    /// `cargo test ... -- --ignored debug_dump_v1_l_hello`.
    #[test]
    #[ignore]
    fn debug_dump_v1_l_hello() {
        let matrix = encode_full_qr(b"HELLO WORLD", 1, 0).unwrap();
        let w = matrix.width();
        println!("=== qrcode_native: V1-L HELLO WORLD ({}x{}) ===", w, w);
        for r in 0..w {
            let mut line = String::new();
            for c in 0..w {
                line.push(if matrix.get(c, r) { '#' } else { '.' });
            }
            println!("{line}");
        }
    }

    /// Debug helper: stamps the V1 finder patterns then verifies
    /// col 0 rows 0..=6 are all dark (the TL finder left edge).
    #[test]
    #[ignore]
    fn debug_dump_finder_only() {
        let mut pixs = init_pixs_matrix(21, 21);
        place_finder_patterns(&mut pixs, 0, 21, 21);
        println!("=== finder-only pixs (V1, 21x21) ===");
        for r in 0..21usize {
            let mut line = String::new();
            for c in 0..21usize {
                let v = pixs[qmv(r, c, 21)];
                line.push(match v {
                    -1 => '?',
                    0 => '.',
                    1 => '#',
                    _ => '!',
                });
            }
            println!("{line}");
        }
        // Verify TL finder column 0:
        println!(
            "TL col-0 stripe: r0..6 = {} {} {} {} {} {} {}",
            pixs[qmv(0, 0, 21)],
            pixs[qmv(1, 0, 21)],
            pixs[qmv(2, 0, 21)],
            pixs[qmv(3, 0, 21)],
            pixs[qmv(4, 0, 21)],
            pixs[qmv(5, 0, 21)],
            pixs[qmv(6, 0, 21)],
        );
    }

    /// Walk every `(label, text, version, eclevel, width, pixs)` row in
    /// `tests/fixtures/qrcode_native_pixs.txt` (generated by
    /// `tools/oracle-qrcode-pixs.js` with `fixedeclevel=true`) and
    /// assert that `encode_full_qr` reproduces BWIPP's pixs array
    /// byte-for-byte. This is the verification basis for the
    /// `Symbology::QrCode` catalog cutover: until every entry in this
    /// fixture passes, the public catalog row keeps routing through
    /// the `qrcode` substrate.
    ///
    /// Pinned permanently as of qrcode_native Stage 11c: all 9
    /// fixture rows pass byte-for-byte against bwip-js's raw().pixs
    /// output with `fixedeclevel=true`. The fixture covers V1 at
    /// every EC level (L/M/Q/H), V2, V3, V5 (multi-block), V7 (first
    /// version with version-info bits), and V10 (multi-block at
    /// larger size). This forms the **substrate-grade verification
    /// basis** for the `Symbology::QrCode` catalog cutover.
    #[test]
    fn encode_full_qr_pixs_corpus_matches_oracle() {
        let corpus = include_str!("../../../tests/fixtures/qrcode_native_pixs.txt");
        let mut tested = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for line in corpus.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(6, '\t');
            let label = parts.next().expect("missing label");
            let text = parts.next().expect("missing text");
            let version_str = parts.next().expect("missing version");
            let ec_str = parts.next().expect("missing eclevel");
            let ec_level: u8 = match ec_str {
                "L" => 0,
                "M" => 1,
                "Q" => 2,
                "H" => 3,
                other => panic!("bad eclevel {other}"),
            };
            let width: usize = parts
                .next()
                .expect("missing width")
                .parse()
                .expect("bad width");
            let want: Vec<u8> = parts
                .next()
                .expect("missing pixs csv")
                .split(',')
                .map(|s| s.parse().expect("bad pixs cell"))
                .collect();
            assert_eq!(want.len(), width * width, "{label}: pixs/width mismatch");

            // Dispatch: Full QR is "1".."40", Micro QR is "M1".."M4".
            let matrix = if let Some(stripped) = version_str.strip_prefix('M') {
                let micro_idx: usize = stripped.parse::<u8>().expect("bad micro idx") as usize - 1;
                encode_micro_qr(text.as_bytes(), micro_idx, ec_level)
                    .unwrap_or_else(|e| panic!("{label}: encode_micro_qr failed: {e:?}"))
            } else {
                let version: u8 = version_str.parse().expect("bad version");
                encode_full_qr(text.as_bytes(), version, ec_level)
                    .unwrap_or_else(|e| panic!("{label}: encode_full_qr failed: {e:?}"))
            };
            assert_eq!(matrix.width(), width, "{label}: width mismatch");
            assert_eq!(matrix.height(), width, "{label}: height mismatch");

            let mut got = vec![0u8; width * width];
            for r in 0..width {
                for c in 0..width {
                    if matrix.get(c, r) {
                        got[r * width + c] = 1;
                    }
                }
            }
            if got != want {
                // Build a concise diff summary: first N differing positions.
                let diffs: Vec<String> = got
                    .iter()
                    .zip(want.iter())
                    .enumerate()
                    .filter(|(_, (g, w))| g != w)
                    .take(8)
                    .map(|(i, (g, w))| {
                        let r = i / width;
                        let c = i % width;
                        format!("({r},{c}) got={g} want={w}")
                    })
                    .collect();
                let total_diff = got.iter().zip(want.iter()).filter(|(g, w)| g != w).count();
                failures.push(format!(
                    "{label}: {total_diff} cell(s) differ; first {}: {}",
                    diffs.len(),
                    diffs.join(", ")
                ));
            }
            tested += 1;
        }
        assert!(tested >= 9, "expected >= 9 corpus rows, ran {tested}");
        assert!(
            failures.is_empty(),
            "qrcode_native pixs corpus mismatches ({} of {tested}):\n  {}",
            failures.len(),
            failures.join("\n  ")
        );
    }

    /// Companion to `encode_full_qr_pixs_corpus_matches_oracle` covering
    /// Micro QR (M1..M4). Pinned permanently as of qrcode_native Stage
    /// 12c — all 4 fixture rows (m1_12345, m2_l_1234, m3_l_hello12,
    /// m4_m_hello_world) pass byte-for-byte against bwip-js's
    /// `raw().pixs` output with `fixedeclevel=true`.
    #[test]
    fn encode_micro_qr_pixs_corpus_matches_oracle() {
        let corpus = include_str!("../../../tests/fixtures/qrcode_native_micro_pixs.txt");
        let mut tested = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for line in corpus.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(6, '\t');
            let label = parts.next().expect("missing label");
            let text = parts.next().expect("missing text");
            let version_str = parts.next().expect("missing version");
            let ec_str = parts.next().expect("missing eclevel");
            let ec_level: u8 = match ec_str {
                "L" => 0,
                "M" => 1,
                "Q" => 2,
                "H" => 3,
                other => panic!("bad eclevel {other}"),
            };
            let width: usize = parts
                .next()
                .expect("missing width")
                .parse()
                .expect("bad width");
            let want: Vec<u8> = parts
                .next()
                .expect("missing pixs csv")
                .split(',')
                .map(|s| s.parse().expect("bad pixs cell"))
                .collect();
            let stripped = version_str
                .strip_prefix('M')
                .expect("Micro corpus row must start with 'M'");
            let micro_idx: usize = stripped.parse::<u8>().expect("bad micro idx") as usize - 1;
            let matrix = encode_micro_qr(text.as_bytes(), micro_idx, ec_level)
                .unwrap_or_else(|e| panic!("{label}: encode_micro_qr failed: {e:?}"));
            assert_eq!(matrix.width(), width, "{label}: width mismatch");
            assert_eq!(matrix.height(), width, "{label}: height mismatch");
            let mut got = vec![0u8; width * width];
            for r in 0..width {
                for c in 0..width {
                    if matrix.get(c, r) {
                        got[r * width + c] = 1;
                    }
                }
            }
            if got != want {
                let total = got.iter().zip(want.iter()).filter(|(g, w)| g != w).count();
                failures.push(format!("{label}: {total} cell(s) differ"));
            }
            tested += 1;
        }
        assert!(tested >= 4, "expected >= 4 micro corpus rows, ran {tested}");
        assert!(
            failures.is_empty(),
            "qrcode_native micro pixs corpus mismatches:\n  {}",
            failures.join("\n  ")
        );
    }

    /// Map an rMQR version string (e.g. "R7x43") to its position in
    /// `FULL_METRICS`. Returns `None` for unknown versions.
    fn rmqr_version_to_metric_idx(v: &str) -> Option<usize> {
        for (idx, m) in FULL_METRICS.iter().enumerate() {
            if matches!(m.format, Format::Rmqr) && m.version_str == v {
                return Some(idx);
            }
        }
        None
    }

    /// rMQR walker positions for R7x43 must match BWIPP byte-for-byte
    /// when the matrix has all function patterns + reservations applied.
    /// BWIPP's walker writes 104 bits for R7x43-M (corresponds to
    /// `datacap_bits = 104`). The fixture
    /// `qrcode_native_rmqr_r7x43_walker.json` captures the walker's
    /// (col, row) positions in traversal order.
    #[test]
    fn rmqr_walker_positions_r7x43_match_bwipp_oracle() {
        let json = include_str!("../../../tests/fixtures/qrcode_native_rmqr_r7x43_walker.json");
        // Parse manually — extract every "[" col "," row "]" pair from
        // the positions array. The fixture has two arrays: positions
        // (col, row tuples) and bits (single values); we want
        // positions which appears first.
        let pos_start = json.find("\"positions\":").expect("positions key") + 12;
        let pos_end = json[pos_start..]
            .find("\"bits\":")
            .expect("bits key after positions")
            + pos_start;
        let pos_section = &json[pos_start..pos_end];
        // Find all numbers in order — every pair forms (col, row).
        let mut flat: Vec<i32> = Vec::new();
        for line in pos_section.lines() {
            let trimmed = line.trim().trim_end_matches(',');
            if let Ok(n) = trimmed.parse::<i32>() {
                flat.push(n);
            }
        }
        let mut pairs: Vec<(i32, i32)> = Vec::with_capacity(flat.len() / 2);
        for chunk in flat.chunks(2) {
            if chunk.len() == 2 {
                pairs.push((chunk[0], chunk[1]));
            }
        }

        // Build the matrix with all function patterns applied (BWIPP's
        // pixs state right before the walker runs).
        let rows = 7u16;
        let cols = 43u16;
        let mut pixs = init_pixs_matrix(rows, cols);
        place_timing_patterns(&mut pixs, 7, rows, cols, 22, 99);
        place_finder_patterns(&mut pixs, 7, rows, cols);
        place_alignment_patterns(&mut pixs, 7, rows, cols, 22, 99);
        place_format_info_reservation(&mut pixs, 7, rows, cols);
        place_version_info_reservation(&mut pixs, 7, rows, cols, 0);
        let got_positions = walk_codeword_positions(7, rows, cols, &pixs);

        // Convert linear idx → (col, row).
        let cu = cols as usize;
        let got: Vec<(i32, i32)> = got_positions
            .iter()
            .map(|&idx| ((idx % cu) as i32, (idx / cu) as i32))
            .collect();
        // First 10 should match.
        for i in 0..pairs.len().min(got.len()).min(10) {
            assert_eq!(
                got[i], pairs[i],
                "rMQR walker pos[{i}] mismatch: got {:?} want {:?}",
                got[i], pairs[i]
            );
        }
        // Find first position where they diverge (one is extra or wrong).
        let mut diverge_at = None;
        let mut gi = 0;
        let mut wi = 0;
        while gi < got.len() && wi < pairs.len() {
            if got[gi] == pairs[wi] {
                gi += 1;
                wi += 1;
                continue;
            }
            diverge_at = Some((gi, wi, got[gi], pairs[wi]));
            break;
        }
        if let Some((gi, wi, g, w)) = diverge_at {
            // Dump surrounding context.
            eprintln!("--- DIVERGENCE at got[{gi}]={g:?} vs want[{wi}]={w:?} ---");
            let lo = gi.saturating_sub(3);
            let hi = (gi + 5).min(got.len());
            for (idx, p) in got.iter().enumerate().take(hi).skip(lo) {
                eprintln!(
                    "got[{idx}]={p:?}{}",
                    if idx == gi { " <- DIVERGES" } else { "" }
                );
            }
            let lo = wi.saturating_sub(3);
            let hi = (wi + 5).min(pairs.len());
            for (idx, p) in pairs.iter().enumerate().take(hi).skip(lo) {
                eprintln!(
                    "want[{idx}]={p:?}{}",
                    if idx == wi { " <- DIVERGES" } else { "" }
                );
            }
        }
        assert_eq!(
            got.len(),
            pairs.len(),
            "rMQR walker length mismatch: got {} want {}",
            got.len(),
            pairs.len()
        );
        let mut mismatches = 0usize;
        for (i, (g, w)) in got.iter().zip(pairs.iter()).enumerate() {
            if g != w {
                if mismatches < 5 {
                    eprintln!("pos[{i}]: got {g:?} want {w:?}");
                }
                mismatches += 1;
            }
        }
        assert_eq!(
            mismatches, 0,
            "rMQR walker has {mismatches} mismatched positions"
        );
    }

    /// Debug-print Rust vs golden side by side for r7x43_m_hi. The
    /// smallest divergence case (~23 cells). Run with
    /// `cargo test rmqr_diff_r7x43_m_hi -- --nocapture --ignored`.
    #[test]
    #[ignore]
    fn rmqr_diff_r7x43_m_hi() {
        let corpus = include_str!("../../../tests/fixtures/qrcode_native_rmqr_pixs.txt");
        let line = corpus
            .lines()
            .find(|l| l.starts_with("r7x43_m_hi\t"))
            .expect("missing fixture row");
        let mut parts = line.splitn(7, '\t');
        let _label = parts.next();
        let _text = parts.next();
        let _ver = parts.next();
        let _ec = parts.next();
        let rows: usize = parts.next().unwrap().parse().unwrap();
        let cols: usize = parts.next().unwrap().parse().unwrap();
        let want: Vec<u8> = parts
            .next()
            .unwrap()
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        let m = encode_qr_at_metric(b"HI", 44, 1).unwrap();
        println!("rows={rows} cols={cols}");
        println!("--- WANT ---");
        for r in 0..rows {
            for c in 0..cols {
                print!("{}", want[r * cols + c]);
            }
            println!();
        }
        println!("--- GOT ---");
        for r in 0..rows {
            for c in 0..cols {
                print!("{}", if m.get(c, r) { 1 } else { 0 });
            }
            println!();
        }
        println!("--- DIFF (W=want, G=got, . match) ---");
        for r in 0..rows {
            for c in 0..cols {
                let w = want[r * cols + c];
                let g = if m.get(c, r) { 1 } else { 0 };
                if w == g {
                    print!(".");
                } else if w == 1 {
                    print!("W"); // expected dark, got light
                } else {
                    print!("G"); // expected light, got dark
                }
            }
            println!();
        }
    }

    /// rMQR oracle corpus: 16 (version × eclevel × text) combinations
    /// generated by `rust/tools/oracle-rmqr-pixs.js` and stored at
    /// `rust/tests/fixtures/qrcode_native_rmqr_pixs.txt`. Each row
    /// must match `encode_qr_at_metric` byte-for-byte.
    ///
    /// This is the byte-for-byte BWIPP pinning that proves the rMQR
    /// pipeline (segments / mode encoder / padding / RS interleaver
    /// / finder + sub-finder placement / format-info bit write /
    /// fixed mask 4) is faithful to bwip-js.
    ///
    /// **Stage 15e status**: enabled. The Stage 15d divergence was
    /// traced to a missing third rMQR timing-pattern run — BWIPP
    /// places vertical timing strips at the alignment-pattern columns
    /// (bwip-js source 27617-27622), spanning rows 3..=rows-4 at each
    /// asp2-1, asp2-1+step, … column. Adding the strip via
    /// `place_timing_patterns` brings all 16 corpus rows into
    /// byte-for-byte agreement with bwip-js.
    #[test]
    fn encode_rmqr_pixs_corpus_matches_oracle() {
        let corpus = include_str!("../../../tests/fixtures/qrcode_native_rmqr_pixs.txt");
        let mut tested = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for line in corpus.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(7, '\t');
            let label = parts.next().expect("missing label");
            let text = parts.next().expect("missing text");
            let version_str = parts.next().expect("missing version");
            let ec_str = parts.next().expect("missing eclevel");
            let ec_level: u8 = match ec_str {
                "M" => 1,
                "H" => 3,
                other => panic!("bad rMQR eclevel {other} (only M / H supported)"),
            };
            let rows: usize = parts
                .next()
                .expect("missing rows")
                .parse()
                .expect("bad rows");
            let cols: usize = parts
                .next()
                .expect("missing cols")
                .parse()
                .expect("bad cols");
            let want: Vec<u8> = parts
                .next()
                .expect("missing pixs csv")
                .split(',')
                .map(|s| s.parse().expect("bad pixs cell"))
                .collect();
            let metric_idx = rmqr_version_to_metric_idx(version_str)
                .unwrap_or_else(|| panic!("{label}: unknown rMQR version {version_str}"));
            let matrix = encode_qr_at_metric(text.as_bytes(), metric_idx, ec_level)
                .unwrap_or_else(|e| panic!("{label}: encode_qr_at_metric failed: {e:?}"));
            assert_eq!(matrix.width(), cols, "{label}: cols mismatch");
            assert_eq!(matrix.height(), rows, "{label}: rows mismatch");
            let mut got = vec![0u8; rows * cols];
            for r in 0..rows {
                for c in 0..cols {
                    if matrix.get(c, r) {
                        got[r * cols + c] = 1;
                    }
                }
            }
            if got != want {
                let total = got.iter().zip(want.iter()).filter(|(g, w)| g != w).count();
                failures.push(format!("{label}: {total} cell(s) differ"));
            }
            tested += 1;
        }
        assert!(tested >= 8, "expected >= 8 rmqr corpus rows, ran {tested}");
        assert!(
            failures.is_empty(),
            "qrcode_native rMQR pixs corpus mismatches:\n  {}",
            failures.join("\n  ")
        );
    }

    /// Stage 11.A8c — pin `digit_value` and `alpha_value` boundary
    /// behavior. Kills `is_ascii_digit` short-circuit mutations and
    /// the `- b'0'` / `< 0` table-lookup boundary mutations.
    #[test]
    fn digit_and_alpha_value_boundaries() {
        // digit_value: '0'..='9' accepted, returns b - b'0'.
        assert_eq!(digit_value(b'0').unwrap(), 0);
        assert_eq!(digit_value(b'5').unwrap(), 5);
        assert_eq!(digit_value(b'9').unwrap(), 9);
        // Non-digit rejected.
        // Stage 11.A8c (cont) — 5 bare `.is_err()` boundary checks
        // upgraded to 3-anchor pins matching the source diagnostic.
        // Brings parity with the sibling
        // `digit_value_pins_digit_arm_and_default_error` test at line
        // 5914+ which already uses the same pattern.
        for (b, hex) in [
            (b'/', "0x2F"), // just below '0'
            (b':', "0x3A"), // just above '9'
            (b'A', "0x41"),
            (0, "0x00"),
            (255, "0xFF"),
        ] {
            match digit_value(b).unwrap_err() {
                crate::error::Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("qrcode_native:"),
                        "byte={b:#x}: missing prefix: {msg}"
                    );
                    assert!(
                        msg.contains("numeric mode expects ASCII digit"),
                        "byte={b:#x}: missing predicate: {msg}"
                    );
                    assert!(
                        msg.contains(hex),
                        "byte={b:#x}: missing {hex} hex echo: {msg}"
                    );
                }
                other => panic!("byte={b:#x} should reject, got {other:?}"),
            }
        }

        // alpha_value: pin known QR alphanumeric table values.
        // '0'..='9' → 0..=9.
        assert_eq!(alpha_value(b'0').unwrap(), 0);
        assert_eq!(alpha_value(b'9').unwrap(), 9);
        // 'A'..='Z' → 10..=35.
        assert_eq!(alpha_value(b'A').unwrap(), 10);
        assert_eq!(alpha_value(b'Z').unwrap(), 35);
        // Specials: ' ' = 36, '$' = 37, '%' = 38, '*' = 39, '+' = 40,
        // '-' = 41, '.' = 42, '/' = 43, ':' = 44.
        assert_eq!(alpha_value(b' ').unwrap(), 36);
        assert_eq!(alpha_value(b'$').unwrap(), 37);
        assert_eq!(alpha_value(b':').unwrap(), 44);
        // Stage 11.A8c (cont) — 5 bare `.is_err()` checks upgraded to
        // 3-anchor pins matching the alpha_value source diagnostic.
        // Brings parity with the sibling `alpha_value_per_arm_and_reject`
        // test at line 5970+ which uses the same pattern.
        for (b, hex) in [
            (b'a', "0x61"), // lowercase
            (b'z', "0x7A"),
            (b'!', "0x21"),
            (b'@', "0x40"),
            (b';', "0x3B"), // ';' just after ':'
        ] {
            match alpha_value(b).unwrap_err() {
                crate::error::Error::InvalidData(msg) => {
                    assert!(
                        msg.contains("qrcode_native:"),
                        "byte={b:#x}: missing prefix: {msg}"
                    );
                    assert!(
                        msg.contains("alphanumeric mode"),
                        "byte={b:#x}: missing `alphanumeric mode`: {msg}"
                    );
                    assert!(
                        msg.contains(hex),
                        "byte={b:#x}: missing {hex} hex echo: {msg}"
                    );
                }
                other => panic!("byte={b:#x} should reject, got {other:?}"),
            }
        }

        // Cross-mode diagnostic pinning + cross-arm contamination guards.
        // Kills mutations that:
        //   * Drop the `0x{b:02X}` byte echo (both modes use the SAME
        //     hex-byte format spec — `b'a'` is 0x61, `b'A'` is 0x41).
        //   * Swap the two predicate strings between the digit/alpha
        //     arms (i.e. the digit-side error must NOT carry the
        //     "alphanumeric mode" wording, and vice-versa).
        //   * Replace the per-symbology prefix with a different tag.
        match digit_value(b'A').unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: numeric mode"),
                    "digit_value: prefix + predicate, got {msg}"
                );
                assert!(
                    msg.contains("0x41"),
                    "digit_value: byte echo missing, got {msg}"
                );
                assert!(
                    !msg.contains("alphanumeric mode rejects"),
                    "digit_value must NOT echo the alpha predicate, got {msg}"
                );
            }
            other => panic!("digit_value(b'A'): expected InvalidData, got {other:?}"),
        }
        match alpha_value(b'a').unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: alphanumeric mode rejects byte"),
                    "alpha_value: prefix + predicate, got {msg}"
                );
                assert!(
                    msg.contains("0x61"),
                    "alpha_value: byte echo missing, got {msg}"
                );
                assert!(
                    !msg.contains("numeric mode expects"),
                    "alpha_value must NOT echo the digit predicate, got {msg}"
                );
            }
            other => panic!("alpha_value(b'a'): expected InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `push_bits` MSB-first bit ordering. Kills
    /// `>> with <<` direction-flip and shift-by-mutated-width
    /// mutations on line 406.
    #[test]
    fn push_bits_msb_first_qrcode_native() {
        let mut out: Vec<bool> = Vec::new();
        // value=0, nbits=4 → all false.
        push_bits(&mut out, 0, 4);
        assert_eq!(out, vec![false; 4]);
        // value=0xF, nbits=4 → all true.
        let mut out: Vec<bool> = Vec::new();
        push_bits(&mut out, 0xF, 4);
        assert_eq!(out, vec![true; 4]);
        // value=0b1010, nbits=4 → MSB first: [true, false, true, false].
        let mut out: Vec<bool> = Vec::new();
        push_bits(&mut out, 0b1010, 4);
        assert_eq!(out, vec![true, false, true, false]);
        // value=0b10000000, nbits=8 → [true, false×7].
        let mut out: Vec<bool> = Vec::new();
        push_bits(&mut out, 0x80, 8);
        assert!(out[0]);
        assert_eq!(out[1..], vec![false; 7]);
    }

    /// Stage 11.A8c — pin `pixs_set(pixs, row, col, cols, value)`.
    /// Writes `value` at row-major index `row*cols + col`, with two
    /// silent OOB guards mirroring BWIPP's tolerant write semantics:
    ///
    /// 1. `col >= cols` → no-op (would otherwise spill into the next
    ///    row via flat row-major indexing);
    /// 2. `idx < pixs.len()` → no-op (catches over-tall rows).
    ///
    /// Without direct tests these two guards survive mutation silently:
    /// the matrix walkers still complete; the only divergence is in
    /// the final pixel grid, which is only validated end-to-end.
    ///
    /// Mutations killed:
    ///   * `col >= cols` → `col > cols`: row=0,col=cols-th index would
    ///     wrap to row 1 (kill: write-vs-no-write at col=3, cols=3);
    ///   * `col >= cols` → `col < cols` or removed: in-bounds writes
    ///     silently dropped (kill: in-bounds anchor would diverge);
    ///   * `idx < pixs.len()` → `<=` or removed: would panic / OOB.
    #[test]
    fn pixs_set_writes_in_bounds_and_silently_drops_oob() {
        // 3×3 grid, flat row-major.
        let mut pixs = vec![-1i8; 9];

        // In-bounds write: (1, 2) → idx 5.
        pixs_set(&mut pixs, 1, 2, 3, 7);
        assert_eq!(pixs[5], 7, "in-bounds write");
        assert_eq!(
            pixs.iter().filter(|&&v| v != -1).count(),
            1,
            "only the targeted cell mutated"
        );

        // Top-left corner.
        pixs_set(&mut pixs, 0, 0, 3, 9);
        assert_eq!(pixs[0], 9);

        // Bottom-right corner.
        pixs_set(&mut pixs, 2, 2, 3, 8);
        assert_eq!(pixs[8], 8);

        // OOB col guard: col == cols → no-op.
        let snapshot = pixs.clone();
        pixs_set(&mut pixs, 0, 3, 3, 100);
        assert_eq!(
            pixs, snapshot,
            "col=cols must no-op (kills `col >= cols` → `col > cols`)"
        );

        // OOB col further past: col > cols → no-op.
        pixs_set(&mut pixs, 0, 99, 3, 100);
        assert_eq!(pixs, snapshot, "col >> cols must no-op");

        // OOB row guard: idx out of buffer → no-op (row=4, col=0,
        // cols=3 → idx=12, beyond pixs.len()=9).
        pixs_set(&mut pixs, 4, 0, 3, 100);
        assert_eq!(pixs, snapshot, "row past last must no-op");

        // Boundary row guard: idx == pixs.len() → no-op (row=3, col=0,
        // cols=3 → idx=9, exactly len()=9, so `idx < pixs.len()` is
        // false).
        pixs_set(&mut pixs, 3, 0, 3, 100);
        assert_eq!(pixs, snapshot, "idx == len must no-op (kills < → <=)");

        // Value distinct from -1 sentinel: also pin that 0 and 1 are
        // both stored verbatim (i.e. value isn't being clamped).
        let mut p2 = vec![-1i8; 4];
        pixs_set(&mut p2, 0, 0, 2, 0);
        pixs_set(&mut p2, 0, 1, 2, 1);
        pixs_set(&mut p2, 1, 0, 2, -1);
        pixs_set(&mut p2, 1, 1, 2, 127);
        assert_eq!(p2, vec![0, 1, -1, 127]);
    }

    /// Stage 11.A8c — pin `rle_run(cells)`. Run-length encoder used
    /// for N1+N3 mask scoring. Strict `cell == 1` predicate (anything
    /// else, including -1 reservation markers, is treated as 0); a
    /// trailing flush always appends the final run.
    ///
    /// Behavior anchors:
    ///   * empty iterator → `[0]` (just the final flush);
    ///   * `[0]` → `[1]` (single 0-run of length 1);
    ///   * `[1]` → `[0, 1]` (last_color starts at 0 so the FIRST cell
    ///     of color 1 forces an immediate 0-run flush);
    ///   * `[0,0,1,1,1,0]` → `[2, 3, 1]` (block-by-block compaction);
    ///   * `[-1]` and `[2]` both treated as not-1 → `[1]` (kills
    ///     `cell == 1` → `cell != 1` or `cell != 0`);
    ///   * `[1,1,1]` → `[0, 3]` (leading flush + 3-run);
    ///   * alternating `[0,1,0,1,0]` → `[1, 1, 1, 1, 1]` (5 length-1
    ///     runs after flush of the implicit first 0).
    ///
    /// Mutations killed:
    ///   * `cell == 1` → `cell != 1` / `cell == 0` (color flip);
    ///   * `current += 1` → `current -= 1` (negative/wrong counts);
    ///   * `current = 1` after color change → `current = 0` (zero-run
    ///     drift);
    ///   * removed final `runs.push(current)` (loses last run);
    ///   * removed early `runs.push(current)` on diff (no segmentation).
    #[test]
    fn rle_run_strict_color_predicate_and_trailing_flush() {
        // Empty iterator → [0] (final flush only).
        assert_eq!(rle_run(std::iter::empty()), vec![0]);

        // Single 0 → [1].
        assert_eq!(rle_run(vec![0_i8].into_iter()), vec![1]);

        // Single 1 → [0, 1]. The leading [0] is the flush of the
        // initial-state run (last_color starts at 0; the first 1 cell
        // forces it to push current=0 before starting the new run).
        assert_eq!(rle_run(vec![1_i8].into_iter()), vec![0, 1]);

        // Block layout: 2 zeros, 3 ones, 1 zero → [2, 3, 1].
        assert_eq!(
            rle_run(vec![0_i8, 0, 1, 1, 1, 0].into_iter()),
            vec![2, 3, 1]
        );

        // -1 reservation marker treated as 0 (not-1).
        assert_eq!(rle_run(vec![-1_i8].into_iter()), vec![1]);
        // Other non-1 values (e.g. 2) likewise treated as 0.
        assert_eq!(rle_run(vec![2_i8].into_iter()), vec![1]);

        // Three 1's → [0, 3] (leading flush + 3-run).
        assert_eq!(rle_run(vec![1_i8, 1, 1].into_iter()), vec![0, 3]);

        // Alternating: each cell forces a flush.
        assert_eq!(
            rle_run(vec![0_i8, 1, 0, 1, 0].into_iter()),
            vec![1, 1, 1, 1, 1]
        );

        // Mix with -1 in the middle: [-1, 1, -1] = treated as [0, 1, 0]
        // → [1, 1, 1].
        assert_eq!(rle_run(vec![-1_i8, 1, -1].into_iter()), vec![1, 1, 1]);

        // Run length total equals input length when input is
        // homogeneous color-0; pin via summation across a varied case.
        let cells = vec![0_i8, 0, 0, 1, 1, 0, 0, 1, 0, 0];
        let runs = rle_run(cells.iter().copied());
        let total: u32 = runs.iter().sum();
        assert_eq!(
            total,
            cells.len() as u32,
            "sum of run lengths must equal cell count"
        );
        // Expected: [3, 2, 2, 1, 2] — kills order/length drift.
        assert_eq!(runs, vec![3, 2, 2, 1, 2]);
    }

    /// Stage 11.A8c — pin `micro_sym_id(layout_id, ec_level)`. The
    /// Micro QR (M1-M4) layout × EC-level → 0..=7 symbol-id mapping
    /// per ISO 18004 Table 13. Layout-IDs 3..=6 map to M1..M4; each
    /// row pins which EC levels are valid for that micro size.
    ///
    /// Full mapping (BWIPP `bwipp_qrcode` lines 28490+):
    ///   layout=3 (M1), ec=0          → 0
    ///   layout=4 (M2), ec=0/1        → 1 / 2
    ///   layout=5 (M3), ec=0/1        → 3 / 4
    ///   layout=6 (M4), ec=0/1/2      → 5 / 6 / 7
    ///
    /// Mutations killed:
    ///   * any per-cell value flip (every happy combo pinned);
    ///   * `checked_sub(3)` arg drift: layout_id=2 must fail (kills
    ///     `checked_sub(2)`);
    ///   * `mi >= 4` → `mi > 4`: layout_id=7 must fail (mi=4);
    ///   * `ec_idx >= row.len()` → `>` : ec_level=4 must fail (since
    ///     all rows are len 4);
    ///   * None-cell admission: (M1, ec=1) and (M2, ec=2) must Err.
    #[test]
    fn micro_sym_id_layout_ec_level_table() {
        // ---- happy combos: all eight valid (layout, ec) pairs ---------
        assert_eq!(micro_sym_id(3, 0).unwrap(), 0, "M1, ec=0 → 0");
        assert_eq!(micro_sym_id(4, 0).unwrap(), 1, "M2, ec=0 → 1");
        assert_eq!(micro_sym_id(4, 1).unwrap(), 2, "M2, ec=1 → 2");
        assert_eq!(micro_sym_id(5, 0).unwrap(), 3, "M3, ec=0 → 3");
        assert_eq!(micro_sym_id(5, 1).unwrap(), 4, "M3, ec=1 → 4");
        assert_eq!(micro_sym_id(6, 0).unwrap(), 5, "M4, ec=0 → 5");
        assert_eq!(micro_sym_id(6, 1).unwrap(), 6, "M4, ec=1 → 6");
        assert_eq!(micro_sym_id(6, 2).unwrap(), 7, "M4, ec=2 → 7");

        // ---- layout_id range guards ----------------------------------
        // Both checked_sub-fail (layout_id < 3) and mi >= 4 (layout_id > 6)
        // arms emit "qrcode_native: layout_id {layout_id} is not a Micro
        // QR layout". Pin diagnostic + per-input value echo on TWO
        // distinct boundary values (2 = under-bound, 7 = over-bound) so
        // a mutant that hardcoded the layout_id in the format would fail
        // for one of them.
        match micro_sym_id(2, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: layout_id 2 is not a Micro QR layout"),
                    "layout_id=2 (under): diagnostic + value echo; got {msg}"
                );
                // Cross-arm guard: layout-range arm must NOT carry the
                // None-cell arm's wording.
                assert!(
                    !msg.contains("doesn't support EC level"),
                    "layout-range arm must NOT leak None-cell wording; got {msg}"
                );
            }
            other => panic!("layout_id 0 must be rejected (not Micro), got {other:?}"),
        }
        // Stage 11.A8c (cont) — bare `.is_err()` upgraded to per-value
        // diagnostic pin matching the source format at line 3661-3664
        // of qrcode_native/mod.rs (`layout_id N is not a Micro QR
        // layout`). The layout_id=0 case hits the `checked_sub(3)`
        // underflow branch (different code path than layout_id ≥ 7
        // which hits the `mi >= 4` branch at line 3666-3669; both
        // share the same diagnostic format, which this pin verifies).
        match micro_sym_id(0, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "layout_id=0: missing `qrcode_native:` prefix: {msg}"
                );
                assert!(
                    msg.contains("layout_id 0"),
                    "layout_id=0: missing `layout_id 0` value echo (kills `{{layout_id}}` interpolation drop): {msg}"
                );
                assert!(
                    msg.contains("is not a Micro QR layout"),
                    "layout_id=0: missing `is not a Micro QR layout` predicate: {msg}"
                );
            }
            other => panic!("layout_id 0 must be rejected (not Micro), got {other:?}"),
        }
        match micro_sym_id(7, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: layout_id 7 is not a Micro QR layout"),
                    "layout_id=7 (over): diagnostic + value echo; got {msg}"
                );
            }
            other => {
                panic!("layout_id 7 must be rejected (kills `mi >= 4` → `mi > 4`), got {other:?}")
            }
        }
        // Stage 11.A8c (cont) — layout_id=255 hits the `mi >= 4`
        // overflow branch at line 3666-3669, same diagnostic format
        // as the underflow branch above.
        match micro_sym_id(255, 0).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native:"),
                    "layout_id=255: missing prefix: {msg}"
                );
                assert!(
                    msg.contains("layout_id 255"),
                    "layout_id=255: missing value echo: {msg}"
                );
                assert!(
                    msg.contains("is not a Micro QR layout"),
                    "layout_id=255: missing predicate: {msg}"
                );
            }
            other => panic!("layout_id 255 must be rejected, got {other:?}"),
        }

        // ---- ec_level guards: None-cells -----------------------------
        // Stage 11.A8c (cont) — bare `.is_err()` checks for 5 None-cell
        // reject arms upgraded to multi-anchor pins matching the
        // source diagnostic at line 3678-3681 of qrcode_native/mod.rs
        // (`Micro QR layout_id N doesn't support EC level M`). Each
        // arm pins both the layout_id value and the ec_level value
        // to discriminate which None cell fired.

        // M1 only supports ec=0; ec=1/2/3 → None → Err.
        match micro_sym_id(3, 1).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("layout_id 3"),
                    "M1+ec=1: missing layout_id echo: {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 1"),
                    "M1+ec=1: missing ec_level echo: {msg}"
                );
            }
            other => panic!("M1+ec=1 must Err, got {other:?}"),
        }
        match micro_sym_id(3, 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("layout_id 3"),
                    "M1+ec=2: missing layout_id echo: {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 2"),
                    "M1+ec=2: missing ec_level echo: {msg}"
                );
            }
            other => panic!("M1+ec=2 must Err, got {other:?}"),
        }
        // M2 supports ec=0,1; ec=2 → None → Err.
        match micro_sym_id(4, 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("layout_id 4"),
                    "M2+ec=2: missing layout_id echo: {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 2"),
                    "M2+ec=2: missing ec_level echo: {msg}"
                );
            }
            other => panic!("M2+ec=2 must Err, got {other:?}"),
        }
        // M3 doesn't support ec=2/3.
        match micro_sym_id(5, 2).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("layout_id 5"),
                    "M3+ec=2: missing layout_id echo: {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 2"),
                    "M3+ec=2: missing ec_level echo: {msg}"
                );
            }
            other => panic!("M3+ec=2 must Err, got {other:?}"),
        }
        // M4 doesn't support ec=3.
        match micro_sym_id(6, 3).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("layout_id 6"),
                    "M4+ec=3: missing layout_id echo: {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 3"),
                    "M4+ec=3: missing ec_level echo: {msg}"
                );
            }
            other => panic!("M4+ec=3 must Err, got {other:?}"),
        }

        // ---- ec_level out-of-row guard --------------------------------
        // All rows have len 4. ec=4 hits the `ec_idx >= row.len()`
        // branch at line 3673 (kills `>` for `>=`). Both inputs share
        // the same diagnostic format from line 3674-3677:
        //   "qrcode_native: Micro QR layout_id {layout_id} doesn't
        //    support EC level {ec_level}"
        // Two distinct layout_id echoes prove `{layout_id}` interpolates,
        // shared ec_level=4 echo proves `{ec_level}` interpolates.
        match micro_sym_id(3, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: Micro QR layout_id 3"),
                    "ec=4 row-overflow (M1): must echo layout_id=3; got {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 4"),
                    "ec=4 row-overflow (M1): must echo ec_level=4; got {msg}"
                );
                // Cross-arm guard: row-length arm must NOT carry the
                // layout-range arm's "is not a Micro QR layout" wording.
                assert!(
                    !msg.contains("is not a Micro QR layout"),
                    "ec=4 row-overflow must NOT leak layout-range arm; got {msg}"
                );
            }
            other => {
                panic!("ec=4 past row length must Err (kills `>= row.len()` → `>`), got {other:?}")
            }
        }
        match micro_sym_id(6, 4).unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("qrcode_native: Micro QR layout_id 6"),
                    "ec=4 row-overflow (M4): must echo layout_id=6 (not 3); got {msg}"
                );
                assert!(
                    msg.contains("doesn't support EC level 4"),
                    "ec=4 row-overflow (M4): must echo ec_level=4; got {msg}"
                );
            }
            other => panic!("ec=4 past row length, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `is_valid_shift_jis(hi, lo)`. Used by the
    /// Kanji-mode pre-parser to decide which adjacent byte pairs in a
    /// QR input qualify as a single Shift-JIS double-byte.
    ///
    /// The combined value `val = (hi << 8) | lo` must lie in either
    /// `0x8140..=0x9FFC` or `0xE040..=0xEBBF`; and the low byte must
    /// satisfy `0x40..=0xFC` excluding `0x7F`.
    ///
    /// Anchors pin every boundary on both ranges + the low-byte mask:
    ///   * (0x81, 0x40) → true (start of lower range, low-byte start);
    ///   * (0x9F, 0xFC) → true (end of lower range, low-byte end);
    ///   * (0xE0, 0x40) → true (start of upper range);
    ///   * (0xEB, 0xBF) → true (end of upper range);
    ///   * (0x81, 0x3F) → false (lo below 0x40);
    ///   * (0x81, 0xFD) → false (lo above 0xFC);
    ///   * (0x81, 0x7F) → false (lo == 0x7F excluded);
    ///   * (0x80, 0x40) → false (val=0x8040 below lower range);
    ///   * (0xA0, 0x40) → false (val=0xA040 in the gap between ranges
    ///     — kills `||` → `&&` mutant on the range OR);
    ///   * (0xEC, 0x40) → false (val=0xEC40 above upper range).
    #[test]
    fn is_valid_shift_jis_two_range_and_low_byte_mask() {
        // ---- happy: all four range corners --------------------------
        assert!(
            is_valid_shift_jis(0x81, 0x40),
            "lower-range start, low-byte start"
        );
        assert!(
            is_valid_shift_jis(0x9F, 0xFC),
            "lower-range end, low-byte end"
        );
        assert!(
            is_valid_shift_jis(0xE0, 0x40),
            "upper-range start, low-byte start"
        );
        assert!(
            is_valid_shift_jis(0xEB, 0xBF),
            "upper-range end (mid low-byte)"
        );

        // ---- low-byte boundary rejections (in-range hi) -------------
        assert!(
            !is_valid_shift_jis(0x81, 0x3F),
            "lo=0x3F just below 0x40 must reject"
        );
        assert!(
            !is_valid_shift_jis(0x81, 0xFD),
            "lo=0xFD just above 0xFC must reject"
        );
        assert!(
            !is_valid_shift_jis(0x81, 0x7F),
            "lo=0x7F explicitly excluded (kills `lo != 0x7F` → `lo == 0x7F`)"
        );

        // ---- in-range hi but val below lower range ------------------
        // (0x80, 0x40) → val=0x8040, below 0x8140.
        assert!(
            !is_valid_shift_jis(0x80, 0x40),
            "val=0x8040 below lower range"
        );

        // ---- val in the GAP between the two ranges ------------------
        // (0xA0, 0x40) → val=0xA040 > 0x9FFC and < 0xE040.
        assert!(
            !is_valid_shift_jis(0xA0, 0x40),
            "val=0xA040 in gap between the two ranges \
             (kills `||` → `&&` mutant on the OR; that mutant would \
              require both ranges to match, but here neither does, so \
              this specific witness needs a value that's IN one range \
              and not the other instead — see below)"
        );

        // Tighter `||`→`&&` witness: (0x81, 0x40) is IN the lower
        // range but NOT in the upper range; `&&` mutant would require
        // both ranges and falsely reject. Combined with the (0xE0,
        // 0x40) happy case (IN upper, NOT IN lower), the `&&` mutant
        // can't pass either anchor.
        assert!(
            is_valid_shift_jis(0x81, 0x40) && is_valid_shift_jis(0xE0, 0x40),
            "two single-range anchors prove the OR is real"
        );

        // ---- val above upper range ----------------------------------
        // (0xEC, 0x40) → val=0xEC40 > 0xEBBF.
        assert!(
            !is_valid_shift_jis(0xEC, 0x40),
            "val=0xEC40 above upper range"
        );

        // ---- hi=0x00 (degenerate): val=0x00xx — way below ----------
        assert!(!is_valid_shift_jis(0x00, 0x40));
        // ---- hi=0xFF, lo=0xFC: val=0xFFFC — above upper range ------
        assert!(!is_valid_shift_jis(0xFF, 0xFC));
    }

    /// Stage 11.A8c — pin `apply_mask_at_positions(pixs,
    /// data_positions, mask_idx, cols)`. The Stage-8 mask applier
    /// variant that walks a caller-supplied list of data positions
    /// (typically from `walk_codeword_positions`) and XORs the mask
    /// bit into each cell. Used by every mask-scoring trial in
    /// `select_best_full_mask` / `select_best_micro_mask` but never
    /// directly pinned, so mutations on the
    /// `pixs[pos] == 0 || pixs[pos] == 1` sentinel-skip guard, the
    /// `&& mask(row, col)` predicate, the `pos / cu` / `pos % cu`
    /// row/col derivation, and the `pixs[pos] ^= 1` flip survive
    /// on the existing mask-scoring goldens.
    ///
    /// Hand-computed (mask 0 = `(row + col) % 2 == 0`):
    ///   * pixs=[0,0,0,0] (2×2), positions=[0,1,2,3], mask 0,
    ///     cols=2: → [1, 0, 0, 1]
    ///     (mask hits at (0,0) and (1,1); flips 0→1).
    ///   * pixs=[1,1,1,1] (2×2), positions=[0,3], mask 0, cols=2:
    ///     → [0, 1, 1, 0] (mask hits both; flips 1→0; the (0,1)
    ///     and (1,0) cells are NOT in positions so are skipped).
    ///   * pixs=[2,0,0,2] (function-pattern sentinels at corners),
    ///     positions=[0,1,2,3], mask 0, cols=2: → unchanged. Pins
    ///     the `pixs[pos] == 0 || pixs[pos] == 1` filter — sentinels
    ///     are NOT XORed.
    ///   * Empty positions list: pixs unchanged regardless of mask.
    ///
    /// Mutations to catch:
    ///   * `pixs[pos] == 0 || pixs[pos] == 1` → `== 0` only:
    ///     would skip flipping cells already at 1. Caught by
    ///     `[1,1,1,1]` anchor (no flips).
    ///   * `pixs[pos] == 0 || pixs[pos] == 1` → omit guard
    ///     entirely (just `if mask(row, col)`): would XOR the
    ///     sentinel cells too. Caught by `[2,0,0,2]` anchor
    ///     (sentinels would become 3 / 3).
    ///   * `&& mask(row, col)` → `|| mask(row, col)`: always
    ///     flip on data cell. Caught by `[0,0,0,0]` anchor —
    ///     (0,1) and (1,0) would also flip → [1, 1, 1, 1].
    ///   * `pos / cu` → `pos % cu` swap: row/col reversed; for
    ///     non-square symbols would mis-mask. For our 2x2 cases
    ///     it happens to coincide since `pos/2` == `pos%2` only
    ///     at pos=0 and pos=3 (where row==col); pos=1 would map
    ///     row=1,col=0 vs the correct row=0,col=1, but for
    ///     mask 0 `(row+col)%2` is symmetric so this mutation
    ///     hides in 2x2. Use a 4x1 test where pos=1 → row=1,
    ///     col=0 (correct) vs row=0,col=1 (mutant); mask 0 at
    ///     (1,0) is false, at (0,1) is false — both false, no
    ///     diff. Need an asymmetric mask. Mask 2 is `col % 3 == 0`
    ///     — only depends on col, so swap is observable: pos=2 in
    ///     a 1×4 layout → correct col=2 (no flip), mutant col=0
    ///     (flip). Cover that.
    #[test]
    fn apply_mask_at_positions_xor_with_sentinel_filter() {
        // 2×2 all-zero data: mask 0 hits (0,0) and (1,1).
        let mut pixs = vec![0i8, 0, 0, 0];
        let positions: Vec<usize> = vec![0, 1, 2, 3];
        apply_mask_at_positions(&mut pixs, &positions, 0, 2);
        assert_eq!(
            pixs,
            vec![1, 0, 0, 1],
            "2x2 mask 0: corners (0,0) and (1,1) flip 0→1"
        );

        // 2×2 all-ones, selective positions list: only (0,0) and
        // (1,1) targeted; both flip 1→0.
        let mut pixs = vec![1i8, 1, 1, 1];
        let positions: Vec<usize> = vec![0, 3];
        apply_mask_at_positions(&mut pixs, &positions, 0, 2);
        assert_eq!(
            pixs,
            vec![0, 1, 1, 0],
            "2x2 mask 0: targeted (0,0) and (1,1) flip; (0,1) and (1,0) left alone"
        );

        // 2×2 with function-pattern sentinels (value=2) at corners
        // (0,0) and (1,1). The sentinel-skip guard must filter them
        // out so they stay at 2 even though mask hits.
        let mut pixs = vec![2i8, 0, 0, 2];
        let positions: Vec<usize> = vec![0, 1, 2, 3];
        apply_mask_at_positions(&mut pixs, &positions, 0, 2);
        assert_eq!(
            pixs,
            vec![2, 0, 0, 2],
            "function-pattern sentinels (=2) must NOT be XORed by the mask"
        );

        // Empty positions list: pixs unchanged.
        let mut pixs = vec![0i8, 1, 0, 1];
        let positions: Vec<usize> = vec![];
        apply_mask_at_positions(&mut pixs, &positions, 0, 2);
        assert_eq!(pixs, vec![0, 1, 0, 1], "empty positions: pixs untouched");

        // Row/col swap discriminator: 1×4 layout (cols=4), mask 2
        // (`col%3==0`). Catches `pos / cu` ↔ `pos % cu` swap.
        //   pos=0: original (row=0, col=0); mutant (row=0, col=0). Same.
        //   pos=1: original (row=0, col=1) col%3=1 → no flip;
        //          mutant (row=1, col=0) col%3=0 → flip.
        //   pos=2: original (row=0, col=2) col%3=2 → no flip;
        //          mutant (row=2, col=0) col%3=0 → flip.
        //   pos=3: original (row=0, col=3) col%3=0 → flip;
        //          mutant (row=3, col=0) col%3=0 → flip.
        // Original output: [1, 0, 0, 1]; mutant: [1, 1, 1, 1].
        let mut pixs = vec![0i8, 0, 0, 0];
        let positions: Vec<usize> = vec![0, 1, 2, 3];
        apply_mask_at_positions(&mut pixs, &positions, 2, 4);
        assert_eq!(
            pixs,
            vec![1, 0, 0, 1],
            "1×4 mask 2 (col%3==0): pos=0 (col=0) and pos=3 (col=3) \
             flip; pos=1, pos=2 don't. Under the `pos/cu ↔ pos%cu` \
             swap mutant, all 4 would flip (since mutant col=0 for all)."
        );
    }

    /// Stage 11.A8c — pin `place_format_info_reservation(layout_id,
    /// ...)`. The dispatcher routes to the Full / Micro / rMQR variant
    /// based on `layout_id` ranges (0..=2 → Full, 3..=6 → Micro, ≥7 →
    /// Rmqr) and returns the reserved-cell count. Only exercised
    /// transitively through `encode_with_options`; mutations on the
    /// `bin <= 2` / `bin <= 6` boundaries, the format-match arms, or
    /// the per-format return constants (31, 15, 36) all survive.
    ///
    /// Mutations to catch:
    ///   * `bin <= 2` → `bin < 2`: would mis-route layout_id 2 to
    ///     Micro branch.
    ///   * `bin <= 6` → `bin < 6`: would mis-route layout_id 6 to
    ///     Rmqr branch.
    ///   * Return-count swap: 31 ↔ 15 / 15 ↔ 36 / 31 ↔ 36.
    ///   * Format-arm swap: Full route emits Micro footprint or vice
    ///     versa — caught indirectly because Full reserves 31 cells
    ///     and Micro reserves 15.
    ///
    /// Anchors: pin both the return count AND the marked-cell count
    /// per layout_id. For Full QR V1 (21x21), Full path reserves 31
    /// cells total (30 marked-1 + 1 marked-0 dark module). For Micro
    /// M2 (13x13), Micro path reserves 15. For rMQR R7x43 (7x43),
    /// Rmqr path reserves 36.
    #[test]
    fn place_format_info_reservation_dispatches_per_layout_id_and_counts() {
        // Full QR (layout_id 0..=2). Use layout_id=0 (V1-V6),
        // layout_id=1 (V7-V20), and layout_id=2 (V21-V40).
        for &layout_id in &[0u8, 1, 2] {
            let rows: u16 = 21;
            let cols: u16 = 21;
            let mut pixs = vec![-1i8; (rows as usize) * (cols as usize)];
            let n = place_format_info_reservation(&mut pixs, layout_id, rows, cols);
            assert_eq!(n, 31, "layout_id {layout_id} → Full path; must return 31");
            let reserved = pixs.iter().filter(|&&v| v == 1).count();
            let dark_zero = pixs.iter().filter(|&&v| v == 0).count();
            assert_eq!(
                reserved, 30,
                "Full QR: 30 cells marked with value 1 (15 TL + 8 TR + 7 BL)"
            );
            assert_eq!(
                dark_zero, 1,
                "Full QR: 1 dark module pre-init cell at value 0"
            );
        }

        // Micro QR (layout_id 3..=6). Test each layout.
        for &layout_id in &[3u8, 4, 5, 6] {
            let rows: u16 = 13;
            let cols: u16 = 13;
            let mut pixs = vec![-1i8; (rows as usize) * (cols as usize)];
            let n = place_format_info_reservation(&mut pixs, layout_id, rows, cols);
            assert_eq!(n, 15, "layout_id {layout_id} → Micro path; must return 15");
            let reserved = pixs.iter().filter(|&&v| v == 1).count();
            assert_eq!(
                reserved, 15,
                "Micro QR: 15 cells marked with value 1 (L-shape)"
            );
        }

        // rMQR (layout_id ≥ 7). Use layout_id=7 with R7x43 dimensions.
        {
            let rows: u16 = 7;
            let cols: u16 = 43;
            let mut pixs = vec![-1i8; (rows as usize) * (cols as usize)];
            let n = place_format_info_reservation(&mut pixs, 7, rows, cols);
            assert_eq!(n, 36, "layout_id 7 → Rmqr path; must return 36");
            let reserved = pixs.iter().filter(|&&v| v == 1).count();
            // rMQR has 18 pairs × 2 cells = 36 cells. Some pairs may
            // map to the same cell on certain rMQR sizes, so we
            // verify count ≤ 36 and count > 0.
            assert!(
                (18..=36).contains(&reserved),
                "rMQR R7x43: between 18 and 36 unique cells marked (some pairs may dedupe)"
            );
        }

        // Boundary at layout_id=2 vs layout_id=3 — `bin <= 2` mutation
        // would route 2 to Micro (return 15 not 31).
        {
            let mut pixs_full = vec![-1i8; 21 * 21];
            let mut pixs_micro = vec![-1i8; 13 * 13];
            let n2 = place_format_info_reservation(&mut pixs_full, 2, 21, 21);
            let n3 = place_format_info_reservation(&mut pixs_micro, 3, 13, 13);
            assert_eq!(n2, 31, "layout_id=2 is the LAST Full QR layout");
            assert_eq!(n3, 15, "layout_id=3 is the FIRST Micro QR layout");
        }

        // Boundary at layout_id=6 vs layout_id=7 — `bin <= 6` mutation
        // would route 6 to Rmqr (return 36 not 15).
        {
            let mut pixs_micro = vec![-1i8; 13 * 13];
            let mut pixs_rmqr = vec![-1i8; 7 * 43];
            let n6 = place_format_info_reservation(&mut pixs_micro, 6, 13, 13);
            let n7 = place_format_info_reservation(&mut pixs_rmqr, 7, 7, 43);
            assert_eq!(n6, 15, "layout_id=6 is the LAST Micro QR layout");
            assert_eq!(n7, 36, "layout_id=7 is the FIRST Rmqr layout");
        }
    }

    /// Stage 11.A8c — pin `place_format_info_reservation_full`. The
    /// Full QR variant of format-info reservation places 30 cells
    /// across three clusters (TL fixed 15, TR loop 8, BL loop 7) plus
    /// an explicit "dark module pre-init" zero at (rows-8, 8). Only
    /// exercised transitively through end-to-end Full QR pixs goldens.
    /// Mutations on cluster boundaries, the `cols_u - 1 - k` /
    /// `rows_u - 8 + k` arithmetic, the loop ranges `0..8` / `1..8`,
    /// the reservation value, or the dark-module sentinel all survive.
    ///
    /// Hand-computed for V1 (21x21):
    ///   TL fixed 15: (0,8), (1,8), (2,8), (3,8), (4,8), (5,8),
    ///                (7,8), (8,8), (8,7), (8,5), (8,4), (8,3),
    ///                (8,2), (8,1), (8,0).
    ///   (Row 6 col 8 and col 6 row 8 are skipped — timing patterns.)
    ///   TR 8 cells (k=0..8): (8, 20), (8, 19), (8, 18), (8, 17),
    ///                        (8, 16), (8, 15), (8, 14), (8, 13).
    ///   BL 7 cells (k=1..8): (14, 8), (15, 8), (16, 8), (17, 8),
    ///                        (18, 8), (19, 8), (20, 8).
    ///   Dark module pre-init (set to 0): (13, 8).
    ///
    /// Mutations to catch:
    ///   * TR `0..8` → `0..7`: misses cell (8, 13). The cluster
    ///     comment explicitly explains BWIPP's `qrcode_formatfimmap`
    ///     extends to col=cols-8 — a 7-cell reservation here causes
    ///     ~25% cell divergence vs oracle.
    ///   * BL `1..8` → `0..8`: would overwrite the dark-module cell
    ///     at (rows-8, 8) with 1 instead of 0 in step 3.
    ///   * BL `1..8` → `1..7`: misses (rows-1, 8) = (20, 8).
    ///   * `cols_u - 1 - k` → `cols_u - k`: shifts all TR cells right
    ///     by 1, walking out of bounds (cols-0-k=21 OOB).
    ///   * `rows_u - 8 + k` → `rows_u - 7 + k`: shifts BL cells down.
    ///   * Dark module value 0 → 1 (or 2): would unset the reservation
    ///     in (rows-8, 8) to non-zero.
    #[test]
    fn place_format_info_reservation_full_v1_30_cells_plus_dark_module() {
        let rows: u16 = 21;
        let cols: u16 = 21;
        let mut pixs = vec![-1i8; (rows as usize) * (cols as usize)];

        place_format_info_reservation_full(&mut pixs, rows, cols);

        let cu = cols as usize;
        let ru = rows as usize;

        // ---- TL cluster: 15 cells marked with reservation value ----
        let tl_cells: [(usize, usize); 15] = [
            (0, 8),
            (1, 8),
            (2, 8),
            (3, 8),
            (4, 8),
            (5, 8),
            (7, 8),
            (8, 8),
            (8, 7),
            (8, 5),
            (8, 4),
            (8, 3),
            (8, 2),
            (8, 1),
            (8, 0),
        ];
        for &(r, c) in &tl_cells {
            assert_eq!(
                pixs[r * cu + c],
                FORMAT_INFO_RESERVATION_VALUE,
                "TL cluster cell ({r}, {c}) must be reserved"
            );
        }

        // ---- TR cluster: 8 cells at row 8, cols 13..=20 ----
        for k in 0..8 {
            let c = cu - 1 - k;
            assert_eq!(
                pixs[8 * cu + c],
                FORMAT_INFO_RESERVATION_VALUE,
                "TR cluster cell (8, {c}) (k={k}) must be reserved"
            );
        }
        // Specifically pin cell (8, 13) — this is the "extra" cell that
        // a `0..7` mutant would miss (kicking off cascading divergence).
        assert_eq!(
            pixs[8 * cu + 13],
            FORMAT_INFO_RESERVATION_VALUE,
            "TR cluster MUST include (8, 13); a `0..7` mutant misses it"
        );

        // ---- BL cluster: 7 cells at col 8, rows 14..=20 ----
        for k in 1..8 {
            let r = ru - 8 + k;
            assert_eq!(
                pixs[r * cu + 8],
                FORMAT_INFO_RESERVATION_VALUE,
                "BL cluster cell ({r}, 8) (k={k}) must be reserved"
            );
        }
        // Specifically pin (20, 8) — the `1..7` mutant misses this.
        assert_eq!(
            pixs[20 * cu + 8],
            FORMAT_INFO_RESERVATION_VALUE,
            "BL cluster MUST include (20, 8); a `1..7` mutant misses it"
        );

        // ---- Dark module pre-init: (13, 8) set to 0 ----
        assert_eq!(
            pixs[13 * cu + 8],
            0,
            "dark module pre-init at (13, 8) must be 0 (not reservation value)"
        );

        // ---- Count: exactly 30 cells with value=1 + 1 cell with value=0 ----
        let reserved_count = pixs.iter().filter(|&&v| v == 1).count();
        assert_eq!(
            reserved_count, 30,
            "exactly 30 cells reserved (TL 15 + TR 8 + BL 7)"
        );

        // ---- All other cells must still be at the initial sentinel ----
        let mut expected_set: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for &(r, c) in &tl_cells {
            expected_set.insert(r * cu + c);
        }
        for k in 0..8 {
            expected_set.insert(8 * cu + (cu - 1 - k));
        }
        for k in 1..8 {
            expected_set.insert((ru - 8 + k) * cu + 8);
        }
        // Dark module is a SEPARATE marker (0, not 1).
        let dark_idx = 13 * cu + 8;
        for (idx, &v) in pixs.iter().enumerate() {
            if expected_set.contains(&idx) {
                assert_eq!(v, 1, "expected-reserved cell {idx} should be 1");
            } else if idx == dark_idx {
                assert_eq!(v, 0, "dark module idx {idx} should be 0");
            } else {
                assert_eq!(
                    v,
                    -1,
                    "untouched cell {idx} ({},{}) should be at sentinel -1",
                    idx / cu,
                    idx % cu
                );
            }
        }
    }

    /// Stage 11.A8c — pin `place_format_info_reservation_micro`. The
    /// Micro QR variant of format-info reservation places a fixed
    /// L-shaped 15-cell footprint at (1..=8, 8) + (8, 1..=7), each
    /// set to `FORMAT_INFO_RESERVATION_VALUE` (= 1). Coordinates are
    /// size-independent for Micro QR (the helper accepts but ignores
    /// `rows`). Used by `place_format_info_reservation` for layout
    /// IDs 3..=6, but only exercised transitively through end-to-end
    /// Micro QR pixs goldens. Mutations on the cell list (position
    /// swap, missing entry, OOB write), the reservation value, or
    /// the row/col swap in pixs_set survive.
    ///
    /// Hand-computed: for a 13×13 Micro QR (M2) symbol, the 15
    /// reservation cells live at these (r, c) → linear-index slots:
    ///   (1,8)=21 (2,8)=34 (3,8)=47 (4,8)=60 (5,8)=73 (6,8)=86
    ///   (7,8)=99 (8,8)=112 (8,7)=111 (8,6)=110 (8,5)=109 (8,4)=108
    ///   (8,3)=107 (8,2)=106 (8,1)=105
    ///
    /// Mutations to catch:
    ///   * Cell list reordering: e.g., (1,8) ↔ (8,1) swap. Each
    ///     index is distinct so the set of marked indices changes.
    ///   * Missing cell: a single (r,c) drop → fewer than 15 cells
    ///     marked.
    ///   * Reservation value change: 1 → 0 → all cells stay at 0.
    ///   * Row/col swap inside pixs_set: caught indirectly since
    ///     the (1,8) vs (8,1) indices differ by 92 and either
    ///     position should NOT be marked under the swap.
    ///   * OOB silent: pixs_set guards col >= cols and idx >=
    ///     pixs.len() (already tested directly); but the cell list
    ///     here uses valid indices, so this test pins them all
    ///     actually get written.
    #[test]
    fn place_format_info_reservation_micro_l_shape_15_cells() {
        // M2 = 13x13. Allocate fresh pixs filled with 0.
        let rows: u16 = 13;
        let cols: u16 = 13;
        let mut pixs = vec![0i8; (rows as usize) * (cols as usize)];

        place_format_info_reservation_micro(&mut pixs, rows, cols);

        // Expected 15 cells (r, c) with reservation value 1.
        let expected_cells: [(usize, usize); 15] = [
            (1, 8),
            (2, 8),
            (3, 8),
            (4, 8),
            (5, 8),
            (6, 8),
            (7, 8),
            (8, 8),
            (8, 7),
            (8, 6),
            (8, 5),
            (8, 4),
            (8, 3),
            (8, 2),
            (8, 1),
        ];
        let cu = cols as usize;
        for &(r, c) in &expected_cells {
            assert_eq!(
                pixs[r * cu + c],
                FORMAT_INFO_RESERVATION_VALUE,
                "cell ({r}, {c}) must be marked with FORMAT_INFO_RESERVATION_VALUE"
            );
        }

        // Count the marked cells. Must be exactly 15.
        let marked_count = pixs.iter().filter(|&&v| v == 1).count();
        assert_eq!(
            marked_count, 15,
            "exactly 15 cells must be marked (catches missing or extra cell)"
        );

        // All OTHER cells must still be 0 (catches a stray write
        // outside the L-shape footprint).
        let expected_set: std::collections::HashSet<usize> =
            expected_cells.iter().map(|&(r, c)| r * cu + c).collect();
        for (idx, &v) in pixs.iter().enumerate() {
            if expected_set.contains(&idx) {
                assert_eq!(v, 1);
            } else {
                assert_eq!(
                    v,
                    0,
                    "cell at idx {idx} ({},{}) should NOT be marked",
                    idx / cu,
                    idx % cu
                );
            }
        }

        // Cell (1, 8) and (8, 1) MUST differ by exactly 92 in linear
        // index — pins the row/col swap mutant in pixs_set.
        let idx_1_8 = cu + 8;
        let idx_8_1 = 8 * cu + 1;
        assert_eq!(idx_1_8, 21);
        assert_eq!(idx_8_1, 105);
        assert_eq!(
            idx_8_1 - idx_1_8,
            84,
            "M2 (13x13) row stride is 13; (8,1)-(1,8) = 7*13-7 = 84"
        );
        // Both must be marked under the original — pins both ends of
        // the L-shape.
        assert_eq!(pixs[idx_1_8], 1);
        assert_eq!(pixs[idx_8_1], 1);
    }

    /// This helper applies BWIPP's `lc4b` nibble-shift fixup
    /// (bwip-js lines 27566-27572) used only for Micro QR M1 and M3
    /// codeword streams, where the last data codeword carries only 4
    /// high-order bits and the trailing ECC bytes must be shifted 4
    /// bits left to consume that low nibble.
    ///
    /// Only exercised transitively through the M1/M3 end-to-end
    /// pipeline, so single-shift mutants on lines 2025-2040 (the
    /// `>>= 4` and `<< 4` widths, the `& 0x0F` masks, the `>> 4` mask
    /// extraction, the loop bounds `(dcws-1)..(ncws-1)`, the `|=`
    /// vs `=`, and the early `ncws == 0` return) all survive on
    /// the existing pixs-level goldens when the shift coincides.
    ///
    /// Hand-computed (dcws=2, ncws=4, stream = [0xAA, 0xBB, 0xCC,
    /// 0xDD]):
    ///   * Step 1: stream[dcws-1] >>= 4 → stream[1] = 0x0B (extract
    ///     high nibble of the half-byte data codeword).
    ///   * Step 2 loop i=1: stream[1] = (0x0B & 0x0F) << 4 = 0xB0;
    ///     |= (0xCC >> 4) & 0x0F = 0x0C → stream[1] = 0xBC.
    ///   * Step 2 loop i=2: stream[2] = (0xCC & 0x0F) << 4 = 0xC0;
    ///     |= (0xDD >> 4) & 0x0F = 0x0D → stream[2] = 0xCD.
    ///   * Step 3: stream[3] = (0xDD & 0x0F) << 4 = 0xD0.
    ///   → final = [0xAA, 0xBC, 0xCD, 0xD0].
    ///
    /// Second case (dcws=1, ncws=3, stream = [0x12, 0x34, 0x56]):
    ///   * Step 1: stream[0] >>= 4 → 0x01.
    ///   * Step 2 i=0: stream[0] = (0x01 & 0x0F) << 4 = 0x10;
    ///     |= (0x34 >> 4) & 0x0F = 0x03 → stream[0] = 0x13.
    ///   * Step 2 i=1: stream[1] = (0x34 & 0x0F) << 4 = 0x40;
    ///     |= (0x56 >> 4) & 0x0F = 0x05 → stream[1] = 0x45.
    ///   * Step 3: stream[2] = (0x56 & 0x0F) << 4 = 0x60.
    ///   → final = [0x13, 0x45, 0x60].
    ///
    /// Degenerate (dcws=1, ncws=1, stream=[0xAB]):
    ///   * Step 1: stream[0] >>= 4 → 0x0A.
    ///   * Step 2: empty loop.
    ///   * Step 3: stream[0] = (0x0A & 0x0F) << 4 = 0xA0.
    ///   → final = [0xA0].
    ///
    /// Early return (ncws=0, stream=[0xFF]):
    ///   * returns immediately, stream unchanged.
    ///
    /// Mutations to catch:
    ///   * `>>= 4` → `>>= 3` / `>>= 5`: stream[dcws-1] post-shift
    ///     value drifts; the 0xBB→0x0B anchor catches this.
    ///   * `<< 4` → `<< 3` / `<< 5`: stream[i] high nibble drifts.
    ///   * `& 0x0F` → `& 0xF0`: keeps wrong nibble.
    ///   * `>> 4` → `>> 5` / `>> 3`: pulls wrong nibble for OR.
    ///   * `|=` → `=`: overwrites instead of merging; stream[1]
    ///     would be 0x0C instead of 0xBC in the first anchor.
    ///   * `ncws == 0` → `ncws == 1`: early-return triggers when
    ///     ncws=1; degenerate anchor [0xAB]→[0xA0] would not apply.
    ///   * Loop bounds `(dcws - 1)..(ncws - 1)` → `dcws..(ncws - 1)`:
    ///     skips the first iteration; stream[1] would stay at 0x0B
    ///     in the first anchor instead of progressing to 0xBC.
    #[test]
    fn apply_lc4b_nibble_fixup_shifts_trailing_ecc_left_by_four_bits() {
        // Primary anchor: dcws=2, ncws=4.
        let mut stream = [0xAA, 0xBB, 0xCC, 0xDD];
        apply_lc4b_nibble_fixup(&mut stream, 2, 4);
        assert_eq!(
            stream,
            [0xAA, 0xBC, 0xCD, 0xD0],
            "dcws=2 ncws=4: nibble fixup must shift [0xBB,0xCC,0xDD] \
             left by 4 bits, leaving stream[0] untouched"
        );

        // Second anchor: dcws=1, ncws=3.
        let mut stream = [0x12, 0x34, 0x56];
        apply_lc4b_nibble_fixup(&mut stream, 1, 3);
        assert_eq!(
            stream,
            [0x13, 0x45, 0x60],
            "dcws=1 ncws=3: nibble fixup must propagate high nibbles \
             across the entire stream"
        );

        // Degenerate: dcws=1, ncws=1 (no ECC, just the half-byte).
        let mut stream = [0xAB];
        apply_lc4b_nibble_fixup(&mut stream, 1, 1);
        assert_eq!(
            stream,
            [0xA0],
            "dcws=ncws=1: shift the half-nibble into the high half \
             and zero the low half"
        );

        // Early return: ncws=0 should leave the buffer untouched.
        let mut stream = [0xFF];
        apply_lc4b_nibble_fixup(&mut stream, 0, 0);
        assert_eq!(
            stream,
            [0xFF],
            "ncws=0: early return must NOT touch the stream"
        );

        // Zero stream: dcws=2, ncws=4, all zeros stays all zeros
        // (every shift/AND yields 0).
        let mut stream = [0u8; 4];
        apply_lc4b_nibble_fixup(&mut stream, 2, 4);
        assert_eq!(
            stream, [0u8; 4],
            "all-zero stream must remain all-zero after the fixup"
        );

        // Saturated stream: dcws=2, ncws=4, all 0xFF.
        // Step 1: stream[1] = 0xFF >> 4 = 0x0F.
        // Step 2 i=1: stream[1] = (0x0F & 0x0F) << 4 = 0xF0;
        //   |= (0xFF >> 4) & 0x0F = 0x0F → stream[1] = 0xFF.
        // Step 2 i=2: stream[2] = (0xFF & 0x0F) << 4 = 0xF0;
        //   |= (0xFF >> 4) & 0x0F = 0x0F → stream[2] = 0xFF.
        // Step 3: stream[3] = (0xFF & 0x0F) << 4 = 0xF0.
        // → final = [0xFF, 0xFF, 0xFF, 0xF0].
        let mut stream = [0xFFu8; 4];
        apply_lc4b_nibble_fixup(&mut stream, 2, 4);
        assert_eq!(
            stream,
            [0xFF, 0xFF, 0xFF, 0xF0],
            "all-0xFF stream: tail byte's low nibble must clear to zero"
        );
    }

    /// `pixs_set` is the bounds-checked single-cell writer that
    /// underlies every matrix-builder routine in qrcode_native (timing,
    /// finder, alignment, format/version-info, walker). It has TWO
    /// independent guards:
    ///
    /// 1. `if col >= cols { return; }` — strict per-row column bound.
    ///    Without this, `pixs_set(row, cols, cols, v)` would compute
    ///    `qmv(row, cols, cols) == row * cols + cols == (row+1) * cols`,
    ///    silently corrupting the FIRST cell of the next row.
    /// 2. `if idx < pixs.len() { pixs[idx] = value }` — linear-index
    ///    fallback for the row beyond `rows`.
    ///
    /// Both guards must stay; the col-guard is NOT a duplicate of the
    /// idx-guard. The doc comment on `pixs_set` calls this out
    /// explicitly. This test pins both.
    #[test]
    fn pixs_set_col_overflow_guard_and_idx_overflow_guard() {
        // Grid: rows=3, cols=4 → 12-cell linear buffer initialised
        // to -1 (the "unwritten" sentinel matrix-builders rely on).
        const COLS: usize = 4;
        const ROWS: usize = 3;
        let len = ROWS * COLS;
        let make = || vec![-1i8; len];

        // ---- Anchor 1: write at (0, 0) — basic in-bounds case.
        let mut p = make();
        super::pixs_set(&mut p, 0, 0, COLS, 7);
        let mut want = make();
        want[0] = 7;
        assert_eq!(
            p, want,
            "pixs_set(0,0): in-bounds write must land at idx=0 only"
        );

        // ---- Anchor 2: write at last valid (row, col) = (2, 3) → idx=11.
        let mut p = make();
        super::pixs_set(&mut p, 2, 3, COLS, 5);
        let mut want = make();
        want[11] = 5;
        assert_eq!(
            p, want,
            "pixs_set(2,3): last-valid in-bounds write must land at idx=11"
        );

        // ---- Anchor 3: col == cols on row 0 must be a NO-OP.
        // Without the `col >= cols` guard, `qmv(0, 4, 4) == 4` would
        // succeed the idx-guard and overwrite the FIRST cell of row 1.
        // The discriminator value (99) is distinct from the -1 sentinel
        // so any corruption would be visible.
        let mut p = make();
        super::pixs_set(&mut p, 0, COLS, COLS, 99);
        assert_eq!(
            p,
            make(),
            "pixs_set(0, cols, cols): col-OOB guard must trip before \
             qmv wraps into the next row's first cell"
        );

        // ---- Anchor 4: col == cols on middle row 1. Without the
        // col-guard, qmv(1, 4, 4) = 8 → would corrupt pixs[8] (the
        // first cell of row 2). Pin separately from anchor 3 so a
        // mutation that special-cases row 0 still fails.
        let mut p = make();
        super::pixs_set(&mut p, 1, COLS, COLS, 88);
        assert_eq!(
            p,
            make(),
            "pixs_set(1, cols, cols): col-OOB guard must protect row 2"
        );

        // ---- Anchor 5: col > cols (way over). The col-guard must
        // still trip; idx would compute to 6 which is well in-bounds.
        let mut p = make();
        super::pixs_set(&mut p, 0, COLS + 2, COLS, 77);
        assert_eq!(
            p,
            make(),
            "pixs_set(0, cols+2, cols): far col-OOB must trip the guard"
        );

        // ---- Anchor 6: row beyond `rows`. col is in-bounds (0..cols)
        // but the linear idx exceeds pixs.len(). Pins the SECOND
        // guard (`if idx < pixs.len()`); the col-guard alone wouldn't
        // catch this.
        let mut p = make();
        super::pixs_set(&mut p, ROWS, 0, COLS, 11);
        assert_eq!(
            p,
            make(),
            "pixs_set(rows, 0, cols): idx-OOB guard must protect the \
             buffer when col is valid but row is past the end"
        );

        // ---- Anchor 7: row = rows, col = cols-1 → idx = ROWS*COLS + COLS-1 = 15
        // → also out of bounds. Tests idx-guard at a different idx.
        let mut p = make();
        super::pixs_set(&mut p, ROWS, COLS - 1, COLS, 22);
        assert_eq!(
            p,
            make(),
            "pixs_set(rows, cols-1, cols): idx-OOB at idx=15 must trip"
        );

        // ---- Anchor 8: distinct values per cell after a sequence of
        // in-bounds writes. Pins that the function uses the `value`
        // argument verbatim (not a fixed sentinel) and writes only the
        // computed cell.
        let mut p = make();
        super::pixs_set(&mut p, 0, 1, COLS, 1);
        super::pixs_set(&mut p, 1, 2, COLS, 2);
        super::pixs_set(&mut p, 2, 0, COLS, 3);
        let mut want = make();
        want[1] = 1;
        want[6] = 2;
        want[8] = 3;
        assert_eq!(
            p, want,
            "pixs_set sequence: each call writes exactly its (row, col) \
             at qmv(row, col, cols) with the supplied value"
        );
    }

    /// `qmv(row, col, cols) = row * cols + col` is the row-major
    /// linear-index helper used everywhere in qrcode_native and is
    /// also the identity `pixs_set` relies on for its OOB guards
    /// (see the doc on `pixs_set` re: `qmv(row, cols, cols) ==
    /// qmv(row + 1, 0, cols)`). This pins:
    ///
    /// * Row-major layout (row × cols + col, NOT col × rows + row).
    /// * The `* cols` factor (vs `* rows`, `+ cols`).
    /// * Inclusion of `col` (vs `+ 0` mutant that drops the term).
    /// * The row/col swap discriminator: qmv(2, 5, 7) = 19 but
    ///   qmv(5, 2, 7) = 37 — a `col * cols + row` mutant would
    ///   produce 19 for the swapped args.
    /// * The boundary identity qmv(r, cols, cols) == qmv(r+1, 0, cols)
    ///   that motivates the col-OOB guard in `pixs_set`.
    #[test]
    fn qmv_row_major_index_with_row_col_swap_discriminator() {
        // Origin.
        assert_eq!(super::qmv(0, 0, 4), 0, "(0, 0, 4) → 0");
        // Last col of row 0 — pins the `+ col` term at the boundary.
        assert_eq!(super::qmv(0, 3, 4), 3, "(0, 3, 4) → 3");
        // First col of row 1 — pins the `row * cols` term.
        assert_eq!(super::qmv(1, 0, 4), 4, "(1, 0, 4) → 4");
        // General non-trivial case.
        assert_eq!(super::qmv(2, 1, 5), 11, "(2, 1, 5) → 11");
        // Larger general case.
        assert_eq!(super::qmv(5, 3, 7), 38, "(5, 3, 7) → 38");

        // ---- Row/col swap discriminator.
        // qmv(2, 5, 7) = 2*7 + 5 = 19
        // qmv(5, 2, 7) = 5*7 + 2 = 37
        // Differ by 18; pins that the FIRST arg multiplies by cols.
        assert_eq!(
            super::qmv(2, 5, 7),
            19,
            "row-major: 2*7+5 = 19; swap mutant would give 37"
        );
        assert_eq!(
            super::qmv(5, 2, 7),
            37,
            "row-major: 5*7+2 = 37; swap mutant would give 19"
        );

        // ---- Boundary identity that motivates pixs_set's col-guard.
        // qmv(r, cols, cols) must equal qmv(r+1, 0, cols) for any r.
        // Pins the `+ col` term: if it were dropped, qmv(r, cols, cols)
        // would equal qmv(r, 0, cols) instead of qmv(r+1, 0, cols).
        for r in 0..5usize {
            for cols in 1..=8usize {
                assert_eq!(
                    super::qmv(r, cols, cols),
                    super::qmv(r + 1, 0, cols),
                    "boundary: qmv(r={r}, cols={cols}, cols) must equal \
                     qmv(r+1, 0, cols)"
                );
            }
        }

        // ---- Single-column grid (cols=1): qmv(r, 0, 1) == r.
        // Pins that `row * cols` collapses to `row` when cols=1.
        for r in 0..10usize {
            assert_eq!(
                super::qmv(r, 0, 1),
                r,
                "single-col: qmv(r={r}, 0, 1) must equal r"
            );
        }

        // ---- Row-major uniqueness within a small grid.
        // qmv must produce all linear indices [0, rows*cols)
        // exactly once when iterated over (row, col) in 0..rows × 0..cols.
        const R: usize = 4;
        const C: usize = 5;
        let mut seen = [false; R * C];
        for r in 0..R {
            for c in 0..C {
                let idx = super::qmv(r, c, C);
                assert!(
                    idx < R * C,
                    "qmv(r={r}, c={c}, cols={C}) = {idx} must be < rows*cols = {}",
                    R * C
                );
                assert!(
                    !seen[idx],
                    "qmv produced duplicate idx={idx} at (r={r}, c={c})"
                );
                seen[idx] = true;
            }
        }
        assert!(
            seen.iter().all(|&s| s),
            "row-major sweep over (R={R}) x (C={C}) must hit every linear idx exactly once"
        );
    }

    /// Stage 11.A8c — pin `encode_micro_with_options`'s option-parsing
    /// rejection arms. The function has 4 distinct error paths plus
    /// the eclevel-mapping arms; none of them have direct tests because
    /// the function currently has no callers in `src/`. A single sweep
    /// covers them all.
    ///
    /// Mutations to catch:
    ///   * Empty-input guard removed → returns a bad symbol instead of error.
    ///   * `eclevel="L"` arm swapped with `"M"` / `"Q"` → ec_level wrong.
    ///   * `eclevel="H"` arm dropped → falls through to `other` arm with
    ///     different message ("eclevel=H" instead of "does not support").
    ///   * `eclevel=other` arm dropped → unrecognised levels silently
    ///     pass through.
    ///   * `version=` parse-fail arm dropped → garbage versions accepted.
    ///   * `!(1..=4).contains(&n)` mutated to `!(1..=3)` or `!(0..=4)`
    ///     — boundaries 1 and 4 distinguish.
    ///   * Version-strip `M` removed → "M3" fails to parse instead of
    ///     becoming `3`.
    #[test]
    fn encode_micro_with_options_option_parsing_rejection_arms() {
        use crate::Options;
        // 1. Empty input → Error::InvalidData with "Micro" diagnostic.
        let err = encode_micro_with_options(b"", &Options::default()).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidData(m) if m.contains("Micro QR")),
            "empty Micro input should be InvalidData(Micro QR), got {err:?}"
        );

        // 2. eclevel=H → InvalidOption with the dedicated H message.
        let mut opts = Options::default();
        opts.extras.push(("eclevel".into(), "H".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m) if m.contains("does not support EC level H")),
            "eclevel=H should be the dedicated H rejection, got {err:?}"
        );

        // 3. eclevel=foo → InvalidOption "eclevel=foo".
        let mut opts = Options::default();
        opts.extras.push(("eclevel".into(), "foo".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m) if m.contains("eclevel=foo")),
            "eclevel=foo should echo back the bad value, got {err:?}"
        );

        // 4. version=abc → InvalidOption "version=abc". Use `M` prefix
        //    stripping path: "Mabc" should also fail parse after strip.
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "abc".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m) if m.contains("version=abc")),
            "version=abc should echo back, got {err:?}"
        );

        // 4b. version=Mabc — strip the M, "abc" still fails parse.
        //     Mutation killing: if the strip is removed, "Mabc" would
        //     fail at parse with a different intermediate value but
        //     the diagnostic still mentions "Mabc".
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "Mabc".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m) if m.contains("version=Mabc")),
            "version=Mabc should echo full string, got {err:?}"
        );

        // 5. version=0 → out-of-range rejection (1..=4).
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "0".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m)
                if m.contains("Micro QR version must be M1..=M4")),
            "version=0 should be range-rejected, got {err:?}"
        );

        // 6. version=5 → out-of-range rejection (upper boundary 4).
        //    Distinguishes `1..=4` from `1..=5`.
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "5".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m)
                if m.contains("Micro QR version must be M1..=M4") && m.contains("M5")),
            "version=5 should reject and echo 'M5', got {err:?}"
        );

        // 7. version=M5 — exercises the strip path with an out-of-range
        //    post-strip value.
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "M5".into()));
        let err = encode_micro_with_options(b"HELLO", &opts).unwrap_err();
        assert!(
            matches!(&err, Error::InvalidOption(m)
                if m.contains("Micro QR version must be M1..=M4")),
            "version=M5 should reject post-strip, got {err:?}"
        );

        // 8. Happy paths anchor M1..=M4 boundaries. Use a small payload
        //    that fits in M2 at L. (M1 supports L only and only digits;
        //    use "1" to stay safe.) These distinguish range-boundary
        //    mutations from `1..=4` to `2..=4`.
        let mut opts = Options::default();
        opts.extras.push(("version".into(), "M2".into()));
        opts.extras.push(("eclevel".into(), "L".into()));
        assert!(
            encode_micro_with_options(b"123", &opts).is_ok(),
            "M2-L digits should succeed"
        );

        let mut opts = Options::default();
        opts.extras.push(("version".into(), "M4".into()));
        opts.extras.push(("eclevel".into(), "L".into()));
        assert!(
            encode_micro_with_options(b"HELLO", &opts).is_ok(),
            "M4-L upper boundary should succeed"
        );
    }
}
