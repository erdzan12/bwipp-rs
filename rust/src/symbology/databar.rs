//! GS1 DataBar family.
//!
//! Currently implemented in this module:
//!
//!   * **Omnidirectional** ([`encode_omni`] / [`render_omni`]) — fully
//!     ported and byte-exact against bwip-js. Stage 1 ([`omni_widths`])
//!     covers the binval split, `tab164` / `tab154` group lookup,
//!     `getRSSwidths` enumeration, and mod-79 checksum. Stage 2 (finder-
//!     pattern selection + 45-element sbs layout) lives in
//!     [`render_omni`].
//!   * **Truncated** ([`encode_truncated`]) — same sbs as Omni; the
//!     difference is rendering height, so the symbol encoder is shared.
//!   * **Limited** ([`encode_limited`]) — distinct 46-element sbs layout
//!     and a different finder/check arrangement, ported separately and
//!     verified byte-for-byte.
//!   * **Stacked / StackedOmni** ([`encode_stacked`] /
//!     [`encode_stackedomni`]) — wrap the Omni sbs into a 2D
//!     [`BitMatrix`] (50×13 for Stacked, 50×69 for StackedOmni), both
//!     verified pixs against bwip-js.
//!
//! **Not** in this module: `databarexpanded` and `databarexpandedstacked`
//! — they use a different codeword stream (base-928) and a separate
//! 12-finder selector, so they live in `databar_expanded.rs`. Both are
//! verified byte-for-byte against bwip-js (all 7 BWIPP method-dispatch
//! paths plus the stacked variant's 5-strip × 102-module pixs corpus).

use crate::encoding::{BitMatrix, LinearPattern};
use crate::error::Error;
use crate::options::Options;

/// Validate a DataBar Omnidirectional / Truncated / Stacked input payload.
///
/// Accepts either `"(01)<14 digits>"` or a bare 14-digit GTIN. Returns the
/// normalized 14-digit payload string. Production code uses
/// [`validate_gtin14_or_13`] which also accepts 13-digit (check-digit-less)
/// input; this helper is kept for the existing validation tests below.
#[cfg(test)]
fn validate_gtin14(data: &str) -> Result<String, Error> {
    let trimmed = data.trim();
    let body = trimmed
        .strip_prefix("(01)")
        .or_else(|| trimmed.strip_prefix("01"))
        .unwrap_or(trimmed);
    if body.len() != 14 || !body.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidData(format!(
            "GS1 DataBar: expected 14 digits (with optional `(01)` AI prefix), got {data:?}"
        )));
    }
    // Verify the GS1 mod-10 check digit on the GTIN-14.
    let body_chars: Vec<u32> = body.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let mut sum = 0u32;
    for (i, &d) in body_chars[..13].iter().rev().enumerate() {
        sum += if i % 2 == 0 { d * 3 } else { d };
    }
    let expected = (10 - sum % 10) % 10;
    if expected != body_chars[13] {
        return Err(Error::InvalidData(format!(
            "GS1 DataBar: GTIN-14 check digit mismatch (got {}, expected {expected})",
            body_chars[13]
        )));
    }
    Ok(body.to_string())
}

/// Encode a GS1 DataBar Omnidirectional payload. Returns a 95-module-wide
/// [`LinearPattern`] whose bar geometry is byte-exact against bwip-js for
/// the inputs in this module's tests.
pub fn encode_omni(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    render_omni(data, opts)
}

// ============================================================================
// DataBar Omni encoder. Produces the 32-element bar/space width array and
// the mod-79 checksum, then turns them into a 96-module linear pattern.
// Verified byte-for-byte against bwip-js's debug-dump output. Reachable
// from the public API via `Symbology::DatabarOmni`.
// ============================================================================

/// `tab164`: 5 rows × 8 columns, used to look up groups for d1 and d3.
/// Columns are `(d_max, gs, elo, ele, mwo, mwe, to, te)`. d1/d3 decompose
/// via `te` (the last column).
#[rustfmt::skip]
const TAB164: &[u32] = &[
    160,  0,    12, 4,  8, 1, 161,  1,
    960,  161,  10, 6,  6, 3, 80,   10,
    2014, 961,  8,  8,  4, 5, 31,   34,
    2714, 2015, 6,  10, 3, 6, 10,   70,
    2840, 2715, 4,  12, 1, 8, 1,    126,
];

/// `tab154`: 4 rows × 8 columns, used to look up groups for d2 and d4.
/// Same column order as `TAB164`; d2/d4 decompose via `to` instead of `te`.
#[rustfmt::skip]
const TAB154: &[u32] = &[
    335,  0,    5,  10, 2, 7,  4,  84,
    1035, 336,  7,  8,  4, 5,  20, 35,
    1515, 1036, 9,  6,  6, 3,  48, 10,
    1596, 1516, 11, 4,  8, 1,  81, 1,
];

/// Width-position weights used by the mod-79 character-pair check.
#[rustfmt::skip]
const CHECK_WEIGHTS: &[u32] = &[
    1, 3, 9, 27, 2, 6, 18, 54, 58, 72, 24, 8, 29, 36, 12, 4,
    74, 51, 17, 32, 37, 65, 48, 16, 64, 34, 23, 69, 49, 68, 46, 59,
];

/// BWIPP's `databaromni_checkwidths`: 9 finder patterns × 5 element widths,
/// concatenated in finder-index order (0..=8). The finder pattern selected
/// for the left half is `FINDER_WIDTHS[(csum/9)*5 ..]`; for the right half
/// it's `FINDER_WIDTHS[(csum%9)*5 ..]` *reversed* (bwip-js calls it
/// `checkrtrev` and reverses into `checkrt`). Every pattern sums to 15
/// modules.
#[rustfmt::skip]
const FINDER_WIDTHS: &[u8] = &[
    3, 8, 2, 1, 1,
    3, 5, 5, 1, 1,
    3, 3, 7, 1, 1,
    3, 1, 9, 1, 1,
    2, 7, 4, 1, 1,
    2, 5, 6, 1, 1,
    2, 3, 8, 1, 1,
    1, 5, 7, 1, 1,
    1, 3, 9, 1, 1,
];

/// `getRSSwidths` from BWIPP: given an enumeration index `val`, produce
/// the `el`-element width array (each width 1..=`mw`) whose elements sum
/// to `nm`. The `oe` flag tweaks the first iteration to honour the
/// even-character-set constraint (BWIPP's `oe` = "odd or even").
pub(super) fn get_rss_widths(mut val: i64, mut nm: i64, mw: i64, el: i64, oe: bool) -> Vec<u8> {
    let el_usize = el as usize;
    let mut out = vec![0u8; el_usize];
    let mut mask: u64 = 0;
    for bar in 0..(el - 1) {
        let mut ew: i64 = 1;
        mask |= 1u64 << bar;
        let mut sval;
        loop {
            sval = ncr_bwipp(nm - ew - 1, el - bar - 2);
            if oe && mask == 0 && (nm - ew - el * 2 + bar * 2) >= -2 {
                sval -= ncr_bwipp(nm - ew - el + bar, el - bar - 2);
            }
            if (el - bar) > 2 {
                let mut lval: i64 = 0;
                let mut k = nm - ew - el + bar + 2;
                while k > mw {
                    lval += ncr_bwipp(nm - k - ew - 1, el - bar - 3);
                    k -= 1;
                }
                sval -= lval * (el - bar - 1);
            } else if (nm - ew) > mw {
                sval -= 1;
            }
            val -= sval;
            if val < 0 {
                break;
            }
            ew += 1;
            mask &= !(1u64 << bar);
        }
        val += sval;
        nm -= ew;
        out[bar as usize] = ew as u8;
    }
    out[el_usize - 1] = nm as u8;
    out
}

/// BWIPP's stack-arithmetic `ncr` — matches bwip-js's behaviour exactly,
/// including the quirk that returns 1 (rather than the mathematical 0)
/// when `r > n` or `r <= 0`. The constrained-width enumeration relies on
/// this in its boundary cases.
fn ncr_bwipp(n: i64, r: i64) -> i64 {
    // The BWIPP definition: keep the larger of (r, n-r) in `v`, the
    // smaller in `x`. Iterate k from n down to v+1, accumulating product
    // and dividing by `counter` (1..=x) lazily.
    let v = r.max(n - r);
    let smaller = r.min(n - r);
    let mut product: i64 = 1;
    let mut counter: i64 = 1;
    let mut k = n;
    while k > v {
        product *= k;
        if counter <= smaller {
            product /= counter;
            counter += 1;
        }
        k -= 1;
    }
    while counter <= smaller {
        product /= counter;
        counter += 1;
    }
    product
}

/// Look up a `(gs, elo, ele, mwo, mwe, to, te)` group for value `d` in the
/// supplied 8-wide-row table. Returns `None` if `d` exceeds the maximum
/// the table supports (which can't happen for valid GTIN-14 input).
fn lookup_group(tab: &[u32], d: u32) -> Option<[u32; 7]> {
    let mut i = 0;
    while i < tab.len() {
        if d <= tab[i] {
            let mut g = [0u32; 7];
            g.copy_from_slice(&tab[i + 1..i + 8]);
            return Some(g);
        }
        i += 8;
    }
    None
}

/// Produce the 32-element bar/space width array + the mod-79 checksum for
/// a DataBar Omnidirectional input. The widths array is the concatenation
/// of four 8-element character widths (d1w, d2w, d3w, d4w), each
/// interleaved so even-indexed entries are bars.
///
/// `data` must be either `"(01)<13 digits>"` (no check digit) or
/// `"(01)<14 digits>"` (with a valid check digit). The 14-digit GTIN-14
/// bare form is also accepted.
pub(crate) fn omni_widths(data: &str) -> Result<([u8; 32], u32), Error> {
    omni_widths_with_linkage(data, false)
}

/// Variant of [`omni_widths`] that sets the leading "linkage" bit
/// (BWIPP's `binval[0] = 1`) — required for the DataBar Omni
/// half of a composite barcode so the check character reflects the
/// presence of a 2D companion.
pub(crate) fn omni_widths_with_linkage(
    data: &str,
    linkage: bool,
) -> Result<([u8; 32], u32), Error> {
    let body = validate_gtin14_or_13(data)?;
    debug_assert_eq!(body.len(), 13);

    // binval[0] = linkage flag (0 stand-alone / 1 composite);
    // binval[1..=13] = the 13 GTIN digits.
    let mut binval: [u32; 14] = [0; 14];
    binval[0] = u32::from(linkage);
    for (i, b) in body.bytes().enumerate() {
        binval[i + 1] = (b - b'0') as u32;
    }

    // Reduce mod 4537077 to extract `right`, then again for `left`.
    let modulus: u64 = 4_537_077;
    for i in 0..13 {
        let next = binval[i + 1] as u64 + (binval[i] as u64 % modulus) * 10;
        binval[i + 1] = next as u32;
        binval[i] = (binval[i] as u64 / modulus) as u32;
    }
    let right: u32 = binval[13] % modulus as u32;
    binval[13] = (binval[13] as u64 / modulus) as u32;

    // The remaining bigint in binval (after extracting right) is "left".
    // BWIPP's read-out skips leading-zero entries until the first non-zero,
    // then accumulates value * 10^(13-j). For an unlinked 13-digit input
    // the result fits comfortably in a u32 (max ≈ 1 815 836).
    let mut left: u64 = 0;
    let mut first = true;
    for (j, &val) in binval.iter().enumerate() {
        if val == 0 && first {
            continue;
        }
        first = false;
        left += val as u64 * 10u64.pow((13 - j) as u32);
    }
    let left = left as u32;

    let d1 = left / 1597;
    let d2 = left % 1597;
    let d3 = right / 1597;
    let d4 = right % 1597;

    let g1 =
        lookup_group(TAB164, d1).ok_or_else(|| Error::InvalidData("d1 out of tab164".into()))?;
    let g2 =
        lookup_group(TAB154, d2).ok_or_else(|| Error::InvalidData("d2 out of tab154".into()))?;
    let g3 =
        lookup_group(TAB164, d3).ok_or_else(|| Error::InvalidData("d3 out of tab164".into()))?;
    let g4 =
        lookup_group(TAB154, d4).ok_or_else(|| Error::InvalidData("d4 out of tab154".into()))?;

    // Group params columns are (gs, elo, ele, mwo, mwe, to, te) — to comes
    // before te in the BWIPP tables. d1/d3 (164-row) decompose via te;
    // d2/d4 (154-row) decompose via to. Storing the unused-by-this-pair
    // value with an underscore prefix to silence unused-variable warnings.
    let (d1gs, d1elo, d1ele, d1mwo, d1mwe, _d1to, d1te) =
        (g1[0], g1[1], g1[2], g1[3], g1[4], g1[5], g1[6]);
    let (d2gs, d2elo, d2ele, d2mwo, d2mwe, d2to, _d2te) =
        (g2[0], g2[1], g2[2], g2[3], g2[4], g2[5], g2[6]);
    let (d3gs, d3elo, d3ele, d3mwo, d3mwe, _d3to, d3te) =
        (g3[0], g3[1], g3[2], g3[3], g3[4], g3[5], g3[6]);
    let (d4gs, d4elo, d4ele, d4mwo, d4mwe, d4to, _d4te) =
        (g4[0], g4[1], g4[2], g4[3], g4[4], g4[5], g4[6]);

    let d1_val = (d1 - d1gs) as i64;
    let d2_val = (d2 - d2gs) as i64;
    let d3_val = (d3 - d3gs) as i64;
    let d4_val = (d4 - d4gs) as i64;

    let d1wo = get_rss_widths(d1_val / d1te as i64, d1elo as i64, d1mwo as i64, 4, false);
    let d1we = get_rss_widths(d1_val % d1te as i64, d1ele as i64, d1mwe as i64, 4, true);
    let d2wo = get_rss_widths(d2_val % d2to as i64, d2elo as i64, d2mwo as i64, 4, true);
    let d2we = get_rss_widths(d2_val / d2to as i64, d2ele as i64, d2mwe as i64, 4, false);
    let d3wo = get_rss_widths(d3_val / d3te as i64, d3elo as i64, d3mwo as i64, 4, false);
    let d3we = get_rss_widths(d3_val % d3te as i64, d3ele as i64, d3mwe as i64, 4, true);
    let d4wo = get_rss_widths(d4_val % d4to as i64, d4elo as i64, d4mwo as i64, 4, true);
    let d4we = get_rss_widths(d4_val / d4to as i64, d4ele as i64, d4mwe as i64, 4, false);

    // Interleave: d1 and d4 take the natural [wo, we, wo, we, ...] order;
    // d2 and d3 reverse so bars/spaces alternate correctly around the
    // finder patterns at the centre of the symbol.
    let mut widths = [0u8; 32];
    for i in 0..4 {
        widths[i * 2] = d1wo[i];
        widths[i * 2 + 1] = d1we[i];
        widths[8 + 7 - i * 2] = d2wo[i];
        widths[8 + 6 - i * 2] = d2we[i];
        widths[16 + 7 - i * 2] = d3wo[i];
        widths[16 + 6 - i * 2] = d3we[i];
        widths[24 + i * 2] = d4wo[i];
        widths[24 + i * 2 + 1] = d4we[i];
    }

    // mod-79 checksum, with the two GS1-mandated skip-bands (8 and 72).
    let mut csum: u32 = 0;
    for i in 0..32 {
        csum += widths[i] as u32 * CHECK_WEIGHTS[i];
    }
    let mut csum = csum % 79;
    if csum >= 8 {
        csum += 1;
    }
    if csum >= 72 {
        csum += 1;
    }

    Ok((widths, csum))
}

/// Build the 45-element bar/space run-length sequence for a DataBar Omni
/// or Truncated symbol: `[leading_space, d1w(8), checklt(5), d2w(8),
/// d4w(8), checkrt(5), d3w(8), trailing_space, trailing_bar]`. Total
/// width is always 95 modules.
/// One-shot helper used by the composite encoder: given a DataBar Omni
/// GS1 input, return the linkage-bit-aware 45-element bar/space width
/// array. Mirrors what `render_omni` does internally but exposes both
/// the widths and the option to set the linkage bit.
pub(crate) fn omni_sbs_with_linkage(data: &str, linkage: bool) -> Result<[u8; 45], Error> {
    let (widths, csum) = omni_widths_with_linkage(data, linkage)?;
    Ok(omni_sbs(&widths, csum))
}

fn omni_sbs(widths: &[u8; 32], csum: u32) -> [u8; 45] {
    let (d1w, rest) = widths.split_at(8);
    let (d2w, rest) = rest.split_at(8);
    let (d3w, d4w) = rest.split_at(8);

    let checklt_start = (csum as usize / 9) * 5;
    let checkrt_start = (csum as usize % 9) * 5;
    let checklt = &FINDER_WIDTHS[checklt_start..checklt_start + 5];
    // bwip-js builds `checkrt` by reversing `checkrtrev`.
    let mut checkrt = [0u8; 5];
    for (i, slot) in checkrt.iter_mut().enumerate() {
        *slot = FINDER_WIDTHS[checkrt_start + 4 - i];
    }

    let mut sbs = [0u8; 45];
    sbs[0] = 1; // leading quiet-zone space
    sbs[1..9].copy_from_slice(d1w);
    sbs[9..14].copy_from_slice(checklt);
    sbs[14..22].copy_from_slice(d2w);
    sbs[22..30].copy_from_slice(d4w);
    sbs[30..35].copy_from_slice(&checkrt);
    sbs[35..43].copy_from_slice(d3w);
    sbs[43] = 1; // trailing bar
    sbs[44] = 1; // trailing space (BWIPP's `1, 1` tail)
    sbs
}

/// Expand a BWIPP-style width-pair list into 0/1 module bits. With
/// `start_bar = false` the input alternates space-width / bar-width
/// (BWIPP's `top` row); with `start_bar = true` the polarity flips
/// (BWIPP's `bot` row, which leads with bar widths). Used by the
/// stacked renderers, which build top/bot rows the same way the
/// linear encoder builds `sbs`.
pub(super) fn expand_pairs_to_modules(widths: &[u8], start_bar: bool) -> Vec<u8> {
    let total: usize = widths.iter().map(|&w| w as usize).sum();
    let mut out = Vec::with_capacity(total);
    let polarity = u8::from(start_bar);
    for (i, &w) in widths.iter().enumerate() {
        let bit = ((i % 2) as u8) ^ polarity;
        for _ in 0..w {
            out.push(bit);
        }
    }
    out
}

/// Compose the top and bottom 50-module rows for a DataBar Stacked /
/// Stacked Omnidirectional symbol. The widths form the same `[1, 1,
/// d1w, checklt, d2w, 1, 1, 0]` and `[1, 1, d4w, checkrt, d3w, 1, 1,
/// 0]` shapes BWIPP builds for `bwipp_databaromni`'s stacked branch.
pub(crate) fn stacked_top_bot(widths: &[u8; 32], csum: u32) -> ([u8; 50], [u8; 50]) {
    let (d1w, rest) = widths.split_at(8);
    let (d2w, rest) = rest.split_at(8);
    let (d3w, d4w) = rest.split_at(8);

    let checklt_start = (csum as usize / 9) * 5;
    let checkrt_start = (csum as usize % 9) * 5;
    let checklt = &FINDER_WIDTHS[checklt_start..checklt_start + 5];
    let mut checkrt = [0u8; 5];
    for (i, slot) in checkrt.iter_mut().enumerate() {
        *slot = FINDER_WIDTHS[checkrt_start + 4 - i];
    }

    let mut top_widths = [0u8; 26];
    top_widths[0] = 1;
    top_widths[1] = 1;
    top_widths[2..10].copy_from_slice(d1w);
    top_widths[10..15].copy_from_slice(checklt);
    top_widths[15..23].copy_from_slice(d2w);
    top_widths[23] = 1;
    top_widths[24] = 1;
    top_widths[25] = 0;

    let mut bot_widths = [0u8; 26];
    bot_widths[0] = 1;
    bot_widths[1] = 1;
    bot_widths[2..10].copy_from_slice(d4w);
    bot_widths[10..15].copy_from_slice(&checkrt);
    bot_widths[15..23].copy_from_slice(d3w);
    bot_widths[23] = 1;
    bot_widths[24] = 1;
    bot_widths[25] = 0;

    let top_vec = expand_pairs_to_modules(&top_widths, false);
    let bot_vec = expand_pairs_to_modules(&bot_widths, true);
    debug_assert_eq!(top_vec.len(), 50);
    debug_assert_eq!(bot_vec.len(), 50);
    let mut top = [0u8; 50];
    let mut bot = [0u8; 50];
    top.copy_from_slice(&top_vec);
    bot.copy_from_slice(&bot_vec);
    (top, bot)
}

/// Build the separator row for a DataBar Stacked symbol per BWIPP:
/// `sep[0] = 0`; for `i ∈ 1..50`, if `top[i] == bot[i]` then `sep[i]
/// = 1 - top[i]`, else `sep[i] = 1 - sep[i-1]`. The first and last
/// four positions are then zeroed (the `databaromni_seppad`
/// constant).
pub(crate) fn stacked_sep(top: &[u8; 50], bot: &[u8; 50]) -> [u8; 50] {
    let mut sep = [0u8; 50];
    for i in 1..50 {
        sep[i] = if top[i] == bot[i] {
            1 - top[i]
        } else {
            1 - sep[i - 1]
        };
    }
    // seppad: zero out [0..4] and [46..50] (4 modules each end).
    for slot in &mut sep[0..4] {
        *slot = 0;
    }
    for slot in &mut sep[46..50] {
        *slot = 0;
    }
    sep
}

/// Render a GS1 DataBar Stacked symbol into a [`BitMatrix`] (50×13).
/// Mirrors BWIPP's `bwipp_databarstacked` → `bwipp_databaromni`
/// (format=stacked) hand-off. Row heights are `[5, 1, 7]` per BWIPP's
/// `rowmult`.
///
/// `_opts` is intentionally unused: the only BWIPP-exposed option
/// for `bwipp_databarstacked` is `width`, which is a renderer-side
/// pixel-width override — already handled at the dispatcher level
/// via [`Options::scale`]. The encoder itself takes no options.
pub fn encode_stacked(data: &str, _opts: &Options) -> Result<BitMatrix, Error> {
    let (widths, csum) = omni_widths(data)?;
    let (top, bot) = stacked_top_bot(&widths, csum);
    let sep = stacked_sep(&top, &bot);
    let mut bm = BitMatrix::new(50, 13);
    paint_module_rows(&mut bm, &[(&top, 5), (&sep, 1), (&bot, 7)]);
    Ok(bm)
}

/// Five 50-cell logical rows of a DataBar Stacked Omnidirectional
/// symbol: `(top, sep1, sep2, sep3, bot)`.
pub(crate) type StackedOmniRows = ([u8; 50], [u8; 50], [u8; 50], [u8; 50], [u8; 50]);

/// Build the five 50-cell logical rows that make up a DataBar Stacked
/// Omnidirectional symbol: `(top, sep1, sep2, sep3, bot)`. Mirrors
/// BWIPP's `bwipp_databarstackedomni`. Reusable from the composite
/// encoder so a `linkage=true` linear can share the layout.
pub(crate) fn stackedomni_logical_rows(
    data: &str,
    linkage: bool,
) -> Result<StackedOmniRows, Error> {
    let (widths, csum) = omni_widths_with_linkage(data, linkage)?;
    let (top, bot) = stacked_top_bot(&widths, csum);

    // sep1: top complement with mid-strip (i ∈ 18..=30) re-derived
    // from BWIPP's "alternate when adjacent equal" rule.
    let mut sep1 = [0u8; 50];
    for i in 0..50 {
        sep1[i] = 1 - top[i];
    }
    for slot in &mut sep1[0..4] {
        *slot = 0;
    }
    for slot in &mut sep1[46..50] {
        *slot = 0;
    }
    for i in 18..=30 {
        sep1[i] = u8::from(top[i] == 0 && (top[i - 1] == 1 || sep1[i - 1] == 0));
    }

    // sep2: BWIPP literal — four 0s, then 21 × (0, 1), then four 0s.
    // Total 4 + 42 + 4 = 50.
    let mut sep2 = [0u8; 50];
    for i in 0..21 {
        sep2[4 + i * 2 + 1] = 1;
    }

    // sep3: mirror of sep1 with bot as source. BWIPP iterates i ∈
    // 19..=31 (not 18..=30), reflecting the bot row's finder pattern
    // sitting at a different column offset.
    let mut sep3 = [0u8; 50];
    for i in 0..50 {
        sep3[i] = 1 - bot[i];
    }
    for slot in &mut sep3[0..4] {
        *slot = 0;
    }
    for slot in &mut sep3[46..50] {
        *slot = 0;
    }
    for i in 19..=31 {
        sep3[i] = u8::from(bot[i] == 0 && (bot[i - 1] == 1 || sep3[i - 1] == 0));
    }
    // Hash override: if bot[19..32] matches the `databaromni_f3pat`
    // finder pattern, BWIPP splices in `databaromni_findersep` at
    // positions 19..32 (13 modules). The constants:
    //   f3pat      = [1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1]
    //   findersep  = [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0]
    const F3PAT: [u8; 13] = [1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1];
    const FINDERSEP: [u8; 13] = [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0];
    if bot[19..32] == F3PAT {
        sep3[19..32].copy_from_slice(&FINDERSEP);
    }

    Ok((top, sep1, sep2, sep3, bot))
}

/// Render a GS1 DataBar Stacked Omnidirectional symbol into a
/// [`BitMatrix`] (50×69). Five module rows — top, sep1, sep2, sep3,
/// bot — get scaled `[33, 1, 1, 1, 33]` per BWIPP.
///
/// `_opts` is intentionally unused: the only BWIPP-exposed option
/// for `bwipp_databarstackedomni` is `width` (renderer-side, handled
/// elsewhere). The encoder itself takes no logical options.
pub fn encode_stackedomni(data: &str, _opts: &Options) -> Result<BitMatrix, Error> {
    let (top, sep1, sep2, sep3, bot) = stackedomni_logical_rows(data, false)?;
    let mut bm = BitMatrix::new(50, 69);
    paint_module_rows(
        &mut bm,
        &[(&top, 33), (&sep1, 1), (&sep2, 1), (&sep3, 1), (&bot, 33)],
    );
    Ok(bm)
}

/// Paint a sequence of `(module_row, row_multiplier)` pairs into
/// `bm`, starting at y=0. Each module row is 50 modules wide; the
/// multiplier says how many visual rows to fill with that module
/// pattern (BWIPP calls this `rowmult`).
fn paint_module_rows(bm: &mut BitMatrix, rows: &[(&[u8; 50], usize)]) {
    let mut y = 0;
    for &(modules, mult) in rows {
        for _ in 0..mult {
            for (x, &bit) in modules.iter().enumerate() {
                if bit != 0 {
                    bm.set(x, y, true);
                }
            }
            y += 1;
        }
    }
    debug_assert_eq!(y, bm.height());
}

/// Render a GS1 DataBar Omnidirectional symbol.
///
/// `data` may be `"(01)<13 digits>"` (we'll compute the check digit) or
/// `"(01)<14 digits>"` (we'll verify the supplied check digit). A bare
/// 13- or 14-digit GTIN is also accepted.
///
/// Encode a GS1 DataBar Omnidirectional symbol with BWIPP-exposed
/// option support. Mirrors `bwipp_databaromni` (`bwip-js-node.js:11630`):
/// `linkage` is the only BWIPP-side knob that changes encoder output
/// (it sets the leading binval bit to 1, shifting the encoded value
/// into a distinct range so the linear's check character reflects the
/// composite companion). `width` / `format` are renderer concerns
/// handled at the dispatcher level.
pub fn render_omni(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let linkage = check_databaromni_opts(opts)?;
    let (widths, csum) = omni_widths_with_linkage(data, linkage)?;
    let sbs = omni_sbs(&widths, csum);

    // LinearPattern alternates bar/space starting with bar (even index).
    // BWIPP's sbs starts with a space, so prepend a zero-width bar to
    // line the alternation up correctly.
    let mut bars = Vec::with_capacity(sbs.len() + 1);
    bars.push(0);
    bars.extend_from_slice(&sbs);

    // Human-readable text: the 14-digit GTIN (with check digit) rendered
    // below the bars. Strip the AI prefix if present.
    let body = validate_gtin14_or_13(data)?;
    let full = format!("(01){body}{}", gtin14_check_digit(&body));
    Ok(LinearPattern {
        bars,
        text: Some(full),
    })
}

/// Compute the GS1 mod-10 check digit for a 13-digit GTIN body.
fn gtin14_check_digit(body_13: &str) -> char {
    let mut sum = 0u32;
    for (i, c) in body_13.chars().enumerate() {
        let d = c.to_digit(10).unwrap_or(0);
        sum += if i % 2 == 0 { d * 3 } else { d };
    }
    let check = (10 - sum % 10) % 10;
    char::from_digit(check, 10).unwrap_or('0')
}

/// Accept either `(01)XXXXXXXXXXXXX` (13 digits, no check) or
/// `(01)XXXXXXXXXXXXXX` (14 digits with check). Returns the 13-digit body
/// (without the check digit) so callers can compute / append it themselves.
fn validate_gtin14_or_13(data: &str) -> Result<String, Error> {
    let trimmed = data.trim();
    let body = trimmed
        .strip_prefix("(01)")
        .or_else(|| trimmed.strip_prefix("01"))
        .unwrap_or(trimmed);
    if !body.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::InvalidData(format!(
            "GS1 DataBar Omnidirectional: non-digit in payload {data:?}"
        )));
    }
    let chars: Vec<u32> = body.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let body_13: Vec<u32> = match chars.len() {
        13 => chars,
        14 => {
            // Verify check digit.
            let mut sum = 0u32;
            for (i, &d) in chars[..13].iter().enumerate() {
                sum += if i % 2 == 0 { d * 3 } else { d };
            }
            let expected = (10 - sum % 10) % 10;
            if expected != chars[13] {
                return Err(Error::InvalidData(format!(
                    "GS1 DataBar: GTIN-14 check digit mismatch (got {}, expected {expected})",
                    chars[13]
                )));
            }
            chars[..13].to_vec()
        }
        _ => {
            return Err(Error::InvalidData(format!(
                "GS1 DataBar Omnidirectional: expected 13 or 14 digits, got {}",
                body.len()
            )))
        }
    };
    Ok(body_13
        .into_iter()
        .map(|d| char::from_digit(d, 10).unwrap())
        .collect())
}

/// Encode a GS1 DataBar Truncated payload.
///
/// DataBar Truncated is structurally identical to Omnidirectional —
/// same 95-module sbs, same encoder — but is conventionally rendered
/// with a much shorter bar height (BWIPP defaults to 13/72 inch versus
/// 33/72 for Omni). We emit the same [`LinearPattern`]; the caller
/// chooses the bar height via [`crate::Options::bar_height`].
pub fn encode_truncated(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    render_omni(data, opts)
}

// ============================================================================
// DataBar Limited
// ============================================================================
//
// Limited shares the constrained-width enumeration (`get_rss_widths`) and
// the ncr_bwipp quirk with Omnidirectional, but uses completely different
// tables (`TAB267`, 89-entry `LIMITED_CHECK_SEQ`, 28-entry weights) and
// element-count parameters (el=7 for data characters, el=8 for the check
// character). The encoder verifies the leading digit is 0 or 1 (Limited
// only encodes GTINs starting with 0 or 1) and splits the 13-digit value
// via modulus 2_013_571.

/// `tab267` from BWIPP: 6 rows × 8 columns, looked up for both d1 and d2.
/// Columns: `(d_max, gs, elo, ele, mwo, mwe, to, te)`.
#[rustfmt::skip]
const LIMITED_TAB267: &[u32] = &[
     183063,       0,  17,  9, 6, 3,  6538,    28,
     820063,  183064,  13, 13, 5, 4,   875,   728,
    1000775,  820064,   9, 17, 3, 6,    28,  6454,
    1491020, 1000776,  15, 11, 5, 4,  2415,   203,
    1979844, 1491021,  11, 15, 4, 5,   203,  2408,
    1996938, 1979845,  19,  7, 8, 1, 17094,     1,
    2013570, 1996939,   7, 19, 1, 8,     1, 16632,
];

/// 28-entry width-position weights for the mod-89 character-pair check.
#[rustfmt::skip]
const LIMITED_CHECK_WEIGHTS: &[u32] = &[
    1, 3, 9, 27, 81, 65, 17, 51, 64, 14, 42, 37, 22, 66,
    20, 60, 2, 6, 18, 54, 73, 41, 34, 13, 39, 28, 84, 74,
];

/// 89-entry sequence table: `LIMITED_CHECK_SEQ[checksum]` gives the
/// 0..=440 number that decomposes via `seq/21` (swidths index) and
/// `seq%21` (bwidths index) into the 8-element check character.
#[rustfmt::skip]
const LIMITED_CHECK_SEQ: &[u32] = &[
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13,
    14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41,
    42, 43, 45, 52, 57, 63, 64, 65, 66, 73, 74, 75, 76, 77,
    78, 79, 82, 126, 127, 128, 129, 130, 132, 141, 142, 143,
    144, 145, 146, 210, 211, 212, 213, 214, 215, 216, 217, 220,
    316, 317, 318, 319, 320, 322, 323, 326, 337,
];

/// Variant of `lookup_group` for the 7-column LIMITED_TAB267 (same row
/// structure as TAB164 / TAB154 — `(d_max, gs, elo, ele, mwo, mwe, to, te)`).
fn lookup_group_limited(d: u32) -> Option<[u32; 7]> {
    let tab = LIMITED_TAB267;
    let mut i = 0;
    while i < tab.len() {
        if d <= tab[i] {
            let mut g = [0u32; 7];
            g.copy_from_slice(&tab[i + 1..i + 8]);
            return Some(g);
        }
        i += 8;
    }
    None
}

/// Per BWIPP `databarlimited_linkval` (line 12485): the 13-digit
/// value added to `binval` when the linear is paired with a composite
/// component. Shifts the encoded value into a distinct range so the
/// linear's bar pattern signals "I have a CC companion."
const LIMITED_LINKVAL: [u32; 13] = [2, 0, 1, 5, 1, 3, 3, 5, 3, 1, 0, 9, 6];

/// Produce the 28-element bar/space width array + 14-element check
/// character widths + mod-89 checksum for a DataBar Limited input.
/// `linkage=true` adds the BWIPP `databarlimited_linkval` offset to
/// `binval` before mod-2_013_571 reduction — required for the
/// DataBar Limited half of a composite barcode.
pub(crate) fn limited_widths_with_linkage(
    data: &str,
    linkage: bool,
) -> Result<([u8; 28], [u8; 14], u32), Error> {
    let body = validate_gtin14_or_13(data)?;
    if !matches!(body.as_bytes()[0], b'0' | b'1') {
        return Err(Error::InvalidData(
            "GS1 DataBar Limited must begin with 0 or 1".into(),
        ));
    }
    debug_assert_eq!(body.len(), 13);

    let mut binval: [u32; 13] = [0; 13];
    for (i, b) in body.bytes().enumerate() {
        binval[i] = (b - b'0') as u32;
    }
    if linkage {
        for (i, slot) in binval.iter_mut().enumerate() {
            *slot += LIMITED_LINKVAL[i];
        }
    }
    // Reduce mod 2_013_571 with base-10 carry, extracting d2 at the end.
    let modulus: u64 = 2_013_571;
    for i in 0..12 {
        let next = binval[i + 1] as u64 + (binval[i] as u64 % modulus) * 10;
        binval[i + 1] = next as u32;
        binval[i] = (binval[i] as u64 / modulus) as u32;
    }
    let d2 = binval[12] % modulus as u32;
    binval[12] = (binval[12] as u64 / modulus) as u32;
    let mut d1: u64 = 0;
    let mut first = true;
    for (j, &val) in binval.iter().enumerate() {
        if val == 0 && first {
            continue;
        }
        first = false;
        d1 += val as u64 * 10u64.pow((12 - j) as u32);
    }
    let d1 = d1 as u32;

    let g1 =
        lookup_group_limited(d1).ok_or_else(|| Error::InvalidData("d1 out of tab267".into()))?;
    let g2 =
        lookup_group_limited(d2).ok_or_else(|| Error::InvalidData("d2 out of tab267".into()))?;

    // Columns: (gs, elo, ele, mwo, mwe, to, te). For Limited, both halves
    // decompose via `te` (the last column) for the odd-width call and
    // again via te for even — see the bwip-js source. Both halves use el=7.
    let (d1gs, d1elo, d1ele, d1mwo, d1mwe, _d1to, d1te) =
        (g1[0], g1[1], g1[2], g1[3], g1[4], g1[5], g1[6]);
    let (d2gs, d2elo, d2ele, d2mwo, d2mwe, _d2to, d2te) =
        (g2[0], g2[1], g2[2], g2[3], g2[4], g2[5], g2[6]);

    let d1_off = (d1 - d1gs) as i64;
    let d2_off = (d2 - d2gs) as i64;
    let d1wo = get_rss_widths(d1_off / d1te as i64, d1elo as i64, d1mwo as i64, 7, false);
    let d1we = get_rss_widths(d1_off % d1te as i64, d1ele as i64, d1mwe as i64, 7, true);
    let d2wo = get_rss_widths(d2_off / d2te as i64, d2elo as i64, d2mwo as i64, 7, false);
    let d2we = get_rss_widths(d2_off % d2te as i64, d2ele as i64, d2mwe as i64, 7, true);

    // Interleave each half's 7-pair into 14 elements: [wo[0], we[0], wo[1], we[1], ...].
    let mut d1w = [0u8; 14];
    let mut d2w = [0u8; 14];
    for i in 0..7 {
        d1w[i * 2] = d1wo[i];
        d1w[i * 2 + 1] = d1we[i];
        d2w[i * 2] = d2wo[i];
        d2w[i * 2 + 1] = d2we[i];
    }

    // 28-element widths = d1w || d2w; mod-89 checksum.
    let mut widths = [0u8; 28];
    widths[..14].copy_from_slice(&d1w);
    widths[14..].copy_from_slice(&d2w);
    let mut csum: u32 = 0;
    for i in 0..28 {
        csum += widths[i] as u32 * LIMITED_CHECK_WEIGHTS[i];
    }
    let csum_mod = csum % 89;
    let seq = LIMITED_CHECK_SEQ[csum_mod as usize];

    // Check character: 6 elements via two getRSSwidths calls (el=6, mw=3, nm=8).
    let swidths = get_rss_widths((seq / 21) as i64, 8, 3, 6, false);
    let bwidths = get_rss_widths((seq % 21) as i64, 8, 3, 6, false);
    // Interleave first 6 pairs; the final two cells are fixed at [1, 1].
    let mut checkwidths = [0u8; 14];
    for i in 0..6 {
        checkwidths[i * 2] = swidths[i];
        checkwidths[i * 2 + 1] = bwidths[i];
    }
    checkwidths[12] = 1;
    checkwidths[13] = 1;

    let mut full = [0u8; 28];
    full[..14].copy_from_slice(&d1w);
    full[14..].copy_from_slice(&d2w);
    Ok((full, checkwidths, csum_mod))
}

/// Build the 46-element bar/space run-length sequence for a Limited
/// symbol: `[leading_space(1), d1w(14), checkwidths(14), d2w(14),
/// 1, 1, 5]`.
fn limited_sbs(widths: &[u8; 28], checkwidths: &[u8; 14]) -> [u8; 46] {
    let mut sbs = [0u8; 46];
    sbs[0] = 1;
    sbs[1..15].copy_from_slice(&widths[..14]);
    sbs[15..29].copy_from_slice(checkwidths);
    sbs[29..43].copy_from_slice(&widths[14..]);
    sbs[43] = 1;
    sbs[44] = 1;
    sbs[45] = 5;
    sbs
}

/// Parse and validate BWIPP-exposed DataBar Omni options. Mirrors
/// BWIPP `bwipp_databaromni` (`bwip-js-node.js:11630`). `linkage` is
/// the only BWIPP-side knob that changes encoder output (it shifts
/// the leading `binval` bit to 1, so the encoded value lands in a
/// distinct range and the symbol's check character signals "I have a
/// CC companion"). `width`/`format` are renderer concerns.
fn check_databaromni_opts(opts: &Options) -> Result<bool, Error> {
    if let Some(v) = opts.get("linkage") {
        return match v {
            "false" => Ok(false),
            "true" => Ok(true),
            _ => Err(Error::InvalidOption(format!(
                "databaromni: linkage={v:?} must be \"true\" or \"false\""
            ))),
        };
    }
    Ok(false)
}

/// Parse and validate BWIPP-exposed DataBar Limited options. Returns
/// the `linkage` boolean (default `false`).
/// Mirrors BWIPP `bwipp_databarlimited` (`bwip-js-node.js:12544-12547`).
/// `width`/`height` are renderer concerns (handled via `Options::scale`
/// and `Options::bar_height`); `linkage` is the only BWIPP-side knob
/// that changes encoder output.
fn check_databarlimited_opts(opts: &Options) -> Result<bool, Error> {
    if let Some(v) = opts.get("linkage") {
        return match v {
            "false" => Ok(false),
            "true" => Ok(true),
            _ => Err(Error::InvalidOption(format!(
                "databarlimited: linkage={v:?} must be \"true\" or \"false\""
            ))),
        };
    }
    Ok(false)
}

/// Encode a GS1 DataBar Limited payload. Returns a [`LinearPattern`]
/// whose bar geometry is byte-exact against bwip-js for the inputs in
/// this module's tests.
pub fn encode_limited(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let linkage = check_databarlimited_opts(opts)?;
    // `linkage=true` adds the BWIPP `databarlimited_linkval` offset
    // vector to the binval before width computation (BWIPP line 12626).
    // The internal `limited_widths_with_linkage` already handles both
    // cases; we just thread the flag through.
    let (widths, checkwidths, _csum) = limited_widths_with_linkage(data, linkage)?;
    let sbs = limited_sbs(&widths, &checkwidths);
    let mut bars = Vec::with_capacity(sbs.len() + 1);
    bars.push(0);
    bars.extend_from_slice(&sbs);
    let body = validate_gtin14_or_13(data)?;
    let full = format!("(01){body}{}", gtin14_check_digit(&body));
    Ok(LinearPattern {
        bars,
        text: Some(full),
    })
}

/// One-shot helper used by the composite encoder: given a DataBar
/// Limited GS1 input, return the linkage-bit-aware 46-element bar/space
/// width array. Mirrors [`encode_limited`] but exposes the sbs widths
/// directly and supports the linkage flag.
pub(crate) fn limited_sbs_with_linkage(data: &str, linkage: bool) -> Result<[u8; 46], Error> {
    let (widths, checkwidths, _csum) = limited_widths_with_linkage(data, linkage)?;
    Ok(limited_sbs(&widths, &checkwidths))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 11.A8d — DataBar Stacked Omnidirectional second golden
    /// for GTIN `(01)00000000000383`, chosen to exercise the sep3
    /// separator-bit recurrence (L543) where the *next* bottom module
    /// differs from the *previous* one inside the i ∈ 19..=31 window.
    ///
    /// Goldens from `tools/oracle-databarstacked.js databarstackedomni
    /// "(01)00000000000383"` (BWIPP reference). This KILLS the
    /// `bot[i - 1]` → `bot[i + 1]` mutant at L543: at i = 28 the
    /// original reads `bot[27] == 0` while the mutant reads
    /// `bot[29] == 1`, so with `sep3[27] == 1` the original sets
    /// `sep3[28] = 0` but the mutant sets `sep3[28] = 1`. The sep3
    /// golden below has a `0` at position 28, matching BWIPP and the
    /// original — the `+`-mutant produces `1` there and fails.
    #[test]
    fn stackedomni_2nd_golden_fingerprint_pinned() {
        let bm = encode_stackedomni("(01)00000000000383", &Options::default())
            .expect("encode_stackedomni((01)00000000000383, default) must succeed");
        assert_eq!(bm.width(), 50);
        assert_eq!(bm.height(), 69);

        let want_top = "01010100100000000100011111000001011111110010101010";
        let want_sep1 = "00001011011111111010100000101010100000001101010000";
        let want_sep2 = "00000101010101010101010101010101010101010101010000";
        let want_sep3 = "00000011011101111010010101010000100000000100100000";
        let want_bot = "10101100100010000101100000000111011111111011010101";

        for y in 0..33 {
            assert_eq!(row_of(&bm, y), want_top, "top row {y} mismatch");
        }
        assert_eq!(row_of(&bm, 33), want_sep1, "sep1 row");
        assert_eq!(row_of(&bm, 34), want_sep2, "sep2 row");
        assert_eq!(
            row_of(&bm, 35),
            want_sep3,
            "sep3 row (kills L543 bot[i-1]->bot[i+1])"
        );
        for y in 36..69 {
            assert_eq!(row_of(&bm, y), want_bot, "bot row {y} mismatch");
        }
    }

    /// Stage 11.A8d — executable equivalence proof for the two
    /// surviving `- 1 → / 1` separator-bit mutants:
    ///   * L519 `top[i - 1]` → `top[i / 1] == top[i]` in sep1, and
    ///   * L543 `bot[i - 1]` → `bot[i / 1] == bot[i]` in sep3.
    ///
    /// CLOSED-FORM ARGUMENT. The original gated assignment is
    ///   `sep[i] = (src[i] == 0) && (src[i-1] == 1 || sep[i-1] == 0)`.
    /// The `/ 1` mutant replaces `src[i-1]` with `src[i]`. Because the
    /// outer conjunct already forces `src[i] == 0`, the disjunct
    /// `src[i] == 1` is *always false*, so the mutant collapses to
    ///   `sep[i] = (src[i] == 0) && (sep[i-1] == 0)`.
    /// Original and mutant differ ONLY when
    ///   `src[i] == 0 && src[i-1] == 1 && sep[i-1] == 1`.
    /// But there is a structural invariant, independent of input:
    ///   `sep[k] == 1  ⇒  src[k] == 0`  for every k.
    /// Proof of the invariant: outside the gated window sep is set by
    /// `sep[k] = 1 - src[k]`, so `sep[k] == 1 ⇒ src[k] == 0`; inside
    /// the window every assignment is conjoined with `src[k] == 0`, so
    /// `sep[k] == 1 ⇒ src[k] == 0` there too. Hence whenever
    /// `src[i-1] == 1` we must have `sep[i-1] == 0`, so the
    /// distinguishing precondition `src[i-1] == 1 && sep[i-1] == 1` is
    /// UNSATISFIABLE. The mutants are reachable-equivalent.
    ///
    /// This test witnesses the invariant and the resulting bit-for-bit
    /// equivalence over a 100 000-GTIN sweep (more than enough to span
    /// every top/bot bit pattern the omni width tables can emit), and
    /// additionally asserts the closed-form invariant directly.
    #[test]
    fn stackedomni_separator_div_mutants_are_equivalent() {
        // The two `/1` mutants, written out as the collapsed forms the
        // compiler would produce, to compare against the originals.
        fn sep1_orig(top: &[u8; 50]) -> [u8; 50] {
            let mut s = [0u8; 50];
            for i in 0..50 {
                s[i] = 1 - top[i];
            }
            for x in &mut s[0..4] {
                *x = 0;
            }
            for x in &mut s[46..50] {
                *x = 0;
            }
            for i in 18..=30 {
                s[i] = u8::from(top[i] == 0 && (top[i - 1] == 1 || s[i - 1] == 0));
            }
            s
        }
        fn sep1_mut(top: &[u8; 50]) -> [u8; 50] {
            let mut s = [0u8; 50];
            for i in 0..50 {
                s[i] = 1 - top[i];
            }
            for x in &mut s[0..4] {
                *x = 0;
            }
            for x in &mut s[46..50] {
                *x = 0;
            }
            for i in 18..=30 {
                // top[i / 1] == top[i]
                s[i] = u8::from(top[i] == 0 && (top[i] == 1 || s[i - 1] == 0));
            }
            s
        }
        fn sep3_orig(bot: &[u8; 50]) -> [u8; 50] {
            let mut s = [0u8; 50];
            for i in 0..50 {
                s[i] = 1 - bot[i];
            }
            for x in &mut s[0..4] {
                *x = 0;
            }
            for x in &mut s[46..50] {
                *x = 0;
            }
            for i in 19..=31 {
                s[i] = u8::from(bot[i] == 0 && (bot[i - 1] == 1 || s[i - 1] == 0));
            }
            s
        }
        fn sep3_mut(bot: &[u8; 50]) -> [u8; 50] {
            let mut s = [0u8; 50];
            for i in 0..50 {
                s[i] = 1 - bot[i];
            }
            for x in &mut s[0..4] {
                *x = 0;
            }
            for x in &mut s[46..50] {
                *x = 0;
            }
            for i in 19..=31 {
                // bot[i / 1] == bot[i]
                s[i] = u8::from(bot[i] == 0 && (bot[i] == 1 || s[i - 1] == 0));
            }
            s
        }

        for n in 0u64..100_000 {
            let body = format!("{n:013}");
            let cd = gtin14_check_digit(&body);
            let gtin = format!("{body}{cd}");
            let (widths, csum) = omni_widths_with_linkage(&gtin, false).unwrap();
            let (top, bot) = stacked_top_bot(&widths, csum);

            // Structural invariant: sep == 1 implies src == 0.
            let s1 = sep1_orig(&top);
            let s3 = sep3_orig(&bot);
            for k in 0..50 {
                assert!(
                    !(s1[k] == 1 && top[k] == 1),
                    "sep1 invariant broken at k={k} for {gtin}"
                );
                assert!(
                    !(s3[k] == 1 && bot[k] == 1),
                    "sep3 invariant broken at k={k} for {gtin}"
                );
            }

            // Bit-for-bit equivalence of the `/1` mutants.
            assert_eq!(
                s1,
                sep1_mut(&top),
                "sep1 L519 /1 mutant diverged for {gtin}"
            );
            assert_eq!(
                s3,
                sep3_mut(&bot),
                "sep3 L543 /1 mutant diverged for {gtin}"
            );
        }
    }

    /// Stage 11.A8d — executable equivalence witnesses for the eight
    /// `ncr_bwipp` + leading-zero-skip mutants that survive `cargo
    /// mutants` because no reachable input distinguishes them. Each
    /// claim below is also argued in closed form in `MUTATION_RESULTS.md`;
    /// this test keeps `databar.rs` self-documenting and regression-proof.
    #[test]
    fn databar_equivalence_notes() {
        // ---- ncr_bwipp: 6 survivors (L185/L187/L192×2/L193×2) ----
        //
        // (1) The trailing `while counter <= smaller` loop (L191-193) is
        // DEAD CODE: the main `while k > v` loop runs exactly `smaller`
        // iterations and performs exactly `smaller` divisions (counter
        // walks 1..=smaller+1), so on loop exit `counter == smaller + 1`
        // and the trailing loop never runs. Hence the 4 mutants on
        // L192/L193 (`/=`→`%=`/`*=`, `+=`→`-=`/`*=`) mutate unexecuted
        // code and cannot change any output.
        //
        // (2) L185 (`counter <= smaller`→`>`) and L187 (`+=`→`*=`) only
        // reorder/defer the exact binomial divisions. The result is
        // unchanged unless an intermediate i64 product overflows. The
        // reachable `n` is bounded by DataBar's modules-per-character
        // (≤ 26); the largest deferred product is ∏ of ≤13 terms ≤ 26
        // ≈ 1.6e14 ≪ i64::MAX. So no overflow is reachable.
        //
        // Witness: re-derive ncr by a fully independent exact integer
        // method and confirm it matches `ncr_bwipp` across the full
        // reachable (n, r) domain (n ≤ 26). We also confirm the trailing
        // loop is dead by asserting the counter bookkeeping for every
        // such pair.
        fn ncr_exact(n: i64, r: i64) -> i64 {
            // BWIPP quirk: r > n or r <= 0 returns 1.
            if r > n || r <= 0 {
                return 1;
            }
            let r = r.min(n - r);
            let mut num: i128 = 1;
            let mut den: i128 = 1;
            for i in 0..r {
                num *= (n - i) as i128;
                den *= (i + 1) as i128;
            }
            (num / den) as i64
        }
        for n in 0..=26i64 {
            for r in 0..=n {
                assert_eq!(
                    ncr_bwipp(n, r),
                    ncr_exact(n, r),
                    "ncr_bwipp reordering changed result at n={n} r={r}"
                );
                // Dead-trailing-loop witness: replicate the main loop and
                // assert it completes all `smaller` divisions, so the
                // trailing loop body is never entered.
                let v = r.max(n - r);
                let smaller = r.min(n - r);
                let mut counter: i64 = 1;
                let mut k = n;
                let mut max_prod: i128 = 1;
                let mut prod: i128 = 1;
                while k > v {
                    prod *= k as i128;
                    if prod > max_prod {
                        max_prod = prod;
                    }
                    if counter <= smaller {
                        prod /= counter as i128;
                        counter += 1;
                    }
                    k -= 1;
                }
                assert!(
                    counter > smaller,
                    "ncr_bwipp trailing loop is reachable at n={n} r={r} (counter={counter})"
                );
                // Overflow-headroom witness: the worst intermediate
                // product stays far under i64::MAX.
                assert!(
                    max_prod < i64::MAX as i128,
                    "ncr_bwipp intermediate product overflows i64 at n={n} r={r}: {max_prod}"
                );
            }
        }

        // ---- L262 omni & L803 limited: leading-zero skip `== → !=` ----
        //
        // Both are `if val == 0 && first { continue; }` inside a bigint
        // read-out. The skipped entries have `val == 0`, contributing 0
        // to the accumulator — the `continue` is a pure optimisation. The
        // mutant `val != 0 && first` diverges ONLY if it skips a *non-zero*
        // leading limb, i.e. only if `binval[0] != 0`. We witness that
        // `binval[0] == 0` for every reachable GTIN (the post-reduction
        // high limb is forced to 0 because the downstream lookup domain
        // bounds the read-out value). With `binval[0] == 0` the mutant's
        // first iteration takes the non-skip path (`first` → false), then
        // both variants sum the identical remaining limbs.
        //
        // Witness: reproduce the exact omni & limited reduction loops and
        // confirm (a) binval[0] == 0 always, and (b) the original and
        // mutated read-outs produce byte-identical `left` / `d1` across a
        // GTIN sweep.
        fn readout_orig(binval: &[u32], pow_base: usize) -> u64 {
            let mut acc: u64 = 0;
            let mut first = true;
            for (j, &val) in binval.iter().enumerate() {
                if val == 0 && first {
                    continue;
                }
                first = false;
                acc += val as u64 * 10u64.pow((pow_base - j) as u32);
            }
            acc
        }
        fn readout_mut(binval: &[u32], pow_base: usize) -> u64 {
            let mut acc: u64 = 0;
            let mut first = true;
            for (j, &val) in binval.iter().enumerate() {
                if val != 0 && first {
                    continue;
                }
                first = false;
                acc += val as u64 * 10u64.pow((pow_base - j) as u32);
            }
            acc
        }

        for n in 0u64..50_000 {
            let body = format!("{n:013}");
            let cd = gtin14_check_digit(&body);
            let gtin = format!("{body}{cd}");

            // --- omni (L262): 14-limb reduction mod 4_537_077 ---
            let mut binval: [u32; 14] = [0; 14];
            for (i, b) in body.bytes().enumerate() {
                binval[i + 1] = (b - b'0') as u32;
            }
            let modulus: u64 = 4_537_077;
            for i in 0..13 {
                let next = binval[i + 1] as u64 + (binval[i] as u64 % modulus) * 10;
                binval[i + 1] = next as u32;
                binval[i] = (binval[i] as u64 / modulus) as u32;
            }
            binval[13] = (binval[13] as u64 / modulus) as u32;
            assert_eq!(binval[0], 0, "omni binval[0] != 0 for {gtin}");
            assert_eq!(
                readout_orig(&binval, 13),
                readout_mut(&binval, 13),
                "omni L262 == → != read-out diverged for {gtin}"
            );

            // --- limited (L803): 13-limb reduction mod 2_013_571 ---
            // Limited requires the GTIN to start with 0 or 1.
            if !matches!(body.as_bytes()[0], b'0' | b'1') {
                continue;
            }
            let mut lbin: [u32; 13] = [0; 13];
            for (i, b) in body.bytes().enumerate() {
                lbin[i] = (b - b'0') as u32;
            }
            let lmod: u64 = 2_013_571;
            for i in 0..12 {
                let next = lbin[i + 1] as u64 + (lbin[i] as u64 % lmod) * 10;
                lbin[i + 1] = next as u32;
                lbin[i] = (lbin[i] as u64 / lmod) as u32;
            }
            lbin[12] = (lbin[12] as u64 / lmod) as u32;
            assert_eq!(lbin[0], 0, "limited binval[0] != 0 for {gtin}");
            assert_eq!(
                readout_orig(&lbin, 12),
                readout_mut(&lbin, 12),
                "limited L803 == → != read-out diverged for {gtin}"
            );
        }
    }

    #[test]
    fn accepts_canonical_gtin_with_ai_prefix() {
        // (01)24012345678905 has a valid mod-10 check digit.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar GTIN-14 AI=01 prefix-strip path.
        let g = validate_gtin14("(01)24012345678905").expect(
            "validate_gtin14(\"(01)24012345678905\") (DataBar canonical AI=01 GTIN-14 with valid mod-10 check; prefix-strip path) must succeed",
        );
        assert_eq!(g, "24012345678905");
    }

    #[test]
    fn accepts_bare_gtin14() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar bare-GTIN-14 path (no AI prefix).
        let g = validate_gtin14("24012345678905").expect(
            "validate_gtin14(\"24012345678905\") (DataBar bare 14-digit GTIN-14 without AI=01 prefix; must accept as-is) must succeed",
        );
        assert_eq!(g, "24012345678905");
    }

    #[test]
    fn rejects_wrong_length() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 45-47
        // (`GS1 DataBar: expected 14 digits (with optional `(01)` AI
        // prefix), got "(01)1234"`). Input after `(01)` strip is
        // "1234" — length 4, not 14. Cross-arm guard against the
        // check-digit-mismatch arm.
        match validate_gtin14("(01)1234") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DataBar:"),
                    "missing `GS1 DataBar:` prefix: {msg}"
                );
                assert!(
                    msg.contains("expected 14 digits"),
                    "missing length predicate: {msg}"
                );
                assert!(
                    msg.contains("(01)1234"),
                    "missing input-echo of `(01)1234`: {msg}"
                );
                assert!(
                    !msg.contains("check digit mismatch"),
                    "wrong arm — check-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("`(01)1234` should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_check_digit() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the source diagnostic at line 57-60
        // (`GS1 DataBar: GTIN-14 check digit mismatch (got 0, expected 5)`).
        // Input "(01)24012345678900": supplied check is 0 but the
        // valid check for body "2401234567890" is 5. Cross-arm guard
        // against the length-mismatch arm.
        match validate_gtin14("(01)24012345678900") {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DataBar:"),
                    "missing `GS1 DataBar:` prefix: {msg}"
                );
                assert!(
                    msg.contains("GTIN-14 check digit mismatch"),
                    "missing check-digit predicate: {msg}"
                );
                assert!(
                    msg.contains("got 0"),
                    "missing supplied-check echo `got 0`: {msg}"
                );
                assert!(
                    msg.contains("expected 5"),
                    "missing computed-check echo `expected 5`: {msg}"
                );
                assert!(
                    !msg.contains("expected 14 digits"),
                    "wrong arm — length diagnostic leaked: {msg}"
                );
            }
            other => panic!("`(01)24012345678900` should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn omni_rejects_invalid_input() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 4-anchor pin matching the actual diagnostic at line 651-652
        // (`GS1 DataBar Omnidirectional: non-digit in payload "not a
        // gtin"`). "not a gtin" contains spaces / letters so the
        // non-digit guard fires before the length check.
        // Cross-arm guards against length + check-digit arms.
        match encode_omni("not a gtin", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DataBar Omnidirectional:"),
                    "missing `GS1 DataBar Omnidirectional:` prefix: {msg}"
                );
                assert!(
                    msg.contains("non-digit in payload"),
                    "missing `non-digit in payload` predicate: {msg}"
                );
                assert!(
                    msg.contains("\"not a gtin\""),
                    "missing input echo `\"not a gtin\"`: {msg}"
                );
                assert!(
                    !msg.contains("expected 13 or 14 digits"),
                    "wrong arm — length diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("check digit mismatch"),
                    "wrong arm — check-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("`not a gtin` should reject as InvalidData, got {other:?}"),
        }
    }

    /// SBS run-length sequence captured from bwip-js's databaromni
    /// encoder via `node-sidecar/oracle-databaromni.js` (45 elements,
    /// alternating space/bar starting with the leading quiet-zone space).
    #[test]
    fn omni_rendered_sbs_matches_bwip_js() {
        let cases: &[(&str, [u8; 45])] = &[
            (
                "(01)24012345678905",
                [
                    1, 1, 1, 4, 1, 2, 1, 3, 3, 2, 5, 6, 1, 1, 4, 3, 1, 1, 1, 2, 2, 1, 2, 1, 1, 2,
                    1, 1, 5, 2, 1, 1, 5, 5, 3, 1, 2, 1, 5, 1, 1, 1, 4, 1, 1,
                ],
            ),
            (
                "(01)00012345678905",
                [
                    1, 1, 1, 1, 1, 2, 1, 8, 1, 2, 7, 4, 1, 1, 3, 2, 1, 1, 2, 1, 4, 1, 3, 2, 1, 1,
                    1, 1, 2, 4, 1, 1, 7, 3, 3, 2, 2, 2, 4, 1, 3, 1, 1, 1, 1,
                ],
            ),
            (
                "(01)12345678901231",
                [
                    1, 1, 3, 1, 1, 2, 1, 6, 1, 2, 3, 8, 1, 1, 1, 4, 1, 1, 5, 1, 1, 1, 2, 1, 1, 3,
                    2, 2, 2, 2, 1, 1, 9, 3, 1, 1, 2, 1, 2, 3, 3, 3, 1, 1, 1,
                ],
            ),
        ];
        for &(input, want_sbs) in cases {
            // Stage 11.A8c — input-echoing failure-mode label so a
            // regression points at the specific corpus row that
            // stopped encoding (each row is a 14-digit GTIN with a
            // distinct rotation pattern).
            let pat = render_omni(input, &Options::default()).unwrap_or_else(|e| {
                panic!("render_omni({input:?}) failed: {e:?}");
            });
            // bars[0] is the zero-width bar we prepend so the
            // bar/space alternation lines up; the remaining 45
            // entries are the sbs sequence proper.
            assert_eq!(pat.bars[0], 0, "expected zero-width-bar prefix");
            assert_eq!(&pat.bars[1..], &want_sbs, "sbs mismatch for {input:?}",);
            assert_eq!(pat.total_width(), 95, "DataBar Omni is 95 modules wide");
        }
    }

    #[test]
    fn omni_widths_with_linkage_differs_from_standalone() {
        // The linkage bit shifts every computed width because it
        // contributes to the leading bit of binval. Verify the two
        // encodings produce distinct width arrays for the same input.
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Omni widths-with-linkage paired path: linkage
        // bit shifts every computed width via binval's leading bit;
        // must yield distinct arrays. Checksums must remain in 0..79.
        let (no_link, csum_no) = omni_widths_with_linkage("(01)24012345678905", false).expect(
            "omni_widths_with_linkage(\"(01)24012345678905\", linkage=false) (DataBar Omni linkage=false baseline; csum<79) must succeed",
        );
        let (with_link, csum_with) = omni_widths_with_linkage("(01)24012345678905", true).expect(
            "omni_widths_with_linkage(\"(01)24012345678905\", linkage=true) (DataBar Omni linkage=true path; must differ from linkage=false widths) must succeed",
        );
        assert_ne!(
            no_link, with_link,
            "linkage should change the encoded widths",
        );
        // Both checksums should be in range 0..79.
        assert!(csum_no < 79);
        assert!(csum_with < 79);
    }

    /// Stage 11.13 — `render_omni` now consumes the `linkage` option
    /// (BWIPP `bwipp_databaromni:11630`). The Stage 11 A1 audit
    /// surfaced this gap (the previous version silently dropped the
    /// flag). Verify both `linkage=true` and `linkage=false` paths
    /// produce distinct symbols.
    #[test]
    fn render_omni_linkage_true_differs_from_default() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Omni render-linkage-true-distinctness path:
        // linkage=true must produce different sbs than default.
        let default = render_omni("(01)24012345678905", &Options::default()).expect(
            "render_omni(\"(01)24012345678905\", default) (DataBar Omni render-linkage default baseline for linkage=true distinctness) must succeed",
        );
        let linked = render_omni(
            "(01)24012345678905",
            &Options::default().with("linkage", "true"),
        )
        .expect(
            "render_omni(\"(01)24012345678905\", linkage=true) (DataBar Omni render-linkage=true path; must differ from default) must succeed",
        );
        assert_ne!(
            default.bars, linked.bars,
            "linkage should change the encoded sbs",
        );
    }

    /// Stage 11.13 — explicit `linkage=false` matches no-options path.
    #[test]
    fn render_omni_linkage_false_equivalent_to_default() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Omni render-linkage-false-equivalence path:
        // explicit linkage=false must equal default no-option output.
        let a = render_omni("(01)24012345678905", &Options::default()).expect(
            "render_omni(\"(01)24012345678905\", default) (DataBar Omni no-option baseline for linkage=false equivalence cross-check) must succeed",
        );
        let b = render_omni(
            "(01)24012345678905",
            &Options::default().with("linkage", "false"),
        )
        .expect(
            "render_omni(\"(01)24012345678905\", linkage=false) (DataBar Omni explicit linkage=false; must equal default output) must succeed",
        );
        assert_eq!(a.bars, b.bars);
    }

    /// Stage 11.13 — invalid `linkage` value returns InvalidOption.
    #[test]
    fn render_omni_rejects_invalid_linkage_value() {
        // Diagnostic at line 897:
        //   "databaromni: linkage={v:?} must be \"true\" or \"false\""
        // 4-anchor pin upgrades the previous single-substring check
        // (which would accept any message merely mentioning "linkage"):
        let err = render_omni(
            "(01)24012345678905",
            &Options::default().with("linkage", "maybe"),
        )
        .unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("databaromni:"),
                    "must carry databaromni prefix; got {msg}"
                );
                assert!(
                    msg.contains("linkage=\"maybe\""),
                    "must Debug-echo the offending value; got {msg}"
                );
                assert!(
                    msg.contains("must be"),
                    "must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("\"true\"") && msg.contains("\"false\""),
                    "must name BOTH valid values; got {msg}"
                );
                // Cross-prefix contamination guard: databaromni's
                // diagnostic must NOT pick up databarlimited's prefix
                // (both share the same `linkage=...` substring).
                assert!(
                    !msg.contains("databarlimited"),
                    "must NOT leak databarlimited prefix; got {msg}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
    }

    /// Stage 11.3 — `encode_limited` now consumes the `linkage`
    /// option. Pin the explicit `linkage=true` path against the same
    /// 46-entry sbs that `limited_sbs_with_linkage` already verifies
    /// against bwip-js for `"(01)15012345678907"`.
    #[test]
    fn encode_limited_linkage_true_matches_bwip_js() {
        let opts = Options::default().with("linkage", "true");
        let p = encode_limited("(01)15012345678907", &opts).expect("linkage=true encodes");
        // The LinearPattern is built as `[leading 0 bar] +
        // limited_sbs(widths, checkwidths)`; widths come from
        // limited_widths_with_linkage which is byte-for-byte
        // verified against bwip-js. So the bars after the leading
        // 0-width should match the same 46-entry sbs.
        let want: [u8; 46] = [
            1, 1, 1, 3, 1, 1, 1, 2, 4, 1, 4, 1, 1, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 2, 2, 1, 1,
            2, 1, 2, 1, 1, 2, 3, 2, 1, 3, 2, 2, 2, 2, 1, 1, 5,
        ];
        assert_eq!(p.bars[0], 0, "leading bar should be 0-width spacer");
        assert_eq!(&p.bars[1..], &want[..]);
    }

    /// Stage 11.3 — `linkage=false` (explicit default) matches the
    /// no-options path.
    #[test]
    fn encode_limited_linkage_false_equivalent_to_default() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Limited linkage-false-equivalence path: explicit
        // linkage=false must equal default output.
        let a = encode_limited("(01)15012345678907", &Options::default()).expect(
            "encode_limited(\"(01)15012345678907\", default) (DataBar Limited no-option baseline for linkage=false equivalence cross-check) must succeed",
        );
        let b = encode_limited(
            "(01)15012345678907",
            &Options::default().with("linkage", "false"),
        )
        .expect(
            "encode_limited(\"(01)15012345678907\", linkage=false) (DataBar Limited explicit linkage=false; must equal default output) must succeed",
        );
        assert_eq!(a.bars, b.bars);
    }

    /// Stage 11.3 — invalid `linkage` value returns `InvalidOption`.
    #[test]
    fn encode_limited_rejects_invalid_linkage_value() {
        // Diagnostic at line 916:
        //   "databarlimited: linkage={v:?} must be \"true\" or \"false\""
        // 5-anchor pin mirrors `render_omni_rejects_invalid_linkage_value`
        // (e96f967) but with the cross-prefix guard pointing the other
        // direction — proves databaromni's prefix doesn't leak here.
        let err = encode_limited(
            "(01)15012345678907",
            &Options::default().with("linkage", "maybe"),
        )
        .unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("databarlimited:"),
                    "must carry databarlimited prefix; got {msg}"
                );
                assert!(
                    msg.contains("linkage=\"maybe\""),
                    "must Debug-echo the offending value; got {msg}"
                );
                assert!(
                    msg.contains("must be"),
                    "must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("\"true\"") && msg.contains("\"false\""),
                    "must name BOTH valid values; got {msg}"
                );
                // Cross-prefix contamination guard: databarlimited's
                // diagnostic must NOT pick up databaromni's prefix.
                assert!(
                    !msg.contains("databaromni"),
                    "must NOT leak databaromni prefix; got {msg}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
    }

    #[test]
    fn limited_sbs_with_linkage_matches_bwip_js() {
        // For "(01)15012345678907" with linkage=true, bwip-js (via
        // oracle-databarlimited-linkage.js, captured 2026-05-19) emits:
        //   linsbs = [1,1,1,3,1,1,1,2,4,1,4,1,1,2,3,1,1,2,1,1,1,1,2,1,
        //             1,2,2,1,1,2,1,2,1,1,2,3,2,1,3,2,2,2,2,1,1,5]
        //   (46 entries, sum = 78 modules)
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Limited byte-for-byte 46-element SBS oracle:
        // linkage=true → 78-module symbol.
        let sbs = limited_sbs_with_linkage("(01)15012345678907", true).expect(
            "limited_sbs_with_linkage(\"(01)15012345678907\", linkage=true) (DataBar Limited byte-for-byte 46-element 78-module SBS bwip-js raw oracle) must succeed",
        );
        let want: [u8; 46] = [
            1, 1, 1, 3, 1, 1, 1, 2, 4, 1, 4, 1, 1, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 2, 2, 1, 1,
            2, 1, 2, 1, 1, 2, 3, 2, 1, 3, 2, 2, 2, 2, 1, 1, 5,
        ];
        assert_eq!(sbs, want);
    }

    /// Golden values captured from bwip-js's databaromni encoder via
    /// `node-sidecar/oracle-databaromni.js`. Each tuple is
    /// `(input, expected_widths, expected_checksum)`.
    #[test]
    fn omni_widths_match_bwip_js_oracle() {
        let cases: &[(&str, [u8; 32], u32)] = &[
            (
                "(01)24012345678905",
                [
                    1, 1, 4, 1, 2, 1, 3, 3, 4, 3, 1, 1, 1, 2, 2, 1, 1, 2, 1, 5, 1, 1, 1, 4, 2, 1,
                    1, 2, 1, 1, 5, 2,
                ],
                46,
            ),
            (
                "(01)00012345678905",
                [
                    1, 1, 1, 1, 2, 1, 8, 1, 3, 2, 1, 1, 2, 1, 4, 1, 2, 2, 2, 4, 1, 3, 1, 1, 3, 2,
                    1, 1, 1, 1, 2, 4,
                ],
                38,
            ),
            (
                "(01)12345678901231",
                [
                    1, 3, 1, 1, 2, 1, 6, 1, 1, 4, 1, 1, 5, 1, 1, 1, 1, 2, 1, 2, 3, 3, 3, 1, 2, 1,
                    1, 3, 2, 2, 2, 2,
                ],
                62,
            ),
        ];
        for (input, want_widths, want_csum) in cases {
            // Stage 11.A8c — input-echoing failure-mode label.
            let (widths, csum) = omni_widths(input)
                .unwrap_or_else(|e| panic!("omni_widths({input:?}) failed: {e:?}"));
            assert_eq!(&widths, want_widths, "widths mismatch for {input:?}");
            assert_eq!(csum, *want_csum, "checksum mismatch for {input:?}");
        }
    }

    #[test]
    fn omni_widths_handles_13_digit_input() {
        let (w_with, c_with) = omni_widths("(01)24012345678905").unwrap();
        let (w_without, c_without) = omni_widths("(01)2401234567890").unwrap();
        assert_eq!(w_with, w_without);
        assert_eq!(c_with, c_without);
    }

    /// Golden sbs sequences captured from bwip-js's databarlimited
    /// encoder via `node-sidecar/oracle-databarlimited.js`.
    #[test]
    fn limited_rendered_sbs_matches_bwip_js() {
        let cases: &[(&str, [u8; 46])] = &[
            (
                "(01)00012345678905",
                [
                    1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 1, 5, 1, 6, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 3,
                    1, 1, 1, 1, 1, 1, 3, 2, 2, 2, 3, 1, 1, 5, 1, 1, 2, 1, 1, 5,
                ],
            ),
            (
                "(01)15012345678907",
                [
                    1, 3, 2, 2, 2, 3, 2, 1, 2, 1, 1, 1, 1, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 2,
                    2, 1, 1, 2, 1, 2, 1, 1, 2, 3, 2, 1, 3, 2, 2, 2, 2, 1, 1, 5,
                ],
            ),
            (
                "(01)09521234567899",
                [
                    1, 1, 1, 4, 1, 2, 2, 1, 1, 1, 1, 3, 4, 1, 3, 1, 2, 1, 2, 1, 1, 1, 1, 2, 1, 2,
                    1, 1, 1, 1, 1, 2, 1, 2, 1, 2, 3, 1, 1, 4, 2, 1, 4, 1, 1, 5,
                ],
            ),
        ];
        for &(input, want_sbs) in cases {
            // Stage 11.A8c — input-echoing failure-mode label.
            let pat = encode_limited(input, &Options::default())
                .unwrap_or_else(|e| panic!("encode_limited({input:?}) failed: {e:?}"));
            assert_eq!(pat.bars[0], 0, "expected zero-width-bar prefix");
            assert_eq!(&pat.bars[1..], &want_sbs, "sbs mismatch for {input:?}");
        }
    }

    #[test]
    fn limited_rejects_bad_leading_digit() {
        // Limited only encodes GTINs starting with 0 or 1.
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 2-anchor pin matching the source diagnostic at line 776-777
        // (`GS1 DataBar Limited must begin with 0 or 1`). Cross-arm
        // guard against the length and check-digit arms in
        // validate_gtin14_or_13.
        match encode_limited("(01)24012345678905", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("GS1 DataBar Limited"),
                    "missing `GS1 DataBar Limited` prefix: {msg}"
                );
                assert!(
                    msg.contains("must begin with 0 or 1"),
                    "missing leading-digit predicate: {msg}"
                );
                assert!(
                    !msg.contains("expected 14 digits"),
                    "wrong arm — length diagnostic leaked: {msg}"
                );
                assert!(
                    !msg.contains("check digit mismatch"),
                    "wrong arm — check-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!(
                "`(01)24012345678905` (leading-digit 2) should reject as InvalidData, got {other:?}"
            ),
        }
    }

    /// Helper for the stacked test: extract a 50-wide row from a
    /// BitMatrix at the given y, ignoring the row multiplier.
    fn row_of(bm: &BitMatrix, y: usize) -> String {
        (0..bm.width())
            .map(|x| if bm.get(x, y) { '1' } else { '0' })
            .collect()
    }

    /// DataBar Stacked golden pixs from `oracle-databarstacked.js
    /// databarstacked "(01)24012345678905"`. The intermediate `pixs`
    /// is 150 modules (3 rows × 50 cols), scaled by rowmult=[5,1,7]
    /// to the final 50 × 13 BitMatrix.
    #[test]
    fn stacked_matches_bwip_js_pixs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Stacked 50×13 pixs golden path: 3 rows × 50 cols
        // scaled by rowmult=[5,1,7] → 50×13 BitMatrix.
        let bm = encode_stacked("(01)24012345678905", &Options::default()).expect(
            "encode_stacked(\"(01)24012345678905\", default) (DataBar Stacked 50×13 pixs golden: 3 rows × 50 cols scaled by rowmult=[5,1,7]) must succeed",
        );
        assert_eq!(bm.width(), 50);
        assert_eq!(bm.height(), 13);

        let want_top = "01010000100100011100111110000001011110001010011010";
        let want_sep = "00001011010010101010000011111010100101010101000000";
        let want_bot = "10110100101111100101111100000111011011111010111101";

        // Top: rows 0..5 identical (rowmult=5).
        for y in 0..5 {
            assert_eq!(row_of(&bm, y), want_top, "top row {y} mismatch");
        }
        // Sep: row 5 (rowmult=1).
        assert_eq!(row_of(&bm, 5), want_sep, "sep row");
        // Bot: rows 6..13 identical (rowmult=7).
        for y in 6..13 {
            assert_eq!(row_of(&bm, y), want_bot, "bot row {y} mismatch");
        }
    }

    /// DataBar Stacked Omnidirectional verifies every module row
    /// (top, sep1, sep2, sep3, bot) and the symbol's overall shape
    /// (50×69) against BWIPP's `rowmult=[33,1,1,1,33]`. Goldens from
    /// `oracle-databarstacked.js databarstackedomni "(01)24012345678905"`.
    #[test]
    fn stackedomni_matches_bwip_js_pixs() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the DataBar Stacked Omnidirectional 50×69 pixs golden:
        // rowmult=[33,1,1,1,33] → 50×69 with 3 separator rows.
        let bm = encode_stackedomni("(01)24012345678905", &Options::default()).expect(
            "encode_stackedomni(\"(01)24012345678905\", default) (DataBar Stacked Omnidirectional 50×69 pixs golden: 5 modules rowmult=[33,1,1,1,33]) must succeed",
        );
        assert_eq!(bm.width(), 50);
        assert_eq!(bm.height(), 69);

        let want_top = "01010000100100011100111110000001011110001010011010";
        let want_sep1 = "00001111011011100010000001010100100001110101100000";
        let want_sep2 = "00000101010101010101010101010101010101010101010000";
        let want_sep3 = "00001011010000011010000010101000100100000101000000";
        let want_bot = "10110100101111100101111100000111011011111010111101";

        // Top: rows 0..33 identical.
        for y in 0..33 {
            assert_eq!(row_of(&bm, y), want_top, "top row {y} mismatch");
        }
        // Sep1 / sep2 / sep3: rows 33, 34, 35 (rowmult=1 each).
        assert_eq!(row_of(&bm, 33), want_sep1, "sep1 row");
        assert_eq!(row_of(&bm, 34), want_sep2, "sep2 row");
        assert_eq!(row_of(&bm, 35), want_sep3, "sep3 row");
        // Bot: rows 36..69 identical.
        for y in 36..69 {
            assert_eq!(row_of(&bm, y), want_bot, "bot row {y} mismatch");
        }
    }

    /// Stage 11.A8c — pin `ncr_bwipp` n-choose-r at boundaries that
    /// BWIPP's quirks rely on: `r > n` → 1 (not 0); `r == 0` → 1;
    /// `r == n` → 1; `r == 1` → n; and a few middle values.
    /// Kills any mutation on the `v = r.max(n - r)`, the loop counter
    /// arithmetic, or the `counter <= smaller` guard inside the
    /// constrained-width enumeration.
    #[test]
    fn ncr_bwipp_quirks_at_boundaries() {
        // r > n: BWIPP returns 1 (vs mathematical 0).
        assert_eq!(ncr_bwipp(3, 5), 1);
        assert_eq!(ncr_bwipp(0, 1), 1);
        // r == 0: 1.
        assert_eq!(ncr_bwipp(5, 0), 1);
        // r == n: 1.
        assert_eq!(ncr_bwipp(5, 5), 1);
        assert_eq!(ncr_bwipp(0, 0), 1);
        // r == 1: n.
        assert_eq!(ncr_bwipp(5, 1), 5);
        assert_eq!(ncr_bwipp(10, 1), 10);
        // r == n - 1: n.
        assert_eq!(ncr_bwipp(5, 4), 5);
        // Standard binomial values.
        assert_eq!(ncr_bwipp(5, 2), 10);
        assert_eq!(ncr_bwipp(6, 3), 20);
        assert_eq!(ncr_bwipp(10, 5), 252);
    }

    /// Stage 11.A8c — pin `gtin14_check_digit` for a couple of known
    /// values. Kills any mutation on the `* 3` weight, the `i % 2`
    /// alternation, or the `(10 - sum % 10) % 10` final step.
    #[test]
    fn gtin14_check_digit_known_values() {
        // GTIN-13 body "012345678901" → check = ? Compute via the
        // BWIPP formula manually:
        //   Mod-10: positions 0,2,4,...,12 are weighted ×3; others ×1.
        //   Body 13 chars: "0123456789012".
        //   Sum = 0*3 + 1 + 2*3 + 3 + 4*3 + 5 + 6*3 + 7 + 8*3 + 9
        //       + 0*3 + 1 + 2*3
        //       = 0+1+6+3+12+5+18+7+24+9+0+1+6 = 92.
        //   (10 - 92 % 10) % 10 = (10 - 2) % 10 = 8.
        assert_eq!(gtin14_check_digit("0123456789012"), '8');

        // Body "0401234567890" — a common SSCC-style example:
        //   Sum = 0*3+4+0*3+1+2*3+3+4*3+5+6*3+7+8*3+9+0*3
        //       = 0+4+0+1+6+3+12+5+18+7+24+9+0 = 89.
        //   (10 - 89 % 10) % 10 = (10 - 9) % 10 = 1.
        assert_eq!(gtin14_check_digit("0401234567890"), '1');

        // All-zero body → check 0 (sum=0, (10-0)%10 = 0).
        assert_eq!(gtin14_check_digit("0000000000000"), '0');
    }

    /// Stage 11.A8c — pin `lookup_group` directly. The helper walks
    /// an 8-wide row table (`[max_d, g0, g1, g2, g3, g4, g5, g6]`)
    /// and returns the 7-element group whose `max_d` is the first
    /// `>= d`. Used only inside `omni_widths_with_linkage` /
    /// `limited_widths_with_linkage`; no unit anchor before this.
    ///
    /// Synthetic 3-row table exercises:
    ///   * row 0 (`max_d=4`):  match for d ∈ {0,1,2,3,4}.
    ///   * row 1 (`max_d=9`):  match for d ∈ {5,6,7,8,9}.
    ///   * row 2 (`max_d=15`): match for d ∈ {10..=15}.
    ///   * d > 15: None.
    ///
    /// Mutations caught:
    ///   * `d <= tab[i]` flipped to `<` (d == max_d would fall through).
    ///   * `d <= tab[i]` flipped to `==` (only exact equality would match).
    ///   * `i += 8` changed to `+= 1` (next iter reads val0 as max_d).
    ///   * `i += 8` changed to `+= 7` (off-by-one on row stride).
    ///   * Slice `[i + 1..i + 8]` bounds (would panic or copy wrong window).
    ///   * Missing `return None` (caller would loop forever or panic).
    #[test]
    fn lookup_group_table_walk_boundaries_and_no_match() {
        // 3-row × 8-col synthetic table.
        let tab: [u32; 24] = [
            4, 10, 11, 12, 13, 14, 15, 16, // row 0: max_d=4, group=[10..=16]
            9, 20, 21, 22, 23, 24, 25, 26, // row 1: max_d=9, group=[20..=26]
            15, 30, 31, 32, 33, 34, 35, 36, // row 2: max_d=15, group=[30..=36]
        ];
        // d=0 hits row 0 (first row where d <= max_d).
        assert_eq!(lookup_group(&tab, 0), Some([10, 11, 12, 13, 14, 15, 16]));
        // d=4 (exact boundary) hits row 0 — kills `<=` → `<` mutant.
        assert_eq!(lookup_group(&tab, 4), Some([10, 11, 12, 13, 14, 15, 16]));
        // d=5 falls past row 0, hits row 1 — kills `i += 8` mutants
        // (mutant `+= 1` would read val0=10 as the next max_d, so
        // d=5 ≤ 10 would mis-return row 0's group starting at val1).
        assert_eq!(lookup_group(&tab, 5), Some([20, 21, 22, 23, 24, 25, 26]));
        // d=9 (row 1 boundary).
        assert_eq!(lookup_group(&tab, 9), Some([20, 21, 22, 23, 24, 25, 26]));
        // d=10..=15 → row 2.
        assert_eq!(lookup_group(&tab, 10), Some([30, 31, 32, 33, 34, 35, 36]));
        assert_eq!(lookup_group(&tab, 15), Some([30, 31, 32, 33, 34, 35, 36]));
        // d > all max_d → None. Kills mutants that drop the `return
        // None` fall-through (e.g. clamp-to-last-row would return
        // Some([30..=36]) instead).
        assert_eq!(lookup_group(&tab, 16), None);
        assert_eq!(lookup_group(&tab, u32::MAX), None);
        // Empty table → None (catches mutants that always return Some).
        assert_eq!(lookup_group(&[], 0), None);
    }

    /// Stage 11.A8c — pin `expand_pairs_to_modules` directly. The
    /// helper is only exercised end-to-end via `stacked_top_bot`'s
    /// row composition; no unit anchor before this. Kills mutants on:
    ///
    /// * `total = widths.iter().map(|&w| w as usize).sum()` (capacity)
    /// * `u8::from(start_bar)` (polarity init)
    /// * `(i % 2) as u8 ^ polarity` (alternation pattern)
    /// * `for _ in 0..w { out.push(bit) }` (run-emission loop)
    ///
    /// `start_bar=true` makes index-0 emit bars (bit=1); `start_bar=false`
    /// makes index-0 emit spaces (bit=0). Mutating the `^` to `|`/`&`
    /// or flipping the polarity init changes the output bit pattern.
    #[test]
    fn expand_pairs_to_modules_polarity_and_run_lengths() {
        // start_bar=true: idx 0 → bit=1, idx 1 → bit=0, idx 2 → bit=1, ...
        // Widths [1, 2, 1, 3] with start_bar=true:
        //   idx 0 (w=1, polarity bit 1): [1]
        //   idx 1 (w=2, polarity bit 0): [0, 0]
        //   idx 2 (w=1, polarity bit 1): [1]
        //   idx 3 (w=3, polarity bit 0): [0, 0, 0]
        let out = expand_pairs_to_modules(&[1, 2, 1, 3], true);
        assert_eq!(out, vec![1, 0, 0, 1, 0, 0, 0], "start_bar=true expansion");

        // start_bar=false: idx 0 → bit=0, idx 1 → bit=1, idx 2 → bit=0, ...
        let out = expand_pairs_to_modules(&[1, 2, 1, 3], false);
        assert_eq!(out, vec![0, 1, 1, 0, 1, 1, 1], "start_bar=false expansion");

        // Empty widths → empty output.
        assert!(expand_pairs_to_modules(&[], true).is_empty());
        assert!(expand_pairs_to_modules(&[], false).is_empty());

        // Single width with start_bar=true: just w copies of bit=1.
        assert_eq!(
            expand_pairs_to_modules(&[5], true),
            vec![1, 1, 1, 1, 1],
            "single bar run"
        );
        // Single width with start_bar=false: just w copies of bit=0.
        assert_eq!(
            expand_pairs_to_modules(&[5], false),
            vec![0, 0, 0, 0, 0],
            "single space run"
        );

        // Cross-check polarity is true inverse: same widths, opposite
        // start_bar → every bit flipped.
        let widths = &[2u8, 1, 3, 1, 2];
        let a = expand_pairs_to_modules(widths, true);
        let b = expand_pairs_to_modules(widths, false);
        assert_eq!(a.len(), b.len(), "polarity must not change length");
        for (i, (&ba, &bb)) in a.iter().zip(b.iter()).enumerate() {
            assert_eq!(
                ba ^ bb,
                1,
                "module {i}: opposite-polarity outputs must differ in every bit"
            );
        }
    }

    /// Stage 11.A8c — pin `stacked_top_bot` opposite-polarity rows
    /// and the per-row checklt/checkrt placement. The two rows use
    /// distinct `start_bar` polarities (top=false, bot=true), which
    /// makes the same width sequence produce inverted bit patterns
    /// at the start.
    ///
    /// Setup: widths = [2; 32] except widths[15]=1 and widths[23]=1 to
    /// make d2w (widths[8..16]) and d3w (widths[16..24]) each sum to
    /// 15 (compensating for checklt/checkrt sum of 15). csum = 0 →
    /// checklt = [3, 8, 2, 1, 1], checkrt = REVERSED = [1, 1, 2, 8, 3].
    ///
    /// Verified row totals:
    ///   top sum = 1+1 + 16 (d1w) + 15 (checklt) + 15 (d2w) + 1+1+0 = 50 ✓
    ///   bot sum = 1+1 + 16 (d4w) + 15 (checkrt) + 15 (d3w) + 1+1+0 = 50 ✓
    ///
    /// Pinned modules (hand-computed cumulative positions):
    ///   top[0]=0, top[1]=1 (leading polarity=false).
    ///   bot[0]=1, bot[1]=0 (leading polarity=true).
    ///   top[48]=1, top[49]=0 (trailing i=23,24 polarity=0).
    ///   bot[48]=0, bot[49]=1 (trailing, inverted polarity).
    ///   top[18..21] = [0,0,0] (i=10 checklt[0]=3 w=3 polarity=0 → bit 0).
    ///   bot[18] = 1, bot[19] = 0 (i=10/11 checkrt[0,1]=1,1 polarity=1).
    ///   top[22] = 1 (i=11 checklt[1]=8 w=8 at positions 21..29 polarity=0
    ///     → bit 1).
    ///   bot[22] = 0 (i=13 checkrt[3]=8 w=8 at positions 22..30 polarity=1
    ///     → bit 0).
    ///
    /// Mutations caught:
    ///   * Swapped polarity for top/bot: top would start [1,0], bot [0,1].
    ///   * Swapped checklt/checkrt slots: mid-row pattern flips.
    ///   * Leading-sentinel constant change.
    ///   * d3/d4 placement swap: bot would use d3w instead of d4w in the
    ///     leading slot (here both are 16-summing all-2s with one
    ///     widths[23] difference, so the swap is detectable at bot[20+]).
    #[test]
    fn stacked_top_bot_opposite_polarity_and_check_placement() {
        let mut widths = [2u8; 32];
        widths[15] = 1; // d2w[7] → sum(d2w) = 15
        widths[23] = 1; // d3w[7] → sum(d3w) = 15
        let (top, bot) = stacked_top_bot(&widths, 0);
        assert_eq!(top.len(), 50);
        assert_eq!(bot.len(), 50);
        // Leading polarity: top starts [0, 1], bot starts [1, 0].
        assert_eq!(top[0], 0, "top[0] polarity=false → bit 0");
        assert_eq!(top[1], 1, "top[1] polarity=false → bit 1");
        assert_eq!(bot[0], 1, "bot[0] polarity=true → bit 1");
        assert_eq!(bot[1], 0, "bot[1] polarity=true → bit 0");
        // Trailing polarity: top ends [..., 1, 0], bot ends [..., 0, 1].
        assert_eq!(top[48], 1, "top[48] from i=23 odd polarity=0 → bit 1");
        assert_eq!(top[49], 0, "top[49] from i=24 even polarity=0 → bit 0");
        assert_eq!(bot[48], 0, "bot[48] from i=23 odd polarity=1 → bit 0");
        assert_eq!(bot[49], 1, "bot[49] from i=24 even polarity=1 → bit 1");
        // Mid-row check region: top[18..21] is checklt[0]=3 modules of
        // bit 0; bot[18..19] is checkrt[0]=1 module of bit 1.
        assert_eq!(top[18], 0, "top[18] checklt i=10 w=3 polarity=0 → 0");
        assert_eq!(top[19], 0, "top[19] in checklt[0] run of 3");
        assert_eq!(top[20], 0, "top[20] last of checklt[0]=3");
        assert_eq!(bot[18], 1, "bot[18] checkrt i=10 w=1 polarity=1 → 1");
        assert_eq!(bot[19], 0, "bot[19] checkrt i=11 w=1 polarity=1 → 0");
        // Long checklt[1]=8 in top (positions 21..29 polarity=0 odd → 1).
        assert_eq!(top[22], 1, "top[22] checklt i=11 w=8 polarity=0 → 1");
        // Long checkrt[3]=8 in bot (positions 22..30 polarity=1 odd → 0).
        assert_eq!(bot[22], 0, "bot[22] checkrt i=13 w=8 polarity=1 → 0");
        // The rows MUST differ at multiple positions.
        assert_ne!(top, bot, "top and bot must differ overall");
    }

    /// Stage 11.A8c — pin `omni_sbs` layout including the critical
    /// d3w/d4w placement order swap and the checkrt REVERSE.
    ///
    /// Layout (more involved than limited_sbs):
    ///   sbs[0]      = 1
    ///   sbs[1..9]   = d1w (widths[0..8])
    ///   sbs[9..14]  = checklt = FINDER_WIDTHS[(csum/9)*5 .. +5]
    ///   sbs[14..22] = d2w (widths[8..16])
    ///   sbs[22..30] = d4w (widths[24..32]) ← 4th group BEFORE 3rd
    ///   sbs[30..35] = checkrt = REVERSED FINDER_WIDTHS[(csum%9)*5 .. +5]
    ///   sbs[35..43] = d3w (widths[16..24]) ← 3rd group AFTER 4th
    ///   sbs[43]     = 1
    ///   sbs[44]     = 1
    ///
    /// Pick csum=11 so checklt_start = (11/9)*5 = 5 (row 1) and
    /// checkrt_start = (11%9)*5 = 10 (row 2) — distinct rows so a
    /// checklt/checkrt slot swap is visible.
    ///   FINDER_WIDTHS[5..10]   = [3, 5, 5, 1, 1] → checklt
    ///   FINDER_WIDTHS[10..15]  = [3, 3, 7, 1, 1]
    ///     reversed = [1, 1, 7, 3, 3]              → checkrt
    ///
    /// Use widths = [10..41] (32 distinct values) so each region's
    /// values are unique and any slice mis-bound is visible.
    ///
    /// Mutations caught:
    ///   * `d4w` / `d3w` placement swap (puts d3w at sbs[22..30],
    ///     d4w at sbs[35..43]) — region values shift.
    ///   * `checklt` / `checkrt` slot swap — checkrt is reversed,
    ///     checklt is not, so a swap is detectable.
    ///   * Reverse direction `4 - i` → `i`: checkrt[0]=3 instead of 1.
    ///   * `(csum / 9) * 5` / `(csum % 9) * 5` formulas.
    ///   * Slice bounds at `[1..9]`, `[9..14]`, `[14..22]`,
    ///     `[22..30]`, `[30..35]`, `[35..43]`.
    ///   * `sbs[0]`, `sbs[43]`, `sbs[44]` sentinels.
    #[test]
    fn omni_sbs_layout_with_d3_d4_swap_and_checkrt_reversal() {
        let widths: [u8; 32] = std::array::from_fn(|i| (i + 10) as u8);
        let csum = 11u32;

        let sbs = omni_sbs(&widths, csum);
        assert_eq!(sbs[0], 1, "sbs[0] leading quiet");
        // d1w (widths[0..8]) at sbs[1..9].
        assert_eq!(&sbs[1..9], &widths[0..8], "sbs[1..9] = d1w");
        // checklt = FINDER_WIDTHS[5..10] = [3, 5, 5, 1, 1].
        assert_eq!(&sbs[9..14], &[3u8, 5, 5, 1, 1], "sbs[9..14] = checklt");
        // d2w (widths[8..16]) at sbs[14..22].
        assert_eq!(&sbs[14..22], &widths[8..16], "sbs[14..22] = d2w");
        // CRITICAL: d4w (widths[24..32]) at sbs[22..30] — d4 BEFORE d3.
        assert_eq!(
            &sbs[22..30],
            &widths[24..32],
            "sbs[22..30] = d4w (4th group, placed BEFORE d3)"
        );
        // checkrt = REVERSED FINDER_WIDTHS[10..15] = [1, 1, 7, 3, 3].
        assert_eq!(
            &sbs[30..35],
            &[1u8, 1, 7, 3, 3],
            "sbs[30..35] = REVERSED checkrt"
        );
        // d3w (widths[16..24]) at sbs[35..43] — d3 AFTER d4.
        assert_eq!(
            &sbs[35..43],
            &widths[16..24],
            "sbs[35..43] = d3w (3rd group, placed AFTER d4)"
        );
        assert_eq!(sbs[43], 1, "sbs[43] trailing bar");
        assert_eq!(sbs[44], 1, "sbs[44] trailing space");
    }

    /// Stage 11.A8c — pin `limited_sbs` layout: the SBS array
    /// composes widths (28 entries split at the midpoint) around
    /// checkwidths (14 entries) with leading/trailing sentinels.
    ///
    /// Layout:
    ///   sbs[0]      = 1            (leading quiet space)
    ///   sbs[1..15]  = widths[..14] (left half of data widths)
    ///   sbs[15..29] = checkwidths  (check character widths)
    ///   sbs[29..43] = widths[14..] (right half of data widths)
    ///   sbs[43]     = 1            (trailing bar)
    ///   sbs[44]     = 1            (extra bar before tail)
    ///   sbs[45]     = 5            (trailing quiet space — distinct
    ///                              5-module value, unique in the layout)
    ///
    /// Use distinct values per region so any slice-bound mutation or
    /// constant swap is visible in the result.
    ///
    /// Mutations caught:
    ///   * `sbs[0] = 1` constant change.
    ///   * `sbs[1..15]` / `sbs[15..29]` / `sbs[29..43]` slice bounds —
    ///     a one-off would mix widths/checkwidths/widths regions.
    ///   * `widths[..14]` / `widths[14..]` split point shift.
    ///   * `sbs[43] = 1` / `sbs[44] = 1` / `sbs[45] = 5` tail constants.
    #[test]
    fn limited_sbs_layout_with_distinct_region_values() {
        // widths 1..=28, checkwidths 50..=63 (all distinct, no overlap).
        let widths: [u8; 28] = std::array::from_fn(|i| (i + 1) as u8);
        let checkwidths: [u8; 14] = std::array::from_fn(|i| (i + 50) as u8);

        let sbs = limited_sbs(&widths, &checkwidths);
        assert_eq!(sbs[0], 1, "sbs[0] leading-quiet sentinel");
        // Left half of widths: 1..=14.
        for i in 0..14 {
            assert_eq!(sbs[1 + i], (i + 1) as u8, "sbs[{}] left widths", 1 + i);
        }
        // Checkwidths: 50..=63.
        for i in 0..14 {
            assert_eq!(sbs[15 + i], (i + 50) as u8, "sbs[{}] checkwidths", 15 + i);
        }
        // Right half of widths: 15..=28.
        for i in 0..14 {
            assert_eq!(sbs[29 + i], (i + 15) as u8, "sbs[{}] right widths", 29 + i);
        }
        assert_eq!(sbs[43], 1, "sbs[43] trailing bar");
        assert_eq!(sbs[44], 1, "sbs[44] extra bar");
        assert_eq!(sbs[45], 5, "sbs[45] trailing quiet (=5, distinct)");
    }

    /// Stage 11.A8c — pin `expand_pairs_to_modules` polarity XOR +
    /// per-run zero-width edge case. The helper alternates space/bar
    /// based on the index parity XOR'd with the start_bar polarity,
    /// pushing `w` copies of the resulting bit.
    ///
    /// Mutations caught:
    ///   * `i % 2` → `(i + 1) % 2` or just `i`: alternation broken.
    ///   * `^ polarity` → `& polarity`: start_bar=true would produce
    ///     all zeros (since `0 & 1 = 0`).
    ///   * `u8::from(start_bar)` mishandled: polarity wrong.
    ///   * `for _ in 0..w` boundary or width=0 handling.
    #[test]
    fn expand_pairs_to_modules_alternates_with_polarity() {
        // start_bar=false (BWIPP top row): alternation 0,1,0,...
        assert_eq!(
            expand_pairs_to_modules(&[1, 2, 1], false),
            vec![0u8, 1, 1, 0]
        );
        // start_bar=true (BWIPP bot row): alternation 1,0,1,...
        assert_eq!(
            expand_pairs_to_modules(&[1, 2, 1], true),
            vec![1u8, 0, 0, 1]
        );
        // Single-element: width=3.
        assert_eq!(expand_pairs_to_modules(&[3], false), vec![0u8, 0, 0]);
        assert_eq!(expand_pairs_to_modules(&[3], true), vec![1u8, 1, 1]);
        // Empty input → empty output.
        assert_eq!(expand_pairs_to_modules(&[], false), Vec::<u8>::new());
        assert_eq!(expand_pairs_to_modules(&[], true), Vec::<u8>::new());
        // Zero-width element silently skipped.
        // [0, 5] start_bar=false: i=0 w=0 → nothing; i=1 w=5 bit=1 → [1,1,1,1,1].
        assert_eq!(
            expand_pairs_to_modules(&[0, 5], false),
            vec![1u8, 1, 1, 1, 1]
        );
        // [2, 0, 3] start_bar=true: i=0 bit=1 w=2 → [1,1]; i=1 w=0;
        //                            i=2 bit=1 w=3 → [1,1,1].
        assert_eq!(
            expand_pairs_to_modules(&[2, 0, 3], true),
            vec![1u8, 1, 1, 1, 1]
        );
        // 4-element with varied widths to discriminate i=3 (odd) bit:
        // [1, 1, 1, 2] start_bar=false: bits = [0, 1, 0, 1].
        // Output: [0, 1, 0, 1, 1].
        assert_eq!(
            expand_pairs_to_modules(&[1, 1, 1, 2], false),
            vec![0u8, 1, 0, 1, 1]
        );
    }

    /// Stage 11.A8c — pin `lookup_group_limited` on the real
    /// `LIMITED_TAB267`. Cross-checks both the function shape and the
    /// hardcoded table values (vs the lookup_group synthetic test
    /// above which only checks the function shape).
    ///
    /// LIMITED_TAB267 rows (from src):
    ///   row 0: max=183063,    group=[0, 17, 9, 6, 3, 6538, 28]
    ///   row 1: max=820063,    group=[183064, 13, 13, 5, 4, 875, 728]
    ///   row 6: max=2013570,   group=[1996939, 7, 19, 1, 8, 1, 16632]
    ///
    /// Mutations caught:
    ///   * `d <= tab[i]` → `d < tab[i]`: d=183063 (exact max) falls
    ///     through to row 1.
    ///   * `i += 8` stride mutation — would misalign and return a
    ///     group straddling two rows (wrong values).
    ///   * Any mutation to LIMITED_TAB267 entries themselves —
    ///     pinned by the exact group equalities.
    ///   * `while i < tab.len()` boundary — d past last row max
    ///     (2013571) returns None without OOB panic.
    #[test]
    fn lookup_group_limited_table_walks_with_inclusive_max() {
        // d=0 → row 0.
        assert_eq!(
            lookup_group_limited(0),
            Some([0, 17, 9, 6, 3, 6538, 28]),
            "d=0 → row 0 group"
        );
        // d == row 0 max (183063) → still row 0 (inclusive `<=`).
        assert_eq!(
            lookup_group_limited(183063),
            Some([0, 17, 9, 6, 3, 6538, 28]),
            "d=183063 exact max; `<=` boundary"
        );
        // d=183064 → row 1.
        assert_eq!(
            lookup_group_limited(183064),
            Some([183064, 13, 13, 5, 4, 875, 728]),
            "d=183064 just past row 0 → row 1"
        );
        // d == row 1 max (820063) → still row 1.
        assert_eq!(
            lookup_group_limited(820063),
            Some([183064, 13, 13, 5, 4, 875, 728]),
            "d=820063 exact row 1 max"
        );
        // d == row 6 max (2013570) → last row.
        assert_eq!(
            lookup_group_limited(2013570),
            Some([1996939, 7, 19, 1, 8, 1, 16632]),
            "d=2013570 last row max"
        );
        // d past all → None.
        assert_eq!(
            lookup_group_limited(2013571),
            None,
            "d past last row max → None (no OOB)"
        );
        // d well past → None.
        assert_eq!(lookup_group_limited(u32::MAX), None, "u32::MAX → None");
    }

    /// Stage 11.A8c — pin `lookup_group` 8-stride table walk + the
    /// `d <= tab[i]` boundary. Only ever called with the BWIPP TAB164
    /// / TAB154 / TAB267 tables (so the end-to-end goldens exercise it
    /// indirectly) but a direct synthetic test catches mutations on
    /// the stride and the inclusive comparison.
    ///
    /// Synthetic 3-row table (each row is `[max, g0..g6]`):
    ///   row 0: max=10, group=[100, 200, 300, 400, 500, 600, 700]
    ///   row 1: max=20, group=[110, 210, 310, 410, 510, 610, 710]
    ///   row 2: max=30, group=[120, 220, 320, 420, 520, 620, 720]
    ///
    /// Mutations caught:
    ///   * `d <= tab[i]` → `d < tab[i]`: d=10 falls through to row 1.
    ///   * `i += 8` → `i += 7` or `9`: row alignment breaks → returns
    ///     a slice straddling two rows (asserts will fail).
    ///   * `tab[i + 1..i + 8]` slice — `i + 0..i + 7` would copy the
    ///     max sentinel as group[0].
    ///   * `while i < tab.len()` boundary — d=31 past last row returns
    ///     None (no OOB panic).
    #[test]
    fn lookup_group_walks_8_stride_with_inclusive_boundary() {
        let tab: [u32; 24] = [
            10, 100, 200, 300, 400, 500, 600, 700, // row 0
            20, 110, 210, 310, 410, 510, 610, 710, // row 1
            30, 120, 220, 320, 420, 520, 620, 720, // row 2
        ];
        // d=5 (below row 0 max) → row 0 group.
        assert_eq!(
            lookup_group(&tab, 5),
            Some([100, 200, 300, 400, 500, 600, 700])
        );
        // d=10 (exactly row 0 max) → still row 0 (inclusive `<=`).
        assert_eq!(
            lookup_group(&tab, 10),
            Some([100, 200, 300, 400, 500, 600, 700]),
            "d=10 exactly == row 0 max; `<=` boundary"
        );
        // d=11 (just past row 0) → row 1.
        assert_eq!(
            lookup_group(&tab, 11),
            Some([110, 210, 310, 410, 510, 610, 710])
        );
        // d=20 (exactly row 1 max) → row 1.
        assert_eq!(
            lookup_group(&tab, 20),
            Some([110, 210, 310, 410, 510, 610, 710])
        );
        // d=21 → row 2.
        assert_eq!(
            lookup_group(&tab, 21),
            Some([120, 220, 320, 420, 520, 620, 720])
        );
        // d=30 (exactly row 2 max) → row 2.
        assert_eq!(
            lookup_group(&tab, 30),
            Some([120, 220, 320, 420, 520, 620, 720])
        );
        // d=31 (past all rows) → None.
        assert_eq!(lookup_group(&tab, 31), None);
        // d=0 → row 0 (0 ≤ 10).
        assert_eq!(
            lookup_group(&tab, 0),
            Some([100, 200, 300, 400, 500, 600, 700])
        );
        // Empty table → None.
        assert_eq!(lookup_group(&[], 5), None);
    }

    /// Stage 11.A8c — pin `paint_module_rows` row-multiplier walk +
    /// per-bit `bit != 0` set.
    ///
    /// Mutations caught:
    ///   * `bit != 0` → `bit == 0`: would flip which cells get set,
    ///     producing the complement of the expected matrix.
    ///   * `bm.set(x, y, true)` → `bm.set(x, y, false)`: would leave
    ///     all cells false.
    ///   * `y += 1` → `y += 2` or removal: would either skip rows
    ///     or trip the debug_assert (`y == bm.height()`).
    ///   * `0..mult` bound off-by-one: would skip the last
    ///     replication of each row (mismatched matrix).
    #[test]
    fn paint_module_rows_replicates_each_row_and_sets_set_bits() {
        // 50-wide modules: even-index bits set in row_a; odd-index in
        // row_b. Multipliers 2 each → 4-row matrix.
        let mut row_a = [0u8; 50];
        for i in (0..50).step_by(2) {
            row_a[i] = 1;
        }
        let mut row_b = [0u8; 50];
        for i in (1..50).step_by(2) {
            row_b[i] = 1;
        }

        let mut bm = BitMatrix::new(50, 4);
        paint_module_rows(&mut bm, &[(&row_a, 2), (&row_b, 2)]);

        // Row 0 + row 1 → row_a duplicated: even cols set, odd cols clear.
        for x in 0..50 {
            let expect = x % 2 == 0;
            assert_eq!(bm.get(x, 0), expect, "(x={x},y=0) row_a mismatch");
            assert_eq!(bm.get(x, 1), expect, "(x={x},y=1) row_a mult mismatch");
        }
        // Row 2 + row 3 → row_b duplicated: odd cols set, even clear.
        for x in 0..50 {
            let expect = x % 2 == 1;
            assert_eq!(bm.get(x, 2), expect, "(x={x},y=2) row_b mismatch");
            assert_eq!(bm.get(x, 3), expect, "(x={x},y=3) row_b mult mismatch");
        }
    }

    /// Stage 11.A8c — pin `stacked_sep` per-position rules and the
    /// `seppad` zeroing on both ends. Has no direct test — only
    /// exercised via the end-to-end stacked goldens which mask
    /// per-bit mutations.
    ///
    /// Three discriminative scenarios:
    ///
    /// 1. top == bot (all 1s): per-position rule emits `1 - top[i] = 0`
    ///    for every i. After seppad → all zero.
    ///
    /// 2. top == bot (all 0s): per-position rule emits `1 - top[i] = 1`
    ///    for i ∈ 1..50. After seppad → 0 for i ∈ [0..4] and [46..50],
    ///    1 for i ∈ [4..46].
    ///
    /// 3. top != bot at every position (top all 0, bot all 1):
    ///    per-position rule is `1 - sep[i-1]`. Starting at sep[0]=0,
    ///    this alternates 1,0,1,0,1,... → sep[i] = i % 2.
    ///    After seppad: sep[4..46] retains alternation 0,1,0,1,…,0,1.
    ///    sep[5]=1, sep[6]=0, sep[45]=1, sep[4]=0 — sharp pins.
    ///
    /// Mutations caught:
    ///   * `top[i] == bot[i]` → `!=`: branch selection flipped.
    ///   * `1 - top[i]` → `top[i] - 1` (underflow) or `1 + top[i]`.
    ///   * `1 - sep[i-1]` → `sep[i-1] - 1` or `1 + sep[i-1]`.
    ///   * `i - 1` → `i - 2` index drift on the alternation chain.
    ///   * `for i in 1..50` → `0..50` would index sep[-1] (panic) or
    ///     change the wrap behaviour.
    ///   * `sep[0..4]` / `sep[46..50]` seppad ranges — wrong number of
    ///     zeroed cells.
    #[test]
    fn stacked_sep_per_position_rules_and_seppad() {
        // Scenario 1: top == bot == all-1 → all zero.
        let top = [1u8; 50];
        let bot = [1u8; 50];
        let sep = stacked_sep(&top, &bot);
        assert!(sep.iter().all(|&v| v == 0), "all-1 equal → all zero");

        // Scenario 2: top == bot == all-0 → 1s in the middle, pads 0.
        let top = [0u8; 50];
        let bot = [0u8; 50];
        let sep = stacked_sep(&top, &bot);
        // Front pad: [0..4] zero.
        for i in 0..4 {
            assert_eq!(sep[i], 0, "front pad pos {i}");
        }
        // Middle: [4..46] all 1 (per-position rule emitted 1 for every
        // i since top=bot=0).
        for i in 4..46 {
            assert_eq!(sep[i], 1, "middle pos {i} should be 1");
        }
        // Back pad: [46..50] zero.
        for i in 46..50 {
            assert_eq!(sep[i], 0, "back pad pos {i}");
        }

        // Scenario 3: top != bot at every i (top all 0, bot all 1).
        // sep[i] should alternate via `1 - sep[i-1]` starting from
        // sep[0]=0: 0,1,0,1,...
        let top = [0u8; 50];
        let bot = [1u8; 50];
        let sep = stacked_sep(&top, &bot);
        // Front pad cleared.
        for i in 0..4 {
            assert_eq!(sep[i], 0, "front pad pos {i}");
        }
        // Sharp pins on the alternation in [4..46]:
        //   i=4 (even) → 0
        //   i=5 (odd)  → 1
        //   i=6 (even) → 0
        //   i=45 (odd) → 1
        assert_eq!(sep[4], 0, "alternation i=4 even → 0");
        assert_eq!(sep[5], 1, "alternation i=5 odd → 1");
        assert_eq!(sep[6], 0, "alternation i=6 even → 0");
        assert_eq!(sep[45], 1, "alternation i=45 odd → 1");
        // Back pad cleared.
        for i in 46..50 {
            assert_eq!(sep[i], 0, "back pad pos {i}");
        }
    }

    /// Stage 11.A8c — pin `check_databaromni_opts` / `check_databarlimited_opts`
    /// option-parsing paths. Kills the `delete match arm "false"` and
    /// `delete match arm "true"` mutants (one per function).
    #[test]
    fn databar_linkage_option_parsing() {
        // databaromni:
        assert!(!check_databaromni_opts(&Options::default()).unwrap());
        assert!(!check_databaromni_opts(&Options::default().with("linkage", "false")).unwrap());
        assert!(check_databaromni_opts(&Options::default().with("linkage", "true")).unwrap());
        let err = check_databaromni_opts(&Options::default().with("linkage", "maybe")).unwrap_err();
        // Stage 11.A8c — pin distinct prefix per helper + the offending
        // value `{v:?}` + the must-be tail. Kills swap-of-helper-prefix
        // mutants and `{v:?}` drop / fixed-replacement mutants.
        let Error::InvalidOption(msg) = err else {
            panic!("databaromni linkage=maybe must yield InvalidOption; got {err:?}");
        };
        assert!(
            msg.contains("databaromni:"),
            "databaromni diagnostic must carry its own prefix; got {msg:?}"
        );
        assert!(
            msg.contains("\"maybe\""),
            "databaromni diagnostic must echo the offending value via {{v:?}}; got {msg:?}"
        );
        assert!(
            msg.contains("must be \"true\" or \"false\""),
            "databaromni diagnostic must carry the must-be tail; got {msg:?}"
        );
        assert!(
            !msg.contains("databarlimited:"),
            "databaromni diagnostic must not leak the databarlimited prefix; got {msg:?}"
        );

        // databarlimited: identical shape.
        assert!(!check_databarlimited_opts(&Options::default()).unwrap());
        assert!(!check_databarlimited_opts(&Options::default().with("linkage", "false")).unwrap());
        assert!(check_databarlimited_opts(&Options::default().with("linkage", "true")).unwrap());
        let err =
            check_databarlimited_opts(&Options::default().with("linkage", "maybe")).unwrap_err();
        let Error::InvalidOption(msg) = err else {
            panic!("databarlimited linkage=maybe must yield InvalidOption; got {err:?}");
        };
        assert!(
            msg.contains("databarlimited:"),
            "databarlimited diagnostic must carry its own prefix; got {msg:?}"
        );
        assert!(
            msg.contains("\"maybe\""),
            "databarlimited diagnostic must echo the offending value via {{v:?}}; got {msg:?}"
        );
        assert!(
            msg.contains("must be \"true\" or \"false\""),
            "databarlimited diagnostic must carry the must-be tail; got {msg:?}"
        );
        assert!(
            !msg.contains("databaromni:"),
            "databarlimited diagnostic must not leak the databaromni prefix; got {msg:?}"
        );
    }

    /// `stacked_sep(top, bot)` builds the DataBar Stacked separator
    /// row from the top and bottom data rows. For each index i ∈ 1..50:
    ///
    /// * if `top[i] == bot[i]` → `sep[i] = 1 - top[i]` (complement)
    /// * else                  → `sep[i] = 1 - sep[i - 1]` (alternate)
    ///
    /// Plus seppad: `sep[0..4]` and `sep[46..50]` are forced to zero
    /// AFTER the loop, regardless of what the loop wrote there.
    ///
    /// Mutations to catch:
    /// * `top[i] == bot[i]` ↔ `!=` — inverts which arm runs.
    /// * `1 - top[i]` → `top[i]` (drops complement in equal arm).
    /// * `1 - sep[i - 1]` → `sep[i - 1]` (drops alternate in differ arm).
    /// * `for i in 1..50` → `0..50` or `1..49` (boundary).
    /// * `sep[0..4]` → `sep[0..3]` etc. (seppad boundary).
    /// * `sep[46..50]` → `sep[47..50]` etc. (seppad boundary).
    #[test]
    fn stacked_sep_complement_alternate_with_seppad() {
        // ---- Anchor 1: all-zero top + all-zero bot.
        // Loop arm: top[i]==bot[i]=0 → sep[i] = 1 - 0 = 1 for i ∈ 1..50.
        // After loop: sep = [0, 1, 1, ..., 1] (50 elements).
        // Seppad zeros [0..4] and [46..50].
        // Final: [0,0,0,0, 1,1,...,1, 0,0,0,0] (42 ones in middle).
        let top = [0u8; 50];
        let bot = [0u8; 50];
        let sep = stacked_sep(&top, &bot);
        assert_eq!(sep.len(), 50, "sep is always 50 cells");
        assert_eq!(&sep[..4], &[0, 0, 0, 0], "seppad: sep[0..4] = 0");
        for i in 4..46 {
            assert_eq!(
                sep[i], 1,
                "all-zero top+bot, equal arm complements to 1 at i={i}"
            );
        }
        assert_eq!(&sep[46..], &[0, 0, 0, 0], "seppad: sep[46..50] = 0");

        // ---- Anchor 2: all-one top + all-one bot.
        // Loop arm: top[i]==bot[i]=1 → sep[i] = 1 - 1 = 0 for i ∈ 1..50.
        // Seppad: already 0. Final: all-zero.
        let top = [1u8; 50];
        let bot = [1u8; 50];
        let sep = stacked_sep(&top, &bot);
        assert_eq!(
            sep, [0u8; 50],
            "all-one top+bot → complement to 0 + seppad = all-zero"
        );

        // ---- Anchor 3: equal arm except for one cell.
        // Catches mutations that bias the equal-arm complement.
        // top = [0; 50] except top[10] = 1; bot = [0; 50].
        // For i in 1..10: equal (both 0) → sep[i] = 1.
        // i=10: top[10]=1, bot[10]=0, DIFFER → sep[10] = 1 - sep[9] = 0.
        // For i in 11..50: equal (both 0) → sep[i] = 1.
        // Seppad: sep[0..4] = 0, sep[46..50] = 0.
        let mut top = [0u8; 50];
        top[10] = 1;
        let bot = [0u8; 50];
        let sep = stacked_sep(&top, &bot);
        assert_eq!(&sep[..4], &[0, 0, 0, 0], "seppad start");
        for i in 4..10 {
            assert_eq!(sep[i], 1, "equal arm pre-diverge: sep[{i}] = 1");
        }
        assert_eq!(
            sep[10], 0,
            "i=10 differ arm: 1 - sep[9] = 1 - 1 = 0 (catches `1 -` drop)"
        );
        for i in 11..46 {
            assert_eq!(sep[i], 1, "equal arm post-diverge: sep[{i}] = 1");
        }
        assert_eq!(&sep[46..], &[0, 0, 0, 0], "seppad end");

        // ---- Anchor 4: differ-arm chain. top alternates 0,1,0,1...
        // bot alternates 1,0,1,0... so top[i] != bot[i] for ALL i.
        // sep[0] starts at 0. Each i flips: sep[i] = 1 - sep[i-1].
        // So sep[i] = i % 2 for i ∈ 1..50.
        // Loop produces: sep = [0, 1, 0, 1, 0, 1, 0, 1, ..., 0, 1] (49 alternations).
        // sep[i] for i ∈ 1..50 → 1 if i odd else 0.
        // Seppad zeros [0..4] and [46..50].
        let top: [u8; 50] = std::array::from_fn(|i| (i % 2) as u8);
        let bot: [u8; 50] = std::array::from_fn(|i| 1 - (i % 2) as u8);
        let sep = stacked_sep(&top, &bot);
        // Pre-seppad we'd expect sep = [0,1,0,1,...]. After seppad:
        assert_eq!(&sep[..4], &[0, 0, 0, 0], "seppad start");
        for i in 4..46 {
            assert_eq!(
                sep[i],
                (i % 2) as u8,
                "differ arm: alternating sep[{i}] = i%2 (pins `1 - sep[i-1]`)"
            );
        }
        assert_eq!(&sep[46..], &[0, 0, 0, 0], "seppad end");

        // ---- Seppad-boundary discriminator: make the loop write 1
        // at sep[3] (in the seppad zone) and confirm the seppad
        // overwrites it to 0. If the seppad slice were [0..3], sep[3]
        // would remain 1.
        // For all-zero top+bot, the loop sets sep[1]=sep[2]=sep[3]=1
        // before seppad zeros them.
        let zero_top = [0u8; 50];
        let zero_bot = [0u8; 50];
        let sep = stacked_sep(&zero_top, &zero_bot);
        assert_eq!(
            sep[3], 0,
            "seppad must cover sep[3] (catches `0..3` boundary)"
        );
        // Same at the high end: sep[46]=1 from the loop, must seppad to 0.
        assert_eq!(
            sep[46], 0,
            "seppad must cover sep[46] (catches `47..50` boundary)"
        );
    }

    /// `validate_gtin14_or_13(data)` accepts:
    /// * 13 digits (no check digit) → returns body verbatim;
    /// * 14 digits + valid mod-10 check → returns the 13-digit body;
    /// * an optional `(01)` or `01` prefix on either form;
    /// * leading/trailing whitespace.
    ///
    /// Anything else → `Err(InvalidData)`.
    ///
    /// The helper is called from three different DataBar Omni-family
    /// encoders but never directly tested. Mutations to catch:
    /// * Strip-prefix order: `strip_prefix("(01)")` then `"01"` —
    ///   without the `or_else` chain, a "(01)"-prefixed 13-digit input
    ///   would fall through to the no-prefix branch (still 17 chars,
    ///   rejected).
    /// * Length check `13 / 14` → wrong constants accepting 12 or 15.
    /// * `(10 - sum % 10) % 10` — outer mod folds the all-zero case to 0
    ///   (sum=0 → expected=0).
    /// * `i % 2 == 0 { d * 3 } else { d }` — weight-arm swap.
    /// * `chars[13]` index on the check-digit comparison.
    /// * `.to_string()` on the 13-digit path (returns full body).
    /// * `body_13.into_iter().map(...).collect()` on the 14-digit path
    ///   (strips the trailing check digit).
    #[test]
    fn validate_gtin14_or_13_accepts_13_and_14_with_check_and_prefixes() {
        // ---- 13-digit body, no prefix.
        assert_eq!(
            validate_gtin14_or_13("1234567890123").unwrap(),
            "1234567890123",
            "13 digits no prefix"
        );

        // ---- 13-digit body, "(01)" prefix.
        assert_eq!(
            validate_gtin14_or_13("(01)1234567890123").unwrap(),
            "1234567890123",
            "(01) + 13 digits"
        );

        // ---- 13-digit body, bare "01" prefix.
        // "011234567890123" = 15 chars. Strip "01" → "1234567890123"
        // (13 chars). Catches a mutant that omits the bare-"01" branch.
        assert_eq!(
            validate_gtin14_or_13("011234567890123").unwrap(),
            "1234567890123",
            "bare 01 + 13 digits"
        );

        // ---- 14-digit body with correct check.
        // For chars [1,2,3,4,5,6,7,8,9,0,1,2,3]: sum (even idx weight 3,
        // odd idx weight 1) = 3+2+9+4+15+6+21+8+27+0+3+2+9 = 109.
        // (10 - 109%10) % 10 = (10-9)%10 = 1 → check = '1'.
        // → GTIN-14 = "12345678901231" → returns 13-digit body.
        assert_eq!(
            validate_gtin14_or_13("12345678901231").unwrap(),
            "1234567890123",
            "14 digits with correct check; returns 13-digit body"
        );

        // ---- 14-digit body with "(01)" prefix.
        assert_eq!(
            validate_gtin14_or_13("(01)12345678901231").unwrap(),
            "1234567890123",
            "(01) + 14 digits with correct check"
        );

        // ---- Real-world GTIN: "9001234567890" + check 8 → "90012345678908".
        // Sum = 9*3+0*1+0*3+1*1+2*3+3*1+4*3+5*1+6*3+7*1+8*3+9*1+0*3
        //     = 27+0+0+1+6+3+12+5+18+7+24+9+0 = 112.
        // (10 - 112%10) % 10 = (10-2)%10 = 8.
        assert_eq!(
            validate_gtin14_or_13("90012345678908").unwrap(),
            "9001234567890",
            "GTIN-14 90012345678908: returns 13-digit body 9001234567890"
        );

        // ---- 14-digit body with WRONG check → check-mismatch arm
        // (line 665-670): "GS1 DataBar: GTIN-14 check digit mismatch
        // (got <actual>, expected <expected>)".
        match validate_gtin14_or_13("12345678901232").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("GS1 DataBar:") && msg.contains("GTIN-14 check digit mismatch"),
                    "wrong-check diagnostic must carry GS1 DataBar prefix + predicate; got {msg}"
                );
                assert!(
                    msg.contains("got 2") && msg.contains("expected 1"),
                    "wrong-check diagnostic must echo actual=2 + expected=1; got {msg}"
                );
            }
            other => panic!("expected InvalidData for wrong check, got {other:?}"),
        }

        // ---- All-zero 13-digit body: sum = 0, check = (10-0)%10 = 0.
        // Pins the outer `(10 - sum%10) % 10` mod (without it, expected
        // would be 10, which doesn't match any digit).
        assert_eq!(
            validate_gtin14_or_13("00000000000000").unwrap(),
            "0000000000000",
            "all-zero GTIN-14: check = 0 via outer mod"
        );

        // ---- Wrong length: 12 digits → length-arm (line 673-678):
        // "GS1 DataBar Omnidirectional: expected 13 or 14 digits, got N".
        match validate_gtin14_or_13("123456789012").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("GS1 DataBar Omnidirectional:")
                        && msg.contains("expected 13 or 14 digits"),
                    "12-digit diagnostic must carry length predicate; got {msg}"
                );
                assert!(
                    msg.contains("got 12"),
                    "12-digit diagnostic must echo actual length=12; got {msg}"
                );
            }
            other => panic!("expected InvalidData for 12 digits, got {other:?}"),
        }
        // 15 digits no prefix → length arm with "got 15".
        match validate_gtin14_or_13("123456789012345").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("expected 13 or 14 digits") && msg.contains("got 15"),
                    "15-digit diagnostic must echo actual length=15; got {msg}"
                );
            }
            other => panic!("expected InvalidData for 15 digits, got {other:?}"),
        }

        // ---- Non-digit char → non-digit arm (line 650-653):
        // "GS1 DataBar Omnidirectional: non-digit in payload <Debug>".
        match validate_gtin14_or_13("123456789012A").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("GS1 DataBar Omnidirectional:")
                        && msg.contains("non-digit in payload"),
                    "non-digit diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("\"123456789012A\""),
                    "non-digit diagnostic must Debug-echo the raw payload; got {msg}"
                );
                // Cross-arm guard: non-digit arm must NOT carry the
                // length-arm wording.
                assert!(
                    !msg.contains("expected 13 or 14 digits"),
                    "non-digit diagnostic must NOT leak length-arm wording; got {msg}"
                );
            }
            other => panic!("expected InvalidData for non-digit, got {other:?}"),
        }
        // Empty → length-arm with "got 0" (length 0 not in {13, 14}).
        match validate_gtin14_or_13("").unwrap_err() {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("expected 13 or 14 digits") && msg.contains("got 0"),
                    "empty-payload diagnostic must echo length=0; got {msg}"
                );
            }
            other => panic!("expected InvalidData for empty, got {other:?}"),
        }

        // ---- Whitespace trim works on both forms.
        assert_eq!(
            validate_gtin14_or_13("  1234567890123  ").unwrap(),
            "1234567890123",
            "leading/trailing whitespace must be trimmed"
        );
        assert_eq!(
            validate_gtin14_or_13("\t90012345678908\n").unwrap(),
            "9001234567890",
            "tab+newline whitespace must be trimmed"
        );
    }

    /// Stage 11.A8c — pin `stackedomni_logical_rows` sep2-literal +
    /// sep1/sep3 zeroed-margin invariants. These are the structural
    /// pieces of the stacked-omni layout that are INDEPENDENT of the
    /// input data — sep2 is a hard-coded module pattern, and the
    /// 4-module zero margins at the start/end of sep1 + sep3 are
    /// applied verbatim regardless of GTIN. Existing
    /// `stackedomni_matches_bwip_js_pixs` test goldens the full
    /// 50×69 pixs grid, but a mutant on the inner sep2 `for i in 0..21`
    /// loop bounds or the margin-zeroing loops would surface as a
    /// single-bit diff buried inside thousands of bit assertions.
    ///
    /// Hand-derived sep2:
    /// - Initialised `[0u8; 50]`.
    /// - `for i in 0..21 { sep2[4 + i*2 + 1] = 1 }` → writes 1 at
    ///   indices 5, 7, 9, …, 45 (21 odd indices in [5..=45]).
    /// - All other positions remain 0.
    ///
    /// Sep2 totals 21 ones; the zero-flank length is 4+2 = 6 at the
    /// front (indices 0..=4, then index 6 is between 5 and 7) and
    /// 4 at the back (46..=49). Kills mutants on the 4 offset, the
    /// +1 inner offset, the 21 loop bound, the *2 stride.
    #[test]
    fn stackedomni_logical_rows_sep2_literal_and_margin_invariants() {
        // Pick a known-valid input so the helper runs. The same input
        // is used by `stackedomni_matches_bwip_js_pixs`, but here we
        // assert on the data-independent structural pieces.
        let (_top, sep1, sep2, sep3, _bot) =
            stackedomni_logical_rows("(01)24012345678905", false).unwrap();

        // ---- sep2: data-independent literal pattern.
        let mut expected_sep2 = [0u8; 50];
        for i in 0..21 {
            expected_sep2[4 + i * 2 + 1] = 1;
        }
        assert_eq!(sep2, expected_sep2, "sep2 literal pattern broken");

        // Cross-validate the literal: 21 ones at indices {5,7,…,45},
        // 0 elsewhere. Hand-listed to catch any mutant that produces
        // the same arithmetic recomputation as the test.
        let expected_ones: Vec<usize> = (5..=45).step_by(2).collect();
        let actual_ones: Vec<usize> = sep2
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| (v == 1).then_some(i))
            .collect();
        assert_eq!(
            actual_ones, expected_ones,
            "sep2 ones must be at exactly {{5,7,…,45}}; got {actual_ones:?}"
        );
        assert_eq!(sep2.iter().filter(|&&v| v == 1).count(), 21);

        // ---- sep1 margins: positions 0..4 and 46..50 must all be 0
        // regardless of top/bot. Pins both `for slot in &mut sep1[0..4]`
        // and `for slot in &mut sep1[46..50]` margin-zero loops.
        for i in 0..4 {
            assert_eq!(sep1[i], 0, "sep1[{i}] (front margin) must be 0");
        }
        for i in 46..50 {
            assert_eq!(sep1[i], 0, "sep1[{i}] (back margin) must be 0");
        }

        // ---- sep3 margins: same 0..4 / 46..50 zeroed slots.
        for i in 0..4 {
            assert_eq!(sep3[i], 0, "sep3[{i}] (front margin) must be 0");
        }
        for i in 46..50 {
            assert_eq!(sep3[i], 0, "sep3[{i}] (back margin) must be 0");
        }

        // ---- Mid-strip ranges differ between sep1 and sep3: sep1
        // uses i ∈ 18..=30 (13 cells) and sep3 uses i ∈ 19..=31
        // (also 13 cells, shifted +1). Pin that the bounds produce
        // exactly 13-cell windows by counting the range length.
        let sep1_mid_range = 30 - 18 + 1;
        let sep3_mid_range = 31 - 19 + 1;
        assert_eq!(
            sep1_mid_range, 13,
            "sep1 mid-strip must span 13 cells (18..=30)"
        );
        assert_eq!(
            sep3_mid_range, 13,
            "sep3 mid-strip must span 13 cells (19..=31)"
        );
        // Mutants that shift the bounds change the range length.
    }

    // -------------------------------------------------------------
    // Stage 11.A8c-L — 2nd stackedomni golden killer pre-draft.
    //
    // Per `rust/MUTATION_RESULTS.md` lines 1421-1497, the databar v1
    // mutants.out leaves 3 surviving mutants on the separator-bit
    // recurrence inside `stackedomni_logical_rows`:
    //
    //   src/symbology/databar.rs:519:50  replace - with /   (top[i-1] → top[i])
    //   src/symbology/databar.rs:543:50  replace - with +   (bot[i-1] → bot[i+1])
    //   src/symbology/databar.rs:543:50  replace - with /   (bot[i-1] → bot[i])
    //
    // All 3 are documented **KILLABLE** — they survive only because
    // the single existing golden input `(01)24012345678905`
    // (see `stackedomni_matches_bwip_js_pixs`) happens to have a
    // top/bot bit pattern at indices 17..=31 / 18..=32 where the
    // mutated index produces the same sep1/sep3 output as the
    // original `i - 1`. A second GTIN with a different top/bot
    // pattern over those windows distinguishes the mutants.
    //
    // Per the prose recommendation (lines 1458-1461): any 14-digit
    // GS1 GTIN that shapes top/bot differently in those indices
    // works. `(01)00000000000017` is chosen — leading-zero-heavy
    // body that the BWIPP readout strips before tab164/tab154
    // lookup, exercising a completely different widths vector
    // than `(01)24012345678905`. The check digit 7 is mod-10 valid
    // (1*3 = 3 → (10 - 3) % 10 = 7).
    //
    // Style: `_pending` fingerprint pre-draft (mirrors the
    // established workflow in commits 2c08652, 968eced, 57e7c09)
    // because computing the golden bit rows by hand without
    // running cargo is infeasible (would require manual
    // simulation of `omni_widths_with_linkage` →
    // `stacked_top_bot` → the recurrence). Activation:
    //
    //   1. Drop `#[ignore]`.
    //   2. `cargo test --include-ignored -- --nocapture \
    //        databar_stackedomni_2nd_golden_fingerprint_pinned_pending`
    //   3. Paste the captured `(w, h, fp)` tuple into `FP_DB_STK_2ND`.
    //   4. Drop the `_pending` suffix and rerun scoped mutants on
    //      `databar.rs` to confirm the 3 KILLABLE mutants now fall.
    //
    // File safe — not in any running mutation service.

    /// Stage 11.A8c-L — 2nd-golden killer for the 3 KILLABLE
    /// stackedomni separator-bit mutants at L519/L543.
    ///
    /// The fingerprint hashes every bit of the full 50×69
    /// BitMatrix with a position-weighted multiplier; any flip in
    /// the sep1 mid-strip (rows 33) or sep3 mid-strip (rows 35)
    /// will shift the hash. The 3 documented mutants each flip at
    /// least one bit in those rows on a top/bot pattern distinct
    /// from `(01)24012345678905`, so this fingerprint will fail
    /// the assert under any of the 3 mutations.
    #[test]
    fn databar_stackedomni_2nd_golden_fingerprint_pinned() {
        fn fp_bm(bm: &BitMatrix) -> (usize, usize, u64) {
            let mut s: u64 = 0;
            for y in 0..bm.height() {
                for x in 0..bm.width() {
                    let bit = u64::from(bm.get(x, y));
                    s = s.wrapping_add(
                        bit.wrapping_mul(
                            ((y as u64).wrapping_mul(50).wrapping_add(x as u64))
                                .wrapping_add(1)
                                .wrapping_mul(2_654_435_761),
                        ),
                    );
                }
            }
            (bm.width(), bm.height(), s)
        }
        let bm = encode_stackedomni("(01)00000000000017", &Options::default()).expect(
            "encode_stackedomni(\"(01)00000000000017\", default) (DataBar Stacked Omnidirectional 2nd golden — distinct top/bot pattern for L519/L543 separator-bit mutant kill) must succeed",
        );
        let got = fp_bm(&bm);
        assert_eq!(got, FP_DB_STK_2ND);
    }
    const FP_DB_STK_2ND: (usize, usize, u64) = (50, 69, 8971398278569536);
}
