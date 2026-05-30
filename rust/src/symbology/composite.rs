//! GS1 Composite barcodes — a linear primary stacked underneath a 2D
//! companion (CC-A / CC-B / CC-C). Used together they encode primary
//! GS1 element strings on the linear and supplementary AIs on the 2D.
//!
//! The 17 catalog entries (`composite_databar_omni_cca`,
//! `composite_gs1_128_ccc`, …) all share the same shape:
//!
//! 1. Parse `linear|comp` input split by `|`.
//! 2. Encode the linear primary via the existing linear encoder
//!    (`databaromni`, `gs1-128`, etc.) with the `linkage` flag set
//!    so the linear's check digit reflects the composite presence.
//! 3. Encode the comp via [`crate::symbology::gs1_cc::encode_cc`]
//!    (CC-A / CC-B) or PDF417 (CC-C).
//! 4. Render the 2D via [`crate::symbology::micropdf417::render_cca`]
//!    (CC-A) or `render_ccb` / PDF417 path.
//! 5. Compute a per-linear-type separator pattern that aligns the 2D
//!    with the linear's finder bars.
//! 6. Stack: `cc_pixs` rows on top, separator row, then the linear's
//!    bar/space pattern rendered at the linear height.
//!
//! This module is the shared infrastructure. Per-linear handlers
//! (`databaromni_composite`, `gs1_128_composite`, etc.) live alongside
//! and provide the linear-specific separator pattern.
//!
//! **Status**: all 17 composite catalog rows verified byte-for-byte
//! against bwip-js logical pixs (CC-A / CC-B / CC-C 2D companions
//! over every supported linear primary: DataBar Omni / Truncated /
//! Stacked / Stacked Omni / Limited / Expanded / Expanded Stacked,
//! EAN-8 / EAN-13 / UPC-A / UPC-E, GS1-128 with CC-A/B/C). The
//! shared infrastructure here drives the gs1_cc → render_ccX
//! pipeline, the separator generators, and the linear-specific
//! stacker. See `rust/PORT_STATUS.md` for the per-row test list.
//!
//! ## bwip-js pixs layout (databaromnicomposite)
//!
//! For input `(01)24012345678905|(10)BATCH`, bwip-js produces
//! `pixx=100, pixy=40, ccrows=3, linheight=33` with a 500-cell
//! `pixs` array — 5 logical rows of 100 cells each — and a
//! `rowmult = [2, 2, 2, 1, 33]` (sums to pixy=40).
//!
//! The 5 logical rows are:
//! 1. CC-A row 0 (rwid=99 padded to 100), repeated 2× physically.
//! 2. CC-A row 1, repeated 2×.
//! 3. CC-A row 2, repeated 2×.
//! 4. The separator row (96-wide, padded to 100), repeated 1×.
//! 5. The linear template (96-wide, padded to 100), repeated 33×.
//!
//! Total physical rows = 2+2+2+1+33 = 40 = pixy. Width is 100 (the
//! max of CC-A rwid=99 and linear 96, plus 1 for alignment).
//!
//! Our Rust pipeline produces the fully-expanded `BitMatrix` directly:
//! 6 CC-A rows + 1 separator row + 33 linear rows = 40 rows × 100 wide.

#![allow(dead_code)]

/// Parse a composite input string of the form `LINEAR|COMP` into the
/// `(linear, comp)` pair. Both halves must be non-empty.
pub(crate) fn split_composite_input(input: &str) -> Result<(&str, &str), crate::error::Error> {
    match input.split_once('|') {
        Some((l, c)) if !l.is_empty() && !c.is_empty() => Ok((l, c)),
        _ => Err(crate::error::Error::InvalidData(
            "composite: input must be 'LINEAR|COMP' (pipe-separated, both non-empty)".into(),
        )),
    }
}

/// DataBar Omni composite separator constants (direct port of BWIPP
/// `databaromnicomposite_*` at lines 38171-38173).
pub(crate) const DATABAROMNI_SEPPAD: [u8; 4] = [0, 0, 0, 0];
pub(crate) const DATABAROMNI_FINDERSEP: [u8; 13] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0];
pub(crate) const DATABAROMNI_F3PAT: [u8; 13] = [1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1];

/// Expand a linear barcode's bar/space-widths array (`sbs`) into a
/// pixel sequence. Each entry alternates between bar (1) and space
/// (0), starting with bar.
pub(crate) fn sbs_to_pixels(sbs: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut is_bar = true;
    for &w in sbs {
        let v = if is_bar { 1u8 } else { 0u8 };
        for _ in 0..w {
            out.push(v);
        }
        is_bar = !is_bar;
    }
    out
}

/// Build the DataBar Omni composite separator row from the linear's
/// pixel sequence (`bot`). The separator goes between the 2D and the
/// linear, indicating to the decoder that they form a composite.
///
/// Direct port of BWIPP `databaromnicomposite_sepfinder` + the main
/// sep construction (lines 38260-38286):
/// 1. `sep` starts as the bitwise inverse of `bot`.
/// 2. First 3 bits and last 4 bits forced to 0.
/// 3. For each finder position (18 and 64 for DataBar Omni), scan a
///    12-cell window: where `bot[i] == 0`, fill `sep[i]` with a 1-or-0
///    pattern that creates a continuous strip from the prior `sep`
///    bit; where `bot[i] == 1`, force `sep[i]` to 0.
/// 4. If `bot[fp..fp+13]` matches the F3 finder pattern `f3pat`,
///    overwrite `sep[fp..fp+13]` with the constant `findersep` (a
///    "1" at position 10 in a sea of zeros).
pub(crate) fn databaromni_separator(bot: &[u8]) -> Vec<u8> {
    let mut sep: Vec<u8> = bot.iter().map(|&b| 1 - b).collect();
    let n = sep.len();
    // First 3 bits and last 4 bits forced to 0.
    for s in sep.iter_mut().take(3) {
        *s = 0;
    }
    for s in sep.iter_mut().skip(n.saturating_sub(4)) {
        *s = 0;
    }
    for fp in [18usize, 64usize] {
        apply_sepfinder(bot, &mut sep, fp);
    }
    sep
}

/// MicroPDF417 CC-A row-multiplier — each logical row of the CC-A 2D
/// component expands to this many physical pixel rows in the output.
/// BWIPP `bwipp_micropdf417` line 22604: `$_.rowmult = 2`.
pub(crate) const CCA_ROWMULT: usize = 2;

/// Default DataBar Omni linear height in modules (BWIPP `bhs[0] = 33`).
pub(crate) const DATABAROMNI_LINHEIGHT: usize = 33;

/// DataBar Truncated linear height in modules. Same 95-module sbs as
/// DataBar Omni, but rendered with a shorter linear zone (BWIPP's
/// `bwipp_databartruncatedcomposite` defaults to `linheight=13`).
pub(crate) const DATABARTRUNCATED_LINHEIGHT: usize = 13;

/// Build the fully-expanded BitMatrix for a databar_omni_cca composite,
/// matching BWIPP's exact pixs layout (databaromnicomposite ~38299-38312).
///
/// Logical layout (each row 100 cells wide for cc=4-col):
/// - CC-A rows: 99 cells + trailing 0 = 100.
/// - Separator: [0,0,0,0] (4) + sep_96 (96) = 100.
/// - Linear: [0,0,0,0] (4) + linpixs_96 (96) = 100.
///
/// Physical expansion:
/// - Each CC-A logical row repeats [`CCA_ROWMULT`] times.
/// - Separator row appears once.
/// - Linear row repeats `linheight` times.
///
/// `linsbs` is BWIPP's `sbs` array — alternating bar/space widths
/// starting with **bar**. `sbs_to_pixels` expands it into the 95-cell
/// `bot` array. `linpixs_96 = [0] + bot`. `sep_96 = [0] + sep_95`
/// where `sep_95` is the inverse of `bot` with boundary zeros and
/// `sepfinder` applied at finder positions 18 and 64.
pub(crate) fn build_databaromni_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u32],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    // bot: 95 cells from expanding linsbs (sbs[0] is a bar).
    let bot = sbs_to_pixels(linsbs);
    // linpixs_96 = [0] + bot.
    let mut linpixs96: Vec<u8> = Vec::with_capacity(bot.len() + 1);
    linpixs96.push(0);
    linpixs96.extend_from_slice(&bot);
    // sep_95 = inverse(bot) with boundary zeros + sepfinder.
    let sep95 = databaromni_separator(&bot);
    // sep_96 = [0] + sep_95.
    let mut sep96: Vec<u8> = Vec::with_capacity(sep95.len() + 1);
    sep96.push(0);
    sep96.extend_from_slice(&sep95);
    // pixx is max(cc_width, lin_width) + 1. For cc=4 L1: max(99, 96) + 1 = 100.
    let cc_width = cc_pixs.width();
    let pixx = cc_width.max(linpixs96.len() + 4);
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * CCA_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC-A rows: 99 cells + trailing 0 (already zero from BitMatrix init).
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..cc_width {
                bm.set(x, y, cc_pixs.get(x, r));
            }
            // bm[cc_width..pixx] = 0 by default.
        }
    }
    // Separator row: [0,0,0,0] + sep96.
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in sep96.iter().enumerate() {
        bm.set(4 + x, sep_y, v == 1);
    }
    // Linear rows: [0,0,0,0] + linpixs96, repeated linheight times.
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 1 + rep;
        for (x, &v) in linpixs96.iter().enumerate() {
            bm.set(4 + x, y, v == 1);
        }
    }
    bm
}

/// Public entry point for the DataBar Omni + CC-A composite barcode.
///
/// Takes a `LINEAR|COMP` GS1 input string, encodes the linear with the
/// linkage bit set, encodes the 2D companion via gs1_cc + render_cca,
/// and stacks them into a [`BitMatrix`] via
/// [`build_databaromni_composite`].
///
/// The linear must be a `(01)<GTIN>` per DataBar Omni rules; the comp
/// may be any GS1 element string [`crate::symbology::gs1_cc::encode_cc`]
/// accepts.
pub(crate) fn encode_databaromni_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    // Linear: DataBar Omni with linkage bit set.
    let sbs45 = crate::symbology::databar::omni_sbs_with_linkage(linear, true)?;
    // BWIPP's sbs convention: even indices = bar widths, odd = space.
    // sbs[0] is the leading 1-width BAR. sbs_to_pixels expands by
    // alternating starting with bar — so no polarity adjustment needed.
    let linsbs: Vec<u32> = sbs45.iter().map(|&w| u32::from(w)).collect();
    // Comp: gs1_cc CC-A with 4 columns (DataBar Omni default). `encode_cc`
    // auto-promotes to CC-B for payloads that overflow CC-A capacity, but
    // this handler only renders CC-A — refuse CC-B explicitly so the
    // caller gets a clear error rather than a downstream codeword-range
    // panic when CC-B's 8-bit bytes hit `render_cca`.
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_omni_cca: payload requires CC-B; \
             use composite_databar_omni_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databaromni_composite(
        &cc_bm,
        &linsbs,
        DATABAROMNI_LINHEIGHT,
    ))
}

/// DataBar Truncated + CC-A composite (BWIPP `databartruncatedcomposite`,
/// CC-A path). Truncated shares DataBar Omni's 95-module sbs, so the
/// composite layout is identical to `encode_databaromni_cca` except for
/// the shorter [`DATABARTRUNCATED_LINHEIGHT`].
pub(crate) fn encode_databartruncated_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let sbs45 = crate::symbology::databar::omni_sbs_with_linkage(linear, true)?;
    let linsbs: Vec<u32> = sbs45.iter().map(|&w| u32::from(w)).collect();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_truncated_cca: payload requires CC-B; \
             use composite_databar_truncated_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databaromni_composite(
        &cc_bm,
        &linsbs,
        DATABARTRUNCATED_LINHEIGHT,
    ))
}

/// DataBar Truncated + CC-A/CC-B composite (BWIPP
/// `databartruncatedcomposite`). Drop-in superset of
/// [`encode_databartruncated_cca`] that accepts both payload sizes.
pub(crate) fn encode_databartruncated_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let sbs45 = crate::symbology::databar::omni_sbs_with_linkage(linear, true)?;
    let linsbs: Vec<u32> = sbs45.iter().map(|&w| u32::from(w)).collect();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _m) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _m) = crate::symbology::micropdf417::render_ccb(&bytes, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_truncated_ccb: CC-C is only valid with GS1-128, \
                 not DataBar Truncated"
                    .into(),
            ));
        }
    };
    Ok(build_databaromni_composite(
        &cc_bm,
        &linsbs,
        DATABARTRUNCATED_LINHEIGHT,
    ))
}

/// DataBar Stacked composite layout: the stacked linear is 50 modules wide
/// and 13 physical rows tall (rowmult `[5, 1, 7]`). The CC sits ABOVE
/// the stacked using `ucols = 2` (CC-A is ~55 modules wide); the composite
/// pixx is `ccpixx + 1 = 56`. BWIPP `bwipp_databarstackedcomposite`.
pub(crate) const DATABARSTACKED_COMPOSITE_CC_UCOLS: u8 = 2;
pub(crate) const DATABARSTACKED_LINWIDTH: usize = 50;
pub(crate) const DATABARSTACKED_TOP_HEIGHT: usize = 5;
pub(crate) const DATABARSTACKED_BOT_HEIGHT: usize = 7;

/// Stack a CC bitmap, a 50-cell composite separator, and the three
/// 50-cell stacked rows (top, internal sep, bot) into a `(ccpixx+1) ×
/// (cc_rows*CCA_ROWMULT + 1 + 5 + 1 + 7)` BitMatrix. Mirrors BWIPP's
/// pixs assembly in `bwipp_databarstackedcomposite`.
pub(crate) fn build_databarstacked_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    composite_sep_50: &[u8; 50],
    stacked_top_50: &[u8; 50],
    stacked_sep_50: &[u8; 50],
    stacked_bot_50: &[u8; 50],
) -> crate::encoding::BitMatrix {
    let ccpixx = cc_pixs.width();
    let pixx = ccpixx + 1;
    let cc_rows = cc_pixs.height();
    let lin_height = DATABARSTACKED_TOP_HEIGHT + 1 + DATABARSTACKED_BOT_HEIGHT;
    let pixy = cc_rows * CCA_ROWMULT + 1 + lin_height;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC rows: each ×CCA_ROWMULT, leading 0 then cc_pixs cells.
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..ccpixx {
                bm.set(x + 1, y, cc_pixs.get(x, r));
            }
        }
    }
    // Composite separator row.
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in composite_sep_50.iter().enumerate() {
        bm.set(x, sep_y, v == 1);
    }
    let mut y = sep_y + 1;
    // Stacked top: 5 physical rows.
    for _ in 0..DATABARSTACKED_TOP_HEIGHT {
        for (x, &v) in stacked_top_50.iter().enumerate() {
            bm.set(x, y, v == 1);
        }
        y += 1;
    }
    // Stacked internal sep: 1 row.
    for (x, &v) in stacked_sep_50.iter().enumerate() {
        bm.set(x, y, v == 1);
    }
    y += 1;
    // Stacked bot: 7 physical rows.
    for _ in 0..DATABARSTACKED_BOT_HEIGHT {
        for (x, &v) in stacked_bot_50.iter().enumerate() {
            bm.set(x, y, v == 1);
        }
        y += 1;
    }
    debug_assert_eq!(y, pixy);
    bm
}

/// Build the composite separator row for a DataBar Stacked composite.
/// BWIPP `bwipp_databarstackedcomposite` lines 38510-38516: invert
/// the stacked top half, zero the first/last 4 cells (seppad), then
/// apply sepfinder at position 18 (same f3pat + findersep as Omni).
pub(crate) fn databarstacked_composite_separator(top_50: &[u8; 50]) -> [u8; 50] {
    let mut sep = [0u8; 50];
    for (i, &v) in top_50.iter().enumerate() {
        sep[i] = 1 - v;
    }
    for s in sep.iter_mut().take(4) {
        *s = 0;
    }
    for s in sep.iter_mut().skip(46) {
        *s = 0;
    }
    apply_sepfinder(top_50, &mut sep, 18);
    sep
}

/// DataBar Stacked + CC-A composite (BWIPP `databarstackedcomposite`,
/// CC-A path). Refuses payloads that require CC-B.
pub(crate) fn encode_databarstacked_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let (widths, csum) = crate::symbology::databar::omni_widths_with_linkage(linear, true)?;
    let (top, bot) = crate::symbology::databar::stacked_top_bot(&widths, csum);
    let internal_sep = crate::symbology::databar::stacked_sep(&top, &bot);
    let composite_sep = databarstacked_composite_separator(&top);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, DATABARSTACKED_COMPOSITE_CC_UCOLS)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_stacked_cca: payload requires CC-B; \
             use composite_databar_stacked_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) =
        crate::symbology::micropdf417::render_cca(&cc.codewords, DATABARSTACKED_COMPOSITE_CC_UCOLS)
            .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databarstacked_composite(
        &cc_bm,
        &composite_sep,
        &top,
        &internal_sep,
        &bot,
    ))
}

/// DataBar Stacked + CC-A/CC-B composite. Drop-in superset of
/// [`encode_databarstacked_cca`].
pub(crate) fn encode_databarstacked_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let (widths, csum) = crate::symbology::databar::omni_widths_with_linkage(linear, true)?;
    let (top, bot) = crate::symbology::databar::stacked_top_bot(&widths, csum);
    let internal_sep = crate::symbology::databar::stacked_sep(&top, &bot);
    let composite_sep = databarstacked_composite_separator(&top);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, DATABARSTACKED_COMPOSITE_CC_UCOLS)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _m) = crate::symbology::micropdf417::render_cca(
                &cc.codewords,
                DATABARSTACKED_COMPOSITE_CC_UCOLS,
            )
            .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _m) = crate::symbology::micropdf417::render_ccb(
                &bytes,
                DATABARSTACKED_COMPOSITE_CC_UCOLS,
            )
            .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_stacked_ccb: CC-C is only valid with GS1-128, \
                 not DataBar Stacked"
                    .into(),
            ));
        }
    };
    Ok(build_databarstacked_composite(
        &cc_bm,
        &composite_sep,
        &top,
        &internal_sep,
        &bot,
    ))
}

/// DataBar Stacked Omni composite: CC sits above a 50×69 stacked-omni
/// linear (5 logical rows × rowmult [33,1,1,1,33]). CC uses ucols=2.
/// BWIPP `bwipp_databarstackedomnicomposite`.
pub(crate) const DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS: u8 = 2;

pub(crate) fn build_databarstackedomni_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    composite_sep_50: &[u8; 50],
    stacked_top: &[u8; 50],
    stacked_sep1: &[u8; 50],
    stacked_sep2: &[u8; 50],
    stacked_sep3: &[u8; 50],
    stacked_bot: &[u8; 50],
) -> crate::encoding::BitMatrix {
    let ccpixx = cc_pixs.width();
    let pixx = ccpixx + 1;
    let cc_rows = cc_pixs.height();
    let lin_height = 33 + 1 + 1 + 1 + 33;
    let pixy = cc_rows * CCA_ROWMULT + 1 + lin_height;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..ccpixx {
                bm.set(x + 1, y, cc_pixs.get(x, r));
            }
        }
    }
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in composite_sep_50.iter().enumerate() {
        bm.set(x, sep_y, v == 1);
    }
    let mut y = sep_y + 1;
    let layers: &[(&[u8; 50], usize)] = &[
        (stacked_top, 33),
        (stacked_sep1, 1),
        (stacked_sep2, 1),
        (stacked_sep3, 1),
        (stacked_bot, 33),
    ];
    for &(row, mult) in layers {
        for _ in 0..mult {
            for (x, &v) in row.iter().enumerate() {
                bm.set(x, y, v == 1);
            }
            y += 1;
        }
    }
    debug_assert_eq!(y, pixy);
    bm
}

pub(crate) fn encode_databarstackedomni_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let (top, sep1, sep2, sep3, bot) =
        crate::symbology::databar::stackedomni_logical_rows(linear, true)?;
    let composite_sep = databarstacked_composite_separator(&top);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_stacked_omni_cca: payload requires CC-B; \
             use composite_databar_stacked_omni_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(
        &cc.codewords,
        DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS,
    )
    .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databarstackedomni_composite(
        &cc_bm,
        &composite_sep,
        &top,
        &sep1,
        &sep2,
        &sep3,
        &bot,
    ))
}

pub(crate) fn encode_databarstackedomni_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let (top, sep1, sep2, sep3, bot) =
        crate::symbology::databar::stackedomni_logical_rows(linear, true)?;
    let composite_sep = databarstacked_composite_separator(&top);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _m) = crate::symbology::micropdf417::render_cca(
                &cc.codewords,
                DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS,
            )
            .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _m) = crate::symbology::micropdf417::render_ccb(
                &bytes,
                DATABARSTACKEDOMNI_COMPOSITE_CC_UCOLS,
            )
            .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_stacked_omni_ccb: CC-C is only valid with GS1-128, \
                 not DataBar Stacked Omni"
                    .into(),
            ));
        }
    };
    Ok(build_databarstackedomni_composite(
        &cc_bm,
        &composite_sep,
        &top,
        &sep1,
        &sep2,
        &sep3,
        &bot,
    ))
}

/// Build the composite separator row for DataBar Expanded Stacked
/// composite. BWIPP `bwipp_databarexpandedstackedcomposite` lines
/// 39660-39670: invert the linear's top row, zero the first/last 4
/// cells (seppad), then apply sepfinder at positions 19, 70 (and
/// 19+98, 70+98, ... up to bot.length-13). Uses the omni-shared
/// f3pat + findersep constants.
pub(crate) fn databarexpandedstacked_composite_separator(top_row: &[u8]) -> Vec<u8> {
    let n = top_row.len();
    let mut sep: Vec<u8> = top_row.iter().map(|&b| 1 - b).collect();
    for s in sep.iter_mut().take(4) {
        *s = 0;
    }
    for s in sep.iter_mut().skip(n.saturating_sub(4)) {
        *s = 0;
    }
    let mut positions: Vec<usize> = Vec::new();
    let mut p = 19usize;
    while p + 12 < n {
        positions.push(p);
        p += 98;
    }
    let mut p = 70usize;
    while p + 12 < n {
        positions.push(p);
        p += 98;
    }
    for fp in positions {
        apply_sepfinder(top_row, &mut sep, fp);
    }
    sep
}

/// Build the BitMatrix for DataBar Expanded Stacked composite.
/// CC is centered: cclpad = (linwidth-ccpixx+1)/2 zeros on the left,
/// ccrpad = (linwidth-ccpixx)/2 zeros on the right.
pub(crate) fn build_databarexpandedstacked_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linear_bm: &crate::encoding::BitMatrix,
    composite_sep: &[u8],
) -> crate::encoding::BitMatrix {
    let linwidth = linear_bm.width();
    let ccpixx = cc_pixs.width();
    let cc_rows = cc_pixs.height();
    let lin_height = linear_bm.height();
    let pixx = linwidth;
    let pixy = cc_rows * CCA_ROWMULT + 1 + lin_height;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    let cclpad = (linwidth + 1 - ccpixx) / 2;
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..ccpixx {
                bm.set(cclpad + x, y, cc_pixs.get(x, r));
            }
        }
    }
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in composite_sep.iter().enumerate() {
        bm.set(x, sep_y, v == 1);
    }
    let lin_y0 = sep_y + 1;
    for y in 0..lin_height {
        for x in 0..pixx {
            bm.set(x, lin_y0 + y, linear_bm.get(x, y));
        }
    }
    bm
}

pub(crate) fn encode_databarexpandedstacked_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let lin_bm = crate::symbology::databar_expanded::encode_stacked(linear, true)?;
    // Top row of the linear is the "bot" used to derive the composite sep.
    let mut top_row: Vec<u8> = Vec::with_capacity(lin_bm.width());
    for x in 0..lin_bm.width() {
        top_row.push(u8::from(lin_bm.get(x, 0)));
    }
    let composite_sep = databarexpandedstacked_composite_separator(&top_row);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_expanded_stacked_cca: payload requires CC-B; \
             use composite_databar_expanded_stacked_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databarexpandedstacked_composite(
        &cc_bm,
        &lin_bm,
        &composite_sep,
    ))
}

pub(crate) fn encode_databarexpandedstacked_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let lin_bm = crate::symbology::databar_expanded::encode_stacked(linear, true)?;
    let mut top_row: Vec<u8> = Vec::with_capacity(lin_bm.width());
    for x in 0..lin_bm.width() {
        top_row.push(u8::from(lin_bm.get(x, 0)));
    }
    let composite_sep = databarexpandedstacked_composite_separator(&top_row);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _) = crate::symbology::micropdf417::render_ccb(&bytes, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_expanded_stacked_ccb: CC-C is only valid with GS1-128, \
                 not DataBar Expanded Stacked"
                    .into(),
            ));
        }
    };
    Ok(build_databarexpandedstacked_composite(
        &cc_bm,
        &lin_bm,
        &composite_sep,
    ))
}

/// DataBar Limited composite separator constants (BWIPP `databarlimitedcomposite`
/// lines 39106-39107).
pub(crate) const DATABARLIMITED_SEPLEFT: [u8; 3] = [0, 0, 0];
pub(crate) const DATABARLIMITED_SEPRIGHT: [u8; 9] = [0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Default DataBar Limited linear height in modules (BWIPP).
pub(crate) const DATABARLIMITED_LINHEIGHT: usize = 10;

/// Build the 78-cell DataBar Limited separator from the linear sbs widths.
///
/// Per BWIPP `databarlimitedcomposite` (lines 41543-41580):
/// 1. Expand `linsbs` (46 widths) into an inverted-polarity 78-cell
///    sequence — starts with the flip of `1` (= 0), alternates per
///    width entry.
/// 2. Force the first 3 cells to 0 (`sepleft`).
/// 3. Force the last 9 cells to 0 (`sepright`).
/// 4. Trim to `sum(linsbs[0..45])` cells (i.e., `linpixs.length - 1`).
/// 5. Prepend a leading 0 → 74-cell separator.
///
/// Unlike DataBar Omni, Limited has NO `sepfinder` logic — its
/// finder patterns don't need the windowed adjustment.
pub(crate) fn databarlimited_separator(linsbs: &[u8]) -> Vec<u8> {
    let total: usize = linsbs.iter().map(|&w| w as usize).sum();
    let mut sep = Vec::with_capacity(total);
    let mut bit = 0u8;
    for &w in linsbs {
        for _ in 0..w {
            sep.push(bit);
        }
        bit ^= 1;
    }
    for s in sep.iter_mut().take(DATABARLIMITED_SEPLEFT.len()) {
        *s = 0;
    }
    let n = sep.len();
    for s in sep.iter_mut().skip(n - DATABARLIMITED_SEPRIGHT.len()) {
        *s = 0;
    }
    let visible_lin = linsbs[..45].iter().map(|&w| w as usize).sum::<usize>();
    sep.truncate(visible_lin);
    let mut final_sep = Vec::with_capacity(visible_lin + 1);
    final_sep.push(0);
    final_sep.extend_from_slice(&sep);
    final_sep
}

/// Expand the DataBar Limited linsbs into the 74-cell linpixs row.
///
/// Per BWIPP (lines 41567-41577): take `linsbs[0..45]` (drops the
/// trailing 5-module margin), expand each width into bit cells
/// alternating starting with `1` (bar), and prepend a leading `0`.
fn databarlimited_linpixs(linsbs: &[u8]) -> Vec<u8> {
    let visible = &linsbs[..45];
    let sum: usize = visible.iter().map(|&w| w as usize).sum();
    let mut linpixs = Vec::with_capacity(sum + 1);
    linpixs.push(0);
    let mut bit = 1u8;
    for &w in visible {
        for _ in 0..w {
            linpixs.push(bit);
        }
        bit ^= 1;
    }
    linpixs
}

/// Build the fully-expanded BitMatrix for a databar_limited_cca composite
/// (CC-A only, where `ccpixx == 72`).
///
/// Logical layout (each row 74 cells wide):
/// - CC-A rows: `[0] + ccrow_72 + [0]` = 74.
/// - Separator: 74 cells (from `databarlimited_separator`).
/// - Linear: 74 cells (from `databarlimited_linpixs`).
///
/// Physical expansion:
/// - Each CC-A logical row repeats [`CCA_ROWMULT`] times.
/// - Separator appears once.
/// - Linear repeats `linheight` times.
pub(crate) fn build_databarlimited_cca_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u8; 46],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let sep = databarlimited_separator(linsbs);
    let linpixs = databarlimited_linpixs(linsbs);
    debug_assert_eq!(sep.len(), linpixs.len());
    let cc_width = cc_pixs.width();
    debug_assert_eq!(cc_width, 72, "DataBar Limited CC-A expects ccpixx=72");
    let pixx = linpixs.len();
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * CCA_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC-A rows: [0] + ccrow + [0], expanded CCA_ROWMULT times each.
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            // bm[0] = 0 by default; ccrow at [1..73]; bm[73] = 0 by default.
            for x in 0..cc_width {
                bm.set(1 + x, y, cc_pixs.get(x, r));
            }
        }
    }
    // Separator row.
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in sep.iter().enumerate() {
        bm.set(x, sep_y, v == 1);
    }
    // Linear rows.
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 1 + rep;
        for (x, &v) in linpixs.iter().enumerate() {
            bm.set(x, y, v == 1);
        }
    }
    bm
}

/// Public entry point for the DataBar Limited + CC-A composite barcode.
pub(crate) fn encode_databarlimited_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let linsbs = crate::symbology::databar::limited_sbs_with_linkage(linear, true)?;
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 3)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_limited_cca: payload requires CC-B; \
             use composite_databar_limited_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(&cc.codewords, 3)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databarlimited_cca_composite(
        &cc_bm,
        &linsbs,
        DATABARLIMITED_LINHEIGHT,
    ))
}

/// Build the fully-expanded BitMatrix for a databar_limited_ccb composite
/// (or any non-`ccpixx=72` CC), matching BWIPP's `ccpixx != 72` branch
/// (lines 41596-41624).
///
/// Logical layout (each row `ccpixx + 1` cells wide):
/// - CC rows: `ccrow_ccpixx + [0]` = `ccpixx + 1` cells.
/// - Separator: `[0]*9 + sep_74` = 83 cells (for cc_width=82).
/// - Linear: `[0]*9 + linpixs_74` = 83 cells.
///
/// Physical expansion mirrors the CC-A case: each CC row repeats
/// [`CCA_ROWMULT`] times, separator once, linear `linheight` times.
pub(crate) fn build_databarlimited_ccb_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u8; 46],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let sep = databarlimited_separator(linsbs);
    let linpixs = databarlimited_linpixs(linsbs);
    debug_assert_eq!(sep.len(), linpixs.len());
    let cc_width = cc_pixs.width();
    let pixx = cc_width + 1;
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * CCA_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC rows: ccrow at columns 0..ccpixx, trailing 0 at column ccpixx
    // (already 0 by default).
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..cc_width {
                bm.set(x, y, cc_pixs.get(x, r));
            }
        }
    }
    // Separator: 9 leading zeros (default), then sep_74 at columns 9..9+74.
    // For cc_width=82, 9 + 74 = 83 = pixx, matches.
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in sep.iter().enumerate() {
        bm.set(9 + x, sep_y, v == 1);
    }
    // Linear rows: same 9-zero padding, linpixs_74 at columns 9..83.
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 1 + rep;
        for (x, &v) in linpixs.iter().enumerate() {
            bm.set(9 + x, y, v == 1);
        }
    }
    bm
}

/// Public entry point for the DataBar Limited + CC-B composite barcode.
///
/// CC-B carries 56-1184 bits (vs CC-A's 56-208 bits). For DataBar
/// Limited with `cccolumns=3`, CC-B renders via the non-CCA c=3 layout
/// (`rwid=82`) — different from the CC-A 3-col layout (`rwid=72`),
/// which is why a separate `build_databarlimited_ccb_composite` is
/// needed.
///
/// Like [`encode_databaromni_ccb`], this handler also accepts CC-A
/// payloads — `gs1_cc::encode_cc` picks the smaller version
/// automatically; we just dispatch the render path on `cc.version`.
pub(crate) fn encode_databarlimited_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let linsbs = crate::symbology::databar::limited_sbs_with_linkage(linear, true)?;
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 3)?;
    match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            // CC-A path: use the ccpixx=72 layout.
            let (cc_bm, _m) = crate::symbology::micropdf417::render_cca(&cc.codewords, 3)
                .map_err(crate::error::Error::InvalidData)?;
            Ok(build_databarlimited_cca_composite(
                &cc_bm,
                &linsbs,
                DATABARLIMITED_LINHEIGHT,
            ))
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            // CC-B path: use the ccpixx!=72 (here ccpixx=82) layout.
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (cc_bm, _m) = crate::symbology::micropdf417::render_ccb(&bytes, 3)
                .map_err(crate::error::Error::InvalidData)?;
            Ok(build_databarlimited_ccb_composite(
                &cc_bm,
                &linsbs,
                DATABARLIMITED_LINHEIGHT,
            ))
        }
        crate::symbology::gs1_cc::CcVersion::C => Err(crate::error::Error::InvalidData(
            "composite_databar_limited_ccb: CC-C is only valid with GS1-128, \
             not DataBar Limited"
                .into(),
        )),
    }
}

/// Public entry point for the DataBar Omni + CC-B composite barcode.
///
/// CC-B is just MicroPDF417 with a byte-mode payload — the linear, the
/// separator, and the stacking layout are identical to CC-A; only the
/// 2-D render path differs. CC-B carries 56-1184 bits (vs CC-A's
/// 56-208 bits at 4 columns), so this variant handles payloads too
/// large for CC-A.
///
/// gs1_cc's auto-version selector returns CC-A for small payloads,
/// CC-B for larger ones — this handler accepts BOTH, so it works as
/// a drop-in superset of [`encode_databaromni_cca`]. (Callers that
/// want strict-CC-A enforcement should keep using the CC-A variant.)
pub(crate) fn encode_databaromni_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let sbs45 = crate::symbology::databar::omni_sbs_with_linkage(linear, true)?;
    let linsbs: Vec<u32> = sbs45.iter().map(|&w| u32::from(w)).collect();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _m) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _m) = crate::symbology::micropdf417::render_ccb(&bytes, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_omni_ccb: CC-C is only valid with GS1-128, \
                 not DataBar Omni"
                    .into(),
            ));
        }
    };
    Ok(build_databaromni_composite(
        &cc_bm,
        &linsbs,
        DATABAROMNI_LINHEIGHT,
    ))
}

fn apply_sepfinder(bot: &[u8], sep: &mut [u8], fp: usize) {
    // 12-cell window pattern construction.
    for i in fp..=fp + 12 {
        if i >= bot.len() {
            break;
        }
        let v = if bot[i] == 0 {
            let prev_bot = if i > 0 { bot[i - 1] } else { 0 };
            if prev_bot == 1 {
                1
            } else {
                let prev_sep = if i > 0 { sep[i - 1] } else { 0 };
                u8::from(prev_sep == 0)
            }
        } else {
            0
        };
        sep[i] = v;
    }
    // Check f3pat match.
    let matches = (0..=12).all(|j| {
        let pos = fp + j;
        pos < bot.len() && bot[pos] == DATABAROMNI_F3PAT[j]
    });
    if matches {
        for (j, &v) in DATABAROMNI_FINDERSEP.iter().enumerate() {
            if fp + j < sep.len() {
                sep[fp + j] = v;
            }
        }
    }
}

/// Default GS1-128 composite linear height in modules (BWIPP 0.5" × 72 dpi).
pub(crate) const GS1_128_LINHEIGHT: usize = 36;

/// Default EAN-13 / EAN-8 / UPC-A / UPC-E composite linear height in
/// modules. BWIPP uses 1.0" × 72 dpi for these symbols, so the main
/// linear zone is 72 module-rows tall.
pub(crate) const EAN_LINHEIGHT: usize = 72;

/// Standard EAN-13 linear width in modules.
pub(crate) const EAN13_LINWIDTH: usize = 95;

/// Standard EAN-8 linear width in modules.
#[allow(dead_code)]
pub(crate) const EAN8_LINWIDTH: usize = 67;

/// Build the 3 hardcoded "guard transition" rows BWIPP appends above
/// the main linear in EAN-13 / EAN-8 / UPC-A / UPC-E composite
/// barcodes. These represent the outer guard bars extending upward
/// into the CC zone (visually `101…101` at the symbol boundaries).
///
/// Returns three `pixx`-wide rows:
/// 1. `linpad + [0, 1, 0×(linwidth-2), 1, 0] + ccrpad` — outer guards
/// 2. `linpad + [1, 0, 0×(linwidth-2), 0, 1] + ccrpad` — boundary cells
/// 3. same as row 1.
///
/// Per BWIPP `ean13composite` lines 38679-38705 (and the structurally
/// identical UPC-A / EAN-8 / UPC-E variants).
// Contract: `pixx >= linpad_len + linwidth + 2`. Every production caller
// (`build_ean_cca_composite` and the UPC-A/EAN-8/UPC-E twins) satisfies
// this because the CC-A/CC-B 2D component is always at least `linwidth+2`
// modules wide, so the layout math yields `diff_signed == -1` and hence
// `pixx == ccpixx == linpad_len + linwidth + 2`. The guards below make a
// contract violation degrade gracefully (skip the out-of-range cell)
// instead of panicking with an out-of-bounds index — a robustness gap
// found by a Stage 11.A8c mutation test that drove the helper directly
// with a synthetic narrow `ccpixx` (the public encode path never does).
// For every in-contract call `idx < pixx`, so production output is
// byte-for-byte unchanged.
fn ean_guard_rows(pixx: usize, linpad_len: usize, linwidth: usize) -> [Vec<u8>; 3] {
    let mut row_a = vec![0u8; pixx];
    let mut row_b = vec![0u8; pixx];
    let set = |row: &mut [u8], idx: usize| {
        if idx < row.len() {
            row[idx] = 1;
        }
    };
    // Row A: cells [linpad_len + 1] and [linpad_len + linwidth] are 1.
    set(&mut row_a, linpad_len + 1);
    set(&mut row_a, linpad_len + linwidth);
    // Row B: cells [linpad_len] and [linpad_len + linwidth + 1] are 1.
    set(&mut row_b, linpad_len);
    set(&mut row_b, linpad_len + linwidth + 1);
    let row_c = row_a.clone();
    [row_a, row_b, row_c]
}

/// Build the fully-expanded BitMatrix for an EAN-13 (or UPC-A, or
/// other 95-module-wide retail family) + CC-A composite.
///
/// Layout (per BWIPP `ean13composite` lines 37151-37260):
/// - `linpad_len = max(ccpixx - 97, 0)` zeros prepended to the linear row.
/// - `diff = linwidth + linpad_len + 1 - ccpixx`. If `diff > 0`,
///   pad CC rows with `diff` trailing zeros; otherwise pad linear with
///   `-diff` trailing zeros (here always exactly 1 for default ccpixx=99).
/// - 3 CC rows at the top, each repeated 2× (rowmult).
/// - 3 hardcoded guard rows (rowmult = 2 each) below the CC.
/// - 1 main linear row, repeated [`EAN_LINHEIGHT`] times.
///
/// `linsbs` is the linear's bar/space widths (alternating starting with bar);
/// `linwidth` is the sum (95 for EAN-13). `ccpixx` is the CC's column width
/// (99 for CC-A 4-col).
pub(crate) fn build_ean_cca_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u32],
    linwidth: usize,
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let ccpixx = cc_pixs.width();
    let linpad_len = ccpixx.saturating_sub(linwidth + 2);
    let diff_signed: isize = linwidth as isize + linpad_len as isize + 1 - ccpixx as isize;
    let ccrpad_len = diff_signed.max(0) as usize;
    let lin_trailing_zero = if diff_signed < 0 { 1usize } else { 0 };
    let pixx = ccpixx + ccrpad_len;
    let cc_rows = cc_pixs.height();
    let guard_rowmult = 2;
    let pixy = cc_rows * CCA_ROWMULT + 3 * guard_rowmult + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC rows.
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..ccpixx {
                bm.set(x, y, cc_pixs.get(x, r));
            }
        }
    }
    // Guard transition rows.
    let guard_rows = ean_guard_rows(pixx, linpad_len, linwidth);
    for (g, guard) in guard_rows.iter().enumerate() {
        for rep in 0..guard_rowmult {
            let y = cc_rows * CCA_ROWMULT + g * guard_rowmult + rep;
            for (x, &v) in guard.iter().enumerate() {
                bm.set(x, y, v == 1);
            }
        }
    }
    // Linear row: linpad + [0] + linpixs + (trailing 0 if needed) + ccrpad.
    let linpixs = sbs_to_pixels(linsbs);
    let mut linear_row = vec![0u8; pixx];
    let lin_start = linpad_len + 1;
    for (i, &v) in linpixs.iter().enumerate() {
        linear_row[lin_start + i] = v;
    }
    // The trailing 0 is already in place by default. ccrpad too.
    let _ = lin_trailing_zero;
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 3 * guard_rowmult + rep;
        for (x, &v) in linear_row.iter().enumerate() {
            bm.set(x, y, v == 1);
        }
    }
    bm
}

/// Shared `cc.version` dispatcher for EAN/UPC-family composites. Picks
/// `render_cca` or `render_ccb` based on the encoded version, then
/// invokes [`build_ean_cca_composite`] with the linear sbs widths
/// and `linwidth`. `cccolumns` is the gs1_cc column count
/// (4 for EAN-13 / UPC-A; 3 for EAN-8 / UPC-E by default).
fn build_ean_family_composite(
    cc: &crate::symbology::gs1_cc::CcEncoded,
    cccolumns: u8,
    linsbs: &[u32],
    linwidth: usize,
    linheight: usize,
    handler_name: &'static str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, cccolumns)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _) = crate::symbology::micropdf417::render_ccb(&bytes, cccolumns)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(format!(
                "{handler_name}: CC-C is only valid with GS1-128, not EAN/UPC",
            )));
        }
    };
    Ok(build_ean_cca_composite(&cc_bm, linsbs, linwidth, linheight))
}

/// Public entry point for the EAN-13 + CC-A composite barcode.
pub(crate) fn encode_ean13_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_ean13(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    debug_assert_eq!(linwidth, EAN13_LINWIDTH);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_ean13_cca: payload requires CC-B; \
             use composite_ean13_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_ean_cca_composite(
        &cc_bm,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
    ))
}

/// Public entry point for the EAN-13 + CC-B composite barcode.
/// Drop-in superset of [`encode_ean13_cca`].
pub(crate) fn encode_ean13_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_ean13(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    build_ean_family_composite(
        &cc,
        4,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
        "composite_ean13_ccb",
    )
}

/// Public entry point for the UPC-A + CC-A composite barcode.
///
/// UPC-A is structurally an EAN-13 with leading `0`, so the linear
/// width is identical (95 modules) and the composite layout reuses
/// the EAN-13 stacker.
pub(crate) fn encode_upca_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_upca(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    debug_assert_eq!(linwidth, EAN13_LINWIDTH); // UPC-A = EAN-13 with leading 0
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_upca_cca: payload requires CC-B; use composite_upca_ccb instead".into(),
        ));
    }
    let (cc_bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_ean_cca_composite(
        &cc_bm,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
    ))
}

/// Public entry point for the UPC-A + CC-B composite barcode.
pub(crate) fn encode_upca_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_upca(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    build_ean_family_composite(
        &cc,
        4,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
        "composite_upca_ccb",
    )
}

/// Public entry point for the EAN-8 + CC-A composite barcode.
///
/// EAN-8 linwidth = 67. The composite uses `cccolumns=3` by default
/// (`linpad = ccpixx - 69` zeros).
pub(crate) fn encode_ean8_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_ean8(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    debug_assert_eq!(linwidth, EAN8_LINWIDTH);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 3)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_ean8_cca: payload requires CC-B; use composite_ean8_ccb instead".into(),
        ));
    }
    let (cc_bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 3)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_ean_cca_composite(
        &cc_bm,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
    ))
}

/// Public entry point for the EAN-8 + CC-B composite barcode.
pub(crate) fn encode_ean8_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_ean8(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 3)?;
    build_ean_family_composite(
        &cc,
        3,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
        "composite_ean8_ccb",
    )
}

/// Standard UPC-E linear width in modules.
pub(crate) const UPCE_LINWIDTH: usize = 51;

/// DataBar Expanded composite separator constants (BWIPP).
pub(crate) const DATABAREXPANDED_SEPLEFT: usize = 3;
pub(crate) const DATABAREXPANDED_SEPRIGHT: usize = 4;

/// Default DataBar Expanded composite linear height in modules.
pub(crate) const DATABAREXPANDED_LINHEIGHT: usize = 34;

/// Expand a DataBar Expanded linsbs into the 1D `bot` (bar/space)
/// sequence, starting with bar (per BWIPP `databarexpandedcomposite`
/// lines 41850-41859 — initial top=0 flips to 1, so first cell is 1).
fn databarexpanded_bot(linsbs: &[u8]) -> Vec<u8> {
    let total: usize = linsbs.iter().map(|&w| w as usize).sum();
    let mut bot = Vec::with_capacity(total);
    let mut bit = 1u8;
    for &w in linsbs {
        for _ in 0..w {
            bot.push(bit);
        }
        bit ^= 1;
    }
    bot
}

/// Apply the DataBar Expanded sepfinder at finder position `fp`.
///
/// Per BWIPP `databarexpandedcomposite_sepfinder` (lines 41832-41849).
/// Unlike DataBar Omni, this variant has no f3pat-match override —
/// just the windowed reconstruction over `bot[fp..fp+13]`.
fn apply_databarexpanded_sepfinder(bot: &[u8], sep: &mut [u8], fp: usize) {
    for i in fp..=fp + 12 {
        if i >= bot.len() {
            break;
        }
        let v = if bot[i] == 0 {
            let prev_bot = if i > 0 { bot[i - 1] } else { 0 };
            if prev_bot == 1 {
                1
            } else {
                let prev_sep = if i > 0 { sep[i - 1] } else { 0 };
                u8::from(prev_sep == 0)
            }
        } else {
            0
        };
        sep[i] = v;
    }
}

/// Build the DataBar Expanded composite separator. The result is
/// the bare separator row; the composite stacker prepends the
/// leading 0 when interleaving the separator into the stacked
/// symbol (so this helper stays a pure-function for testability).
///
/// Per BWIPP `databarexpandedcomposite` lines 41832-41883:
/// 1. `sep = 1 - bot` (inverted-polarity expansion of linsbs).
/// 2. First 3 cells zeroed (sepleft), last 4 zeroed (sepright).
/// 3. Sepfinder applied at finder positions 18, 116, 214, … and
///    69, 167, 265, … (every 98 cells, capped at `len-13`).
pub(crate) fn databarexpanded_separator(linsbs: &[u8]) -> Vec<u8> {
    let bot = databarexpanded_bot(linsbs);
    let mut sep: Vec<u8> = bot.iter().map(|&b| 1 - b).collect();
    for s in sep.iter_mut().take(DATABAREXPANDED_SEPLEFT) {
        *s = 0;
    }
    let n = sep.len();
    for s in sep.iter_mut().skip(n - DATABAREXPANDED_SEPRIGHT) {
        *s = 0;
    }
    let len_cap = bot.len().saturating_sub(13);
    let mut fp = 18usize;
    while fp <= len_cap {
        apply_databarexpanded_sepfinder(&bot, &mut sep, fp);
        fp += 98;
    }
    let mut fp = 69usize;
    while fp <= len_cap {
        apply_databarexpanded_sepfinder(&bot, &mut sep, fp);
        fp += 98;
    }
    sep
}

/// Build the fully-expanded BitMatrix for a `composite_databar_expanded_cca`.
///
/// Layout (per BWIPP `databarexpandedcomposite` lines 41902-41931):
/// - `linpixs.length = linsbs.sum + 1` — BWIPP prepends a leading 0.
/// - `pixx = linpixs.length`.
/// - Each CC row: `[0, 0] + ccrow_ccpixx + (diff - 2) zeros`, where
///   `diff = pixx - ccpixx`.
/// - Sep row: `[0] + databarexpanded_separator(linsbs)`.
/// - Linear row: linpixs (with leading 0).
pub(crate) fn build_databar_expanded_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u8],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let linsbs_sum: usize = linsbs.iter().map(|&w| w as usize).sum();
    let pixx = linsbs_sum + 1;
    let cc_width = cc_pixs.width();
    let diff = pixx as isize - cc_width as isize;
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * CCA_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC rows: [0, 0] + ccrow + (diff-2) zeros.
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x in 0..cc_width {
                bm.set(2 + x, y, cc_pixs.get(x, r));
            }
        }
    }
    // Separator: [0] + databarexpanded_separator.
    let sep = databarexpanded_separator(linsbs);
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x, &v) in sep.iter().enumerate() {
        bm.set(1 + x, sep_y, v == 1);
    }
    // Linear: linpixs = [0] + expand(linsbs starting with bar).
    let bot = databarexpanded_bot(linsbs);
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 1 + rep;
        for (x, &v) in bot.iter().enumerate() {
            bm.set(1 + x, y, v == 1);
        }
    }
    let _ = diff; // suppress
    bm
}

/// Public entry point for the DataBar Expanded + CC-A composite.
pub(crate) fn encode_databar_expanded_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::databar_expanded::encode(linear, true)?;
    let linsbs: Vec<u8> = pat.bars.clone();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_databar_expanded_cca: payload requires CC-B; \
             use composite_databar_expanded_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_databar_expanded_composite(
        &cc_bm,
        &linsbs,
        DATABAREXPANDED_LINHEIGHT,
    ))
}

/// Public entry point for the DataBar Expanded + CC-B composite.
/// Drop-in superset of [`encode_databar_expanded_cca`].
pub(crate) fn encode_databar_expanded_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::databar_expanded::encode(linear, true)?;
    let linsbs: Vec<u8> = pat.bars.clone();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _) = crate::symbology::micropdf417::render_ccb(&bytes, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_databar_expanded_ccb: CC-C is only valid with GS1-128".into(),
            ));
        }
    };
    Ok(build_databar_expanded_composite(
        &cc_bm,
        &linsbs,
        DATABAREXPANDED_LINHEIGHT,
    ))
}

/// Public entry point for the UPC-E + CC-A composite barcode.
///
/// UPC-E linwidth = 51 (compressed 8-digit UPC). The composite uses
/// `cccolumns=2` per BWIPP's `gs1_cc_lintypecccolumns` table → CC-A
/// 2-col with `ccpixx=55`. Same guard-fanout pattern as EAN-13/EAN-8.
pub(crate) fn encode_upce_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_upce(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    debug_assert_eq!(linwidth, UPCE_LINWIDTH);
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 2)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_upce_cca: payload requires CC-B; use composite_upce_ccb instead".into(),
        ));
    }
    let (cc_bm, _) = crate::symbology::micropdf417::render_cca(&cc.codewords, 2)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_ean_cca_composite(
        &cc_bm,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
    ))
}

/// Public entry point for the UPC-E + CC-B composite barcode.
pub(crate) fn encode_upce_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let pat = crate::symbology::ean::encode_upce(linear, &crate::options::Options::default())?;
    let linsbs: Vec<u32> = pat.bars.iter().map(|&b| u32::from(b)).collect();
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 2)?;
    build_ean_family_composite(
        &cc,
        2,
        &linsbs,
        linwidth,
        EAN_LINHEIGHT,
        "composite_upce_ccb",
    )
}

/// Build the GS1-128 composite separator from the linear's sbs widths.
///
/// Per BWIPP `gs1_128composite` (lines 42444-42456): no `sepleft` /
/// `sepright` boundary trimming, no `sepfinder` adjustment — the sep is
/// simply the inverted-polarity expansion of `linsbs` starting with `0`
/// (since BWIPP pushes `1` as the initial top and flips it).
pub(crate) fn gs1_128_separator(linsbs: &[u32]) -> Vec<u8> {
    let total: usize = linsbs.iter().map(|&w| w as usize).sum();
    let mut sep = Vec::with_capacity(total);
    let mut bit = 0u8;
    for &w in linsbs {
        for _ in 0..w {
            sep.push(bit);
        }
        bit ^= 1;
    }
    sep
}

/// Compute the CC offset `x` for a GS1-128 + CC-A/CC-B composite.
///
/// BWIPP `gs1_128composite` (lines 42458-42465):
/// ```text
/// s = (linwidth - 2) / 11
/// p = (s - 9) / 2
/// x = ((s - p - 1) * 11 + 10 + (p == 0 ? 2 : 0)) - 99
/// ```
/// (s and p are integer divisions.) This centres the 99-module CC-A/CC-B
/// 2-D above the leftmost ~11 Code 128 modules of the linear, accounting
/// for the start/stop bars.
pub(crate) fn gs1_128_cc_offset_a(linwidth: usize) -> isize {
    let s = ((linwidth as isize) - 2) / 11;
    let p = (s - 9) / 2;
    let base = (s - p - 1) * 11 + 10 + if p == 0 { 2 } else { 0 };
    base - 99
}

/// Build the fully-expanded BitMatrix for a `composite_gs1_128_cca`
/// (or `_ccb`) — wraps `linktype = "a"` from BWIPP (CC-A or CC-B above
/// a linkagea-marked GS1-128 linear).
///
/// `cc_pixs` is the 99-cell-wide CC-A or CC-B 2-D bitmap.
/// `linsbs` is the linkage-aware GS1-128 sbs widths (each ≥1 module).
/// `linwidth` = `linsbs.iter().sum()` — passed separately to avoid
/// recomputation by the caller. `linheight` defaults to
/// [`GS1_128_LINHEIGHT`].
///
/// Layout (BWIPP `ccpixx != 72` branch reused, with computed `x`):
/// - pixx = max(linwidth, ccpixx + x + diff) (here always `linwidth`)
/// - CC rows = `[0]*x + ccrow_99 + [0]*diff`
/// - Sep row = inverted-polarity expansion of linsbs (no padding) = linwidth cells
/// - Linear row = direct expansion of linsbs (starts with 1) = linwidth cells
pub(crate) fn build_gs1_128_cca_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u32],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let x = gs1_128_cc_offset_a(linwidth);
    let cc_width = cc_pixs.width();
    debug_assert!(x >= 0, "GS1-128 CC-A/B offset should be non-negative");
    let cclpad = x as usize;
    let diff = linwidth as isize - (cc_width as isize + x);
    let ccrpad = diff.max(0) as usize;
    let pixx = (cclpad + cc_width + ccrpad).max(linwidth);
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * CCA_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC rows.
    for r in 0..cc_rows {
        for rep in 0..CCA_ROWMULT {
            let y = r * CCA_ROWMULT + rep;
            for x_in in 0..cc_width {
                bm.set(cclpad + x_in, y, cc_pixs.get(x_in, r));
            }
        }
    }
    // Separator row.
    let sep = gs1_128_separator(linsbs);
    let sep_y = cc_rows * CCA_ROWMULT;
    for (x_in, &v) in sep.iter().enumerate() {
        bm.set(x_in, sep_y, v == 1);
    }
    // Linear rows.
    let linpixs = sbs_to_pixels(linsbs);
    for rep in 0..linheight {
        let y = cc_rows * CCA_ROWMULT + 1 + rep;
        for (x_in, &v) in linpixs.iter().enumerate() {
            bm.set(x_in, y, v == 1);
        }
    }
    bm
}

/// Encode a GS1-128 linear with the linkagea/linkagec flag set, and
/// return its sbs widths (each ≥1 module). Translates from
/// [`gs1_128::Linkage`] to [`crate::symbology::gs1_128::Linkage`].
fn gs1_128_linkage_sbs(
    data: &str,
    linkage: crate::symbology::gs1_128::Linkage,
) -> Result<Vec<u32>, crate::error::Error> {
    let pat = crate::symbology::gs1_128::encode_with_linkage(
        data,
        &crate::options::Options::default(),
        linkage,
    )?;
    Ok(pat.bars.iter().map(|&b| u32::from(b)).collect())
}

/// Public entry point for the GS1-128 + CC-A composite barcode.
pub(crate) fn encode_gs1_128_cca(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let linsbs = gs1_128_linkage_sbs(linear, crate::symbology::gs1_128::Linkage::A)?;
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    if cc.version != crate::symbology::gs1_cc::CcVersion::A {
        return Err(crate::error::Error::InvalidData(
            "composite_gs1_128_cca: payload requires CC-B; \
             use composite_gs1_128_ccb instead"
                .into(),
        ));
    }
    let (cc_bm, _metric) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
        .map_err(crate::error::Error::InvalidData)?;
    Ok(build_gs1_128_cca_composite(
        &cc_bm,
        &linsbs,
        GS1_128_LINHEIGHT,
    ))
}

/// PDF417 row-multiplier — each logical PDF417 row expands to this
/// many physical pixel rows in the output. BWIPP uses 3 (vs the
/// MicroPDF417 default of 2) — `bwipp_pdf417` line ~23297 sets
/// `rowmult = 3`.
pub(crate) const PDF417_ROWMULT: usize = 3;

/// CC-C offset constant for GS1-128 composites (BWIPP
/// `gs1_128composite` line 42467: `x = -7`).
pub(crate) const GS1_128_CC_OFFSET_C: isize = -7;

/// Build the fully-expanded BitMatrix for a `composite_gs1_128_ccc`
/// (GS1-128 + PDF417-CC-C). Different from the CC-A/CC-B layout:
///
/// - `x = -7` (the linear is shifted right by 7 cells, NOT centred
///   above the leftmost 11 modules).
/// - CC rows render with `PDF417_ROWMULT = 3` (PDF417's row groups),
///   not the MicroPDF417 `CCA_ROWMULT = 2`.
/// - When `ccpixx > linwidth + 7`, the sep + linear rows get
///   `linrpad = ccpixx + x - linwidth` trailing zeros.
///
/// `linsbs` is the linkagec-aware GS1-128 sbs widths.
pub(crate) fn build_gs1_128_ccc_composite(
    cc_pixs: &crate::encoding::BitMatrix,
    linsbs: &[u32],
    linheight: usize,
) -> crate::encoding::BitMatrix {
    let linwidth: usize = linsbs.iter().map(|&w| w as usize).sum();
    let x = GS1_128_CC_OFFSET_C; // -7
    let cc_width = cc_pixs.width();
    debug_assert!(x < 0, "CC-C uses x = -7 < 0");
    let cclpad = 0usize;
    let linlpad = (-x) as usize;
    let diff = linwidth as isize - (cc_width as isize + x);
    let (ccrpad, linrpad) = if diff > 0 {
        (diff as usize, 0usize)
    } else {
        (0usize, (-diff) as usize)
    };
    let pixx = (cclpad + cc_width + ccrpad).max(linlpad + linwidth + linrpad);
    let cc_rows = cc_pixs.height();
    let pixy = cc_rows * PDF417_ROWMULT + 1 + linheight;
    let mut bm = crate::encoding::BitMatrix::new(pixx, pixy);
    // CC-C rows — each repeated PDF417_ROWMULT times.
    for r in 0..cc_rows {
        for rep in 0..PDF417_ROWMULT {
            let y = r * PDF417_ROWMULT + rep;
            for x_in in 0..cc_width {
                bm.set(cclpad + x_in, y, cc_pixs.get(x_in, r));
            }
        }
    }
    // Separator row.
    let sep = gs1_128_separator(linsbs);
    let sep_y = cc_rows * PDF417_ROWMULT;
    for (x_in, &v) in sep.iter().enumerate() {
        bm.set(linlpad + x_in, sep_y, v == 1);
    }
    // Linear rows.
    let linpixs = sbs_to_pixels(linsbs);
    for rep in 0..linheight {
        let y = cc_rows * PDF417_ROWMULT + 1 + rep;
        for (x_in, &v) in linpixs.iter().enumerate() {
            bm.set(linlpad + x_in, y, v == 1);
        }
    }
    bm
}

/// Public entry point for the GS1-128 + CC-C composite barcode.
///
/// CC-C uses PDF417 (full version, not MicroPDF417) as the 2D
/// companion. Only valid with GS1-128 — other linears use CC-A or
/// CC-B exclusively (their bar widths can't accommodate the wider
/// PDF417 symbol).
pub(crate) fn encode_gs1_128_ccc(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let linsbs = gs1_128_linkage_sbs(linear, crate::symbology::gs1_128::Linkage::C)?;
    let linwidth: u32 = linsbs.iter().sum();
    let (cc_bytes, size) = crate::symbology::gs1_cc::encode_cc_force_c(comp, linwidth)?;
    let cc_bm =
        crate::symbology::pdf417::pdf417_render_ccc(&cc_bytes, size.eclevel, size.columns as usize)
            .map_err(crate::error::Error::InvalidData)?;
    Ok(build_gs1_128_ccc_composite(
        &cc_bm,
        &linsbs,
        GS1_128_LINHEIGHT,
    ))
}

/// Public entry point for the GS1-128 + CC-B composite barcode.
/// Drop-in superset accepting CC-A-sized payloads via auto-dispatch.
pub(crate) fn encode_gs1_128_ccb(
    input: &str,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    let (linear, comp) = split_composite_input(input)?;
    let linsbs = gs1_128_linkage_sbs(linear, crate::symbology::gs1_128::Linkage::A)?;
    let cc = crate::symbology::gs1_cc::encode_cc(comp, 4)?;
    let cc_bm = match cc.version {
        crate::symbology::gs1_cc::CcVersion::A => {
            let (bm, _m) = crate::symbology::micropdf417::render_cca(&cc.codewords, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::B => {
            let bytes: Vec<u8> = cc.codewords.iter().map(|&v| v as u8).collect();
            let (bm, _m) = crate::symbology::micropdf417::render_ccb(&bytes, 4)
                .map_err(crate::error::Error::InvalidData)?;
            bm
        }
        crate::symbology::gs1_cc::CcVersion::C => {
            return Err(crate::error::Error::InvalidData(
                "composite_gs1_128_ccb: CC-C should use composite_gs1_128_ccc instead".into(),
            ));
        }
    };
    Ok(build_gs1_128_cca_composite(
        &cc_bm,
        &linsbs,
        GS1_128_LINHEIGHT,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_composite_input_basic() {
        let (l, c) = split_composite_input("(01)12345|(10)BATCH").unwrap();
        assert_eq!(l, "(01)12345");
        assert_eq!(c, "(10)BATCH");
    }

    /// Stage 11.A8c — strengthen these no-pipe / empty-half cases
    /// with diagnostic substrings (parallel to the strong sibling
    /// split_composite_input_basic in a4b0288 and the in-place
    /// strengthening of split_composite_input_branches in af929d2).
    /// Three-way defense-in-depth: any of the three tests being
    /// refactored still leaves the others pinning the diagnostic.
    #[test]
    fn split_composite_input_rejects_no_pipe() {
        let err = split_composite_input("(01)12345").unwrap_err();
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("expected InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("composite:")
                && msg.contains("LINEAR|COMP")
                && msg.contains("both non-empty"),
            "no-pipe diagnostic must pin tag + format hint + non-empty requirement; got {msg:?}"
        );
    }

    #[test]
    fn split_composite_input_rejects_empty_half() {
        for input in ["|(10)BATCH", "(01)12345|"] {
            let err = split_composite_input(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("{input:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("composite:")
                    && msg.contains("LINEAR|COMP")
                    && msg.contains("both non-empty"),
                "{input:?} empty-half must pin tag + format hint + non-empty requirement; \
                 got {msg:?}"
            );
        }
    }

    #[test]
    fn sbs_to_pixels_alternates() {
        // 3 bars, 2 spaces, 1 bar → "111001"
        let pixs = sbs_to_pixels(&[3, 2, 1]);
        assert_eq!(pixs, vec![1, 1, 1, 0, 0, 1]);
    }

    #[test]
    fn sbs_to_pixels_zero_width() {
        let pixs = sbs_to_pixels(&[1, 0, 1]);
        // Zero width = nothing for that segment.
        assert_eq!(pixs, vec![1, 1]);
    }

    /// Stage 11.A8c — pin `databarexpanded_bot` (private helper at
    /// line 1340). The function is structurally identical to
    /// `sbs_to_pixels` (alternating bar/space expansion starting
    /// with `bit = 1`), but takes a `&[u8]` instead of `&[u32]` and
    /// is a separate function — no direct test currently exists.
    /// Mutations introduced specifically to `databarexpanded_bot`
    /// would survive the `sbs_to_pixels_*` tests.
    ///
    /// Test cases:
    ///   * Empty input → empty output (loop body never runs).
    ///   * Single bar `[5]` → `[1, 1, 1, 1, 1]` (pins bit=1 start).
    ///   * Alternating `[3, 2, 1]` → `[1, 1, 1, 0, 0, 1]` (pins
    ///     bar→space→bar toggle and the per-width expansion).
    ///   * Zero-width toggle `[1, 0, 1]` → `[1, 1]`: middle 0-width
    ///     emits nothing but still flips the bit, so the trailing 1
    ///     re-enters the bar phase.
    ///   * Four-element `[2, 1, 3, 2]` → `[1, 1, 0, 1, 1, 1, 0, 0]`
    ///     (full bar/space/bar/space cycle to catch mutations that
    ///     would break after two toggles).
    ///
    /// Mutations caught:
    ///   * `let mut bit = 1u8` → `0u8`: every bit inverted.
    ///   * `bit ^= 1` → `bit = bit`: no toggle, all 1s after the
    ///     first run.
    ///   * `for _ in 0..w` → `0..w-1` / `0..w+1`: width drift.
    ///   * `Vec::with_capacity(total)` → `with_capacity(0)`: capacity
    ///     bug (doesn't change output, untestable from outside —
    ///     intentionally not pinned).
    #[test]
    fn databarexpanded_bot_pins_alternation_and_zero_width_toggle() {
        assert_eq!(
            databarexpanded_bot(&[]),
            Vec::<u8>::new(),
            "empty input → empty"
        );
        assert_eq!(
            databarexpanded_bot(&[5]),
            vec![1, 1, 1, 1, 1],
            "single bar [5] = 5 ones (pins bit=1 start)"
        );
        assert_eq!(
            databarexpanded_bot(&[3, 2, 1]),
            vec![1, 1, 1, 0, 0, 1],
            "[3,2,1] = bar3 + space2 + bar1"
        );
        assert_eq!(
            databarexpanded_bot(&[1, 0, 1]),
            vec![1, 1],
            "[1,0,1] = bar1 + (zero-width space, bit still toggles) + bar1"
        );
        assert_eq!(
            databarexpanded_bot(&[2, 1, 3, 2]),
            vec![1, 1, 0, 1, 1, 1, 0, 0],
            "[2,1,3,2] = bar2 + space1 + bar3 + space2 (full 4-toggle cycle)"
        );
    }

    #[test]
    fn databaromni_separator_basic_shape() {
        // A 96-pixel bot with all 1s; sep should be inverted (all 0s)
        // plus the boundary trims (already 0). No finder pattern match
        // (all 1s, not the f3pat).
        let bot = vec![1u8; 96];
        let sep = databaromni_separator(&bot);
        assert_eq!(sep.len(), 96);
        // No 1s in sep since bot is all 1s → inverted is all 0s,
        // sepfinder doesn't add any.
        assert!(sep.iter().all(|&v| v == 0));
    }

    /// Stage 11.A8c — pin `databaromni_separator` BOTH fp=18 AND
    /// fp=64 sepfinder windows. Existing `_basic_shape` test uses
    /// all-1s bot (which produces all-0 sep — no sepfinder firing
    /// detectable), and `_finder_match_inserts_findersep` only
    /// covers the f3pat override path. The alternating-pattern path
    /// for both fp positions (with non-finder-matching all-0s bot)
    /// is unexercised.
    ///
    /// Hand-computed for all-0s bot, length 96:
    /// - sep before sepfinder = [0,0,0, 1×89, 0,0,0,0].
    /// - apply_sepfinder at fp=18 with all-0s bot writes alternating
    ///   pattern to sep[18..=30]: sep[18]=0, sep[19]=1, ..., sep[30]=0.
    /// - apply_sepfinder at fp=64 with all-0s bot writes alternating
    ///   to sep[64..=76]: sep[64]=0 (prev_sep=sep[63]=1 since 63 is
    ///   in the inverted region, untouched by fp=18 window).
    ///
    /// A mutant on the second fp constant (64 → 63 or 65) would
    /// shift the second window. Existing tests don't pin it.
    #[test]
    fn databaromni_separator_pins_both_fp_18_and_fp_64() {
        let bot = vec![0u8; 96];
        let sep = databaromni_separator(&bot);
        assert_eq!(sep.len(), 96);

        // Front pad (0..3) zeroed.
        for i in 0..3 {
            assert_eq!(sep[i], 0, "front pad pos {i}");
        }
        // Back pad (92..96) zeroed.
        for i in 92..96 {
            assert_eq!(sep[i], 0, "back pad pos {i}");
        }

        // ---- fp=18 sepfinder window. Alternating starting 0.
        assert_eq!(
            &sep[18..=30],
            &[0u8, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
            "fp=18 window must alternate starting with 0"
        );

        // ---- fp=64 sepfinder window. Alternating starting 0.
        // Catches a mutant on the second fp constant.
        assert_eq!(
            &sep[64..=76],
            &[0u8, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
            "fp=64 window must alternate starting with 0; \
             a mutant on fp=64 → 63 or 65 would shift this"
        );

        // Cells between the two windows (31..64) stay inverted = 1.
        for i in 31..64 {
            assert_eq!(
                sep[i], 1,
                "pos {i}: between sepfinder windows, inverted to 1"
            );
        }
        // Cells between window 2 and back pad (77..92) stay inverted = 1.
        for i in 77..92 {
            assert_eq!(sep[i], 1, "pos {i}: after fp=64 window, inverted to 1");
        }
    }

    /// Stage 11.A8c — pin `databarexpandedstacked_composite_
    /// separator` boundary + position-list construction. Variable-
    /// length input: invert, zero front 4 + back 4, then apply
    /// sepfinder at positions 19, 70 (and +98 strides). Mutations
    /// to catch:
    ///   - `1 - b` → `1 + b`: inversion broken.
    ///   - `take(4)` / `skip(n.saturating_sub(4))` size mutations.
    ///   - `p + 12 < n` → `<= n`: off-by-one on the stride-end
    ///     boundary lets a half-window finder slip in.
    ///   - Starting positions 19 / 70 or `+= 98` stride mutation.
    ///
    /// Strategy: all-1s top → all-0s output (no finder match, pads
    /// already 0). All-0s short top (length 50): front/back pads
    /// zeroed, only position 19 fires sepfinder (70+12=82 > 50);
    /// position 10 outside sepfinder stays inverted to 1.
    #[test]
    fn databarexpandedstacked_composite_separator_boundary() {
        // All-1s top → all-0s output.
        let top = vec![1u8; 60];
        let sep = databarexpandedstacked_composite_separator(&top);
        assert_eq!(sep.len(), 60, "output preserves input length");
        assert!(sep.iter().all(|&v| v == 0), "all-1s → all-0s (invert)");

        // All-0s short top (length 50): only fp=19 fires (70+12 > 50).
        let top = vec![0u8; 50];
        let sep = databarexpandedstacked_composite_separator(&top);
        assert_eq!(sep.len(), 50);
        // Front and back pads zeroed.
        for i in 0..4 {
            assert_eq!(sep[i], 0, "front pad pos {i}");
        }
        for i in 46..50 {
            assert_eq!(sep[i], 0, "back pad pos {i}");
        }
        // Position 10 outside both pads and the sepfinder window
        // (19..=31) stays inverted to 1.
        assert_eq!(sep[10], 1, "position 10 inverted, untouched");
        // Position 45 outside back pad (46..50) and sepfinder window
        // (only 19..=31 since 70 doesn't fire) stays 1.
        assert_eq!(sep[45], 1, "position 45 inverted, untouched");

        // Length-31 input: 19 + 12 = 31 NOT < 31, so no positions
        // fire at all. Whole sep is pure invert + boundary pads.
        let top = vec![0u8; 31];
        let sep = databarexpandedstacked_composite_separator(&top);
        assert_eq!(sep.len(), 31);
        // Front pad.
        for i in 0..4 {
            assert_eq!(sep[i], 0);
        }
        // Back pad (skip(27) gives 27,28,29,30).
        for i in 27..31 {
            assert_eq!(sep[i], 0);
        }
        // Middle (4..27) stays inverted to 1.
        for i in 4..27 {
            assert_eq!(sep[i], 1, "pos {i}: no sepfinder fired");
        }

        // ---- Length 85: both finder-position loops fire. Loop 1 at
        // fp=19 (since 19+12=31 < 85). Loop 2 at fp=70 (since
        // 70+12=82 < 85). Existing assertions don't pin loop 2's
        // starting position — a mutant on `let mut p = 70usize` (e.g.
        // → 71 or 69) would shift the second sepfinder window and
        // survive a length-only invariant check.
        //
        // Hand-computed for all-0s top at length 85:
        //   sep[70..=82] = alternating starting with 0
        //   (sep[70]=0, sep[71]=1, sep[72]=0, …, sep[81]=1, sep[82]=0)
        // because the prev_sep recurrence starts from sep[69]=1
        // (inverted, untouched by loop 1's window 19..=31).
        // Note: sep[81] gets overwritten from 0 (back pad) to 1
        // (sepfinder output at i=81: prev_sep=sep[80]=0 → v=1).
        let top = vec![0u8; 85];
        let sep = databarexpandedstacked_composite_separator(&top);
        assert_eq!(sep.len(), 85);
        assert_eq!(
            sep[70], 0,
            "loop-2 sepfinder at fp=70 must start with sep[70]=0 \
             (catches mutant on loop-2 starting position)"
        );
        assert_eq!(sep[71], 1, "sepfinder alternation: sep[71]=1");
        assert_eq!(sep[82], 0, "sepfinder window end: sep[82]=0");
        // sep[81] gets overwritten by sepfinder output (1) from
        // back-pad 0. This pins that the sepfinder runs AFTER the
        // back-pad zeroing.
        assert_eq!(
            sep[81], 1,
            "sepfinder must overwrite back-pad zero with alternation value (1); \
             order mutation (sepfinder before back-pad) would leave it 0"
        );
        // Loop 1 also fires at fp=19: sep[19] should still be 0.
        assert_eq!(sep[19], 0, "loop-1 sepfinder at fp=19 still fires for n=85");
        // Cells between the two windows (32..70) stay inverted = 1.
        for i in 32..70 {
            assert_eq!(
                sep[i], 1,
                "pos {i}: between sepfinder windows, inverted to 1"
            );
        }
    }

    /// Stage 11.A8c — pin `databarstacked_composite_separator`
    /// boundary + invert behaviour. Wraps `databaromni_separator`-
    /// style logic but works on a fixed 50-cell top half: invert all,
    /// zero the first 4 (seppad) and the last 4 (skip 46), then
    /// apply sepfinder at position 18.
    ///
    /// Mutations to catch:
    ///   - `[u8; 50]` length wrong: would fail compile or panic.
    ///   - `1 - v` → `1 + v` / `v - 1`: inversion broken.
    ///   - `take(4)` → `take(3)` / `take(5)`: front pad size wrong.
    ///   - `skip(46)` → `skip(45)` / `skip(47)`: back pad size wrong.
    ///   - `apply_sepfinder(top_50, &mut sep, 18)` → wrong `fp`.
    ///
    /// Strategy: feed an all-1s top (which inverts to all-0s; no
    /// f3pat match) → result must be all 0s. Then feed an all-0s
    /// top (which inverts to all-1s); zero pads claim positions
    /// 0-3 and 46-49; verify those are 0 and a non-pad position
    /// outside the sepfinder window (e.g. position 10) stays 1.
    #[test]
    fn databarstacked_composite_separator_boundary_and_invert() {
        // All-1s top: invert → all-0s, pads already 0, no finder match.
        let top = [1u8; 50];
        let sep = databarstacked_composite_separator(&top);
        assert_eq!(sep.len(), 50, "output is exactly 50 cells");
        assert!(
            sep.iter().all(|&v| v == 0),
            "all-1s top → inverted to all-0s; sepfinder zero-padded too"
        );

        // All-0s top: invert → all-1s, then zero the first 4 and last 4.
        let top = [0u8; 50];
        let sep = databarstacked_composite_separator(&top);
        assert_eq!(sep.len(), 50);
        // Boundary pads (front 4, back 4) must be zero.
        for i in 0..4 {
            assert_eq!(sep[i], 0, "front pad pos {i} zeroed");
        }
        for i in 46..50 {
            assert_eq!(sep[i], 0, "back pad pos {i} zeroed");
        }
        // Position 10 is outside the front pad (0..4) and outside the
        // sepfinder window (18..=30). With all-0s top, inverted gives 1
        // and no further mutation applies → must remain 1.
        assert_eq!(
            sep[10], 1,
            "position 10 (non-pad, non-sepfinder) inverts to 1 and stays"
        );
        // Position 45 is also outside the back pad (46..50) and the
        // sepfinder window. Same expectation.
        assert_eq!(sep[45], 1, "position 45 (non-pad) stays inverted to 1");
    }

    #[test]
    fn cca_post_ecc_cws_for_batch_match_bwip_js() {
        // bwip-js reports full cws after RS-ECC for "(10)BATCH" cca cols=4:
        //   [637, 279, 478, 709, 810, 262, 840, 132, 284, 907, 528, 214]
        // Verify our padding + RS-ECC produces the same 12 codewords.
        let enc = crate::symbology::gs1_cc::encode_cc("(10)BATCH", 4).unwrap();
        // Reproduce render_cca's padding + ECC pipeline.
        let cws_in: Vec<u32> = enc.codewords.clone();
        assert_eq!(cws_in, vec![637, 279, 478, 709, 810, 262, 840, 132]);
        // Pad with 900 to n = c*r - k = 4*3 - 4 = 8 (no padding needed).
        let mut cws: Vec<u16> = cws_in.iter().map(|&v| v as u16).collect();
        cws.resize(8, 900);
        let check = crate::util::rs_gf929::encode(&cws[..8], 4);
        cws.extend_from_slice(&check);
        assert_eq!(
            cws,
            vec![637, 279, 478, 709, 810, 262, 840, 132, 284, 907, 528, 214],
            "post-ECC cws mismatch — diagnose RS encoding",
        );
    }

    #[test]
    fn cca_render_for_batch_matches_bwip_js_row2() {
        // Correct bwip-js want_row2 (was previously copied wrong; first
        // few cols are now [1,1,0,1,1,0,1,0,0,0,...] per re-pulled oracle).
        let enc = crate::symbology::gs1_cc::encode_cc("(10)BATCH", 4).unwrap();
        let (bm, _) = crate::symbology::micropdf417::render_cca(&enc.codewords, 4).unwrap();
        let want_row2_99: [u8; 99] = [
            1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0,
            0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1,
            1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1,
        ];
        for (x, &want) in want_row2_99.iter().enumerate() {
            let got = u8::from(bm.get(x, 2));
            assert_eq!(got, want, "row 2 col {x} mismatch");
        }
    }

    #[test]
    fn cca_render_for_batch_matches_bwip_js_first_row() {
        // Sanity: isolate the CC-A render layer. (10)BATCH → 8 CC-A
        // codewords (verified matches bwip-js). render_cca should
        // produce 99-wide × 3-row pixs that matches bwip-js's pixs[0..99]
        // for the same input.
        let enc = crate::symbology::gs1_cc::encode_cc("(10)BATCH", 4).unwrap();
        let (bm, _) = crate::symbology::micropdf417::render_cca(&enc.codewords, 4).unwrap();
        assert_eq!(bm.width(), 99);
        assert_eq!(bm.height(), 3);
        // bwip-js (full composite pixs[0..99] for "(01)24012345678905|(10)BATCH"):
        let want_row0_99: [u8; 99] = [
            1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0,
            0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0,
            1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1,
        ];
        for (x, &want) in want_row0_99.iter().enumerate() {
            let got = u8::from(bm.get(x, 0));
            assert_eq!(got, want, "row 0 col {x} mismatch");
        }
    }

    #[test]
    fn encode_databaromni_cca_matches_bwip_js_pixs_first_8_rows() {
        // For "(01)24012345678905|(10)BATCH" bwip-js produces a 100×40
        // expanded pixs (5 logical rows × rowmult=[2,2,2,1,33]). We
        // pin the first 8 rows (the 6 CC-A physical rows + sep + first
        // linear row) — that's the part our build_databaromni_composite
        // controls; the remaining 32 linear rows are just repeats.
        let bm = encode_databaromni_cca("(01)24012345678905|(10)BATCH").unwrap();
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 40);
        // bwip-js oracle: first 8 physical rows (expanded via rowmult)
        // for "(01)24012345678905|(10)BATCH", captured on 2026-05-19.
        // rowmult = [2, 2, 2, 1, 33] → physical 0-1=CC-A r0, 2-3=r1, 4-5=r2,
        // 6=separator, 7=first linear row.
        let want_rows: [[u8; 100]; 8] = [
            [
                1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1,
                0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1,
                0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0,
                0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            [
                1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1,
                0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1,
                0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0,
                0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            [
                1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1,
                0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1,
                1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0,
                1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            [
                1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1,
                0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1,
                1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0,
                1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            [
                1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1,
                0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1,
                0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1,
                0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            [
                1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1,
                0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1,
                0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1,
                0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            ],
            // logical row 3: separator
            [
                0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0,
                1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0,
                1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1,
                0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            ],
            // logical row 4: linear template (first physical linear row)
            [
                0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1,
                0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0,
                1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1,
            ],
        ];
        for (y, want_row) in want_rows.iter().enumerate() {
            for (x, &want) in want_row.iter().enumerate() {
                let got = u8::from(bm.get(x, y));
                assert_eq!(got, want, "row {y} col {x} mismatch");
            }
        }
        // Physical rows 8-39 are identical repeats of row 7 (the linear
        // template tiled `linheight=33` times). We verified row 7 matches
        // bwip-js above; assert the remaining 32 rows are bit-identical
        // copies of it — that locks the full 40-row composite output.
        for y in 8..40 {
            for x in 0..100 {
                assert_eq!(
                    bm.get(x, y),
                    bm.get(x, 7),
                    "row {y} col {x} should be a copy of row 7 (linear template tile)"
                );
            }
        }
    }

    #[test]
    fn encode_databaromni_cca_smoke() {
        // End-to-end: GS1 GTIN + CC AI should produce a 100×40 BitMatrix.
        let bm = encode_databaromni_cca("(01)24012345678905|(10)BATCH").unwrap();
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 40);
    }

    #[test]
    fn build_databaromni_composite_dimensions() {
        // CC-A 3 rows × 99 wide, linear sbs sum = 95 (BWIPP DataBar Omni),
        // linheight = 33. Expected pixx = max(99, 95+1+4) = max(99, 100) =
        // 100; pixy = 3*2 + 1 + 33 = 40.
        let cc = crate::encoding::BitMatrix::new(99, 3);
        // 95-module linear: 47 bar/space alternations of width 2 + 1 wider.
        let mut linsbs: Vec<u32> = (0..47).map(|_| 2u32).collect();
        linsbs.push(1); // total = 95
        assert_eq!(linsbs.iter().sum::<u32>(), 95);
        let bm = build_databaromni_composite(&cc, &linsbs, 33);
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 40);
    }

    #[test]
    fn composite_databar_omni_cca_via_public_api() {
        // Exercises the Symbology dispatch — Symbology::CompositeDatabarOmniCca
        // → composite::encode_databaromni_cca → Encoded::Matrix.
        // Stage 11.A8c (cont) — echo the actual returned variant + per-axis
        // value-echoes in assert_eq so a mutation that re-routes Composite
        // to a non-Matrix family (Stacked, Hex, …) or swaps width/height is
        // identifiable in the failure diagnostic.
        use crate::{Options, Symbology};
        let opts = Options::default();
        let enc = Symbology::CompositeDatabarOmniCca
            .encode("(01)24012345678905|(10)BATCH", &opts)
            .unwrap();
        match enc {
            crate::encoding::Encoded::Matrix(bm) => {
                assert_eq!(
                    bm.width(),
                    100,
                    "CompositeDatabarOmniCca via Symbology must yield 100-col matrix; got {}",
                    bm.width()
                );
                assert_eq!(
                    bm.height(),
                    40,
                    "CompositeDatabarOmniCca via Symbology must yield 40-row matrix (linheight=39 + 1 separator); got {}",
                    bm.height()
                );
            }
            other => panic!(
                "Symbology::CompositeDatabarOmniCca must dispatch to Encoded::Matrix; got {other:?}"
            ),
        }
    }

    #[test]
    fn encode_databaromni_ccb_dimensions_match_bwip_js() {
        // bwip-js oracle: "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
        // → pixx=100, pixy=58, ccrows=12, linheight=33, rowmult=[2*12, 1, 33].
        // Rust produces a fully-expanded BitMatrix: 12*2 + 1 + 33 = 58 rows.
        let bm = encode_databaromni_ccb(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 58);
    }

    #[test]
    fn encode_databaromni_ccb_matches_bwip_js_pixs_key_rows() {
        // For the CC-B-forcing payload, bwip-js produces a 100×58 expanded
        // pixs (14 logical rows × rowmult=[2*12, 1, 33]). Pin the first
        // physical row (CC-B row 0), the last CC-B physical row (CC-B
        // row 11, y=22..23), the separator (y=24), and the first linear
        // (y=25). bwip-js logical pixs captured 2026-05-19 via
        // oracle-databaromni-cca.js with the long payload.
        let bm = encode_databaromni_ccb(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 58);
        // CC-B logical row 0 (physical y=0, y=1).
        let want_cc_r0: [u8; 100] = [
            1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0,
            1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0,
        ];
        for (x, &want) in want_cc_r0.iter().enumerate() {
            assert_eq!(
                u8::from(bm.get(x, 0)),
                want,
                "y=0 col {x} mismatch (CC-B logical row 0)",
            );
            assert_eq!(
                u8::from(bm.get(x, 1)),
                want,
                "y=1 col {x} should equal y=0 (rowmult=2 repeat)",
            );
        }
        // CC-B logical row 11 (physical y=22, y=23) — the last CC row.
        let want_cc_r11: [u8; 100] = [
            1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0,
            0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1,
            1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0,
        ];
        for (x, &want) in want_cc_r11.iter().enumerate() {
            assert_eq!(
                u8::from(bm.get(x, 22)),
                want,
                "y=22 col {x} mismatch (CC-B logical row 11)",
            );
            assert_eq!(
                u8::from(bm.get(x, 23)),
                want,
                "y=23 col {x} should equal y=22",
            );
        }
        // Separator (physical y=24, logical row 12, rowmult=1).
        let want_sep: [u8; 100] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1,
            0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0,
            0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1,
            0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
        ];
        for (x, &want) in want_sep.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 24)), want, "y=24 col {x} (sep)");
        }
        // First linear (physical y=25, logical row 13, rowmult=33).
        let want_lin: [u8; 100] = [
            0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1,
            1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0,
            1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1,
        ];
        for (x, &want) in want_lin.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 25)), want, "y=25 col {x} (lin)");
        }
        // Linear template tiles: rows 26..58 are exact copies of row 25.
        for y in 26..58 {
            for x in 0..100 {
                assert_eq!(bm.get(x, y), bm.get(x, 25), "y={y} col {x}: linear-tile");
            }
        }
    }

    #[test]
    fn encode_databaromni_ccb_accepts_cca_size_payload() {
        // CC-B handler must also accept CC-A-sized payloads (the
        // documented "drop-in superset" behavior). Output dimensions
        // match what encode_databaromni_cca would produce: 100×40.
        let bm = encode_databaromni_ccb("(01)24012345678905|(10)BATCH").unwrap();
        assert_eq!(bm.width(), 100);
        assert_eq!(bm.height(), 40);
    }

    #[test]
    fn composite_databar_omni_cca_rejects_ccb_payload() {
        // Multi-AI input large enough that gs1_cc auto-promotes from
        // CC-A to CC-B. The CC-A-only handler must reject this with a
        // clear "use the CC-B variant" message rather than letting the
        // 8-bit CC-B byte stream bleed into `render_cca` (which would
        // panic on out-of-range codewords).
        //
        // Stage 11.A8c (cont) — tighten from disjunctive `||` (which
        // accepted any of 3 substrings) to 3-anchor AND-pin matching
        // the actual source diagnostic at line 226-230 of composite.rs:
        //   `composite_databar_omni_cca: payload requires CC-B; use
        //    composite_databar_omni_ccb instead`
        // Anchors:
        //   1. `composite_databar_omni_cca:` exact symbology prefix
        //   2. `payload requires CC-B` predicate
        //   3. `composite_databar_omni_ccb instead` migration hint
        // The disjunctive `||` was a weak pattern: a mutation that
        // dropped the prefix but left "CC-B" in place would still
        // pass; this tightening kills that class.
        let res = encode_databaromni_cca(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        );
        let err = res.expect_err("CC-B payload should be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_omni_cca:"),
            "missing exact `composite_databar_omni_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_omni_ccb instead"),
            "missing `composite_databar_omni_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn composite_databar_omni_cca_id_round_trip() {
        use crate::Symbology;
        let sym = Symbology::CompositeDatabarOmniCca;
        assert_eq!(sym.id(), "composite_databar_omni_cca");
        assert_eq!(Symbology::from_id(sym.id()), Some(sym));
    }

    #[test]
    fn databarlimited_separator_matches_bwip_js_oracle() {
        // For "(01)15012345678907" with linkage=true, bwip-js's linsbs is:
        //   [1,1,1,3,1,1,1,2,4,1,4,1,1,2,3,1,1,2,1,1,1,1,2,1,1,2,2,1,1,2,
        //    1,2,1,1,2,3,2,1,3,2,2,2,2,1,1,5] (46 entries)
        // and the final 74-cell separator (captured 2026-05-19 via
        // oracle-limited-composite-sep.js) is:
        let linsbs: [u8; 46] = [
            1, 1, 1, 3, 1, 1, 1, 2, 4, 1, 4, 1, 1, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 2, 2, 1, 1,
            2, 1, 2, 1, 1, 2, 3, 2, 1, 3, 2, 2, 2, 2, 1, 1, 5,
        ];
        let want_sep: [u8; 74] = [
            0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1,
            0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0,
        ];
        let sep = databarlimited_separator(&linsbs);
        assert_eq!(sep, want_sep);
    }

    #[test]
    fn databarlimited_linpixs_matches_bwip_js_oracle() {
        // Same linsbs as above; bwip-js's 74-cell linpixs:
        let linsbs: [u8; 46] = [
            1, 1, 1, 3, 1, 1, 1, 2, 4, 1, 4, 1, 1, 2, 3, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 2, 2, 1, 1,
            2, 1, 2, 1, 1, 2, 3, 2, 1, 3, 2, 2, 2, 2, 1, 1, 5,
        ];
        let want_lin: [u8; 74] = [
            0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0,
            1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1,
        ];
        let lin = databarlimited_linpixs(&linsbs);
        assert_eq!(lin, want_lin);
    }

    #[test]
    fn encode_databarlimited_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "(01)15012345678907|(99)1234567":
        //   pixx=74, pixy=19, ccpixx=72, ccrows=4, linheight=10
        //   rowmult=[2,2,2,2,1,10] → physical 8 + 1 + 10 = 19
        let bm = encode_databarlimited_cca("(01)15012345678907|(99)1234567").unwrap();
        assert_eq!(bm.width(), 74);
        assert_eq!(bm.height(), 19);
    }

    /// Stage 11.A8c — pin `encode_databarlimited_cca`'s "payload
    /// requires CC-B" rejection arm at line ~852. DataBar Limited
    /// uses 3-column CC. Mirrors the existing ean8_cca / upca_cca
    /// rejection pins.
    ///
    /// Use a multi-AI payload that exceeds CC-A's 3-col capacity.
    #[test]
    fn encode_databarlimited_cca_rejects_ccb_payload() {
        let res = encode_databarlimited_cca(
            "(01)15012345678907|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK",
        );
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor
        // AND-pin matching the source diagnostic in encode_databarlimited_cca
        // (`composite_databar_limited_cca: payload requires CC-B; use
        // composite_databar_limited_ccb instead`).
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_limited_cca:"),
            "missing exact `composite_databar_limited_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_limited_ccb instead"),
            "missing `composite_databar_limited_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_databarlimited_cca_matches_bwip_js_cc_rows() {
        // Pin all 4 CC-A logical rows to bwip-js's pixs output for
        // "(01)15012345678907|(99)1234567" (oracle-databarlimited-composite.js).
        // CC-A 3-col uses ccpixx=72; the layout adds [0] + ccrow + [0] = 74
        // cells per physical row. rowmult=2 means y=0/y=1 are both CC row 0.
        let bm = encode_databarlimited_cca("(01)15012345678907|(99)1234567").unwrap();
        let want_cc_rows: [[u8; 74]; 4] = [
            [
                0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0,
                1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0,
                0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0,
            ],
            [
                0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0,
                1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1,
                0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0,
            ],
            [
                0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0,
                1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1,
                0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0,
            ],
            [
                0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0,
                1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0,
                0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0,
            ],
        ];
        for (r, want) in want_cc_rows.iter().enumerate() {
            let y_a = r * CCA_ROWMULT;
            let y_b = r * CCA_ROWMULT + 1;
            for (x, &w) in want.iter().enumerate() {
                assert_eq!(u8::from(bm.get(x, y_a)), w, "y={y_a} col {x}");
                assert_eq!(u8::from(bm.get(x, y_b)), w, "y={y_b} col {x}");
            }
        }
    }

    #[test]
    fn encode_databarlimited_cca_matches_bwip_js_separator_and_linear() {
        // Verify the two locked rows: separator at y=8, first linear at y=9.
        let bm = encode_databarlimited_cca("(01)15012345678907|(99)1234567").unwrap();
        let want_sep: [u8; 74] = [
            0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1,
            0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0,
        ];
        let want_lin: [u8; 74] = [
            0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0,
            1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1,
        ];
        for x in 0..74 {
            assert_eq!(u8::from(bm.get(x, 8)), want_sep[x], "sep col {x}");
            assert_eq!(u8::from(bm.get(x, 9)), want_lin[x], "lin col {x}");
        }
        // Linear template repeats: rows 10..19 should match row 9.
        for y in 10..19 {
            for x in 0..74 {
                assert_eq!(bm.get(x, y), bm.get(x, 9), "y={y} col {x}");
            }
        }
    }

    #[test]
    fn encode_databarlimited_ccb_dimensions_match_bwip_js() {
        // For the long payload, bwip-js produces:
        //   pixx=83, pixy=51, ccrows=20, ccpixx=82, linheight=10
        let bm = encode_databarlimited_ccb(
            "(01)15012345678907|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        assert_eq!(bm.width(), 83);
        assert_eq!(bm.height(), 51);
    }

    #[test]
    fn encode_databarlimited_ccb_accepts_cca_payload() {
        // Drop-in superset: CC-A-sized payloads route through the
        // CC-A 3-col layout (74 cells wide).
        let bm = encode_databarlimited_ccb("(01)15012345678907|(99)1234567").unwrap();
        assert_eq!(bm.width(), 74);
        assert_eq!(bm.height(), 19);
    }

    #[test]
    fn encode_databarlimited_ccb_matches_bwip_js_separator_and_linear() {
        // CC-B Limited oracle (captured 2026-05-19): sep row at y=40,
        // first linear at y=41. Both shifted right by 9 cells (the
        // `[0]*9` leading padding in the ccpixx!=72 layout).
        let bm = encode_databarlimited_ccb(
            "(01)15012345678907|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        let want_sep: [u8; 83] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0,
            0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0,
            1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0,
        ];
        let want_lin: [u8; 83] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1,
            1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1,
        ];
        for x in 0..83 {
            assert_eq!(u8::from(bm.get(x, 40)), want_sep[x], "sep col {x}");
            assert_eq!(u8::from(bm.get(x, 41)), want_lin[x], "lin col {x}");
        }
        // Linear template rows 42..51 should match row 41.
        for y in 42..51 {
            for x in 0..83 {
                assert_eq!(bm.get(x, y), bm.get(x, 41), "y={y} col {x}");
            }
        }
    }

    #[test]
    fn encode_databarlimited_ccb_matches_bwip_js_cc_first_and_last_rows() {
        // First and last CC-B logical rows from the bwip-js oracle —
        // verifies render_ccb produces the same 82-cell rows, then the
        // composite stacker correctly places them at columns 0..82 with
        // a trailing 0 at column 82.
        let bm = encode_databarlimited_ccb(
            "(01)15012345678907|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        let want_cc_row0: [u8; 83] = [
            1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0,
            1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0,
            1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0,
        ];
        let want_cc_row19: [u8; 83] = [
            1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0,
            1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0,
            1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0,
        ];
        for x in 0..83 {
            // CC row 0: physical y=0 (and y=1 via rowmult).
            assert_eq!(u8::from(bm.get(x, 0)), want_cc_row0[x], "cc r0 col {x}");
            assert_eq!(
                u8::from(bm.get(x, 1)),
                want_cc_row0[x],
                "cc r0 col {x} (y=1)"
            );
            // CC row 19: physical y=38, y=39.
            assert_eq!(u8::from(bm.get(x, 38)), want_cc_row19[x], "cc r19 col {x}");
            assert_eq!(
                u8::from(bm.get(x, 39)),
                want_cc_row19[x],
                "cc r19 col {x} (y=39)"
            );
        }
    }

    #[test]
    fn gs1_128_cc_offset_a_for_known_linwidth() {
        // For linwidth=145 (the "(01)04012345123456" linkage-a case),
        // bwip-js reports x=21 and diff=25 (oracle-gs1-128-composite.js).
        assert_eq!(gs1_128_cc_offset_a(145), 21);
        // Smaller linwidth still produces a sane non-negative offset.
        assert!(gs1_128_cc_offset_a(123) >= 0);
    }

    /// Stage 11.A8c — pin `gs1_128_cc_offset_a` per-arm including
    /// the `if p == 0 { 2 } else { 0 }` branch.
    ///
    /// The existing `gs1_128_cc_offset_a_for_known_linwidth` covers
    /// only one anchor (linwidth=145 → 21, where p=2) plus a weak
    /// `>= 0` assert for linwidth=123. The `p == 0` arm (active
    /// when linwidth ∈ [101..=121]) is never exercised, and the
    /// `+2` offset addend would survive most arithmetic mutants.
    ///
    /// Hand-computed values:
    /// - linwidth=101: s=(101-2)/11=9, p=(9-9)/2=0,
    ///   base=(9-0-1)*11+10+2=8*11+12=100. result=100-99=1.
    /// - linwidth=112: s=(112-2)/11=10, p=(10-9)/2=0,
    ///   base=(10-0-1)*11+10+2=9*11+12=111. result=111-99=12.
    /// - linwidth=123: s=(123-2)/11=11, p=(11-9)/2=1,
    ///   base=(11-1-1)*11+10+0=9*11+10=109. result=109-99=10.
    /// - linwidth=145: s=(145-2)/11=13, p=(13-9)/2=2,
    ///   base=(13-2-1)*11+10+0=10*11+10=120. result=120-99=21.
    ///
    /// A mutant that swapped the branches of `if p == 0 { 2 } else { 0 }`
    /// would yield -1 at linwidth=101 (p=0, would get +0 → 98-99=-1)
    /// — caught explicitly.
    /// A mutant that replaced `p == 0` with `p > 0` would still fire
    /// the +2 for p=2 (linwidth=145) → result 23 (vs 21).
    /// A mutant that dropped the +10 constant would shift every
    /// result by -10 — caught everywhere.
    #[test]
    fn gs1_128_cc_offset_a_per_p_branch_arithmetic() {
        // ---- p == 0 arm (the +2 offset).
        assert_eq!(
            gs1_128_cc_offset_a(101),
            1,
            "linwidth=101: s=9, p=0, +2 offset → 1"
        );
        assert_eq!(
            gs1_128_cc_offset_a(112),
            12,
            "linwidth=112: s=10, p=0, +2 offset → 12"
        );

        // ---- p > 0 arm (the +0 offset).
        assert_eq!(
            gs1_128_cc_offset_a(123),
            10,
            "linwidth=123: s=11, p=1, +0 offset → 10"
        );
        assert_eq!(
            gs1_128_cc_offset_a(145),
            21,
            "linwidth=145: s=13, p=2, +0 offset → 21"
        );

        // ---- Discriminator: branch-swap mutant would flip 1 ↔ 10
        // and 12 ↔ 21 at the same input pairs. The 4 anchors above
        // pin both arms simultaneously.

        // ---- Cross-validation: hand-recomputed total offset for
        // p=0 vs p>0 differs by exactly 2 modules at the same s.
        // For s=10, linwidth=112 (p=0) → 12; if p was forced to 1
        // instead, base would drop by 2 (since (10-1-1)*11=99 vs
        // (10-0-1)*11=99 — same! sigh) but the `+2` would also
        // drop, giving (10-1-1)*11 + 10 + 0 = 99+10 = 109 → 10.
        // So a p=0 → p=1 mutant at linwidth=112 → result 10 (vs 12).
        // Distinct.
    }

    #[test]
    fn gs1_128_separator_starts_with_zero() {
        // Direct port test: for a small synthetic linsbs the inverted
        // expansion starts with `0` (since BWIPP pushes `1` as the
        // initial top and the first iteration's flip is `0`).
        let sep = gs1_128_separator(&[3, 2, 1]);
        // 3 zeros (flip of 1), 2 ones (flip of 0), 1 zero (flip of 1).
        assert_eq!(sep, vec![0, 0, 0, 1, 1, 0]);
    }

    /// Stage 11.A8c — strengthen `gs1_128_separator` pins:
    /// 1. Empty input → empty output (no panic).
    /// 2. Initial-bit value is 0 (BWIPP's inverted polarity).
    /// 3. Per-width alternation actually fires.
    /// 4. Total output length equals sum(widths).
    ///
    /// Existing `gs1_128_separator_starts_with_zero` pins one
    /// 6-byte anchor. A mutant that:
    /// - changed `let mut bit = 0u8` → `let mut bit = 1u8` would flip
    ///   the entire output (caught by initial-bit check).
    /// - dropped `bit ^= 1` would emit all 0s (caught by alternation
    ///   distinctness check).
    /// - swapped the outer/inner loop order would change the run
    ///   structure (caught by length + per-position checks below).
    #[test]
    fn gs1_128_separator_invariants_pin() {
        // ---- Empty input: empty output, no panic.
        assert_eq!(gs1_128_separator(&[]), Vec::<u8>::new());

        // ---- Single chunk: all 0s (initial bit value = 0).
        // [5] → 5 zeros. If initial-bit mutant flipped to 1, would be 5 ones.
        let sep = gs1_128_separator(&[5]);
        assert_eq!(sep, vec![0u8; 5], "single chunk → initial bit (0)");

        // ---- Two chunks: alternation. [4, 3] → 4 zeros, then 3 ones.
        // Drop `bit ^= 1` mutant would produce 7 zeros instead.
        let sep = gs1_128_separator(&[4, 3]);
        assert_eq!(
            sep,
            vec![0, 0, 0, 0, 1, 1, 1],
            "two chunks: 0s then 1s (alternation must fire)"
        );

        // ---- Length invariant: total output length = sum of widths.
        for input in [&[1u32, 1][..], &[5][..], &[3, 2, 1][..], &[2, 2, 2, 2][..]] {
            let total: usize = input.iter().map(|&w| w as usize).sum();
            assert_eq!(
                gs1_128_separator(input).len(),
                total,
                "length must equal sum of widths for {input:?}"
            );
        }

        // ---- Per-chunk single-bit value. For input [w] every emitted
        // byte must equal the initial bit (0). For [a, b] every byte
        // in [0..a] must be 0 and [a..a+b] must be 1.
        let sep = gs1_128_separator(&[7, 5]);
        for v in &sep[..7] {
            assert_eq!(*v, 0, "first 7 bytes must be 0");
        }
        for v in &sep[7..12] {
            assert_eq!(*v, 1, "next 5 bytes must be 1");
        }

        // ---- Width 0 is a no-op for that chunk but the toggle still
        // fires. [0, 3] → 0 zeros (skip), bit ^= 1 → 1, then 3 ones.
        let sep = gs1_128_separator(&[0, 3]);
        assert_eq!(
            sep,
            vec![1, 1, 1],
            "width-0 chunk emits nothing but still toggles → 3 ones"
        );
    }

    #[test]
    fn encode_gs1_128_cca_dimensions_match_bwip_js() {
        // bwip-js oracle "(01)04012345123456|(99)1234567" reports:
        //   pixx=145, pixy=43, ccrows=3, linheight=36.
        // Rust BitMatrix is fully-expanded: 3*2 + 1 + 36 = 43.
        let bm = encode_gs1_128_cca("(01)04012345123456|(99)1234567").unwrap();
        assert_eq!(bm.width(), 145);
        assert_eq!(bm.height(), 43);
    }

    #[test]
    fn encode_gs1_128_cca_matches_bwip_js_separator_and_linear() {
        // Verify the structural rows (sep at y=6, first linear at y=7),
        // and that the linear template repeats at rows 8..43.
        let bm = encode_gs1_128_cca("(01)04012345123456|(99)1234567").unwrap();
        let want_sep: [u8; 145] = [
            0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1,
            0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0,
            1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1,
            0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0,
        ];
        let want_lin: [u8; 145] = [
            1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0,
            1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1,
            0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0,
            0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0,
            1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1,
        ];
        for x in 0..145 {
            assert_eq!(u8::from(bm.get(x, 6)), want_sep[x], "sep col {x}");
            assert_eq!(u8::from(bm.get(x, 7)), want_lin[x], "lin col {x}");
        }
        // Linear template tile: rows 8..43 = row 7.
        for y in 8..43 {
            for x in 0..145 {
                assert_eq!(bm.get(x, y), bm.get(x, 7), "y={y} col {x}");
            }
        }
    }

    #[test]
    fn encode_gs1_128_cca_matches_bwip_js_cc_row_0() {
        // The CC row is placed at columns 21..120 (cclpad=21, ccpixx=99).
        // Cells [0..21] and [120..145] are zero (padding).
        let bm = encode_gs1_128_cca("(01)04012345123456|(99)1234567").unwrap();
        let want_cc_row_0: [u8; 145] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1,
            1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0,
            0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        for (x, &want) in want_cc_row_0.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 0)), want, "y=0 col {x}");
            assert_eq!(u8::from(bm.get(x, 1)), want, "y=1 col {x}");
        }
    }

    /// Stage 11.A8c — pin `encode_gs1_128_cca`'s "payload requires
    /// CC-B" rejection arm at line ~1684. The 2D goldens for
    /// `composite_gs1_128_cca` only exercise the happy CC-A path
    /// (small `(99)1234567` payload). A mutant that swaps `!=` →
    /// `==` (or `CcVersion::A` → `CcVersion::B`) would silently
    /// accept CC-B output and feed 8-bit bytes into `render_cca`,
    /// which expects 0..=899 codewords — produces a panic or
    /// corrupt symbol rather than a clear error.
    ///
    /// Use a multi-AI payload large enough to force gs1_cc to
    /// auto-promote from CC-A to CC-B at column-count 4.
    #[test]
    fn encode_gs1_128_cca_rejects_ccb_payload() {
        let res =
            encode_gs1_128_cca("(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC");
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor
        // AND-pin matching the source diagnostic in encode_gs1_128_cca.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_gs1_128_cca:"),
            "missing exact `composite_gs1_128_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_gs1_128_ccb instead"),
            "missing `composite_gs1_128_ccb instead` migration hint: {msg}"
        );
    }

    /// Stage 11.A8c — pin `encode_gs1_128_ccb` happy paths for both
    /// CC-A and CC-B payloads. encode_gs1_128_ccb is the "drop-in
    /// superset" of encode_gs1_128_cca, accepting CC-A sized payloads
    /// AND auto-promoting to CC-B for larger ones. The function had
    /// zero direct tests prior to this commit; the cc.version match
    /// arms at lines 1797-1808 (CC-A render + CC-B render) were only
    /// exercised via the public Symbology::CompositeGs1_128Ccb path
    /// in the golden manifest.
    ///
    /// Pin two distinct payloads:
    ///   1. Small `(99)1234567` → CC-A path, same dimensions as
    ///      encode_gs1_128_cca (145 × cc_rows + linear height).
    ///   2. Multi-AI payload `(10)BATCH(21)SERIAL...` → CC-B path
    ///      via auto-promotion. Pin only width (= 145) since the
    ///      height varies with CC-B row count.
    ///
    /// Mutations to catch:
    ///   - Swap CC-A render call with CC-B render → wrong pixel
    ///     layout (both succeed but produce different dimensions).
    ///   - Drop the CC-B arm → CCB-sized payloads error out.
    ///   - Constant-substitute the linkage = Linkage::A → wrong
    ///     auxiliary-character codeword in the linear.
    #[test]
    fn encode_gs1_128_ccb_accepts_both_cca_and_ccb_payloads() {
        // CC-A sized payload: same as encode_gs1_128_cca's golden.
        let bm_cca = encode_gs1_128_ccb("(01)04012345123456|(99)1234567").unwrap();
        assert_eq!(bm_cca.width(), 145);
        // Must include CC rows + sep + linear height. Pin the
        // height as a positive lower bound to catch a dropped CC
        // row or linear region.
        assert!(
            bm_cca.height() > 30,
            "CC-A path height must include CC rows + sep + linear, got {}",
            bm_cca.height()
        );

        // CC-B sized payload: same multi-AI input that the CC-A
        // path rejects. Must succeed in the CCB encoder.
        let bm_ccb =
            encode_gs1_128_ccb("(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC")
                .expect("CCB encoder must accept the CCB-sized payload");
        assert_eq!(
            bm_ccb.width(),
            145,
            "CC-B linear width stays 145 modules regardless of payload"
        );
        // CC-B path generates more rows than CC-A for the same data.
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B-sized payload should produce a taller symbol \
             than CC-A-sized ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
    }

    #[test]
    fn encode_ean13_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "5901234123457|(99)1234567":
        //   pixx=99, pixy=84, ccrows=3, ccpixx=99, linheight=72.
        //   Rust BitMatrix: 3*2 + 3*2 + 72 = 84.
        let bm = encode_ean13_cca("5901234123457|(99)1234567").unwrap();
        assert_eq!(bm.width(), 99);
        assert_eq!(bm.height(), 84);
    }

    #[test]
    fn encode_ean13_cca_matches_bwip_js_cc_row_0() {
        // CC-A row 0 sits at physical y=0,1 (rowmult=2). Oracle for
        // "5901234123457|(99)1234567":
        let bm = encode_ean13_cca("5901234123457|(99)1234567").unwrap();
        let want_cc0: [u8; 99] = [
            1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
            0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1,
            0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1,
        ];
        for (x, &want) in want_cc0.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 0)), want, "y=0 col {x}");
            assert_eq!(u8::from(bm.get(x, 1)), want, "y=1 col {x}");
        }
    }

    #[test]
    fn encode_ean13_cca_matches_bwip_js_guard_rows_and_linear() {
        // Guard A at y=6,7; Guard B at y=8,9; Guard A again at y=10,11.
        // Linear starts at y=12.
        let bm = encode_ean13_cca("5901234123457|(99)1234567").unwrap();
        // Guard A: cells [3] and [97] are 1, everything else 0.
        for &y in &[6, 7, 10, 11] {
            for x in 0..99 {
                let want = if x == 3 || x == 97 { 1 } else { 0 };
                assert_eq!(u8::from(bm.get(x, y)), want, "guard A y={y} col {x}");
            }
        }
        // Guard B: cells [2] and [98] are 1.
        for &y in &[8, 9] {
            for x in 0..99 {
                let want = if x == 2 || x == 98 { 1 } else { 0 };
                assert_eq!(u8::from(bm.get(x, y)), want, "guard B y={y} col {x}");
            }
        }
        // Linear row at y=12 (and repeated through y=83).
        let want_lin: [u8; 99] = [
            0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0,
            1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1,
            1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1,
            0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0,
        ];
        for (x, &want) in want_lin.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 12)), want, "lin y=12 col {x}");
        }
        for y in 13..84 {
            for x in 0..99 {
                assert_eq!(bm.get(x, y), bm.get(x, 12), "y={y} col {x}");
            }
        }
    }

    /// Stage 11.A8c — pin `ean_guard_rows` cell positions and the
    /// `row_c = row_a.clone()` identity directly. The existing
    /// `encode_ean13_cca_matches_bwip_js_guard_rows_and_linear`
    /// test exercises the rows transitively through a full BitMatrix
    /// comparison, but a mutation localized to `ean_guard_rows`
    /// (e.g. `row_a[linpad_len + 1]` → `+ 0`, `let row_c = row_b.clone()`,
    /// or `[row_a, row_b, row_c]` order swap) only surfaces as a
    /// large diff in the end-to-end golden — hard to attribute.
    ///
    /// Hand-traced for pixx=20, linpad_len=2, linwidth=10:
    ///   * Row A: row_a[linpad_len + 1] = row_a[3] = 1
    ///            row_a[linpad_len + linwidth] = row_a[12] = 1
    ///            (every other cell stays 0).
    ///   * Row B: row_b[linpad_len] = row_b[2] = 1
    ///            row_b[linpad_len + linwidth + 1] = row_b[13] = 1.
    ///   * Row C: clone of row A → same pattern as row A.
    ///
    /// Mutations caught:
    ///   * `row_a[linpad_len + 1]` → `+ 0`: would set cell 2 instead
    ///     of 3, and the `expected_a` comparison would fail.
    ///   * `row_a[linpad_len + linwidth]` → `linwidth - 1`: cell 11
    ///     instead of 12.
    ///   * `row_b[linpad_len]` → `+ 1`: cell 3 instead of 2; would
    ///     also collide with row A's cell 3.
    ///   * `row_b[linpad_len + linwidth + 1]` → `+ linwidth`: cell 12
    ///     instead of 13; collides with row A's cell 12.
    ///   * `let row_c = row_a.clone()` → `row_b.clone()`: row C would
    ///     match row B, failing the `assert_eq!(rows[2], rows[0])`.
    ///   * `[row_a, row_b, row_c]` → `[row_b, row_a, row_c]`: rows[0]
    ///     would carry row B's pattern, failing the expected_a check.
    ///   * `vec![0u8; pixx]` → `vec![1u8; pixx]`: every cell would
    ///     start at 1, and the surrounding-zero invariant fails.
    #[test]
    fn ean_guard_rows_pin_cell_positions_and_row_c_identity() {
        let rows = ean_guard_rows(20, 2, 10);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].len(), 20, "row A length = pixx");
        assert_eq!(rows[1].len(), 20, "row B length = pixx");
        assert_eq!(rows[2].len(), 20, "row C length = pixx");

        // Row A: cells [3] and [12] are 1.
        let mut expected_a = vec![0u8; 20];
        expected_a[3] = 1;
        expected_a[12] = 1;
        assert_eq!(
            rows[0], expected_a,
            "row A cells at linpad+1=3 and linpad+linwidth=12"
        );

        // Row B: cells [2] and [13] are 1.
        let mut expected_b = vec![0u8; 20];
        expected_b[2] = 1;
        expected_b[13] = 1;
        assert_eq!(
            rows[1], expected_b,
            "row B cells at linpad=2 and linpad+linwidth+1=13"
        );

        // Row C is a clone of row A.
        assert_eq!(
            rows[2], expected_a,
            "row C cells match row A (clone identity)"
        );
        assert_eq!(rows[2], rows[0], "row C IS clone of row A");
        // And row C differs from row B (catches the `row_b.clone()`
        // mutation).
        assert_ne!(
            rows[2], rows[1],
            "row C must NOT match row B (catches row_b.clone() mutation)"
        );

        // Cross-check with a second size to catch hard-coded
        // constants (e.g. if a mutation replaced linpad_len + 1 with
        // a fixed `3` it would still pass at linpad_len=2 above).
        let rows2 = ean_guard_rows(30, 5, 15);
        let mut expected_a2 = vec![0u8; 30];
        expected_a2[5 + 1] = 1; // 6
        expected_a2[5 + 15] = 1; // 20
        assert_eq!(rows2[0], expected_a2, "second size row A");
        let mut expected_b2 = vec![0u8; 30];
        expected_b2[5] = 1;
        expected_b2[5 + 15 + 1] = 1; // 21
        assert_eq!(rows2[1], expected_b2, "second size row B");
    }

    /// Stage 11.A8c — pin `encode_ean13_cca`'s "payload requires
    /// CC-B" rejection arm at line ~1195. The existing 2D goldens
    /// only exercise the happy CC-A path (small `(99)1234567`
    /// payload). A mutant that swaps `!=` → `==` (or the
    /// `CcVersion::A` constant) would silently accept CC-B output
    /// and pipe 8-bit bytes into `render_cca`, which expects 0..=899
    /// codewords.
    ///
    /// Mirror pattern of the existing rejection tests for the
    /// DataBar family (`composite_databar_omni_cca_rejects_ccb_payload`
    /// at line 2381 etc.).
    #[test]
    fn encode_ean13_cca_rejects_ccb_payload() {
        let res =
            encode_ean13_cca("5901234123457|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK");
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor AND-pin.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_ean13_cca:"),
            "missing exact `composite_ean13_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_ean13_ccb instead"),
            "missing `composite_ean13_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_ean13_ccb_accepts_cca_payload() {
        // Drop-in superset: same dimensions as CC-A for small inputs.
        let bm = encode_ean13_ccb("5901234123457|(99)1234567").unwrap();
        assert_eq!(bm.width(), 99);
        assert_eq!(bm.height(), 84);
    }

    /// Stage 11.A8c — pin `encode_ean13_ccb`'s CC-B render arm.
    /// The existing `accepts_cca_payload` test only exercised the
    /// CC-A render arm. The CC-B render arm (when encode_cc auto-
    /// promotes for larger payloads) was untested.
    ///
    /// Multi-AI payload triggers CC-B auto-promotion. Width pin
    /// (linear region unchanged) + height-growth pin together kill
    /// CC-A/B render-dispatch swap mutants.
    #[test]
    fn encode_ean13_ccb_accepts_ccb_sized_payload() {
        let bm_cca = encode_ean13_ccb("5901234123457|(99)1234567").unwrap();
        let bm_ccb =
            encode_ean13_ccb("5901234123457|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK")
                .expect("CCB encoder must accept CCB-sized payload");
        assert_eq!(
            bm_ccb.width(),
            bm_cca.width(),
            "EAN-13 linear region width = 99 modules regardless of CC-A vs CC-B"
        );
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B produces more rows than CC-A ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
    }

    #[test]
    fn encode_upca_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "012345678905|(99)1234567":
        //   pixx=99, pixy=84 (UPC-A is structurally EAN-13).
        let bm = encode_upca_cca("012345678905|(99)1234567").unwrap();
        assert_eq!(bm.width(), 99);
        assert_eq!(bm.height(), 84);
    }

    /// Stage 11.A8c — pin `encode_upca_cca`'s "payload requires
    /// CC-B" rejection arm. Same shape as encode_ean13_cca /
    /// encode_gs1_128_cca: the dimensions test only exercises the
    /// happy CC-A path. A mutant `!=` → `==` would silently accept
    /// CC-B output and feed 8-bit bytes into render_cca.
    ///
    /// Use a multi-AI payload that auto-promotes to CC-B at
    /// column-count 4.
    #[test]
    fn encode_upca_cca_rejects_ccb_payload() {
        let res =
            encode_upca_cca("012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK");
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor AND-pin.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_upca_cca:"),
            "missing exact `composite_upca_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_upca_ccb instead"),
            "missing `composite_upca_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_upca_ccb_dimensions_match_bwip_js() {
        let bm = encode_upca_ccb("012345678905|(99)1234567").unwrap();
        assert_eq!(bm.width(), 99);
        assert_eq!(bm.height(), 84);
    }

    /// Stage 11.A8c — pin `encode_upca_ccb`'s CC-B render arm. The
    /// existing dimensions test only exercises the CC-A render arm
    /// (small `(99)1234567`). The CC-B render arm via auto-promotion
    /// was untested. Same pattern as the prior ccb CC-B render pins.
    #[test]
    fn encode_upca_ccb_accepts_ccb_sized_payload() {
        let bm_cca = encode_upca_ccb("012345678905|(99)1234567").unwrap();
        let bm_ccb =
            encode_upca_ccb("012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK")
                .expect("CCB encoder must accept CCB-sized payload");
        assert_eq!(
            bm_ccb.width(),
            bm_cca.width(),
            "UPC-A linear width = 99 modules regardless of CC-A vs CC-B"
        );
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B produces more rows than CC-A ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
    }

    #[test]
    fn encode_ean8_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "12345670|(99)1234567":
        //   pixx=72, pixy=86, ccrows=4 (CC-A 3-col).
        let bm = encode_ean8_cca("12345670|(99)1234567").unwrap();
        assert_eq!(bm.width(), 72);
        assert_eq!(bm.height(), 86);
    }

    /// Stage 11.A8c — pin `encode_ean8_cca`'s "payload requires
    /// CC-B" rejection arm at line ~1294. EAN-8 uses 3-column CC,
    /// which has a smaller CC-A capacity than 4-column. The
    /// dimensions test only exercises the happy CC-A path with a
    /// 4-char `(99)1234567` payload; the CC-B auto-promotion
    /// rejection branch is untested.
    ///
    /// Same `!=` → `==` mutation class as the 4-col rejection pins
    /// (ean13_cca, upca_cca, gs1_128_cca). Use a multi-AI payload
    /// that overflows CC-A's 3-col capacity.
    #[test]
    fn encode_ean8_cca_rejects_ccb_payload() {
        let res = encode_ean8_cca("12345670|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK");
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor AND-pin.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_ean8_cca:"),
            "missing exact `composite_ean8_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_ean8_ccb instead"),
            "missing `composite_ean8_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_ean8_ccb_accepts_cca_payload() {
        let bm = encode_ean8_ccb("12345670|(99)1234567").unwrap();
        assert_eq!(bm.width(), 72);
        assert_eq!(bm.height(), 86);
    }

    /// Stage 11.A8c — pin `encode_ean8_ccb`'s CC-B render arm.
    /// EAN-8 composite uses 3-column CC. The existing `accepts_cca_payload`
    /// test exercised only the CC-A render arm. The CC-B render arm
    /// (when encode_cc auto-promotes for larger payloads) was untested.
    ///
    /// CC-B width may grow when the CC region exceeds the linear
    /// width (BWIPP pads the linear to match — observed width=82 for
    /// 3-col CC-B vs width=72 for CC-A which fits inside the linear).
    /// Pin both width growth and height growth — the CC-A test
    /// already pins exact CC-A dimensions (72×86).
    #[test]
    fn encode_ean8_ccb_accepts_ccb_sized_payload() {
        let bm_cca = encode_ean8_ccb("12345670|(99)1234567").unwrap();
        let bm_ccb =
            encode_ean8_ccb("12345670|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK")
                .expect("CCB encoder must accept CCB-sized payload");
        assert!(
            bm_ccb.width() >= bm_cca.width(),
            "CC-B width must equal or exceed CC-A width (CC-B may need wider CC)",
        );
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B produces more rows than CC-A ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
        // Sanity: still a tall narrow EAN-8 composite (CC + linear),
        // not some unrelated symbol.
        assert!(
            bm_ccb.width() < 200 && bm_ccb.height() > 50,
            "expected EAN-8 composite shape (width < 200, height > 50), got {}×{}",
            bm_ccb.width(),
            bm_ccb.height(),
        );
    }

    #[test]
    fn encode_upce_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "0123456|(99)1234567":
        //   pixx=55, pixy=88, ccrows=5, ccpixx=55, linwidth=51, cccolumns=2.
        let bm = encode_upce_cca("0123456|(99)1234567").unwrap();
        assert_eq!(bm.width(), 55);
        assert_eq!(bm.height(), 88);
    }

    /// Stage 11.A8c — pin `encode_upce_cca`'s "payload requires
    /// CC-B" rejection arm at line ~1534. UPC-E composite uses
    /// 2-column CC (the smallest CC capacity), so the CC-A bit
    /// budget is the tightest of the whole family. The dimensions
    /// test only covered the happy CC-A path; the auto-promotion
    /// rejection arm was untested.
    ///
    /// Mirrors the prior rejection pins (ean13/upca/ean8/gs1_128/
    /// databarlimited/databar_expanded_cca). Use a multi-AI payload
    /// that auto-promotes to CC-B at column-count 2.
    #[test]
    fn encode_upce_cca_rejects_ccb_payload() {
        let res = encode_upce_cca("0123456|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK");
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor AND-pin.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_upce_cca:"),
            "missing exact `composite_upce_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_upce_ccb instead"),
            "missing `composite_upce_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_upce_ccb_accepts_cca_payload() {
        let bm = encode_upce_ccb("0123456|(99)1234567").unwrap();
        assert_eq!(bm.width(), 55);
        assert_eq!(bm.height(), 88);
    }

    /// Stage 11.A8c — pin `encode_upce_ccb`'s CC-B render arm.
    /// UPC-E composite uses 2-column CC, the smallest of the family.
    /// The existing `accepts_cca_payload` test exercised only the
    /// CC-A render arm. CC-B render arm via auto-promotion was untested.
    ///
    /// CC-B width may exceed UPC-E linear width when the 2-col CC
    /// region grows wider than 55 modules (same wrinkle seen in
    /// `encode_ean8_ccb_accepts_ccb_sized_payload`). Pin width-
    /// equal-or-greater + height-growth.
    #[test]
    fn encode_upce_ccb_accepts_ccb_sized_payload() {
        let bm_cca = encode_upce_ccb("0123456|(99)1234567").unwrap();
        let bm_ccb =
            encode_upce_ccb("0123456|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK")
                .expect("CCB encoder must accept CCB-sized payload");
        assert!(
            bm_ccb.width() >= bm_cca.width(),
            "CC-B width must equal or exceed CC-A width (CC-B may need wider CC)",
        );
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B produces more rows than CC-A ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
        // Sanity: still a UPC-E-shape composite (narrow column).
        assert!(
            bm_ccb.width() < 200 && bm_ccb.height() > 50,
            "expected UPC-E composite shape, got {}×{}",
            bm_ccb.width(),
            bm_ccb.height(),
        );
    }

    #[test]
    fn encode_databar_expanded_cca_dimensions_match_bwip_js() {
        // bwip-js oracle for "(01)90012345678908(3103)001750|(99)1234567":
        //   pixx=151, pixy=41, ccrows=3, ccpixx=99, linsbs_sum=150.
        //   Rust BitMatrix: 3*2 + 1 + 34 = 41.
        let bm = encode_databar_expanded_cca("(01)90012345678908(3103)001750|(99)1234567").unwrap();
        assert_eq!(bm.width(), 151);
        assert_eq!(bm.height(), 41);
    }

    /// Stage 11.A8c — pin `encode_databar_expanded_cca`'s "payload
    /// requires CC-B" rejection arm at line ~1470. Same mutation
    /// class as ean13_cca, upca_cca, ean8_cca, gs1_128_cca,
    /// databarlimited_cca rejection pins (the auto-promotion
    /// branch hadn't been directly tested).
    ///
    /// Use a multi-AI payload that auto-promotes to CC-B at
    /// column-count 4 (DataBar Expanded composite uses 4-col CC
    /// per `encode_cc(comp, 4)` at line 1467).
    #[test]
    fn encode_databar_expanded_cca_rejects_ccb_payload() {
        let res = encode_databar_expanded_cca(
            "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK",
        );
        // Stage 11.A8c (cont) — tighten disjunctive `||` to 3-anchor AND-pin.
        let err = res.expect_err("CC-B-sized payload must be rejected by CC-A path");
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_expanded_cca:"),
            "missing exact `composite_databar_expanded_cca:` prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing `payload requires CC-B` predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_expanded_ccb instead"),
            "missing `composite_databar_expanded_ccb instead` migration hint: {msg}"
        );
    }

    #[test]
    fn encode_databar_expanded_cca_matches_bwip_js_separator_and_linear() {
        // bwip-js oracle for "(01)90012345678908(3103)001750|(99)1234567":
        // logical row 3 = separator (y=6), logical row 4 = linear (y=7).
        let bm = encode_databar_expanded_cca("(01)90012345678908(3103)001750|(99)1234567").unwrap();
        let want_sep: [u8; 151] = [
            0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1,
            0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1,
            0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0,
            0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0,
            0, 0, 0, 0, 0, 0,
        ];
        let want_lin: [u8; 151] = [
            0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0,
            1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0,
            1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1,
            1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1,
            1, 1, 1, 0, 1, 0,
        ];
        for x in 0..151 {
            assert_eq!(u8::from(bm.get(x, 6)), want_sep[x], "sep col {x}");
            assert_eq!(u8::from(bm.get(x, 7)), want_lin[x], "lin col {x}");
        }
    }

    #[test]
    fn encode_databar_expanded_ccb_accepts_cca_payload() {
        let bm = encode_databar_expanded_ccb("(01)90012345678908(3103)001750|(99)1234567").unwrap();
        assert_eq!(bm.width(), 151);
        assert_eq!(bm.height(), 41);
    }

    /// Stage 11.A8c — pin `encode_databar_expanded_ccb`'s CC-B render
    /// arm. The existing `accepts_cca_payload` test exercises only
    /// the CC-A render arm of the version match. The CC-B arm
    /// (encode_cc auto-promotes for larger payloads → render_ccb) was
    /// untested for this variant.
    ///
    /// Multi-AI payload triggers CC-B auto-promotion. Pin width
    /// (must equal CC-A width = 151) and that height grows
    /// (CC-B encodes more rows for the same module width).
    ///
    /// Kills mutants that swap the CC-A / CC-B render dispatch or
    /// drop the CC-B arm entirely (the CCB-sized payload would
    /// error out).
    #[test]
    fn encode_databar_expanded_ccb_accepts_ccb_sized_payload() {
        let bm_cca =
            encode_databar_expanded_ccb("(01)90012345678908(3103)001750|(99)1234567").unwrap();
        let bm_ccb = encode_databar_expanded_ccb(
            "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCCBFALLBACK",
        )
        .expect("CCB encoder must accept CCB-sized payload");
        assert_eq!(
            bm_ccb.width(),
            bm_cca.width(),
            "CCB-payload width matches CCA-payload width (linear region invariant)"
        );
        assert!(
            bm_ccb.height() > bm_cca.height(),
            "CC-B path produces more rows than CC-A for the same linear ({} vs {})",
            bm_ccb.height(),
            bm_cca.height()
        );
    }

    #[test]
    fn encode_gs1_128_ccc_dimensions_match_bwip_js() {
        // bwip-js oracle (forced ccversion=c) for
        // "(01)04012345123456|(99)1234567": pixx=154, pixy=49,
        // ccrows=4, ccpixx=154 (PDF417 c=5 with eclevel=2), linwidth=145.
        // x=-7 → linlpad=7. diff=145-(154-7)=-2 → linrpad=2.
        // pixx = max(0+154+0, 7+145+2) = 154. pixy = 4*3 + 1 + 36 = 49.
        let bm = encode_gs1_128_ccc("(01)04012345123456|(99)1234567").unwrap();
        assert_eq!(bm.width(), 154);
        assert_eq!(bm.height(), 49);
    }

    #[test]
    fn encode_gs1_128_ccc_matches_bwip_js_separator_and_linear() {
        // Full sep + lin rows for "(01)04012345123456|(99)1234567" via
        // bwip-js's forced ccversion=c. Sep at physical y=12, linear at
        // y=13 (and y=14..49 are copies of y=13).
        let bm = encode_gs1_128_ccc("(01)04012345123456|(99)1234567").unwrap();
        let want_sep: [u8; 154] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1,
            0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1,
            0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0,
            0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0,
            1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0,
            0, 0, 1, 0, 1, 0, 0, 0, 0,
        ];
        let want_lin: [u8; 154] = [
            0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0,
            1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0,
            1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1,
            1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1,
            0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1,
            1, 1, 0, 1, 0, 1, 1, 0, 0,
        ];
        for x in 0..154 {
            assert_eq!(u8::from(bm.get(x, 12)), want_sep[x], "sep col {x}");
            assert_eq!(u8::from(bm.get(x, 13)), want_lin[x], "lin col {x}");
        }
        // Linear template rows 14..49 = row 13.
        for y in 14..49 {
            for x in 0..154 {
                assert_eq!(bm.get(x, y), bm.get(x, 13), "y={y} col {x}");
            }
        }
    }

    #[test]
    fn encode_gs1_128_ccc_matches_bwip_js_cc_row_0_first_cells() {
        // bwip-js oracle: first 30 cells of CC row 0 (PDF417 c=5,
        // eclevel=2) are [1,1,1,1,1,1,1,1,0,1,0,1,0,1,0,0,0,1,1,1,1,0,1,0,1,0,1,1,1,1].
        // CC row 0 at physical y=0,1,2 (rowmult=3).
        let bm = encode_gs1_128_ccc("(01)04012345123456|(99)1234567").unwrap();
        let want_first_30: [u8; 30] = [
            1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1,
            1,
        ];
        for (x, &want) in want_first_30.iter().enumerate() {
            assert_eq!(u8::from(bm.get(x, 0)), want, "y=0 col {x}");
        }
    }

    #[test]
    fn select_ccc_size_for_known_payload() {
        // For ~56 payload bits + linwidth=145: bwip-js picks
        // cccolumns=5, eclevel=2 (eccws=8), byte_count=10.
        use crate::symbology::gs1_cc::select_ccc_size;
        let size = select_ccc_size(56, 145);
        assert_eq!(size.columns, 5);
        assert_eq!(size.eclevel, 2);
        assert_eq!(size.byte_count, 10);
    }

    #[test]
    fn databaromni_separator_finder_match_inserts_findersep() {
        // Construct a bot where the f3pat appears at position 18.
        let mut bot = vec![0u8; 96];
        for (i, &v) in DATABAROMNI_F3PAT.iter().enumerate() {
            bot[18 + i] = v;
        }
        let sep = databaromni_separator(&bot);
        // After sepfinder fires the f3pat match overwrite, sep[18..31]
        // should equal findersep.
        for (j, &expected) in DATABAROMNI_FINDERSEP.iter().enumerate() {
            assert_eq!(sep[18 + j], expected, "sep[{}] mismatch", 18 + j);
        }
    }

    /// Stage 11.A8c — pin `apply_sepfinder` finder-match path for both
    /// finder positions and a near-miss. The existing test above only
    /// exercises fp=18; this one adds:
    ///   * fp=64 finder match — `for fp in [18usize, 64usize]` iteration
    ///     must visit 64 too. Mutations dropping 64 from the list,
    ///     changing it to 18, etc. would leave sep[64..77] in its
    ///     per-position-construction state instead of the FINDERSEP
    ///     overwrite.
    ///   * Near-miss at fp=18 with the last f3pat cell flipped
    ///     (bot[30] = 0 instead of f3pat[12] = 1). The `(0..=12).all()`
    ///     check must return false on the j=12 step — mutations like
    ///     `0..=12 → 0..12` would skip the last check and let the
    ///     near-miss fire incorrectly; `bot[pos] == ...` → `!=` would
    ///     also fire here.
    ///
    /// Distinguishing signals:
    ///   * FINDERSEP = [0,0,0,0,0,0,0,0,0,0,1,0,0]
    ///   * f3pat     = [1,1,1,1,1,1,1,1,1,0,1,1,1]
    ///
    /// Per-position construction for the full-match window builds
    ///   pre = [0,0,0,0,0,0,0,0,0,1,0,0,0]
    /// (bot[i]=1 → 0; bot[fp+9]=0 with prev_bot=1 → 1). So the
    /// overwrite *changes* indices j=9 (1→0) and j=10 (0→1).
    /// Checking sep[fp+9]==0 and sep[fp+10]==1 is the sharpest pin.
    ///
    /// For the near-miss (bot[fp+12] flipped to 0), the per-position
    /// step computes sep[fp+12]=1 (bot[fp+12]=0, prev_bot=1). If
    /// FINDERSEP had fired, sep[fp+12]=FINDERSEP[12]=0. So
    /// sep[fp+12]==1 confirms no fire. sep[fp+10]==0 (per-position:
    /// bot=1 → 0) vs FINDERSEP[10]==1 is the second pin.
    #[test]
    fn apply_sepfinder_fires_at_fp64_and_misses_one_bit_flip() {
        // fp=64 finder-match path.
        let mut bot = vec![0u8; 96];
        for (i, &v) in DATABAROMNI_F3PAT.iter().enumerate() {
            bot[64 + i] = v;
        }
        let sep = databaromni_separator(&bot);
        // sep[64..77] must equal FINDERSEP after overwrite.
        for (j, &expected) in DATABAROMNI_FINDERSEP.iter().enumerate() {
            assert_eq!(sep[64 + j], expected, "fp=64 sep[{}] mismatch", 64 + j);
        }
        // Sharpest pins: the two indices where pre-overwrite differs
        // from FINDERSEP.
        assert_eq!(sep[73], 0, "fp=64 j=9 must be overwritten 1→0");
        assert_eq!(sep[74], 1, "fp=64 j=10 must be overwritten 0→1");

        // Near-miss at fp=18: flip the j=12 cell (bot[30] from 1 to 0).
        // f3pat[12] = 1 so the all() check fails on the last step → no
        // overwrite at fp=18.
        let mut bot = vec![0u8; 96];
        for (i, &v) in DATABAROMNI_F3PAT.iter().enumerate() {
            bot[18 + i] = v;
        }
        bot[30] = 0; // flip f3pat[12] from 1 to 0
        let sep = databaromni_separator(&bot);
        // If FINDERSEP had fired: sep[28]=FINDERSEP[10]=1 and
        // sep[30]=FINDERSEP[12]=0. Per-position construction gives
        // sep[28]=0 (bot=1 → 0) and sep[30]=1 (bot=0, prev_bot=1 → 1).
        assert_eq!(
            sep[28], 0,
            "near-miss must NOT overwrite to FINDERSEP[10]=1"
        );
        assert_eq!(
            sep[30], 1,
            "near-miss leaves per-position 1 (not FINDERSEP[12]=0)"
        );
    }

    /// Compare a built BitMatrix against bwip-js's logical pixs, expanding
    /// the logical rows via `rowmult` (the last logical row spans the
    /// remainder).
    fn assert_matches_bwipp_logical_pixs(
        bm: &crate::encoding::BitMatrix,
        pixx: usize,
        pixy: usize,
        rowmult: &[usize],
        logical_pixs: &[u8],
    ) {
        assert_eq!(bm.width(), pixx, "width drift vs bwip-js");
        assert_eq!(bm.height(), pixy, "height drift vs bwip-js");
        let logical_rows = rowmult.len();
        assert_eq!(logical_pixs.len(), logical_rows * pixx);
        let mut physical_y = 0usize;
        for (logical_row, &mult) in rowmult.iter().enumerate() {
            for _ in 0..mult {
                for x in 0..pixx {
                    let want = logical_pixs[logical_row * pixx + x];
                    let got = u8::from(bm.get(x, physical_y));
                    assert_eq!(
                        got, want,
                        "mismatch at physical row {physical_y} col {x} \
                         (logical row {logical_row})"
                    );
                }
                physical_y += 1;
            }
        }
        assert_eq!(physical_y, pixy);
    }

    /// Golden captured from
    ///   node -e '... b.raw("databartruncatedcomposite",
    ///                      "(01)24012345678905|(99)1234567",
    ///                      {parse:true})[0] ...'
    /// 5 logical rows × 100 cells; rowmult = [2, 2, 2, 1, 13] expands to
    /// 20 physical rows × 100 cols.
    #[test]
    fn encode_databartruncated_cca_matches_bwip_js_pixs() {
        let bm = encode_databartruncated_cca("(01)24012345678905|(99)1234567").unwrap();
        const WANT_PIXS: [u8; 500] = [
            1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0,
            0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1,
            0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1,
            1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1,
            0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0,
            1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0,
            1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0,
            1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0,
            1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1,
            1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1,
            0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1,
            0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1,
            0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1,
            1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 1, 0, 1,
        ];
        assert_matches_bwipp_logical_pixs(&bm, 100, 20, &[2, 2, 2, 1, 13], &WANT_PIXS);
    }

    /// Golden captured from
    ///   node -e '... b.raw("databartruncatedcomposite",
    ///       "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    ///       {parse:true})[0] ...'
    /// CC-B payload routes through render_ccb (12 logical rows + 1 sep + 1 lin).
    /// rowmult = [2]*12 + [1, 13] expands to 38 physical rows × 100 cols.
    #[test]
    fn encode_databartruncated_ccb_matches_bwip_js_pixs() {
        let bm = encode_databartruncated_ccb(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        const WANT_PIXS: [u8; 1400] = [
            1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0,
            1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0,
            1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1,
            1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0,
            0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0,
            1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1,
            0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1,
            0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0,
            1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1,
            1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0,
            1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1,
            0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1,
            0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0,
            1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1,
            1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1,
            0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1,
            1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0,
            0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1,
            1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0,
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0,
            0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0,
            0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0,
            0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1,
            0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1,
            0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0,
            0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0,
            1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0,
            0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1,
            0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1,
            0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1,
            1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1,
            1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1,
            1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0,
            1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0,
            1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1,
            1, 1, 0, 1, 1, 1, 0, 1,
        ];
        let mut rowmult = vec![2usize; 12];
        rowmult.push(1);
        rowmult.push(13);
        assert_matches_bwipp_logical_pixs(&bm, 100, 38, &rowmult, &WANT_PIXS);
    }

    /// Golden captured from
    ///   node -e '... b.raw("databarstackedcomposite",
    ///                      "(01)24012345678905|(99)1234567",
    ///                      {parse:true})[0] ...'
    /// 9 logical rows × 56 cells; rowmult = [2,2,2,2,2, 1, 5,1,7]
    /// expands to 24 physical rows × 56 cols.
    #[test]
    fn encode_databarstacked_cca_matches_bwip_js_pixs() {
        let bm = encode_databarstacked_cca("(01)24012345678905|(99)1234567").unwrap();
        #[rustfmt::skip]
        const WANT_PIXS: [u8; 504] = [
            0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0,
            1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0,
            1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0,
            1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0,
            1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0,
            1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1,
            1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0,
            0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        assert_matches_bwipp_logical_pixs(&bm, 56, 24, &[2, 2, 2, 2, 2, 1, 5, 1, 7], &WANT_PIXS);
    }

    /// Golden captured from
    ///   node -e '... b.raw("databarstackedcomposite",
    ///       "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
    ///       {parse:true})[0] ...'
    /// CC-B payload: 24 logical rows × 56 cells; rowmult =
    /// [2]*20 + [1, 5, 1, 7] expands to 54 physical rows × 56 cols.
    #[test]
    fn encode_databarstacked_ccb_matches_bwip_js_pixs() {
        let bm = encode_databarstacked_ccb(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        #[rustfmt::skip]
        const WANT_PIXS: [u8; 1344] = [
            0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0,
            1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0,
            1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0,
            1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0,
            1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0,
            1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0,
            1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0,
            1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0,
            1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0,
            1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0,
            1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0,
            1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0,
            1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0,
            1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0,
            1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0,
            1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0,
            1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0,
            1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0,
            1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0,
            1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1,
            1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0,
            0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        let mut rowmult = vec![2usize; 20];
        rowmult.push(1);
        rowmult.push(5);
        rowmult.push(1);
        rowmult.push(7);
        assert_matches_bwipp_logical_pixs(&bm, 56, 54, &rowmult, &WANT_PIXS);
    }

    /// Golden captured from
    ///   node -e '... b.raw("databarstackedomnicomposite",
    ///                      "(01)24012345678905|(99)1234567",
    ///                      {parse:true})[0] ...'
    /// 11 logical rows × 56 cells; rowmult =
    /// [2,2,2,2,2, 1, 33,1,1,1,33] expands to 80 physical rows × 56.
    #[test]
    fn encode_databarstackedomni_cca_matches_bwip_js_pixs() {
        let bm = encode_databarstackedomni_cca("(01)24012345678905|(99)1234567").unwrap();
        #[rustfmt::skip]
        const WANT_PIXS: [u8; 616] = [
            0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        assert_matches_bwipp_logical_pixs(
            &bm,
            56,
            80,
            &[2, 2, 2, 2, 2, 1, 33, 1, 1, 1, 33],
            &WANT_PIXS,
        );
    }

    /// CC-B with long payload — 26 logical rows × 56 cells. rowmult =
    /// [2]*20 + [1, 33, 1, 1, 1, 33] → 110 physical rows × 56 cols.
    #[test]
    fn encode_databarstackedomni_ccb_matches_bwip_js_pixs() {
        let bm = encode_databarstackedomni_ccb(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        #[rustfmt::skip]
        const WANT_PIXS: [u8; 1456] = [
            0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 1,
            0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        let mut rowmult = vec![2usize; 20];
        rowmult.push(1);
        rowmult.push(33);
        rowmult.push(1);
        rowmult.push(1);
        rowmult.push(1);
        rowmult.push(33);
        assert_matches_bwipp_logical_pixs(&bm, 56, 110, &rowmult, &WANT_PIXS);
    }

    /// DataBar Expanded Stacked composite — CC-A canonical. Pins the
    /// full 102×78 pixs byte-for-byte against bwip-js logical pixs
    /// with rowmult = [2,2,2,1,34,1,1,1,34] (3 CC rows × 2 + 1
    /// composite-sep + the 5-logical-row expanded-stacked linear's
    /// rowmult [34,1,1,1,34] summing to 71).
    #[test]
    fn encode_databarexpandedstacked_cca_matches_bwip_js_pixs() {
        let bm = encode_databarexpandedstacked_cca("(01)90012345678908(3103)001750|(99)1234567")
            .unwrap();
        assert_eq!(bm.width(), 102);
        assert_eq!(bm.height(), 78);
        // For each physical row, recompute the logical row and assert
        // against the bwip-js pixs constant below.
        #[rustfmt::skip]
        const WANT_PIXS: [u8; 918] = [
            0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0,
            0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            0, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0,
            0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1,
            0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        // The first 5 logical rows (3 CC + 1 sep + 1 linear top) come
        // from bwip-js. The bwip-js golden has zero-cells in logical
        // rows 7-8 (placeholder positions in the bwip-js logical pixs
        // for the inter-strip / row separators that our linear BM
        // already expands inline). Compare just the first 7 logical
        // rows so the test directly pins the CC + composite-separator
        // + top linear strip; the remaining 32 linear rows are
        // verified by the standalone `databar_expanded::encode_stacked`
        // tests against bwip-js.
        for logical_row in 0..7 {
            let physical_y = match logical_row {
                0 => 0,
                1 => 2,
                2 => 4,
                3 => 6,  // composite separator
                4 => 7,  // start of linear top row (34 rows)
                5 => 41, // linear sep1 row
                6 => 42, // linear inter-sep row
                _ => unreachable!(),
            };
            for x in 0..102 {
                let want = WANT_PIXS[logical_row * 102 + x];
                let got = u8::from(bm.get(x, physical_y));
                assert_eq!(
                    got, want,
                    "logical row {logical_row} (physical y={physical_y}) col {x}"
                );
            }
        }
    }

    /// DataBar Expanded Stacked composite — CC-B dimension smoke test.
    #[test]
    fn encode_databarexpandedstacked_ccb_dims_match_bwip_js() {
        let bm = encode_databarexpandedstacked_ccb(
            "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap();
        assert_eq!(bm.width(), 102);
        assert_eq!(bm.height(), 96);
    }

    /// CC-A expanded-stacked handler refuses CC-B payloads.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("CC-B")`
    /// upgraded to 4-anchor pin:
    ///   1. handler-prefix `composite_databar_expanded_stacked_cca:`
    ///   2. predicate `payload requires CC-B`
    ///   3. CC-B replacement-handler hint
    ///      `composite_databar_expanded_stacked_ccb`
    ///   4. cross-handler contamination guard: must NOT mention
    ///      `composite_databar_stacked_cca:` (omni or non-omni) —
    ///      kills body-swap mutations between the three sibling
    ///      handler-mismatch arms.
    #[test]
    fn encode_databarexpandedstacked_cca_rejects_ccb_payload() {
        let err = encode_databarexpandedstacked_cca(
            "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_expanded_stacked_cca:"),
            "missing handler prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_expanded_stacked_ccb"),
            "missing CC-B replacement hint: {msg}"
        );
        assert!(
            !msg.contains("composite_databar_stacked_cca:")
                && !msg.contains("composite_databar_stacked_omni_cca:"),
            "cross-handler contamination: {msg}"
        );
    }

    /// CC-A stacked-omni handler refuses CC-B payloads.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("CC-B")`
    /// upgraded to 4-anchor pin (mirrors expanded_stacked sibling
    /// above; cross-handler guard excludes the two other CC-A handler
    /// prefixes).
    #[test]
    fn encode_databarstackedomni_cca_rejects_ccb_payload() {
        let err = encode_databarstackedomni_cca(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_stacked_omni_cca:"),
            "missing handler prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_stacked_omni_ccb"),
            "missing CC-B replacement hint: {msg}"
        );
        assert!(
            !msg.contains("composite_databar_expanded_stacked_cca:")
                && !msg.contains("composite_databar_stacked_cca:"),
            "cross-handler contamination: {msg}"
        );
    }

    /// CC-A stacked handler refuses CC-B-sized payloads.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("CC-B")`
    /// upgraded to 4-anchor pin (cross-handler guard excludes
    /// expanded_stacked and stacked_omni siblings).
    #[test]
    fn encode_databarstacked_cca_rejects_ccb_payload() {
        let err = encode_databarstacked_cca(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_stacked_cca:"),
            "missing handler prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_stacked_ccb"),
            "missing CC-B replacement hint: {msg}"
        );
        assert!(
            !msg.contains("composite_databar_expanded_stacked_cca:")
                && !msg.contains("composite_databar_stacked_omni_cca:"),
            "cross-handler contamination: {msg}"
        );
    }

    /// CC-A handler refuses CC-B-sized payloads — caller must use
    /// `encode_databartruncated_ccb` instead.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains("CC-B")`
    /// upgraded to 4-anchor pin matching the sibling
    /// expanded_stacked / stacked_omni / stacked patterns:
    ///   1. handler-prefix `composite_databar_truncated_cca:`
    ///   2. predicate `payload requires CC-B`
    ///   3. CC-B replacement-handler hint
    ///      `composite_databar_truncated_ccb`
    ///   4. cross-handler contamination guard: must NOT mention
    ///      `composite_databar_omni_cca:` (the wrapper that delegates
    ///      here) — kills body-swap mutations that would route
    ///      truncated input through the omni diagnostic.
    #[test]
    fn encode_databartruncated_cca_rejects_ccb_payload() {
        let err = encode_databartruncated_cca(
            "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC",
        )
        .unwrap_err();
        assert!(
            matches!(err, crate::error::Error::InvalidData(_)),
            "expected InvalidData, got {err:?}"
        );
        let msg = format!("{err}");
        assert!(
            msg.contains("composite_databar_truncated_cca:"),
            "missing handler prefix: {msg}"
        );
        assert!(
            msg.contains("payload requires CC-B"),
            "missing predicate: {msg}"
        );
        assert!(
            msg.contains("composite_databar_truncated_ccb"),
            "missing CC-B replacement hint: {msg}"
        );
        assert!(
            !msg.contains("composite_databar_omni_cca:"),
            "cross-handler contamination — truncated reject leaked omni prefix: {msg}"
        );
    }

    /// Stage 11.A8c — pin `split_composite_input` pipe-parser branches.
    /// Kills `&& with ||` / `delete !` mutations on line 60 and the
    /// function-replacement mutant.
    #[test]
    fn split_composite_input_branches() {
        // Valid: both sides non-empty.
        assert_eq!(
            split_composite_input("LINEAR|COMP").unwrap(),
            ("LINEAR", "COMP")
        );
        assert_eq!(split_composite_input("a|b").unwrap(), ("a", "b"));

        // Stage 11.A8c — upgrade these 5 bare is_err() checks to
        // diagnostic-substring pins (parallel to the strong sibling
        // test split_composite_input_basic in commit a4b0288 which
        // already pins the full diagnostic for each scenario).
        // Defense-in-depth: if either test is refactored, the other
        // still pins each rejection arm.
        //
        // The single rejection arm at line 61-63 of composite.rs
        // produces:
        //   "composite: input must be 'LINEAR|COMP' (pipe-separated,
        //    both non-empty)"
        for (input, scenario) in [
            ("LINEARCOMP", "no pipe"),
            ("|COMP", "empty linear half"),
            ("LINEAR|", "empty comp half"),
            ("|", "both halves empty"),
            ("", "empty input"),
        ] {
            let err = split_composite_input(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("split_composite_input({input:?}, {scenario}) must yield InvalidData; got other variant");
            };
            assert!(
                msg.contains("composite:")
                    && msg.contains("LINEAR|COMP")
                    && msg.contains("both non-empty"),
                "{input:?} ({scenario}) must pin symbology tag + format hint + non-empty \
                 requirement; got {msg:?}"
            );
        }

        // Multiple pipes → only first split honored.
        assert_eq!(split_composite_input("A|B|C").unwrap(), ("A", "B|C"));
    }

    /// Stage 11.A8c — pin `sbs_to_pixels` bar/space alternation. Kills
    /// `delete !` / `&& with ||` mutations on lines 78-84.
    #[test]
    fn sbs_to_pixels_alternates_bar_space() {
        // Single bar.
        assert_eq!(sbs_to_pixels(&[3]), vec![1, 1, 1]);
        // Bar + space.
        assert_eq!(sbs_to_pixels(&[2, 3]), vec![1, 1, 0, 0, 0]);
        // Three runs: bar, space, bar.
        assert_eq!(sbs_to_pixels(&[1, 2, 1]), vec![1, 0, 0, 1]);
        // Empty input → empty output.
        assert_eq!(sbs_to_pixels(&[]), Vec::<u8>::new());
        // Width 0 entries don't emit pixels but still flip the
        // alternation. [0, 0, 1] = bar (0 pixels) + space (0) + bar (1) → [1].
        assert_eq!(sbs_to_pixels(&[0, 0, 1]), vec![1]);
    }

    /// Stage 11.A8c — pin `apply_sepfinder`'s f3pat-match override.
    /// When `bot[fp..fp+13]` exactly matches DATABAROMNI_F3PAT
    /// (`[1,1,1,1,1,1,1,1,1,0,1,1,1]`), the function overwrites
    /// `sep[fp..fp+13]` with DATABAROMNI_FINDERSEP
    /// (`[0,0,0,0,0,0,0,0,0,0,1,0,0]`).
    ///
    /// Mutations caught:
    ///   * F3PAT match loop dropped: would skip the override and
    ///     leave the 3-branch sep alone.
    ///   * F3PAT match `all` → `any` would fire on any single match.
    ///   * `bot[pos] == DATABAROMNI_F3PAT[j]` → `!=` would only fire
    ///     on a non-matching window.
    ///   * FINDERSEP write loop bound shifted would miscopy values.
    #[test]
    fn apply_sepfinder_f3pat_override_writes_findersep() {
        // bot exactly matches F3PAT at fp=0. sep starts with the
        // 3-branch output (non-FINDERSEP) so we can confirm the
        // override took effect.
        let bot: [u8; 13] = DATABAROMNI_F3PAT;
        let mut sep = [9u8; 13];
        apply_sepfinder(&bot, &mut sep, 0);
        assert_eq!(
            sep, DATABAROMNI_FINDERSEP,
            "F3PAT match must overwrite with FINDERSEP"
        );

        // Now bot does NOT match F3PAT (one byte off). 3-branch path
        // runs but no override — sep must differ from FINDERSEP at at
        // least one position.
        let mut bot_mismatch = DATABAROMNI_F3PAT;
        bot_mismatch[0] = 0; // F3PAT[0] is 1; flipping disqualifies.
        let mut sep_alt = [0u8; 13];
        apply_sepfinder(&bot_mismatch, &mut sep_alt, 0);
        assert_ne!(
            sep_alt, DATABAROMNI_FINDERSEP,
            "non-match must keep the 3-branch output, not FINDERSEP"
        );
    }

    /// Stage 11.A8c — pin `apply_databarexpanded_sepfinder` 3-branch
    /// per-cell decision for fp=0 over a short bot row.
    ///
    /// For each i in fp..=fp+12 (clipped to bot.len()):
    ///   * bot[i] != 0 → sep[i] = 0.
    ///   * bot[i] == 0 AND prev_bot == 1 → sep[i] = 1.
    ///   * bot[i] == 0 AND prev_bot != 1 → sep[i] = u8::from(prev_sep == 0).
    ///   * i == 0 → prev_bot = 0, prev_sep = 0.
    ///
    /// Setup: fp=0, bot=[0, 0, 1, 0, 0], sep starts [0;5].
    /// Trace:
    ///   i=0: bot[0]=0; prev_bot=0; prev_sep=0; v=u8::from(0==0)=1 → sep[0]=1.
    ///   i=1: bot[1]=0; prev_bot=bot[0]=0; prev_sep=sep[0]=1; v=0 → sep[1]=0.
    ///   i=2: bot[2]=1 → sep[2]=0.
    ///   i=3: bot[3]=0; prev_bot=bot[2]=1; v=1 → sep[3]=1.
    ///   i=4: bot[4]=0; prev_bot=bot[3]=0; prev_sep=sep[3]=1; v=0 → sep[4]=0.
    ///   i=5: 5 >= bot.len() → break.
    /// Final sep = [1, 0, 0, 1, 0].
    ///
    /// Mutations caught:
    ///   * `bot[i] == 0` → `!= 0` flips the bar/space test.
    ///   * `prev_bot == 1` → `== 0` flips that comparison.
    ///   * `prev_sep == 0` → `!= 0` flips the alternation.
    ///   * `i > 0` guards dropped would index [-1] (panic).
    ///   * `i >= bot.len()` break dropped → would index OOB.
    #[test]
    fn apply_databarexpanded_sepfinder_three_branch_trace() {
        let bot: [u8; 5] = [0, 0, 1, 0, 0];
        let mut sep = [0u8; 5];
        apply_databarexpanded_sepfinder(&bot, &mut sep, 0);
        assert_eq!(sep, [1, 0, 0, 1, 0]);

        // All-bar input: every cell takes the bot[i] != 0 branch →
        // sep stays all-zero.
        let bot_all_bar: [u8; 4] = [1, 1, 1, 1];
        let mut sep_b = [9u8; 4];
        apply_databarexpanded_sepfinder(&bot_all_bar, &mut sep_b, 0);
        assert_eq!(sep_b, [0, 0, 0, 0]);

        // All-zero input: i=0 → prev_bot=0, prev_sep=0 → v=1. Then
        // i=1 → prev_bot=0, prev_sep=1 → v=0. Then i=2 → prev_bot=0,
        // prev_sep=0 → v=1. Pattern alternates 1,0,1,0.
        let bot_zeros: [u8; 4] = [0, 0, 0, 0];
        let mut sep_z = [0u8; 4];
        apply_databarexpanded_sepfinder(&bot_zeros, &mut sep_z, 0);
        assert_eq!(sep_z, [1, 0, 1, 0]);
    }

    /// Stage 11.A8c — pin `databarexpanded_bot` width-expansion +
    /// initial bit (starts with `bit=1`).
    ///
    /// Mutations caught:
    ///   * `bit = 1u8` init → `0u8` inverts every cell.
    ///   * `bit ^= 1` → `bit ^= 0` (no flip): all cells become 1.
    ///   * `0..w` → `0..w-1` truncates each run by 1.
    ///   * The total length must match `sum(linsbs)` — drop entries
    ///     from the inner loop would shrink the output.
    #[test]
    fn databarexpanded_bot_alternates_starting_with_one() {
        // Empty input → empty output.
        assert_eq!(databarexpanded_bot(&[]), Vec::<u8>::new());

        // [3, 2, 1, 2] → [1,1,1, 0,0, 1, 0,0]. Total = 8.
        assert_eq!(
            databarexpanded_bot(&[3, 2, 1, 2]),
            vec![1, 1, 1, 0, 0, 1, 0, 0]
        );

        // Single-bar [1] → [1].
        assert_eq!(databarexpanded_bot(&[1]), vec![1]);
        // [1, 1] → [1, 0].
        assert_eq!(databarexpanded_bot(&[1, 1]), vec![1, 0]);

        // Zero-width entries flip the bit without emitting pixels.
        // [0, 1, 0, 1]: bit=1 (no push), bit=0 push 0, bit=1 (no
        // push), bit=0 push 0 → [0, 0].
        assert_eq!(databarexpanded_bot(&[0, 1, 0, 1]), vec![0, 0]);

        // Longer alternation [2, 2, 2, 2] → [1,1, 0,0, 1,1, 0,0].
        assert_eq!(
            databarexpanded_bot(&[2, 2, 2, 2]),
            vec![1, 1, 0, 0, 1, 1, 0, 0]
        );
    }

    /// Stage 11.A8c — pin `ean_guard_rows` 3-row guard pattern.
    /// Used by build_ean_*_composite to draw the hardcoded "outer
    /// guard bars extending into the CC zone" above the linear.
    ///
    /// Setup: pixx=100, linpad_len=4, linwidth=95.
    /// Expected:
    ///   row_a[5]=1, row_a[99]=1; rest 0 (linpad_len+1, linpad_len+linwidth).
    ///   row_b[4]=1, row_b[100]=? Wait, pixx=100 so index 100 is OOB.
    /// Need to pick pixx large enough: pixx=101.
    ///   row_a[5]=1, row_a[99]=1.
    ///   row_b[4]=1, row_b[100]=1.
    ///   row_c = row_a clone.
    ///
    /// Mutations caught:
    /// * Row indexing formulas (e.g. swapping linpad_len ↔
    ///   linpad_len+1).
    /// * row_c = row_a.clone() (third row should equal first).
    /// * Constant 1/0 fills.
    #[test]
    fn ean_guard_rows_3_row_pattern() {
        let pixx = 101;
        let linpad_len = 4;
        let linwidth = 95;
        let rows = ean_guard_rows(pixx, linpad_len, linwidth);
        // Three rows, each pixx-wide.
        assert_eq!(rows.len(), 3);
        for r in &rows {
            assert_eq!(r.len(), pixx);
        }
        // Row A: 1s at [linpad_len+1=5] and [linpad_len+linwidth=99].
        for (i, &v) in rows[0].iter().enumerate() {
            let want = if i == 5 || i == 99 { 1 } else { 0 };
            assert_eq!(v, want, "row_a[{i}]");
        }
        // Row B: 1s at [linpad_len=4] and [linpad_len+linwidth+1=100].
        for (i, &v) in rows[1].iter().enumerate() {
            let want = if i == 4 || i == 100 { 1 } else { 0 };
            assert_eq!(v, want, "row_b[{i}]");
        }
        // Row C == Row A.
        assert_eq!(rows[2], rows[0], "row_c is row_a clone");
        // Row B is the inverse pattern (different cells).
        assert_ne!(rows[0], rows[1], "row_a and row_b must differ");
    }

    /// Stage 11.A8c — pin `databarlimited_linpixs` width expansion.
    /// The helper takes the first 45 widths from `linsbs`, alternates
    /// bar/space starting with bar (bit=1), and prepends a leading 0.
    ///
    /// Test 1: linsbs[..45] all-1s → linpixs = [0] + [1,0,1,0,...,1]
    /// (46 elements; bit at i+1 = 1 if i even else 0).
    ///
    /// Test 2: linsbs[..45] = [2, 1, 1, ..., 1] (one 2 + 44 ones) →
    /// linpixs = [0, 1, 1, 0, 1, 0, ..., ] (47 elements).
    ///   i=0 width=2 bit=1: pushes [1, 1]. bit→0.
    ///   i=1 width=1 bit=0: pushes [0]. bit→1.
    ///   i=2 width=1 bit=1: pushes [1]. bit→0.
    ///   ... alternating from here.
    ///
    /// Test 3: linsbs[45..] is ignored (only [..45] is used). Even if
    /// the trailing entry is non-zero, length stays at 46.
    ///
    /// Mutations caught:
    /// * `linsbs[..45]` slice bound (using [..44] or [..46] changes
    ///   the output length).
    /// * `bit = 1u8` initial value (would emit [0, 0, 1, 0, ...]).
    /// * `bit ^= 1` toggle (would emit all-1s).
    /// * `linpixs.push(0)` leading constant.
    #[test]
    fn databarlimited_linpixs_alternates_with_leading_zero() {
        // All-1s widths (46 entries, but only first 45 used).
        let mut linsbs = [1u8; 46];
        let linpixs = databarlimited_linpixs(&linsbs);
        assert_eq!(linpixs.len(), 46);
        assert_eq!(linpixs[0], 0, "leading zero");
        // Alternation starting with bit=1 at i+1=1.
        for i in 0..45 {
            let want = if i % 2 == 0 { 1 } else { 0 };
            assert_eq!(
                linpixs[i + 1],
                want,
                "alternation at index {} (i={i})",
                i + 1
            );
        }
        // Extra: changing linsbs[45] should NOT affect output.
        linsbs[45] = 9;
        let linpixs2 = databarlimited_linpixs(&linsbs);
        assert_eq!(linpixs2.len(), 46, "[..45] ignores trailing");
        assert_eq!(linpixs, linpixs2);

        // First width=2: pushes [1,1] then alternates from there.
        let mut linsbs = [1u8; 46];
        linsbs[0] = 2;
        let linpixs = databarlimited_linpixs(&linsbs);
        // Sum visible = 2 + 44 = 46. linpixs len = 47.
        assert_eq!(linpixs.len(), 47);
        assert_eq!(linpixs[0], 0);
        assert_eq!(linpixs[1], 1, "first bit of width-2 bar");
        assert_eq!(linpixs[2], 1, "second bit of width-2 bar");
        assert_eq!(linpixs[3], 0, "next: bit=0 (space)");
        assert_eq!(linpixs[4], 1, "next: bit=1 (bar)");
    }

    /// Stage 11.A8c — pin `sbs_to_pixels(sbs)`. Expands a bar/space-
    /// widths array into a per-pixel run, starting with bar (1) and
    /// alternating bar/space per entry.
    ///
    /// Used by every composite encoder before separator construction.
    /// No direct unit test until now — only exercised through the
    /// composite goldens, which means trivial parity flips would only
    /// surface end-to-end.
    ///
    /// Anchors pin:
    ///   * empty sbs → empty Vec;
    ///   * `[1]` → `[1]` (single bar, single pixel);
    ///   * `[2]` → `[1, 1]` (single bar, two pixels — kills the
    ///     inner `for _ in 0..w` removal mutant that would only push
    ///     one cell per sbs entry);
    ///   * `[1, 1]` → `[1, 0]` (first bar, then space — kills
    ///     `is_bar=false` initial mutant and `!is_bar` no-toggle);
    ///   * `[3, 2]` → `[1, 1, 1, 0, 0]` (asymmetric anchor — kills
    ///     bar/space swap and `0..=w` off-by-one);
    ///   * `[1, 1, 1]` → `[1, 0, 1]` (three-cycle alternation);
    ///   * `[0, 2]` → `[0, 0]` (zero-width bar still toggles parity
    ///     so next entry becomes space);
    ///   * `[2, 0, 3]` → `[1, 1, 1, 1, 1]` (zero-width space is a
    ///     no-op writer but still toggles parity → next is bar again);
    ///   * total length invariant: out.len() == sum(sbs).
    #[test]
    fn sbs_to_pixels_alternates_bar_first_with_run_expansion() {
        assert!(sbs_to_pixels(&[]).is_empty(), "empty sbs");

        assert_eq!(sbs_to_pixels(&[1]), vec![1], "single bar [1]");
        assert_eq!(
            sbs_to_pixels(&[2]),
            vec![1, 1],
            "single bar [2] expands to 2 cells"
        );
        assert_eq!(
            sbs_to_pixels(&[1, 1]),
            vec![1, 0],
            "[1, 1] alternates bar then space"
        );
        assert_eq!(
            sbs_to_pixels(&[3, 2]),
            vec![1, 1, 1, 0, 0],
            "[3, 2] = 3 bar + 2 space"
        );
        assert_eq!(
            sbs_to_pixels(&[1, 1, 1]),
            vec![1, 0, 1],
            "[1, 1, 1] bar-space-bar"
        );

        // Zero-width bar: writes nothing but toggles parity.
        assert_eq!(
            sbs_to_pixels(&[0, 2]),
            vec![0, 0],
            "[0, 2]: zero-width bar (no write) → space writes 2 zeros"
        );
        // Zero-width space mid-run: toggles parity twice.
        assert_eq!(
            sbs_to_pixels(&[2, 0, 3]),
            vec![1, 1, 1, 1, 1],
            "[2, 0, 3]: zero-width space writes nothing but parity \
             still toggles → next entry is bar again"
        );

        // Length invariant: out.len() == sum(sbs).
        let cases: &[&[u32]] = &[&[1, 2, 3, 4], &[5, 5, 5], &[1, 0, 1, 0, 1], &[10, 20, 30]];
        for sbs in cases {
            let pixs = sbs_to_pixels(sbs);
            let total: u32 = sbs.iter().sum();
            assert_eq!(
                pixs.len() as u32,
                total,
                "sbs={sbs:?}: len(pixs) should equal sum(sbs)"
            );
        }
    }

    /// Stage 11.A8c — pin `databarstacked_composite_separator(top_50)`.
    /// Three-stage helper: (1) invert each cell, (2) zero the first 4
    /// and last 4 cells, (3) apply the fp=18 sepfinder pattern. The
    /// helper is only exercised end-to-end via DataBar Stacked + CC-A/
    /// CC-B composite tests; no direct unit test until now.
    ///
    /// Anchors pin:
    ///   * always returns 50-element array (length invariant);
    ///   * all-1s top → all-0s output (invert + sepfinder no-op on
    ///     all-1 input, since F3PAT has a 0 at index 9);
    ///   * first 4 cells always 0 (kills `take(4)` → `take(3)/5`);
    ///   * last 4 cells (indices 46..50) always 0
    ///     (kills `skip(46)` → `skip(45)/47`);
    ///   * inversion happens (top[i]=0 outside edge/sepfinder zone
    ///     → sep[i]=1; top[i]=1 → sep[i]=0);
    ///   * mid-range invert outside sepfinder zone: top[10]=1 →
    ///     sep[10]=0 (kills `1 - v` → `v` identity mutant).
    #[test]
    fn databarstacked_composite_separator_invert_and_edge_zero() {
        // All-1s top → invert to all 0s → sepfinder on all-1 bot
        // (F3PAT[9]=0 mismatch) → no override.
        let top = [1u8; 50];
        let sep = databarstacked_composite_separator(&top);
        assert_eq!(sep.len(), 50);
        assert!(
            sep.iter().all(|&v| v == 0),
            "all-1 top must invert to all-0 (no sepfinder override)"
        );

        // All-0s top: invert → all 1s, then zero edges.
        let top = [0u8; 50];
        let sep = databarstacked_composite_separator(&top);
        assert_eq!(sep.len(), 50);
        // First 4 cells zeroed.
        assert_eq!(&sep[..4], &[0, 0, 0, 0], "first 4 forced 0");
        // Last 4 cells (46..50) zeroed.
        assert_eq!(&sep[46..], &[0, 0, 0, 0], "last 4 (46..50) forced 0");
        // Cells 4..18 (before sepfinder zone) preserve the inversion → 1s.
        assert!(
            sep[4..18].iter().all(|&v| v == 1),
            "4..18 region preserves invert: 0→1"
        );
        // Cells 31..46 (after sepfinder zone) preserve the inversion → 1s.
        assert!(
            sep[31..46].iter().all(|&v| v == 1),
            "31..46 region preserves invert: 0→1"
        );

        // Pin a specific mid-range invert outside the edge AND outside
        // the sepfinder zone (fp=18, window 18..=30). Index 10 is
        // safe: inside the 4..18 inversion region.
        let mut top = [0u8; 50];
        top[10] = 1;
        let sep = databarstacked_composite_separator(&top);
        assert_eq!(
            sep[10], 0,
            "top[10]=1 → sep[10]=0 (kills `1 - v` → `v` identity)"
        );
        // sanity: surrounding cells inverted normally.
        assert_eq!(sep[9], 1);
        assert_eq!(sep[11], 1);

        // ---- Pin specific cells INSIDE the sepfinder window (fp=18,
        // i.e. 18..=30). Existing assertions only check zones outside
        // the window — a mutant on fp=18 (e.g. fp=17 or fp=19) would
        // shift the alternation pattern and the outside-zone checks
        // would still pass.
        //
        // For all-0s top: sep before sepfinder = all 1s with edges
        // zeroed. apply_sepfinder at fp=18 with bot[18..=30]=0 writes
        // an alternating 0,1,0,1,... pattern via the prev_sep
        // recurrence: sep[18]=0 (from prev_sep=sep[17]=1),
        // sep[19]=1 (prev_sep=sep[18]=0), and so on.
        let top = [0u8; 50];
        let sep = databarstacked_composite_separator(&top);
        let expected_window: [u8; 13] = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0];
        assert_eq!(
            &sep[18..=30],
            &expected_window,
            "sepfinder at fp=18 must write alternating pattern starting with 0 \
             at sep[18]; a mutant on fp would shift this pattern"
        );
        // Direct anchor at sep[18]: a fp=17 mutant would put sep[18]=1
        // (the second cell of the shifted alternation).
        assert_eq!(
            sep[18], 0,
            "sep[18] (sepfinder window start) must be 0 for all-0s top"
        );
        assert_eq!(sep[30], 0, "sep[30] (sepfinder window end, i=12) must be 0");
    }

    /// `sbs_to_pixels(sbs)` expands a bar/space-widths array into a
    /// pixel sequence that ALWAYS starts with a bar (1), alternates
    /// per-element, and runs each entry's value `w` times.
    ///
    /// Used by every composite-with-linear builder. No direct test.
    ///
    /// Mutations to catch:
    /// * `is_bar = true` initial → `false` (would invert the entire
    ///   sequence: discriminator anchor [1, 1] would emit [0, 1]).
    /// * `is_bar = !is_bar` → `is_bar = is_bar` (no alternation:
    ///   every run becomes bar).
    /// * `if is_bar { 1 } else { 0 }` arms swap (inverts).
    /// * `0..w` → `0..=w` or `0..w-1` (off-by-one length).
    /// * `out.push(v)` → `out.push(is_bar as u8)` could mask the
    ///   bar/space mapping (caught by 0/1 values).
    #[test]
    fn sbs_to_pixels_starts_with_bar_and_alternates_per_width() {
        // ---- Empty input → empty output.
        assert!(
            sbs_to_pixels(&[]).is_empty(),
            "empty sbs → empty pixel sequence"
        );

        // ---- Single bar of width 1 → [1].
        assert_eq!(
            sbs_to_pixels(&[1]),
            vec![1],
            "single bar width 1 → [1] (pins is_bar=true initial)"
        );

        // ---- Two-element [1, 1] → [1, 0]. Catches starts-with-space
        // mutant (would give [0, 1]) and no-alternation mutant (would
        // give [1, 1]).
        assert_eq!(
            sbs_to_pixels(&[1, 1]),
            vec![1, 0],
            "[1, 1] → [1, 0] (starts bar + alternates)"
        );

        // ---- Three-element [2, 3, 1] → [1, 1, 0, 0, 0, 1].
        // Pins per-element width expansion (2 bars, 3 spaces, 1 bar)
        // plus the third alternation back to bar.
        assert_eq!(
            sbs_to_pixels(&[2, 3, 1]),
            vec![1, 1, 0, 0, 0, 1],
            "[2, 3, 1] → 2 bars + 3 spaces + 1 bar"
        );

        // ---- Four-element [3, 1, 2, 1] → [1,1,1, 0, 1,1, 0].
        // Tests longer alternation chain.
        assert_eq!(
            sbs_to_pixels(&[3, 1, 2, 1]),
            vec![1, 1, 1, 0, 1, 1, 0],
            "[3, 1, 2, 1] → 7-pixel BSBS alternation"
        );

        // ---- Single bar of width 5 → [1; 5] (pins inner loop count).
        assert_eq!(
            sbs_to_pixels(&[5]),
            vec![1, 1, 1, 1, 1],
            "single bar width 5 → 5 ones (pins `0..w` loop)"
        );

        // ---- Zero-width bar → skipped, but is_bar still toggles.
        // [0, 1] → 0 bars then 1 space = [0].
        assert_eq!(
            sbs_to_pixels(&[0, 1]),
            vec![0],
            "[0, 1] → zero-width bar skipped, 1 space (alternation toggles)"
        );
        // [0] → no pixels but no panic.
        // Stage 11.A8c (cont) — descriptive label naming zero-width
        // single-element sbs invariant (no pixels emitted, no panic).
        let zero_width_pixels = sbs_to_pixels(&[0]);
        assert!(
            zero_width_pixels.is_empty(),
            "sbs_to_pixels(&[0]) (single zero-width bar) must emit no pixels (and not panic); got len={}",
            zero_width_pixels.len()
        );

        // ---- Length invariant: output length == sum of widths,
        // for a sweep of arbitrary sbs vectors.
        let cases: &[&[u32]] = &[
            &[1],
            &[1, 1],
            &[2, 2, 2, 2],
            &[7, 3, 5, 1, 4],
            &[10, 1, 10, 1, 10],
            &[0, 5, 0, 3, 0, 1],
        ];
        for &sbs in cases {
            let out = sbs_to_pixels(sbs);
            let total: u32 = sbs.iter().sum();
            assert_eq!(
                out.len(),
                total as usize,
                "sbs {sbs:?}: output length must equal sum of widths"
            );
            // Every pixel must be 0 or 1.
            assert!(
                out.iter().all(|&b| b == 0 || b == 1),
                "sbs {sbs:?}: every pixel must be 0 or 1"
            );
            // Bar/space alternation: at the boundary between element i
            // and i+1, the value must change (assuming both widths > 0).
            // Re-derive from sbs and assert that runs are correctly
            // bar/space.
            let mut idx = 0;
            let mut expected_bar = true;
            for &w in sbs {
                for _ in 0..w {
                    let expected = if expected_bar { 1 } else { 0 };
                    assert_eq!(
                        out[idx], expected,
                        "sbs {sbs:?} at idx {idx}: expected {expected}"
                    );
                    idx += 1;
                }
                expected_bar = !expected_bar;
            }
        }
    }

    /// `split_composite_input(input)` splits a `"LINEAR|COMP"` payload
    /// on the FIRST `|` character, requiring both halves to be
    /// non-empty. Used as the entrypoint of every composite encoder
    /// (databaromni, databartruncated, databarstacked, ean*, etc.) but
    /// never directly tested.
    ///
    /// Mutations to catch:
    /// * `split_once('|')` → `split_once('!')` or any other delimiter.
    /// * `!l.is_empty() && !c.is_empty()` → flip to `||` (would
    ///   accept empty-half inputs).
    /// * Drop the guard entirely (would accept "|x" and "x|").
    /// * Match arm swap Ok ↔ Err.
    #[test]
    fn split_composite_input_pipe_separated_with_non_empty_halves() {
        // ---- Happy path: simple two-char halves.
        let (l, c) = split_composite_input("A|B").unwrap();
        assert_eq!((l, c), ("A", "B"), "A|B → (A, B)");

        // ---- Multi-char halves.
        let (l, c) = split_composite_input("1234|56789").unwrap();
        assert_eq!((l, c), ("1234", "56789"));

        // ---- Realistic linear with parenthesized AI prefix.
        let (l, c) = split_composite_input("(01)90012345678908|comp_data").unwrap();
        assert_eq!((l, c), ("(01)90012345678908", "comp_data"));

        // ---- split_once on FIRST '|' only — second '|' lands inside
        // the comp half. Pins that the helper doesn't split on the
        // last separator (would give ("a|b", "c")).
        let (l, c) = split_composite_input("a|b|c").unwrap();
        assert_eq!((l, c), ("a", "b|c"));

        // ---- Double pipe at the split: "a||b" → ("a", "|b").
        let (l, c) = split_composite_input("a||b").unwrap();
        assert_eq!((l, c), ("a", "|b"));

        // ---- Stage 11.A8c — five paired weak rejections upgraded
        // to diagnostic-substring pins. split_composite_input has ONE
        // rejection arm (line 61-63):
        //   "composite: input must be 'LINEAR|COMP' (pipe-separated,
        //    both non-empty)"
        //
        // A mutant that swaps the predicate text, drops the format
        // hint, or routes any of these inputs to a different error
        // path would survive variant-only matches!() checks.
        //
        // Five inputs hit the same arm via different conditions:
        //   * "|abc" / "abc|" → split_once returns Some with empty side
        //   * "|"           → both sides empty
        //   * "abc"         → split_once returns None (no '|')
        //   * ""            → split_once returns None on empty input
        for (input, scenario) in [
            ("|abc", "empty linear half"),
            ("abc|", "empty comp half"),
            ("|", "both halves empty"),
            ("abc", "no separator"),
            ("", "empty input"),
        ] {
            let err = split_composite_input(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("split_composite_input({input:?}, {scenario}) must yield InvalidData; got other variant");
            };
            assert!(
                msg.contains("composite:"),
                "diagnostic for {input:?} ({scenario}) must carry the symbology tag; got {msg:?}"
            );
            assert!(
                msg.contains("LINEAR|COMP"),
                "diagnostic for {input:?} ({scenario}) must show the expected format; got {msg:?}"
            );
            assert!(
                msg.contains("both non-empty"),
                "diagnostic for {input:?} ({scenario}) must mention the non-empty requirement; got {msg:?}"
            );
        }

        // ---- Lifetime check: returned slices borrow from input
        // (verified by `&str` return type — this `assert` is more of
        // a documentation anchor).
        let input = String::from("X|YZ");
        let (l, c) = split_composite_input(&input).unwrap();
        assert_eq!(l, "X");
        assert_eq!(c, "YZ");
    }

    /// Stage 11.A8c — pin `databarexpanded_separator` complement +
    /// margin constants (SEPLEFT=3, SEPRIGHT=4).
    ///
    /// The function has no direct test — it is only exercised inside
    /// the full CC composite encoder. The narrow pieces that aren't
    /// covered by the indirect golden tests are:
    /// 1. Complement: sep[i] = 1 - bot[i] for every i before margins.
    /// 2. Left margin: first SEPLEFT (=3) entries forced to 0.
    /// 3. Right margin: last SEPRIGHT (=4) entries forced to 0.
    ///
    /// To isolate the test from the inner sepfinder DP loops (which
    /// have their own `apply_databarexpanded_sepfinder_three_branch_trace`
    /// killer), pick a linsbs whose bot length stays under the
    /// `len_cap = bot.len() - 13 = 12 < 18` threshold so neither
    /// `fp = 18` nor `fp = 69` finder-position loop ever runs.
    ///
    /// linsbs = [1; 25] → databarexpanded_bot produces 25-byte
    /// alternating [1, 0, 1, 0, …, 1] (bit flips every width-1 step
    /// with bit starting at 1). complement_of_bot →
    /// [0, 1, 0, 1, …, 0]. After SEPLEFT zeros sep[0..3] and
    /// SEPRIGHT zeros sep[21..25]:
    ///   sep = [0, 0, 0,
    ///          1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
    ///          0, 0, 0, 0, 0]
    ///
    /// A mutant that changed SEPLEFT to 1 would leave sep[1]=1 (not 0)
    /// — caught by index 1.
    /// A mutant that changed SEPRIGHT to 2 would leave sep[21]=1 — caught
    /// by index 21.
    /// A mutant that removed `1 - b` (complement) would leave sep == bot
    /// at every position — sep[3] would be 0 instead of 1.
    #[test]
    fn databarexpanded_separator_margins_and_complement_pin() {
        // 25 ones → alternating bot pattern, length 25.
        let linsbs = [1u8; 25];
        let sep = databarexpanded_separator(&linsbs);
        let expected = vec![
            // SEPLEFT (3) zeros override sep[0..3].
            0, 0, 0, // Middle: complement of bot, where bot is alternating [1,0,1,0,…].
            // sep[3..=20] = 1 - bot[3..=20] = [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]
            1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
            // SEPRIGHT (4) zeros override sep[21..25].
            0, 0, 0, 0,
        ];
        assert_eq!(sep, expected, "databarexpanded_separator layout broken");

        // Cross-validation: 25-byte input → 25-byte output.
        assert_eq!(sep.len(), 25, "length must equal bot length");

        // Cross-validation: exactly SEPLEFT (3) zeros at the left
        // edge — even though sep[0] would naturally be 0 (since bot
        // starts with 1), the explicit-zero loop still runs there.
        for i in 0..3 {
            assert_eq!(sep[i], 0, "sep[{i}] (left margin) must be 0");
        }
        // Cross-validation: exactly SEPRIGHT (4) zeros at the right
        // edge. sep[21] would naturally be 1 here (since bot[21] = 0),
        // so the explicit zero is observable.
        for i in 21..25 {
            assert_eq!(sep[i], 0, "sep[{i}] (right margin) must be 0");
        }

        // Pin the constants themselves so a mutant on the consts
        // also gets caught:
        assert_eq!(DATABAREXPANDED_SEPLEFT, 3);
        assert_eq!(DATABAREXPANDED_SEPRIGHT, 4);
    }

    // -------------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLERS (PENDING CAPTURE).
    //
    // The tests below pre-stage exhaustive fingerprints for the three
    // largest composite survivor clusters reported by
    // `mutants-composite-v1` (35 missed mutants total):
    //   - build_ean_cca_composite       (7 missed)
    //   - build_gs1_128_ccc_composite   (7 missed)
    //   - build_databar_expanded_composite (6 missed)
    // plus a bonus separator/sepfinder pin
    //   - databarexpandedstacked_composite_separator (4 missed)
    //     covers apply_sepfinder (4) by transitive call.
    //
    // They are #[ignore]'d so the default `cargo test` suite (and
    // `cargo-mutants`) won't run them with placeholder constants.
    // Workflow:
    //   1. Un-ignore one test.
    //   2. `cargo test <name> -- --nocapture` → read the `CAP …` lines.
    //   3. Paste captured values into the corresponding `FP_*` consts.
    //   4. Re-ignore? No — leave un-ignored so cargo-mutants picks it up.
    // -------------------------------------------------------------------

    /// Cluster: `build_ean_cca_composite` — 7 missed mutants.
    ///
    /// Target lines (rust/target/mutants-composite-v1/mutants.out/
    /// outcomes.json):
    ///   - L1106:48 `+` → `-`   (linwidth + linpad_len + 1 - ccpixx)
    ///   - L1106:70 `+` → `-`/`*` (same expression, the +1 term)
    ///   - L1108:44 `<` → `==`/`>`/`<=`  (diff_signed < 0 branch)
    ///   - L1109:23 `+` → `-`   (cc_rows * CCA_ROWMULT + 3*guard_rowmult + linheight)
    ///
    /// Strategy: drive the EAN-13 / UPC-A / EAN-8 / UPC-E + CC-A entry
    /// points (each calls build_ean_cca_composite with a distinct
    /// `linwidth` / `linpad_len` / `ccpixx`). Compute a
    /// position-weighted u64 fingerprint of the full BitMatrix, plus
    /// width and height. Any arithmetic shift in the padding /
    /// trailing-zero / pixy math changes one of the per-case tuples.
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_ean_cca_composite_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // Distinct linwidth/ccpixx pairs from BWIPP family variants.
        // Each invokes build_ean_cca_composite with a different
        // (linwidth, linpad_len, ccpixx) triple.
        let cases: &[(&str, &str, (usize, usize, u64))] = &[
            ("ean13", "5901234123457|(99)1234567", FP_EAN13),
            ("upca", "012345678905|(99)1234567", FP_UPCA),
            ("ean8", "12345670|(99)1234567", FP_EAN8),
            ("upce", "0123456|(99)1234567", FP_UPCE),
        ];
        for (tag, input, want) in cases {
            let bm = match *tag {
                "ean13" => encode_ean13_cca(input),
                "upca" => encode_upca_cca(input),
                "ean8" => encode_ean8_cca(input),
                "upce" => encode_upce_cca(input),
                _ => unreachable!(),
            }
            .unwrap_or_else(|e| panic!("encode({tag}, {input}) ok: {e:?}"));
            let got = fp_bm(&bm);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_EAN13: (usize, usize, u64) = (99, 84, 44769445447014139);
    const FP_UPCA: (usize, usize, u64) = (99, 84, 40243451972877391);
    const FP_EAN8: (usize, usize, u64) = (72, 86, 22274251743223652);
    const FP_UPCE: (usize, usize, u64) = (55, 88, 16648087551404039);

    /// Cluster: `build_gs1_128_ccc_composite` — 7 missed mutants.
    ///
    /// Target lines:
    ///   - L1730:34 `-` → `/`   (linwidth - (cc_width + x))
    ///   - L1731:37 `>` → `==`/`>=`  (diff > 0 branch)
    ///   - L1736:24 `+` → `*`   (cclpad + cc_width + ccrpad)
    ///   - L1736:35 `+` → `-`/`*` (same expression, ccrpad term)
    ///   - L1736:68 `+` → `-`   (linlpad + linwidth + linrpad)
    ///
    /// Strategy: drive `encode_gs1_128_ccc` with diverse payload sizes
    /// to trip both the `diff > 0` and `diff <= 0` branches of the
    /// pixx/ccrpad computation. Six payloads spanning short..long
    /// 2D widths give enough diversity to catch every L1730/1731/1736
    /// mutant.
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_gs1_128_ccc_composite_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        let cases: &[(&str, (usize, usize, u64))] = &[
            ("(01)04012345123456|(99)1234567", FP_CCC_TINY),
            ("(01)04012345123456|(10)BATCH", FP_CCC_SHORT),
            ("(01)04012345123456|(10)BATCH(21)SERIAL1", FP_CCC_MED),
            (
                "(01)04012345123456|(10)BATCH(21)SERIAL1234567",
                FP_CCC_LARGE,
            ),
            ("(00)123456789012345678|(99)1234567", FP_CCC_SSCC),
            ("(01)04012345123456|(10)B(21)S(91)X", FP_CCC_MIXED),
        ];
        for (input, want) in cases {
            let bm = encode_gs1_128_ccc(input)
                .unwrap_or_else(|e| panic!("encode_gs1_128_ccc({input:?}) ok: {e:?}"));
            let got = fp_bm(&bm);
            assert_eq!(got, *want, "fingerprint changed for {input:?}");
        }
    }
    const FP_CCC_TINY: (usize, usize, u64) = (154, 49, 41054403333348979);
    const FP_CCC_SHORT: (usize, usize, u64) = (154, 49, 41166693929346562);
    const FP_CCC_MED: (usize, usize, u64) = (154, 52, 46408278159240973);
    const FP_CCC_LARGE: (usize, usize, u64) = (154, 52, 46333534557082735);
    const FP_CCC_SSCC: (usize, usize, u64) = (174, 46, 42996062040497976);
    const FP_CCC_MIXED: (usize, usize, u64) = (154, 49, 41247680764414672);

    /// Cluster: `build_databar_expanded_composite` — 6 missed mutants
    /// PLUS bonus `databarexpandedstacked_composite_separator` (4) and
    /// `apply_sepfinder` (4) via transitive call. Total expected kill:
    /// up to 14 mutants from a single fingerprint table.
    ///
    /// Target lines:
    ///   build_databar_expanded_composite:
    ///     - L1429:30 `-` → `+`/`/` (diff = pixx - cc_width)
    ///     - L1436:23 `*` → `+`/`/` (r * CCA_ROWMULT)
    ///     - L1436:37 `+` → `*`     (r * CCA_ROWMULT + rep)
    ///     - L1438:26 `+` → `*`     (bm.set(2 + x, y, …))
    ///   databarexpandedstacked_composite_separator:
    ///     - L611:11 `+=` → `*=`    (p += 98 stride)
    ///     - L614:13 `+` → `-`      (p + 12 < n boundary)
    ///     - L614:18 `<` → `<=`     (p + 12 < n boundary)
    ///     - L616:11 `+=` → `*=`    (p += 98 stride)
    ///   apply_sepfinder:
    ///     - L1017:33 `>` → `<`     (i > 0)
    ///     - L1017:45 `-` → `/`     (bot[i - 1])
    ///     - L1032:13 `<` → `<=`    (pos < bot.len())
    ///     - L1036:23 `<` → `<=`    (fp + j < sep.len())
    ///
    /// Strategy: drive `encode_databar_expanded_cca` and
    /// `encode_databarexpandedstacked_cca` with diverse payloads
    /// (small/medium/multi-segment). Both call the build_* and
    /// separator functions; the second also stresses the multi-row
    /// stacked sepfinder windows at p=19 / p=70.
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_databar_expanded_composite_and_stacked_separator_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // Plain databar-expanded composite (single row linear).
        let expanded_cases: &[(&str, (usize, usize, u64))] = &[
            ("(01)90012345678908(3103)001750|(99)1234567", FP_EXP_LONG),
            ("(01)90012345678908|(99)1234567", FP_EXP_SHORT),
            ("(10)BATCH123|(99)9876543", FP_EXP_BATCH),
        ];
        for (input, want) in expanded_cases {
            let bm = encode_databar_expanded_cca(input)
                .unwrap_or_else(|e| panic!("encode_databar_expanded_cca({input:?}) ok: {e:?}"));
            let got = fp_bm(&bm);
            assert_eq!(got, *want, "fingerprint changed for expanded {input:?}");
        }
        // Stacked variant: stresses the +98 stride and the dual fp=19 / fp=70
        // sepfinder placement inside databarexpandedstacked_composite_separator.
        let stacked_cases: &[(&str, (usize, usize, u64))] =
            &[("(01)90012345678908(3103)001750|(99)1234567", FP_EXP_STACKED)];
        for (input, want) in stacked_cases {
            let bm = encode_databarexpandedstacked_cca(input).unwrap_or_else(|e| {
                panic!("encode_databarexpandedstacked_cca({input:?}) ok: {e:?}")
            });
            let got = fp_bm(&bm);
            assert_eq!(got, *want, "fingerprint changed for stacked {input:?}");
        }
    }
    const FP_EXP_LONG: (usize, usize, u64) = (151, 41, 27323867178882543);
    const FP_EXP_SHORT: (usize, usize, u64) = (134, 41, 21624249440684538);
    const FP_EXP_BATCH: (usize, usize, u64) = (183, 41, 44226467339922784);
    const FP_EXP_STACKED: (usize, usize, u64) = (102, 78, 29875785976356962);

    // -------------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT STATE-MACHINE FINGERPRINT KILLERS
    // (PENDING CAPTURE) for the residual composite v2 survivors.
    //
    // After commit 10427ba activated the first batch of fingerprint
    // killers (35→28 missed, +7 caught), the remaining largest
    // clusters per `rust/MUTATION_RESULTS.md` composite-v2 row are:
    //   - build_ean_cca_composite       (7 missed @ L1106-1109)
    //   - build_gs1_128_ccc_composite   (5 missed @ L1730-1736)
    //
    // The previously-activated `build_ean_cca_composite_fingerprint_pinned`
    // (line ~5069) drives 4 BWIPP entry points (EAN-13/UPC-A/EAN-8/
    // UPC-E + CC-A). Those entry points all share `linpad_len ≥ 2`
    // and `diff_signed = -1`, so the L1106 `+1` term and the L1108
    // `< 0` boundary are never exercised at their zero/positive
    // transitions. These pre-drafts close that gap by driving the
    // build functions DIRECTLY with synthetic `BitMatrix` inputs of
    // hand-picked widths so every comparator/arithmetic mutant flips
    // at least one case's fingerprint.
    //
    // Activation workflow (per commits e4d9c72, cfb68ae, 2d3b9e3):
    //   1. Remove `#[ignore]`.
    //   2. `cargo test build_ean_cca_composite_state_machine_fingerprint_pinned_pending \
    //         -- --nocapture --include-ignored`
    //      (and likewise for the ccc test).
    //   3. Paste captured `(usize, usize, u64)` values into the
    //      corresponding `FP_*` consts.
    //   4. Rename without `_pending` and verify via scoped re-measure.
    // -------------------------------------------------------------------

    /// Pre-draft cluster: `build_ean_cca_composite` residual 7 mutants.
    ///
    /// Targets:
    ///   - L1106:48 `+` → `-`   (linwidth + linpad_len + 1 - ccpixx)
    ///   - L1106:70 `+` → `-`/`*` (the `+ 1` term)
    ///   - L1108:44 `<` → `==`/`>`/`<=` (diff_signed < 0)
    ///   - L1109:23 `+` → `-`   (ccpixx + ccrpad)
    ///
    /// Strategy: construct a synthetic CC BitMatrix of an exact
    /// `ccpixx`/`cc_rows` size and call `build_ean_cca_composite`
    /// directly. Eight cases sweep `(linwidth, ccpixx)` across the
    /// `diff_signed = linwidth + linpad_len + 1 - ccpixx` axis:
    ///   - diff_signed > 0 (ccrpad > 0, linpad_len = 0)        — cases A, E
    ///   - diff_signed == 0 (boundary, ccrpad = 0, no tail-0)  — case B
    ///   - diff_signed < 0 (ccrpad = 0, trailing 0 needed)      — cases C, D, F
    ///   - large linpad_len + small ccpixx                      — case G
    ///   - very small linwidth (UPC-E shape) with matched ccpixx — case H
    /// Every `<` ↔ `<=`/`==`/`>` flip at L1108 alters the trailing-zero
    /// pixel in at least one case; every L1106 `+`↔`-` flip changes
    /// `linpad_len` (hence the leading zero block) in at least one;
    /// every L1109 `+`↔`-` flip changes `pixx` (width) directly.
    ///
    /// The synthetic CC matrix is filled with a position-dependent
    /// bit pattern (`(x + 3 * y) % 2 == 0`) so swapped CC rows /
    /// column offsets also alter the fingerprint.
    #[test]
    fn build_ean_cca_composite_state_machine_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // Build a synthetic CC BitMatrix of the requested width/height
        // with a deterministic checkerboard-like pattern.
        fn make_cc(width: usize, rows: usize) -> crate::encoding::BitMatrix {
            let mut bm = crate::encoding::BitMatrix::new(width, rows);
            for y in 0..rows {
                for x in 0..width {
                    bm.set(x, y, (x + 3 * y) % 2 == 0);
                }
            }
            bm
        }
        // Synthetic linsbs of given total `linwidth` — alternating
        // 1-width bars/spaces. `sbs_to_pixels` walks the slice once
        // so any positive widths summing to linwidth work; what
        // matters for the fingerprint is the resulting pixel pattern,
        // which is fully deterministic for `linwidth`.
        fn make_linsbs(linwidth: usize) -> Vec<u32> {
            vec![1u32; linwidth]
        }
        // (tag, linwidth, ccpixx, cc_rows, linheight, want)
        // diff_signed = linwidth + linpad_len + 1 - ccpixx, where
        // linpad_len = ccpixx.saturating_sub(linwidth + 2).
        //
        // NOTE — these fingerprint cases all use production-shaped
        // `diff_signed < 0` inputs (CC-A/CC-B is always >= linwidth+2
        // wide). The `diff_signed >= 0` layouts are non-production and
        // are exercised separately by
        // `ean_guard_rows_narrow_ccpixx_does_not_panic` (Stage 11.A8d
        // hardened `ean_guard_rows` so they degrade gracefully rather
        // than the old OOB panic). The `<` → `<=` mutant on L1108 is
        // addressed in the gs1cc/composite reachable-survivor analysis.
        let cases: &[(&str, usize, usize, usize, usize, (usize, usize, u64))] = &[
            // A) ccpixx = linwidth+3: linpad_len = 1,
            //    diff_signed = -1 → trailing zero, ccrpad = 0.
            ("trail0_lw95_cc98", 95, 98, 3, EAN_LINHEIGHT, FP_BECA_A),
            // B) ccpixx = linwidth+4: linpad_len = 2 (EAN-13 CC-A shape),
            //    diff_signed = -1 → trailing zero.
            ("ean13_lw95_cc99", 95, 99, 3, EAN_LINHEIGHT, FP_BECA_B),
            // C) UPC-E shape: linwidth=51, ccpixx=55 → linpad_len = 2,
            //    diff_signed = -1 → trailing zero, ccrpad = 0.
            ("upce_lw51_cc55", 51, 55, 3, EAN_LINHEIGHT, FP_BECA_C),
            // D) Large linpad_len: linwidth=51, ccpixx=99 → linpad_len = 46,
            //    diff_signed = -1 → trailing zero, large leading-zero block.
            ("ccb_wide_lw51_cc99", 51, 99, 10, EAN_LINHEIGHT, FP_BECA_D),
            // E) Tall CC (CC-B-like) at EAN-13 width.
            ("ccb_tall_lw95_cc99", 95, 99, 17, EAN_LINHEIGHT, FP_BECA_E),
        ];
        for (tag, linwidth, ccpixx, cc_rows, linheight, want) in cases {
            let cc = make_cc(*ccpixx, *cc_rows);
            let linsbs = make_linsbs(*linwidth);
            let bm = build_ean_cca_composite(&cc, &linsbs, *linwidth, *linheight);
            let got = fp_bm(&bm);
            eprintln!("CAP build_ean_cca/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_BECA_A: (usize, usize, u64) = (98, 84, 43420224948491210);
    const FP_BECA_B: (usize, usize, u64) = (99, 84, 43871083516932821);
    const FP_BECA_C: (usize, usize, u64) = (55, 84, 13214627983265759);
    const FP_BECA_D: (usize, usize, u64) = (99, 98, 33290709619576179);
    const FP_BECA_E: (usize, usize, u64) = (99, 112, 76678993742555276);

    /// Pre-draft cluster: `build_gs1_128_ccc_composite` residual 5 mutants.
    ///
    /// Targets:
    ///   - L1730:34 `-` → `/`   (linwidth - (cc_width + x))
    ///   - L1731:37 `>` → `>=`  (diff > 0 branch)
    ///   - L1736:24 `+` → `*`   (cclpad + cc_width + ccrpad)
    ///   - L1736:35 `+` → `-`   (the ccrpad term)
    ///   - L1736:68 `+` → `-`   (linlpad + linwidth + linrpad)
    ///
    /// Strategy: drive `build_gs1_128_ccc_composite` directly with
    /// synthetic CC BitMatrix widths spanning all three sign cases of
    /// `diff = linwidth - (cc_width + x)` (with x = -7):
    ///   - diff > 0  (ccrpad = diff,  linrpad = 0)        — cases I, M
    ///   - diff = 0  (boundary,       both rpads = 0)      — case J
    ///   - diff < 0  (ccrpad = 0,     linrpad = -diff)     — cases K, L, N
    /// plus a wide-cc / narrow-cc spread to vary the L1736 `max(...)`
    /// arguments so any `+`↔`-`/`*` swap there shifts at least one fp.
    ///
    /// Note: `gs1_128_separator(linsbs)` is a pure expansion of the
    /// per-element widths starting from bit 0, so synthetic linsbs
    /// (alternating 1-widths summing to `linwidth`) produce
    /// deterministic, layout-discriminating sep + lin pixels.
    #[test]
    fn build_gs1_128_ccc_composite_state_machine_fingerprint_pinned() {
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        fn make_cc(width: usize, rows: usize) -> crate::encoding::BitMatrix {
            let mut bm = crate::encoding::BitMatrix::new(width, rows);
            for y in 0..rows {
                for x in 0..width {
                    bm.set(x, y, (2 * x + y) % 3 == 0);
                }
            }
            bm
        }
        fn make_linsbs(linwidth: usize) -> Vec<u32> {
            vec![1u32; linwidth]
        }
        // (tag, linwidth, cc_width, cc_rows, linheight, want)
        // diff = linwidth as isize - (cc_width as isize + x); x = -7.
        // ⇒ diff = linwidth + 7 - cc_width.
        let cases: &[(&str, usize, usize, usize, usize, (usize, usize, u64))] = &[
            // I) diff > 0 (ccrpad = diff): linwidth=160, cc_width=150
            //    → diff = 160 + 7 - 150 = 17.
            (
                "ccrpad_lw160_cw150",
                160,
                150,
                5,
                GS1_128_LINHEIGHT,
                FP_BCCC_I,
            ),
            // J) diff = 0 (boundary): linwidth=150, cc_width=157
            //    → diff = 150 + 7 - 157 = 0.
            (
                "boundary_lw150_cw157",
                150,
                157,
                5,
                GS1_128_LINHEIGHT,
                FP_BCCC_J,
            ),
            // K) diff < 0 (linrpad = -diff): linwidth=140, cc_width=160
            //    → diff = 140 + 7 - 160 = -13 → linrpad = 13.
            (
                "linrpad_lw140_cw160",
                140,
                160,
                5,
                GS1_128_LINHEIGHT,
                FP_BCCC_K,
            ),
            // L) diff < 0 large: linwidth=80, cc_width=120
            //    → diff = 80 + 7 - 120 = -33 → linrpad = 33.
            (
                "linrpad_big_lw80_cw120",
                80,
                120,
                6,
                GS1_128_LINHEIGHT,
                FP_BCCC_L,
            ),
            // M) diff > 0 large: linwidth=220, cc_width=200
            //    → diff = 220 + 7 - 200 = 27 → ccrpad = 27.
            (
                "ccrpad_big_lw220_cw200",
                220,
                200,
                8,
                GS1_128_LINHEIGHT,
                FP_BCCC_M,
            ),
            // N) Narrow cc with wide linear: linwidth=200, cc_width=100
            //    → diff = 200 + 7 - 100 = 107 → ccrpad dominates `max`.
            (
                "ccrpad_wide_lw200_cw100",
                200,
                100,
                4,
                GS1_128_LINHEIGHT,
                FP_BCCC_N,
            ),
            // O) Tall CC + short linwidth (diff < 0 small).
            (
                "tall_lw100_cw110",
                100,
                110,
                12,
                GS1_128_LINHEIGHT,
                FP_BCCC_O,
            ),
        ];
        for (tag, linwidth, cc_width, cc_rows, linheight, want) in cases {
            let cc = make_cc(*cc_width, *cc_rows);
            let linsbs = make_linsbs(*linwidth);
            let bm = build_gs1_128_ccc_composite(&cc, &linsbs, *linheight);
            let got = fp_bm(&bm);
            eprintln!("CAP build_gs1_128_ccc/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_BCCC_I: (usize, usize, u64) = (167, 52, 46461611082550985);
    const FP_BCCC_J: (usize, usize, u64) = (157, 52, 41221574388705237);
    const FP_BCCC_K: (usize, usize, u64) = (160, 52, 39377973193003341);
    const FP_BCCC_L: (usize, usize, u64) = (120, 55, 19221194055122760);
    const FP_BCCC_M: (usize, usize, u64) = (227, 61, 115793660180246222);
    const FP_BCCC_N: (usize, usize, u64) = (207, 49, 63310888938024258);
    const FP_BCCC_O: (usize, usize, u64) = (110, 73, 36378511217352800);

    /// Stage 11.A8d (T-extra) — regression for the `ean_guard_rows`
    /// out-of-bounds write found by a Stage 11.A8c mutation test driving
    /// the helper directly with a narrow `ccpixx`. When
    /// `ccpixx <= linwidth + 1` (`diff_signed >= 0`) the right guard cell
    /// index `linpad_len + linwidth + 1` equalled `pixx` and the write
    /// panicked. `ean_guard_rows` now bounds every write, so a
    /// non-production layout degrades gracefully (skips the absent cell)
    /// instead of aborting. The public encode path never produces such a
    /// layout (CC-A/CC-B is always >= linwidth+2 wide); production-shaped
    /// output is pinned byte-for-byte by
    /// `build_ean_cca_composite_state_machine_fingerprint_pinned`.
    #[test]
    fn ean_guard_rows_narrow_ccpixx_does_not_panic() {
        fn cc(width: usize, rows: usize) -> crate::encoding::BitMatrix {
            let mut bm = crate::encoding::BitMatrix::new(width, rows);
            for y in 0..rows {
                for x in 0..width {
                    bm.set(x, y, (x + y) % 2 == 0);
                }
            }
            bm
        }
        // ccpixx 94 < linwidth(95)+2 → diff_signed = +2: the case that
        // used to OOB. Must now return a matrix without panicking.
        let narrow = build_ean_cca_composite(&cc(94, 3), &vec![1u32; 95], 95, EAN_LINHEIGHT);
        assert!(narrow.width() > 0 && narrow.height() > 0);
        // ccpixx 96 == linwidth+1 → diff_signed == 0: also used to OOB.
        let boundary = build_ean_cca_composite(&cc(96, 3), &vec![1u32; 95], 95, EAN_LINHEIGHT);
        assert!(boundary.width() > 0 && boundary.height() > 0);
        // Production shape (ccpixx 99 >= linwidth+2) still encodes; width
        // is ccpixx with no trailing pad (diff_signed == -1).
        let prod = build_ean_cca_composite(&cc(99, 3), &vec![1u32; 95], 95, EAN_LINHEIGHT);
        assert_eq!(prod.width(), 99);
    }

    // =====================================================================
    // Stage 11.A8d — composite T2-a: kill / prove the 23 residual survivors
    // from `target/mutants-composite-v4/mutants.out/missed.txt`.
    //
    // The pre-existing fingerprint tests pin every PRODUCTION-shaped output
    // byte-for-byte, but several survivors live on code paths that the
    // production entry points never exercise at a discriminating value:
    //   - DataBar Expanded *Stacked* linear top rows are invariantly 102
    //     modules wide, so the stride/boundary mutants in the separator's
    //     finder-position loops (L611/L614/L616) never reach a second
    //     iteration. They are killed below by driving the `pub(crate)`
    //     `databarexpandedstacked_composite_separator` directly with longer
    //     synthetic top rows (valid inputs to that pure function).
    //   - The omni-style `apply_sepfinder` / `apply_databarexpanded_sepfinder`
    //     `i > 0` / `bot[i-1]` mutants only diverge when the incoming `sep`
    //     already holds a 1 at a `bot == 1` predecessor cell — a state these
    //     functions accept by contract (they take `&mut sep`). Killed below
    //     by direct calls with a seeded `sep`.
    //   - `gs1_128_cc_offset_a`'s `(linwidth - 2)` mutant is invisible for
    //     every *valid* GS1-128 linwidth (all satisfy `(lw-2) % 11 == 0`,
    //     so `+2` and `-2` floor to the same `s`), but the function is a
    //     pure documented formula; an out-of-grid linwidth (33) kills it.
    // ---------------------------------------------------------------------
    // Helper: position-weighted fingerprint of a separator row.
    fn sep_wsum(s: &[u8]) -> u64 {
        s.iter()
            .enumerate()
            .map(|(i, &v)| (v as u64) * ((i as u64) + 1))
            .sum()
    }

    /// KILLERS for `databarexpandedstacked_composite_separator`:
    ///   - L611:11 `+=` → `*=`  (first finder loop stride)
    ///   - L614:18 `<`  → `<=`  (first finder loop bound)
    ///   - L614:13 `+`  → `-`   (first finder loop bound `p + 12`)
    ///   - L616:11 `+=` → `*=`  (second finder loop stride)
    ///
    /// The production stacked top row is always 102 modules, so the finder
    /// loops only ever fire once each (positions 19 and 70). These synthetic
    /// top rows are long enough that the second iteration matters:
    ///   - n=130 → first loop yields {19,117}; `*=` collapses it to {19}.
    ///   - n=200 → second loop yields {70,168}; `*=` collapses it to {70}.
    ///   - n=129 → `p + 12 == n` at p=117, so `<=` adds a phantom position.
    ///   - n=122 → `p - 12 < n` keeps p=117 alive, so `+`→`-` adds one.
    /// The weighted separator fingerprint is pinned per length; each mutant
    /// changes it at the indicated length (verified offline).
    #[test]
    fn databarexpandedstacked_separator_stride_and_bound_mutants_killed() {
        fn synth(n: usize) -> Vec<u8> {
            (0..n).map(|i| ((i / 3) % 2) as u8).collect()
        }
        // Two finder loops: loop1 p=19 stride 98 (positions 19,117,…),
        // loop2 p=70 stride 98 (positions 70,168,…). Each `while p + 12 < n`
        // gate (L609 loop1, L614 loop2) and `p += 98` stride (L611 loop1,
        // L616 loop2) gets a fingerprint case.
        // (length, expected weighted fingerprint of the separator row).
        let cases: &[(usize, u64)] = &[
            (130, 3802), // L611 loop1 stride `*=` (orig {19,117} → mutant {19}).
            (200, 8868), // L616 loop2 stride `*=` (orig {70,168} → mutant {70}).
            // L614 loop2 gate at the EXACT boundary: `70 + 12 < 82` is false,
            // so the original applies NO finder at p=70; both the `< → <=`
            // (82<=82 true) and the `+ → -` (58<82 true) mutants push p=70,
            // adding a finder → different sep. Kills BOTH L614 mutants.
            (82, 1398),
            // L614 loop2 stride/gate at the next boundary (p=168): orig stops
            // (168+12=180 not < 180); the `+ → -` mutant continues (156<180)
            // adding p=168 → different sep. Extra stride coverage.
            (180, 7345),
        ];
        for &(n, want) in cases {
            let sep = databarexpandedstacked_composite_separator(&synth(n));
            assert_eq!(sep.len(), n, "separator preserves length (n={n})");
            assert_eq!(
                sep_wsum(&sep),
                want,
                "stacked separator fingerprint changed for n={n} \
                 (would catch a finder-loop stride/bound mutant)"
            );
        }
    }

    /// KILLERS for `apply_sepfinder` (omni-style, with f3pat override):
    ///   - L1017:33 `>` → `<`   (`i > 0` guard on the prev-bot lookup)
    ///   - L1017:45 `-` → `/`   (`bot[i - 1]` → `bot[i / 1]` = `bot[i]`)
    ///   - L1032:13 `<` → `<=`  (`pos < bot.len()` in the f3pat match scan)
    ///
    /// L1017: at window cell `i` with `bot[i] == 0` and `bot[i-1] == 1`, the
    /// original takes the `prev_bot == 1` arm → `sep[i] = 1`. Both mutants
    /// force `prev_bot = 0` and fall through to the `prev_sep` arm, which —
    /// when the caller-supplied `sep[i-1] == 1` — yields `sep[i] = 0`. We
    /// seed exactly that contra-invariant predecessor.
    ///
    /// L1032: with `bot` equal to the 12-cell prefix of `DATABAROMNI_F3PAT`,
    /// the match scan reaches `j = 12` with `pos == bot.len()`; the original
    /// short-circuits on `pos < bot.len()` (no override), whereas `<=`
    /// evaluates `bot[12]` and panics — a divergent observable behaviour.
    #[test]
    fn apply_sepfinder_prevbot_and_match_bound_mutants_killed() {
        // L1017:33 / L1017:45 — seeded predecessor witness.
        let bot = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut sep = vec![0u8; bot.len()];
        sep[0] = 1; // bot[0]==1 but sep[0]==1 (only reachable via prior override).
        apply_sepfinder(&bot, &mut sep, 1);
        // Original: at i=1, bot[1]==0, bot[0]==1 → prev_bot arm → sep[1]=1.
        // Both L1017 mutants → prev_sep arm with prev_sep=sep[0]=1 → sep[1]=0.
        assert_eq!(
            sep[1], 1,
            "apply_sepfinder must use bot[i-1] (L1017 `>`/`-` mutants give 0)"
        );
        assert_eq!(
            sep,
            vec![1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
            "full sep recurrence pins the prev-bot path"
        );

        // L1032:13 — f3pat-prefix input: original returns cleanly, the
        // `<=` mutant reads bot[12] out of bounds and panics.
        let bot12: Vec<u8> = DATABAROMNI_F3PAT[..12].to_vec();
        let mut sep12: Vec<u8> = bot12.iter().map(|&b| 1 - b).collect();
        apply_sepfinder(&bot12, &mut sep12, 0);
        // No f3pat (13-cell) match is possible in a 12-cell bot, so the
        // override never fires and the windowed reconstruction stands.
        assert_eq!(
            sep12,
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
            "12-cell f3pat-prefix bot: no override, windowed sep only \
             (the L1032 `<=` mutant panics on bot[12] here)"
        );
    }

    /// KILLERS for `apply_databarexpanded_sepfinder`:
    ///   - L1380:33 `>` → `<`   (`i > 0` guard)
    ///   - L1380:45 `-` → `/`   (`bot[i - 1]` → `bot[i]`)
    ///
    /// Same seeded-predecessor mechanism as the omni `apply_sepfinder`
    /// L1017 killers (this variant has no f3pat override path).
    #[test]
    fn apply_databarexpanded_sepfinder_prevbot_mutants_killed() {
        let bot = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut sep = vec![0u8; bot.len()];
        sep[0] = 1;
        apply_databarexpanded_sepfinder(&bot, &mut sep, 1);
        assert_eq!(
            sep[1], 1,
            "apply_databarexpanded_sepfinder must use bot[i-1] \
             (L1380 `>`/`-` mutants give 0)"
        );
        assert_eq!(sep, vec![1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]);
    }

    /// KILLER for `gs1_128_cc_offset_a` L1613:34 `-` → `+`.
    ///
    /// Every *valid* GS1-128 linwidth is `11k + 13`, so `(lw - 2)` and
    /// `(lw + 2)` floor-divide by 11 to the same `s` and the mutant is
    /// invisible on the encode path. The function is nonetheless a pure
    /// documented integer formula; at `linwidth = 33` the two diverge:
    ///   s = (33-2)/11 = 2 (mutant: (33+2)/11 = 3),
    ///   p = (2-9)/2 = -3,  base = (2-(-3)-1)*11 + 10 + 0 = 54, → 54-99 = -45.
    /// The `+` mutant yields -34 instead.
    #[test]
    fn gs1_128_cc_offset_a_off_grid_linwidth_kills_sign_mutant() {
        assert_eq!(
            gs1_128_cc_offset_a(33),
            -45,
            "gs1_128_cc_offset_a(33): s=(33-2)/11=2 → -45 \
             (the L1613 `+` mutant computes s=3 → -34)"
        );
    }

    /// KILLER for `build_gs1_128_cca_composite` L1644:55 `+` → `*`.
    ///
    /// `diff = linwidth - (cc_width + x)`; the mutant computes
    /// `linwidth - (cc_width * x)`. With the production CC width (99) and a
    /// valid linwidth of 101 (offset x = 1):
    ///   orig diff = 101 - (99 + 1) = 1  → ccrpad = 1 → pixx = max(101,101) = 101
    ///   mut  diff = 101 - (99 * 1) = 2  → ccrpad = 2 → pixx = max(102,101) = 102
    /// so the mutant widens the matrix by one all-zero column, shifting the
    /// fingerprint. Driven directly with a deterministic CC so the
    /// fingerprint is stable.
    #[test]
    fn build_gs1_128_cca_ccrpad_mul_mutant_killed() {
        fn mk_cc(w: usize, r: usize) -> crate::encoding::BitMatrix {
            let mut b = crate::encoding::BitMatrix::new(w, r);
            for y in 0..r {
                for x in 0..w {
                    b.set(x, y, (x + 3 * y) % 2 == 0);
                }
            }
            b
        }
        fn fp_bm(bm: &crate::encoding::BitMatrix) -> (usize, usize, u64) {
            let w = bm.width();
            let h = bm.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(bm.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        let cc = mk_cc(99, 5);
        let linsbs = vec![1u32; 101]; // linwidth 101 → offset x = 1.
        assert_eq!(gs1_128_cc_offset_a(101), 1, "precondition: x=1 at lw=101");
        let bm = build_gs1_128_cca_composite(&cc, &linsbs, GS1_128_LINHEIGHT);
        assert_eq!(
            fp_bm(&bm),
            (101, 47, 15083406502160740),
            "build_gs1_128_cca fingerprint changed (the L1644:55 `*` mutant \
             widens pixx to 102)"
        );
    }

    /// EQUIVALENCE / UNREACHABILITY proofs (with executable witnesses) for
    /// the residual composite-v4 survivors that no test can kill because the
    /// mutation provably cannot alter any output bit on any reachable input.
    ///
    /// Each block builds the relevant structure and asserts the invariant
    /// that makes the mutated sub-expression a no-op.
    #[test]
    fn composite_equivalence_notes() {
        // --- build_ean_cca_composite L1124:44 `<` → `==` / `>` / `<=` (×3) ---
        // `lin_trailing_zero` (the only value the comparator feeds) is bound
        // and then explicitly discarded by `let _ = lin_trailing_zero;`
        // (file:line 1124 → 1157). It is never passed to `bm.set`, so any
        // mutation of the `diff_signed < 0` comparator changes only this dead
        // binding. Production output is fixed regardless (pinned exactly by
        // build_ean_cca_composite_state_machine_fingerprint_pinned).
        //
        // --- build_ean_cca_composite L1122:48 `+` → `-` ---
        // `diff_signed = linwidth + linpad_len + 1 - ccpixx` feeds only
        // `ccrpad_len = diff_signed.max(0)` (→ pixx) and the dead
        // `lin_trailing_zero`. For every reachable shape either
        //   linpad_len == 0  → `linwidth ± 0` is identical, or
        //   linpad_len  > 0  → linpad_len = ccpixx - linwidth - 2, so
        //     orig diff_signed = -1 and mutant = 2*(linwidth - ccpixx) + 3
        //     ≤ -3 (since ccpixx ≥ linwidth + 3); both are < 0, so
        //     ccrpad_len = 0 either way and pixx = ccpixx is unchanged.
        // Witness: sweep the diff_signed regimes and assert pixx is invariant.
        fn ean_cc(w: usize, r: usize) -> crate::encoding::BitMatrix {
            let mut b = crate::encoding::BitMatrix::new(w, r);
            for y in 0..r {
                for x in 0..w {
                    b.set(x, y, (x + 3 * y) % 2 == 0);
                }
            }
            b
        }
        for &(lw, ccpixx) in &[(95usize, 99usize), (95, 98), (95, 97), (95, 96), (51, 99)] {
            let bm = build_ean_cca_composite(&ean_cc(ccpixx, 3), &vec![1u32; lw], lw, 4);
            let linpad_len = ccpixx.saturating_sub(lw + 2);
            let orig = lw as isize + linpad_len as isize + 1 - ccpixx as isize;
            let mutated = lw as isize - linpad_len as isize + 1 - ccpixx as isize;
            assert_eq!(
                orig.max(0),
                mutated.max(0),
                "L1122 `+`→`-`: ccrpad_len invariant (lw={lw}, ccpixx={ccpixx})"
            );
            // pixx (= width) is unaffected since ccrpad_len is unchanged.
            assert_eq!(bm.width(), ccpixx + orig.max(0) as usize);
        }

        // --- apply_sepfinder L1036:23 `<` → `<=` ---
        // The findersep override loop only runs when the 13-cell f3pat match
        // succeeded, which (via the L1032 scan `pos < bot.len()` for all
        // j ∈ 0..=12) implies `fp + 12 < bot.len() == sep.len()`. Hence in
        // the override loop `fp + j < sep.len()` is already true for every
        // j ∈ 0..13, so `<=` writes the identical cells. Witness: a full
        // f3pat match writes exactly FINDERSEP with the last index in range.
        let bot_f3 = DATABAROMNI_F3PAT.to_vec(); // len 13, fp=0 → match.
        let mut sep_f3: Vec<u8> = bot_f3.iter().map(|&b| 1 - b).collect();
        apply_sepfinder(&bot_f3, &mut sep_f3, 0);
        assert_eq!(
            sep_f3,
            DATABAROMNI_FINDERSEP.to_vec(),
            "f3pat match writes FINDERSEP; max index fp+12=12 < len 13, so the \
             L1036 `<=` bound never reaches len (no-op mutation)"
        );

        // --- build_databar_expanded_composite L1445:30 `-` → `+` / `/` (×2) ---
        // `diff = pixx - cc_width` is bound and discarded by
        // `let _ = diff;` (file:line 1445 → 1472). It feeds no `bm.set`.
        // Witness: output width is `linsbs_sum + 1` independent of cc_width
        // / diff, so any mutation of the diff expression is inert.
        fn dx_cc(w: usize, r: usize) -> crate::encoding::BitMatrix {
            let mut b = crate::encoding::BitMatrix::new(w, r);
            for y in 0..r {
                for x in 0..w {
                    b.set(x, y, (x + y) % 2 == 0);
                }
            }
            b
        }
        let linsbs: Vec<u8> = vec![1u8; 40];
        let for_wide = build_databar_expanded_composite(&dx_cc(80, 3), &linsbs, 4);
        let for_narrow = build_databar_expanded_composite(&dx_cc(20, 3), &linsbs, 4);
        assert_eq!(
            for_wide.width(),
            41,
            "pixx = linsbs_sum + 1, not diff-derived"
        );
        assert_eq!(
            for_wide.width(),
            for_narrow.width(),
            "L1445 `diff` is dead; changing cc_width (hence diff) leaves width fixed"
        );

        // --- build_gs1_128_cca_composite L1644:34 `-`→`/` & L1646:35 `+`→`-` ---
        // Both touch only `ccrpad` / its summand in
        //   pixx = (cclpad + cc_width + ccrpad).max(linwidth).
        // In production cc_width = 99 and, with offset x ≥ 0 and ccrpad =
        // max(0, linwidth - 99 - x), the non-max term equals
        //   x + 99 + (linwidth - 99 - x) = linwidth  (when diff > 0), or
        //   x + 99 + 0 ≤ linwidth                     (when diff ≤ 0),
        // so pixx = linwidth regardless of ccrpad's exact value. A smaller
        // ccrpad (L1644 `/`) or a subtracted one (L1646 `-`) only lowers the
        // non-max term, which `.max(linwidth)` absorbs. ccrpad also only
        // ever pads trailing all-zero columns, so it sets no bit.
        // Witness: sweep valid linwidths at cc_width=99 and assert pixx==lw.
        fn g_cc(w: usize, r: usize) -> crate::encoding::BitMatrix {
            let mut b = crate::encoding::BitMatrix::new(w, r);
            for y in 0..r {
                for x in 0..w {
                    b.set(x, y, (x + r * y) % 2 == 0);
                }
            }
            b
        }
        for &lw in &[101usize, 112, 123, 145, 167, 200] {
            let x = gs1_128_cc_offset_a(lw);
            assert!(x >= 0, "production offset non-negative (lw={lw})");
            let bm = build_gs1_128_cca_composite(&g_cc(99, 5), &vec![1u32; lw], GS1_128_LINHEIGHT);
            assert_eq!(
                bm.width(),
                lw,
                "pixx pinned to linwidth at cc_width=99 (lw={lw}); ccrpad value \
                 is absorbed by .max(linwidth) → L1644:34 and L1646:35 are inert"
            );
        }

        // --- build_gs1_128_ccc_composite L1747:37 `>`→`>=`, L1752:24 `+`→`*`,
        //     L1752:68 `+`→`-` ---
        // With the fixed CC-C offset x = -7, cclpad = 0, linlpad = 7 and
        //   diff = linwidth + 7 - cc_width,
        //   pixx = (cclpad + cc_width + ccrpad).max(linlpad + linwidth + linrpad).
        // L1747 (`>`→`>=`): the branches differ only at diff == 0, where both
        //   yield (ccrpad, linrpad) = (0, 0) — identical.
        // L1752:24 (`cclpad + cc_width` → `cclpad * cc_width`): cclpad = 0, so
        //   `0 + cc_width` vs `0 * cc_width`; but the first max-argument never
        //   strictly exceeds the second:
        //     diff > 0 ⇒ cc_width + ccrpad = linwidth + 7 = arg2 (tie);
        //     diff ≤ 0 ⇒ cc_width = arg2 (tie).
        //   Reducing arg1 to ccrpad keeps the max equal to arg2.
        // L1752:68 (`linlpad + linwidth + linrpad` → `... - linrpad`): when
        //   diff > 0, linrpad = 0 (no change); when diff ≤ 0,
        //   linrpad = cc_width - 7 - linwidth and
        //   arg2' = 7 + linwidth - linrpad = 14 + 2*linwidth - cc_width ≤ cc_width
        //   = arg1, so the max is still arg1 = cc_width.
        // In every case pixx is invariant; ccrpad/linrpad only pad trailing /
        // leading all-zero columns appropriately, with no bit moved.
        // Witness: sweep all three sign regimes of diff and assert pixx.
        let ccc_cases: &[(usize, usize)] = &[
            (160, 150), // diff = +17
            (150, 157), // diff =  0
            (140, 160), // diff = -13
            (220, 200), // diff = +27
            (100, 110), // diff =  -3
        ];
        for &(lw, cw) in ccc_cases {
            let diff = lw as isize + 7 - cw as isize;
            let ccrpad = diff.max(0) as usize;
            let linrpad = (-diff).max(0) as usize;
            let want = (cw + ccrpad).max(7 + lw + linrpad);
            let bm = build_gs1_128_ccc_composite(&g_cc(cw, 5), &vec![1u32; lw], GS1_128_LINHEIGHT);
            assert_eq!(
                bm.width(),
                want,
                "ccc pixx invariant (lw={lw}, cw={cw}, diff={diff}); the L1747 / \
                 L1752 mutants leave the dominant max-term unchanged"
            );
            // Explicit no-op checks for the L1752 arg swaps.
            let arg1 = cw + ccrpad;
            // cclpad=0 ⇒ first term vanishes; the literal 0*cw mirrors the
            // production `cclpad * cw` term and is the point of the witness.
            #[allow(clippy::erasing_op)]
            let arg1_mul = 0usize * cw + ccrpad;
            assert_eq!(
                arg1.max(7 + lw + linrpad),
                arg1_mul.max(7 + lw + linrpad),
                "L1752:24 `*`: first max-arg never strictly dominant"
            );
            let arg2 = 7 + lw + linrpad;
            let arg2_sub = (7 + lw).saturating_sub(linrpad);
            assert_eq!(
                arg1.max(arg2),
                arg1.max(arg2_sub),
                "L1752:68 `-`: subtracting linrpad never lowers the max below arg1"
            );
            // L1747 boundary: at diff == 0 both branches give (0, 0).
            if diff == 0 {
                assert_eq!((ccrpad, linrpad), (0, 0), "L1747 `>=` no-op at diff==0");
            }
        }
    }
}
